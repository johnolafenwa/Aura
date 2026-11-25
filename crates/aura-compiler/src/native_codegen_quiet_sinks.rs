//! Finds the `mut` receivers and parameters a function never changes, so a
//! call need not hand it a write-through sink for them.
//!
//! A caller whose `mut` argument lies in a place a cleanup reads, such as a
//! `with` binding, makes a write-through sink for it: a copy of the value
//! that the callee's changes update, and that a trap's cleanup then sees.
//! Making the sink copies the value, and each update copies it again.
//! A callee that never stores to the parameter, writes it back from a call,
//! moves it, lends a part of it, or lets a closure or task reach it, publishes
//! nothing through the sink, and the value the caller's cleanup holds stays
//! the parameter's value throughout the call. Such a call hands over no
//! sink, and enters the function's sink-free body when it has one.

use std::collections::{HashMap, HashSet};

use crate::mir::ownership::{instruction_accesses, terminator_roles, Access};
use crate::mir::{
    instruction_place_mentions, CallTarget, Instruction, MethodDispatch, MirFunction, MirModule,
    MirReceiverKind, Operand, Rvalue,
};
use crate::runtime_ops::RuntimeOp;

fn root(place: &str) -> &str {
    place.split('.').next().unwrap_or(place)
}

/// The call slots, receiver first, of the `mut` receiver and parameters of
/// each function of `module` that the function never changes. A writeback
/// from a call of a function by name, or of a method that dispatches
/// statically, to a slot that function never changes leaves the place as
/// it was.
pub(super) fn quiet_sink_slots(module: &MirModule) -> HashMap<String, HashSet<usize>> {
    let facts = module
        .functions
        .iter()
        .filter_map(|function| Some((function.name.as_str(), local_facts(function)?)))
        .collect::<HashMap<_, _>>();
    let mut quiet = facts
        .iter()
        .map(|(name, facts)| {
            let slots = facts
                .slots
                .iter()
                .filter(|(param, _)| !facts.changed.contains(*param))
                .map(|(param, slot)| (*param, *slot))
                .collect::<HashMap<_, _>>();
            (*name, slots)
        })
        .collect::<HashMap<_, _>>();
    // A writeback from a call that may change its slot changes the place.
    let mut shrank = true;
    while shrank {
        shrank = false;
        for (name, facts) in &facts {
            for (param, callee, slot) in &facts.writebacks {
                let kept = quiet
                    .get(callee)
                    .is_some_and(|slots| slots.values().any(|quiet| quiet == slot));
                if !kept {
                    shrank |= quiet
                        .get_mut(name)
                        .and_then(|slots| slots.remove(param))
                        .is_some();
                }
            }
        }
    }
    quiet
        .into_iter()
        .filter(|(_, slots)| !slots.is_empty())
        .map(|(name, slots)| (name.to_string(), slots.into_values().collect()))
        .collect()
}

/// What one function does to its `mut` receiver and parameters.
struct LocalFacts<'f> {
    /// Each `mut` receiver or parameter, with its call slot.
    slots: HashMap<&'f str, usize>,
    /// The ones the function changes itself.
    changed: HashSet<&'f str>,
    /// The writebacks of calls to them: the parameter, the function the
    /// call runs, and the slot it writes back.
    writebacks: Vec<(&'f str, &'f str, usize)>,
}

