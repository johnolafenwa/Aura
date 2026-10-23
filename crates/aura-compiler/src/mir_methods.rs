//! Resolved member calls, `try` conversions, and cleanup callees in MIR
//! (Batch 7 step 8h).
//!
//! `resolve_methods` names what each of these runs, from the types MIR
//! records:
//!
//! - every `Member` and `TraitMember` call gets its `MethodDispatch`: the
//!   method's function, an op or nothing on a built-in receiver, one arm
//!   for each member of a union receiver, or the candidates a runtime
//!   receiver type picks from, most specific first;
//! - every `Try` whose operand's error type is not the function's names the
//!   `from` method of the most specific `From` impl;
//! - every `PushCleanup` names its `CleanupCallee`: the class's `close` or
//!   `__exit__`, the resource's `close` op, a `TaskGroup` close, or nothing.
//!
//! `lower` runs the pass after the ownership pass, and the native boundary
//! runs it on an implicit module. `lower_implicit` skips it, so `aura run`
//! pays nothing: the interpreter reads the runtime value instead.
//!
//! `validate_methods` resolves each call in a reachable block again and
//! refuses one that names another callee. The native boundary also refuses
//! one that names none, so a call that resolves to nothing stops the build.
//! The interpreter reads the runtime value, so it accepts such a call when
//! it names no callee: a generic builtin body can call a method that no
//! type of the program declares.

use super::{
    collect_type_params_from_type, CallTarget, CleanupAction, CleanupCallee, Instruction,
    MethodCandidate, MethodDispatch, MirArg, MirClass, MirFunction, MirLoanValidationContext,
    MirModule, MirReceiverKind, Operand, Rvalue,
};
use crate::call::BuiltinMember;
use crate::runtime_ops::{ResourceOp, RuntimeOp};
use crate::sema::Type;
use std::collections::{BTreeSet, HashMap};

/// Names the callee of every member call, `try` conversion, and cleanup
/// that does not name one yet.
pub(crate) fn resolve_methods(module: &mut MirModule) {
    let resolved = {
        // A module the validator cannot read keeps its calls; the validator
        // reports what is wrong with it.
        let Ok(context) = MirLoanValidationContext::new(module) else {
            return;
        };
        let resolver = Resolver { context: &context };
        module
            .functions
            .iter()
            .chain(module.top_level.iter())
            .map(|function| resolver.function_callees(function))
            .collect::<Vec<_>>()
    };
    let functions = module
        .functions
        .iter_mut()
        .chain(module.top_level.iter_mut());
    for (function, callees) in functions.zip(resolved) {
        for ((block, index), callee) in callees {
            let instruction = &mut function.blocks[block].instructions[index];
            apply(instruction, callee);
        }
    }
}

/// Who runs a module that `validate_methods` checks.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum MethodsConsumer {
    /// The native emitter runs each callee MIR names, so every member
    /// call, conversion, and cleanup must name one.
    Native,
    /// The interpreter reads the runtime value, so a callee that resolves
    /// to nothing may stay unnamed.
    Interpreter,
}

/// Refuses a member call, `try` conversion, or cleanup whose callee is not
/// the one its types resolve to.
pub(crate) fn validate_methods(
    module: &MirModule,
    consumer: MethodsConsumer,
) -> std::result::Result<(), String> {
    let context = MirLoanValidationContext::new(module)?;
    let resolver = Resolver { context: &context };
    for function in module.functions.iter().chain(module.top_level.iter()) {
        for block in super::reachable_mir_blocks(function)? {
            for instruction in &block.instructions {
                resolver.validate_instruction(function, instruction, consumer)?;
            }
        }
    }
    Ok(())
}

/// Compares the callee an instruction names with the one it resolves to.
fn check_named<T: PartialEq>(
    named: Option<&T>,
    resolved: std::result::Result<T, String>,
    consumer: MethodsConsumer,
    unresolved: impl FnOnce(String) -> String,
    wrong: impl FnOnce() -> String,
    missing: impl FnOnce() -> String,
) -> std::result::Result<(), String> {
    match (named, resolved) {
        (Some(named), Ok(resolved)) if *named == resolved => Ok(()),
        (Some(_), Ok(_)) => Err(wrong()),
        (None, Ok(_)) => Err(missing()),
        (None, Err(_)) if consumer == MethodsConsumer::Interpreter => Ok(()),
        (_, Err(reason)) => Err(unresolved(reason)),
    }
}

