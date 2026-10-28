//! Finds the boxes that already carry the tag of their static type.
//!
//! Coercing a box to its own static type tags it, because the runtime may
//! have made the box without the shape the type records: an element type,
//! an integer kind, or a generic enum's type arguments. The tag costs a
//! runtime call that compares types. Every plain assignment to a local of
//! a fully known type tags the box it stores (see `coerce_value_at`), and so
//! does every `return` and every argument passed to a named function. So a
//! box read out of a local that only such assignments write, a parameter
//! that only such calls fill, or the result of a function that only returns
//! through `return`, already carries its type's tag, and coercing it to
//! that type tags nothing.
//!
//! A local is left out when anything else may write it: a `try`, a loan
//! it holds, a mutable lend, a closure capture, a `for` binding, or one of
//! the emitter's lent and lookup forms. A local passed for write-back stays
//! in, since storing what the callee hands back tags it, and so does a
//! local a mutable loan borrows, since the loan changes what the box holds,
//! not the box.

use std::collections::HashSet;

use crate::mir::{Instruction, MirFunction, MirModule, Operand, Rvalue, Terminator};

/// What the module's functions promise about the tags of the boxes they
/// pass and return.
#[derive(Debug, Default)]
pub(super) struct ProgramShapes {
    /// The free functions only named calls reach, whose callers tag every
    /// argument with its parameter's type.
    direct_only: HashSet<String>,
    /// The functions that return only through `return`, which tags the
    /// value with the return type.
    tagged_returns: HashSet<String>,
    /// The functions whose parameters only plain assignments write, so a
    /// write-back hands back a box that carries its parameter's tag.
    tagged_writebacks: HashSet<String>,
}

impl ProgramShapes {
    /// `value_functions` names the functions the module uses as values,
    /// which the runtime may call with boxes it made.
    pub(super) fn new<'a>(
        module: &MirModule,
        value_functions: impl IntoIterator<Item = &'a str>,
    ) -> Self {
        let value_functions = value_functions.into_iter().collect::<HashSet<_>>();
        let mut shapes = Self::default();
        for function in &module.functions {
            let name = function.name.as_str();
            if function.receiver.is_none()
                && !value_functions.contains(name)
                && !matches!(name, "main" | "__script")
                && crate::mir::display_wrapper_type_name(name).is_none()
            {
                shapes.direct_only.insert(function.name.clone());
            }
            let instructions = || {
                function
                    .blocks
                    .iter()
                    .flat_map(|block| block.instructions.iter())
            };
            if !instructions().any(|instruction| {
                matches!(
                    instruction,
                    Instruction::Assign {
                        value: Rvalue::Try { .. },
                        ..
                    } | Instruction::WriteLoan {
                        value: Rvalue::Try { .. },
                        ..
                    }
                )
            }) {
                shapes.tagged_returns.insert(function.name.clone());
                let is_param = |place: &str| {
                    function
                        .params
                        .iter()
                        .any(|param| param.name == root(place))
                };
                if !instructions().any(|instruction| match instruction {
                    Instruction::ReadLoan { target, .. } => is_param(target),
                    Instruction::ReturnLoan { loan, origin } => is_param(loan) || is_param(origin),
                    _ => false,
                }) {
                    shapes.tagged_writebacks.insert(function.name.clone());
                }
            }
        }
        shapes
    }

    /// Whether a call of `function` returns a box that carries its return
    /// type's tag.
    pub(super) fn returns_tagged(&self, function: &str) -> bool {
        self.tagged_returns.contains(function)
    }

    /// Whether a call of `function` writes back boxes that carry their
    /// parameters' tags: each is the box the call passed, which the caller
    /// tagged, or one a plain assignment stored in the parameter.
    pub(super) fn writes_back_tagged(&self, function: &str) -> bool {
        self.tagged_writebacks.contains(function)
    }
}

/// Whether a shaped box of type `ty` keeps its tag. A `float32` list
/// narrows its elements when tagged, so its tag stays.
pub(super) fn keeps_tag(ty: &aura_core::types::Type) -> bool {
    !crate::native_runtime::canonical_runtime_type_name(ty).contains("float32")
}

