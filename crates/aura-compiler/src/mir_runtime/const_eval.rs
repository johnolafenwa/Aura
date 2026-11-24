//! The compiler-only entry into the interpreter. Only functions whose complete
//! call graph passes the effect check can reach the ordinary execution loop.
use super::*;
use crate::const_eval::{FrozenValue, MEMORY_LIMIT, STEP_LIMIT};
use crate::mir::{CleanupCallee, MethodDispatch};
use crate::runtime_ops::{DictOp, ListOp, SetOp};

pub(crate) fn evaluate_constants(module: MirModule) -> Result<BTreeMap<String, FrozenValue>> {
    use aura_runtime::array_simd::{with_kernel_level, KernelLevel};
    // Recursive Aura calls use Rust stack frames. Give evaluation one stack size
    // on every platform and account for each active frame in its memory budget.
    let windows_paths = crate::current_build_config().os == "windows";
    thread::Builder::new()
        .stack_size(32 * 1024 * 1024)
        .spawn(move || {
            with_kernel_level(KernelLevel::Baseline, || {
                evaluate_on_stack(module, windows_paths)
            })
        })
        .map_err(|err| {
            Diagnostic::coded(
                "AU3028",
                format!("cannot allocate constant evaluation stack: {err}"),
            )
        })?
        .join()
        .unwrap_or_else(|panic| panic::resume_unwind(panic))
}

fn evaluate_on_stack(
    module: MirModule,
    windows_paths: bool,
) -> Result<BTreeMap<String, FrozenValue>> {
    let mut runtime = MirRuntime::new_with_stdout_sink_and_program_args(
        module,
        Default::default(),
        None,
        CancellationContext::default(),
        Default::default(),
    );
    runtime.safepoints_enabled = false;
    let mut result = BTreeMap::new();
    let module = Arc::clone(&runtime.module);
    let mut unavailable = HashSet::new();
    for constant in &module.constants {
        if constant.frozen.is_some() {
            continue;
        }
        if !runtime.pure_initializer(&constant.initializer, &unavailable) {
            unavailable.insert(constant.key.clone());
            continue;
        }
        runtime.eval_budget = Some(Budget {
            windows_paths,
            ..Budget::default()
        });
        let value = aura_runtime::runtime_value::regex_host::with_local_patterns(|| {
            runtime.read_module_constant(&constant.key, &constant.initializer)
        })
        .and_then(|value| {
            if const_memory::value_bytes(&value, 0) > MEMORY_LIMIT {
                return Err(Budget::memory_error());
            }
            Ok(value)
        })
        .map_err(|mut error| {
            if !matches!(error.code.as_str(), "AU3027" | "AU3028") {
                error.code = "AU3026".to_string();
            }
            let binding = constant.key.rsplit("::").next().unwrap_or(&constant.key);
            error.message = format!(
                "while evaluating module constant `{binding}`: {}",
                error.message
            );
            for frame in &mut error.call_frames {
                if let Some(key) = frame.function.strip_prefix("__aura_const_init::") {
                    frame.function = format!("const {}", key.rsplit("::").next().unwrap_or(key));
                }
            }
            if error.span.is_none() {
                error.span = error
                    .call_frames
                    .first()
                    .map(|frame| frame.span.start)
                    .or_else(|| runtime.functions.get(&constant.initializer).map(|f| f.span));
            }
            error
        })?;
        if let Some(frozen) = FrozenValue::freeze(&value) {
            result.insert(constant.key.clone(), frozen);
        } else {
            unavailable.insert(constant.key.clone());
        }
    }
    Ok(result)
}

