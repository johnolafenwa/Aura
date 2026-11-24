//! The compiler-only entry into the interpreter. Only functions whose complete
//! call graph passes the effect check can reach the ordinary execution loop.
use super::*;
use crate::const_eval::{FrozenValue, MEMORY_LIMIT, STEP_LIMIT};
use crate::mir::{CleanupCallee, MethodDispatch};
use crate::runtime_ops::{DictOp, ListOp, SetOp};

pub(crate) fn evaluate_constants(module: MirModule) -> Result<BTreeMap<String, FrozenValue>> {
    // Recursive Aura calls use Rust stack frames. Give evaluation one stack size
    // on every platform and account for each active frame in its memory budget.
    thread::Builder::new().stack_size(32 * 1024 * 1024)
        .spawn(move || evaluate_on_stack(module))
        .map_err(|err| Diagnostic::coded("AU3028", format!("cannot allocate constant evaluation stack: {err}")))?
        .join().unwrap_or_else(|panic| panic::resume_unwind(panic))
}

fn evaluate_on_stack(module: MirModule) -> Result<BTreeMap<String, FrozenValue>> {
    let mut runtime = MirRuntime::new_with_stdout_sink_and_program_args(
        module, Default::default(), None, CancellationContext::default(), Default::default(),
    );
    runtime.safepoints_enabled = false;
    let mut result = BTreeMap::new();
    for constant in runtime.module.constants.clone() {
        if constant.frozen.is_some() || !runtime.pure_initializer(&constant.initializer) {
            continue;
        }
        runtime.eval_budget = Some(Budget::default());
        let value = runtime.read_module_constant(&constant.key, &constant.initializer).map_err(|mut error| {
            if !matches!(error.code.as_str(), "AU3027" | "AU3028") {
                error.code = "AU3026".to_string();
            }
            let binding = constant.key.rsplit("::").next().unwrap_or(&constant.key);
            error.message = format!("while evaluating module constant `{binding}`: {}", error.message);
            for frame in &mut error.call_frames {
                if let Some(key) = frame.function.strip_prefix("__aura_const_init::") {
                    frame.function = format!("const {}", key.rsplit("::").next().unwrap_or(key));
                }
            }
            if error.span.is_none() {
                error.span = error.call_frames.first().map(|frame| frame.span.start);
            }
            error
        })?;
        if let Some(frozen) = FrozenValue::freeze(&value) {
            result.insert(constant.key, frozen);
        }
    }
    Ok(result)
}

impl MirRuntime {
    fn pure_initializer(&self, initializer: &str) -> bool {
        let mut pending = vec![initializer.to_string()];
        let mut visited = HashSet::new();
        while let Some(name) = pending.pop() {
            if !visited.insert(name.clone()) { continue; }
            let Some(function) = self.functions.get(&name) else { return false; };
            pending.extend(function.params.iter().filter_map(|p| p.default_function.clone()));
            for inst in function.blocks.iter().flat_map(|b| &b.instructions) {
                let value = match inst {
                    Instruction::Assign { value, .. } | Instruction::WriteLoan { value, .. } => value,
                    Instruction::PushCleanup { callee: Some(CleanupCallee::Method { function }), .. } => { pending.push(function.clone()); continue; }
                    Instruction::PushCleanup { callee: Some(CleanupCallee::Nothing), .. } => continue,
                    Instruction::PushCleanup { .. } => return false,
                    _ => continue,
                };
                match value {
                    Rvalue::StartTask { .. } => return false,
                    Rvalue::ModuleConstant { initializer, .. } => pending.push(initializer.clone()),
                    Rvalue::Try { from: Some(function), .. } => pending.push(function.clone()),
                    Rvalue::FormatString { .. } => pending.extend(self.display_functions.values().cloned()),
                    Rvalue::Call { callee, .. } => match callee {
                        CallTarget::Name(name) => {
                            if let Some(op) = builtin_function_for_name(name, |name| self.declares_user_function(name)) {
                                if !pure_op(op) { return false; }
                            } else { pending.push(name.clone()); }
                        }
                        CallTarget::Value(Operand::Function { name, .. }) => pending.push(name.clone()),
                        CallTarget::Runtime { op, .. } if pure_op(*op) => {}
                        CallTarget::Member { dispatch: Some(dispatch), .. }
                        | CallTarget::TraitMember { dispatch: Some(dispatch), .. } if pure_dispatch(dispatch, &mut pending) => {}
                        _ => return false,
                    },
                    _ => {}
                }
            }
        }
        true
    }
}

fn pure_dispatch(dispatch: &MethodDispatch, pending: &mut Vec<String>) -> bool {
    match dispatch {
        MethodDispatch::Nothing | MethodDispatch::UnionClone => true,
        MethodDispatch::Static { function } => { pending.push(function.clone()); true }
        MethodDispatch::Runtime(op) => pure_op(*op),
        MethodDispatch::Union(arms) => arms.iter().all(|d| pure_dispatch(d,pending)),
        MethodDispatch::Candidates { candidates, .. } => { pending.extend(candidates.iter().map(|c| c.function.clone())); true }
    }
}

