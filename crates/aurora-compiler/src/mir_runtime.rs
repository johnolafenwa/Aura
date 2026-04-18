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
use crate::integer::IntegerValue;
use crate::mir::{
    CallTarget, Instruction, MirArg, MirClass, MirFormatPart, MirFunction, MirMethod, MirModule,
    MirParam, MirReceiverKind, MirSelectKind, MirTraitImpl, Operand, Rvalue, Terminator,
};
use crate::runtime_value::{
    cast_numeric_value, option_none, option_some, result_err, result_ok, send_error_closed,
    CancellationContext, ChannelValue, EnumVariantValue, InstanceValue, MapValue, RangeValue,
    RunOutput, SetValue, TaskGroupValue, TaskValue, TryRecvResult, Value, VecValue,
};
use crate::sema::{substitute_type, Type};

pub fn run(module: &MirModule) -> Result<RunOutput> {
    let module = module.clone();
    let handle = match thread::Builder::new()
        .name("aurora-mir-runtime".to_string())
        .stack_size(64 * 1024 * 1024)
        .spawn(move || {
            let stdout = Arc::new(Mutex::new(String::new()));
            let mut runtime =
                MirRuntime::new(module, stdout.clone(), CancellationContext::default());
            let value = runtime.run_main()?;
            let rendered_stdout = lock_stdout(&stdout).clone();
            Ok(RunOutput {
                value,
                stdout: rendered_stdout,
            })
        }) {
        Ok(handle) => handle,
        Err(error) => {
            return Err(Diagnostic::new(format!(
                "failed to start Aurora MIR runtime thread: {}",
                error
            )))
        }
    };
    match handle.join() {
        Ok(result) => result,
        Err(_) => Err(Diagnostic::new(
            "Aurora MIR runtime panicked while executing the program",
        )),
    }
}

fn lock_stdout(stdout: &Arc<Mutex<String>>) -> std::sync::MutexGuard<'_, String> {
    stdout
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

pub fn run_serialized_mir(mir_json: &[u8], source_path: &str, source: &str) -> Result<RunOutput> {
    let module = match serde_json::from_slice::<MirModule>(mir_json) {
        Ok(module) => module,
        Err(error) => {
            return Err(Diagnostic::new(format!(
                "failed to deserialize embedded MIR: {}",
                error
            )))
        }
    };
    validate_runtime_module_complexity(&module)?;
    let _ = source_path;
    let _ = source;
    run(&module)
}

const MAX_CALL_DEPTH: usize = 1024;
const MAX_EMBEDDED_RUNTIME_BYTES: usize = 1 << 30;
const MAX_RUNTIME_BLOCKS: usize = 1_000_000;
const MAX_RUNTIME_INSTRUCTIONS: usize = 1_000_000;
const MAX_RUNTIME_TERMINATOR_ARMS: usize = 1_000_000;

fn render_runtime_error(path: &str, source: &str, error: &Diagnostic) -> String {
    error.render_with_source(path, source)
}

fn write_stream(mut stream: impl Write, text: &str) -> io::Result<()> {
    stream.write_all(text.as_bytes())?;
    stream.flush()
}

fn validate_embedded_runtime_length(name: &str, len: usize) -> std::result::Result<(), String> {
    if len > MAX_EMBEDDED_RUNTIME_BYTES {
        return Err(format!(
            "embedded {} length {} exceeds the supported runtime limit of {} bytes",
            name, len, MAX_EMBEDDED_RUNTIME_BYTES
        ));
    }
    Ok(())
}

fn validate_runtime_module_complexity(module: &MirModule) -> Result<()> {
    let mut total_blocks = 0usize;
    let mut total_instructions = 0usize;
    let mut total_arms = 0usize;
    for function in module.functions.iter().chain(module.top_level.iter()) {
        total_blocks = total_blocks.saturating_add(function.blocks.len());
        if total_blocks > MAX_RUNTIME_BLOCKS {
            return Err(Diagnostic::new(format!(
                "embedded MIR exceeds the supported block limit of {}",
                MAX_RUNTIME_BLOCKS
            )));
        }
        for block in &function.blocks {
            total_instructions = total_instructions.saturating_add(block.instructions.len());
            if total_instructions > MAX_RUNTIME_INSTRUCTIONS {
                return Err(Diagnostic::new(format!(
                    "embedded MIR exceeds the supported instruction limit of {}",
                    MAX_RUNTIME_INSTRUCTIONS
                )));
            }
            total_arms = total_arms.saturating_add(match &block.terminator {
                Terminator::Match { arms, .. } => arms.len(),
                Terminator::Select { arms, .. } => arms.len(),
                _ => 0,
            });
            if total_arms > MAX_RUNTIME_TERMINATOR_ARMS {
                return Err(Diagnostic::new(format!(
                    "embedded MIR exceeds the supported branching-arm limit of {}",
                    MAX_RUNTIME_TERMINATOR_ARMS
                )));
            }
        }
    }
    Ok(())
}

fn deadline_after_millis_with(
    millis: u64,
    checked_add: impl FnOnce(StdDuration) -> Option<Instant>,
) -> Result<Instant> {
    checked_add(StdDuration::from_millis(millis)).ok_or_else(|| {
        Diagnostic::new(format!(
            "duration `{}ms` overflows the MIR runtime deadline range",
            millis
        ))
    })
}

