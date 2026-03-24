use std::collections::{BTreeMap, HashMap};
use std::io::{self, Write};
use std::panic;
use std::slice;
use std::str;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration as StdDuration, Instant};

use crate::ast::UnaryOp;
use crate::diag::{Diagnostic, Result};
use crate::interpreter::{
    cast_numeric_value, CancellationContext, ChannelValue, EnumVariantValue, InstanceValue,
    RangeValue, RunOutput, TaskGroupValue, TaskValue, TryRecvResult, Value,
};
use crate::mir::{
    CallTarget, Instruction, MirArg, MirClass, MirFunction, MirMethod, MirModule, MirParam,
    MirReceiverKind, MirSelectKind, MirTraitImpl, Operand, Rvalue, Terminator,
};
use crate::sema::Type;

pub fn run(module: &MirModule) -> Result<RunOutput> {
    let stdout = Arc::new(Mutex::new(String::new()));
    let mut runtime = MirRuntime::new(
        module.clone(),
        stdout.clone(),
        CancellationContext::default(),
    );
    let value = runtime.run_main()?;
    let rendered_stdout = stdout.lock().unwrap().clone();
    Ok(RunOutput {
        value,
        stdout: rendered_stdout,
    })
}

pub fn run_serialized_mir(mir_json: &[u8], source_path: &str, source: &str) -> Result<RunOutput> {
    let module = serde_json::from_slice::<MirModule>(mir_json).map_err(|error| {
        Diagnostic::new(format!("failed to deserialize embedded MIR: {}", error))
    })?;
    let _ = source_path;
    let _ = source;
    run(&module)
}

fn render_runtime_error(path: &str, source: &str, error: &Diagnostic) -> String {
    error.render_with_source(path, source)
}

fn write_stream(mut stream: impl Write, text: &str) -> io::Result<()> {
    stream
        .write_all(text.as_bytes())
        .and_then(|_| stream.flush())
}

fn run_serialized_mir_entrypoint(mir_json: &[u8], source_path: &str, source: &str) -> i32 {
    match run_serialized_mir(mir_json, source_path, source) {
        Ok(output) => {
            if let Err(error) = write_stream(io::stdout().lock(), &output.stdout) {
                if error.kind() == io::ErrorKind::BrokenPipe {
                    return 0;
                }
                let _ = writeln!(io::stderr().lock(), "failed to write to stdout: {}", error);
                return 1;
            }
            if let Value::Int(code) = output.value {
                return code as i32;
            }
            0
        }
        Err(error) => {
            let rendered = render_runtime_error(source_path, source, &error);
            let _ = writeln!(io::stderr().lock(), "{}", rendered);
            1
        }
    }
}

#[no_mangle]
pub extern "C" fn aurora_native_run(
    mir_ptr: *const u8,
    mir_len: usize,
    source_path_ptr: *const u8,
    source_path_len: usize,
    source_ptr: *const u8,
    source_len: usize,
) -> i32 {
    let result = panic::catch_unwind(|| {
        if mir_ptr.is_null() || source_path_ptr.is_null() || source_ptr.is_null() {
            let _ = writeln!(
                io::stderr().lock(),
                "aurora native runtime received a null input"
            );
            return 1;
        }

        let mir_json = unsafe { slice::from_raw_parts(mir_ptr, mir_len) };
        let source_path_bytes = unsafe { slice::from_raw_parts(source_path_ptr, source_path_len) };
        let source_bytes = unsafe { slice::from_raw_parts(source_ptr, source_len) };

        let source_path = match str::from_utf8(source_path_bytes) {
            Ok(text) => text,
            Err(error) => {
                let _ = writeln!(
                    io::stderr().lock(),
                    "embedded source path is not valid UTF-8: {}",
                    error
                );
                return 1;
            }
        };

        let source = match str::from_utf8(source_bytes) {
            Ok(text) => text,
            Err(error) => {
                let _ = writeln!(
                    io::stderr().lock(),
                    "embedded source is not valid UTF-8: {}",
                    error
                );
                return 1;
            }
        };

        run_serialized_mir_entrypoint(mir_json, source_path, source)
    });

    match result {
        Ok(code) => code,
        Err(_) => {
            let _ = writeln!(io::stderr().lock(), "aurora native runtime panicked");
            1
        }
    }
}

struct MirRuntime {
    module: Arc<MirModule>,
    functions: HashMap<String, MirFunction>,
    classes: HashMap<String, MirClass>,
    trait_impls: Vec<MirTraitImpl>,
    stdout: Arc<Mutex<String>>,
    cancellation: CancellationContext,
}

struct CallOutcome {
    value: Value,
    updated_receiver: Option<Value>,
    updated_params: Vec<(usize, Value)>,
}

#[derive(Clone)]
struct EvaluatedMirArg {
    name: Option<String>,
    value: Value,
    writeback_place: Option<String>,
}

enum RvalueOutcome {
    Value(Value),
    Return(Value),
}

#[derive(Default)]
struct Env {
    values: HashMap<String, Value>,
    types: HashMap<String, Type>,
}

impl Env {
    fn define_typed(&mut self, name: impl Into<String>, ty: Type, value: Value) {
        let name = name.into();
        self.types.insert(name.clone(), ty);
        self.values.insert(name, value);
    }

    fn read_place(&self, place: &str) -> Result<Value> {
        let mut segments = place.split('.');
        let root = segments
            .next()
            .ok_or_else(|| Diagnostic::new("empty MIR place"))?;
        let mut value = self
            .values
            .get(root)
            .cloned()
            .ok_or_else(|| Diagnostic::new(format!("unknown MIR place `{}`", place)))?;
        for segment in segments {
            let Value::Instance(instance) = value else {
                return Err(Diagnostic::new(format!(
                    "cannot access field `{}` on non-instance MIR place `{}`",
                    segment, place
                )));
            };
            value = instance.fields.get(segment).cloned().ok_or_else(|| {
                Diagnostic::new(format!(
                    "class `{}` has no field `{}` in MIR place `{}`",
                    instance.class_name, segment, place
                ))
            })?;
        }
        Ok(value)
    }

    fn write_place(&mut self, place: &str, value: Value) -> Result<()> {
        let segments = place.split('.').collect::<Vec<_>>();
        let Some((root, rest)) = segments.split_first() else {
            return Err(Diagnostic::new("empty MIR place"));
        };

        if rest.is_empty() {
            self.values.insert((*root).to_string(), value);
            return Ok(());
        }

        let mut root_value = self
            .values
            .get(*root)
            .cloned()
            .ok_or_else(|| Diagnostic::new(format!("unknown MIR place `{}`", place)))?;
        write_nested_place(&mut root_value, rest, value, place)?;
        self.values.insert((*root).to_string(), root_value);
        Ok(())
    }

