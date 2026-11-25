//! `str` values that native code keeps as words, stage 1: the `str`
//! parameters a function only reads.
//!
//! The words of a `str` are `{ptr, len, owner}`. `ptr` and `len` name its
//! UTF-8 bytes, which stay valid for as long as the words are used. `owner`
//! is 0 while the bytes are borrowed (a literal of the program, or an
//! element of a `list[str]` under a loan), and otherwise the box of the
//! `str`, which every runtime operation that takes a box still takes.
//!
//! A function that reads a `str` parameter through its bytes (see
//! [`ViewRead`]) gets a second body, its **view twin**, that takes the
//! three words of each such parameter: the owner in the parameter's own
//! word, and `ptr` and `len` after the function's other words. A call
//! whose argument for each such parameter is a literal, a loaned element
//! of a `list[str]`, or a parameter the calling twin holds as words,
//! enters the twin and makes no box for the argument. Every other caller,
//! the runtime included, enters the function's own symbol.
//!
//! Inside the twin a read that knows how to use the bytes uses them. Any
//! other use loads the parameter's box as before; when the caller passed
//! no owner, the first such load makes the box from the bytes, and the
//! twin releases the box it made when it returns. The box is one the task's
//! ledger knows, so a trap frees it.

use std::collections::{HashMap, HashSet};

use crate::mir::{
    instruction_place_mentions, terminator_place_mentions, CallTarget, Instruction, MethodDispatch,
    MirFormatPart, MirFunction, MirModule, MirReceiverKind, Operand, Rvalue,
};

use super::*;

/// How generated code reads the `str` receiver of a runtime operation from
/// its bytes.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum ViewRead {
    /// The entry point takes `(ptr, len)` and returns a new `str` box.
    NewBox(RuntimeFn),
    /// The result is `len`.
    ByteLen,
}

/// The way to run `op` on a `str` receiver held as words, when there is
/// one. A runtime entry point that reads `(ptr, len)` is one row here.
pub(super) fn receiver_view_read(op: RuntimeOp) -> Option<ViewRead> {
    match op {
        RuntimeOp::Value(ValueOp::Clone) => Some(ViewRead::NewBox(RuntimeFn::StringFromView)),
        RuntimeOp::Str(StrOp::ByteLen) => Some(ViewRead::ByteLen),
        _ => None,
    }
}

/// One parameter a view twin takes as words.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) struct ViewParam {
    /// Its position among the function's parameters.
    pub(super) position: usize,
    /// Whether the twin never loads its box, so a caller need not pass one
    /// it would have to make or look up.
    pub(super) pure: bool,
}

/// What a function does with one of its `str` parameters.
#[derive(Default)]
struct ParamUse {
    /// It is read through its bytes.
    reads: bool,
    /// It is used in a way that takes its box.
    boxes: bool,
    /// The function's first block takes its box, so every call does.
    boxes_on_entry: bool,
    /// The parameters of other functions it is passed to.
    forwards: Vec<(String, usize)>,
}

fn is_str(ty: &Type) -> bool {
    *ty == Type::named("str")
}

/// The positions of the parameters of `function` that could be views: a
/// borrowed `str` is only read. A method's receiver is not among them.
fn borrowed_strs(function: &MirFunction) -> HashMap<&str, usize> {
    function
        .params
        .iter()
        .enumerate()
        .filter(|(_, param)| param.passing == MirReceiverKind::Borrow && is_str(&param.ty))
        .map(|(position, param)| (param.name.as_str(), position))
        .collect()
}

/// The use `function` makes of each of `candidates`. `positions` gives the
/// candidate positions of a function of the program.
fn param_uses(
    function: &MirFunction,
    candidates: &HashMap<&str, usize>,
    positions: &HashMap<&str, HashMap<&str, usize>>,
) -> HashMap<usize, ParamUse> {
    let mut uses = candidates
        .values()
        .map(|position| (*position, ParamUse::default()))
        .collect::<HashMap<_, _>>();
    let candidate = |operand: &Operand| match operand {
        Operand::Place(place) => candidates.get(place.as_str()).copied(),
        _ => None,
    };
    let dicts = function
        .local_types
        .iter()
        .filter(|local| {
            matches!(&local.ty, Type::Named(name, args)
                if name == "dict" && args.first().is_some_and(is_str))
        })
        .map(|local| local.name.as_str())
        .collect::<HashSet<_>>();
    // A loaned element, and an element read that may be held as words
    // (see `element_slots`), compare from their bytes.
    let mut loans = read_str_loans(function);
    loans.extend(str_element_reads(function));
    let views = |operand: &Operand| {
        matches!(operand, Operand::String(_))
            || matches!(operand, Operand::Place(place)
                if candidates.contains_key(place.as_str()) || loans.contains(place.as_str()))
    };
    for block in &function.blocks {
        // The terminator, last, takes what it names as a box.
        for instruction in block.instructions.iter().map(Some).chain([None]) {
            let mut mentions = Vec::new();
            match instruction {
                Some(instruction) => instruction_place_mentions(instruction, &mut mentions),
                None => mentions.extend(terminator_place_mentions(&block.terminator)),
            }
            let mentioned = mentions
                .into_iter()
                .filter_map(|place| candidates.get(place.split('.').next().unwrap_or(place)))
                .copied()
                .collect::<HashSet<_>>();
            if mentioned.is_empty() {
                continue;
            }
            let mut forwards = Vec::new();
            let reads = instruction.is_some_and(|instruction| match instruction {
                Instruction::Assign {
                    value:
                        Rvalue::Binary {
                            op: BinaryOp::Eq | BinaryOp::NotEq,
                            left,
                            right,
                            ..
                        },
                    ..
                } => views(left) && views(right),
                Instruction::Assign {
                    value:
                        Rvalue::Call {
                            callee:
                                CallTarget::Runtime {
                                    op,
                                    receiver: Some(receiver),
                                    ..
                                },
                            args,
                            ..
                        },
                    ..
                } => match (op, args.as_slice()) {
                    (
                        RuntimeOp::Dict(DictOp::Get | DictOp::ContainsKey)
                        | RuntimeOp::Set(SetOp::Contains),
                        [key],
                    ) => candidate(receiver).is_none() && candidate(&key.value).is_some(),
                    (op, []) => receiver_view_read(*op).is_some(),
                    _ => false,
                },
                // The bytes of a `str` are copied from its words.
                Instruction::Assign { value, .. } if str_to_bytes(value).is_some() => true,
                // The entry loan of a dict under the key a fused test
                // probed reads the probe's slot.
                Instruction::BeginElementLoan {
                    source, selector, ..
                } => candidate(selector).is_some() && dicts.contains(source.as_str()),
                // An f-string part is pushed from its bytes.
                Instruction::Assign {
                    value: Rvalue::FormatString { parts },
                    ..
                } => parts.iter().all(|part| match part {
                    MirFormatPart::Value(_) | MirFormatPart::Literal(_) => true,
                    part => part.operand().and_then(candidate).is_none(),
                }),
                Instruction::Assign { value, .. } if direct_callee(value).is_some() => {
                    let (callee, args) = direct_callee(value).expect("checked above");
                    let callee_positions = positions.get(callee);
                    // A receiver is passed as a box.
                    let receiver = match value {
                        Rvalue::Call {
                            callee: CallTarget::Member { object, .. },
                            ..
                        } => candidate(object).is_none(),
                        _ => true,
                    };
                    receiver
                        && args.iter().enumerate().all(|(index, argument)| {
                            let Some(position) = candidate(&argument.value) else {
                                return true;
                            };
                            let passed = argument.writeback_place.is_none()
                                && callee_positions.is_some_and(|positions| {
                                    positions.values().any(|at| *at == index)
                                });
                            if passed {
                                forwards.push((position, (callee.to_string(), index)));
                            }
                            passed
                        })
                }
                _ => false,
            });
            for position in mentioned {
                let entry = uses.entry(position).or_default();
                if !reads {
                    entry.boxes = true;
                    entry.boxes_on_entry |= block.label == function.entry;
                } else if forwards.iter().all(|(from, _)| *from != position) {
                    entry.reads = true;
                }
            }
            if reads {
                for (position, target) in forwards {
                    uses.entry(position).or_default().forwards.push(target);
                }
            }
        }
    }
    uses
}

/// The function a call runs by name, with its arguments: a call of a
/// function by name with no type arguments, or a method call that
/// dispatches statically, whose arguments leave out the receiver.
pub(super) fn direct_callee(value: &Rvalue) -> Option<(&str, &[MirArg])> {
    match value {
        Rvalue::Call {
            callee: CallTarget::Name(callee),
            args,
            type_args,
        } if type_args.is_empty() => Some((callee, args)),
        Rvalue::Call {
            callee:
                CallTarget::Member {
                    dispatch: Some(MethodDispatch::Static { function }),
                    ..
                },
            args,
            type_args,
        } if type_args.is_empty() => Some((function, args)),
        _ => None,
    }
}

/// The `str` locals of `function` that an element read of a list assigns.
fn str_element_reads(function: &MirFunction) -> HashSet<&str> {
    let strs = function
        .local_types
        .iter()
        .filter(|local| is_str(&local.ty))
        .map(|local| local.name.as_str())
        .collect::<HashSet<_>>();
    function
        .blocks
        .iter()
        .flat_map(|block| &block.instructions)
        .filter_map(|instruction| match instruction {
            Instruction::Assign { target, value }
                if strs.contains(target.as_str()) && element_read(value).is_some() =>
            {
                Some(target.as_str())
            }
            _ => None,
        })
        .collect()
}

/// The list and the index and span arguments of `value`, when it reads an
/// element of a list local.
fn element_read(value: &Rvalue) -> Option<(&str, &[MirArg; 3])> {
    match value {
        Rvalue::Call {
            callee:
                CallTarget::Runtime {
                    op: RuntimeOp::List(ListOp::Index | ListOp::IndexUnchecked),
                    receiver: Some(Operand::Place(list)),
                    ..
                },
            args,
            ..
        } => Some((list, <&[MirArg; 3]>::try_from(args.as_slice()).ok()?)),
        _ => None,
    }
}