/// The facts of `function`, or `None` when it has no `mut` receiver or
/// parameter, or a mutable loan that may write any of them.
fn local_facts(function: &MirFunction) -> Option<LocalFacts<'_>> {
    let mut slots = HashMap::<&str, usize>::new();
    let mut slot = 0usize;
    if function.receiver.is_some() {
        if function.receiver == Some(MirReceiverKind::BorrowMut) {
            slots.insert("self", slot);
        }
        slot += 1;
    }
    for param in &function.params {
        if param.passing == MirReceiverKind::BorrowMut {
            slots.insert(param.name.as_str(), slot);
        }
        slot += 1;
    }
    if slots.is_empty() {
        return None;
    }
    let mut changed = HashSet::<&str>::new();
    let mut writebacks = Vec::new();
    for block in &function.blocks {
        for instruction in &block.instructions {
            match instruction {
                // A mutable loan may write whatever it reaches, through a
                // loan of a loan too, so it changes every parameter.
                Instruction::BeginLoan { mutable: true, .. }
                | Instruction::BeginElementLoan { mutable: true, .. }
                | Instruction::Reborrow { mutable: true, .. }
                | Instruction::ReborrowElement { mutable: true, .. }
                | Instruction::BeginReturnedLoan { mutable: true, .. }
                | Instruction::WriteLoan { .. } => return None,
                // A lend empties the place while its call runs.
                Instruction::Lend { place, .. } | Instruction::EndLend { place, .. } => {
                    changed.insert(root(place));
                }
                Instruction::Assign {
                    value: Rvalue::Closure { .. } | Rvalue::StartTask { .. },
                    ..
                } => {
                    let mut mentions = Vec::new();
                    instruction_place_mentions(instruction, &mut mentions);
                    changed.extend(mentions.into_iter().map(root));
                }
                _ => {}
            }
            let called = called_writebacks(instruction);
            let handle = handle_receiver(instruction);
            for (place, access) in instruction_accesses(instruction) {
                if access == Access::Writeback && handle == Some(place) {
                    continue;
                }
                if access == Access::Writeback {
                    if let Some((callee, slot)) = called.get(place) {
                        if slots.contains_key(root(place)) {
                            writebacks.push((root(place), *callee, *slot));
                        }
                        continue;
                    }
                }
                if matches!(
                    access,
                    Access::Assign | Access::Writeback | Access::Move | Access::TakePart
                ) {
                    changed.insert(root(place));
                }
            }
        }
        for (operand, _) in terminator_roles(&block.terminator) {
            if let Operand::MovePlace(place) = operand {
                changed.insert(root(place));
            }
        }
    }
    Some(LocalFacts {
        slots,
        changed,
        writebacks,
    })
}

/// The receiver place of a method of a resource handle, `random.Rng`,
/// `str`, `Queue`, `Task` or `TaskGroup`. Native code changes the handle's
/// state where the handle points and stores nothing back to the place.
fn handle_receiver(instruction: &Instruction) -> Option<&str> {
    match instruction {
        Instruction::Assign {
            value:
                Rvalue::Call {
                    callee:
                        CallTarget::Runtime {
                            op:
                                RuntimeOp::Resource(_)
                                | RuntimeOp::Rng(_)
                                | RuntimeOp::Str(_)
                                | RuntimeOp::Queue(_)
                                | RuntimeOp::Task(_)
                                | RuntimeOp::TaskGroup(_),
                            receiver: Some(_),
                            receiver_place: Some(place),
                        },
                    ..
                },
            ..
        } => Some(place),
        _ => None,
    }
}

/// The places a call of a function by name, or of a method that dispatches
/// statically, writes back, each with the function and the call slot. The
/// places of one call never overlap, since each is a mutable borrow.
fn called_writebacks(instruction: &Instruction) -> HashMap<&str, (&str, usize)> {
    let Instruction::Assign {
        value: Rvalue::Call { callee, args, .. },
        ..
    } = instruction
    else {
        return HashMap::new();
    };
    let (function, receiver, first_arg) = match callee {
        CallTarget::Name(function) => (function.as_str(), None, 0),
        CallTarget::Member {
            receiver_place,
            dispatch: Some(MethodDispatch::Static { function }),
            ..
        } => (function.as_str(), receiver_place.as_deref(), 1),
        _ => return HashMap::new(),
    };
    receiver
        .map(|place| (place, 0))
        .into_iter()
        .chain(
            args.iter().enumerate().filter_map(|(index, arg)| {
                Some((arg.writeback_place.as_deref()?, index + first_arg))
            }),
        )
        .map(|(place, slot)| (place, (function, slot)))
        .collect()
}

impl super::FunctionCompiler<'_> {
    /// Leaves out of `places`, one per call slot, the sinks of the slots of
    /// `callee` that it never changes, and returns those places: their
    /// writebacks bring back the value the place holds, so storing them
    /// publishes nothing new.
    pub(super) fn drop_quiet_sinks(
        &self,
        callee: &str,
        places: &mut [Option<String>],
    ) -> Vec<String> {
        let Some(quiet) = self.quiet_sinks.get(callee) else {
            return Vec::new();
        };
        let mut dropped = Vec::new();
        for slot in quiet {
            if let Some(place) = places.get_mut(*slot).and_then(Option::take) {
                dropped.push(place);
            }
        }
        // A place another slot writes back too may change.
        dropped.retain(|place| !places.iter().flatten().any(|other| other == place));
        dropped
    }
}