    fn place_type(&self, place: &str) -> Option<&Type> {
        self.types.get(place)
    }

    fn set_place_type(&mut self, place: &str, ty: Type) {
        self.types.insert(place.to_string(), ty);
    }
}

fn write_nested_place(
    current: &mut Value,
    segments: &[&str],
    value: Value,
    full_place: &str,
) -> Result<()> {
    let Value::Instance(instance) = current else {
        return Err(Diagnostic::new(format!(
            "cannot assign nested MIR place `{}` on non-instance value",
            full_place
        )));
    };

    if segments.len() == 1 {
        instance.fields.insert(segments[0].to_string(), value);
        return Ok(());
    }

    let child = instance.fields.get_mut(segments[0]).ok_or_else(|| {
        Diagnostic::new(format!(
            "class `{}` has no field `{}` in MIR place `{}`",
            instance.class_name, segments[0], full_place
        ))
    })?;
    write_nested_place(child, &segments[1..], value, full_place)
}

impl MirRuntime {
    fn new(
        module: MirModule,
        stdout: Arc<Mutex<String>>,
        cancellation: CancellationContext,
    ) -> Self {
        let functions = module
            .functions
            .iter()
            .cloned()
            .map(|function| (function.name.clone(), function))
            .collect::<HashMap<_, _>>();
        let classes = module
            .classes
            .iter()
            .cloned()
            .map(|class| (class.name.clone(), class))
            .collect::<HashMap<_, _>>();
        let trait_impls = module.trait_impls.clone();
        Self {
            module: Arc::new(module),
            functions,
            classes,
            trait_impls,
            stdout,
            cancellation,
        }
    }

    fn find_trait_impl_method(&self, receiver_ty: &Type, field: &str) -> Option<&MirMethod> {
        self.trait_impls.iter().find_map(|trait_impl| {
            if &trait_impl.for_type != receiver_ty {
                return None;
            }
            trait_impl
                .methods
                .iter()
                .find(|method| method.name == field)
        })
    }

    fn find_trait_impl_method_for_class_name(
        &self,
        class_name: &str,
        field: &str,
    ) -> Option<&MirMethod> {
        let mut matches =
            self.trait_impls
                .iter()
                .filter_map(|trait_impl| match &trait_impl.for_type {
                    Type::Named(name, _) if name == class_name => trait_impl
                        .methods
                        .iter()
                        .find(|method| method.name == field),
                    _ => None,
                });
        let first = matches.next()?;
        if matches.next().is_some() {
            return None;
        }
        Some(first)
    }

    fn run_main(&mut self) -> Result<Value> {
        if let Some(main_fn) = self.functions.get("main").cloned() {
            return Ok(self.call_function(&main_fn, None, Vec::new())?.value);
        }

        let Some(top_level) = self.module.top_level.clone() else {
            return Err(Diagnostic::new(
                "no `main` function or top-level script statements were found",
            ));
        };
        Ok(self.call_function(&top_level, None, Vec::new())?.value)
    }

    fn infer_value_type(value: &Value) -> Option<Type> {
        match value {
            Value::Int(_) => Some(Type::named("int32")),
            Value::Float(_) => Some(Type::named("float64")),
            Value::Bool(_) => Some(Type::named("bool")),
            Value::String(_) => Some(Type::named("String")),
            Value::Duration(_) => Some(Type::named("Duration")),
            Value::Range(_) => Some(Type::named("Range")),
            Value::ModuleNamespace(_) => None,
            Value::Unit => Some(Type::Unit),
            Value::Instance(instance) => Some(Type::named(&instance.class_name)),
            Value::EnumVariant(variant) => Some(Type::named(&variant.enum_name)),
            Value::Channel(_) | Value::Task(_) | Value::TaskGroup(_) => None,
        }
    }

    fn validate_value_fits_type(
        &self,
        value: &Value,
        ty: &Type,
        span: Option<crate::diag::Span>,
    ) -> Result<()> {
        if let Some((min, max)) = crate::sema::integer_type_bounds(ty) {
            let Value::Int(value) = value else {
                return Ok(());
            };
            let value = *value as i128;
            if value < min || value > max {
                let message = format!("integer value `{}` does not fit in `{}`", value, ty);
                return Err(match span {
                    Some(span) => Diagnostic::at(span, message),
                    None => Diagnostic::new(message),
                });
            }
        }
        Ok(())
    }

    fn resolve_place_type(&self, place: &str, env: &Env) -> Option<Type> {
        let mut segments = place.split('.');
        let root = segments.next()?;
        let mut current = env.place_type(root).cloned().or_else(|| {
            env.read_place(root)
                .ok()
                .and_then(|value| Self::infer_value_type(&value))
        })?;

        for segment in segments {
            let Type::Named(class_name, args) = current else {
                return None;
            };
            if !args.is_empty() {
                return None;
            }
            let class = self.classes.get(&class_name)?;
            let field = class.fields.iter().find(|field| field.name == segment)?;
            current = field.ty.clone();
        }

        Some(current)
    }

    fn call_function(
        &mut self,
        function: &MirFunction,
        receiver: Option<Value>,
        args: Vec<EvaluatedMirArg>,
    ) -> Result<CallOutcome> {
        let mut env = Env::default();
        for local in &function.local_types {
            env.set_place_type(&local.name, local.ty.clone());
        }
        if function.receiver.is_some() {
            let Some(receiver) = receiver else {
                return Err(Diagnostic::new(format!(
                    "MIR function `{}` is missing its receiver",
                    function.name
                )));
            };
            let receiver_ty =
                Self::infer_value_type(&receiver).unwrap_or_else(|| Type::named("Unknown"));
            env.define_typed("self", receiver_ty, receiver);
        }

        let bound_args = bind_args(&function.params, args)?;
        for (param, argument) in function.params.iter().zip(bound_args.iter()) {
            self.validate_value_fits_type(&argument.value, &param.ty, None)?;
            env.define_typed(&param.name, param.ty.clone(), argument.value.clone());
        }

        let value = self.execute_function(function, &mut env)?;
        let updated_receiver = if function.receiver == Some(MirReceiverKind::BorrowMut) {
            Some(env.read_place("self")?)
        } else {
            None
        };
        let mut updated_params = Vec::new();
        for (index, param) in function.params.iter().enumerate() {
            if param.passing == MirReceiverKind::BorrowMut {
                updated_params.push((index, env.read_place(&param.name)?));
            }
        }
        Ok(CallOutcome {
            value,
            updated_receiver,
            updated_params,
        })
    }