impl MirRuntime {
    fn pure_initializer(&self, initializer: &str, unavailable: &HashSet<String>) -> bool {
        let mut pending = vec![initializer.to_string()];
        let mut visited = HashSet::new();
        while let Some(name) = pending.pop() {
            if !visited.insert(name.clone()) {
                continue;
            }
            let Some(function) = self.functions.get(&name) else {
                return false;
            };
            // Any callable entering this closed graph originates in a function
            // operand, a closure constructor, or another checked callee.
            for block in &function.blocks {
                let operands = block
                    .instructions
                    .iter()
                    .flat_map(crate::mir::ownership::operand_roles)
                    .chain(crate::mir::ownership::terminator_roles(&block.terminator));
                for (operand, _) in operands {
                    if let Operand::Function { name, .. } = operand {
                        self.queue_callee(&mut pending, name, None);
                    }
                }
            }
            for inst in function.blocks.iter().flat_map(|b| &b.instructions) {
                let value = match inst {
                    Instruction::Assign { value, .. } | Instruction::WriteLoan { value, .. } => {
                        value
                    }
                    Instruction::PushCleanup {
                        callee: Some(CleanupCallee::Method { function }),
                        ..
                    } => {
                        pending.push(function.clone());
                        continue;
                    }
                    Instruction::PushCleanup {
                        callee: Some(CleanupCallee::Nothing),
                        ..
                    } => continue,
                    Instruction::PushCleanup { .. } => return false,
                    _ => continue,
                };
                match value {
                    Rvalue::StartTask { .. } => return false,
                    Rvalue::Closure { function, .. } => {
                        self.queue_callee(&mut pending, function, None)
                    }
                    Rvalue::ModuleConstant { key, .. } if unavailable.contains(key) => {
                        return false
                    }
                    Rvalue::ModuleConstant { initializer, .. } => pending.push(initializer.clone()),
                    Rvalue::Try {
                        from: Some(function),
                        ..
                    } => pending.push(function.clone()),
                    Rvalue::FormatString { parts } if !self.display_functions.is_empty() => {
                        let mut env = Env::default();
                        for local in &function.local_types {
                            env.set_place_type(&local.name, local.ty.clone());
                        }
                        for param in &function.params {
                            env.set_place_type(&param.name, param.ty.clone());
                        }
                        for part in parts {
                            let ty = match part {
                                MirFormatPart::Formatted { value_type, .. } => {
                                    Some(value_type.clone())
                                }
                                MirFormatPart::Value(value) => {
                                    self.resolve_operand_type(value, &env)
                                }
                                _ => continue,
                            };
                            if !self.queue_display(ty, &mut pending) {
                                return false;
                            }
                        }
                    }
                    Rvalue::Call { callee, args, .. } => match callee {
                        CallTarget::Name(name) => {
                            if let Some(op) = builtin_function_for_name(name, |name| {
                                self.declares_user_function(name)
                            }) {
                                if !pure_op(op, &function.module_name) {
                                    return false;
                                }
                            } else {
                                self.queue_callee(&mut pending, name, Some(args));
                            }
                        }
                        CallTarget::Value(_) => {}
                        CallTarget::Runtime { op, .. } if pure_op(*op, &function.module_name) => {}
                        CallTarget::Member {
                            dispatch: Some(dispatch),
                            ..
                        }
                        | CallTarget::TraitMember {
                            dispatch: Some(dispatch),
                            ..
                        } if self.pure_dispatch(
                            dispatch,
                            args,
                            &function.module_name,
                            &mut pending,
                        ) => {}
                        _ => return false,
                    },
                    _ => {}
                }
            }
        }
        true
    }

    /// Formatting a container walks its payload until a nominal Display
    /// implementation takes over. Use the same field and variant types here.
    fn queue_display(&self, ty: Option<Type>, callees: &mut Vec<String>) -> bool {
        let mut types = Vec::from_iter(ty);
        let mut expanded = HashSet::new();
        if types.is_empty() {
            callees.extend(self.display_functions.values().cloned());
        }
        while let Some(ty) = types.pop() {
            match ty {
                Type::Named(name, args) => {
                    if let Some(display) = self.display_functions.get(&name) {
                        callees.push(display.clone());
                        continue;
                    }
                    // Rendering opaque regex wrappers would expose a local pattern index.
                    if matches!(name.as_str(), "regex.Regex" | "regex.Captures") {
                        return false;
                    }
                    types.extend(args.iter().cloned());
                    if !expanded.insert(name.clone()) {
                        continue;
                    }
                    if let Some(class) = self.classes.get(&name) {
                        let substitutions = class.type_params.iter().cloned().zip(args).collect();
                        types.extend(
                            class
                                .fields
                                .iter()
                                .map(|field| substitute_type(&field.ty, &substitutions)),
                        );
                    } else if let Some(en) = self.module.enums.iter().find(|en| en.name == name) {
                        let substitutions = en.type_params.iter().cloned().zip(args).collect();
                        types.extend(
                            en.variants
                                .iter()
                                .flat_map(|item| &item.payloads)
                                .map(|payload| substitute_type(payload, &substitutions)),
                        );
                    } else if name == "Unknown"
                        || self.trait_impls.iter().any(|imp| imp.trait_name == name)
                    {
                        callees.extend(self.display_functions.values().cloned());
                    }
                }
                Type::Tuple(items) => types.extend(items),
                Type::Union(u) => types.extend(u.members),
                Type::TypeParam(_) => callees.extend(self.display_functions.values().cloned()),
                _ => {}
            }
        }
        true
    }