/// The loans of `function` that select a `str` to read.
fn read_str_loans(function: &MirFunction) -> HashSet<&str> {
    let strs = function
        .local_types
        .iter()
        .filter(|local| is_str(&local.ty))
        .map(|local| local.name.as_str())
        .collect::<HashSet<_>>();
    function
        .blocks
        .iter()
        .flat_map(|block| &block.instructions)
        .filter_map(|instruction| match instruction {
            Instruction::BeginElementLoan {
                loan,
                mutable: false,
                ..
            } if strs.contains(loan.as_str()) => Some(loan.as_str()),
            _ => None,
        })
        .collect()
}

/// The functions of `module` that get a view twin, each with the
/// parameters the twin takes as words, in order.
pub(super) fn plan_views(module: &MirModule) -> HashMap<String, Vec<ViewParam>> {
    let positions = module
        .functions
        .iter()
        .map(|function| (function.name.as_str(), borrowed_strs(function)))
        .filter(|(_, positions)| !positions.is_empty())
        .collect::<HashMap<_, _>>();
    if positions.is_empty() {
        return HashMap::new();
    }
    let uses = module
        .functions
        .iter()
        .filter_map(|function| {
            let candidates = positions.get(function.name.as_str())?;
            Some((
                function.name.as_str(),
                param_uses(function, candidates, &positions),
            ))
        })
        .collect::<HashMap<_, _>>();
    // A parameter is worth its words when some read uses the bytes, here
    // or in a function it is passed on to, and some call does not need
    // its box from the start.
    let mut useful = HashSet::<(&str, usize)>::new();
    let mut grew = true;
    while grew {
        grew = false;
        for (name, uses) in &uses {
            for (position, param) in uses {
                let reads = !param.boxes_on_entry
                    && (param.reads
                        || param
                            .forwards
                            .iter()
                            .any(|(callee, at)| useful.contains(&(callee.as_str(), *at))));
                if reads {
                    grew |= useful.insert((name, *position));
                }
            }
        }
    }
    // It is pure when no path loads its box.
    let mut pure = useful
        .iter()
        .filter(|(name, position)| !uses[name][position].boxes)
        .copied()
        .collect::<HashSet<_>>();
    let mut shrank = true;
    while shrank {
        let boxed = pure
            .iter()
            .filter(|(name, position)| {
                uses[name][position]
                    .forwards
                    .iter()
                    .any(|(callee, at)| !pure.contains(&(callee.as_str(), *at)))
            })
            .copied()
            .collect::<Vec<_>>();
        shrank = !boxed.is_empty();
        for param in boxed {
            pure.remove(&param);
        }
    }
    // A twin is made for a function some call can enter with words.
    let useful_views = {
        let mut views = HashMap::<String, Vec<ViewParam>>::new();
        for (name, position) in &useful {
            views.entry(name.to_string()).or_default().push(ViewParam {
                position: *position,
                pure: false,
            });
        }
        views
    };
    let kept = crate::mir::ownership::kept_arguments(module);
    let explicit = crate::mir::ownership::explicit_locals(module);
    let mut entered = HashSet::<&str>::new();
    for caller in module.functions.iter().chain(module.top_level.iter()) {
        let loans = read_str_loans(caller);
        let own = positions.get(caller.name.as_str());
        let tracked = explicit.get(&caller.name).map(|locals| &locals.tracked);
        let elements = element_slots(
            caller,
            |place| tracked.is_some_and(|tracked| tracked.contains(place)),
            |instruction| {
                crate::mir::ownership::callee_borrowed_operands(instruction, |name| kept.get(name))
            },
            &useful_views,
        );
        for instruction in caller.blocks.iter().flat_map(|block| &block.instructions) {
            let Some((callee, args)) = (match instruction {
                Instruction::Assign { value, .. } => direct_callee(value),
                _ => None,
            }) else {
                continue;
            };
            let Some((callee, _)) = positions.get_key_value(callee) else {
                continue;
            };
            let callee = *callee;
            let mut params = args
                .iter()
                .enumerate()
                .filter(|(index, _)| useful.contains(&(callee, *index)))
                .map(|(_, argument)| &argument.value)
                .peekable();
            let words = params.peek().is_some()
                && params.all(|operand| {
                    matches!(operand, Operand::String(_))
                        || matches!(operand, Operand::Place(place)
                        if loans.contains(place.as_str())
                            || elements.lists.contains_key(place.as_str())
                            || own.is_some_and(|own| {
                                own.get(place.as_str()).is_some_and(|position| {
                                    useful.contains(&(caller.name.as_str(), *position))
                                })
                            }))
                });
            if words {
                entered.insert(callee);
            }
        }
    }
    let mut views = HashMap::new();
    for name in entered {
        let mut params = useful
            .iter()
            .filter(|(function, _)| *function == name)
            .map(|param| ViewParam {
                position: param.1,
                pure: pure.contains(param),
            })
            .collect::<Vec<_>>();
        params.sort_by_key(|param| param.position);
        views.insert(name.to_string(), params);
    }
    views
}

/// The local a `clone()` of a `str` place is assigned to, and that place,
/// when `instruction` is that.
fn copied_str(instruction: &Instruction) -> Option<(&str, &str)> {
    match instruction {
        Instruction::Assign {
            target,
            value:
                Rvalue::Call {
                    callee:
                        CallTarget::Runtime {
                            op: RuntimeOp::Value(ValueOp::Clone),
                            receiver: Some(Operand::Place(copied)),
                            ..
                        },
                    ..
                },
        } => Some((target.as_str(), copied.as_str())),
        _ => None,
    }
}

/// The local `instruction` appends to a list by moving it, when it is an
/// `append` of a moved local.
fn appended_local(instruction: &Instruction) -> Option<&str> {
    let Instruction::Assign {
        value:
            Rvalue::Call {
                callee:
                    CallTarget::Runtime {
                        op: RuntimeOp::List(ListOp::Push),
                        receiver: Some(Operand::Place(_)),
                        ..
                    },
                args,
                ..
            },
        ..
    } = instruction
    else {
        return None;
    };
    match args.as_slice() {
        [MirArg {
            value: Operand::MovePlace(place),
            writeback_place: None,
            ..
        }] => Some(place),
        _ => None,
    }
}

/// The `list[str]` locals of `function` that a literal of two or more `str`
/// literals assigns once and that the function only reads elements and
/// the length of, lends elements of to read, and releases. Such a local holds the program's one frozen copy of
/// the literal (see `aura_direct_str_literal_list_cell`), made on first use,
/// so a call builds no list and its release frees none.
pub(super) fn str_literal_lists(function: &MirFunction) -> HashSet<String> {
    let lists = function
        .local_types
        .iter()
        .filter(|local| {
            matches!(&local.ty, Type::Named(name, args)
                if name == "list" && args.len() == 1 && is_str(&args[0]))
                && !function.params.iter().any(|param| param.name == local.name)
        })
        .map(|local| local.name.as_str())
        .collect::<HashSet<_>>();
    if lists.is_empty() {
        return HashSet::new();
    }
    let root = |place: &str| place.split('.').next().unwrap_or(place).to_string();
    let mut literals = HashMap::<&str, usize>::new();
    let mut refused = HashSet::<String>::new();
    let mut mentions = Vec::new();
    for block in &function.blocks {
        for instruction in &block.instructions {
            match instruction {
                Instruction::Assign {
                    target,
                    value: Rvalue::VecLiteral { elements, .. },
                } if lists.contains(target.as_str())
                    && elements.len() >= 2
                    && elements.iter().any(
                        |element| matches!(element, Operand::String(text) if !text.is_empty()),
                    )
                    && elements
                        .iter()
                        .all(|element| matches!(element, Operand::String(_))) =>
                {
                    *literals.entry(target.as_str()).or_default() += 1;
                    continue;
                }
                Instruction::BeginElementLoan {
                    source,
                    mutable: false,
                    ..
                } if lists.contains(source.as_str()) => {
                    // The selector may not name the list itself.
                    mentions.clear();
                    instruction_place_mentions(instruction, &mut mentions);
                    if mentions
                        .iter()
                        .filter(|place| root(place) == *source)
                        .count()
                        > 1
                    {
                        refused.insert(source.clone());
                    }
                    continue;
                }
                Instruction::Release { place } if lists.contains(place.as_str()) => continue,
                // A read of an element or of the length only reads the list.
                Instruction::Assign {
                    value:
                        Rvalue::Call {
                            callee:
                                CallTarget::Runtime {
                                    op:
                                        RuntimeOp::List(
                                            ListOp::Index
                                            | ListOp::IndexUnchecked
                                            | ListOp::Len
                                            | ListOp::IsEmpty,
                                        ),
                                    receiver: Some(Operand::Place(list)),
                                    ..
                                },
                            args,
                            ..
                        },
                    ..
                } if lists.contains(list.as_str()) => {
                    // An argument may not name the list itself.
                    if args.iter().any(|arg| {
                        matches!(&arg.value, Operand::Place(place) | Operand::MovePlace(place)
                            if root(place) == *list)
                    }) {
                        refused.insert(list.clone());
                    }
                    continue;
                }
                _ => {}
            }
            mentions.clear();
            instruction_place_mentions(instruction, &mut mentions);
            refused.extend(mentions.iter().map(|place| root(place)));
        }
        refused.extend(
            terminator_place_mentions(&block.terminator)
                .into_iter()
                .map(root),
        );
    }
    literals
        .into_iter()
        .filter(|(list, count)| *count == 1 && !refused.contains(*list))
        .map(|(list, _)| list.to_string())
        .collect()
}

