//! What a function may write of its `mut` parameters.
//!
//! A call replaces each `mut` argument with a value the callee's `ensures`
//! describes. A part of the argument the callee cannot write keeps the
//! value it had, so the caller needs no `ensures` to say so. This module
//! reads which parts those are from the MIR bodies: for each function and
//! each `mut` parameter, `self` included, the field paths the body may
//! write, or every part when a write is not one the walk can place.
//!
//! The walk is syntactic and conservative. It counts assignments, writes
//! through a loan as writes of the whole place the loan borrows, runtime
//! operations that change their receiver, cleanups, and the `mut`
//! arguments of every call. A call of a known function writes what that
//! function writes, found as the least fixed point over the call graph.
//! Any other call, such as one through a function value or a trait, writes
//! each place it could. A parameter a closure captures by reference is
//! written everywhere.
//!
//! Two `mut` parameters never share an object: a mutable borrow is
//! exclusive, and objects are owned values, not shared references. So a
//! write through one parameter leaves every other argument as it was.

use std::collections::{BTreeMap, BTreeSet};

use super::exec::{closure_captures, root_of, rvalue_writes, union_payload_index};
use crate::mir::{
    CallTarget, Instruction, MethodDispatch, MirArg, MirFunction, MirReceiverKind, Rvalue,
    Terminator,
};

/// How many fields deep a written path is kept apart. A deeper write
/// counts as a write of the path's first fields, which bounds the paths a
/// recursive function collects.
const MAX_PATH: usize = 4;

/// The parts of one `mut` parameter a function may write: each field path
/// in the set, or with `None` any part.
pub(crate) type Written = Option<BTreeSet<String>>;

/// For each function, what it may write of each `mut` parameter.
pub(crate) type Frames<'a> = BTreeMap<&'a str, BTreeMap<String, Written>>;

/// One thing a body does that may write a place.
enum Effect<'a> {
    /// A write of the place.
    Write(String),
    /// A call that passes `place` as the `mut` parameter `param` of
    /// `callee`, and writes what the callee writes of it.
    Call {
        callee: &'a str,
        param: &'a str,
        place: &'a str,
    },
}

/// What the walk reads from one function.
struct Body<'a> {
    name: &'a str,
    /// The `mut` parameters, `self` included.
    params: Vec<&'a str>,
    effects: Vec<Effect<'a>>,
    /// The place each loan borrows from.
    loans: BTreeMap<&'a str, &'a str>,
}

impl<'a> Body<'a> {
    fn of(function: &'a MirFunction, functions: &BTreeMap<&'a str, &'a MirFunction>) -> Self {
        let mut params: Vec<&str> = function
            .params
            .iter()
            .filter(|param| param.passing == MirReceiverKind::BorrowMut)
            .map(|param| param.name.as_str())
            .collect();
        if function.receiver == Some(MirReceiverKind::BorrowMut) {
            params.push("self");
        }
        let mut effects = Vec::new();
        let mut loans = BTreeMap::new();
        // A function with no `mut` parameter has nothing to report.
        let blocks = if params.is_empty() {
            &[][..]
        } else {
            &function.blocks[..]
        };
        for block in blocks {
            for instruction in &block.instructions {
                match instruction {
                    Instruction::BeginLoan { loan, source, .. }
                    | Instruction::BeginElementLoan { loan, source, .. } => {
                        loans.insert(loan.as_str(), source.as_str());
                    }
                    Instruction::BeginReturnedLoan { loan, origin, .. } => {
                        loans.insert(loan.as_str(), origin.as_str());
                    }
                    Instruction::Reborrow { loan, parent, .. }
                    | Instruction::ReborrowElement { loan, parent, .. } => {
                        loans.insert(loan.as_str(), parent.as_str());
                    }
                    Instruction::Assign { target, value } => {
                        effects.push(Effect::Write(target.clone()));
                        value_effects(value, functions, &mut effects);
                    }
                    Instruction::WriteLoan { loan, value } => {
                        effects.push(Effect::Write(loan.clone()));
                        value_effects(value, functions, &mut effects);
                    }
                    Instruction::PopCleanup { place, .. } => {
                        effects.push(Effect::Write(place.clone()));
                    }
                    _ => {}
                }
            }
            if let Terminator::ForRange { binding, .. } = &block.terminator {
                effects.push(Effect::Write(binding.clone()));
            }
        }
        // Any call may run a closure, which writes what it captured.
        effects.extend(closure_captures(function).into_iter().map(Effect::Write));
        Self {
            name: &function.name,
            params,
            effects,
            loans,
        }
    }