    fn queue_callee(&self, pending: &mut Vec<String>, name: &str, args: Option<&[MirArg]>) {
        pending.push(name.to_string());
        let Some(function) = self.functions.get(name) else {
            return;
        };
        let positional = args.map_or(0, |args| {
            args.iter().take_while(|arg| arg.name.is_none()).count()
        });
        for (index, param) in function.params.iter().enumerate() {
            if let Some(default) = &param.default_function {
                if index >= positional
                    && !args.is_some_and(|args| {
                        args.iter()
                            .any(|arg| arg.name.as_ref() == Some(&param.name))
                    })
                {
                    pending.push(default.clone());
                }
            }
        }
    }

    fn pure_dispatch(
        &self,
        dispatch: &MethodDispatch,
        args: &[MirArg],
        module: &str,
        pending: &mut Vec<String>,
    ) -> bool {
        match dispatch {
            MethodDispatch::Nothing | MethodDispatch::UnionClone => true,
            MethodDispatch::Static { function } => {
                self.queue_callee(pending, function, Some(args));
                true
            }
            MethodDispatch::Runtime(op) => pure_op(*op, module),
            MethodDispatch::Union(arms) => arms
                .iter()
                .all(|d| self.pure_dispatch(d, args, module, pending)),
            MethodDispatch::Candidates { candidates, .. } => {
                for candidate in candidates {
                    self.queue_callee(pending, &candidate.function, Some(args));
                }
                true
            }
        }
    }
}

/// Deny newly added runtime operations until their effects have been classified.
fn pure_op(op: RuntimeOp, module: &str) -> bool {
    match op {
        RuntimeOp::Scalar(_)
        | RuntimeOp::Value(_)
        | RuntimeOp::Str(_)
        | RuntimeOp::List(_)
        | RuntimeOp::Dict(_)
        | RuntimeOp::Set(_)
        | RuntimeOp::Span(_) => true,
        RuntimeOp::Array(_) => true,
        RuntimeOp::Function(
            FunctionOp::RegexIsMatch
            | FunctionOp::RegexCount
            | FunctionOp::RegexGroupIndex
            | FunctionOp::RegexFind
            | FunctionOp::RegexMatches
            | FunctionOp::RegexFindAll
            | FunctionOp::RegexCaptures
            | FunctionOp::RegexSplit
            | FunctionOp::RegexReplace
            | FunctionOp::RegexReplaceAll,
        ) => module == "regex",
        RuntimeOp::Function(
            FunctionOp::TimeFormat
            | FunctionOp::TimeParseRfc3339
            | FunctionOp::TimeParse
            | FunctionOp::TimeFailed
            | FunctionOp::TimeParsedSeconds
            | FunctionOp::TimeParsedNanos
            | FunctionOp::TimeParsedOffset,
        ) => module == "time",
        RuntimeOp::Function(op) => matches!(
            op,
            FunctionOp::ArrayZeros
                | FunctionOp::ArrayFull
                | FunctionOp::ArrayFromList
                | FunctionOp::Abs
                | FunctionOp::Min
                | FunctionOp::Max
                | FunctionOp::Sqrt
                | FunctionOp::Round
                | FunctionOp::Divmod
                | FunctionOp::Ord
                | FunctionOp::Chr
                | FunctionOp::Range
                | FunctionOp::ParseInt32
                | FunctionOp::ParseInt64
                | FunctionOp::ParseFloat64
                | FunctionOp::DurationMilliseconds
                | FunctionOp::DurationSeconds
                | FunctionOp::DurationMinutes
                | FunctionOp::ListWithCapacity
                | FunctionOp::DictWithCapacity
                | FunctionOp::SetWithCapacity
                | FunctionOp::ListEmpty
                | FunctionOp::DictEmpty
                | FunctionOp::SetEmpty
                | FunctionOp::TimeRfc3339
                | FunctionOp::TimeDurationSeconds
                | FunctionOp::TimeDurationNanos
                | FunctionOp::TimeDuration
                | FunctionOp::SmallPowMod
                | FunctionOp::BigAddWords
        ),
        RuntimeOp::Host(host)
            if matches!(
                host.name(),
                "str.to_bytes" | "str.from_bytes" | "regex::__aura_escape"
            ) =>
        {
            true
        }
        RuntimeOp::Host(host) if host.name() == "time::__aura_error" => module == "time",
        RuntimeOp::Host(host) if host.name().starts_with("regex::") => module == "regex",
        RuntimeOp::Host(host) if host.name().starts_with("collections::") => {
            module == "collections"
        }
        RuntimeOp::Host(host) => matches!(
            host.name().split_once("::"),
            Some((
                "bytes" | "text" | "math" | "json" | "toml" | "path" | "gzip" | "numbers" | "image",
                _
            ))
        ),
        _ => false,
    }
}