/// A callee the pass records, with the instruction it belongs to.
enum Resolved {
    Dispatch(MethodDispatch),
    From(String),
    Cleanup(CleanupCallee),
}

fn apply(instruction: &mut Instruction, resolved: Resolved) {
    match (instruction, resolved) {
        (
            Instruction::Assign { value, .. } | Instruction::WriteLoan { value, .. },
            Resolved::Dispatch(resolved),
        ) => {
            if let Rvalue::Call {
                callee:
                    CallTarget::Member { dispatch, .. } | CallTarget::TraitMember { dispatch, .. },
                ..
            } = value
            {
                *dispatch = Some(resolved);
            }
        }
        (
            Instruction::Assign {
                value: Rvalue::Try { from, .. },
                ..
            },
            Resolved::From(function),
        ) => {
            *from = Some(function);
        }
        (Instruction::PushCleanup { callee, .. }, Resolved::Cleanup(resolved)) => {
            *callee = Some(resolved);
        }
        _ => {}
    }
}

struct Resolver<'a, 'b> {
    context: &'a MirLoanValidationContext<'b>,
}

impl Resolver<'_, '_> {
    /// The callees `function` does not name yet, by block and instruction.
    /// A call that resolves to nothing keeps its gap, and the validator
    /// reports why.
    fn function_callees(&self, function: &MirFunction) -> Vec<((usize, usize), Resolved)> {
        let mut callees = Vec::new();
        for (block_index, block) in function.blocks.iter().enumerate() {
            for (index, instruction) in block.instructions.iter().enumerate() {
                let resolved = match instruction {
                    Instruction::Assign { value, .. } | Instruction::WriteLoan { value, .. } => {
                        match value {
                            Rvalue::Call {
                                callee:
                                    callee @ (CallTarget::Member { dispatch: None, .. }
                                    | CallTarget::TraitMember { dispatch: None, .. }),
                                args,
                                ..
                            } => self
                                .call_dispatch(function, callee, args)
                                .ok()
                                .map(Resolved::Dispatch),
                            Rvalue::Try {
                                value, from: None, ..
                            } => self
                                .try_from(function, value)
                                .ok()
                                .flatten()
                                .map(Resolved::From),
                            _ => None,
                        }
                    }
                    Instruction::PushCleanup {
                        place,
                        action,
                        callee: None,
                    } => self
                        .cleanup_callee(function, place, action)
                        .ok()
                        .map(Resolved::Cleanup),
                    _ => None,
                };
                if let Some(resolved) = resolved {
                    callees.push(((block_index, index), resolved));
                }
            }
        }
        callees
    }

    fn validate_instruction(
        &self,
        function: &MirFunction,
        instruction: &Instruction,
        consumer: MethodsConsumer,
    ) -> std::result::Result<(), String> {
        let name = &function.name;
        match instruction {
            Instruction::Assign { value, .. } | Instruction::WriteLoan { value, .. } => {
                match value {
                    Rvalue::Call {
                        callee:
                            callee @ (CallTarget::Member { dispatch, .. }
                            | CallTarget::TraitMember { dispatch, .. }),
                        args,
                        ..
                    } => {
                        let field = callee.member_parts().map_or("", |parts| parts.field);
                        check_named(
                            dispatch.as_ref(),
                            self.call_dispatch(function, callee, args),
                            consumer,
                            |reason| {
                                format!(
                                    "invalid MIR member call `.{field}` in `{name}` resolves to no method: {reason}"
                                )
                            },
                            || {
                                format!(
                                    "invalid MIR member call `.{field}` in `{name}` names a dispatch its receiver does not resolve to"
                                )
                            },
                            || format!("invalid MIR member call `.{field}` in `{name}` names no dispatch"),
                        )
                    }
                    Rvalue::Try { value, from, .. } => match self.try_from(function, value) {
                        Ok(None) if from.is_none() => Ok(()),
                        Ok(None) => Err(format!(
                            "invalid MIR `try` in `{name}` names a `From` conversion between equal error types"
                        )),
                        resolved => check_named(
                            from.as_ref(),
                            resolved.map(|from| from.unwrap_or_default()),
                            consumer,
                            |reason| format!("invalid MIR `try` in `{name}`: {reason}"),
                            || {
                                format!(
                                    "invalid MIR `try` in `{name}` names a `From` conversion its error types do not resolve to"
                                )
                            },
                            || format!("invalid MIR `try` in `{name}` names no `From` conversion"),
                        ),
                    },
                    _ => Ok(()),
                }
            }
            Instruction::PushCleanup {
                place,
                action,
                callee,
            } => check_named(
                callee.as_ref(),
                self.cleanup_callee(function, place, action),
                consumer,
                |reason| format!("invalid MIR cleanup `{place}` in `{name}` resolves to no exit: {reason}"),
                || {
                    format!(
                        "invalid MIR cleanup `{place}` in `{name}` names a callee that is not its exit"
                    )
                },
                || format!("invalid MIR cleanup `{place}` in `{name}` names no callee"),
            ),
            _ => Ok(()),
        }
    }

