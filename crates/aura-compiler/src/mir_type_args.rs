//! Type arguments of calls to generic functions (Batch 7 step 8h).
//!
//! A generic function that no pass specializes keeps one body. A backend
//! passes each argument whose parameter names a type parameter through that
//! parameter's boxed layout, and it needs the type each type parameter
//! stands for at the call to box the argument with the right type.
//!
//! `record_call_type_args` writes that type on every `Name` call to such a
//! function as `Rvalue::Call::type_args`, one entry for each type parameter
//! the callee's signature names. The callee's return type is matched
//! against the call's destination first, then each parameter type against
//! its argument's type. The first type a parameter meets is its argument. A
//! type parameter that nothing fixes stands for itself.
//!
//! The pass runs after the ownership pass: in `lower`, and at the native
//! boundary for an implicit module. `lower_implicit` skips it, so `aura run`
//! pays nothing for it. The interpreter reads values, not layouts, so it
//! ignores the type arguments.
//!
//! `validate_call_type_args` refuses a call whose type arguments do not name
//! exactly the callee's type parameters.

use std::collections::{BTreeMap, BTreeSet, HashMap};

use super::{
    collect_type_params_from_type, CallTarget, Instruction, MirFunction, MirLoanValidationContext,
    MirModule, Operand, Rvalue,
};
use crate::sema::Type;

/// The parameter types a call of `function` binds its arguments to: the
/// receiver's type first when it has one, then each parameter's.
fn callee_param_types(function: &MirFunction) -> Vec<&Type> {
    let receiver = function
        .receiver
        .and_then(|_| {
            function
                .local_types
                .iter()
                .find(|local| local.name == "self")
        })
        .map(|local| &local.ty);
    receiver
        .into_iter()
        .chain(function.params.iter().map(|param| &param.ty))
        .collect()
}

/// The type parameters `function`'s signature names, in name order.
pub(crate) fn callee_type_params(function: &MirFunction) -> BTreeSet<String> {
    let mut type_params = BTreeSet::new();
    for ty in callee_param_types(function) {
        collect_type_params_from_type(ty, &mut type_params);
    }
    collect_type_params_from_type(&function.return_type, &mut type_params);
    type_params
}

/// Records the type arguments of every call to a generic function.
pub(crate) fn record_call_type_args(module: &mut MirModule) {
    let records = {
        // A module the validator cannot read keeps its calls; the validator
        // reports what is wrong with it.
        let Ok(context) = MirLoanValidationContext::new(module) else {
            return;
        };
        let generic = generic_functions(module);
        if generic.is_empty() {
            return;
        }
        let mut records = Vec::new();
        let functions = module
            .functions
            .iter()
            .map(Some)
            .chain([module.top_level.as_ref()]);
        for (function_index, function) in functions.enumerate() {
            let Some(function) = function else {
                continue;
            };
            for (block_index, block) in function.blocks.iter().enumerate() {
                for (instruction_index, instruction) in block.instructions.iter().enumerate() {
                    let (target, value) = match instruction {
                        Instruction::Assign { target, value } => (target, value),
                        Instruction::WriteLoan { loan, value } => (loan, value),
                        _ => continue,
                    };
                    let Rvalue::Call {
                        callee: CallTarget::Name(name),
                        args,
                        ..
                    } = value
                    else {
                        continue;
                    };
                    let Some((callee, type_params)) = generic.get(name.as_str()) else {
                        continue;
                    };
                    let type_args = call_type_args(
                        &context,
                        function,
                        callee,
                        type_params,
                        args.iter().map(|argument| &argument.value),
                        target,
                    );
                    records.push((function_index, block_index, instruction_index, type_args));
                }
            }
        }
        records
    };
    for (function_index, block, instruction, type_args) in records {
        let function = match module.functions.get_mut(function_index) {
            Some(function) => function,
            None => module
                .top_level
                .as_mut()
                .expect("a record past the functions is in the top level"),
        };
        if let Instruction::Assign {
            value: Rvalue::Call {
                type_args: slot, ..
            },
            ..
        }
        | Instruction::WriteLoan {
            value: Rvalue::Call {
                type_args: slot, ..
            },
            ..
        } = &mut function.blocks[block].instructions[instruction]
        {
            *slot = type_args;
        }
    }
}