/// The root local `place` names.
fn root(place: &str) -> &str {
    place.split('.').next().unwrap_or(place)
}

/// The locals of `function` whose box always carries the tag of the
/// local's type. `special` names the locals the emitter writes in a form of
/// its own: lent elements, lent lookup results, and lookup slots.
pub(super) fn shaped_locals(
    function: &MirFunction,
    program: &ProgramShapes,
    special: impl Fn(&str) -> bool,
) -> HashSet<String> {
    let mut written = HashSet::<&str>::new();
    let mut excluded = HashSet::<&str>::new();
    let mut lends = Vec::<(&str, &str)>::new();
    let direct_only = program.direct_only.contains(&function.name);
    for param in &function.params {
        if direct_only {
            written.insert(&param.name);
        } else {
            excluded.insert(&param.name);
        }
    }
    if function.receiver.is_some() {
        excluded.insert("self");
    }
    for block in &function.blocks {
        for instruction in &block.instructions {
            match instruction {
                Instruction::Assign { target, value } => {
                    exclude_rvalue_writes(value, &mut excluded);
                    if target.contains('.') {
                        continue;
                    }
                    if matches!(value, Rvalue::Try { .. }) || special(target) {
                        excluded.insert(target);
                    } else {
                        written.insert(target);
                    }
                }
                Instruction::WriteLoan { loan, value } => {
                    exclude_rvalue_writes(value, &mut excluded);
                    excluded.insert(root(loan));
                }
                Instruction::ReadLoan { target, .. } => {
                    excluded.insert(root(target));
                }
                Instruction::Lend {
                    place,
                    temp,
                    mutable,
                }
                | Instruction::EndLend {
                    place,
                    temp,
                    mutable,
                } => {
                    if *mutable {
                        excluded.insert(root(place));
                        excluded.insert(temp);
                    } else if place.contains('.') {
                        // The temporary holds a field's box, which no
                        // assignment here tagged.
                        excluded.insert(temp);
                    } else if matches!(instruction, Instruction::Lend { .. }) {
                        lends.push((place, temp));
                        written.insert(temp);
                    }
                }
                Instruction::BeginLoan { loan, .. }
                | Instruction::BeginElementLoan { loan, .. }
                | Instruction::BeginReturnedLoan { loan, .. }
                | Instruction::Reborrow { loan, .. }
                | Instruction::ReborrowElement { loan, .. } => {
                    excluded.insert(loan);
                }
                Instruction::ReturnLoan { loan, origin } => {
                    excluded.insert(root(loan));
                    excluded.insert(root(origin));
                }
                Instruction::Safepoint
                | Instruction::Retain { .. }
                | Instruction::Release { .. }
                | Instruction::IndexInRange { .. }
                | Instruction::EndLoan { .. }
                | Instruction::Eval { .. }
                | Instruction::PushCleanup { .. }
                | Instruction::PopCleanup { .. } => {}
            }
        }
        if let Terminator::ForRange { binding, .. } = &block.terminator {
            excluded.insert(root(binding));
        }
    }
    let mut shaped = written
        .difference(&excluded)
        .copied()
        .collect::<HashSet<_>>();
    // A temporary a lend fills holds the lent place's box, so it is shaped
    // only while the place is.
    let mut changed = true;
    while changed {
        changed = false;
        for (place, temp) in &lends {
            if !shaped.contains(*place) && shaped.remove(*temp) {
                changed = true;
            }
        }
    }
    shaped.into_iter().map(str::to_string).collect()
}

/// Leaves out the locals `value` lets a closure write: its captures. A
/// write-back argument stays in, since storing what the callee hands back
/// tags it.
fn exclude_rvalue_writes<'a>(value: &'a Rvalue, excluded: &mut HashSet<&'a str>) {
    let Rvalue::Closure { captures, .. } = value else {
        return;
    };
    for capture in captures {
        if let Operand::Place(place) | Operand::MovePlace(place) = &capture.value {
            excluded.insert(root(place));
        }
        if let Some(place) = &capture.source_place {
            excluded.insert(root(place));
        }
    }
}