    /// The type an operand loads with. An integer literal loads as an
    /// `int64`.
    fn operand_type(&self, function: &MirFunction, operand: &Operand) -> Option<Type> {
        match operand {
            Operand::Int(_) => Some(Type::named("int64")),
            other => self.context.operand_type(function, other).ok().flatten(),
        }
    }

    fn call_dispatch(
        &self,
        function: &MirFunction,
        callee: &CallTarget,
        args: &[MirArg],
    ) -> std::result::Result<MethodDispatch, String> {
        let Some(parts) = callee.member_parts() else {
            return Err("the call is not a member call".to_string());
        };
        let receiver = self
            .operand_type(function, parts.object)
            .ok_or("its receiver has no type")?;
        let call = MemberCall {
            field: parts.field,
            trait_name: parts.trait_name,
            args,
            writes_back: parts.receiver_place.is_some(),
        };
        self.dispatch(&receiver, &call)
    }

    /// What `receiver.field(args)` runs.
    fn dispatch(
        &self,
        receiver: &Type,
        call: &MemberCall<'_>,
    ) -> std::result::Result<MethodDispatch, String> {
        if let Some(members) = self.union_members(receiver) {
            if call.field == "clone" && call.trait_name.is_none() {
                return Ok(MethodDispatch::UnionClone);
            }
            let arms = members
                .iter()
                .map(|member| self.union_arm(member, call))
                .collect::<std::result::Result<Vec<_>, _>>()?;
            if !arms
                .iter()
                .any(|arm| matches!(arm, MethodDispatch::Static { .. }))
            {
                return Err(format!(
                    "no member of `{receiver}` declares `.{}`",
                    call.field
                ));
            }
            return Ok(MethodDispatch::Union(arms));
        }
        match receiver {
            Type::Function { .. } | Type::Closure { .. } | Type::Callable(_) => {
                Err(format!("`.{}` is called on a callable value", call.field))
            }
            Type::Named(name, _) if self.context.classes.contains_key(name.as_str()) => {
                self.class_method(name, receiver, call)
            }
            ty if is_scalar_type(ty) || *ty == Type::Unit => {
                if call.trait_name.is_none() {
                    if let Some(op) = RuntimeOp::resolve_member(ty, call.field, |_| false) {
                        return Ok(MethodDispatch::Runtime(op));
                    }
                }
                self.class_method(&ty.to_string(), ty, call)
            }
            other => self.opaque_dispatch(other, call),
        }
    }

    /// The members of a union receiver whose tag a call switches on, with
    /// each member that names a known class by its canonical key spelled as
    /// that class. A union that names a type parameter has no inline tag.
    fn union_members(&self, receiver: &Type) -> Option<Vec<Type>> {
        let Type::Union(union) = receiver else {
            return None;
        };
        let mut type_params = BTreeSet::new();
        collect_type_params_from_type(receiver, &mut type_params);
        if !type_params.is_empty() {
            return None;
        }
        Some(
            union
                .members
                .iter()
                .enumerate()
                .map(|(index, member)| {
                    union
                        .keys
                        .get(index)
                        .and_then(|key| canonical_named_type_from_key(key))
                        .filter(|candidate| {
                            matches!(candidate, Type::Named(name, _)
                                if self.context.classes.contains_key(name.as_str()))
                        })
                        .unwrap_or_else(|| member.clone())
                })
                .collect(),
        )
    }