/// Deny newly added runtime operations until their effects have been classified.
fn pure_op(op: RuntimeOp) -> bool {
    match op {
        RuntimeOp::Scalar(_) | RuntimeOp::Value(_) | RuntimeOp::Str(_) | RuntimeOp::List(_)
        | RuntimeOp::Dict(_) | RuntimeOp::Set(_) | RuntimeOp::Span(_) => true,
        RuntimeOp::Function(op) => matches!(op, FunctionOp::Abs | FunctionOp::Min | FunctionOp::Max
            | FunctionOp::Sqrt | FunctionOp::Round | FunctionOp::Divmod | FunctionOp::Ord | FunctionOp::Chr
            | FunctionOp::Range | FunctionOp::ParseInt32 | FunctionOp::ParseInt64 | FunctionOp::ParseFloat64
            | FunctionOp::DurationMilliseconds | FunctionOp::DurationSeconds | FunctionOp::DurationMinutes
            | FunctionOp::ListWithCapacity | FunctionOp::DictWithCapacity | FunctionOp::SetWithCapacity
            | FunctionOp::ListEmpty | FunctionOp::DictEmpty | FunctionOp::SetEmpty
            | FunctionOp::TimeRfc3339 | FunctionOp::TimeFormat | FunctionOp::TimeParseRfc3339 | FunctionOp::TimeParse
            | FunctionOp::TimeFailed | FunctionOp::TimeParsedSeconds | FunctionOp::TimeParsedNanos
            | FunctionOp::TimeParsedOffset | FunctionOp::TimeDurationSeconds | FunctionOp::TimeDurationNanos | FunctionOp::TimeDuration
            | FunctionOp::SmallPowMod | FunctionOp::BigAddWords),
        RuntimeOp::Host(host) => matches!(host.name().split_once("::"), Some(("bytes" | "text" | "math" | "json" | "toml", _))),
        _ => false,
    }
}

#[derive(Default)]
pub(super) struct Budget { steps: usize, bytes: usize }

impl Budget {
    pub(super) fn step(&mut self) -> Result<()> {
        self.steps += 1;
        if self.steps > STEP_LIMIT {
            return Err(Diagnostic::coded("AU3027", format!("constant evaluation exceeds the limit of {STEP_LIMIT} MIR steps")));
        }
        Ok(())
    }

    fn allocate(&mut self, bytes: usize) -> Result<()> {
        self.bytes = self.bytes.saturating_add(bytes);
        self.check_memory(0)
    }

    pub(super) fn check_memory(&self, frame_bytes: usize) -> Result<()> {
        if self.bytes.saturating_add(frame_bytes) > MEMORY_LIMIT {
            return Err(Diagnostic::coded("AU3028", "constant evaluation exceeds the memory limit of 8 MiB"));
        }
        Ok(())
    }

    pub(super) fn instruction(&mut self, instruction: &Instruction, env: &Env) -> Result<()> {
        self.step()?;
        let (Instruction::Assign { value, .. } | Instruction::WriteLoan { value, .. }) = instruction else { return Ok(()); };
        let payload = |op: &Operand| match op {
            Operand::String(s) => s.len(),
            Operand::Place(p) | Operand::MovePlace(p) => env.place_ref(p).map_or(0, |v| match &*v {
                Value::String(s) => s.len(),
                Value::Vec(v) => v.elements.len().saturating_mul(8),
                Value::Map(v) => v.entries.len().saturating_mul(16),
                Value::Set(v) => v.elements.len().saturating_mul(8),
                _ => 0,
            }),
            _ => 0,
        };
        let count = |arg: Option<&MirArg>| match arg.map(|a| &a.value) {
            Some(Operand::Int(n)) => usize::try_from(*n).unwrap_or(usize::MAX),
            Some(Operand::MovePlace(p) | Operand::Place(p)) => env.place_ref(p).ok().and_then(|v| match &*v {
                Value::Int(n) => n.as_i128().map(|i| usize::try_from(i.max(0)).unwrap_or(usize::MAX)),
                _ => None,
            }).unwrap_or(0),
            _ => 0,
        };
        let bytes = match value {
            Rvalue::Use(Operand::String(s)) => s.len(),
            Rvalue::VecLiteral { elements,.. } | Rvalue::TupleLiteral { elements,.. } | Rvalue::SetLiteral { elements,.. } => elements.iter().fold(elements.len().saturating_mul(8), |n,v| n.saturating_add(payload(v))),
            Rvalue::MapLiteral { entries,.. } => entries.len().saturating_mul(16),
            Rvalue::Construct { fields,.. } => fields.len().saturating_mul(8),
            Rvalue::EnumVariant { payloads,.. } => payloads.len().saturating_mul(8),
            Rvalue::Binary { op: BinaryOp::Add, left, right, .. } => payload(left).saturating_add(payload(right)),
            Rvalue::Call { callee: CallTarget::Runtime { op, receiver, .. }, args, .. } => {
                let n = receiver.as_ref().map_or(0,payload);
                match op {
                    RuntimeOp::Str(StrOp::Repeat) => n.saturating_mul(count(args.first())),
                    RuntimeOp::Str(StrOp::Replace) => n.saturating_add((n+1).saturating_mul(args.get(1).map_or(0, |a| payload(&a.value)))),
                    RuntimeOp::Function(FunctionOp::ListWithCapacity | FunctionOp::SetWithCapacity | FunctionOp::DictWithCapacity)
                    | RuntimeOp::List(ListOp::Reserve) | RuntimeOp::Set(SetOp::Reserve) | RuntimeOp::Dict(DictOp::Reserve) => count(args.first()).saturating_mul(16),
                    RuntimeOp::Value(_) | RuntimeOp::Str(_) => n,
                    RuntimeOp::List(ListOp::Extend) => args.first().map_or(0,|a|payload(&a.value)),
                    RuntimeOp::List(ListOp::Push | ListOp::PushScalar(_) | ListOp::Insert) | RuntimeOp::Dict(DictOp::Set | DictOp::SetIndex) | RuntimeOp::Set(SetOp::Add | SetOp::AddInt64) => 16,
                    _ => 0,
                }
            }
            _ => 0,
        };
        self.allocate(bytes)
    }
}
