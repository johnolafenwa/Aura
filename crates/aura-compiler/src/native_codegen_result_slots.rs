//! `Result` locals that native code keeps as words, and the functions that
//! return a `Result` as words.
//!
//! A function that builds a `Result` and hands it straight to a caller that
//! matches on it, or unwraps it with `try`, would otherwise make a box for
//! the `Result` only for the caller to read and free it. A **result slot**
//! is a local of a `Result` type that is only ever built, moved, matched,
//! unwrapped and released. Native code keeps it as a tag word (0 for `Ok`)
//! and the payload of each variant in words of its own: a scalar, or the
//! payload's box. The payload words of the variant the slot does not hold
//! are zero.
//!
//! A function whose every `return` gives back a result slot gets a second
//! body, its **twin**, that returns the tag and the payload words instead
//! of a box. A call whose result goes into a result slot enters the twin.
//! Every other caller, the runtime included, enters the function's own
//! symbol, which builds the box from the slot when it returns.
//!
//! A payload box in a slot is a box the frame owns, like the box of any
//! local, so the task's ledger frees it when the frame traps.

use std::collections::{HashMap, HashSet};

use crate::mir::{
    instruction_place_mentions, terminator_place_mentions, CallTarget, Instruction, MethodDispatch,
    MirFunction, MirModule, MirOwnership, Operand, Rvalue, Terminator,
};

use super::*;

/// The result slots of each function and the functions that have a twin.
#[derive(Default)]
pub(super) struct ResultSlotPlan {
    /// The result slots of each function that has any, in name order.
    pub(super) slots: HashMap<String, Vec<String>>,
    /// The functions that get a twin.
    pub(super) twins: HashSet<String>,
}

/// One result slot: where its tag and its payloads are kept.
#[derive(Clone, Debug)]
pub(super) struct ResultSlot {
    /// 0 while the slot holds `Ok`, 1 while it holds `Err`.
    tag: Var,
    /// The root that holds the `Ok` payload.
    ok: String,
    /// The root that holds the `Err` payload.
    err: String,
}

/// The direct types of the payloads of the `Result` type `ty` when a slot
/// can hold them: each a scalar, `None`, or one box.
pub(super) fn slot_parts(
    ty: &Type,
    classes: &HashMap<String, MirClass>,
) -> Option<(DirectType, DirectType)> {
    let Type::Named(name, args) = ty else {
        return None;
    };
    let [ok, err] = args.as_slice() else {
        return None;
    };
    if name != "Result" || aura_core::layout::names_type_param(ty) {
        return None;
    }
    let part = |ty: &Type| {
        direct_type(ty, classes)
            .filter(|direct| matches!(direct, DirectType::Scalar(_) | DirectType::Opaque(_)))
    };
    Some((part(ok)?, part(err)?))
}

/// The words of a payload of type `ty` in a twin's results: none for
/// `None`.
fn part_abi(ty: &DirectType) -> Vec<ScalarType> {
    if matches!(ty, DirectType::Scalar(ScalarKind::Unit)) {
        Vec::new()
    } else {
        ty.abi_types()
    }
}

/// Whether the two payloads share one result word, which holds the payload
/// of the variant the tag names.
fn shares_word(ok: &[ScalarType], err: &[ScalarType]) -> bool {
    ok == [types::I64] && err == [types::I64]
}

/// The results of a twin that returns a `Result` with these payloads: the
/// tag, then the payload words.
pub(super) fn twin_results(ok: &DirectType, err: &DirectType) -> Vec<ScalarType> {
    let (ok, err) = (part_abi(ok), part_abi(err));
    let mut results = vec![types::I64];
    if shares_word(&ok, &err) {
        results.push(types::I64);
    } else {
        results.extend(ok);
        results.extend(err);
    }
    results
}

/// The function a call runs when the call names it.
fn static_callee(callee: &CallTarget) -> Option<&str> {
    match callee {
        CallTarget::Name(name) => Some(name),
        CallTarget::Member {
            dispatch: Some(MethodDispatch::Static { function }),
            ..
        } => Some(function),
        _ => None,
    }
}

fn root_of(place: &str) -> &str {
    place.split('.').next().unwrap_or(place)
}