/// The temporaries of `function` that hold a copy of a `str` only for the
/// next operation to append it to a list, as in
/// `out.append(text.clone())`. Only the end of the loan the copy read may
/// lie between the two, so the bytes the copy read are still there when
/// the list takes them.
pub(super) fn appended_copies(function: &MirFunction) -> HashMap<String, String> {
    let mut copies = HashMap::new();
    for block in &function.blocks {
        for (index, instruction) in block.instructions.iter().enumerate() {
            let Some((target, copied)) = copied_str(instruction) else {
                continue;
            };
            let next = block.instructions[index + 1..].iter().find(|next| {
                !matches!(
                    next,
                    Instruction::EndLoan { .. }
                        | Instruction::SourceLocation { .. }
                        | Instruction::SourceName { .. }
                )
            });
            if next.and_then(appended_local) == Some(target) {
                copies.insert(target, copied);
            }
        }
    }
    if copies.is_empty() {
        return HashMap::new();
    }
    // The append is the copy's only use.
    let mut uses = HashMap::<&str, usize>::new();
    let mut mentions = Vec::new();
    for block in &function.blocks {
        for instruction in &block.instructions {
            instruction_place_mentions(instruction, &mut mentions);
        }
        mentions.extend(terminator_place_mentions(&block.terminator));
    }
    for place in mentions {
        *uses
            .entry(place.split('.').next().unwrap_or(place))
            .or_default() += 1;
    }
    let strs = function
        .local_types
        .iter()
        .filter(|local| is_str(&local.ty))
        .map(|local| local.name.as_str())
        .collect::<HashSet<_>>();
    copies
        .into_iter()
        .filter(|(copy, _)| uses.get(copy) == Some(&1) && strs.contains(copy))
        .map(|(copy, copied)| (copy.to_string(), copied.to_string()))
        .collect()
}

/// The `str` operand of `value` when it is `text.to_bytes()`.
pub(super) fn str_to_bytes(value: &Rvalue) -> Option<&Operand> {
    match value {
        Rvalue::Call {
            callee:
                CallTarget::Runtime {
                    op: RuntimeOp::Host(builtin),
                    ..
                },
            args,
            ..
        } if builtin.name() == "str.to_bytes" => match args.as_slice() {
            [text] => Some(&text.value),
            _ => None,
        },
        _ => None,
    }
}

/// The local `instruction` extends a list with by moving it, when it is
/// an `extend` of a moved local.
fn extending_local(instruction: &Instruction) -> Option<&str> {
    let Instruction::Assign {
        value:
            Rvalue::Call {
                callee:
                    CallTarget::Runtime {
                        op: RuntimeOp::List(ListOp::Extend),
                        receiver: Some(Operand::Place(_)),
                        ..
                    },
                args,
                ..
            },
        ..
    } = instruction
    else {
        return None;
    };
    match args.as_slice() {
        [MirArg {
            value: Operand::MovePlace(place),
            writeback_place: None,
            ..
        }] => Some(place),
        _ => None,
    }
}

/// The temporaries of `function` that hold the bytes of a `str` only for
/// the next operation to extend a list with them, as in
/// `out.extend(text.to_bytes())`. Only the end of a loan may lie between
/// the two, so the bytes are still there when the list takes them, and
/// the list is appended to from the `str`'s words with no list made for
/// the bytes.
pub(super) fn extended_bytes(function: &MirFunction) -> HashSet<String> {
    let mut extended = HashSet::new();
    for block in &function.blocks {
        for (index, instruction) in block.instructions.iter().enumerate() {
            let Instruction::Assign { target, value } = instruction else {
                continue;
            };
            if str_to_bytes(value).is_none() {
                continue;
            }
            let next = block.instructions[index + 1..].iter().find(|next| {
                !matches!(
                    next,
                    Instruction::EndLoan { .. }
                        | Instruction::SourceLocation { .. }
                        | Instruction::SourceName { .. }
                )
            });
            if next.and_then(extending_local) == Some(target.as_str()) {
                extended.insert(target.as_str());
            }
        }
    }
    if extended.is_empty() {
        return HashSet::new();
    }
    // The extend is the temporary's only use.
    let mut uses = HashMap::<&str, usize>::new();
    let mut mentions = Vec::new();
    for block in &function.blocks {
        for instruction in &block.instructions {
            instruction_place_mentions(instruction, &mut mentions);
        }
        mentions.extend(terminator_place_mentions(&block.terminator));
    }
    for place in mentions {
        *uses
            .entry(place.split('.').next().unwrap_or(place))
            .or_default() += 1;
    }
    extended
        .into_iter()
        .filter(|temp| uses.get(temp) == Some(&1))
        .map(str::to_string)
        .collect()
}

/// The `str` locals of `function` that a loan read fills only for the
/// next operation to push into an f-string, as `f"{first}"` of a `view`
/// local does. Only source markers lie between the two, and nothing but
/// the local's release reads it again.
pub(super) fn formatted_reads(function: &MirFunction) -> HashSet<String> {
    let strs = function
        .local_types
        .iter()
        .filter(|local| is_str(&local.ty))
        .map(|local| local.name.as_str())
        .collect::<HashSet<_>>();
    let mut reads = HashMap::<&str, usize>::new();
    let mut formatted = HashSet::new();
    let mut uses = HashMap::<&str, usize>::new();
    for block in &function.blocks {
        for (index, instruction) in block.instructions.iter().enumerate() {
            match instruction {
                Instruction::ReadLoan { target, .. } => {
                    *reads.entry(target.as_str()).or_default() += 1;
                    let next = block.instructions[index + 1..].iter().find(|next| {
                        !matches!(
                            next,
                            Instruction::SourceLocation { .. } | Instruction::SourceName { .. }
                        )
                    });
                    if let Some(Instruction::Assign {
                        value: Rvalue::FormatString { parts },
                        ..
                    }) = next
                    {
                        let pushed = parts.iter().any(|part| {
                            matches!(part, MirFormatPart::Value(Operand::Place(place))
                                if place == target)
                        });
                        if pushed && strs.contains(target.as_str()) {
                            formatted.insert(target.as_str());
                        }
                    }
                }
                Instruction::Release { .. } => {}
                instruction => {
                    let mut mentions = Vec::new();
                    instruction_place_mentions(instruction, &mut mentions);
                    for place in mentions {
                        *uses
                            .entry(place.split('.').next().unwrap_or(place))
                            .or_default() += 1;
                    }
                }
            }
        }
        for place in terminator_place_mentions(&block.terminator) {
            *uses
                .entry(place.split('.').next().unwrap_or(place))
                .or_default() += 1;
        }
    }
    formatted
        .into_iter()
        .filter(|local| reads.get(local) == Some(&1) && uses.get(local) == Some(&1))
        .map(str::to_string)
        .collect()
}

/// The f-strings of `function` that only a later f-string in their block
/// takes, as a part: each part of one is one of `formatted`,
/// and only loan reads of `formatted`, releases, source markers and other
/// f-strings lie between the two, none of which can fail. The later one
/// pushes the parts in its place.
pub(super) fn spliced_formats(
    function: &MirFunction,
    formatted: &HashSet<String>,
) -> HashSet<String> {
    let mut uses = HashMap::<&str, usize>::new();
    for block in &function.blocks {
        for instruction in &block.instructions {
            if !matches!(instruction, Instruction::Release { .. }) {
                let mut mentions = Vec::new();
                instruction_place_mentions(instruction, &mut mentions);
                for place in mentions {
                    *uses
                        .entry(place.split('.').next().unwrap_or(place))
                        .or_default() += 1;
                }
            }
        }
        for place in terminator_place_mentions(&block.terminator) {
            *uses
                .entry(place.split('.').next().unwrap_or(place))
                .or_default() += 1;
        }
    }
    let pushes = |parts: &[MirFormatPart], local: &str| {
        parts.iter().any(
            |part| matches!(part, MirFormatPart::Value(Operand::Place(place)) if place == local),
        )
    };
    let mut spliced = HashSet::new();
    for block in &function.blocks {
        for (index, instruction) in block.instructions.iter().enumerate() {
            let Instruction::Assign {
                target,
                value: Rvalue::FormatString { parts },
            } = instruction
            else {
                continue;
            };
            let kept = parts.iter().all(|part| {
                matches!(part, MirFormatPart::Value(Operand::Place(place))
                    if formatted.contains(place))
            });
            // The part that takes it is its only use.
            if !kept || uses.get(target.as_str()) != Some(&1) {
                continue;
            }
            let taken = block.instructions[index + 1..]
                .iter()
                .find(|later| match later {
                    Instruction::SourceLocation { .. }
                    | Instruction::SourceName { .. }
                    | Instruction::Release { .. } => false,
                    Instruction::ReadLoan { target, .. } => !formatted.contains(target),
                    Instruction::Assign {
                        value: Rvalue::FormatString { parts },
                        ..
                    } => pushes(parts, target),
                    _ => true,
                });
            if let Some(Instruction::Assign {
                value: Rvalue::FormatString { parts },
                ..
            }) = taken
            {
                if pushes(parts, target) {
                    spliced.insert(target.clone());
                }
            }
        }
    }
    spliced
}

/// The `str` locals of a function that hold an element of a `list[str]`
/// as words borrowed from the list, as a `for` loop's variable does.
#[derive(Debug, Default)]
pub(super) struct ElementSlots {
    /// Each such local, with the list it reads.
    pub(super) lists: HashMap<String, String>,
}

/// Whether `instruction` reads the `str` local `local` through its bytes,
/// so that holding it as words saves a box. `views` gives the view
/// parameters of the functions that have a twin.
fn reads_words(
    instruction: &Instruction,
    local: &str,
    views: &HashMap<String, Vec<ViewParam>>,
) -> bool {
    let is_local = |operand: &Operand| matches!(operand, Operand::Place(place) if place == local);
    let Instruction::Assign { value, .. } = instruction else {
        return false;
    };
    if str_to_bytes(value).is_some_and(is_local) {
        return true;
    }
    match value {
        Rvalue::Binary {
            op: BinaryOp::Eq | BinaryOp::NotEq,
            left,
            right,
            ..
        } => is_local(left) || is_local(right),
        // `+` joins the words (see `compile_str_slot_concat`).
        Rvalue::Binary {
            op: BinaryOp::Add,
            left,
            right,
            ..
        } => concat_side(left) && concat_side(right) && (is_local(left) || is_local(right)),
        Rvalue::Call {
            callee:
                CallTarget::Runtime {
                    op,
                    receiver: Some(receiver),
                    ..
                },
            args,
            ..
        } => match (op, args.as_slice()) {
            (
                RuntimeOp::Dict(DictOp::Get | DictOp::ContainsKey)
                | RuntimeOp::Set(SetOp::Contains),
                [key],
            ) => is_local(&key.value),
            (op, []) => is_local(receiver) && receiver_view_read(*op).is_some(),
            _ => false,
        },
        Rvalue::FormatString { parts } => parts
            .iter()
            .any(|part| matches!(part, MirFormatPart::Value(value) if is_local(value))),
        _ => direct_callee(value).is_some_and(|(callee, args)| {
            views.get(callee).is_some_and(|params| {
                params.iter().any(|param| {
                    args.get(param.position)
                        .is_some_and(|arg| is_local(&arg.value))
                })
            })
        }),
    }
}

