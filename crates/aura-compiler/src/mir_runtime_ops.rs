//! Resolved runtime operations in MIR (Batch 7 step 8d).
//!
//! `resolve_runtime_ops` rewrites a call to `CallTarget::Runtime` when it
//! names a resolved runtime operation (see `crate::runtime_ops`):
//!
//! - a member call whose receiver has a concrete built-in type, including
//!   the private members that indexing, slicing, iteration, and spans lower
//!   to. A receiver whose type is a type parameter, a union, or a class that
//!   declares the member keeps its `Member` call. A queue or task call that
//!   passes a `timeout` takes the op's timeout form, and a collection op
//!   takes the scalar or literal-key form its receiver and arguments allow
//!   (see `specialized_member_op`).
//! - a `Name` call to a built-in function, such as `print`, `range`, or
//!   `fs::exists`. A user function with the same name keeps its `Name`
//!   call. `print` and `wait_any` are specialized by their arguments here.
//!
//! It also marks each f-string part whose operand is an `int64` as
//! `MirFormatPart::Int64`. The pass is idempotent, so lowering, the
//! interpreter's MIR loader, and the native emitter can each run it.
//!
//! `validate_runtime_ops` rejects an op that does not apply to its receiver
//! or arguments, or that is called with arguments its parameters do not
//! bind.

use super::{CallTarget, Instruction, MirFunction, MirLoanValidationContext, MirModule, Rvalue};
use crate::call::CallableParam;
use crate::mir::{MirArg, MirFormatPart, Operand};
use crate::runtime_ops::{
    builtin_function_for_name, first_type_arg, DictOp, FunctionOp, ListOp, PrintKind, RuntimeOp,
    ScalarElement, SetOp,
};
use crate::sema::Type;

/// Whether `function` is the MIR body lowering gives a runtime-provided
/// builtin module declaration, such as `fs::exists`, so that a function
/// value can call it. A wrapper has its builtin's name and calls it, so it
/// does not shadow the builtin.
pub(crate) fn is_builtin_wrapper(function: &MirFunction) -> bool {
    // Checked by name: building the module's namespace on every call made
    // each interpreter start and every function-value lookup slow.
    function.source_path.is_none()
        && crate::builtin_modules::is_builtin_root_module(&function.module_name)
}

/// Rewrites every call that names a resolved runtime operation, and every
/// `int64` f-string part.
pub(crate) fn resolve_runtime_ops(module: &mut MirModule) {
    let rewrites = {
        // A module the validator cannot read keeps its calls; the validator
        // reports what is wrong with it.
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
        // Rewrites are collected from assignments and loan writes, so each
        // one finds the rvalue it was collected from.
        let instruction = &mut function.blocks[rewrite.block].instructions[rewrite.instruction];
        if let Instruction::Assign { value, .. } | Instruction::WriteLoan { value, .. } =
            instruction
        {
            apply_rewrite(value, rewrite.kind);
        }
    }
}

fn apply_rewrite(value: &mut Rvalue, kind: RewriteKind) {
    if let Rvalue::FormatString { parts } = value {
        if let RewriteKind::FormatInt64(index) = kind {
            if let MirFormatPart::Value(operand) = &mut parts[index] {
                let operand = std::mem::replace(operand, Operand::Unit);
                parts[index] = MirFormatPart::Int64(operand);
            }
        }
        return;
    }
    if let Rvalue::Call { callee, .. } = value {
        let receiver = match (kind, &mut *callee) {
            (
                RewriteKind::Member(_),
                CallTarget::Member {
                    object,
                    receiver_place,
                    ..
                },
            ) => Some((
                std::mem::replace(object, Operand::Unit),
                receiver_place.take(),
            )),
            _ => None,
        };
        if let RewriteKind::Member(op) | RewriteKind::Named(op) = kind {
            let (receiver, receiver_place) = receiver.unzip();
            *callee = CallTarget::Runtime {
                op,
                receiver,
                receiver_place: receiver_place.flatten(),
            };
        }
    }
}

struct Rewrite {
    /// The function's index in `MirModule::functions`, or `None` for the
    /// top-level function.
    function: Option<usize>,
    block: usize,
    instruction: usize,
    kind: RewriteKind,
}