/// The result slots of `function`, and whether every `return` gives one
/// back. `holds` says whether a local may be a slot, `copies` whether a
/// `Result` type owns no box, so that a read of it is as good as a move,
/// and `returns` gives the return type of a function of the program.
fn function_slots<'a>(
    function: &'a MirFunction,
    holds: impl Fn(&str, &Type) -> bool,
    copies: impl Fn(&Type) -> bool,
    returns: impl Fn(&str) -> Option<&'a Type>,
) -> (HashSet<&'a str>, bool) {
    let params = function
        .params
        .iter()
        .map(|param| param.name.as_str())
        .collect::<HashSet<_>>();
    let mut candidates = function
        .local_types
        .iter()
        .filter(|local| {
            local.name != "self"
                && !params.contains(local.name.as_str())
                && holds(&local.name, &local.ty)
        })
        .map(|local| (local.name.as_str(), &local.ty))
        .collect::<HashMap<_, _>>();
    if candidates.is_empty() {
        return (HashSet::new(), false);
    }
    // Whether `operand` gives the whole of candidate `place` away: a move,
    // or a read of a `Result` that owns no box.
    let gives = |operand: &'a Operand| match operand {
        Operand::MovePlace(place) => Some(place.as_str()),
        Operand::Place(place) => candidates
            .get(place.as_str())
            .is_some_and(|ty| copies(ty))
            .then_some(place.as_str()),
        _ => None,
    };
    let mut rejected = HashSet::<&str>::new();
    let mut moves = Vec::<(&str, &str)>::new();
    let reject_mentions = |mentions: Vec<&'a str>, rejected: &mut HashSet<&'a str>| {
        for place in mentions {
            let root = root_of(place);
            if candidates.contains_key(root) {
                rejected.insert(root);
            }
        }
    };
    for block in &function.blocks {
        for instruction in &block.instructions {
            match instruction {
                Instruction::Release { place } if candidates.contains_key(place.as_str()) => {
                    continue
                }
                Instruction::Eval {
                    value: Operand::Place(place) | Operand::MovePlace(place),
                } if candidates.contains_key(place.as_str()) => continue,
                Instruction::Assign { target, value } => {
                    if let Some(ty) = candidates.get(target.as_str()) {
                        let built = match value {
                            Rvalue::EnumVariant {
                                enum_name,
                                variant_name,
                                payloads,
                            } => {
                                enum_name == "Result"
                                    && matches!(variant_name.as_str(), "Ok" | "Err")
                                    && payloads.len() == 1
                            }
                            Rvalue::Call {
                                callee, type_args, ..
                            } => {
                                type_args.is_empty()
                                    && static_callee(callee).and_then(&returns) == Some(*ty)
                            }
                            Rvalue::Use(source) => gives(source)
                                .is_some_and(|source| candidates.get(source) == Some(ty)),
                            _ => false,
                        };
                        if !built {
                            rejected.insert(target);
                        }
                    }
                    match value {
                        Rvalue::Use(source)
                            if gives(source)
                                .is_some_and(|source| candidates.contains_key(source)) =>
                        {
                            let source = gives(source).unwrap_or_default();
                            if candidates.contains_key(target.as_str()) {
                                moves.push((target, source));
                            } else {
                                rejected.insert(source);
                            }
                            continue;
                        }
                        Rvalue::VariantPayload {
                            scrutinee: Operand::Place(place) | Operand::MovePlace(place),
                            variant_name,
                            index: 0,
                        } if candidates.contains_key(place.as_str())
                            && matches!(variant_name.as_str(), "Ok" | "Err") =>
                        {
                            continue
                        }
                        Rvalue::Try {
                            value,
                            releases_on_error,
                            ..
                        } if gives(value).is_some_and(|place| candidates.contains_key(place)) => {
                            let released = releases_on_error.iter().map(String::as_str).collect();
                            reject_mentions(released, &mut rejected);
                            continue;
                        }
                        _ => {}
                    }
                }
                _ => {}
            }
            let mut mentions = Vec::new();
            instruction_place_mentions(instruction, &mut mentions);
            if let Instruction::ReadLoan { target, .. } = instruction {
                mentions.push(target);
            }
            reject_mentions(mentions, &mut rejected);
        }
        match &block.terminator {
            Terminator::Match {
                scrutinee: Operand::Place(place),
                arms,
                ..
            } if candidates.contains_key(place.as_str()) => {
                let tests_tag = arms.iter().all(|arm| {
                    arm.wildcard
                        || (matches!(arm.enum_name.as_deref(), None | Some("Result"))
                            && matches!(arm.variant_name.as_deref(), Some("Ok" | "Err")))
                });
                if !tests_tag {
                    rejected.insert(place);
                }
            }
            Terminator::Return(operand)
                if gives(operand)
                    .is_some_and(|place| candidates.get(place) == Some(&&function.return_type)) => {
            }
            terminator => reject_mentions(terminator_place_mentions(terminator), &mut rejected),
        }
    }
    // Both ends of a move are slots, or neither is.
    let mut grew = true;
    while grew {
        grew = false;
        for (target, source) in &moves {
            if rejected.contains(target) != rejected.contains(source) {
                grew |= rejected.insert(target) | rejected.insert(source);
            }
        }
    }
    candidates.retain(|name, _| !rejected.contains(name));
    let returns_slots = function.blocks.iter().all(|block| match &block.terminator {
        Terminator::Return(Operand::MovePlace(place) | Operand::Place(place)) => {
            candidates.contains_key(place.as_str())
        }
        // The fall-through of a match that covers every variant: nothing
        // reaches it.
        Terminator::Return(Operand::Unit) => true,
        Terminator::Return(_) => false,
        _ => true,
    });
    (candidates.into_keys().collect(), returns_slots)
}