    /// What the call runs on one member of a union receiver.
    fn union_arm(
        &self,
        member: &Type,
        call: &MemberCall<'_>,
    ) -> std::result::Result<MethodDispatch, String> {
        match member {
            Type::Named(name, _) if self.context.classes.contains_key(name.as_str()) => {
                self.class_method(name, member, call)
            }
            ty if is_scalar_type(ty)
                || matches!(
                    ty,
                    Type::Unit
                        | Type::Function { .. }
                        | Type::Closure { .. }
                        | Type::Callable(_)
                        | Type::Union(_)
                ) =>
            {
                Err(format!("member `{ty}` has no method `.{}`", call.field))
            }
            other => self.opaque_dispatch(other, call),
        }
    }

    /// What the call runs on a receiver of a built-in, generic, or erased
    /// type: a trait method the program implements for it, a runtime op,
    /// or the candidates the receiver's runtime type picks from.
    fn opaque_dispatch(
        &self,
        receiver: &Type,
        call: &MemberCall<'_>,
    ) -> std::result::Result<MethodDispatch, String> {
        if let Type::Named(name, _) = receiver {
            // A program's trait method on a built-in type runs, unless a
            // built-in member of that name shadows it.
            if BuiltinMember::resolve(name, call.field).is_none()
                && self.trait_method(receiver, None, call.field).is_some()
            {
                return self.class_method(name, receiver, call);
            }
        }
        if let Some(op) = RuntimeOp::resolve_member(receiver, call.field, |_| false) {
            return Ok(MethodDispatch::Runtime(super::specialized_member_op(
                op, receiver, call.args,
            )));
        }
        if let Type::Named(name, _) = receiver {
            // These handles own no host resource, so closing them does
            // nothing.
            if call.field == "close"
                && matches!(
                    name.as_str(),
                    "net.HttpExchange" | "net.HttpResponse" | "net.WebSocketListener"
                )
            {
                return Ok(MethodDispatch::Nothing);
            }
            let runtime_type = matches!(
                name.as_str(),
                "str" | "Array" | "list" | "dict" | "set" | "Queue" | "Task" | "TaskGroup"
            ) || ResourceOp::ALL
                .iter()
                .any(|op| op.receiver_type_name() == name);
            if runtime_type {
                return Err(format!("the runtime has no member `{name}.{}`", call.field));
            }
        }
        let candidates = self.candidate_types(call.field, call.trait_name);
        let erased = matches!(receiver, Type::TypeParam(_));
        if candidates.len() == 1 && !erased {
            let ty = &candidates[0];
            let Type::Named(name, _) = ty else {
                return Err(format!("candidate `{ty}` is not a named type"));
            };
            return self.class_method(name, ty, call);
        }
        if candidates.is_empty() {
            return Err(format!(
                "no type declares `.{}` for `{receiver}`",
                call.field
            ));
        }
        let candidates = candidates
            .into_iter()
            .map(|ty| {
                let Type::Named(name, _) = &ty else {
                    return Err(format!("candidate `{ty}` is not a named type"));
                };
                let MethodDispatch::Static { function } = self.class_method(name, &ty, call)?
                else {
                    unreachable!("a class method is a static call");
                };
                Ok(MethodCandidate { ty, function })
            })
            .collect::<std::result::Result<Vec<_>, _>>()?;
        Ok(MethodDispatch::Candidates { candidates, erased })
    }