    fn execute_function(&mut self, function: &MirFunction, env: &mut Env) -> Result<Value> {
        let block_map = function
            .blocks
            .iter()
            .enumerate()
            .map(|(index, block)| (block.label.clone(), index))
            .collect::<HashMap<_, _>>();
        let mut current_label = function.entry.clone();
        let mut loop_state = HashMap::<String, i128>::new();
        let mut cleanup_stack = Vec::<String>::new();

        loop {
            let block_index = block_map.get(&current_label).copied().ok_or_else(|| {
                Diagnostic::new(format!(
                    "unknown MIR block `{}` in function `{}`",
                    current_label, function.name
                ))
            })?;
            let block = &function.blocks[block_index];
            for instruction in &block.instructions {
                if let Some(value) =
                    self.execute_instruction(instruction, env, &mut cleanup_stack)?
                {
                    self.unwind_cleanups(&mut cleanup_stack, env, true)?;
                    return Ok(value);
                }
            }

            match self.execute_terminator(
                &block.label,
                &block.terminator,
                env,
                &mut loop_state,
                &mut cleanup_stack,
            )? {
                BlockOutcome::Return(value) => {
                    self.unwind_cleanups(&mut cleanup_stack, env, true)?;
                    return Ok(value);
                }
                BlockOutcome::Goto(next) => current_label = next,
            }
        }
    }

    fn execute_instruction(
        &mut self,
        instruction: &Instruction,
        env: &mut Env,
        cleanup_stack: &mut Vec<String>,
    ) -> Result<Option<Value>> {
        match instruction {
            Instruction::Assign { target, value } => match self.evaluate_rvalue(value, env)? {
                RvalueOutcome::Value(evaluated) => {
                    let span = match value {
                        Rvalue::Unary { span, .. } | Rvalue::Binary { span, .. } => Some(*span),
                        _ => None,
                    };
                    if let Some(target_ty) = self.resolve_place_type(target, env) {
                        self.validate_value_fits_type(&evaluated, &target_ty, span)?;
                        if !target.contains('.') {
                            env.set_place_type(target, target_ty);
                        }
                    } else if !target.contains('.') {
                        if let Some(inferred_ty) = Self::infer_value_type(&evaluated) {
                            env.set_place_type(target, inferred_ty);
                        }
                    }
                    env.write_place(target, evaluated)?;
                    Ok(None)
                }
                RvalueOutcome::Return(value) => Ok(Some(value)),
            },
            Instruction::Eval { value } => {
                let _ = self.evaluate_operand(value, env)?;
                Ok(None)
            }
            Instruction::PushCleanup { place } => {
                cleanup_stack.push(place.clone());
                Ok(None)
            }
            Instruction::PopCleanup {
                place,
                cancel_before_cleanup,
            } => {
                self.pop_cleanup(place, cleanup_stack, env, *cancel_before_cleanup)?;
                Ok(None)
            }
        }
    }

    fn execute_terminator(
        &mut self,
        block_label: &str,
        terminator: &Terminator,
        env: &mut Env,
        loop_state: &mut HashMap<String, i128>,
        _cleanup_stack: &mut Vec<String>,
    ) -> Result<BlockOutcome> {
        match terminator {
            Terminator::Return(value) => {
                Ok(BlockOutcome::Return(self.evaluate_operand(value, env)?))
            }
            Terminator::Goto(label) => Ok(BlockOutcome::Goto(label.clone())),
            Terminator::Branch {
                condition,
                then_label,
                else_label,
            } => match self.evaluate_operand(condition, env)? {
                Value::Bool(true) => Ok(BlockOutcome::Goto(then_label.clone())),
                Value::Bool(false) => Ok(BlockOutcome::Goto(else_label.clone())),
                other => Err(Diagnostic::new(format!(
                    "MIR branch condition must evaluate to `bool`, found `{}`",
                    other.render()
                ))),
            },
            Terminator::ForRange {
                binding,
                iterable,
                body_label,
                exit_label,
            } => {
                let iterable = self.evaluate_operand(iterable, env)?;
                let Value::Range(range) = iterable else {
                    return Err(Diagnostic::new(format!(
                        "MIR `for` requires a `Range`, found `{}`",
                        iterable.render()
                    )));
                };
                let next = loop_state
                    .entry(block_label.to_string())
                    .or_insert(range.start);
                if *next < range.end {
                    let current = *next;
                    *next += 1;
                    env.write_place(binding, Value::Int(current))?;
                    Ok(BlockOutcome::Goto(body_label.clone()))
                } else {
                    loop_state.remove(block_label);
                    Ok(BlockOutcome::Goto(exit_label.clone()))
                }
            }
            Terminator::Match {
                scrutinee,
                arms,
                otherwise,
            } => {
                let scrutinee = self.evaluate_operand(scrutinee, env)?;
                let Value::EnumVariant(variant) = scrutinee else {
                    return Err(Diagnostic::new(format!(
                        "MIR `match` expected an enum value, found `{}`",
                        scrutinee.render()
                    )));
                };
                for arm in arms {
                    if arm.wildcard
                        || (arm.enum_name.as_deref() == Some(variant.enum_name.as_str())
                            && arm.variant_name.as_deref() == Some(variant.variant_name.as_str()))
                    {
                        return Ok(BlockOutcome::Goto(arm.label.clone()));
                    }
                }
                Ok(BlockOutcome::Goto(otherwise.clone()))
            }
            Terminator::Select { arms, otherwise: _ } => self.execute_select(arms, env),
            Terminator::Unreachable => Err(Diagnostic::new("reached unreachable MIR block")),
        }
    }

    fn pop_cleanup(
        &mut self,
        place: &str,
        cleanup_stack: &mut Vec<String>,
        env: &mut Env,
        cancel_before_cleanup: bool,
    ) -> Result<()> {
        let Some(active_place) = cleanup_stack.pop() else {
            return Err(Diagnostic::new(format!(
                "MIR cleanup stack underflow while closing `{}`",
                place
            )));
        };
        if active_place != place {
            return Err(Diagnostic::new(format!(
                "MIR cleanup stack mismatch: expected `{}`, found `{}`",
                place, active_place
            )));
        }
        self.run_cleanup_place(&active_place, env, cancel_before_cleanup)
    }