fn deadline_after_millis(millis: u64) -> Result<Instant> {
    deadline_after_millis_with(millis, |duration| Instant::now().checked_add(duration))
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
                return code.as_i128().unwrap_or(0) as i32;
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
/// # Safety
///
/// `mir_ptr`, `source_path_ptr`, and `source_ptr` must either be valid for reads of their paired
/// lengths or be null when the paired length is zero. The byte buffers must remain alive for the
/// duration of this call and must point to valid UTF-8 for the embedded source path/source
/// payloads.
pub unsafe extern "C" fn aurora_native_run(
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

        for (name, len) in [
            ("MIR payload", mir_len),
            ("source path", source_path_len),
            ("source payload", source_len),
        ] {
            if let Err(message) = validate_embedded_runtime_length(name, len) {
                let _ = writeln!(io::stderr().lock(), "{}", message);
                return 1;
            }
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
        Err(payload) => {
            let message = if let Some(text) = payload.downcast_ref::<&str>() {
                text.to_string()
            } else if let Some(text) = payload.downcast_ref::<String>() {
                text.clone()
            } else {
                "unknown panic".to_string()
            };
            let _ = writeln!(
                io::stderr().lock(),
                "aurora native runtime panicked: {}",
                message
            );
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
    call_depth: usize,
    return_type_stack: Vec<Type>,
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
        let Some(root) = segments.next() else {
            return Err(Diagnostic::new("empty MIR place"));
        };
        let mut value = match self.values.get(root).cloned() {
            Some(value) => value,
            None => return Err(Diagnostic::new(format!("unknown MIR place `{}`", place))),
        };
        for segment in segments {
            let Value::Instance(instance) = value else {
                return Err(Diagnostic::new(format!(
                    "cannot access field `{}` on non-instance MIR place `{}`",
                    segment, place
                )));
            };
            value = match instance.fields.get(segment).cloned() {
                Some(value) => value,
                None => {
                    return Err(Diagnostic::new(format!(
                        "class `{}` has no field `{}` in MIR place `{}`",
                        instance.class_name, segment, place
                    )))
                }
            };
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

        let mut root_value = match self.values.get(*root).cloned() {
            Some(value) => value,
            None => return Err(Diagnostic::new(format!("unknown MIR place `{}`", place))),
        };
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

    let child = match instance.fields.get_mut(segments[0]) {
        Some(child) => child,
        None => {
            return Err(Diagnostic::new(format!(
                "class `{}` has no field `{}` in MIR place `{}`",
                instance.class_name, segments[0], full_place
            )))
        }
    };
    write_nested_place(child, &segments[1..], value, full_place)
}

impl MirRuntime {
    fn new(
        module: MirModule,
        stdout: Arc<Mutex<String>>,
        cancellation: CancellationContext,
    ) -> Self {
        let mut functions = HashMap::new();
        for function in &module.functions {
            functions.insert(function.name.clone(), function.clone());
        }
        let mut classes = HashMap::new();
        for class in &module.classes {
            classes.insert(class.name.clone(), class.clone());
        }
        let trait_impls = module.trait_impls.clone();
        Self {
            module: Arc::new(module),
            functions,
            classes,
            trait_impls,
            stdout,
            cancellation,
            call_depth: 0,
            return_type_stack: Vec::new(),
        }
    }

    fn find_trait_impl_method(&self, receiver_ty: &Type, field: &str) -> Option<&MirMethod> {
        for trait_impl in &self.trait_impls {
            let mut type_params = std::collections::BTreeSet::new();
            collect_type_params_from_type(&trait_impl.for_type, &mut type_params);
            let mut substitutions = HashMap::new();
            if !crate::sema::type_pattern_matches(
                &trait_impl.for_type,
                receiver_ty,
                &type_params,
                &mut substitutions,
            ) {
                continue;
            }
            for method in &trait_impl.methods {
                if method.name == field {
                    return Some(method);
                }
            }
        }
        None
    }

    fn find_from_trait_impl_method(
        &self,
        source_ty: &Type,
        target_ty: &Type,
    ) -> Option<MirFunction> {
        for trait_impl in &self.trait_impls {
            if trait_impl.trait_name != "From" || trait_impl.trait_args.len() != 1 {
                continue;
            }
            let mut type_params = std::collections::BTreeSet::new();
            collect_type_params_from_type(&trait_impl.for_type, &mut type_params);
            for trait_arg in &trait_impl.trait_args {
                collect_type_params_from_type(trait_arg, &mut type_params);
            }
            let mut substitutions = HashMap::new();
            if !crate::sema::type_pattern_matches(
                &trait_impl.for_type,
                target_ty,
                &type_params,
                &mut substitutions,
            ) {
                continue;
            }
            if crate::sema::substitute_type(&trait_impl.trait_args[0], &substitutions) != *source_ty
            {
                continue;
            }
            for method in &trait_impl.methods {
                if method.name == "from" {
                    if let Some(function) = self.functions.get(&method.function_name) {
                        return Some(function.clone());
                    }
                }
            }
        }
        None
    }

    fn current_return_type(&self) -> Option<&Type> {
        self.return_type_stack.last()
    }

    fn convert_try_error_via_from(&mut self, payload: Value, source_ty: &Type) -> Result<Value> {
        let Some(Type::Named(return_name, return_args)) = self.current_return_type() else {
            return Err(Diagnostic::new(
                "MIR `try` is only allowed inside a function returning `Result`",
            ));
        };
        if return_name != "Result" || return_args.len() != 2 {
            return Err(Diagnostic::new(
                "MIR `try` is only allowed inside a function returning `Result`",
            ));
        }
        let target_error_ty = return_args[1].clone();
        if source_ty == &target_error_ty {
            return Ok(payload);
        }
        let Some(function) = self.find_from_trait_impl_method(source_ty, &target_error_ty) else {
            return Err(Diagnostic::new(format!(
                "`try` error type `{}` does not match enclosing `Result` error type `{}`",
                source_ty, target_error_ty
            )));
        };
        let outcome = self.call_function(
            &function,
            None,
            vec![EvaluatedMirArg {
                name: None,
                value: payload,
                writeback_place: None,
            }],
        )?;
        Ok(outcome.value)
    }

    fn find_trait_impl_method_for_class_name(
        &self,
        class_name: &str,
        field: &str,
    ) -> Option<&MirMethod> {
        let mut first = None;
        for trait_impl in &self.trait_impls {
            match &trait_impl.for_type {
                Type::Named(name, _) if name == class_name => {
                    for method in &trait_impl.methods {
                        if method.name == field {
                            if first.is_some() {
                                return None;
                            }
                            first = Some(method);
                            break;
                        }
                    }
                }
                _ => {}
            }
        }
        first
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
            Value::Vec(vector) => Some(Type::Named(
                "Vec".to_string(),
                vec![vector.element_type.clone()],
            )),
            Value::Set(set) => Some(Type::Named(
                "Set".to_string(),
                vec![set.element_type.clone()],
            )),
            Value::Map(map) => Some(Type::Named(
                "Map".to_string(),
                vec![map.key_type.clone(), map.value_type.clone()],
            )),
            Value::Duration(_) => Some(Type::named("Duration")),
            Value::Range(_) => Some(Type::named("Range")),
            Value::ModuleNamespace(_) => None,
            Value::Unit => Some(Type::Unit),
            Value::Instance(instance) => Some(Type::named(&instance.class_name)),
            Value::EnumVariant(variant) => match (
                variant.enum_name.as_str(),
                variant.variant_name.as_str(),
                variant.single_payload(),
            ) {
                ("Option", "Some", Some(payload)) => Self::infer_value_type(payload)
                    .map(|inner| Type::Named("Option".to_string(), vec![inner])),
                ("Option", "None", _) => Some(Type::Named("Option".to_string(), vec![Type::Unit])),
                ("Result", "Ok", Some(payload)) => Self::infer_value_type(payload)
                    .map(|ok| Type::Named("Result".to_string(), vec![ok, Type::Unit])),
                ("Result", "Err", Some(payload)) => Self::infer_value_type(payload)
                    .map(|err| Type::Named("Result".to_string(), vec![Type::Unit, err])),
                ("SendError", "Closed", Some(payload)) => Self::infer_value_type(payload)
                    .map(|inner| Type::Named("SendError".to_string(), vec![inner])),
                _ => Some(Type::named(&variant.enum_name)),
            },
            Value::Channel(_) | Value::Task(_) | Value::TaskGroup(_) => None,
        }
    }

    fn infer_instance_type(&self, instance: &InstanceValue) -> Option<Type> {
        let class = self.classes.get(&instance.class_name)?;
        if class.type_params.is_empty() {
            return Some(Type::named(&instance.class_name));
        }

        let mut substitutions = HashMap::new();
        for field in &class.fields {
            let actual_value = instance.fields.get(&field.name)?;
            let actual_ty = self.infer_runtime_value_type(actual_value)?;
            collect_runtime_type_substitutions(&field.ty, &actual_ty, &mut substitutions);
        }

        let resolved_args = class
            .type_params
            .iter()
            .map(|type_param| {
                substitutions
                    .get(type_param)
                    .cloned()
                    .unwrap_or_else(|| Type::named("Unknown"))
            })
            .collect();
        Some(Type::Named(instance.class_name.clone(), resolved_args))
    }

    fn infer_runtime_value_type(&self, value: &Value) -> Option<Type> {
        match value {
            Value::Instance(instance) => self.infer_instance_type(instance),
            Value::EnumVariant(_variant) => Self::infer_value_type(value).map(|ty| match ty {
                Type::Named(name, args) if name == "Option" && args == vec![Type::Unit] => {
                    Type::Named(name, vec![Type::named("Unknown")])
                }
                Type::Named(name, args)
                    if name == "Result" && args.iter().any(|arg| *arg == Type::Unit) =>
                {
                    Type::Named(
                        name,
                        args.into_iter()
                            .map(|arg| {
                                if arg == Type::Unit {
                                    Type::named("Unknown")
                                } else {
                                    arg
                                }
                            })
                            .collect(),
                    )
                }
                other => other,
            }),
            _ => Self::infer_value_type(value),
        }
    }

    fn validate_value_fits_type(
        &self,
        value: &Value,
        ty: &Type,
        span: Option<crate::diag::Span>,
    ) -> Result<()> {
        if let Some(bounds) = crate::sema::integer_type_bounds(ty) {
            let Value::Int(value) = value else {
                return Ok(());
            };
            if !value.fits_bounds(bounds) {
                let message = format!("integer value `{}` does not fit in `{}`", value, ty);
                return Err(match span {
                    Some(span) => Diagnostic::at(span, message),
                    None => Diagnostic::new(message),
                });
            }
        }
        Ok(())
    }

    fn coerce_value_to_type(
        &self,
        value: Value,
        ty: &Type,
        span: Option<crate::diag::Span>,
    ) -> Result<Value> {
        let coerced = match (&value, ty) {
            (Value::Unit, Type::Named(name, args)) if name == "Option" && args.len() == 1 => {
                option_none()
            }
            (Value::Int(_), Type::Named(name, _))
                if name.starts_with("int") || name.starts_with("uint") =>
            {
                value
            }
            (Value::Float(_), Type::Named(name, _)) if name == "float32" || name == "float64" => {
                cast_numeric_value(value, ty, span)?
            }
            (Value::Int(_), Type::Named(name, _)) if name == "float32" || name == "float64" => {
                cast_numeric_value(value, ty, span)?
            }
            (Value::Float(_), Type::Named(name, _))
                if name.starts_with("int") || name.starts_with("uint") =>
            {
                cast_numeric_value(value, ty, span)?
            }
            _ => value,
        };
        self.validate_value_fits_type(&coerced, ty, span)?;
        Ok(coerced)
    }

    fn resolve_place_type(&self, place: &str, env: &Env) -> Option<Type> {
        let mut segments = place.split('.');
        let root = segments.next()?;
        let mut current = env.place_type(root).cloned().or_else(|| {
            env.read_place(root)
                .ok()
                .and_then(|value| self.infer_runtime_value_type(&value))
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
        if self.call_depth >= MAX_CALL_DEPTH {
            return Err(Diagnostic::at(
                function.span,
                format!(
                    "maximum call depth of {} exceeded while calling `{}`",
                    MAX_CALL_DEPTH, function.name
                ),
            ));
        }
        self.call_depth += 1;
        let outcome = (|| {
            let bound_args = bind_args(&function.params, args)?;
            let mut substitutions = HashMap::new();
            for (param, argument) in function.params.iter().zip(bound_args.iter()) {
                if let Some(actual_ty) = self.infer_runtime_value_type(&argument.value) {
                    collect_runtime_type_substitutions(&param.ty, &actual_ty, &mut substitutions);
                }
            }

            let mut env = Env::default();
            for local in &function.local_types {
                env.set_place_type(&local.name, substitute_type(&local.ty, &substitutions));
            }
            if function.receiver.is_some() {
                let Some(receiver) = receiver else {
                    return Err(Diagnostic::new(format!(
                        "MIR function `{}` is missing its receiver",
                        function.name
                    )));
                };
                let receiver_ty = self
                    .infer_runtime_value_type(&receiver)
                    .unwrap_or_else(|| Type::named("Unknown"));
                env.define_typed("self", receiver_ty, receiver);
            }

            for (param, argument) in function.params.iter().zip(bound_args.iter()) {
                let ty = substitute_type(&param.ty, &substitutions);
                let value = self.coerce_value_to_type(argument.value.clone(), &ty, None)?;
                env.define_typed(&param.name, ty, value);
            }

            let return_type = substitute_type(&function.return_type, &substitutions);
            self.return_type_stack.push(return_type);
            let value_result = self.execute_function(function, &mut env);
            self.return_type_stack.pop();
            let value = value_result?;
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
        })();
        self.call_depth -= 1;
        outcome
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
                BlockOutcome::Goto(next) => {
                    Self::clear_exited_for_range_states(
                        function,
                        &block.label,
                        &next,
                        &mut loop_state,
                    );
                    current_label = next;
                }
            }
        }
    }

    fn clear_exited_for_range_states(
        function: &MirFunction,
        current_label: &str,
        next_label: &str,
        loop_state: &mut HashMap<String, i128>,
    ) {
        for block in &function.blocks {
            let Terminator::ForRange { exit_label, .. } = &block.terminator else {
                continue;
            };
            if block.label != current_label && exit_label == next_label {
                loop_state.remove(&block.label);
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
                        let evaluated = self.coerce_value_to_type(evaluated, &target_ty, span)?;
                        if !target.contains('.') {
                            env.set_place_type(target, target_ty);
                        }
                        env.write_place(target, evaluated)?;
                        return Ok(None);
                    } else if !target.contains('.') {
                        if let Some(inferred_ty) = self.infer_runtime_value_type(&evaluated) {
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
                    env.write_place(binding, Value::Int(IntegerValue::from_signed(current)))?;
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
                        || (arm.enum_name.is_none()
                            && arm.variant_name.as_deref() == Some(variant.variant_name.as_str()))
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
            Rvalue::FormatString { parts } => {
                let mut rendered = String::new();
                for part in parts {
                    match part {
                        MirFormatPart::Literal(text) => rendered.push_str(text),
                        MirFormatPart::Value(value) => {
                            rendered.push_str(&self.evaluate_operand(value, env)?.render())
                        }
                    }
                }
                Ok(RvalueOutcome::Value(Value::String(rendered)))
            }
            Rvalue::Unary { op, value, .. } => {
                let value = self.evaluate_operand(value, env)?;
                let result = match (op, value) {
                    (UnaryOp::Not, Value::Bool(value)) => Value::Bool(!value),
                    (UnaryOp::Neg, Value::Int(value)) => Value::Int(
                        value
                            .checked_neg()
                            .ok_or_else(|| Diagnostic::new("integer overflow"))?,
                    ),
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
                match (variant.variant_name.as_str(), variant.payloads.as_slice()) {
                    ("Ok", [payload]) => Ok(RvalueOutcome::Value(payload.clone())),
                    ("Err", [payload]) => {
                        let source_ty = self
                            .infer_runtime_value_type(payload)
                            .unwrap_or_else(|| Type::named("Unknown"));
                        let payload =
                            self.convert_try_error_via_from(payload.clone(), &source_ty)?;
                        Ok(RvalueOutcome::Return(Value::EnumVariant(
                            EnumVariantValue {
                                enum_name: "Result".to_string(),
                                variant_name: "Err".to_string(),
                                payloads: vec![payload],
                            },
                        )))
                    }
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
            Rvalue::VecLiteral {
                elements,
                element_type,
            } => Ok(RvalueOutcome::Value(Value::Vec(VecValue {
                element_type: element_type.clone(),
                elements: elements
                    .iter()
                    .map(|operand| self.evaluate_operand(operand, env))
                    .collect::<Result<Vec<_>>>()?,
            }))),
            Rvalue::SetLiteral {
                elements,
                element_type,
            } => {
                let mut values = Vec::new();
                for operand in elements {
                    let value = self.evaluate_operand(operand, env)?;
                    if !values.iter().any(|candidate| *candidate == value) {
                        values.push(value);
                    }
                }
                Ok(RvalueOutcome::Value(Value::Set(SetValue {
                    element_type: element_type.clone(),
                    elements: values,
                })))
            }
            Rvalue::MapLiteral {
                entries,
                key_type,
                value_type,
            } => Ok(RvalueOutcome::Value(Value::Map(MapValue {
                key_type: key_type.clone(),
                value_type: value_type.clone(),
                entries: entries
                    .iter()
                    .map(|entry| {
                        Ok((
                            self.evaluate_operand(&entry.key, env)?,
                            self.evaluate_operand(&entry.value, env)?,
                        ))
                    })
                    .collect::<Result<Vec<_>>>()?,
            }))),
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
                payloads,
            } => Ok(RvalueOutcome::Value(Value::EnumVariant(EnumVariantValue {
                enum_name: enum_name.clone(),
                variant_name: variant_name.clone(),
                payloads: payloads
                    .iter()
                    .map(|payload| self.evaluate_operand(payload, env))
                    .collect::<Result<Vec<_>>>()?,
            }))),
            Rvalue::VariantPayload { scrutinee, index } => {
                let scrutinee = self.evaluate_operand(scrutinee, env)?;
                let Value::EnumVariant(variant) = scrutinee else {
                    return Err(Diagnostic::new(format!(
                        "MIR variant payload extraction expected an enum value, found `{}`",
                        scrutinee.render()
                    )));
                };
                let payload = variant.payloads.get(*index).cloned().ok_or_else(|| {
                    Diagnostic::new(format!(
                        "enum variant `{}.{}` does not carry a payload at index {}",
                        variant.enum_name, variant.variant_name, index
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
                    let mut stdout = lock_stdout(&self.stdout);
                    stdout.push_str(&rendered);
                    stdout.push('\n');
                    return Ok(Value::Unit);
                }

                if name == "range" {
                    let values = evaluate_named_args(args, env)?;
                    return build_range(values);
                }

                if name == "queue" {
                    let values = evaluate_named_args(args, env)?;
                    if values.len() > 1 {
                        return Err(Diagnostic::new(format!(
                            "`{}()` expects at most one optional `capacity` argument",
                            name
                        )));
                    }
                    return Ok(Value::Channel(ChannelValue::new()));
                }

                if name == "Vec" {
                    let values = evaluate_named_args(args, env)?;
                    bind_builtin_args(&[], values)?;
                    return Ok(Value::Vec(VecValue {
                        element_type: Type::named("Unknown"),
                        elements: Vec::new(),
                    }));
                }

                if name == "Set" {
                    let values = evaluate_named_args(args, env)?;
                    bind_builtin_args(&[], values)?;
                    return Ok(Value::Set(SetValue {
                        element_type: Type::named("Unknown"),
                        elements: Vec::new(),
                    }));
                }

                if name == "Map" {
                    let values = evaluate_named_args(args, env)?;
                    bind_builtin_args(&[], values)?;
                    return Ok(Value::Map(MapValue {
                        key_type: Type::named("Unknown"),
                        value_type: Type::named("Unknown"),
                        entries: Vec::new(),
                    }));
                }

                if name == "tasks" {
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
                        Value::Int(duration) => duration.as_i128().ok_or_else(|| {
                            Diagnostic::new("`after(...)` duration must fit in signed timer range")
                        })?,
                        Value::Duration(duration) => duration,
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
                        Value::Int(duration) => duration.as_i128().ok_or_else(|| {
                            Diagnostic::new("`sleep(...)` duration must fit in signed timer range")
                        })?,
                        Value::Duration(duration) => duration,
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

                if name == "abs" {
                    let values = evaluate_named_args(args, env)?;
                    let bound = bind_builtin_args(&["value"], values)?;
                    return match bound[0].value.clone() {
                        Value::Int(IntegerValue::Signed(value)) => value
                            .checked_abs()
                            .map(IntegerValue::from_signed)
                            .map(Value::Int)
                            .ok_or_else(|| {
                                Diagnostic::new("`abs(...)` overflowed the signed integer range")
                            }),
                        Value::Int(IntegerValue::Unsigned(value)) => {
                            Ok(Value::Int(IntegerValue::Unsigned(value)))
                        }
                        Value::Float(value) => Ok(Value::Float(value.abs())),
                        other => Err(Diagnostic::new(format!(
                            "`abs(...)` expects an integer or float value, found `{}`",
                            other.render()
                        ))),
                    };
                }

                if name == "min" || name == "max" {
                    let values = evaluate_named_args(args, env)?;
                    let bound = bind_builtin_args(&["left", "right"], values)?;
                    return match (&bound[0].value, &bound[1].value) {
                        (Value::Int(left), Value::Int(right)) => Ok(
                            if (name == "min" && left <= right) || (name == "max" && left >= right)
                            {
                                bound[0].value.clone()
                            } else {
                                bound[1].value.clone()
                            },
                        ),
                        (Value::Float(left), Value::Float(right)) => Ok(
                            if (name == "min" && left <= right) || (name == "max" && left >= right)
                            {
                                bound[0].value.clone()
                            } else {
                                bound[1].value.clone()
                            },
                        ),
                        _ => Err(Diagnostic::new(format!(
                            "`{}` expects matching numeric arguments",
                            name
                        ))),
                    };
                }

                if name == "sqrt" {
                    let values = evaluate_named_args(args, env)?;
                    let bound = bind_builtin_args(&["value"], values)?;
                    return match bound[0].value.clone() {
                        Value::Float(value) => Ok(Value::Float(value.sqrt())),
                        other => Err(Diagnostic::new(format!(
                            "`sqrt(...)` expects `float32` or `float64`, found `{}`",
                            other.render()
                        ))),
                    };
                }

                if name == "parse_int32" {
                    let values = evaluate_named_args(args, env)?;
                    let bound = bind_builtin_args(&["text"], values)?;
                    return match &bound[0].value {
                        Value::String(text) => match text.parse::<i32>() {
                            Ok(value) => Ok(result_ok(Value::Int(IntegerValue::from_signed(
                                value as i128,
                            )))),
                            Err(error) => Ok(result_err(Value::String(error.to_string()))),
                        },
                        other => Err(Diagnostic::new(format!(
                            "`parse_int32(...)` expects `String`, found `{}`",
                            other.render()
                        ))),
                    };
                }

                if name == "parse_int64" {
                    let values = evaluate_named_args(args, env)?;
                    let bound = bind_builtin_args(&["text"], values)?;
                    return match &bound[0].value {
                        Value::String(text) => match text.parse::<i64>() {
                            Ok(value) => Ok(result_ok(Value::Int(IntegerValue::from_signed(
                                value as i128,
                            )))),
                            Err(error) => Ok(result_err(Value::String(error.to_string()))),
                        },
                        other => Err(Diagnostic::new(format!(
                            "`parse_int64(...)` expects `String`, found `{}`",
                            other.render()
                        ))),
                    };
                }

                if name == "parse_float64" {
                    let values = evaluate_named_args(args, env)?;
                    let bound = bind_builtin_args(&["text"], values)?;
                    return match &bound[0].value {
                        Value::String(text) => match text.parse::<f64>() {
                            Ok(value) => Ok(result_ok(Value::Float(value))),
                            Err(error) => Ok(result_err(Value::String(error.to_string()))),
                        },
                        other => Err(Diagnostic::new(format!(
                            "`parse_float64(...)` expects `String`, found `{}`",
                            other.render()
                        ))),
                    };
                }

                if name == "print" || name == "range" {
                    unreachable!("handled earlier");
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
                    Value::Vec(vector) => {
                        return self.evaluate_vec_method(
                            vector.clone(),
                            field,
                            receiver_place.as_deref(),
                            args,
                            env,
                        );
                    }
                    Value::Map(map) => {
                        return self.evaluate_map_method(
                            map.clone(),
                            field,
                            receiver_place.as_deref(),
                            args,
                            env,
                        );
                    }
                    Value::Float(value) if field == "sqrt" => {
                        if !args.is_empty() {
                            return Err(Diagnostic::new("`sqrt` does not take arguments"));
                        }
                        return Ok(Value::Float(value.sqrt()));
                    }
                    Value::Int(value) if field == "to_string" => {
                        if !args.is_empty() {
                            return Err(Diagnostic::new("`to_string` does not take arguments"));
                        }
                        return Ok(Value::String(value.to_string()));
                    }
                    Value::Float(value) if field == "to_string" => {
                        if !args.is_empty() {
                            return Err(Diagnostic::new("`to_string` does not take arguments"));
                        }
                        return Ok(Value::String(Value::Float(*value).render()));
                    }
                    Value::Bool(value) if field == "to_string" => {
                        if !args.is_empty() {
                            return Err(Diagnostic::new("`to_string` does not take arguments"));
                        }
                        return Ok(Value::String(value.to_string()));
                    }
                    Value::String(value) => {
                        return self.evaluate_string_method(value.clone(), field, args, env);
                    }
                    Value::Set(set) => {
                        return self.evaluate_set_method(
                            set.clone(),
                            field,
                            receiver_place.as_deref(),
                            args,
                            env,
                        );
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
                    other => {
                        let resolved_receiver_ty = receiver_place
                            .as_ref()
                            .and_then(|place| self.resolve_place_type(place, env))
                            .and_then(|ty| (!matches!(ty, Type::TypeParam(_))).then_some(ty))
                            .or_else(|| self.infer_runtime_value_type(other));
                        if let Some(resolved_receiver_ty) = resolved_receiver_ty {
                            if let Some(method) = self
                                .find_trait_impl_method(&resolved_receiver_ty, field)
                                .cloned()
                            {
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
                                return Ok(outcome.value);
                            }
                        }
                        Err(Diagnostic::new(format!(
                            "unsupported MIR member call `{}` on `{}`",
                            field,
                            receiver.render()
                        )))
                    }
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
            "put" => {
                let values = evaluate_named_args(args, env)?;
                let bound = bind_builtin_args(&["value"], values)?;
                let Some(value) = bound.into_iter().next().map(|arg| arg.value) else {
                    return Err(Diagnostic::new(format!(
                        "internal error: `{}` should bind one argument",
                        field
                    )));
                };
                match channel.send(value) {
                    Ok(()) => Ok(result_ok(Value::Unit)),
                    Err(value) => Ok(result_err(send_error_closed(value))),
                }
            }
            "get" => {
                let values = evaluate_named_args(args, env)?;
                if values.len() > 1 {
                    return Err(Diagnostic::new(
                        "`get()` expects at most one optional `timeout` argument",
                    ));
                }
                let timeout = if let Some(argument) = values.into_iter().next() {
                    if argument
                        .name
                        .as_deref()
                        .is_some_and(|name| name != "timeout")
                    {
                        return Err(Diagnostic::new(
                            "`get()` only accepts the named argument `timeout`",
                        ));
                    }
                    match argument.value {
                        Value::Duration(duration) => Some(duration),
                        Value::Int(duration) => Some(duration.as_i128().ok_or_else(|| {
                            Diagnostic::new(
                                "`get(timeout=...)` duration must fit in signed timer range",
                            )
                        })?),
                        other => {
                            return Err(Diagnostic::new(format!(
                                "`get(timeout=...)` expects a `Duration`, found `{}`",
                                other.render()
                            )))
                        }
                    }
                } else {
                    None
                };
                let received = if let Some(timeout) = timeout {
                    let timeout = u64::try_from(timeout).map_err(|_| {
                        Diagnostic::new(format!(
                            "duration `{}ms` does not fit in the MIR runtime timer range",
                            timeout
                        ))
                    })?;
                    channel.recv_timeout(StdDuration::from_millis(timeout))
                } else {
                    channel.recv_blocking()
                };
                Ok(match received {
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

    fn evaluate_vec_method(
        &mut self,
        vector: VecValue,
        field: &str,
        receiver_place: Option<&str>,
        args: &[MirArg],
        env: &mut Env,
    ) -> Result<Value> {
        match field {
            "len" => {
                if !args.is_empty() {
                    return Err(Diagnostic::new("`len` does not take arguments"));
                }
                Ok(Value::Int(IntegerValue::from_literal(
                    vector.elements.len() as u128,
                )))
            }
            "is_empty" => {
                if !args.is_empty() {
                    return Err(Diagnostic::new("`is_empty` does not take arguments"));
                }
                Ok(Value::Bool(vector.elements.is_empty()))
            }
            "clone" => {
                if !args.is_empty() {
                    return Err(Diagnostic::new("`clone` does not take arguments"));
                }
                Ok(Value::Vec(vector))
            }
            "push" => {
                let values = evaluate_named_args(args, env)?;
                let bound = bind_builtin_args(&["value"], values)?;
                let mut updated = vector;
                let Some(value) = bound.into_iter().next().map(|arg| arg.value) else {
                    return Err(Diagnostic::new(
                        "internal error: `push` should bind one argument",
                    ));
                };
                updated.elements.push(value);
                let updated_value = Value::Vec(updated);
                let Some(place) = receiver_place else {
                    return Err(Diagnostic::new("`push` requires a mutable vector place"));
                };
                env.write_place(place, updated_value)?;
                Ok(Value::Unit)
            }
            "pop" => {
                if !args.is_empty() {
                    return Err(Diagnostic::new("`pop` does not take arguments"));
                }
                let mut updated = vector;
                let value = updated.elements.pop();
                let Some(place) = receiver_place else {
                    return Err(Diagnostic::new("`pop` requires a mutable vector place"));
                };
                env.write_place(place, Value::Vec(updated))?;
                Ok(value.map(option_some).unwrap_or_else(option_none))
            }
            "get" | "__index_option" => {
                let values = evaluate_named_args(args, env)?;
                let bound = bind_builtin_args(&["index"], values)?;
                let index = self.mir_index_from_value(bound[0].value.clone())?;
                Ok(vector
                    .elements
                    .get(index)
                    .cloned()
                    .map(option_some)
                    .unwrap_or_else(option_none))
            }
            "__index" => {
                let values = evaluate_named_args(args, env)?;
                if values.len() != 3 {
                    return Err(Diagnostic::new(
                        "internal vector indexing requires index, line, and column operands",
                    ));
                }
                let index = self.mir_index_from_value(values[0].value.clone())?;
                let line = self.mir_index_from_value(values[1].value.clone())?;
                let column = self.mir_index_from_value(values[2].value.clone())?;
                vector.elements.get(index).cloned().ok_or_else(|| {
                    Diagnostic::at(
                        crate::diag::Span::new(line, column),
                        format!(
                            "vector index `{}` is out of bounds for length `{}`",
                            index,
                            vector.elements.len()
                        ),
                    )
                })
            }
            "set" => {
                let values = evaluate_named_args(args, env)?;
                let bound = bind_builtin_args(&["index", "value"], values)?;
                let index = self.mir_index_from_value(bound[0].value.clone())?;
                let mut updated = vector;
                let previous = if index < updated.elements.len() {
                    Some(std::mem::replace(
                        &mut updated.elements[index],
                        bound[1].value.clone(),
                    ))
                } else {
                    None
                };
                let Some(place) = receiver_place else {
                    return Err(Diagnostic::new("`set` requires a mutable vector place"));
                };
                env.write_place(place, Value::Vec(updated))?;
                Ok(previous.map(option_some).unwrap_or_else(option_none))
            }
            "__set_index" => {
                let values = evaluate_named_args(args, env)?;
                if values.len() != 4 {
                    return Err(Diagnostic::new(
                        "internal indexed assignment requires index, value, line, and column operands",
                    ));
                }
                let index = self.mir_index_from_value(values[0].value.clone())?;
                let line = self.mir_index_from_value(values[2].value.clone())?;
                let column = self.mir_index_from_value(values[3].value.clone())?;
                let mut updated = vector;
                if index >= updated.elements.len() {
                    return Err(Diagnostic::at(
                        crate::diag::Span::new(line, column),
                        format!(
                            "vector index `{}` is out of bounds for length `{}`",
                            index,
                            updated.elements.len()
                        ),
                    ));
                }
                updated.elements[index] = values[1].value.clone();
                let Some(place) = receiver_place else {
                    return Err(Diagnostic::new(
                        "indexed assignment requires a mutable vector place",
                    ));
                };
                env.write_place(place, Value::Vec(updated))?;
                Ok(Value::Unit)
            }
            "remove" => {
                let values = evaluate_named_args(args, env)?;
                let bound = bind_builtin_args(&["index"], values)?;
                let index = self.mir_index_from_value(bound[0].value.clone())?;
                let mut updated = vector;
                let previous = if index < updated.elements.len() {
                    Some(updated.elements.remove(index))
                } else {
                    None
                };
                let Some(place) = receiver_place else {
                    return Err(Diagnostic::new("`remove` requires a mutable vector place"));
                };
                env.write_place(place, Value::Vec(updated))?;
                Ok(previous.map(option_some).unwrap_or_else(option_none))
            }
            "swap" => {
                let values = evaluate_named_args(args, env)?;
                let bound = bind_builtin_args(&["first", "second"], values)?;
                let first = self.mir_index_from_value(bound[0].value.clone())?;
                let second = self.mir_index_from_value(bound[1].value.clone())?;
                let mut updated = vector;
                let swapped = first < updated.elements.len() && second < updated.elements.len();
                if swapped {
                    updated.elements.swap(first, second);
                }
                let Some(place) = receiver_place else {
                    return Err(Diagnostic::new("`swap` requires a mutable vector place"));
                };
                env.write_place(place, Value::Vec(updated))?;
                Ok(Value::Bool(swapped))
            }
            "contains" => {
                let values = evaluate_named_args(args, env)?;
                let bound = bind_builtin_args(&["value"], values)?;
                Ok(Value::Bool(
                    vector
                        .elements
                        .iter()
                        .any(|candidate| *candidate == bound[0].value),
                ))
            }
            "insert" => {
                let values = evaluate_named_args(args, env)?;
                let bound = bind_builtin_args(&["index", "value"], values)?;
                let index = self.mir_index_from_value(bound[0].value.clone())?;
                let mut updated = vector;
                let inserted = index <= updated.elements.len();
                if inserted {
                    updated.elements.insert(index, bound[1].value.clone());
                }
                let Some(place) = receiver_place else {
                    return Err(Diagnostic::new("`insert` requires a mutable vector place"));
                };
                env.write_place(place, Value::Vec(updated))?;
                Ok(Value::Bool(inserted))
            }
            "clear" => {
                if !args.is_empty() {
                    return Err(Diagnostic::new("`clear` does not take arguments"));
                }
                let mut updated = vector;
                updated.elements.clear();
                let Some(place) = receiver_place else {
                    return Err(Diagnostic::new("`clear` requires a mutable vector place"));
                };
                env.write_place(place, Value::Vec(updated))?;
                Ok(Value::Unit)
            }
            "reverse" => {
                if !args.is_empty() {
                    return Err(Diagnostic::new("`reverse` does not take arguments"));
                }
                let mut updated = vector;
                updated.elements.reverse();
                let Some(place) = receiver_place else {
                    return Err(Diagnostic::new("`reverse` requires a mutable vector place"));
                };
                env.write_place(place, Value::Vec(updated))?;
                Ok(Value::Unit)
            }
            "extend" => {
                let values = evaluate_named_args(args, env)?;
                let bound = bind_builtin_args(&["other"], values)?;
                let Value::Vec(other) = bound[0].value.clone() else {
                    return Err(Diagnostic::new("`extend` requires another `Vec[T]` value"));
                };
                let mut updated = vector;
                updated.elements.extend(other.elements);
                let Some(place) = receiver_place else {
                    return Err(Diagnostic::new("`extend` requires a mutable vector place"));
                };
                env.write_place(place, Value::Vec(updated))?;
                Ok(Value::Unit)
            }
            _ => Err(Diagnostic::new(format!(
                "unsupported vector method `{}`",
                field
            ))),
        }
    }

    fn evaluate_map_method(
        &mut self,
        map: MapValue,
        field: &str,
        receiver_place: Option<&str>,
        args: &[MirArg],
        env: &mut Env,
    ) -> Result<Value> {
        match field {
            "len" => {
                if !args.is_empty() {
                    return Err(Diagnostic::new("`len` does not take arguments"));
                }
                Ok(Value::Int(IntegerValue::from_literal(
                    map.entries.len() as u128
                )))
            }
            "is_empty" => {
                if !args.is_empty() {
                    return Err(Diagnostic::new("`is_empty` does not take arguments"));
                }
                Ok(Value::Bool(map.entries.is_empty()))
            }
            "clone" => {
                if !args.is_empty() {
                    return Err(Diagnostic::new("`clone` does not take arguments"));
                }
                Ok(Value::Map(map))
            }
            "get" => {
                let values = evaluate_named_args(args, env)?;
                let bound = bind_builtin_args(&["key"], values)?;
                Ok(map
                    .entries
                    .iter()
                    .find(|(candidate_key, _)| *candidate_key == bound[0].value)
                    .map(|(_, value)| option_some(value.clone()))
                    .unwrap_or_else(option_none))
            }
            "__index" => {
                let values = evaluate_named_args(args, env)?;
                if values.len() != 3 {
                    return Err(Diagnostic::new(
                        "internal map indexing requires key, line, and column operands",
                    ));
                }
                let key = values[0].value.clone();
                let line = self.mir_index_from_value(values[1].value.clone())?;
                let column = self.mir_index_from_value(values[2].value.clone())?;
                map.entries
                    .iter()
                    .find(|(candidate_key, _)| *candidate_key == key)
                    .map(|(_, value)| value.clone())
                    .ok_or_else(|| {
                        Diagnostic::at(
                            crate::diag::Span::new(line, column),
                            format!("map key `{}` was not present", key.render()),
                        )
                    })
            }
            "set" => {
                let values = evaluate_named_args(args, env)?;
                let bound = bind_builtin_args(&["key", "value"], values)?;
                let mut updated = map;
                let previous = if let Some(index) = updated
                    .entries
                    .iter()
                    .position(|(candidate_key, _)| *candidate_key == bound[0].value)
                {
                    Some(std::mem::replace(
                        &mut updated.entries[index].1,
                        bound[1].value.clone(),
                    ))
                } else {
                    updated
                        .entries
                        .push((bound[0].value.clone(), bound[1].value.clone()));
                    None
                };
                let Some(place) = receiver_place else {
                    return Err(Diagnostic::new("`set` requires a mutable map place"));
                };
                env.write_place(place, Value::Map(updated))?;
                Ok(previous.map(option_some).unwrap_or_else(option_none))
            }
            "__set_index" => {
                let values = evaluate_named_args(args, env)?;
                if values.len() != 4 {
                    return Err(Diagnostic::new(
                        "internal map indexed assignment requires key, value, line, and column operands",
                    ));
                }
                let mut updated = map;
                let key = values[0].value.clone();
                let value = values[1].value.clone();
                if let Some(index) = updated
                    .entries
                    .iter()
                    .position(|(candidate_key, _)| *candidate_key == key)
                {
                    updated.entries[index].1 = value;
                } else {
                    updated.entries.push((key, value));
                }
                let Some(place) = receiver_place else {
                    return Err(Diagnostic::new(
                        "indexed assignment requires a mutable map place",
                    ));
                };
                env.write_place(place, Value::Map(updated))?;
                Ok(Value::Unit)
            }
            "remove" => {
                let values = evaluate_named_args(args, env)?;
                let bound = bind_builtin_args(&["key"], values)?;
                let mut updated = map;
                let removed = if let Some(index) = updated
                    .entries
                    .iter()
                    .position(|(candidate_key, _)| *candidate_key == bound[0].value)
                {
                    Some(updated.entries.remove(index).1)
                } else {
                    None
                };
                let Some(place) = receiver_place else {
                    return Err(Diagnostic::new("`remove` requires a mutable map place"));
                };
                env.write_place(place, Value::Map(updated))?;
                Ok(removed.map(option_some).unwrap_or_else(option_none))
            }
            "contains_key" => {
                let values = evaluate_named_args(args, env)?;
                let bound = bind_builtin_args(&["key"], values)?;
                Ok(Value::Bool(map.entries.iter().any(|(candidate_key, _)| {
                    *candidate_key == bound[0].value
                })))
            }
            "keys" => {
                if !args.is_empty() {
                    return Err(Diagnostic::new("`keys` does not take arguments"));
                }
                Ok(Value::Vec(VecValue {
                    element_type: map.key_type.clone(),
                    elements: map.entries.iter().map(|(key, _)| key.clone()).collect(),
                }))
            }
            "values" => {
                if !args.is_empty() {
                    return Err(Diagnostic::new("`values` does not take arguments"));
                }
                Ok(Value::Vec(VecValue {
                    element_type: map.value_type.clone(),
                    elements: map.entries.iter().map(|(_, value)| value.clone()).collect(),
                }))
            }
            "items" | "entries" => {
                if !args.is_empty() {
                    return Err(Diagnostic::new(format!(
                        "`{}` does not take arguments",
                        field
                    )));
                }
                Ok(Value::Vec(VecValue {
                    element_type: Type::Named(
                        "MapEntry".to_string(),
                        vec![map.key_type.clone(), map.value_type.clone()],
                    ),
                    elements: map
                        .entries
                        .iter()
                        .map(|(key, value)| {
                            Value::Instance(InstanceValue {
                                class_name: "MapEntry".to_string(),
                                fields: BTreeMap::from([
                                    ("key".to_string(), key.clone()),
                                    ("value".to_string(), value.clone()),
                                ]),
                            })
                        })
                        .collect(),
                }))
            }
            "clear" => {
                if !args.is_empty() {
                    return Err(Diagnostic::new("`clear` does not take arguments"));
                }
                let mut updated = map;
                updated.entries.clear();
                let Some(place) = receiver_place else {
                    return Err(Diagnostic::new("`clear` requires a mutable map place"));
                };
                env.write_place(place, Value::Map(updated))?;
                Ok(Value::Unit)
            }
            "extend" => {
                let values = evaluate_named_args(args, env)?;
                let bound = bind_builtin_args(&["other"], values)?;
                let Value::Map(other) = bound[0].value.clone() else {
                    return Err(Diagnostic::new(
                        "`extend` requires another `Map[K, V]` value",
                    ));
                };
                let mut updated = map;
                for (key, value) in other.entries {
                    if let Some(index) = updated
                        .entries
                        .iter()
                        .position(|(candidate_key, _)| *candidate_key == key)
                    {
                        updated.entries[index].1 = value;
                    } else {
                        updated.entries.push((key, value));
                    }
                }
                let Some(place) = receiver_place else {
                    return Err(Diagnostic::new("`extend` requires a mutable map place"));
                };
                env.write_place(place, Value::Map(updated))?;
                Ok(Value::Unit)
            }
            _ => Err(Diagnostic::new(format!(
                "unsupported map method `{}`",
                field
            ))),
        }
    }

    fn evaluate_string_method(
        &mut self,
        text: String,
        field: &str,
        args: &[MirArg],
        env: &mut Env,
    ) -> Result<Value> {
        match field {
            "len" => {
                if !args.is_empty() {
                    return Err(Diagnostic::new("`len` does not take arguments"));
                }
                Ok(Value::Int(IntegerValue::from_literal(text.len() as u128)))
            }
            "contains" => {
                let values = evaluate_named_args(args, env)?;
                let bound = bind_builtin_args(&["text"], values)?;
                let Value::String(needle) = bound[0].value.clone() else {
                    return Err(Diagnostic::new("`contains` requires a `String` argument"));
                };
                Ok(Value::Bool(text.contains(&needle)))
            }
            "starts_with" => {
                let values = evaluate_named_args(args, env)?;
                let bound = bind_builtin_args(&["text"], values)?;
                let Value::String(prefix) = bound[0].value.clone() else {
                    return Err(Diagnostic::new(
                        "`starts_with` requires a `String` argument",
                    ));
                };
                Ok(Value::Bool(text.starts_with(&prefix)))
            }
            "ends_with" => {
                let values = evaluate_named_args(args, env)?;
                let bound = bind_builtin_args(&["text"], values)?;
                let Value::String(suffix) = bound[0].value.clone() else {
                    return Err(Diagnostic::new("`ends_with` requires a `String` argument"));
                };
                Ok(Value::Bool(text.ends_with(&suffix)))
            }
            "split" => {
                let values = evaluate_named_args(args, env)?;
                let bound = bind_builtin_args(&["text"], values)?;
                let Value::String(separator) = bound[0].value.clone() else {
                    return Err(Diagnostic::new("`split` requires a `String` argument"));
                };
                Ok(Value::Vec(VecValue {
                    element_type: Type::named("String"),
                    elements: text
                        .split(&separator)
                        .map(|part| Value::String(part.to_string()))
                        .collect(),
                }))
            }
            "replace" => {
                let values = evaluate_named_args(args, env)?;
                let bound = bind_builtin_args(&["from", "to"], values)?;
                let Value::String(from) = bound[0].value.clone() else {
                    return Err(Diagnostic::new("`replace` requires `String` for `from`"));
                };
                let Value::String(to) = bound[1].value.clone() else {
                    return Err(Diagnostic::new("`replace` requires `String` for `to`"));
                };
                Ok(Value::String(text.replace(&from, &to)))
            }
            "to_lower" => {
                if !args.is_empty() {
                    return Err(Diagnostic::new("`to_lower` does not take arguments"));
                }
                Ok(Value::String(text.to_lowercase()))
            }
            "to_upper" => {
                if !args.is_empty() {
                    return Err(Diagnostic::new("`to_upper` does not take arguments"));
                }
                Ok(Value::String(text.to_uppercase()))
            }
            "join" => {
                let values = evaluate_named_args(args, env)?;
                let bound = bind_builtin_args(&["parts"], values)?;
                let Value::Vec(parts) = bound[0].value.clone() else {
                    return Err(Diagnostic::new("`join` requires `Vec[String]`"));
                };
                let mut rendered_parts = Vec::new();
                for value in parts.elements {
                    let Value::String(part) = value else {
                        return Err(Diagnostic::new("`join` requires `Vec[String]`"));
                    };
                    rendered_parts.push(part);
                }
                Ok(Value::String(rendered_parts.join(&text)))
            }
            "strip_prefix" => {
                let values = evaluate_named_args(args, env)?;
                let bound = bind_builtin_args(&["text"], values)?;
                let Value::String(prefix) = bound[0].value.clone() else {
                    return Err(Diagnostic::new(
                        "`strip_prefix` requires a `String` argument",
                    ));
                };
                Ok(text
                    .strip_prefix(&prefix)
                    .map(|rest| option_some(Value::String(rest.to_string())))
                    .unwrap_or_else(option_none))
            }
            "strip_suffix" => {
                let values = evaluate_named_args(args, env)?;
                let bound = bind_builtin_args(&["text"], values)?;
                let Value::String(suffix) = bound[0].value.clone() else {
                    return Err(Diagnostic::new(
                        "`strip_suffix` requires a `String` argument",
                    ));
                };
                Ok(text
                    .strip_suffix(&suffix)
                    .map(|rest| option_some(Value::String(rest.to_string())))
                    .unwrap_or_else(option_none))
            }
            "trim" => {
                if !args.is_empty() {
                    return Err(Diagnostic::new("`trim` does not take arguments"));
                }
                Ok(Value::String(text.trim().to_string()))
            }
            "clone" => {
                if !args.is_empty() {
                    return Err(Diagnostic::new("`clone` does not take arguments"));
                }
                Ok(Value::String(text))
            }
            _ => Err(Diagnostic::new(format!(
                "unsupported string method `{}`",
                field
            ))),
        }
    }

    fn evaluate_set_method(
        &mut self,
        set: SetValue,
        field: &str,
        receiver_place: Option<&str>,
        args: &[MirArg],
        env: &mut Env,
    ) -> Result<Value> {
        match field {
            "len" => {
                if !args.is_empty() {
                    return Err(Diagnostic::new("`len` does not take arguments"));
                }
                Ok(Value::Int(IntegerValue::from_literal(
                    set.elements.len() as u128
                )))
            }
            "is_empty" => {
                if !args.is_empty() {
                    return Err(Diagnostic::new("`is_empty` does not take arguments"));
                }
                Ok(Value::Bool(set.elements.is_empty()))
            }
            "clone" => {
                if !args.is_empty() {
                    return Err(Diagnostic::new("`clone` does not take arguments"));
                }
                Ok(Value::Set(set))
            }
            "contains" => {
                let values = evaluate_named_args(args, env)?;
                let bound = bind_builtin_args(&["value"], values)?;
                Ok(Value::Bool(
                    set.elements
                        .iter()
                        .any(|candidate| *candidate == bound[0].value),
                ))
            }
            "insert" => {
                let values = evaluate_named_args(args, env)?;
                let bound = bind_builtin_args(&["value"], values)?;
                let mut updated = set;
                let inserted = if updated
                    .elements
                    .iter()
                    .any(|candidate| *candidate == bound[0].value)
                {
                    false
                } else {
                    updated.elements.push(bound[0].value.clone());
                    true
                };
                let Some(place) = receiver_place else {
                    return Err(Diagnostic::new("`insert` requires a mutable set place"));
                };
                env.write_place(place, Value::Set(updated))?;
                Ok(Value::Bool(inserted))
            }
            "remove" => {
                let values = evaluate_named_args(args, env)?;
                let bound = bind_builtin_args(&["value"], values)?;
                let mut updated = set;
                let removed = if let Some(index) = updated
                    .elements
                    .iter()
                    .position(|candidate| *candidate == bound[0].value)
                {
                    updated.elements.remove(index);
                    true
                } else {
                    false
                };
                let Some(place) = receiver_place else {
                    return Err(Diagnostic::new("`remove` requires a mutable set place"));
                };
                env.write_place(place, Value::Set(updated))?;
                Ok(Value::Bool(removed))
            }
            "__index_option" => {
                let values = evaluate_named_args(args, env)?;
                let bound = bind_builtin_args(&["index"], values)?;
                let index = self.mir_index_from_value(bound[0].value.clone())?;
                Ok(set
                    .elements
                    .get(index)
                    .cloned()
                    .map(option_some)
                    .unwrap_or_else(option_none))
            }
            _ => Err(Diagnostic::new(format!(
                "unsupported set method `{}`",
                field
            ))),
        }
    }

    fn mir_index_from_value(&self, value: Value) -> Result<usize> {
        let Value::Int(value) = value else {
            return Err(Diagnostic::new("vector indices must be integers"));
        };
        let index = value
            .as_i128()
            .ok_or_else(|| Diagnostic::new("vector index is outside the supported signed range"))?;
        if index < 0 {
            return Err(Diagnostic::new(format!(
                "vector index `{}` cannot be negative",
                index
            )));
        }
        usize::try_from(index)
            .map_err(|_| Diagnostic::new("vector index does not fit in the MIR address space"))
    }

    fn evaluate_task_method(
        &mut self,
        task: TaskValue,
        field: &str,
        args: &[MirArg],
    ) -> Result<Value> {
        match field {
            "result" => {
                if !args.is_empty() {
                    return Err(Diagnostic::new(format!(
                        "`{}` does not take arguments",
                        field
                    )));
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
            "start" => {
                if args.is_empty() {
                    return Err(Diagnostic::new(format!(
                        "`{}` expects a target function followed by its arguments",
                        field
                    )));
                }
                Err(Diagnostic::new(
                    "task-group start should lower to MIR `Spawn` directly",
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
                        Value::Int(value) => value.as_i128().ok_or_else(|| {
                            Diagnostic::new(
                                "MIR `after(...)` duration must fit in signed timer range",
                            )
                        })?,
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
                    let deadline = deadline_after_millis(millis)?;
                    Ok(Some(deadline))
                }
                _ => Ok(None),
            })
            .collect::<Result<Vec<_>>>()?;
        let ignore_closed_recv = deadlines.iter().any(Option::is_some);

        loop {
            for (index, arm) in arms.iter().enumerate() {
                if let Some(value) =
                    self.try_select_arm(arm, env, deadlines[index], ignore_closed_recv)?
                {
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
        ignore_closed_recv: bool,
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
                    TryRecvResult::Closed if ignore_closed_recv => Ok(None),
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
            Operand::Int(value) => Ok(Value::Int(IntegerValue::from_literal(*value))),
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
                (Value::Int(left), Value::Int(right)) => match left.checked_add(right) {
                    Some(value) => Ok(Value::Int(value)),
                    None => Err(match span {
                        Some(span) => Diagnostic::at(span, "integer overflow"),
                        None => Diagnostic::new("integer overflow"),
                    }),
                },
                (Value::Float(left), Value::Float(right)) => Ok(Value::Float(left + right)),
                (Value::String(left), Value::String(right)) => Ok(Value::String(left + &right)),
                _ => Err(Diagnostic::new(
                    "MIR binary add requires matching supported operand types",
                )),
            },
            BinaryOp::Sub => match (left, right) {
                (Value::Int(left), Value::Int(right)) => match left.checked_sub(right) {
                    Some(value) => Ok(Value::Int(value)),
                    None => Err(match span {
                        Some(span) => Diagnostic::at(span, "integer overflow"),
                        None => Diagnostic::new("integer overflow"),
                    }),
                },
                (Value::Float(left), Value::Float(right)) => Ok(Value::Float(left - right)),
                _ => Err(Diagnostic::new(
                    "MIR binary subtraction requires matching numeric operands",
                )),
            },
            BinaryOp::Mul => match (left, right) {
                (Value::Int(left), Value::Int(right)) => match left.checked_mul(right) {
                    Some(value) => Ok(Value::Int(value)),
                    None => Err(match span {
                        Some(span) => Diagnostic::at(span, "integer overflow"),
                        None => Diagnostic::new("integer overflow"),
                    }),
                },
                (Value::Float(left), Value::Float(right)) => Ok(Value::Float(left * right)),
                _ => Err(Diagnostic::new(
                    "MIR binary multiplication requires matching numeric operands",
                )),
            },
            BinaryOp::Div => match (left, right) {
                (Value::Int(_left), Value::Int(right)) if right.is_zero() => Err(match span {
                    Some(span) => Diagnostic::at(span, "division by zero"),
                    None => Diagnostic::new("division by zero"),
                }),
                (Value::Int(left), Value::Int(right)) => match left.checked_div(right) {
                    Some(value) => Ok(Value::Int(value)),
                    None => Err(match span {
                        Some(span) => Diagnostic::at(span, "integer overflow"),
                        None => Diagnostic::new("integer overflow"),
                    }),
                },
                (Value::Float(_left), Value::Float(right)) if right == 0.0 => Err(match span {
                    Some(span) => Diagnostic::at(span, "division by zero"),
                    None => Diagnostic::new("division by zero"),
                }),
                (Value::Float(left), Value::Float(right)) => Ok(Value::Float(left / right)),
                _ => Err(Diagnostic::new(
                    "MIR binary division requires matching numeric operands",
                )),
            },
            BinaryOp::Mod => match (left, right) {
                (Value::Int(_left), Value::Int(right)) if right.is_zero() => Err(match span {
                    Some(span) => Diagnostic::at(span, "division by zero"),
                    None => Diagnostic::new("division by zero"),
                }),
                (Value::Int(left), Value::Int(right)) => match left.checked_rem(right) {
                    Some(value) => Ok(Value::Int(value)),
                    None => Err(match span {
                        Some(span) => Diagnostic::at(span, "integer overflow"),
                        None => Diagnostic::new("integer overflow"),
                    }),
                },
                (Value::Float(_left), Value::Float(right)) if right == 0.0 => Err(match span {
                    Some(span) => Diagnostic::at(span, "division by zero"),
                    None => Diagnostic::new("division by zero"),
                }),
                (Value::Float(left), Value::Float(right)) => Ok(Value::Float(left % right)),
                _ => Err(Diagnostic::new(
                    "MIR binary remainder requires matching numeric operands",
                )),
            },
            BinaryOp::Less => eval_ordering(BinaryOp::Less, left, right),
            BinaryOp::LessEq => eval_ordering(BinaryOp::LessEq, left, right),
            BinaryOp::Greater => eval_ordering(BinaryOp::Greater, left, right),
            BinaryOp::GreaterEq => eval_ordering(BinaryOp::GreaterEq, left, right),
        }
    }
}

enum BlockOutcome {
    Return(Value),
    Goto(String),
}

fn evaluate_named_args(args: &[MirArg], env: &Env) -> Result<Vec<EvaluatedMirArg>> {
    args.iter()
        .map(|arg| {
            let value = match &arg.value {
                Operand::Place(place) => env.read_place(place)?,
                Operand::Int(value) => Value::Int(IntegerValue::from_literal(*value)),
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

fn collect_runtime_type_substitutions(
    pattern: &Type,
    actual: &Type,
    substitutions: &mut HashMap<String, Type>,
) {
    match pattern {
        Type::TypeParam(name) => {
            substitutions
                .entry(name.clone())
                .or_insert_with(|| actual.clone());
        }
        Type::Named(name, pattern_args) => {
            let Type::Named(actual_name, actual_args) = actual else {
                return;
            };
            if name != actual_name || pattern_args.len() != actual_args.len() {
                return;
            }
            for (pattern_arg, actual_arg) in pattern_args.iter().zip(actual_args.iter()) {
                collect_runtime_type_substitutions(pattern_arg, actual_arg, substitutions);
            }
        }
        Type::Unit | Type::Module(_) => {}
    }
}

fn collect_type_params_from_type(ty: &Type, collected: &mut std::collections::BTreeSet<String>) {
    match ty {
        Type::TypeParam(name) => {
            collected.insert(name.clone());
        }
        Type::Named(_, args) => {
            for arg in args {
                collect_type_params_from_type(arg, collected);
            }
        }
        Type::Unit | Type::Module(_) => {}
    }
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
        (None, Some(stop)) => (IntegerValue::zero(), stop),
        _ => return Err(Diagnostic::new("`range` requires `stop` in MIR runtime")),
    };

    Ok(Value::Range(RangeValue {
        start: start.as_i128().ok_or_else(|| {
            Diagnostic::new("`range` start must fit in signed index space in MIR runtime")
        })?,
        end: stop.as_i128().ok_or_else(|| {
            Diagnostic::new("`range` stop must fit in signed index space in MIR runtime")
        })?,
    }))
}

fn eval_ordering(op: crate::ast::BinaryOp, left: Value, right: Value) -> Result<Value> {
    match (left, right) {
        (Value::Int(left), Value::Int(right)) => Ok(Value::Bool(match op {
            crate::ast::BinaryOp::Less => left < right,
            crate::ast::BinaryOp::LessEq => left <= right,
            crate::ast::BinaryOp::Greater => left > right,
            crate::ast::BinaryOp::GreaterEq => left >= right,
            _ => unreachable!("non-ordering op passed to eval_ordering"),
        })),
        (Value::Float(left), Value::Float(right)) => Ok(Value::Bool(match op {
            crate::ast::BinaryOp::Less => left < right,
            crate::ast::BinaryOp::LessEq => left <= right,
            crate::ast::BinaryOp::Greater => left > right,
            crate::ast::BinaryOp::GreaterEq => left >= right,
            _ => unreachable!("non-ordering op passed to eval_ordering"),
        })),
        _ => Err(Diagnostic::new(
            "MIR ordering comparisons require matching numeric operands",
        )),
    }
}

#[cfg(test)]
#[path = "mir_runtime_tests.rs"]
mod tests;