    /// The method `class_name` runs for the call: its own method, or else
    /// the most specific trait method for `receiver` or the class. A call
    /// through a trait bound looks only at that trait. A `mut self` method
    /// needs a place to write its receiver back to.
    fn class_method(
        &self,
        class_name: &str,
        receiver: &Type,
        call: &MemberCall<'_>,
    ) -> std::result::Result<MethodDispatch, String> {
        let class_type = Type::named(class_name);
        let method = match call.trait_name {
            Some(trait_name) => self
                .trait_method(receiver, Some(trait_name), call.field)
                .or_else(|| self.trait_method(&class_type, Some(trait_name), call.field)),
            None => self
                .context
                .classes
                .get(class_name)
                .and_then(|class| class_method_named(class, call.field))
                .or_else(|| self.trait_method(receiver, None, call.field))
                .or_else(|| self.trait_method(&class_type, None, call.field)),
        }
        .ok_or_else(|| format!("`{class_name}` has no method `{}`", call.field))?;
        if method.receiver == Some(MirReceiverKind::BorrowMut) && !call.writes_back {
            return Err(format!(
                "`{class_name}.{}` takes `mut self`, and its receiver is a temporary",
                call.field
            ));
        }
        Ok(MethodDispatch::Static {
            function: method.function_name.clone(),
        })
    }

    /// The most specific trait impl method `field` for `ty`, through
    /// `trait_name` when given. Two equally specific impls resolve to none.
    fn trait_method(
        &self,
        ty: &Type,
        trait_name: Option<&str>,
        field: &str,
    ) -> Option<&super::MirMethod> {
        let mut best = None;
        let mut best_specificity = 0usize;
        let mut ambiguous = false;
        for trait_impl in self.context.trait_impls {
            if trait_name.is_some_and(|name| trait_impl.trait_name != name) {
                continue;
            }
            let mut type_params = BTreeSet::new();
            collect_type_params_from_type(&trait_impl.for_type, &mut type_params);
            let mut substitutions = HashMap::new();
            if !crate::sema::type_pattern_matches(
                &trait_impl.for_type,
                ty,
                &type_params,
                &mut substitutions,
            ) {
                continue;
            }
            let Some(method) = trait_impl
                .methods
                .iter()
                .find(|method| method.name == field)
            else {
                continue;
            };
            let specificity = crate::sema::trait_impl_specificity_parts(
                &trait_impl.for_type,
                &trait_impl.trait_args,
            );
            if best.is_none() || specificity > best_specificity {
                best = Some(method);
                best_specificity = specificity;
                ambiguous = false;
            } else if specificity == best_specificity {
                ambiguous = true;
            }
        }
        (!ambiguous).then_some(best).flatten()
    }

    /// Every type that declares `field`, most specific first: classes by
    /// name, then trait impls by specificity. A call through a trait bound
    /// looks only at that trait's impls.
    fn candidate_types(&self, field: &str, trait_name: Option<&str>) -> Vec<Type> {
        let mut candidates = Vec::new();
        if trait_name.is_none() {
            for class in self.context.classes.values() {
                if class_method_named(class, field).is_some() {
                    candidates.push((usize::MAX, Type::named(&class.name)));
                }
            }
        }
        for trait_impl in self.context.trait_impls {
            if trait_name.is_some_and(|name| trait_impl.trait_name != name) {
                continue;
            }
            if trait_impl.methods.iter().any(|method| method.name == field) {
                candidates.push((
                    crate::sema::trait_impl_specificity_parts(
                        &trait_impl.for_type,
                        &trait_impl.trait_args,
                    ),
                    trait_impl.for_type.clone(),
                ));
            }
        }
        candidates.sort_by_key(|candidate| std::cmp::Reverse(candidate.0));
        candidates.into_iter().map(|(_, ty)| ty).collect()
    }

    /// The `from` method `try value` converts its error with, or `None`
    /// when the error types agree.
    fn try_from(
        &self,
        function: &MirFunction,
        value: &Operand,
    ) -> std::result::Result<Option<String>, String> {
        let source = self.operand_type(function, value);
        let (Some(source_error), Some(target_error)) = (
            source.as_ref().and_then(result_error_type),
            result_error_type(&function.return_type),
        ) else {
            return Err("`try` needs a `Result` operand and a `Result` return".to_string());
        };
        if source_error == target_error {
            return Ok(None);
        }
        let method = self
            .context
            .trait_impls
            .iter()
            .filter(|trait_impl| {
                trait_impl.trait_name == "From" && trait_impl.trait_args.len() == 1
            })
            .filter_map(|trait_impl| {
                let mut type_params = BTreeSet::new();
                collect_type_params_from_type(&trait_impl.for_type, &mut type_params);
                for trait_arg in &trait_impl.trait_args {
                    collect_type_params_from_type(trait_arg, &mut type_params);
                }
                let mut substitutions = HashMap::new();
                if !crate::sema::type_pattern_matches(
                    &trait_impl.for_type,
                    target_error,
                    &type_params,
                    &mut substitutions,
                ) || crate::sema::substitute_type(&trait_impl.trait_args[0], &substitutions)
                    != *source_error
                {
                    return None;
                }
                let method = trait_impl
                    .methods
                    .iter()
                    .find(|method| method.name == "from")?;
                Some((
                    crate::sema::trait_impl_specificity_parts(
                        &trait_impl.for_type,
                        &trait_impl.trait_args,
                    ),
                    method,
                ))
            })
            .max_by_key(|(specificity, _)| *specificity)
            .map(|(_, method)| method.function_name.clone());
        method.map(Some).ok_or_else(|| {
            format!("no `From[{source_error}] for {target_error}` converts the error")
        })
    }