    fn unwind_cleanups(
        &mut self,
        cleanup_stack: &mut Vec<String>,
        env: &mut Env,
        cancel_before_cleanup: bool,
    ) -> Result<()> {
        while let Some(place) = cleanup_stack.pop() {
            self.run_cleanup_place(&place, env, cancel_before_cleanup)?;
        }
        Ok(())
    }

    fn run_cleanup_place(
        &mut self,
        place: &str,
        env: &mut Env,
        cancel_before_cleanup: bool,
    ) -> Result<()> {
        let resource = env.read_place(place)?;
        match resource {
            Value::TaskGroup(group) => self.close_task_group(group, cancel_before_cleanup),
            Value::Instance(instance) => {
                let class = self
                    .classes
                    .get(&instance.class_name)
                    .cloned()
                    .ok_or_else(|| {
                        Diagnostic::new(format!("unknown MIR class `{}`", instance.class_name))
                    })?;
                let method = class
                    .methods
                    .iter()
                    .find(|method| method.name == "close")
                    .cloned()
                    .ok_or_else(|| {
                        Diagnostic::new(format!(
                            "class `{}` cannot be used with MIR `with` because it has no `close` method",
                            class.name
                        ))
                    })?;
                let function = self
                    .functions
                    .get(&method.function_name)
                    .cloned()
                    .ok_or_else(|| {
                        Diagnostic::new(format!(
                            "unknown MIR method body `{}`",
                            method.function_name
                        ))
                    })?;
                let outcome =
                    self.call_function(&function, Some(Value::Instance(instance)), Vec::new())?;
                if let Some(updated_receiver) = outcome.updated_receiver {
                    env.write_place(place, updated_receiver)?;
                }
                Ok(())
            }
            _ => Err(Diagnostic::new(format!(
                "MIR cleanup place `{}` is not a managed resource",
                place
            ))),
        }
    }

    fn evaluate_rvalue(&mut self, value: &Rvalue, env: &mut Env) -> Result<RvalueOutcome> {
        match value {
            Rvalue::Use(operand) => Ok(RvalueOutcome::Value(self.evaluate_operand(operand, env)?)),
            Rvalue::Unary { op, value, .. } => {
                let value = self.evaluate_operand(value, env)?;
                let result = match (op, value) {
                    (UnaryOp::Not, Value::Bool(value)) => Value::Bool(!value),
                    (UnaryOp::Neg, Value::Int(value)) => Value::Int(-value),
                    (UnaryOp::Neg, Value::Float(value)) => Value::Float(-value),
                    (UnaryOp::Not, other) => {
                        return Err(Diagnostic::new(format!(
                            "`not` expects `bool`, found `{}`",
                            other.render()
                        )))
                    }
                    (UnaryOp::Neg, other) => {
                        return Err(Diagnostic::new(format!(
                            "unary `-` expects a numeric value, found `{}`",
                            other.render()
                        )))
                    }
                };
                Ok(RvalueOutcome::Value(result))
            }
            Rvalue::Cast { value, ty, span } => {
                let value = self.evaluate_operand(value, env)?;
                Ok(RvalueOutcome::Value(cast_numeric_value(
                    value,
                    ty,
                    Some(*span),
                )?))
            }
            Rvalue::Try { value } => {
                let value = self.evaluate_operand(value, env)?;
                let Value::EnumVariant(variant) = value else {
                    return Err(Diagnostic::new(
                        "MIR `try` requires a `Result` value at runtime",
                    ));
                };
                if variant.enum_name != "Result" {
                    return Err(Diagnostic::new(
                        "MIR `try` requires a `Result` value at runtime",
                    ));
                }
                match (variant.variant_name.as_str(), variant.payload) {
                    ("Ok", Some(payload)) => Ok(RvalueOutcome::Value(*payload)),
                    ("Err", Some(payload)) => Ok(RvalueOutcome::Return(Value::EnumVariant(
                        EnumVariantValue {
                            enum_name: "Result".to_string(),
                            variant_name: "Err".to_string(),
                            payload: Some(payload),
                        },
                    ))),
                    _ => Err(Diagnostic::new(
                        "MIR `try` encountered an invalid `Result` payload at runtime",
                    )),
                }
            }
            Rvalue::Spawn {
                detached,
                task_group,
                function,
                args,
            } => Ok(RvalueOutcome::Value(self.spawn_function(
                *detached,
                task_group.as_ref(),
                function,
                args,
                env,
            )?)),
            Rvalue::Binary {
                op,
                left,
                right,
                span,
            } => {
                let left = self.evaluate_operand(left, env)?;
                let right = self.evaluate_operand(right, env)?;
                Ok(RvalueOutcome::Value(self.eval_binary(
                    *op,
                    left,
                    right,
                    Some(*span),
                )?))
            }
            Rvalue::Call { callee, args } => {
                Ok(RvalueOutcome::Value(self.evaluate_call(callee, args, env)?))
            }
            Rvalue::Construct { class_name, fields } => {
                let mut values = BTreeMap::new();
                for field in fields {
                    values.insert(
                        field.name.clone(),
                        self.evaluate_operand(&field.value, env)?,
                    );
                }
                Ok(RvalueOutcome::Value(Value::Instance(InstanceValue {
                    class_name: class_name.clone(),
                    fields: values,
                })))
            }
            Rvalue::EnumVariant {
                enum_name,
                variant_name,
                payload,
            } => Ok(RvalueOutcome::Value(Value::EnumVariant(EnumVariantValue {
                enum_name: enum_name.clone(),
                variant_name: variant_name.clone(),
                payload: payload
                    .as_ref()
                    .map(|payload| self.evaluate_operand(payload, env))
                    .transpose()?
                    .map(Box::new),
            }))),
            Rvalue::VariantPayload { scrutinee } => {
                let scrutinee = self.evaluate_operand(scrutinee, env)?;
                let Value::EnumVariant(variant) = scrutinee else {
                    return Err(Diagnostic::new(format!(
                        "MIR variant payload extraction expected an enum value, found `{}`",
                        scrutinee.render()
                    )));
                };
                let payload = variant.payload.map(|payload| *payload).ok_or_else(|| {
                    Diagnostic::new(format!(
                        "enum variant `{}.{}` does not carry a payload",
                        variant.enum_name, variant.variant_name
                    ))
                })?;
                Ok(RvalueOutcome::Value(payload))
            }
            Rvalue::Member { object, field } => {
                let object = self.evaluate_operand(object, env)?;
                let Value::Instance(instance) = object else {
                    return Err(Diagnostic::new(format!(
                        "cannot access field `{}` on non-instance value `{}`",
                        field,
                        object.render()
                    )));
                };
                let value = instance.fields.get(field).cloned().ok_or_else(|| {
                    Diagnostic::new(format!(
                        "class `{}` has no field `{}`",
                        instance.class_name, field
                    ))
                })?;
                Ok(RvalueOutcome::Value(value))
            }
        }
    }