/// Whether `operand` can be a side of a `+` that joins words: a literal,
/// or a local that holds its own `str`, not a part of one.
fn concat_side(operand: &Operand) -> bool {
    match operand {
        Operand::String(_) => true,
        Operand::Place(place) => !place.contains('.'),
        _ => false,
    }
}

/// The element slots of `function`: the locals `lendable` allows that one
/// element read of a local `list[str]` assigns, that only instructions
/// that read them name, that no terminator names, and that some
/// instruction reads through their bytes. `borrowed` gives the operands an
/// instruction's callee borrows where they lie. The list must stay as it
/// is while such a local is still to be read: a local the list may change
/// under (see `elements::lent_element_reads`) keeps its box.
pub(super) fn element_slots<'f>(
    function: &'f MirFunction,
    lendable: impl Fn(&str) -> bool,
    borrowed: impl Fn(&'f Instruction) -> Vec<&'f Operand>,
    views: &HashMap<String, Vec<ViewParam>>,
) -> ElementSlots {
    let strs = function
        .local_types
        .iter()
        .filter(|local| is_str(&local.ty))
        .map(|local| local.name.as_str())
        .collect::<HashSet<_>>();
    if strs.is_empty() {
        return ElementSlots::default();
    }
    let lent = super::elements::lent_element_reads(
        function,
        |place| strs.contains(place) && lendable(place),
        // A `str` is not a Copy type.
        |_| false,
        borrowed,
    );
    let mut lists = lent.lists;
    let mut named = lent
        .promotions
        .values()
        .flatten()
        .map(String::as_str)
        .collect::<HashSet<_>>();
    for block in &function.blocks {
        named.extend(
            terminator_place_mentions(&block.terminator)
                .into_iter()
                .map(|place| place.split('.').next().unwrap_or(place)),
        );
    }
    lists.retain(|local, list| {
        !named.contains(local.as_str())
            && function
                .local_types
                .iter()
                .map(|typed| (&typed.name, &typed.ty))
                .chain(function.params.iter().map(|param| (&param.name, &param.ty)))
                .any(|(name, ty)| {
                    name == list && *ty == Type::Named("list".to_string(), vec![Type::named("str")])
                })
            && function
                .blocks
                .iter()
                .flat_map(|block| &block.instructions)
                .any(|instruction| reads_words(instruction, local, views))
    });
    ElementSlots { lists }
}

/// The `str` copies of `function` that may take the reference they copy
/// instead: each maps the local a `clone()` of a `str` local assigns to
/// that local, to the scrutinee it was read from as a payload, and to
/// whether the block releases that scrutinee too. Nothing but releases, the
/// copied local's among them, and moves of the copy follows the copy in
/// its block, and the terminator names neither, so the copied local is not
/// read again. Every assignment of the copied local makes a new value (a
/// call, an f-string or a binary operation), or it is that one payload
/// read, so it holds no place another value owns.
pub(super) fn clone_moves(function: &MirFunction) -> HashMap<String, CloneMove> {
    let strs = function
        .local_types
        .iter()
        .filter(|local| is_str(&local.ty))
        .map(|local| local.name.as_str())
        .collect::<HashSet<_>>();
    let mut payloads = HashMap::<&str, Option<&str>>::new();
    let mut sources = HashMap::<&str, Vec<Option<Option<&str>>>>::new();
    for instruction in function.blocks.iter().flat_map(|block| &block.instructions) {
        if let Instruction::Assign { target, value } = instruction {
            let made = match value {
                Rvalue::Call { .. } | Rvalue::FormatString { .. } | Rvalue::Binary { .. } => {
                    Some(None)
                }
                Rvalue::Use(Operand::MovePlace(source)) if !source.contains('.') => {
                    Some(Some(source.as_str()))
                }
                _ => None,
            };
            sources.entry(target.as_str()).or_default().push(made);
            let scrutinee = match value {
                Rvalue::VariantPayload {
                    scrutinee: Operand::Place(scrutinee),
                    ..
                } if !scrutinee.contains('.') => Some(scrutinee.as_str()),
                _ => None,
            };
            payloads
                .entry(target.as_str())
                .and_modify(|seen| *seen = None)
                .or_insert(scrutinee);
        }
    }
    // A local is fresh when each of its values is made new or moved from
    // a fresh local.
    let mut fresh = sources
        .iter()
        .filter(|(_, made)| made.iter().all(Option::is_some))
        .map(|(local, _)| *local)
        .collect::<HashSet<_>>();
    loop {
        let stale = fresh
            .iter()
            .copied()
            .filter(|local| {
                sources[local]
                    .iter()
                    .any(|made| matches!(made, Some(Some(source)) if !fresh.contains(source)))
            })
            .collect::<Vec<_>>();
        if stale.is_empty() {
            break;
        }
        for local in stale {
            fresh.remove(local);
        }
    }
    let mut moves = HashMap::new();
    for block in &function.blocks {
        for (index, instruction) in block.instructions.iter().enumerate() {
            let Some((target, copied)) = copied_str(instruction) else {
                continue;
            };
            let Some(released) = block.instructions[index + 1..]
                .iter()
                .filter(|later| {
                    !matches!(later, Instruction::Assign {
                        value: Rvalue::Use(Operand::MovePlace(moved)),
                        ..
                    } if moved == target)
                })
                .map(|later| match later {
                    Instruction::Release { place } => Some(place.as_str()),
                    _ => None,
                })
                .collect::<Option<HashSet<_>>>()
            else {
                continue;
            };
            let scrutinee = payloads.get(copied).copied().flatten();
            let named = terminator_place_mentions(&block.terminator)
                .into_iter()
                .any(|place| {
                    let root = place.split('.').next().unwrap_or(place);
                    root == copied || Some(root) == scrutinee
                });
            let defined = scrutinee.is_some() || fresh.contains(copied);
            if strs.contains(target)
                && strs.contains(copied)
                && defined
                && !function.params.iter().any(|param| param.name == copied)
                && released.contains(copied)
                && !named
            {
                let scrutinee = scrutinee
                    .map(|scrutinee| (scrutinee.to_string(), released.contains(scrutinee)));
                moves.insert(target.to_string(), (copied.to_string(), scrutinee));
            }
        }
    }
    moves
}

/// A copy [`clone_moves`] finds: the copied local, and the scrutinee it
/// was read from as a payload with whether the copy's block releases it.
pub(super) type CloneMove = (String, Option<(String, bool)>);

/// The signature of a view twin: `signature`, the function's own, then
/// `ptr` and `len` for each of its `count` view parameters.
pub(super) fn view_twin_signature(mut signature: Signature, count: usize) -> Signature {
    signature
        .params
        .extend(std::iter::repeat_n(types::I64, 2 * count));
    signature
}

/// The bodies of a view twin as a body under construction calls them.
#[derive(Clone, Debug)]
pub(super) struct ViewTwin {
    pub(super) framed: FuncRef,
    /// The body without write-through sinks, when the function has one.
    pub(super) sinkless: Option<FuncRef>,
    pub(super) params: Vec<ViewParam>,
}

/// The words of a `str` held as words besides its owner, which the
/// parameter's or local's own variable holds.
#[derive(Clone, Copy, Debug)]
pub(super) struct StrSlot {
    ptr: Var,
    len: Var,
    /// For a parameter of a view twin, the box this body made for the
    /// owner, or 0. A local releases its owner itself.
    made: Option<Var>,
}

impl StrSlot {
    /// Whether the slot is a parameter of a view twin.
    pub(super) fn is_param(&self) -> bool {
        self.made.is_some()
    }
}

/// Declares the `ptr` and `len` of each local `elements` holds as words.
pub(super) fn declare_element_slots(
    builder: &mut dyn FunctionBackend,
    elements: &ElementSlots,
    variable_index: &mut usize,
) -> HashMap<String, StrSlot> {
    let mut locals = elements.lists.keys().collect::<Vec<_>>();
    locals.sort();
    let mut slots = HashMap::new();
    for local in locals {
        let mut declare = || {
            let variable = Var(*variable_index as u32);
            *variable_index += 1;
            builder.declare_var(variable, types::I64);
            let zero = builder.iconst(types::I64, 0);
            builder.def_var(variable, zero);
            variable
        };
        let ptr = declare();
        let len = declare();
        slots.insert(
            local.clone(),
            StrSlot {
                ptr,
                len,
                made: None,
            },
        );
    }
    slots
}

/// Binds the `ptr` and `len` words a view twin of `function` takes, which
/// `words` holds in the order of `params`.
pub(super) fn bind_view_params(
    builder: &mut dyn FunctionBackend,
    function: &MirFunction,
    params: &[ViewParam],
    words: &[Val],
    variable_index: &mut usize,
) -> HashMap<String, StrSlot> {
    let mut declare = |builder: &mut dyn FunctionBackend, value: Val| {
        let variable = Var(*variable_index as u32);
        *variable_index += 1;
        builder.declare_var(variable, types::I64);
        builder.def_var(variable, value);
        variable
    };
    let mut slots = HashMap::new();
    for (param, pair) in params.iter().zip(words.chunks(2)) {
        let ptr = declare(builder, pair[0]);
        let len = declare(builder, pair[1]);
        let zero = builder.iconst(types::I64, 0);
        let made = declare(builder, zero);
        slots.insert(
            function.params[param.position].name.clone(),
            StrSlot {
                ptr,
                len,
                made: Some(made),
            },
        );
    }
    slots
}

