//! Resolved runtime operations in MIR (Batch 7 step 8d).
//!
//! `resolve_runtime_ops` rewrites a member call to `CallTarget::Runtime`
//! when its receiver has a concrete built-in type and the member is a
//! resolved runtime operation (see `crate::runtime_ops`). A receiver whose
//! type is a type parameter, a union, or a class that declares the member
//! keeps its `Member` call. The pass is idempotent, so lowering, the
//! interpreter's MIR loader, and the native emitter can each run it.
//!
//! `validate_runtime_ops` rejects an op that does not apply to its
//! receiver, or that is called with arguments its parameters do not bind.

use super::{CallTarget, Instruction, MirFunction, MirLoanValidationContext, MirModule, Rvalue};
use crate::mir::{MirArg, Operand};
use crate::runtime_ops::RuntimeOp;

/// Rewrites every member call that names a resolved runtime operation.
pub(crate) fn resolve_runtime_ops(module: &mut MirModule) {
    let rewrites = {
        // A module the validator cannot read keeps its member calls; the
        // validator reports what is wrong with it.
        let Ok(context) = MirLoanValidationContext::new(module) else {
            return;
        };
        let mut rewrites = Vec::new();
        for (function_index, function) in module.functions.iter().enumerate() {
            collect_rewrites(&context, function, Some(function_index), &mut rewrites);
        }
        if let Some(top_level) = &module.top_level {
            collect_rewrites(&context, top_level, None, &mut rewrites);
        }
        rewrites
    };
    for rewrite in rewrites {
        let function = match rewrite.function {
            Some(index) => &mut module.functions[index],
            None => module
                .top_level
                .as_mut()
                .expect("a top-level rewrite has a top-level function"),
        };
        // `collect_rewrites` recorded only member calls, so each rewrite
        // finds one.
        if let Instruction::Assign {
            value: Rvalue::Call { callee, .. },
            ..
        }
        | Instruction::WriteLoan {
            value: Rvalue::Call { callee, .. },
            ..
        } = &mut function.blocks[rewrite.block].instructions[rewrite.instruction]
        {
            if let CallTarget::Member {
                object,
                receiver_place,
                ..
            } = callee
            {
                *callee = CallTarget::Runtime {
                    op: rewrite.op,
                    receiver: Some(std::mem::replace(object, Operand::Unit)),
                    receiver_place: receiver_place.take(),
                };
            }
        }
    }
}

struct Rewrite {
    /// The function's index in `MirModule::functions`, or `None` for the
    /// top-level function.
    function: Option<usize>,
    block: usize,
    instruction: usize,
    op: RuntimeOp,
}

fn collect_rewrites(
    context: &MirLoanValidationContext<'_>,
    function: &MirFunction,
    function_index: Option<usize>,
    rewrites: &mut Vec<Rewrite>,
) {
    for (block_index, block) in function.blocks.iter().enumerate() {
        for (instruction_index, instruction) in block.instructions.iter().enumerate() {
            let (Instruction::Assign { value, .. } | Instruction::WriteLoan { value, .. }) =
                instruction
            else {
                continue;
            };
            let Rvalue::Call {
                callee: CallTarget::Member { object, field, .. },
                ..
            } = value
            else {
                continue;
            };
            if let Some(op) = context.resolved_runtime_op(function, object, field) {
                rewrites.push(Rewrite {
                    function: function_index,
                    block: block_index,
                    instruction: instruction_index,
                    op,
                });
            }
        }
    }
}

impl MirLoanValidationContext<'_> {
    /// Whether class `class` declares a method named `field`.
    fn class_declares_method(&self, class: &str, field: &str) -> bool {
        self.classes
            .get(class)
            .is_some_and(|class| class.methods.iter().any(|method| method.name == field))
    }

    /// The runtime operation a member call `object.field` resolves to.
    fn resolved_runtime_op(
        &self,
        function: &MirFunction,
        object: &Operand,
        field: &str,
    ) -> Option<RuntimeOp> {
        let receiver = self.operand_type(function, object).ok().flatten()?;
        RuntimeOp::resolve_member(&receiver, field, |class| {
            self.class_declares_method(class, field)
        })
    }
}

/// Checks every runtime operation in `function`: its receiver has a type the
/// op applies to and no class method shadows it, and its arguments bind to
/// the op's parameters.
pub(super) fn validate_runtime_ops(
    function: &MirFunction,
    context: &MirLoanValidationContext<'_>,
) -> std::result::Result<(), String> {
    for instruction in function.blocks.iter().flat_map(|block| &block.instructions) {
        let (Instruction::Assign { value, .. } | Instruction::WriteLoan { value, .. }) =
            instruction
        else {
            continue;
        };
        let Rvalue::Call {
            callee: CallTarget::Runtime { op, receiver, .. },
            args,
        } = value
        else {
            continue;
        };
        let op = *op;
        match (op.has_receiver(), receiver) {
            (true, Some(object)) => {
                let Some(receiver_type) = context.operand_type(function, object)? else {
                    return Err(format!(
                        "invalid MIR runtime operation `{op}` in `{}` has a receiver of unknown type",
                        function.name
                    ));
                };
                let field = op.member_name();
                let resolved = RuntimeOp::resolve_member(&receiver_type, field, |class| {
                    context.class_declares_method(class, field)
                });
                if resolved != Some(op) {
                    return Err(format!(
                        "invalid MIR runtime operation `{op}` in `{}` does not apply to a receiver of type `{receiver_type}`",
                        function.name
                    ));
                }
            }
            (true, None) => {
                return Err(format!(
                    "invalid MIR runtime operation `{op}` in `{}` has no receiver",
                    function.name
                ))
            }
            (false, Some(_)) => {
                return Err(format!(
                    "invalid MIR runtime operation `{op}` in `{}` takes no receiver",
                    function.name
                ))
            }
            (false, None) => {}
        }
        validate_runtime_op_args(function, op, args)?;
    }
    Ok(())
}

/// Binds `args` to `op`'s parameters as a call binds them: named arguments
/// by name, the rest in order, with every required parameter bound once.
/// The messages name the member as the direct emitter's arity checks did.
fn validate_runtime_op_args(
    function: &MirFunction,
    op: RuntimeOp,
    args: &[MirArg],
) -> std::result::Result<(), String> {
    let member = op.member_name();
    let invalid = |detail: String| {
        format!(
            "invalid MIR runtime operation `{op}` in `{}`: {detail}",
            function.name
        )
    };
    let params = op.params().collect::<Vec<_>>();
    let mut bound = vec![false; params.len()];
    let mut next_positional = 0usize;
    for argument in args {
        let index = match argument.name.as_deref() {
            Some(name) => match params.iter().position(|param| param.name == name) {
                Some(index) => index,
                None => return Err(invalid(format!("`{member}()` has no parameter `{name}`"))),
            },
            None => {
                while next_positional < bound.len() && bound[next_positional] {
                    next_positional += 1;
                }
                next_positional
            }
        };
        if index >= bound.len() {
            return Err(invalid(if params.is_empty() {
                format!("expected `{member}()` to take no arguments")
            } else {
                format!(
                    "expected `{member}()` to take at most {} argument(s), found {}",
                    params.len(),
                    args.len()
                )
            }));
        }
        if bound[index] {
            return Err(invalid(format!(
                "`{member}()` receives `{}` twice",
                params[index].name
            )));
        }
        bound[index] = true;
    }
    for (param, bound) in params.iter().zip(&bound) {
        if param.required && !bound {
            return Err(invalid(format!(
                "expected `{member}()` to receive `{}`",
                param.name
            )));
        }
    }
    Ok(())
}