    fn evaluate_call(
        &mut self,
        callee: &CallTarget,
        args: &[MirArg],
        env: &mut Env,
    ) -> Result<Value> {
        match callee {
            CallTarget::Name(name) => {
                if name == "print" {
                    let values = evaluate_named_args(args, env)?;
                    let bound = bind_builtin_args(&["value"], values)?;
                    let rendered = bound[0].value.render();
                    self.stdout.lock().unwrap().push_str(&rendered);
                    self.stdout.lock().unwrap().push('\n');
                    return Ok(Value::Unit);
                }

                if name == "range" {
                    let values = evaluate_named_args(args, env)?;
                    return build_range(values);
                }

                if name == "channel" {
                    let values = evaluate_named_args(args, env)?;
                    bind_builtin_args(&[], values)?;
                    return Ok(Value::Channel(ChannelValue::new()));
                }

                if name == "task_group" {
                    let values = evaluate_named_args(args, env)?;
                    bind_builtin_args(&[], values)?;
                    return Ok(Value::TaskGroup(TaskGroupValue::new(&self.cancellation)));
                }

                if name == "cancelled" {
                    let values = evaluate_named_args(args, env)?;
                    bind_builtin_args(&[], values)?;
                    return Ok(Value::Bool(self.cancellation.is_cancelled()));
                }

                if name == "after" {
                    let values = evaluate_named_args(args, env)?;
                    let bound = bind_builtin_args(&["duration"], values)?;
                    let duration = match bound[0].value.clone() {
                        Value::Int(duration) | Value::Duration(duration) => duration,
                        _ => {
                            return Err(Diagnostic::new(
                                "`after(...)` expects a duration value in MIR runtime",
                            ))
                        }
                    };
                    return Ok(Value::Duration(duration));
                }

                if name == "sleep" {
                    let values = evaluate_named_args(args, env)?;
                    let bound = bind_builtin_args(&["duration"], values)?;
                    let duration = match bound[0].value.clone() {
                        Value::Int(duration) | Value::Duration(duration) => duration,
                        _ => {
                            return Err(Diagnostic::new(
                                "`sleep(...)` expects a duration value in MIR runtime",
                            ))
                        }
                    };
                    let duration = u64::try_from(duration).map_err(|_| {
                        Diagnostic::new(format!(
                            "duration `{}ms` does not fit in the MIR runtime timer range",
                            duration
                        ))
                    })?;
                    std::thread::sleep(std::time::Duration::from_millis(duration));
                    return Ok(Value::Unit);
                }

                let function =
                    self.functions.get(name).cloned().ok_or_else(|| {
                        Diagnostic::new(format!("unknown MIR function `{}`", name))
                    })?;
                let evaluated_args = evaluate_named_args(args, env)?;
                let outcome = self.call_function(&function, None, evaluated_args.clone())?;
                self.apply_borrowed_param_writebacks(
                    &function.params,
                    &evaluated_args,
                    &outcome.updated_params,
                    env,
                )?;
                Ok(outcome.value)
            }
            CallTarget::Member {
                object,
                field,
                receiver_place,
            } => {
                let receiver = self.evaluate_operand(object, env)?;

                match &receiver {
                    Value::Float(value) if field == "sqrt" => {
                        if !args.is_empty() {
                            return Err(Diagnostic::new("`sqrt` does not take arguments"));
                        }
                        return Ok(Value::Float(value.sqrt()));
                    }
                    Value::String(value) if field == "clone" => {
                        if !args.is_empty() {
                            return Err(Diagnostic::new("`clone` does not take arguments"));
                        }
                        return Ok(Value::String(value.clone()));
                    }
                    Value::Channel(channel) => {
                        return self.evaluate_channel_method(channel.clone(), field, args, env);
                    }
                    Value::Task(task) => {
                        return self.evaluate_task_method(task.clone(), field, args);
                    }
                    Value::TaskGroup(group) => {
                        return self.evaluate_task_group_method(group.clone(), field, args, env);
                    }
                    Value::Instance(instance) => {
                        let resolved_receiver_ty = receiver_place
                            .as_ref()
                            .and_then(|place| self.resolve_place_type(place, env))
                            .filter(|ty| !matches!(ty, Type::TypeParam(_)))
                            .unwrap_or_else(|| Type::named(&instance.class_name));
                        let class =
                            self.classes
                                .get(&instance.class_name)
                                .cloned()
                                .ok_or_else(|| {
                                    Diagnostic::new(format!(
                                        "unknown MIR class `{}`",
                                        instance.class_name
                                    ))
                                })?;
                        let method = class
                            .methods
                            .iter()
                            .find(|method| method.name == *field)
                            .cloned()
                            .or_else(|| {
                                self.find_trait_impl_method(&resolved_receiver_ty, field)
                                    .or_else(|| {
                                        self.find_trait_impl_method_for_class_name(
                                            &instance.class_name,
                                            field,
                                        )
                                    })
                                    .cloned()
                            })
                            .ok_or_else(|| {
                                Diagnostic::new(format!(
                                    "class `{}` has no MIR method `{}`",
                                    class.name, field
                                ))
                            })?;
                        let function = self
                            .functions
                            .get(&method.function_name)
                            .cloned()
                            .ok_or_else(|| {
                                Diagnostic::new(format!(
                                    "unknown MIR method body `{}`",
                                    method.function_name
                                ))
                            })?;
                        let evaluated_args = evaluate_named_args(args, env)?;
                        let outcome = self.call_function(
                            &function,
                            Some(receiver.clone()),
                            evaluated_args.clone(),
                        )?;
                        if method.receiver == Some(MirReceiverKind::BorrowMut) {
                            let updated = outcome.updated_receiver.ok_or_else(|| {
                                Diagnostic::new(format!(
                                    "mutable MIR method `{}` did not return an updated receiver",
                                    field
                                ))
                            })?;
                            if let Some(place) = receiver_place {
                                env.write_place(place, updated)?;
                            }
                        }
                        self.apply_borrowed_param_writebacks(
                            &function.params,
                            &evaluated_args,
                            &outcome.updated_params,
                            env,
                        )?;
                        Ok(outcome.value)
                    }
                    _ => Err(Diagnostic::new(format!(
                        "unsupported MIR member call `{}` on `{}`",
                        field,
                        receiver.render()
                    ))),
                }
            }
        }
    }