/// Each generic function of `module` by name, with its type parameters.
fn generic_functions(module: &MirModule) -> HashMap<&str, (&MirFunction, BTreeSet<String>)> {
    module
        .functions
        .iter()
        .filter_map(|function| {
            let type_params = callee_type_params(function);
            (!type_params.is_empty()).then_some((function.name.as_str(), (function, type_params)))
        })
        .collect()
}

/// The type each of `callee`'s type parameters stands for at a call that
/// passes `args` and stores its result in `target`.
fn call_type_args<'a>(
    context: &MirLoanValidationContext<'_>,
    function: &MirFunction,
    callee: &MirFunction,
    type_params: &BTreeSet<String>,
    args: impl Iterator<Item = &'a Operand>,
    target: &str,
) -> BTreeMap<String, Type> {
    let mut bindings = HashMap::new();
    if let Some(destination) = place_layout_type(context, function, target) {
        bind_type_params(&callee.return_type, &destination, &mut bindings);
    }
    for (param, argument) in callee_param_types(callee).into_iter().zip(args) {
        if let Some(actual) = operand_layout_type(context, function, argument) {
            bind_type_params(param, &actual, &mut bindings);
        }
    }
    type_params
        .iter()
        .map(|name| {
            let ty = bindings
                .remove(name)
                .unwrap_or_else(|| Type::TypeParam(name.clone()));
            (name.clone(), ty)
        })
        .collect()
}

/// The type a backend lays a place out as. A returned view has no layout of
/// its own.
pub(super) fn place_layout_type(
    context: &MirLoanValidationContext<'_>,
    function: &MirFunction,
    place: &str,
) -> Option<Type> {
    let ty = context.place_type(function, place).ok().flatten()?;
    (!names_returned_view(&ty)).then_some(ty)
}

/// The type a backend loads `operand` as. An integer literal that fits in
/// 64 bits loads as an `int64`; a wider one is boxed with no static type.
pub(super) fn operand_layout_type(
    context: &MirLoanValidationContext<'_>,
    function: &MirFunction,
    operand: &Operand,
) -> Option<Type> {
    match operand {
        Operand::Place(place) | Operand::MovePlace(place) => {
            place_layout_type(context, function, place)
        }
        Operand::Int(value) => Some(if i64::try_from(*value).is_ok() {
            Type::named("int64")
        } else {
            Type::named("Unknown")
        }),
        other => context.operand_type(function, other).ok().flatten(),
    }
}

/// Whether `ty` is, or holds as a union member or tuple element, a
/// returned view.
fn names_returned_view(ty: &Type) -> bool {
    match ty {
        Type::ReturnedView(_) => true,
        Type::Tuple(elements) => elements.iter().any(names_returned_view),
        Type::Union(union) => union.members.iter().any(names_returned_view),
        _ => false,
    }
}