#[derive(Default)]
pub(super) struct Budget {
    windows_paths: bool,
    steps: usize,
    bytes: usize,
    frame_bytes: usize,
    reserved: usize,
}

impl Budget {
    pub(super) fn remaining(&self) -> usize {
        MEMORY_LIMIT.saturating_sub(self.bytes.saturating_add(self.frame_bytes))
    }

    pub(super) fn output_allowance(&self) -> usize {
        self.remaining().saturating_add(self.reserved)
    }

    pub(super) fn builtin_result(&mut self, value: &Value) -> Result<()> {
        match value {
            Value::Unit | Value::Bool(_) | Value::Int(_) | Value::Float(_) | Value::Duration(_) => {
                Ok(())
            }
            _ => self.allocate(const_memory::value_bytes(value, 0)),
        }
    }

    /// Lexical path operations use the output platform's separators and root rules.
    pub(super) fn path_builtin(&self, name: &str, args: &[EvaluatedMirArg]) -> Result<Value> {
        use typed_path::Utf8TypedPath;
        let text = |index: usize| match &args[index].value {
            Value::String(text) => Ok(text.as_str()),
            _ => Err(Diagnostic::new("expected a string path")),
        };
        let base = text(0)?;
        let path = if self.windows_paths {
            Utf8TypedPath::windows(base)
        } else {
            Utf8TypedPath::unix(base)
        };
        let component = match name {
            "path::join" => return Ok(Value::string(path.join(text(1)?).as_str())),
            "path::is_absolute" => return Ok(Value::Bool(path.is_absolute())),
            "path::parent" => path.parent().map(|p| Value::string(p.as_str())),
            "path::file_name" => path.file_name().map(Value::string),
            "path::extension" => path.extension().map(Value::string),
            _ => return Err(Diagnostic::new("unknown lexical path operation")),
        };
        Ok(component
            .map(|value| optional_present(Type::named("str"), value))
            .unwrap_or_else(|| optional_absent(Type::named("str"))))
    }

    pub(super) fn step(&mut self) -> Result<()> {
        self.steps += 1;
        if self.steps > STEP_LIMIT {
            return Err(Diagnostic::coded(
                "AU3027",
                format!("constant evaluation exceeds the limit of {STEP_LIMIT} MIR steps"),
            ));
        }
        Ok(())
    }

    fn allocate(&mut self, bytes: usize) -> Result<()> {
        self.bytes = self.bytes.saturating_add(bytes);
        self.check_memory(self.frame_bytes)
    }

    pub(super) fn memory_error() -> Diagnostic {
        Diagnostic::coded(
            "AU3028",
            "constant evaluation exceeds the memory limit of 8 MiB",
        )
    }

    pub(super) fn check_memory(&self, frame_bytes: usize) -> Result<()> {
        if self.bytes.saturating_add(frame_bytes) > MEMORY_LIMIT {
            return Err(Self::memory_error());
        }
        Ok(())
    }