    fn spawn_function(
        &mut self,
        detached: bool,
        task_group: Option<&Operand>,
        function: &str,
        args: &[MirArg],
        env: &Env,
    ) -> Result<Value> {
        let function = self
            .functions
            .get(function)
            .cloned()
            .ok_or_else(|| Diagnostic::new(format!("unknown MIR function `{}`", function)))?;
        self.require_spawnable_function(&function)?;
        let bound_args = evaluate_named_args(args, env)?;

        let group_value = if let Some(group) = task_group {
            let value = self.evaluate_operand(group, env)?;
            let Value::TaskGroup(group) = value else {
                return Err(Diagnostic::new(
                    "MIR task-group spawn requires a task-group value",
                ));
            };
            Some(group)
        } else {
            None
        };

        let cancellation = if let Some(group) = &group_value {
            group.child_cancellation()
        } else if detached {
            CancellationContext::default()
        } else {
            self.cancellation.clone()
        };

        let module = (*self.module).clone();
        let stdout = self.stdout.clone();
        let function_for_thread = function.clone();
        let handle = thread::spawn(move || {
            let mut runtime = MirRuntime::new(module, stdout, cancellation);
            runtime
                .call_function(&function_for_thread, None, bound_args)
                .map(|outcome| outcome.value)
                .map_err(|error| error.message)
        });

        let task = TaskValue::from_handle(handle);
        if let Some(group) = group_value {
            group.register_task(task.clone());
        }

        if detached {
            Ok(Value::Unit)
        } else {
            Ok(Value::Task(task))
        }
    }

    fn apply_borrowed_param_writebacks(
        &mut self,
        params: &[MirParam],
        evaluated_args: &[EvaluatedMirArg],
        updated_params: &[(usize, Value)],
        env: &mut Env,
    ) -> Result<()> {
        for (index, value) in updated_params {
            let Some(param) = params.get(*index) else {
                continue;
            };
            if param.passing != MirReceiverKind::BorrowMut {
                continue;
            }
            let place = evaluated_args
                .get(*index)
                .and_then(|argument| argument.writeback_place.as_deref())
                .ok_or_else(|| {
                    Diagnostic::new(format!(
                        "mutable borrowed MIR parameter `{}` requires a writeback place",
                        param.name
                    ))
                })?;
            env.write_place(place, value.clone())?;
        }
        Ok(())
    }

    fn require_spawnable_function(&self, function: &MirFunction) -> Result<()> {
        if let Some(param) = function
            .params
            .iter()
            .find(|param| param.passing != MirReceiverKind::Value)
        {
            return Err(Diagnostic::new(format!(
                "`spawn` does not yet support borrowed parameter `{}` on function `{}` in MIR runtime",
                param.name, function.name
            )));
        }
        Ok(())
    }

    fn evaluate_channel_method(
        &mut self,
        channel: ChannelValue,
        field: &str,
        args: &[MirArg],
        env: &Env,
    ) -> Result<Value> {
        match field {
            "clone" => {
                if !args.is_empty() {
                    return Err(Diagnostic::new("`clone` does not take arguments"));
                }
                Ok(Value::Channel(channel))
            }
            "send" => {
                let values = evaluate_named_args(args, env)?;
                let bound = bind_builtin_args(&["value"], values)?;
                let value = bound
                    .into_iter()
                    .next()
                    .expect("send should bind one arg")
                    .value;
                match channel.send(value) {
                    Ok(()) => Ok(result_ok(Value::Unit)),
                    Err(value) => Ok(result_err(send_error_closed(value))),
                }
            }
            "recv" => {
                if !args.is_empty() {
                    return Err(Diagnostic::new("`recv` does not take arguments"));
                }
                Ok(match channel.recv_blocking() {
                    Some(value) => option_some(value),
                    None => option_none(),
                })
            }
            "close" => {
                if !args.is_empty() {
                    return Err(Diagnostic::new("`close` does not take arguments"));
                }
                channel.close();
                Ok(Value::Unit)
            }
            _ => Err(Diagnostic::new(format!(
                "unsupported channel method `{}`",
                field
            ))),
        }
    }

    fn evaluate_task_method(
        &mut self,
        task: TaskValue,
        field: &str,
        args: &[MirArg],
    ) -> Result<Value> {
        match field {
            "clone" => {
                if !args.is_empty() {
                    return Err(Diagnostic::new("`clone` does not take arguments"));
                }
                Ok(Value::Task(task))
            }
            "join" => {
                if !args.is_empty() {
                    return Err(Diagnostic::new("`join` does not take arguments"));
                }
                self.join_task(task)
            }
            _ => Err(Diagnostic::new(format!(
                "unsupported task method `{}`",
                field
            ))),
        }
    }

    fn evaluate_task_group_method(
        &mut self,
        group: TaskGroupValue,
        field: &str,
        args: &[MirArg],
        env: &Env,
    ) -> Result<Value> {
        match field {
            "cancel" => {
                if !args.is_empty() {
                    return Err(Diagnostic::new("`cancel` does not take arguments"));
                }
                group.cancel();
                Ok(Value::Unit)
            }
            "spawn" => {
                if args.is_empty() {
                    return Err(Diagnostic::new(
                        "`spawn` expects a target function followed by its arguments",
                    ));
                }
                Err(Diagnostic::new(
                    "task-group spawn should lower to MIR `Spawn` directly",
                ))
            }
            _ => {
                let _ = env;
                Err(Diagnostic::new(format!(
                    "unsupported task-group method `{}`",
                    field
                )))
            }
        }
    }

    fn join_task(&mut self, task: TaskValue) -> Result<Value> {
        task.join_result().map_err(Diagnostic::new)
    }

    fn close_task_group(
        &mut self,
        group: TaskGroupValue,
        cancel_before_cleanup: bool,
    ) -> Result<()> {
        if cancel_before_cleanup {
            group.cancel();
        }

        let mut first_error = None;
        for task in group.drain_tasks() {
            if let Err(error) = self.join_task(task) {
                group.cancel();
                if first_error.is_none() {
                    first_error = Some(error);
                }
            }
        }

        if let Some(error) = first_error {
            return Err(error);
        }

        Ok(())
    }