    /// Adds a write of `place`, or of `path` inside it, to `frame`. A
    /// write through a loan is a write of the place the loan borrows.
    fn record(&self, place: &str, path: &str, frame: &mut BTreeMap<String, Written>) {
        let (mut place, mut path) = (place, path);
        for _ in 0..=self.loans.len() {
            let root = root_of(place);
            if let Some(written) = frame.get_mut(root) {
                let fields = field_path(place[root.len()..].split('.').chain(path.split('.')));
                match (written.as_mut(), fields) {
                    (Some(paths), Some(fields)) => {
                        paths.insert(fields);
                    }
                    _ => *written = None,
                }
                return;
            }
            let Some(source) = self.loans.get(root) else {
                return;
            };
            (place, path) = (source, "");
        }
    }
}

/// The leading field and tuple segments of a path, at most `MAX_PATH` of
/// them, or `None` when there is none: the write is of the whole value. A
/// union payload ends the path, since a write of one may come with a new
/// tag.
fn field_path<'p>(segments: impl Iterator<Item = &'p str>) -> Option<String> {
    let fields: Vec<&str> = segments
        .filter(|segment| !segment.is_empty())
        .take_while(|segment| union_payload_index(segment).is_none())
        .take(MAX_PATH)
        .collect();
    (!fields.is_empty()).then(|| fields.join("."))
}

/// The effects of evaluating `value`.
fn value_effects<'a>(
    value: &'a Rvalue,
    functions: &BTreeMap<&'a str, &'a MirFunction>,
    effects: &mut Vec<Effect<'a>>,
) {
    let known = match value {
        Rvalue::Call {
            callee: CallTarget::Name(name),
            args,
            ..
        } => functions
            .get(name.as_str())
            .map(|callee| (*callee, None, args)),
        Rvalue::Call {
            callee:
                CallTarget::Member {
                    receiver_place,
                    dispatch: Some(MethodDispatch::Static { function }),
                    ..
                }
                | CallTarget::TraitMember {
                    receiver_place,
                    dispatch: Some(MethodDispatch::Static { function }),
                    ..
                },
            args,
            ..
        } => functions
            .get(function.as_str())
            .map(|callee| (*callee, receiver_place.as_deref(), args)),
        _ => None,
    };
    match known {
        Some((callee, receiver, args)) => call_effects(callee, receiver, args, effects),
        None => rvalue_writes(value, &mut |place| {
            effects.push(Effect::Write(place.to_string()));
        }),
    }
}

/// The effects of a call of the known function `callee`: each `mut`
/// argument is written as the callee writes its parameter.
fn call_effects<'a>(
    callee: &'a MirFunction,
    receiver: Option<&'a str>,
    args: &'a [MirArg],
    effects: &mut Vec<Effect<'a>>,
) {
    let name = callee.name.as_str();
    if let Some(place) = receiver.filter(|_| callee.receiver != Some(MirReceiverKind::Borrow)) {
        let mutable = callee.receiver == Some(MirReceiverKind::BorrowMut);
        effects.push(if mutable {
            Effect::Call {
                callee: name,
                param: "self",
                place,
            }
        } else {
            Effect::Write(place.to_string())
        });
    }
    let mut positional = 0;
    for arg in args {
        let param = match &arg.name {
            Some(name) => callee.params.iter().find(|param| &param.name == name),
            None => {
                positional += 1;
                callee.params.get(positional - 1)
            }
        };
        let Some(place) = &arg.writeback_place else {
            continue;
        };
        let mutable = param.filter(|param| param.passing == MirReceiverKind::BorrowMut);
        effects.push(mutable.map_or_else(
            || Effect::Write(place.clone()),
            |param| Effect::Call {
                callee: name,
                param: &param.name,
                place,
            },
        ));
    }
}

/// What each of `functions` may write of its `mut` parameters.
pub(crate) fn frames<'a>(functions: &BTreeMap<&'a str, &'a MirFunction>) -> Frames<'a> {
    let bodies: Vec<Body<'a>> = functions
        .values()
        .map(|function| Body::of(function, functions))
        .collect();
    // Each round adds what the callees were found to write in the round
    // before. Paths have a bounded length, so the rounds end.
    let mut frames: Frames<'a> = bodies
        .iter()
        .map(|body| {
            let empty = |param: &&str| (param.to_string(), Some(BTreeSet::new()));
            (body.name, body.params.iter().map(empty).collect())
        })
        .collect();
    loop {
        let mut changed = false;
        for body in &bodies {
            let mut frame = frames[body.name].clone();
            for effect in &body.effects {
                match effect {
                    Effect::Write(place) => body.record(place, "", &mut frame),
                    Effect::Call {
                        callee,
                        param,
                        place,
                    } => match &frames[callee][*param] {
                        Some(paths) => {
                            for path in paths {
                                body.record(place, path, &mut frame);
                            }
                        }
                        None => body.record(place, "", &mut frame),
                    },
                }
            }
            if frame != frames[body.name] {
                frames.insert(body.name, frame);
                changed = true;
            }
        }
        if !changed {
            return frames;
        }
    }
}