    pub(super) fn instruction(
        &mut self,
        instruction: &Instruction,
        env: &Env,
        depth: usize,
    ) -> Result<()> {
        self.frame_bytes = depth.saturating_mul(128 * 1024);
        self.reserved = 0;
        self.step()?;
        let (Instruction::Assign { value, .. } | Instruction::WriteLoan { value, .. }) =
            instruction
        else {
            return Ok(());
        };
        let payload = |op: &Operand| const_memory::operand_bytes(op, env);
        let bytes = match value {
            Rvalue::Use(Operand::String(s)) => s.len(),
            Rvalue::VecLiteral { elements, .. }
            | Rvalue::TupleLiteral { elements, .. }
            | Rvalue::SetLiteral { elements, .. } => elements
                .iter()
                .fold(elements.len().saturating_mul(8), |n, v| {
                    n.saturating_add(payload(v))
                }),
            Rvalue::FormatString { parts } => const_memory::format_bytes(parts, env),
            Rvalue::MapLiteral { entries, .. } => entries.iter().fold(0usize, |n, entry| {
                n.saturating_add(payload(&entry.key))
                    .saturating_add(payload(&entry.value))
                    .saturating_add(16)
            }),
            Rvalue::Construct { fields, .. } => fields.iter().fold(const_memory::WORD, |n, f| {
                n.saturating_add(payload(&f.value))
            }),
            Rvalue::EnumVariant { payloads, .. } => payloads
                .iter()
                .fold(8usize, |n, v| n.saturating_add(payload(v))),
            Rvalue::Binary {
                op: BinaryOp::Add,
                left,
                right,
                ..
            } if matches!(left, Operand::String(_))
                || matches!(left, Operand::Place(p) | Operand::MovePlace(p) if env.place_ref(p).is_ok_and(|v|matches!(&*v, Value::String(_)))) =>
            {
                payload(left).saturating_add(payload(right))
            }
            Rvalue::Call { callee, args, .. } => {
                let (op, receiver) = match callee {
                    CallTarget::Runtime { op, receiver, .. } => (*op, receiver.as_ref()),
                    CallTarget::Member {
                        dispatch: Some(MethodDispatch::Runtime(op)),
                        object,
                        ..
                    }
                    | CallTarget::TraitMember {
                        dispatch: Some(MethodDispatch::Runtime(op)),
                        object,
                        ..
                    } => (*op, Some(object)),
                    _ => return Ok(()),
                };
                match op {
                    RuntimeOp::Str(op) => {
                        receiver.map_or(0, |r| const_memory::text_output(op, r, args, env))
                    }
                    RuntimeOp::Function(
                        FunctionOp::ListWithCapacity
                        | FunctionOp::SetWithCapacity
                        | FunctionOp::DictWithCapacity,
                    )
                    | RuntimeOp::List(ListOp::Reserve)
                    | RuntimeOp::Set(SetOp::Reserve)
                    | RuntimeOp::Dict(DictOp::Reserve) => {
                        const_memory::count(args.first(), env).saturating_mul(16)
                    }
                    RuntimeOp::Value(_) => receiver.map_or(0, payload),
                    RuntimeOp::List(
                        ListOp::Slice
                        | ListOp::SliceStep
                        | ListOp::Sort
                        | ListOp::SortByKeys
                        | ListOp::Permute,
                    )
                    | RuntimeOp::Dict(DictOp::Keys | DictOp::Values | DictOp::Items) => {
                        receiver.map_or(0, payload)
                    }
                    RuntimeOp::Set(
                        SetOp::Union
                        | SetOp::SymmetricDifference
                        | SetOp::Difference
                        | SetOp::Intersection,
                    ) => receiver
                        .map_or(0, payload)
                        .saturating_add(args.first().map_or(0, |a| payload(&a.value))),
                    RuntimeOp::Dict(DictOp::Update)
                    | RuntimeOp::Set(SetOp::Update)
                    | RuntimeOp::List(ListOp::Extend) => {
                        args.first().map_or(0, |a| payload(&a.value))
                    }
                    RuntimeOp::List(ListOp::Push | ListOp::PushScalar(_) | ListOp::Insert)
                    | RuntimeOp::Dict(DictOp::Set | DictOp::SetIndex)
                    | RuntimeOp::Set(SetOp::Add | SetOp::AddInt64) => args
                        .iter()
                        .fold(16usize, |n, a| n.saturating_add(payload(&a.value))),
                    _ => 0,
                }
            }
            _ => 0,
        };
        self.reserved = bytes;
        self.allocate(bytes)
    }
}