/// The other side of a comparison with a `str` held as words.
enum ViewSide {
    Words(Val, Val),
    Literal(String),
}

/// The words a literal of `len` bytes is compared in, each an offset and
/// a width in bytes: none when the runtime compares it, as it does a long
/// literal. The last word overlaps the one before it when the widths do
/// not add up to `len`.
pub(super) fn literal_chunks(len: usize) -> Vec<(usize, usize)> {
    if len == 0 || len > 16 || cfg!(target_endian = "big") {
        return Vec::new();
    }
    let width = match len {
        8.. => 8,
        4.. => 4,
        2.. => 2,
        _ => 1,
    };
    let mut chunks = (0..len / width)
        .map(|index| (index * width, width))
        .collect::<Vec<_>>();
    if !len.is_multiple_of(width) {
        chunks.push((len - width, width));
    }
    chunks
}

/// How a call has a `str` argument as words.
pub(super) enum HeldWords {
    /// A literal of the program.
    Literal(String),
    /// A parameter the calling twin takes as words.
    Slot(String),
    /// An element of a `list[str]` a loan selects.
    Loan(DirectViewAlternative),
}

/// The words of one argument a call passes to a view twin.
pub(super) struct ViewArgument {
    pub(super) owner: Val,
    pub(super) ptr: Val,
    pub(super) len: Val,
    /// The owner, when the call releases it once the callee returned.
    pub(super) released: Option<Val>,
}