/// The result slots and twins of `module`. A module that does not state
/// ownership has none: its backends decide when a value is released, and a
/// slot follows the releases the module states.
pub(super) fn plan(
    module: &MirModule,
    classes: &HashMap<String, MirClass>,
    explicit_locals: &HashMap<String, crate::mir::ownership::ExplicitLocals>,
) -> ResultSlotPlan {
    let mut plan = ResultSlotPlan::default();
    if module.ownership != MirOwnership::Explicit {
        return plan;
    }
    let return_types = module
        .functions
        .iter()
        .map(|function| (function.name.as_str(), &function.return_type))
        .collect::<HashMap<_, _>>();
    let mut called = HashSet::new();
    let mut candidates = Vec::new();
    for function in &module.functions {
        let Some(locals) = explicit_locals.get(&function.name) else {
            continue;
        };
        let (slots, returns_slots) = function_slots(
            function,
            |name, ty| {
                slot_parts(ty, classes).is_some_and(|(ok, err)| {
                    // A box in a slot is released where the module says.
                    locals.tracked.contains(name)
                        || !(layout_owns_handles(&ok) || layout_owns_handles(&err))
                })
            },
            |ty| {
                slot_parts(ty, classes).is_some_and(|(ok, err)| {
                    !(layout_owns_handles(&ok) || layout_owns_handles(&err))
                })
            },
            |name| return_types.get(name).copied(),
        );
        if slots.is_empty() {
            continue;
        }
        for instruction in function.blocks.iter().flat_map(|block| &block.instructions) {
            if let Instruction::Assign {
                target,
                value: Rvalue::Call { callee, .. },
            } = instruction
            {
                if slots.contains(target.as_str()) {
                    called.extend(static_callee(callee));
                }
            }
        }
        if returns_slots {
            candidates.push(function.name.as_str());
        }
        let mut slots = slots.into_iter().map(str::to_string).collect::<Vec<_>>();
        slots.sort();
        plan.slots.insert(function.name.clone(), slots);
    }
    plan.twins = candidates
        .into_iter()
        .filter(|name| called.contains(name))
        .map(str::to_string)
        .collect();
    plan
}

/// Declares the storage of the result slots `names` of a function: for
/// each, a tag and a root for each payload. The roots are tracked, since a
/// slot releases them where the module releases the slot.
pub(super) fn declare_slots(
    builder: &mut dyn FunctionBackend,
    names: &[String],
    classes: &HashMap<String, MirClass>,
    variable_index: &mut usize,
    variables: &mut HashMap<String, Vec<Var>>,
    variable_types: &mut HashMap<String, DirectType>,
    tracked: &mut HashSet<String>,
) -> HashMap<String, ResultSlot> {
    let mut slots = HashMap::new();
    for name in names {
        let Some((ok_ty, err_ty)) = variable_types
            .get(name)
            .and_then(|ty| slot_parts(&direct_type_to_type(ty), classes))
        else {
            continue;
        };
        let tag = Var(*variable_index as u32);
        *variable_index += 1;
        builder.declare_var(tag, types::I64);
        let zero = builder.iconst(types::I64, 0);
        builder.def_var(tag, zero);
        let ok = format!("{name}%ok");
        let err = format!("{name}%err");
        for (root, ty) in [(&ok, ok_ty), (&err, err_ty)] {
            declare_root_variables(
                builder,
                variable_index,
                variables,
                variable_types,
                root.clone(),
                ty,
                None,
            );
            tracked.insert(root.clone());
        }
        slots.insert(name.clone(), ResultSlot { tag, ok, err });
    }
    slots
}