    fn execute_select(
        &mut self,
        arms: &[crate::mir::MirSelectArm],
        env: &mut Env,
    ) -> Result<BlockOutcome> {
        let deadlines = arms
            .iter()
            .map(|arm| match &arm.kind {
                MirSelectKind::After { duration } => {
                    let value = self.evaluate_operand(duration, env)?;
                    let millis = match value {
                        Value::Int(value) => value,
                        Value::Duration(value) => value,
                        other => {
                            return Err(Diagnostic::new(format!(
                                "MIR `after(...)` expects a duration-like value, found `{}`",
                                other.render()
                            )))
                        }
                    };
                    let millis = u64::try_from(millis).map_err(|_| {
                        Diagnostic::new(format!(
                            "duration `{}ms` does not fit in the MIR runtime timer range",
                            millis
                        ))
                    })?;
                    Ok(Some(
                        Instant::now()
                            .checked_add(StdDuration::from_millis(millis))
                            .unwrap_or_else(Instant::now),
                    ))
                }
                _ => Ok(None),
            })
            .collect::<Result<Vec<_>>>()?;

        loop {
            for (index, arm) in arms.iter().enumerate() {
                if let Some(value) = self.try_select_arm(arm, env, deadlines[index])? {
                    if let Some(binding) = &arm.binding {
                        env.write_place(binding, value)?;
                    }
                    return Ok(BlockOutcome::Goto(arm.label.clone()));
                }
            }
            thread::sleep(StdDuration::from_millis(1));
        }
    }

    fn try_select_arm(
        &mut self,
        arm: &crate::mir::MirSelectArm,
        env: &mut Env,
        deadline: Option<Instant>,
    ) -> Result<Option<Value>> {
        match &arm.kind {
            MirSelectKind::After { .. } => {
                if let Some(deadline) = deadline {
                    if Instant::now() >= deadline {
                        return Ok(Some(Value::Unit));
                    }
                }
                Ok(None)
            }
            MirSelectKind::Recv { channel } => {
                let channel = self.evaluate_operand(channel, env)?;
                let Value::Channel(channel) = channel else {
                    return Err(Diagnostic::new(
                        "MIR `select` recv arm requires a channel value",
                    ));
                };
                match channel.try_recv() {
                    TryRecvResult::Value(value) => Ok(Some(option_some(value))),
                    TryRecvResult::Closed => Ok(Some(option_none())),
                    TryRecvResult::Empty => Ok(None),
                }
            }
            MirSelectKind::Send { channel, value } => {
                let channel = self.evaluate_operand(channel, env)?;
                let Value::Channel(channel) = channel else {
                    return Err(Diagnostic::new(
                        "MIR `select` send arm requires a channel value",
                    ));
                };
                let value = self.evaluate_operand(value, env)?;
                Ok(Some(match channel.send(value) {
                    Ok(()) => result_ok(Value::Unit),
                    Err(value) => result_err(send_error_closed(value)),
                }))
            }
        }
    }

    fn evaluate_operand(&self, operand: &Operand, env: &Env) -> Result<Value> {
        match operand {
            Operand::Place(place) => env.read_place(place),
            Operand::Int(value) => Ok(Value::Int(*value)),
            Operand::Duration(value) => Ok(Value::Duration(*value)),
            Operand::Float(value) => Ok(Value::Float(*value)),
            Operand::Bool(value) => Ok(Value::Bool(*value)),
            Operand::String(value) => Ok(Value::String(value.clone())),
            Operand::Unit => Ok(Value::Unit),
        }
    }

    fn eval_binary(
        &self,
        op: crate::ast::BinaryOp,
        left: Value,
        right: Value,
        span: Option<crate::diag::Span>,
    ) -> Result<Value> {
        use crate::ast::BinaryOp;

        match op {
            BinaryOp::And => match (left, right) {
                (Value::Bool(left), Value::Bool(right)) => Ok(Value::Bool(left && right)),
                _ => Err(Diagnostic::new(
                    "MIR logical operands must both have type `bool`",
                )),
            },
            BinaryOp::Or => match (left, right) {
                (Value::Bool(left), Value::Bool(right)) => Ok(Value::Bool(left || right)),
                _ => Err(Diagnostic::new(
                    "MIR logical operands must both have type `bool`",
                )),
            },
            BinaryOp::Eq => Ok(Value::Bool(left == right)),
            BinaryOp::NotEq => Ok(Value::Bool(left != right)),
            BinaryOp::Add => match (left, right) {
                (Value::Int(left), Value::Int(right)) => {
                    left.checked_add(right).map(Value::Int).ok_or_else(|| {
                        span.map_or_else(
                            || Diagnostic::new("integer overflow"),
                            |span| Diagnostic::at(span, "integer overflow"),
                        )
                    })
                }
                (Value::Float(left), Value::Float(right)) => Ok(Value::Float(left + right)),
                (Value::String(left), Value::String(right)) => Ok(Value::String(left + &right)),
                _ => Err(Diagnostic::new(
                    "MIR binary add requires matching supported operand types",
                )),
            },
            BinaryOp::Sub => match (left, right) {
                (Value::Int(left), Value::Int(right)) => {
                    left.checked_sub(right).map(Value::Int).ok_or_else(|| {
                        span.map_or_else(
                            || Diagnostic::new("integer overflow"),
                            |span| Diagnostic::at(span, "integer overflow"),
                        )
                    })
                }
                (Value::Float(left), Value::Float(right)) => Ok(Value::Float(left - right)),
                _ => Err(Diagnostic::new(
                    "MIR binary subtraction requires matching numeric operands",
                )),
            },
            BinaryOp::Mul => match (left, right) {
                (Value::Int(left), Value::Int(right)) => {
                    left.checked_mul(right).map(Value::Int).ok_or_else(|| {
                        span.map_or_else(
                            || Diagnostic::new("integer overflow"),
                            |span| Diagnostic::at(span, "integer overflow"),
                        )
                    })
                }
                (Value::Float(left), Value::Float(right)) => Ok(Value::Float(left * right)),
                _ => Err(Diagnostic::new(
                    "MIR binary multiplication requires matching numeric operands",
                )),
            },
            BinaryOp::Div => match (left, right) {
                (Value::Int(_left), Value::Int(0)) => Err(span.map_or_else(
                    || Diagnostic::new("division by zero"),
                    |span| Diagnostic::at(span, "division by zero"),
                )),
                (Value::Int(left), Value::Int(right)) => {
                    left.checked_div(right).map(Value::Int).ok_or_else(|| {
                        span.map_or_else(
                            || Diagnostic::new("integer overflow"),
                            |span| Diagnostic::at(span, "integer overflow"),
                        )
                    })
                }
                (Value::Float(left), Value::Float(right)) => Ok(Value::Float(left / right)),
                _ => Err(Diagnostic::new(
                    "MIR binary division requires matching numeric operands",
                )),
            },
            BinaryOp::Mod => match (left, right) {
                (Value::Int(_left), Value::Int(0)) => Err(span.map_or_else(
                    || Diagnostic::new("division by zero"),
                    |span| Diagnostic::at(span, "division by zero"),
                )),
                (Value::Int(left), Value::Int(right)) => {
                    left.checked_rem(right).map(Value::Int).ok_or_else(|| {
                        span.map_or_else(
                            || Diagnostic::new("integer overflow"),
                            |span| Diagnostic::at(span, "integer overflow"),
                        )
                    })
                }
                (Value::Float(left), Value::Float(right)) => Ok(Value::Float(left % right)),
                _ => Err(Diagnostic::new(
                    "MIR binary remainder requires matching numeric operands",
                )),
            },
            BinaryOp::Less => eval_ordering(
                left,
                right,
                |left, right| left < right,
                |left, right| left < right,
            ),
            BinaryOp::LessEq => eval_ordering(
                left,
                right,
                |left, right| left <= right,
                |left, right| left <= right,
            ),
            BinaryOp::Greater => eval_ordering(
                left,
                right,
                |left, right| left > right,
                |left, right| left > right,
            ),
            BinaryOp::GreaterEq => eval_ordering(
                left,
                right,
                |left, right| left >= right,
                |left, right| left >= right,
            ),
        }
    }
}