#[derive(Clone, Copy)]
enum RewriteKind {
    /// A member call becomes this op on its receiver.
    Member(RuntimeOp),
    /// A `Name` call becomes this op.
    Named(RuntimeOp),
    /// The f-string part at this index appends an `int64`.
    FormatInt64(usize),
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
            let mut push = |kind| {
                rewrites.push(Rewrite {
                    function: function_index,
                    block: block_index,
                    instruction: instruction_index,
                    kind,
                })
            };
            match value {
                Rvalue::Call {
                    callee: CallTarget::Member { object, field, .. },
                    args,
                } => {
                    if let Some(op) = context.resolved_runtime_op(function, object, field, args) {
                        push(RewriteKind::Member(op));
                    }
                }
                Rvalue::Call {
                    callee: CallTarget::Name(name),
                    args,
                } => {
                    if let Some(op) = context.resolved_named_op(function, name, args) {
                        push(RewriteKind::Named(op));
                    }
                }
                Rvalue::FormatString { parts } => {
                    for (index, part) in parts.iter().enumerate() {
                        if let MirFormatPart::Value(value) = part {
                            if context.is_int64_operand(function, value) {
                                push(RewriteKind::FormatInt64(index));
                            }
                        }
                    }
                }
                _ => {}
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

    /// Whether a function of the program, not a builtin wrapper, is named
    /// `name`.
    fn declares_user_function(&self, name: &str) -> bool {
        self.functions
            .get(name)
            .is_some_and(|function| !is_builtin_wrapper(function))
    }

    /// The runtime operation a member call `object.field` with `args`
    /// resolves to.
    fn resolved_runtime_op(
        &self,
        function: &MirFunction,
        object: &Operand,
        field: &str,
        args: &[MirArg],
    ) -> Option<RuntimeOp> {
        let receiver = self.operand_type(function, object).ok().flatten()?;
        let op = RuntimeOp::resolve_member(&receiver, field, |class| {
            self.class_declares_method(class, field)
        })?;
        Some(specialized_member_op(op, &receiver, args))
    }

    /// The runtime operation a call to `name` with `args` resolves to.
    fn resolved_named_op(
        &self,
        function: &MirFunction,
        name: &str,
        args: &[MirArg],
    ) -> Option<RuntimeOp> {
        let op = builtin_function_for_name(name, |name| self.declares_user_function(name))?;
        Some(self.specialized_named_op(function, op, args))
    }

    /// The specialization of an unspecialized named op for `args`: `print`
    /// by the layout of its argument, and `wait_any` and `wait_all` by
    /// whether they pass a timeout.
    fn specialized_named_op(
        &self,
        function: &MirFunction,
        op: RuntimeOp,
        args: &[MirArg],
    ) -> RuntimeOp {
        match op {
            RuntimeOp::Print(_) => RuntimeOp::Print(PrintKind::for_type(
                self.print_argument_type(function, args).as_ref(),
            )),
            other => specialized_timeout_op(other, args),
        }
    }

    /// The layout type of the value a `print` call writes, if MIR records
    /// one.
    fn print_argument_type(&self, function: &MirFunction, args: &[MirArg]) -> Option<Type> {
        let value = args
            .iter()
            .find(|argument| argument.name.as_deref().is_none_or(|name| name == "value"))?;
        self.layout_type(function, &value.value)
    }

    /// The type an operand loads with: an integer literal that fits in
    /// 64 bits loads as an `int64`.
    fn layout_type(&self, function: &MirFunction, operand: &Operand) -> Option<Type> {
        match operand {
            Operand::Int(value) => i64::try_from(*value).is_ok().then(|| Type::named("int64")),
            other => self.operand_type(function, other).ok().flatten(),
        }
    }

    fn is_int64_operand(&self, function: &MirFunction, operand: &Operand) -> bool {
        self.layout_type(function, operand) == Some(Type::named("int64"))
    }
}

/// The specialization of an unspecialized member op on a receiver of type
/// `receiver` with `args`:
///
/// - `append` and `__index` on a list of `int64`, `float64`, or `bool`
///   push and read the element without a box;
/// - `get` with a literal key on a dict with `str` keys looks the key up
///   without a box;
/// - `contains` on a set of `int64` searches for the needle without a box;
/// - a queue or task call that binds `timeout` takes the timeout form.
pub(crate) fn specialized_member_op(op: RuntimeOp, receiver: &Type, args: &[MirArg]) -> RuntimeOp {
    let element = first_type_arg(Some(receiver));
    let scalar = element.as_ref().and_then(ScalarElement::of);
    match (op, scalar) {
        (RuntimeOp::List(ListOp::Push), Some(kind)) => RuntimeOp::List(ListOp::PushScalar(kind)),
        (RuntimeOp::List(ListOp::Index), Some(kind)) => RuntimeOp::List(ListOp::IndexScalar(kind)),
        (RuntimeOp::Set(SetOp::Contains), Some(ScalarElement::Int64)) => {
            RuntimeOp::Set(SetOp::ContainsInt64)
        }
        (RuntimeOp::Dict(DictOp::Get), _)
            if element == Some(Type::named("str"))
                && matches!(
                    args,
                    [MirArg {
                        value: Operand::String(_),
                        ..
                    }]
                ) =>
        {
            RuntimeOp::Dict(DictOp::GetStr)
        }
        _ => specialized_timeout_op(op, args),
    }
}

/// The timeout form of an unspecialized op when `args` bind its timeout,
/// such as `Queue.get(timeout)` or `wait_any(tasks, timeout)`, and the op
/// itself otherwise. The timeout is the timeout form's last parameter, so
/// the call binds it by name, or by position once the positional arguments
/// fill every parameter no name binds.
fn specialized_timeout_op(op: RuntimeOp, args: &[MirArg]) -> RuntimeOp {
    let timed = op.with_timeout();
    if timed == op {
        return op;
    }
    let names = |name: &str| {
        args.iter()
            .any(|argument| argument.name.as_deref() == Some(name))
    };
    let open = timed
        .params()
        .iter()
        .filter(|param| !names(param.name))
        .count();
    let positional = args
        .iter()
        .filter(|argument| argument.name.is_none())
        .count();
    if names("timeout") || positional >= open {
        timed
    } else {
        op
    }
}

/// Checks every runtime operation in `function`: its receiver has a type the
/// op applies to and no class method shadows it, or its name resolves to it
/// and its arguments pick its specialization, and its arguments bind to the
/// op's parameters. Each `int64` f-string part must append an `int64`.
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
        if let Rvalue::FormatString { parts } = value {
            validate_format_parts(function, parts, context)?;
            continue;
        }
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
                let field = op.name();
                let resolved = RuntimeOp::resolve_member(&receiver_type, field, |class| {
                    context.class_declares_method(class, field)
                });
                if resolved != Some(op.unspecialized()) {
                    return Err(format!(
                        "invalid MIR runtime operation `{op}` in `{}` does not apply to a receiver of type `{receiver_type}`",
                        function.name
                    ));
                }
                // An unchecked read is the checked read's form; the
                // checks validator decides whether a fact covers it.
                if specialized_member_op(op.unspecialized(), &receiver_type, args) != op.checked() {
                    return Err(format!(
                        "invalid MIR runtime operation `{op}` in `{}` does not match the types of its arguments",
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
            (false, None) => {
                let resolved = builtin_function_for_name(op.name(), |name| {
                    context.declares_user_function(name)
                });
                if resolved != Some(op.unspecialized()) {
                    return Err(format!(
                        "invalid MIR runtime operation `{op}` in `{}` does not name the built-in `{}`",
                        function.name,
                        op.name()
                    ));
                }
                // A `print` of a local with no recorded type is refused
                // later, as an untyped local.
                let untyped_print = matches!(op, RuntimeOp::Print(_))
                    && context.print_argument_type(function, args).is_none();
                if !untyped_print
                    && context.specialized_named_op(function, op.unspecialized(), args) != op
                {
                    return Err(format!(
                        "invalid MIR runtime operation `{op}` in `{}` does not match the types of its arguments",
                        function.name
                    ));
                }
            }
        }
        validate_runtime_op_args(function, op, args)?;
    }
    Ok(())
}

fn validate_format_parts(
    function: &MirFunction,
    parts: &[MirFormatPart],
    context: &MirLoanValidationContext<'_>,
) -> std::result::Result<(), String> {
    for part in parts {
        if let MirFormatPart::Int64(value) = part {
            if !context.is_int64_operand(function, value) {
                return Err(format!(
                    "invalid MIR f-string part in `{}` appends an operand that is not `int64` as one",
                    function.name
                ));
            }
        }
    }
    Ok(())
}

/// Binds `args` to `op`'s parameters as a call binds them: named arguments
/// by name, the rest in order, with every required parameter bound once.
/// `select` takes one or more positional sources, and `range` with one
/// argument binds it to `stop`. The messages name the call as the direct
/// emitter's arity checks did.
fn validate_runtime_op_args(
    function: &MirFunction,
    op: RuntimeOp,
    args: &[MirArg],
) -> std::result::Result<(), String> {
    let member = op.name();
    let invalid = |detail: String| {
        format!(
            "invalid MIR runtime operation `{op}` in `{}`: {detail}",
            function.name
        )
    };
    let params = match op {
        RuntimeOp::Function(FunctionOp::Select) => {
            if args.is_empty() || args.iter().any(|argument| argument.name.is_some()) {
                return Err(invalid(
                    "expected `select(source, ...)` to receive one or more positional sources"
                        .to_string(),
                ));
            }
            return Ok(());
        }
        // As in the checker, one argument binds `stop` unless it names
        // `start`.
        RuntimeOp::Function(FunctionOp::Range)
            if args.len() == 1 && args[0].name.as_deref() != Some("start") =>
        {
            vec![CallableParam::required("stop")]
        }
        _ => op.params(),
    };
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