impl FunctionCompiler<'_> {
    /// The result slot `operand` names.
    fn result_slot_of(&self, operand: &Operand) -> Option<ResultSlot> {
        match operand {
            Operand::Place(place) | Operand::MovePlace(place) => {
                self.result_slots.get(place.as_str()).cloned()
            }
            _ => None,
        }
    }

    /// The words a value of a payload's type goes into a slot as: the
    /// scalar, or a box the slot owns.
    fn result_part_words(&mut self, value: ValueRef) -> std::result::Result<Vec<Val>, String> {
        if !matches!(value.ty, DirectType::Opaque(_)) {
            return Ok(value.values);
        }
        let boxed = self.ensure_opaque(value)?;
        Ok(vec![self.transfer_owned_opaque_value(&boxed)])
    }

    /// A payload of a slot, from the root that holds it: taken when the
    /// read `moves` the slot. A slot that is read whole and not moved owns
    /// no box, so its words are the whole of the payload.
    fn read_result_part(
        &mut self,
        root: &str,
        moves: bool,
    ) -> std::result::Result<ValueRef, String> {
        if moves {
            self.take_place(root)
        } else {
            self.load_root(root)
        }
    }

    /// Makes `slot`, which is empty, hold the variant `ok` names with the
    /// payload `words`.
    fn fill_result_slot(
        &mut self,
        slot: &ResultSlot,
        ok: bool,
        words: Vec<Val>,
    ) -> std::result::Result<(), String> {
        let (held, other) = if ok {
            (&slot.ok, &slot.err)
        } else {
            (&slot.err, &slot.ok)
        };
        for (var, word) in self.local_vars(held)?.into_iter().zip(words) {
            self.builder.def_var(var, word);
        }
        let zeros = self.local_type(other)?.zero_values(&mut *self.builder);
        for (var, zero) in self.local_vars(other)?.into_iter().zip(zeros) {
            self.builder.def_var(var, zero);
        }
        let tag = self.builder.iconst(types::I64, i64::from(!ok));
        self.builder.def_var(slot.tag, tag);
        Ok(())
    }

    /// Whether `slot` holds `Ok`, as a flag.
    fn result_slot_is_ok(&mut self, slot: &ResultSlot) -> Val {
        let tag = self.builder.use_var(slot.tag);
        self.builder.icmp_imm(IntCC::Equal, tag, 0)
    }

    /// Assigns to result slot `target`: a variant built in place, a move
    /// from another slot, or the result of a call, which comes back as
    /// words from a twin and as a box, read out here, from any other
    /// function.
    pub(super) fn compile_result_slot_assign(
        &mut self,
        target: &str,
        value: &Rvalue,
    ) -> std::result::Result<(), String> {
        let slot = self.result_slots[target].clone();
        match value {
            Rvalue::EnumVariant {
                variant_name,
                payloads,
                ..
            } => {
                let ok = variant_name == "Ok";
                let ty = self.local_type(if ok { &slot.ok } else { &slot.err })?;
                let loaded = self.load_operand_for_target(&payloads[0], &ty)?;
                let coerced = self.coerce_value(loaded, &ty)?;
                let words = self.result_part_words(coerced)?;
                self.fill_result_slot(&slot, ok, words)
            }
            Rvalue::Use(operand) => {
                let moves = matches!(operand, Operand::MovePlace(_));
                let Some(source) = self.result_slot_of(operand) else {
                    return Err(format!(
                        "direct backend expected `{target}` to take a result slot"
                    ));
                };
                let tag = self.builder.use_var(source.tag);
                self.builder.def_var(slot.tag, tag);
                for (from, to) in [(&source.ok, &slot.ok), (&source.err, &slot.err)] {
                    let moved = self.read_result_part(from, moves)?;
                    let words = self.transfer_values(&moved);
                    for (var, word) in self.local_vars(to)?.into_iter().zip(words) {
                        self.builder.def_var(var, word);
                    }
                }
                Ok(())
            }
            Rvalue::Call { callee, .. } => {
                let ty = self.type_of_place(target)?;
                self.result_twin_request = static_callee(callee)
                    .filter(|name| self.result_twin_refs.contains_key(*name))
                    .map(str::to_string);
                let compiled = self.compile_rvalue_for_target(value, &ty);
                self.result_twin_request = None;
                let words = self.result_twin_words.take();
                let compiled = compiled?;
                match words {
                    Some(words) => {
                        // The call's own result is the null the twin's
                        // words went in place of.
                        self.clear_temporary_opaque_owned(&compiled);
                        self.fill_result_slot_from_twin(&slot, &words)
                    }
                    None => self.unbox_into_result_slot(&slot, compiled),
                }
            }
            _ => Err(format!(
                "direct backend cannot keep `{target}` as a result slot"
            )),
        }
    }

    /// Makes `slot` hold the `Result` a twin returned as `words`.
    fn fill_result_slot_from_twin(
        &mut self,
        slot: &ResultSlot,
        words: &[Val],
    ) -> std::result::Result<(), String> {
        let ok_ty = self.local_type(&slot.ok)?;
        let err_ty = self.local_type(&slot.err)?;
        let (ok_abi, err_abi) = (part_abi(&ok_ty), part_abi(&err_ty));
        let tag = words[0];
        self.builder.def_var(slot.tag, tag);
        let (ok_words, err_words) = if shares_word(&ok_abi, &err_abi) {
            let is_ok = self.builder.icmp_imm(IntCC::Equal, tag, 0);
            let zero = self.builder.iconst(types::I64, 0);
            (
                vec![self.builder.select(is_ok, words[1], zero)],
                vec![self.builder.select(is_ok, zero, words[1])],
            )
        } else {
            let (ok_words, err_words) = words[1..].split_at(ok_abi.len());
            (ok_words.to_vec(), err_words.to_vec())
        };
        for (root, ty, words) in [(&slot.ok, ok_ty, ok_words), (&slot.err, err_ty, err_words)] {
            // `None` travels in no word.
            let words = if words.is_empty() {
                ty.zero_values(&mut *self.builder)
            } else {
                words
            };
            for (var, word) in self.local_vars(root)?.into_iter().zip(words) {
                self.builder.def_var(var, word);
            }
        }
        Ok(())
    }

    /// The words a twin returns for the `Result` `slot` holds. The slot's
    /// boxes go with them.
    fn result_slot_twin_words(
        &mut self,
        slot: &ResultSlot,
    ) -> std::result::Result<Vec<Val>, String> {
        let tag = self.builder.use_var(slot.tag);
        let ok = self.take_place(&slot.ok)?;
        let err = self.take_place(&slot.err)?;
        let (ok_abi, err_abi) = (part_abi(&ok.ty), part_abi(&err.ty));
        let ok_words = self.transfer_values(&ok);
        let err_words = self.transfer_values(&err);
        let mut words = vec![tag];
        if shares_word(&ok_abi, &err_abi) {
            let is_ok = self.builder.icmp_imm(IntCC::Equal, tag, 0);
            words.push(self.builder.select(is_ok, ok_words[0], err_words[0]));
        } else {
            if !ok_abi.is_empty() {
                words.extend(ok_words);
            }
            if !err_abi.is_empty() {
                words.extend(err_words);
            }
        }
        Ok(words)
    }

    /// The body a call of `name` enters: its twin, when the call's result
    /// goes into a result slot.
    pub(super) fn result_twin_callee(&mut self, name: &str, callee: FuncRef) -> FuncRef {
        if self.result_twin_request.as_deref() != Some(name) {
            return callee;
        }
        match self.result_twin_refs.get(name) {
            Some(twin) => {
                self.result_twin_called = true;
                *twin
            }
            None => callee,
        }
    }

    /// Takes the words of the `Result` out of the results of a call that
    /// entered the twin of `name`, and leaves a null where the function's
    /// own symbol returns the `Result`'s box. Any other call's results stay
    /// as they are.
    pub(super) fn take_result_twin_words(&mut self, name: &str, mut results: Vec<Val>) -> Vec<Val> {
        if !std::mem::take(&mut self.result_twin_called) {
            return results;
        }
        let parts = self
            .function_return_types
            .get(name)
            .and_then(|ty| slot_parts(&direct_type_to_type(ty), &self.classes));
        let (ok, err) = parts.expect("a twin returns a `Result` a slot can hold");
        let count = twin_results(&ok, &err).len();
        let rest = results.split_off(count);
        self.result_twin_words = Some(results);
        let mut results = vec![self.builder.iconst(types::I64, 0)];
        results.extend(rest);
        results
    }

    /// Reads the boxed `Result` `loaded` into `slot`: the payload of the
    /// variant it holds moves out of an owned box, or is copied from a
    /// borrowed one, and an owned box is released.
    fn unbox_into_result_slot(
        &mut self,
        slot: &ResultSlot,
        loaded: ValueRef,
    ) -> std::result::Result<(), String> {
        let value = self.ensure_opaque(loaded)?;
        let owned = self.temporary_owns_opaque(&value);
        if owned {
            // Each branch below releases the box itself.
            self.clear_temporary_opaque_owned(&value);
        }
        let object = value.values[0];
        let is_ok = match self.typed_variant_test(object, &value.ty, "Ok", |this| {
            this.variant_matches_value(object, "Result", "Ok")
        })? {
            Some(is_ok) => is_ok,
            None => self.variant_matches_value(object, "Result", "Ok")?,
        };
        let ok_block = self.builder.create_block();
        let err_block = self.builder.create_block();
        let join_block = self.builder.create_block();
        self.builder.brif(is_ok, ok_block, &[], err_block, &[]);
        for (block, ok) in [(ok_block, true), (err_block, false)] {
            self.builder.switch_to_block(block);
            self.builder.seal_block(block);
            if owned {
                self.mark_temporary_opaque_owned(&value);
            }
            let (root, variant) = if ok {
                (&slot.ok, "Ok")
            } else {
                (&slot.err, "Err")
            };
            let ty = self.local_type(root)?;
            let payload = self.compile_try_payload(&value, variant, &ty, owned)?;
            if owned {
                self.clear_temporary_opaque_owned(&value);
                self.release_opaque_handle(object);
            }
            let coerced = self.coerce_value(payload, &ty)?;
            let words = self.result_part_words(coerced)?;
            self.fill_result_slot(slot, ok, words)?;
            self.release_all_temporary_owned();
            self.builder.jump(join_block, &[]);
        }
        self.builder.switch_to_block(join_block);
        self.builder.seal_block(join_block);
        Ok(())
    }

    /// The payload of variant `variant_name` of the result slot `scrutinee`
    /// names: the slot's own when the read moves the slot, and otherwise a
    /// scalar or the payload's box, which the reader shares. `None` when
    /// `scrutinee` names no result slot.
    pub(super) fn compile_result_slot_payload(
        &mut self,
        scrutinee: &Operand,
        variant_name: &str,
    ) -> std::result::Result<Option<ValueRef>, String> {
        let Some(slot) = self.result_slot_of(scrutinee) else {
            return Ok(None);
        };
        let root = if variant_name == "Ok" {
            &slot.ok
        } else {
            &slot.err
        };
        let payload = match scrutinee {
            Operand::MovePlace(_) => self.take_place(root)?,
            _ => self.load_root(root)?,
        };
        // The payload took its type's tag when it went into the slot.
        if let (DirectType::Opaque(_), [word]) = (&payload.ty, payload.values.as_slice()) {
            self.shaped_boxes.insert(*word);
        }
        Ok(Some(payload))
    }

    /// Releases the payload boxes result slot `place` holds and leaves it
    /// empty. Returns false when `place` is no result slot.
    pub(super) fn release_result_slot(&mut self, place: &str) -> std::result::Result<bool, String> {
        let Some(slot) = self.result_slots.get(place).cloned() else {
            return Ok(false);
        };
        for root in [&slot.ok, &slot.err] {
            if !layout_owns_handles(&self.local_type(root)?) {
                continue;
            }
            // Only one of the two holds a box, and which one is known only
            // when the program runs.
            let var = self.local_vars(root)?[0];
            let held = self.builder.use_var(var);
            let release_block = self.builder.create_block();
            let done_block = self.builder.create_block();
            self.builder.brif(held, release_block, &[], done_block, &[]);
            self.builder.switch_to_block(release_block);
            self.builder.seal_block(release_block);
            self.release_opaque_handle(held);
            self.builder.jump(done_block, &[]);
            self.builder.switch_to_block(done_block);
            self.builder.seal_block(done_block);
            let null = self.builder.iconst(types::I64, 0);
            self.builder.def_var(var, null);
        }
        Ok(true)
    }

    /// The `try` of a result slot: `Ok`'s payload moves into `target`, and
    /// `Err`'s payload returns from the function. Returns false when
    /// `try_value` names no result slot.
    pub(super) fn compile_result_slot_try(
        &mut self,
        target: &str,
        target_ty: &DirectType,
        try_value: &Operand,
        releases_on_error: &[String],
        from: Option<&str>,
    ) -> std::result::Result<bool, String> {
        let Some(slot) = self.result_slot_of(try_value) else {
            return Ok(false);
        };
        let (Operand::MovePlace(source) | Operand::Place(source)) = try_value else {
            return Ok(false);
        };
        let moves = matches!(try_value, Operand::MovePlace(_));
        let source_ty = self.type_of_place(source)?;
        let (source_error_ty, target_error_ty) = self.try_error_types(target, &source_ty)?;
        let is_ok = self.result_slot_is_ok(&slot);
        let ok_block = self.builder.create_block();
        let err_block = self.builder.create_block();
        let join_block = self.builder.create_block();
        self.builder.brif(is_ok, ok_block, &[], err_block, &[]);

        self.builder.switch_to_block(ok_block);
        self.builder.seal_block(ok_block);
        let payload = self.read_result_part(&slot.ok, moves)?;
        if let (DirectType::Opaque(_), [word]) = (&payload.ty, payload.values.as_slice()) {
            self.shaped_boxes.insert(*word);
        }
        let coerced = self.coerce_value(payload, target_ty)?;
        self.store_place(target, coerced)?;
        self.release_all_temporary_owned();
        self.builder.jump(join_block, &[]);

        self.builder.switch_to_block(err_block);
        self.builder.seal_block(err_block);
        let error = self.read_result_part(&slot.err, moves)?;
        self.compile_result_error_return(
            error,
            &source_error_ty,
            &target_error_ty,
            releases_on_error,
            from,
        )?;

        self.builder.switch_to_block(join_block);
        self.builder.seal_block(join_block);
        Ok(true)
    }

    /// Returns `Result.Err(error)` from the function, with `error`
    /// converted when the function's error type differs: as words from a
    /// twin, and as a box from the function's own symbol.
    fn compile_result_error_return(
        &mut self,
        error: ValueRef,
        source_error_ty: &Type,
        target_error_ty: &Type,
        releases_on_error: &[String],
        from: Option<&str>,
    ) -> std::result::Result<(), String> {
        let error = if source_error_ty == target_error_ty {
            error
        } else {
            let from = from.ok_or_else(|| {
                format!(
                    "direct backend `try` names no `From[{source_error_ty}] for {target_error_ty}` conversion"
                )
            })?;
            self.convert_try_error_via_from(error, from, source_error_ty)?
        };
        let Some((ok_ty, err_ty)) = self.result_twin_body.clone() else {
            let return_type = self.return_type.clone();
            let built = match self.construct_variant_in_place_from_values(
                &return_type,
                "Err",
                std::slice::from_ref(&error),
            )? {
                Some(built) => built,
                None => self.compile_enum_variant_from_values(
                    "Result",
                    "Err",
                    vec![error],
                    return_type,
                )?,
            };
            return self.emit_return_value(built, releases_on_error);
        };
        let coerced = self.coerce_value(error, &err_ty)?;
        let err_words = self.result_part_words(coerced)?;
        let mut words = vec![self.builder.iconst(types::I64, 1)];
        if shares_word(&part_abi(&ok_ty), &part_abi(&err_ty)) {
            words.extend(err_words);
        } else {
            if !part_abi(&ok_ty).is_empty() {
                words.extend(ok_ty.zero_values(&mut *self.builder));
            }
            if !part_abi(&err_ty).is_empty() {
                words.extend(err_words);
            }
        }
        self.release_union_read_buffers();
        self.emit_return_words(words, releases_on_error)
    }

    /// The error return of a `try` in a twin whose operand is a boxed
    /// `Result`: the error moves out of the box and returns as words.
    pub(super) fn compile_twin_try_error_return(
        &mut self,
        value: &ValueRef,
        source_is_owned: bool,
        source_error_ty: &Type,
        target_error_ty: &Type,
        releases_on_error: &[String],
        from: Option<&str>,
    ) -> std::result::Result<(), String> {
        if source_is_owned {
            self.mark_temporary_opaque_owned(value);
        }
        let source_error = direct_type(source_error_ty, &self.classes)
            .unwrap_or_else(|| DirectType::Opaque(source_error_ty.clone()));
        let payload = self.compile_try_payload(value, "Err", &source_error, source_is_owned)?;
        if source_is_owned {
            self.clear_temporary_opaque_owned(value);
            self.release_opaque_handle(value.values[0]);
        }
        let payload = self.coerce_value(payload, &source_error)?;
        self.compile_result_error_return(
            payload,
            source_error_ty,
            target_error_ty,
            releases_on_error,
            from,
        )
    }

    /// A match on a result slot tests its tag. Returns false when
    /// `scrutinee` names no result slot.
    pub(super) fn compile_result_slot_match(
        &mut self,
        block_label: &str,
        scrutinee: &Operand,
        arms: &[crate::mir::MirMatchArm],
        otherwise: &str,
    ) -> std::result::Result<bool, String> {
        let Some(slot) = self.result_slot_of(scrutinee) else {
            return Ok(false);
        };
        self.release_all_temporary_owned();
        for arm in arms {
            let arm_block = self.compiled_block(&arm.label)?;
            if arm.wildcard {
                self.builder.jump(arm_block, &[]);
                return Ok(true);
            }
            let is_ok = self.result_slot_is_ok(&slot);
            let next_block = self.builder.create_block();
            if arm.variant_name.as_deref() == Some("Ok") {
                self.builder.brif(is_ok, arm_block, &[], next_block, &[]);
            } else {
                self.builder.brif(is_ok, next_block, &[], arm_block, &[]);
            }
            self.builder.switch_to_block(next_block);
            self.builder.seal_block(next_block);
        }
        if self.closed_matches.contains(block_label) {
            self.builder.trap(TrapCode::User(2));
        } else {
            let otherwise = self.compiled_block(otherwise)?;
            self.builder.jump(otherwise, &[]);
        }
        Ok(true)
    }

    /// Returns the `Result` a result slot holds: as words from a twin, and
    /// as a box, built here, from the function's own symbol. Returns false
    /// when `operand` names no result slot.
    pub(super) fn compile_result_slot_return(
        &mut self,
        operand: &Operand,
    ) -> std::result::Result<bool, String> {
        let Some(slot) = self.result_slot_of(operand) else {
            if self.result_twin_body.is_none() {
                return Ok(false);
            }
            // A twin returns only result slots, but for the fall-through
            // of a match that covers every variant, which nothing reaches.
            self.release_all_temporary_owned();
            self.builder.trap(TrapCode::User(2));
            return Ok(true);
        };
        self.flush_deferred_traps();
        self.release_unconsumed_retains();
        if self.result_twin_body.is_some() {
            let words = self.result_slot_twin_words(&slot)?;
            self.release_union_read_buffers();
            self.emit_return_words(words, &[])?;
            return Ok(true);
        }
        let return_type = self.return_type.clone();
        let is_ok = self.result_slot_is_ok(&slot);
        let ok_block = self.builder.create_block();
        let err_block = self.builder.create_block();
        let join_block = self.builder.create_block();
        self.builder.append_block_param(join_block, types::I64);
        self.builder.brif(is_ok, ok_block, &[], err_block, &[]);
        for (block, root, variant) in [(ok_block, &slot.ok, "Ok"), (err_block, &slot.err, "Err")] {
            self.builder.switch_to_block(block);
            self.builder.seal_block(block);
            let payload = self.take_place(root)?;
            let built = match self.construct_variant_in_place_from_values(
                &return_type,
                variant,
                std::slice::from_ref(&payload),
            )? {
                Some(built) => built,
                None => self.compile_enum_variant_from_values(
                    "Result",
                    variant,
                    vec![payload],
                    return_type.clone(),
                )?,
            };
            let handle = self.transfer_opaque_arg(&built);
            self.release_all_temporary_owned();
            self.builder.jump(join_block, &[handle]);
        }
        self.builder.switch_to_block(join_block);
        self.builder.seal_block(join_block);
        let handle = self.builder.block_params(join_block)[0];
        let built = self.owned_opaque_result(vec![handle], return_type);
        self.emit_return_value(built, &[])?;
        Ok(true)
    }
}

#[cfg(test)]
#[path = "native_codegen_result_slots_tests.rs"]
mod tests;