    /// What the cleanup of `place` runs: the class's `__exit__` for a
    /// manager or its `close`, or the built-in resource's close.
    fn cleanup_callee(
        &self,
        function: &MirFunction,
        place: &str,
        action: &CleanupAction,
    ) -> std::result::Result<CleanupCallee, String> {
        let ty = self
            .context
            .place_type(function, place)
            .ok()
            .flatten()
            .ok_or("the place has no type")?;
        let exit = match action {
            CleanupAction::ManagerExit { .. } => "__exit__",
            CleanupAction::ResourceClose => "close",
        };
        let Type::Named(name, _) = &ty else {
            return Err(format!("`{ty}` is not a class or a resource"));
        };
        if let Some(class) = self.context.classes.get(name.as_str()) {
            let method = class_method_named(class, exit)
                .ok_or_else(|| format!("class `{name}` has no `{exit}` method"))?;
            return Ok(CleanupCallee::Method {
                function: method.function_name.clone(),
            });
        }
        if matches!(action, CleanupAction::ManagerExit { .. }) {
            return Err(format!("`{name}` is not a class"));
        }
        if name == "TaskGroup" {
            return Ok(CleanupCallee::CloseTaskGroup);
        }
        if let Some(op) = RuntimeOp::resolve_member(&ty, "close", |_| false) {
            return Ok(CleanupCallee::Runtime(op));
        }
        if crate::sema::is_builtin_io_resource_type(name, &[]) {
            return Ok(CleanupCallee::Nothing);
        }
        Err(format!("`{ty}` is not a class or a resource"))
    }
}

/// One member call to resolve.
struct MemberCall<'a> {
    field: &'a str,
    trait_name: Option<&'a str>,
    args: &'a [MirArg],
    /// The call names a place its receiver writes back to.
    writes_back: bool,
}

fn class_method_named<'a>(class: &'a MirClass, field: &str) -> Option<&'a super::MirMethod> {
    class.methods.iter().find(|method| method.name == field)
}

/// Whether a value of `ty` is one scalar word: a number or a `bool`.
fn is_scalar_type(ty: &Type) -> bool {
    matches!(ty, Type::Named(name, args) if args.is_empty() && matches!(
        name.as_str(),
        "int32" | "int64" | "uint64" | "bool" | "float32" | "float64"
    ))
}

fn result_error_type(ty: &Type) -> Option<&Type> {
    match ty {
        Type::Named(name, args) if name == "Result" && args.len() == 2 => Some(&args[1]),
        _ => None,
    }
}

/// The module-qualified nominal type a canonical key names, for a key of a
/// non-generic named type. Every other key shape resolves through the
/// member's own spelling.
pub(crate) fn canonical_named_type_from_key(key: &str) -> Option<Type> {
    let encoded = key.strip_prefix("aura-type-key-v1:")?;
    let parsed: serde_json::Value = serde_json::from_str(encoded).ok()?;
    let parts = parsed.as_array()?;
    if parts.len() != 3 || parts[0].as_str()? != "named" {
        return None;
    }
    let name = parts[1].as_str()?;
    if !parts[2].as_array()?.is_empty() {
        return None;
    }
    Some(Type::named(name))
}

#[cfg(test)]
#[path = "mir_methods_tests.rs"]
mod tests;