impl FunctionCompiler<'_> {
    /// Makes the box of the `str` parameter `name` of a view twin when the
    /// caller passed none and no load has made it yet.
    pub(super) fn materialize_str_slot(&mut self, name: &str) -> std::result::Result<(), String> {
        let Some(slot) = self.str_slots.get(name).copied() else {
            return Ok(());
        };
        let variable = self.local_vars(name)?[0];
        let owner = self.builder.use_var(variable);
        let make_block = self.builder.create_block();
        let done_block = self.builder.create_block();
        self.builder.brif(owner, done_block, &[], make_block, &[]);
        self.builder.switch_to_block(make_block);
        self.builder.seal_block(make_block);
        let ptr = self.builder.use_var(slot.ptr);
        let len = self.builder.use_var(slot.len);
        let made = self
            .builder
            .call(self.runtime[RuntimeFn::StringFromView], &[ptr, len])
            .results[0];
        self.builder.def_var(variable, made);
        // A local releases the box it made with its own release.
        if let Some(variable) = slot.made {
            self.builder.def_var(variable, made);
        }
        self.builder.jump(done_block, &[]);
        self.builder.switch_to_block(done_block);
        self.builder.seal_block(done_block);
        Ok(())
    }

    /// Releases `owner` unless it is 0.
    fn release_str_owner(&mut self, owner: Val) {
        let release_block = self.builder.create_block();
        let done_block = self.builder.create_block();
        self.builder
            .brif(owner, release_block, &[], done_block, &[]);
        self.builder.switch_to_block(release_block);
        self.builder.seal_block(release_block);
        self.release_opaque_handle(owner);
        self.builder.jump(done_block, &[]);
        self.builder.switch_to_block(done_block);
        self.builder.seal_block(done_block);
    }

    /// Releases the boxes this view twin made for its parameters, as it
    /// returns.
    pub(super) fn release_made_str_slots(&mut self) {
        let mut made = self
            .str_slots
            .iter()
            .filter_map(|(name, slot)| Some((name.clone(), slot.made?)))
            .collect::<Vec<_>>();
        made.sort();
        for (_, variable) in made {
            let owner = self.builder.use_var(variable);
            self.release_str_owner(owner);
        }
    }

    /// The `ptr` and `len` of `operand` when it is a `str` parameter this
    /// twin holds as words.
    pub(super) fn str_slot_words(&mut self, operand: &Operand) -> Option<(Val, Val)> {
        let Operand::Place(place) = operand else {
            return None;
        };
        let slot = self.str_slots.get(place.as_str()).copied()?;
        Some((
            self.builder.use_var(slot.ptr),
            self.builder.use_var(slot.len),
        ))
    }

    /// Whether the `literal.len()` bytes at `ptr` are `literal`, as 0 or 1.
    /// A short literal is compared in the words both hold.
    pub(super) fn emit_literal_bytes_equal(
        &mut self,
        ptr: Val,
        literal: &[u8],
    ) -> std::result::Result<Val, String> {
        let chunks = literal_chunks(literal.len());
        if chunks.is_empty() {
            let (literal_ptr, literal_len) = self.string_constant(literal)?;
            let inst = self.builder.call(
                self.runtime[RuntimeFn::BytesEqual],
                &[ptr, literal_ptr, literal_len],
            );
            return Ok(inst.results[0]);
        }
        // The bytes may lie anywhere.
        let flags = MemFlags::new();
        let mut difference = self.builder.iconst(types::I64, 0);
        for (offset, width) in chunks {
            let at = offset as i32;
            let read = match width {
                8 => self.builder.load(types::I64, flags, ptr, at),
                4 => self.builder.uload32(flags, ptr, at),
                2 => self.builder.uload16(flags, ptr, at),
                _ => self.builder.uload8(types::I64, flags, ptr, at),
            };
            let mut word = [0u8; 8];
            word[..width].copy_from_slice(&literal[offset..offset + width]);
            let expected = self.builder.iconst(types::I64, i64::from_le_bytes(word));
            let changed = self.builder.bxor(read, expected);
            difference = self.builder.bor(difference, changed);
        }
        let equal = self.builder.icmp_imm(IntCC::Equal, difference, 0);
        Ok(self.builder.uextend(types::I64, equal))
    }

    /// Whether the text at `left`, a `ptr` and a `len`, is `right`, as 0
    /// or 1: the lengths are compared, then the bytes.
    fn emit_view_equality(
        &mut self,
        left: (Val, Val),
        right: &ViewSide,
    ) -> std::result::Result<Val, String> {
        let same_len = match right {
            ViewSide::Words(_, len) => self.builder.icmp(IntCC::Equal, left.1, *len),
            ViewSide::Literal(text) => {
                self.builder
                    .icmp_imm(IntCC::Equal, left.1, text.len() as i64)
            }
        };
        if matches!(right, ViewSide::Literal(text) if text.is_empty()) {
            return Ok(self.builder.uextend(types::I64, same_len));
        }
        let bytes_block = self.builder.create_block();
        let join_block = self.builder.create_block();
        self.builder.append_block_param(join_block, types::I64);
        let zero = self.builder.iconst(types::I64, 0);
        self.builder
            .brif(same_len, bytes_block, &[], join_block, &[zero]);
        self.builder.switch_to_block(bytes_block);
        self.builder.seal_block(bytes_block);
        let equal = match right {
            ViewSide::Literal(text) => self.emit_literal_bytes_equal(left.0, text.as_bytes())?,
            ViewSide::Words(ptr, len) => {
                self.builder
                    .call(self.runtime[RuntimeFn::BytesEqual], &[left.0, *ptr, *len])
                    .results[0]
            }
        };
        self.builder.jump(join_block, &[equal]);
        self.builder.switch_to_block(join_block);
        self.builder.seal_block(join_block);
        Ok(self.builder.block_params(join_block)[0])
    }

    /// The `ptr` and `len` of the text in the `str` box `value`, which
    /// stay valid while the box lives unchanged.
    pub(super) fn str_box_words(&mut self, value: Val) -> (Val, Val) {
        let slot = self.builder.create_sized_stack_slot(8, 3);
        let len_at = self.builder.stack_addr(types::I64, slot, 0);
        let ptr = self
            .builder
            .call(self.runtime[RuntimeFn::StringWords], &[value, len_at])
            .results[0];
        let len = self
            .builder
            .load(types::I64, MemFlags::trusted(), len_at, 0);
        (ptr, len)
    }

    /// `left == right` or `left != right` on two `str` boxes, or on a `str`
    /// box and `literal`, compared in their words, as 0 or 1.
    pub(super) fn compile_str_box_equality(
        &mut self,
        op: BinaryOp,
        left: Val,
        right: Result<Val, &str>,
    ) -> std::result::Result<ValueRef, String> {
        let words = self.str_box_words(left);
        let other = match right {
            Ok(right) => {
                let (ptr, len) = self.str_box_words(right);
                ViewSide::Words(ptr, len)
            }
            Err(literal) => ViewSide::Literal(literal.to_string()),
        };
        let mut equal = self.emit_view_equality(words, &other)?;
        if op == BinaryOp::NotEq {
            equal = self.builder.bxor_imm(equal, 1);
        }
        Ok(ValueRef {
            values: vec![equal],
            ty: DirectType::Scalar(ScalarKind::Bool),
        })
    }

    /// `left == right` or `left != right` where one side is a `str` this
    /// twin holds as words and the other is one too, or a literal.
    pub(super) fn compile_str_view_comparison(
        &mut self,
        op: BinaryOp,
        left: &Operand,
        right: &Operand,
    ) -> std::result::Result<Option<ValueRef>, String> {
        if self.str_slots.is_empty() || !matches!(op, BinaryOp::Eq | BinaryOp::NotEq) {
            return Ok(None);
        }
        let (held, other) = if self.str_slot_words(left).is_some() {
            (left, right)
        } else {
            (right, left)
        };
        let Some(words) = self.str_slot_words(held) else {
            return Ok(None);
        };
        let mut released = None;
        let other = match (other, self.words_of(other)) {
            (Operand::String(text), _) => ViewSide::Literal(text.clone()),
            (_, Some(held @ (HeldWords::Slot(_) | HeldWords::Loan(_)))) => {
                let words = self.load_held_words(&held, false)?;
                released = words.released;
                ViewSide::Words(words.ptr, words.len)
            }
            _ => return Ok(None),
        };
        let mut equal = self.emit_view_equality(words, &other)?;
        self.release_view_arguments(released.into_iter().collect());
        if op == BinaryOp::NotEq {
            equal = self.builder.bxor_imm(equal, 1);
        }
        Ok(Some(ValueRef {
            values: vec![equal],
            ty: DirectType::Scalar(ScalarKind::Bool),
        }))
    }

    /// `left + right` on `str` values where a side is an element this
    /// function holds as words: both sides join from their words, so the
    /// element gets no box of its own. The other side is a literal, another
    /// such element, or a local's box, which stays its local's.
    pub(super) fn compile_str_slot_concat(
        &mut self,
        left: &Operand,
        right: &Operand,
        span: Span,
    ) -> std::result::Result<Option<ValueRef>, String> {
        let slots = &self.str_slots;
        let held = |operand: &Operand| matches!(operand, Operand::Place(place) if slots.contains_key(place.as_str()));
        let sides = [(left, held(left)), (right, held(right))];
        if !(sides[0].1 || sides[1].1) || !concat_side(left) || !concat_side(right) {
            return Ok(None);
        }
        let mut words = Vec::new();
        for (side, held) in sides {
            let (ptr, len) = match side {
                Operand::String(text) => self.string_constant(text.as_bytes())?,
                _ if held => self.str_slot_words(side).expect("a held side has words"),
                _ => {
                    let value = self.load_operand(side)?;
                    let value = self.ensure_opaque(value)?;
                    self.str_box_words(value.values[0])
                }
            };
            words.extend([ptr, len]);
        }
        let (line, column) = self.span_values(Some(span));
        words.extend([line, column]);
        let results = self.runtime_call_results(self.runtime[RuntimeFn::ConcatStrWords], &words);
        Ok(Some(self.owned_opaque_result(results, Type::named("str"))))
    }

    /// A runtime operation on a `str` receiver this twin holds as words,
    /// run on the bytes (see [`receiver_view_read`]).
    pub(super) fn compile_str_view_read(
        &mut self,
        callee: &CallTarget,
        args: &[MirArg],
    ) -> std::result::Result<Option<ValueRef>, String> {
        let CallTarget::Runtime {
            op,
            receiver: Some(receiver),
            ..
        } = callee
        else {
            return Ok(None);
        };
        if !args.is_empty() {
            return Ok(None);
        }
        match receiver_view_read(*op) {
            Some(ViewRead::NewBox(function)) => {
                // A copy of a loaned element is made where the loan is read.
                let Some((ptr, len)) = self.str_slot_words(receiver) else {
                    return Ok(None);
                };
                let results = self.runtime_call_results(self.runtime[function], &[ptr, len]);
                Ok(Some(self.owned_opaque_result(results, Type::named("str"))))
            }
            Some(ViewRead::ByteLen) => {
                let Some(words) = self.load_place_words(receiver)? else {
                    return Ok(None);
                };
                self.release_view_arguments(words.released.into_iter().collect());
                Ok(Some(ValueRef {
                    values: vec![words.len],
                    ty: DirectType::Scalar(ScalarKind::Int64),
                }))
            }
            None => Ok(None),
        }
    }

    /// The selection the loan `place` makes when it selects one element of
    /// a `list[str]` that generated code can read where the list keeps it.
    fn loaned_str_element(&self, place: &str) -> Option<DirectViewAlternative> {
        if place.contains('.')
            || !self.view_places.contains_key(place)
            || self.fused_entry_copies.contains_key(place)
            || self.native_loans.contains_key(place)
        {
            return None;
        }
        let resolved = self.resolve_view_place(place).ok()?;
        let [alternative] = resolved.alternatives.as_slice() else {
            return None;
        };
        let [selector] = alternative.elements.as_slice() else {
            return None;
        };
        (alternative.conditions.is_empty()
            && selector.projection.is_empty()
            && matches!(selector.kind, DirectElementKind::List)
            && is_str(&selector.element_type))
        .then(|| alternative.clone())
    }

    /// Jumps to `join_block` with the words of element `index` of the
    /// `list[str]` `list`, read where the list's view keeps them, or to
    /// `slow_block` when the list has no view or the index is out of range.
    fn emit_str_view_entry(&mut self, list: Val, index: Val, slow_block: Blk, join_block: Blk) {
        use aura_runtime::native_runtime::{
            DirectListAccess, DIRECT_STR_VIEW_ENTRY_BYTES, DIRECT_STR_VIEW_LEN_OFFSET,
            DIRECT_STR_VIEW_PTR_OFFSET,
        };
        let layout = aura_runtime::native_runtime::direct_list_layout();
        let fast_block = self.builder.create_block();
        self.emit_list_access_check(list, DirectListAccess::Str, &layout, slow_block, fast_block);
        self.builder.switch_to_block(fast_block);
        self.builder.seal_block(fast_block);
        let flags = MemFlags::trusted();
        let entries = self
            .builder
            .load(types::I64, flags, list, layout.elements_offset);
        let position = self.emit_list_view_bounds_check(list, index, &layout, slow_block);
        let offset = self
            .builder
            .imul_imm(position, i64::from(DIRECT_STR_VIEW_ENTRY_BYTES));
        let entry = self.builder.iadd(entries, offset);
        let ptr = self
            .builder
            .load(types::I64, flags, entry, DIRECT_STR_VIEW_PTR_OFFSET);
        let len = self
            .builder
            .load(types::I64, flags, entry, DIRECT_STR_VIEW_LEN_OFFSET);
        let borrowed = self.builder.iconst(types::I64, 0);
        self.builder.jump(join_block, &[ptr, len, borrowed]);
    }

    /// Jumps to `join_block` with the words of element `index` of the
    /// `list[str]` `list`, which has no view: read where the list keeps
    /// them when it can, as the view would give them, and otherwise those of
    /// a copy, with the copy's box as their owner.
    fn jump_with_element_words(
        &mut self,
        list: Val,
        index: Val,
        line: Val,
        column: Val,
        join_block: Blk,
    ) {
        let slot = self.builder.create_sized_stack_slot(16, 3);
        let out = self.builder.stack_addr(types::I64, slot, 0);
        let ptr = self
            .builder
            .call(
                self.runtime[RuntimeFn::VecStrElementWords],
                &[list, index, line, column, out],
            )
            .results[0];
        let flags = MemFlags::trusted();
        let len = self.builder.load(types::I64, flags, out, 0);
        let owner = self.builder.load(types::I64, flags, out, 8);
        self.builder.jump(join_block, &[ptr, len, owner]);
    }

    /// Jumps to `join_block` with the words of the `str` in the box
    /// `owner`, which owns them.
    fn jump_with_owned_str(&mut self, owner: Val, join_block: Blk) {
        let ptr = self
            .builder
            .call(self.runtime[RuntimeFn::StringBytes], &[owner])
            .results[0];
        let len = self
            .builder
            .call(self.runtime[RuntimeFn::StringByteLen], &[owner])
            .results[0];
        self.builder.jump(join_block, &[ptr, len, owner]);
    }

    /// Assigns `target` the program's frozen copy of the literal of `str`
    /// literals `elements` (see `str_literal_lists`), which the first run
    /// makes.
    pub(super) fn compile_str_literal_list(
        &mut self,
        target: &str,
        elements: &[Operand],
    ) -> std::result::Result<(), String> {
        let texts = elements
            .iter()
            .filter_map(|element| match element {
                Operand::String(text) => Some(text.as_str()),
                _ => None,
            })
            .collect::<Vec<_>>();
        let lens = texts
            .iter()
            .map(|text| u32::try_from(text.len()).map(u32::to_le_bytes))
            .collect::<std::result::Result<Vec<_>, _>>()
            .map_err(|_| "a `str` literal is too long for a list literal".to_string())?;
        let list_type = Type::Named("list".to_string(), vec![Type::named("str")]);
        let encoded = crate::native_runtime::canonical_runtime_type_name(&list_type);
        let (type_ptr, type_len) = self.string_constant(encoded.as_bytes())?;
        let tag_cell = self.tag_cell(&encoded)?;
        let (texts_ptr, _) = self.string_constant(texts.concat().as_bytes())?;
        let (lens_ptr, _) = self.string_constant(&lens.concat())?;
        let count = self.builder.iconst(types::I64, texts.len() as i64);
        let cell = word_cell(
            self.tag_cells,
            &mut *self.builder,
            &format!("\0str literal list {texts:?}"),
        )?;
        let call = self.builder.call(
            self.runtime[RuntimeFn::StrLiteralListCell],
            &[
                cell, tag_cell, type_ptr, type_len, texts_ptr, lens_ptr, count,
            ],
        );
        let value = self.owned_opaque_result(call.results, list_type);
        self.mark_emptied_store_root(target);
        self.store_place(target, value)
    }

    /// The words of the element the loan `selection` selects: borrowed
    /// from the list when its view holds the element, and otherwise those
    /// of a copy the runtime makes as a read of the loan does.
    fn load_loaned_str_words(
        &mut self,
        selection: &DirectViewAlternative,
    ) -> std::result::Result<(Val, Val, Val), String> {
        let selector = &selection.elements[0];
        let index = self.builder.use_var(selector.variable);
        let slow_block = self.builder.create_block();
        let join_block = self.builder.create_block();
        for _ in 0..3 {
            self.builder.append_block_param(join_block, types::I64);
        }
        let field = match self.class_place(&selection.place)? {
            Some((root, slot, _)) if matches!(slot.kind, classes::SlotKind::Handle) => {
                Some((root, slot))
            }
            _ => None,
        };
        if let Some((root, slot)) = field {
            // A list in a field box of an instance kept in place.
            let object = self.load_root(&root)?.values[0];
            let fast_block = self.builder.create_block();
            let field_block = self.builder.create_block();
            self.emit_class_check(object, slot.access, slow_block, fast_block);
            self.builder.switch_to_block(fast_block);
            self.builder.seal_block(fast_block);
            let block = self.class_block(object);
            let list = self
                .builder
                .load(types::I64, MemFlags::trusted(), block, slot.offset);
            self.builder.brif(list, field_block, &[], slow_block, &[]);
            self.builder.switch_to_block(field_block);
            self.builder.seal_block(field_block);
            // A list without a view reads the element as a copy, which
            // counts the miss so the list makes its view.
            let miss_block = self.builder.create_block();
            self.emit_str_view_entry(list, index, miss_block, join_block);
            self.builder.switch_to_block(miss_block);
            self.builder.seal_block(miss_block);
            let (line, column) = self.span_operands(selector.span);
            self.jump_with_element_words(list, index, line, column, join_block);
            self.builder.switch_to_block(slow_block);
            self.builder.seal_block(slow_block);
            let (collection, projection) = self.load_element_path_root(&selection.place)?;
            let copy = self.element_path_load(collection.values[0], &projection, selection)?;
            self.jump_with_owned_str(copy, join_block);
        } else {
            let (collection, projection) = self.load_element_path_root(&selection.place)?;
            if projection.is_empty() {
                self.emit_str_view_entry(collection.values[0], index, slow_block, join_block);
                self.builder.switch_to_block(slow_block);
                self.builder.seal_block(slow_block);
                let (line, column) = self.span_operands(selector.span);
                self.jump_with_element_words(collection.values[0], index, line, column, join_block);
            } else {
                // A list the runtime walks to.
                self.builder.jump(slow_block, &[]);
                self.builder.switch_to_block(slow_block);
                self.builder.seal_block(slow_block);
                let copy = self.element_path_load(collection.values[0], &projection, selection)?;
                self.jump_with_owned_str(copy, join_block);
            }
        }
        self.builder.switch_to_block(join_block);
        self.builder.seal_block(join_block);
        let words = self.builder.block_params(join_block);
        Ok((words[0], words[1], words[2]))
    }

    /// How a call has `operand` as words without making a box for it, when
    /// it does.
    fn words_of(&self, operand: &Operand) -> Option<HeldWords> {
        match operand {
            Operand::String(text) => Some(HeldWords::Literal(text.clone())),
            Operand::Place(place) if self.str_slots.contains_key(place.as_str()) => {
                Some(HeldWords::Slot(place.clone()))
            }
            Operand::Place(place) => self.loaned_str_element(place).map(HeldWords::Loan),
            _ => None,
        }
    }

    /// The view twin a call of `name` with `args` enters, and the words
    /// the call has for each parameter the twin takes as words: the
    /// function has a twin, the call asks for no other body of it, and it
    /// has each such argument as words.
    pub(super) fn view_twin_call(
        &self,
        name: &str,
        args: &[MirArg],
        generic: bool,
    ) -> Option<(ViewTwin, HashMap<usize, HeldWords>)> {
        let twin = self.view_twin_refs.get(name)?;
        if generic || self.result_twin_request.as_deref() == Some(name) {
            return None;
        }
        let mut held = HashMap::new();
        for param in &twin.params {
            let argument = args
                .get(param.position)
                .filter(|argument| argument.writeback_place.is_none())?;
            held.insert(param.position, self.words_of(&argument.value)?);
        }
        Some((twin.clone(), held))
    }

    /// Loads the words `held` names. `boxed` says whether the reader may
    /// load the box, so that it needs an owner.
    pub(super) fn load_held_words(
        &mut self,
        held: &HeldWords,
        boxed: bool,
    ) -> std::result::Result<ViewArgument, String> {
        Ok(match held {
            HeldWords::Literal(text) => {
                let (ptr, len) = self.string_constant(text.as_bytes())?;
                // A body that may load the box gets the literal's own.
                let owner = if boxed {
                    self.load_borrowed_string_literal(text)?.values[0]
                } else {
                    self.builder.iconst(types::I64, 0)
                };
                ViewArgument {
                    owner,
                    ptr,
                    len,
                    released: None,
                }
            }
            HeldWords::Slot(place) => {
                let slot = self.str_slots[place.as_str()];
                let variable = self.local_vars(place)?[0];
                ViewArgument {
                    owner: self.builder.use_var(variable),
                    ptr: self.builder.use_var(slot.ptr),
                    len: self.builder.use_var(slot.len),
                    released: None,
                }
            }
            HeldWords::Loan(selection) => {
                let (ptr, len, owner) = self.load_loaned_str_words(selection)?;
                ViewArgument {
                    owner,
                    ptr,
                    len,
                    released: Some(owner),
                }
            }
        })
    }

    /// Whether `operand` is a `str` place this body has as words.
    pub(super) fn holds_place_words(&self, operand: &Operand) -> bool {
        matches!(
            self.words_of(operand),
            Some(HeldWords::Slot(_) | HeldWords::Loan(_))
        )
    }

    /// The words of `operand` when it is a `str` place this body has as
    /// words: a parameter held as words, or a loaned element.
    pub(super) fn load_place_words(
        &mut self,
        operand: &Operand,
    ) -> std::result::Result<Option<ViewArgument>, String> {
        match self.words_of(operand) {
            Some(held) if !matches!(held, HeldWords::Literal(_)) => {
                self.load_held_words(&held, false).map(Some)
            }
            _ => Ok(None),
        }
    }

    /// Appends the f-string part `value` to the builder `handle` from its
    /// bytes, when this body has it as words.
    pub(super) fn push_format_words(
        &mut self,
        handle: Val,
        value: &Operand,
    ) -> std::result::Result<bool, String> {
        if let Some(parts) = match value {
            Operand::Place(place) => self.spliced_formats.remove(place.as_str()),
            _ => None,
        } {
            for words in parts {
                self.push_view_argument(handle, words);
            }
            return Ok(true);
        }
        let deferred = match value {
            Operand::Place(place) => self.deferred_str_copies.remove(place.as_str()),
            _ => None,
        };
        let words = match deferred {
            Some(words) => words,
            None => match self.load_place_words(value)? {
                Some(words) => words,
                None => return Ok(false),
            },
        };
        self.push_view_argument(handle, words);
        Ok(true)
    }

    /// The result of `in_place` on `receiver`, the bytes and length of the
    /// `str` key `key`, and `extra`, when this body holds the key as words.
    /// The copy the words were read from, if any, is released after it.
    pub(super) fn call_with_held_str_key(
        &mut self,
        key: &Operand,
        receiver: Val,
        in_place: FuncRef,
        extra: &[Val],
    ) -> std::result::Result<Option<Val>, String> {
        let Some(words) = self.load_place_words(key)? else {
            return Ok(None);
        };
        let mut arguments = vec![receiver, words.ptr, words.len];
        arguments.extend_from_slice(extra);
        let found = self.builder.call(in_place, &arguments).results[0];
        self.release_view_arguments(words.released.into_iter().collect());
        Ok(Some(found))
    }

    /// Boxes the key of the entry loan `loan` that a view twin holds as
    /// words, for the walk to the entry, when it is still deferred.
    pub(super) fn load_deferred_entry_key(
        &mut self,
        loan: &str,
    ) -> std::result::Result<(), String> {
        if let Some((key, variable)) = self.deferred_entry_keys.remove(loan) {
            let key = self.load_root(&key)?.values[0];
            self.builder.def_var(variable, key);
        }
        Ok(())
    }

    /// Pushes `words` into the f-string `handle` builds and releases the
    /// copy they were read from, if any.
    fn push_view_argument(&mut self, handle: Val, words: ViewArgument) {
        self.builder.call(
            self.runtime[RuntimeFn::FormatPushStr],
            &[handle, words.ptr, words.len],
        );
        self.release_view_arguments(words.released.into_iter().collect());
    }

    /// Keeps the parts of the f-string `value` instead of building it into
    /// `target`, when a later f-string takes `target` as a part (see
    /// [`spliced_formats`]) and this body has each part's `str` as words.
    pub(super) fn splice_format(
        &mut self,
        target: &str,
        value: &Rvalue,
    ) -> std::result::Result<bool, String> {
        let Rvalue::FormatString { parts } = value else {
            return Ok(false);
        };
        // The planner made each part a read this body may have kept.
        let reads = parts
            .iter()
            .filter_map(|part| match part {
                MirFormatPart::Value(Operand::Place(place)) => Some(place.as_str()),
                _ => None,
            })
            .collect::<Vec<_>>();
        if !self.spliced_plan.contains(target)
            || !reads
                .iter()
                .all(|read| self.deferred_str_copies.contains_key(*read))
        {
            return Ok(false);
        }
        let parts = reads
            .into_iter()
            .filter_map(|read| self.deferred_str_copies.remove(read))
            .collect();
        self.spliced_formats.insert(target.to_string(), parts);
        self.deferred_reads.insert(target.to_string());
        Ok(true)
    }

    /// Keeps the words of the `str` that `value` copies into `target`,
    /// instead of the copy, when the next operation appends `target` to a
    /// list (see [`appended_copies`]) and this body has the `str` as words.
    pub(super) fn defer_str_copy(&mut self, target: &str) -> std::result::Result<bool, String> {
        let Some(copied) = self.appended_copies.get(target).cloned() else {
            return Ok(false);
        };
        let Some(words) = self.load_place_words(&Operand::Place(copied))? else {
            return Ok(false);
        };
        self.deferred_str_copies.insert(target.to_string(), words);
        Ok(true)
    }

    /// Keeps the words of the loan `loan` instead of reading it into
    /// `target`, when the next operation pushes `target` into an f-string
    /// (see [`formatted_reads`]) and this body has the loan as words.
    pub(super) fn defer_formatted_read(
        &mut self,
        target: &str,
        loan: &str,
    ) -> std::result::Result<bool, String> {
        if !self.formatted_reads.contains(target)
            || self.shadow_read_targets.contains(target)
            || self.lent_payloads.contains(target)
            || self.node_read_targets.contains(target)
        {
            return Ok(false);
        }
        let Some(words) = self.load_place_words(&Operand::Place(loan.to_string()))? else {
            return Ok(false);
        };
        self.deferred_str_copies.insert(target.to_string(), words);
        self.deferred_reads.insert(target.to_string());
        Ok(true)
    }

    /// Refuses a read of the deferred copy `name` other than its append,
    /// which [`appended_copies`] makes the copy's only use.
    pub(super) fn refuse_deferred_str_copy(
        &mut self,
        name: &str,
    ) -> std::result::Result<(), String> {
        let deferred =
            self.deferred_str_copies.contains_key(name) || self.spliced_formats.contains_key(name);
        (!deferred)
            .then_some(())
            .ok_or_else(|| format!("direct backend read the deferred copy `{name}` early"))
    }

    /// Appends the deferred copy `operand` moves to the `list[str]` `list`
    /// from its bytes, when it is one.
    pub(super) fn push_deferred_str_copy(&mut self, list: Val, operand: &Operand) -> bool {
        let Operand::MovePlace(place) = operand else {
            return false;
        };
        let Some(words) = self.deferred_str_copies.remove(place.as_str()) else {
            return false;
        };
        self.builder.call(
            self.runtime[RuntimeFn::VecPushStrView],
            &[list, words.ptr, words.len],
        );
        self.release_view_arguments(words.released.into_iter().collect());
        true
    }

    /// Keeps the words of the `str` whose bytes `value` copies into
    /// `target`, instead of the copy, when the next operation extends a
    /// list with `target` (see [`extended_bytes`]) and this body has the
    /// `str` as words, as a literal, or as the box of a local that owns it.
    pub(super) fn defer_extended_bytes(
        &mut self,
        target: &str,
        value: &Rvalue,
    ) -> std::result::Result<bool, String> {
        if !self.extended_bytes.contains(target) {
            return Ok(false);
        }
        let Some(text) = str_to_bytes(value) else {
            return Ok(false);
        };
        let words = match text {
            Operand::String(text) => {
                let (ptr, len) = self.string_constant(text.as_bytes())?;
                let owner = self.builder.iconst(types::I64, 0);
                ViewArgument {
                    owner,
                    ptr,
                    len,
                    released: None,
                }
            }
            Operand::Place(place) if self.owns_counted_str(place) => {
                let held = self.load_root(place)?.values[0];
                let (ptr, len) = self.str_box_words(held);
                ViewArgument {
                    owner: held,
                    ptr,
                    len,
                    released: None,
                }
            }
            operand => match self.load_place_words(operand)? {
                Some(words) => words,
                None => return Ok(false),
            },
        };
        self.deferred_str_copies.insert(target.to_string(), words);
        Ok(true)
    }

    /// The result of `text.to_bytes()` made from the words of `text`: a
    /// literal, a `str` this body has as words, or one in its box.
    pub(super) fn str_to_bytes_from_words(
        &mut self,
        text: &Operand,
    ) -> std::result::Result<Option<Vec<Val>>, String> {
        let (ptr, len, released) = match text {
            Operand::String(text) => {
                let (ptr, len) = self.string_constant(text.as_bytes())?;
                (ptr, len, None)
            }
            operand => match self.load_place_words(operand)? {
                Some(words) => (words.ptr, words.len, words.released),
                None => {
                    // A `str` in its box is read where the box keeps it.
                    let Operand::Place(place) = operand else {
                        return Ok(None);
                    };
                    let DirectType::Opaque(ty) = self.type_of_place(place)? else {
                        return Ok(None);
                    };
                    if !is_str(&ty) {
                        return Ok(None);
                    }
                    let owner = self.load_place(place)?.values[0];
                    let ptr = self
                        .builder
                        .call(self.runtime[RuntimeFn::StringBytes], &[owner])
                        .results[0];
                    let len = self
                        .builder
                        .call(self.runtime[RuntimeFn::StringByteLen], &[owner])
                        .results[0];
                    (ptr, len, None)
                }
            },
        };
        let made = self
            .builder
            .call(self.runtime[RuntimeFn::StringToBytesView], &[ptr, len])
            .results;
        self.release_view_arguments(released.into_iter().collect());
        Ok(Some(made))
    }

    /// Extends the `list[uint8]` `list` with the deferred bytes `operand`
    /// moves, from their words, when they are deferred.
    pub(super) fn extend_deferred_bytes(&mut self, list: Val, operand: &Operand) -> bool {
        let Operand::MovePlace(place) = operand else {
            return false;
        };
        if !self.extended_bytes.contains(place.as_str()) {
            return false;
        }
        let Some(words) = self.deferred_str_copies.remove(place.as_str()) else {
            return false;
        };
        self.builder.call(
            self.runtime[RuntimeFn::VecExtendUtf8],
            &[list, words.ptr, words.len],
        );
        self.release_view_arguments(words.released.into_iter().collect());
        true
    }

    /// Drops the element slots another way of holding the local or of
    /// walking the list already claims.
    pub(super) fn retain_str_element_slots(&mut self, claimed: impl Fn(&Self, &str, &str) -> bool) {
        let dropped = self
            .str_element_lists
            .iter()
            .filter(|(local, list)| claimed(self, local, list))
            .map(|(local, _)| local.clone())
            .collect::<Vec<_>>();
        self.str_element_lists
            .retain(|local, _| !dropped.contains(local));
        self.str_slots.retain(|local, _| !dropped.contains(local));
    }

    /// Assigns the element read `value` to `target` as words, when
    /// `target` is an element slot: borrowed from the list where its view
    /// keeps the element, and otherwise the words of the copy the runtime
    /// makes, which the local owns.
    pub(super) fn compile_str_element_assign(
        &mut self,
        target: &str,
        value: &Rvalue,
    ) -> std::result::Result<bool, String> {
        let Some(list) = self.str_element_lists.get(target).cloned() else {
            return Ok(false);
        };
        let (receiver, [index, line, column]) = element_read(value)
            .ok_or_else(|| format!("direct backend expected `{target}` to read `{list}`"))?;
        debug_assert_eq!(receiver, list);
        // The local was released or never held a value, unless the MIR
        // says otherwise.
        if self.emptied_store_root.take().as_deref() != Some(target) {
            self.release_root_words(target)?;
        }
        let list = self.load_root(&list)?.values[0];
        let index = self.load_word(index, ScalarKind::Int64, false)?;
        let line = self.load_word(line, ScalarKind::Int32, false)?;
        let column = self.load_word(column, ScalarKind::Int32, false)?;
        let slow_block = self.builder.create_block();
        let join_block = self.builder.create_block();
        for _ in 0..3 {
            self.builder.append_block_param(join_block, types::I64);
        }
        self.emit_str_view_entry(list, index, slow_block, join_block);
        self.builder.switch_to_block(slow_block);
        self.builder.seal_block(slow_block);
        // The runtime reports an index out of range as a read of it does.
        self.jump_with_element_words(list, index, line, column, join_block);
        self.builder.switch_to_block(join_block);
        self.builder.seal_block(join_block);
        let words = self.builder.block_params(join_block).to_vec();
        let slot = self.str_slots[target];
        self.builder.def_var(slot.ptr, words[0]);
        self.builder.def_var(slot.len, words[1]);
        let variable = self.local_vars(target)?[0];
        self.builder.def_var(variable, words[2]);
        Ok(true)
    }

    /// Compiles the copy `target` of a `str` local as a move of the local's
    /// reference, when the copy is the local's last read (see
    /// [`clone_moves`]) and the local owns a counted reference: its own,
    /// one a lookup slot's payload read took when the copy's block releases
    /// the slot, or one a payload read of any other variant took. The local
    /// then releases nothing. A payload a lookup or a result slot lends
    /// keeps its copy.
    pub(super) fn compile_clone_move(&mut self, target: &str) -> std::result::Result<bool, String> {
        let Some((copied, scrutinee)) = self.clone_moves.get(target).cloned() else {
            return Ok(false);
        };
        let counted_payload = |this: &Self, (scrutinee, released): (String, bool)| {
            if this.lookup_slots.contains(&scrutinee) {
                released
            } else {
                !this.result_slots.contains_key(&scrutinee)
            }
        };
        if self.lent_elements.contains_key(&copied)
            || !self.owns_counted_str(&copied)
            || !scrutinee.is_none_or(|scrutinee| counted_payload(self, scrutinee))
        {
            return Ok(false);
        }
        let copied_var = self.local_vars(&copied)?[0];
        let moved = self.builder.use_var(copied_var);
        let zero = self.builder.iconst(types::I64, 0);
        self.builder.def_var(copied_var, zero);
        let moved = ValueRef {
            values: vec![moved],
            ty: DirectType::Opaque(Type::named("str")),
        };
        self.mark_temporary_opaque_owned(&moved);
        self.store_place(target, moved)?;
        Ok(true)
    }

    /// Whether the `str` local `name` holds a counted reference of its own
    /// that its release gives up, rather than words, a loan, a lent box, or
    /// a box a lookup or a list lends it.
    fn owns_counted_str(&self, name: &str) -> bool {
        self.tracked_roots.contains(name)
            && matches!(self.variable_types.get(name), Some(DirectType::Opaque(_)))
            && !self.loan_locals.contains(name)
            && !self
                .writeback_locals
                .iter()
                .any(|(writeback, _)| writeback == name)
            && !self.str_slots.contains_key(name)
            && !self.lookup_slots.contains(name)
            && !self.lent_places.contains(name)
            && !self.lent_payloads.contains(name)
            && !self.element_lend_temps.contains(name)
            && !self.field_lend_temps.contains(name)
            && !self.shadow_read_targets.contains(name)
            && !self.node_read_targets.contains(name)
            && !self.recovery_slots.contains_key(name)
            && !self.result_slots.contains_key(name)
            && !self.text_byte_slices.contains(name)
            && !self.json_views.contains(name)
            && !self.lent_temps.contains_key(name)
            && !self.called_loan_reads.contains_key(name)
            && !self.in_place_callees.contains_key(name)
            && !self.node_payload_values.contains_key(name)
            && !self.view_places.contains_key(name)
    }

    /// Releases the copies a call of a view twin made for its arguments.
    pub(super) fn release_view_arguments(&mut self, owners: Vec<Val>) {
        for owner in owners {
            self.release_str_owner(owner);
        }
    }
}

impl ViewTwin {
    /// Whether the twin may load the box of its parameter at `position`, so
    /// that a caller passes the box it has.
    pub(super) fn boxes(&self, position: usize) -> bool {
        self.params
            .iter()
            .any(|param| param.position == position && !param.pure)
    }

    /// The body a call enters: the one without sinks when the call hands
    /// no sink over and the function has it.
    pub(super) fn callee(&self, sinks: bool) -> FuncRef {
        match self.sinkless {
            Some(sinkless) if !sinks => sinkless,
            _ => self.framed,
        }
    }
}

#[cfg(test)]
#[path = "native_codegen_str_slots_tests.rs"]
mod tests;