/// Binds each type parameter `pattern` names to the matching part of
/// `actual`, keeping a binding already made. A part that does not match
/// binds nothing, and a union binds nothing.
pub(super) fn bind_type_params(
    pattern: &Type,
    actual: &Type,
    bindings: &mut HashMap<String, Type>,
) {
    match (pattern, actual) {
        (Type::TypeParam(name), _) => {
            bindings
                .entry(name.clone())
                .or_insert_with(|| actual.clone());
        }
        (Type::Named(name, pattern_args), Type::Named(actual_name, actual_args))
            if name == actual_name && pattern_args.len() == actual_args.len() =>
        {
            for (pattern_arg, actual_arg) in pattern_args.iter().zip(actual_args) {
                bind_type_params(pattern_arg, actual_arg, bindings);
            }
        }
        (Type::Tuple(pattern_elements), Type::Tuple(actual_elements))
            if pattern_elements.len() == actual_elements.len() =>
        {
            for (pattern_element, actual_element) in pattern_elements.iter().zip(actual_elements) {
                bind_type_params(pattern_element, actual_element, bindings);
            }
        }
        (
            Type::Function {
                params: pattern_params,
                return_type: pattern_return,
            },
            Type::Function {
                params: actual_params,
                return_type: actual_return,
            },
        ) if pattern_params.len() == actual_params.len() => {
            for (pattern_param, actual_param) in pattern_params.iter().zip(actual_params) {
                if pattern_param.passing != actual_param.passing {
                    return;
                }
                bind_type_params(&pattern_param.ty, &actual_param.ty, bindings);
            }
            bind_type_params(pattern_return, actual_return, bindings);
        }
        (Type::ReturnedView(view), Type::ReturnedView(actual_view)) => {
            bind_type_params(&view.pointee, &actual_view.pointee, bindings);
        }
        (Type::Callable(pattern_callable), Type::Callable(actual_callable))
            if pattern_callable.params.len() == actual_callable.params.len() =>
        {
            for (pattern_param, actual_param) in
                pattern_callable.params.iter().zip(&actual_callable.params)
            {
                bind_type_params(&pattern_param.ty, &actual_param.ty, bindings);
            }
            bind_type_params(
                &pattern_callable.return_type,
                &actual_callable.return_type,
                bindings,
            );
        }
        (
            Type::Closure {
                params: pattern_params,
                return_type: pattern_return,
                captures: pattern_captures,
                ..
            },
            Type::Closure {
                params: actual_params,
                return_type: actual_return,
                captures: actual_captures,
                ..
            },
        ) if pattern_params.len() == actual_params.len()
            && pattern_captures.len() == actual_captures.len() =>
        {
            for (pattern_param, actual_param) in pattern_params.iter().zip(actual_params.iter()) {
                bind_type_params(&pattern_param.ty, &actual_param.ty, bindings);
            }
            for (pattern_capture, actual_capture) in
                pattern_captures.iter().zip(actual_captures.iter())
            {
                bind_type_params(&pattern_capture.ty, &actual_capture.ty, bindings);
            }
            bind_type_params(pattern_return, actual_return, bindings);
        }
        _ => {}
    }
}

/// Checks that each call names a type argument for exactly the type
/// parameters of the generic function it calls, and that no other call
/// names one.
pub(crate) fn validate_call_type_args(module: &MirModule) -> std::result::Result<(), String> {
    let generic = generic_functions(module);
    for function in module.functions.iter().chain(module.top_level.as_ref()) {
        for instruction in function.blocks.iter().flat_map(|block| &block.instructions) {
            let (Instruction::Assign {
                value: Rvalue::Call {
                    callee, type_args, ..
                },
                ..
            }
            | Instruction::WriteLoan {
                value: Rvalue::Call {
                    callee, type_args, ..
                },
                ..
            }) = instruction
            else {
                continue;
            };
            let expected = match callee {
                CallTarget::Name(name) => generic.get(name.as_str()).map(|(_, params)| params),
                _ => None,
            };
            let matches = match expected {
                Some(type_params) => type_args.keys().eq(type_params.iter()),
                None => type_args.is_empty(),
            };
            if !matches {
                return Err(format!(
                    "invalid MIR call in `{}` passes {} type argument(s) to a callee with {} type parameter(s)",
                    function.name,
                    type_args.len(),
                    expected.map_or(0, BTreeSet::len)
                ));
            }
        }
    }
    Ok(())
}

#[cfg(test)]
#[path = "mir_type_args_tests.rs"]
mod tests;