enum BlockOutcome {
    Return(Value),
    Goto(String),
}

fn option_some(value: Value) -> Value {
    Value::EnumVariant(EnumVariantValue {
        enum_name: "Option".to_string(),
        variant_name: "Some".to_string(),
        payload: Some(Box::new(value)),
    })
}

fn option_none() -> Value {
    Value::EnumVariant(EnumVariantValue {
        enum_name: "Option".to_string(),
        variant_name: "None".to_string(),
        payload: None,
    })
}

fn result_ok(value: Value) -> Value {
    Value::EnumVariant(EnumVariantValue {
        enum_name: "Result".to_string(),
        variant_name: "Ok".to_string(),
        payload: Some(Box::new(value)),
    })
}

fn result_err(value: Value) -> Value {
    Value::EnumVariant(EnumVariantValue {
        enum_name: "Result".to_string(),
        variant_name: "Err".to_string(),
        payload: Some(Box::new(value)),
    })
}

fn send_error_closed(value: Value) -> Value {
    Value::EnumVariant(EnumVariantValue {
        enum_name: "SendError".to_string(),
        variant_name: "Closed".to_string(),
        payload: Some(Box::new(value)),
    })
}

fn evaluate_named_args(args: &[MirArg], env: &Env) -> Result<Vec<EvaluatedMirArg>> {
    args.iter()
        .map(|arg| {
            let value = match &arg.value {
                Operand::Place(place) => env.read_place(place)?,
                Operand::Int(value) => Value::Int(*value),
                Operand::Duration(value) => Value::Duration(*value),
                Operand::Float(value) => Value::Float(*value),
                Operand::Bool(value) => Value::Bool(*value),
                Operand::String(value) => Value::String(value.clone()),
                Operand::Unit => Value::Unit,
            };
            Ok(EvaluatedMirArg {
                name: arg.name.clone(),
                value,
                writeback_place: arg.writeback_place.clone(),
            })
        })
        .collect()
}

fn bind_args(params: &[MirParam], args: Vec<EvaluatedMirArg>) -> Result<Vec<EvaluatedMirArg>> {
    let names = params
        .iter()
        .map(|param| param.name.as_str())
        .collect::<Vec<_>>();
    bind_builtin_args(&names, args)
}

fn bind_builtin_args(
    expected_names: &[&str],
    args: Vec<EvaluatedMirArg>,
) -> Result<Vec<EvaluatedMirArg>> {
    let mut values = vec![None; expected_names.len()];
    let mut next_positional = 0;

    for argument in args {
        let EvaluatedMirArg {
            name,
            value,
            writeback_place,
        } = argument;
        if let Some(name) = name {
            let Some(index) = expected_names
                .iter()
                .position(|candidate| *candidate == name)
            else {
                return Err(Diagnostic::new(format!("unknown MIR argument `{}`", name)));
            };
            values[index] = Some(EvaluatedMirArg {
                name: Some(name),
                value,
                writeback_place,
            });
            continue;
        }

        while next_positional < values.len() && values[next_positional].is_some() {
            next_positional += 1;
        }
        if next_positional >= values.len() {
            return Err(Diagnostic::new("too many MIR arguments"));
        }
        values[next_positional] = Some(EvaluatedMirArg {
            name: None,
            value,
            writeback_place,
        });
        next_positional += 1;
    }

    values
        .into_iter()
        .map(|value| value.ok_or_else(|| Diagnostic::new("missing MIR argument")))
        .collect()
}

fn build_range(args: Vec<EvaluatedMirArg>) -> Result<Value> {
    let mut start = None;
    let mut stop = None;
    let mut next_positional = 0;

    for argument in args {
        let EvaluatedMirArg { name, value, .. } = argument;
        let Value::Int(value) = value else {
            return Err(Diagnostic::new(
                "`range` requires integer arguments in MIR runtime",
            ));
        };
        match name.as_deref() {
            Some("start") => start = Some(value),
            Some("stop") => stop = Some(value),
            Some(other) => {
                return Err(Diagnostic::new(format!(
                    "unknown MIR `range` argument `{}`",
                    other
                )))
            }
            None => {
                if next_positional == 0 {
                    stop = Some(value);
                } else if next_positional == 1 {
                    start = stop.take();
                    stop = Some(value);
                } else {
                    return Err(Diagnostic::new("`range` takes at most two arguments"));
                }
                next_positional += 1;
            }
        }
    }

    let (start, stop) = match (start, stop) {
        (Some(start), Some(stop)) => (start, stop),
        (None, Some(stop)) => (0, stop),
        _ => return Err(Diagnostic::new("`range` requires `stop` in MIR runtime")),
    };

    Ok(Value::Range(RangeValue { start, end: stop }))
}

fn eval_ordering(
    left: Value,
    right: Value,
    compare_int: impl FnOnce(i128, i128) -> bool + Copy,
    compare_float: impl FnOnce(f64, f64) -> bool + Copy,
) -> Result<Value> {
    match (left, right) {
        (Value::Int(left), Value::Int(right)) => Ok(Value::Bool(compare_int(left, right))),
        (Value::Float(left), Value::Float(right)) => Ok(Value::Bool(compare_float(left, right))),
        _ => Err(Diagnostic::new(
            "MIR ordering comparisons require matching numeric operands",
        )),
    }
}
