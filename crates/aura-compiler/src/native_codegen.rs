use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};

use crate::ast::{BinaryOp, ReceiverKind, UnaryOp};
use crate::diag::Span;
use crate::ffi::FfiType;
use crate::mir::{
    ArithmeticTrap, BasicBlock, CallTarget, CleanupAction, CleanupCallee, Instruction,
    MethodCandidate, MethodDispatch, MirArg, MirClass, MirEnum, MirExternCall, MirFormatPart,
    MirFunction, MirMapEntry, MirModule, MirReceiverKind, Operand, Rvalue, Terminator,
    NATIVE_LOOP_SAFEPOINT_INTERVAL,
};
use crate::native_backend::{
    types, Blk, CallConv, DataDescription, DataHandle, DefineError, FloatCC, FuncHandle, FuncRef,
    FunctionBackend, Inst, IntCC, Linkage, MemFlags, ModuleBackend, ScalarType, Signature,
    TrapCode, Val, Var,
};
use crate::native_runtime::{
    encode_direct_ffi_call_spec, DirectFfiCallSpec, DirectFfiParam, DirectFfiType,
};
use crate::runtime_abi::{
    frame_stack_layout, ownership_layout, AbiType, RuntimeFn, RuntimeTable, FRAME_STACK_ENTRY_BYTES,
};
use crate::runtime_ops::{
    first_type_arg, ArgDefault, ArrayOp, DictOp, FunctionOp, HostBuiltin, ListOp, PrintKind,
    RuntimeOp, RuntimeTarget, ScalarElement, ScalarOp, SetOp, SpanOp, StrOp, ValueOp,
};
use crate::sema::optional_type;
use crate::sema::{substitute_type, FunctionParamContract, Type};

use callables::{CallableShape, CallableShapeKey};

const MODULE_BACKEND_BUSY_ERROR: &str =
    "internal error: direct backend module is defining another function";
const WIDE_INTEGER_BINARY_ERROR: &str =
    "direct backend does not support wide integer binary operation";
const DIRECT_INTERNAL_SLICE_ARITY_ERROR: &str =
    "direct backend expected internal slicing to receive start, start presence, end, end presence, line, and column";

pub fn emit_host_object(module: &MirModule) -> std::result::Result<Vec<u8>, String> {
    emit_host_object_with_metadata(module, "<aura>", "")
}

pub fn emit_host_object_with_metadata(
    module: &MirModule,
    program_path: &str,
    program_source: &str,
) -> std::result::Result<Vec<u8>, String> {
    let mut module = reachability::prune_unreachable_functions(module);
    // Hand-built or older MIR may still spell resolved operations as member
    // calls; the pass is idempotent on lowered MIR.
    crate::mir::resolve_runtime_ops(&mut module);
    crate::mir::checks::mark_covered_accesses(&mut module);
    // The emitter carries out the ownership the module states. The
    // boundary gives an implicit module the ownership pass first.
    crate::mir::validate_at_boundary(&mut module).map_err(|error| match error {
        crate::mir::BoundaryError::LoanFlow(error)
        | crate::mir::BoundaryError::Ownership(error)
        | crate::mir::BoundaryError::Checks(error) => error,
    })?;
    let context = NativeCodegen::with_validated_loans(&module, program_path, program_source)?;
    context.emit()
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ScalarKind {
    Int32,
    Int64,
    Uint64,
    Float32,
    Float64,
    Bool,
    Unit,
}

impl ScalarKind {
    fn signature_type(self) -> ScalarType {
        match self {
            ScalarKind::Int32
            | ScalarKind::Int64
            | ScalarKind::Uint64
            | ScalarKind::Bool
            | ScalarKind::Unit => types::I64,
            ScalarKind::Float32 | ScalarKind::Float64 => types::F64,
        }
    }

    fn zero_value(self, builder: &mut dyn FunctionBackend) -> Val {
        match self {
            ScalarKind::Int32
            | ScalarKind::Int64
            | ScalarKind::Uint64
            | ScalarKind::Bool
            | ScalarKind::Unit => builder.iconst(types::I64, 0),
            ScalarKind::Float32 | ScalarKind::Float64 => builder.f64const(0.0),
        }
    }

    fn is_float(self) -> bool {
        matches!(self, ScalarKind::Float32 | ScalarKind::Float64)
    }

    fn is_integer(self) -> bool {
        matches!(
            self,
            ScalarKind::Int32 | ScalarKind::Int64 | ScalarKind::Uint64
        )
    }
}

fn direct_array_dtype_code(ty: &Type) -> std::result::Result<i64, String> {
    match ty {
        Type::Named(name, arguments) if arguments.is_empty() => match name.as_str() {
            "int32" => Ok(0),
            "int64" => Ok(1),
            "float32" => Ok(2),
            "float64" => Ok(3),
            _ => Err(format!(
                "direct backend does not support Array dtype `{ty}`"
            )),
        },
        _ => Err(format!(
            "direct backend does not support Array dtype `{ty}`"
        )),
    }
}

fn direct_array_binary_opcode(op: BinaryOp) -> std::result::Result<i64, String> {
    match op {
        BinaryOp::Add => Ok(0),
        BinaryOp::Sub => Ok(1),
        BinaryOp::Mul => Ok(2),
        BinaryOp::Div => Ok(3),
        _ => Err(format!(
            "direct backend does not support Array binary operation `{op:?}`"
        )),
    }
}

fn direct_array_element_type(ty: &DirectType) -> Option<&Type> {
    match ty {
        DirectType::Opaque(Type::Named(name, arguments))
            if name == "Array" && arguments.len() == 1 =>
        {
            arguments.first()
        }
        _ => None,
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(i64)]
enum WideIntegerKind {
    Int64 = 0,
    Uint64 = 1,
}

impl WideIntegerKind {
    fn scalar_kind(self) -> ScalarKind {
        match self {
            WideIntegerKind::Int64 => ScalarKind::Int64,
            WideIntegerKind::Uint64 => ScalarKind::Uint64,
        }
    }

    fn is_signed(self) -> bool {
        matches!(self, WideIntegerKind::Int64)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(i64)]
enum WideOverflowOp {
    Add = 0,
    Sub = 1,
    Mul = 2,
    Div = 3,
}

#[derive(Clone, Debug, Eq, PartialEq)]
enum DirectType {
    Scalar(ScalarKind),
    PlainClass(PlainClassType),
    /// An inline union: one tag word followed by the payload words of the
    /// widest member (checkpoint Q9 A). Every member is itself inline, so the
    /// value owns nothing and crosses into the runtime as `Value::Union` only
    /// at container and helper boundaries.
    Union(DirectUnionType),
    /// An inline callable: a descriptor word naming the value's shape (its
    /// lowered function and capture layout) followed by three environment
    /// words (checkpoint Q15 A / Q16 A). Captures that fit in three words
    /// live inline; a wider environment lives in one checked heap block
    /// named by the first word. The descriptor dispatches the value's
    /// calls, releases, and boundary boxes, so two lambdas of one static
    /// type may share a local.
    Callable(DirectCallableType),
    Opaque(Type),
}

/// The words after a callable's descriptor.
const CALLABLE_ENVIRONMENT_WORDS: usize = 3;

#[derive(Clone, Debug, Eq, PartialEq)]
struct DirectCallableType {
    /// The value's static contract (`Type::Function`, `Type::Closure`, or
    /// `Type::Callable`), which fixes the public direct signature of a
    /// call through the value.
    signature: Type,
}

impl DirectCallableType {
    fn new(signature: Type) -> Self {
        Self { signature }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct DirectUnionType {
    union_type: Type,
    members: Vec<DirectUnionMember>,
    /// Payload words after the tag: the widest member's word count.
    payload_words: usize,
    /// Members whose payload word is an owned runtime handle when active.
    owning: Vec<usize>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct DirectUnionMember {
    ty: DirectType,
}

impl DirectUnionType {
    fn new(union_type: Type, members: Vec<DirectType>) -> Self {
        let payload_words = members
            .iter()
            .map(DirectType::value_count)
            .max()
            .unwrap_or(0);
        let owning = members
            .iter()
            .enumerate()
            .filter(|(_, ty)| matches!(ty, DirectType::Opaque(_) | DirectType::Callable(_)))
            .map(|(index, _)| index)
            .collect();
        Self {
            union_type,
            members: members
                .into_iter()
                .map(|ty| DirectUnionMember { ty })
                .collect(),
            payload_words,
            owning,
        }
    }

    /// Whether some member's payload word is an owned handle.
    fn owns_handles(&self) -> bool {
        !self.owning.is_empty()
    }

    fn member(&self, index: usize) -> std::result::Result<&DirectType, String> {
        self.members
            .get(index)
            .map(|member| &member.ty)
            .ok_or_else(|| {
                format!(
                    "union member index {index} is out of range for `{}`",
                    self.union_type
                )
            })
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct PlainClassType {
    class_name: String,
    fields: Vec<PlainClassField>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct PlainClassField {
    name: String,
    ty: DirectType,
}

impl DirectType {
    fn abi_types(&self) -> Vec<ScalarType> {
        match self {
            DirectType::Scalar(kind) => vec![kind.signature_type()],
            DirectType::Union(union) => {
                let mut types = vec![types::I64];
                types.extend(std::iter::repeat_n(types::I64, union.payload_words));
                types
            }
            DirectType::PlainClass(class) => {
                let mut types = Vec::new();
                for field in &class.fields {
                    types.extend(field.ty.abi_types());
                }
                types
            }
            DirectType::Callable(_) => vec![types::I64; 1 + CALLABLE_ENVIRONMENT_WORDS],
            DirectType::Opaque(_) => vec![types::I64],
        }
    }

    fn value_count(&self) -> usize {
        self.abi_types().len()
    }

    fn scalar_kind(&self) -> Option<ScalarKind> {
        match self {
            DirectType::Scalar(kind) => Some(*kind),
            DirectType::PlainClass(_)
            | DirectType::Union(_)
            | DirectType::Callable(_)
            | DirectType::Opaque(_) => None,
        }
    }

    fn zero_values(&self, builder: &mut dyn FunctionBackend) -> Vec<Val> {
        match self {
            DirectType::Scalar(kind) => vec![kind.zero_value(builder)],
            DirectType::PlainClass(class) => {
                let mut values = Vec::new();
                for field in &class.fields {
                    values.extend(field.ty.zero_values(builder));
                }
                values
            }
            DirectType::Union(union) => {
                let mut values = vec![builder.iconst(types::I64, 0)];
                for _ in 0..union.payload_words {
                    values.push(builder.iconst(types::I64, 0));
                }
                values
            }
            DirectType::Callable(_) => (0..1 + CALLABLE_ENVIRONMENT_WORDS)
                .map(|_| builder.iconst(types::I64, 0))
                .collect(),
            DirectType::Opaque(_) => vec![builder.iconst(types::I64, 0)],
        }
    }

    fn field_slice(&self, field_name: &str) -> Option<(usize, usize, DirectType)> {
        if let DirectType::Union(union) = self {
            // A payload projection selects the member's words after the tag;
            // the words are reinterpreted by `extract_field`/`replace_nested_field`.
            let index = crate::mir::union_payload_projection_index(field_name)?;
            let member = union.member(index).ok()?;
            return Some((1, 1 + member.value_count(), member.clone()));
        }
        let DirectType::PlainClass(class) = self else {
            return None;
        };

        let mut start = 0usize;
        for field in &class.fields {
            let end = start + field.ty.value_count();
            if field.name == field_name {
                return Some((start, end, field.ty.clone()));
            }
            start = end;
        }
        None
    }
}

#[derive(Clone)]
struct ValueRef {
    values: Vec<Val>,
    ty: DirectType,
}

/// The layout temporaries one MIR instruction owns while it is emitted:
/// opaque handles, and inline unions whose active member may hold a handle
/// (keyed by the tag word). A boxed copy, a projection's fresh copy, or a
/// call result lives here until a consumer takes it, and the end of the
/// instruction releases what is left. The set is empty at every
/// instruction boundary. Its entries are ordered by value, so releases come
/// out in the same order in every build.
#[derive(Clone, Default)]
struct LayoutScratch {
    opaque: BTreeSet<Val>,
    unions: BTreeMap<Val, (Vec<Val>, DirectUnionType)>,
    /// Inline callables the statement owns, keyed by the descriptor word.
    callables: BTreeMap<Val, Vec<Val>>,
    /// The references MIR `Retain`s made for the instruction that follows,
    /// in order, each with its place. A consuming operand of that place
    /// takes the first one; the end of the instruction releases any left.
    /// A branch of the instruction restores them with the rest.
    retained: Vec<(String, ValueRef)>,
    /// The consuming `Place` operands of the instruction being compiled,
    /// by address, which take the retained references.
    consumers: Vec<*const Operand>,
}

impl LayoutScratch {
    fn contains(&self, value: &Val) -> bool {
        self.opaque.contains(value)
    }

    fn insert(&mut self, value: Val) -> bool {
        self.opaque.insert(value)
    }

    fn remove(&mut self, value: &Val) -> bool {
        self.opaque.remove(value)
    }

    fn clear(&mut self) {
        self.opaque.clear();
        self.unions.clear();
        self.callables.clear();
        self.retained.clear();
        self.consumers.clear();
    }

    fn is_empty(&self) -> bool {
        self.opaque.is_empty() && self.unions.is_empty() && self.callables.is_empty()
    }
}

#[derive(Clone, Copy)]
struct TaskStartMode {
    returns_handle: bool,
    result_is_copy: bool,
}

struct TaskStart<'a> {
    mode: TaskStartMode,
    stack_size: Option<&'a Operand>,
    task_group: &'a Operand,
    function: &'a Operand,
    args: &'a [MirArg],
    spawn_span: Span,
    target: &'a DirectType,
}

struct BoundFunctionValueArgs<'a> {
    slots: Vec<Option<&'a MirArg>>,
    source_slots: Vec<usize>,
}

fn bind_function_value_args<'a>(
    params: &[FunctionParamContract],
    args: &'a [MirArg],
    unknown_named_prefix: &str,
    duplicate_error: &str,
) -> std::result::Result<BoundFunctionValueArgs<'a>, String> {
    let mut slots = vec![None; params.len()];
    let mut source_slots = Vec::with_capacity(args.len());
    let mut next_positional = 0usize;
    for argument in args {
        let slot = if let Some(name) = argument.name.as_deref() {
            let mut named_slot = None;
            for (index, param) in params.iter().enumerate() {
                if param.name == name {
                    named_slot = Some(index);
                    break;
                }
            }
            let Some(named_slot) = named_slot else {
                return Err(format!("{unknown_named_prefix} `{name}`"));
            };
            named_slot
        } else {
            while next_positional < slots.len() && slots[next_positional].is_some() {
                next_positional += 1;
            }
            let slot = next_positional;
            next_positional += 1;
            slot
        };
        if slot >= slots.len() || slots[slot].replace(argument).is_some() {
            return Err(duplicate_error.to_string());
        }
        source_slots.push(slot);
    }
    Ok(BoundFunctionValueArgs {
        slots,
        source_slots,
    })
}

fn function_value_param_types(
    params: &[FunctionParamContract],
    classes: &HashMap<String, MirClass>,
    context: &str,
) -> std::result::Result<Vec<DirectType>, String> {
    let mut direct_types = Vec::with_capacity(params.len());
    for (index, param) in params.iter().enumerate() {
        direct_types.push(ensure_direct_type(
            &param.ty,
            classes,
            &format!("{context} {}", index + 1),
        )?);
    }
    Ok(direct_types)
}

struct NativeCodegen<'a> {
    module: &'a MirModule,
    /// The locals each function states ownership for, by function name.
    explicit_locals: HashMap<String, crate::mir::ownership::ExplicitLocals>,
    reachable_blocks: HashMap<String, HashSet<String>>,
    safepoints_enabled: bool,
    program_path: String,
    program_source: String,
    /// The module backend. A function definition holds it while the
    /// function's body is built, and the body reaches it through its
    /// function backend.
    object: Option<Box<dyn ModuleBackend>>,
    functions: HashMap<String, FuncHandle>,
    function_thunks: HashMap<String, FuncHandle>,
    function_default_binders: HashMap<String, FuncHandle>,
    callable_shapes: HashMap<CallableShapeKey, CallableShape>,
    contract_descriptors: HashMap<String, callables::ContractDescriptor>,
    cleanup_thunks: HashMap<(String, String), FuncHandle>,
    classes: HashMap<String, MirClass>,
    enums: HashMap<String, MirEnum>,
    union_layouts: HashMap<String, crate::union_layout::MirUnionLayout>,
    /// How each method function takes its receiver, by function name.
    method_receivers: HashMap<String, Option<MirReceiverKind>>,
    function_return_types: HashMap<String, DirectType>,
    function_param_types: HashMap<String, Vec<DirectType>>,
    function_writeback_types: HashMap<String, Vec<DirectType>>,
    runtime: RuntimeTable<FuncHandle>,
    string_data: HashMap<Vec<u8>, DataHandle>,
    /// One writable word per runtime type descriptor, where the runtime
    /// keeps the decoded tag after the first use.
    tag_cells: HashMap<String, DataHandle>,
    /// Every function's frame descriptor, which the main wrapper validates.
    frame_descriptors: Vec<DataHandle>,
    /// The module's own retain and release, which `runtime` names in place
    /// of the runtime's.
    ownership: OwnershipFastPaths,
}

/// The retain and release every generated call goes through. Each does the
/// common case in place and calls the runtime's export for the rest.
#[derive(Clone, Copy)]
struct OwnershipFastPaths {
    retain: FuncHandle,
    release: FuncHandle,
    runtime_retain: FuncHandle,
    runtime_release: FuncHandle,
    release_last: FuncHandle,
}

macro_rules! try_or_string_error {
    ($expr:expr, $($fmt:tt)+) => {
        match $expr {
            Ok(value) => value,
            Err(error) => return Err(format!($($fmt)+, error)),
        }
    };
}

fn split_field_path_segments<'a>(
    segments: &'a [&'a str],
) -> std::result::Result<(&'a str, &'a [&'a str]), String> {
    match segments.split_first() {
        Some((head, rest)) => Ok((*head, rest)),
        None => Err("internal error: direct backend received an empty field path".to_string()),
    }
}

/// The arguments of a member call this emitter resolves to `op`, such as
/// one on a member of a union, bound as MIR binds a resolved op's.
fn bind_member_call_args(
    op: RuntimeOp,
    args: &[MirArg],
) -> std::result::Result<std::borrow::Cow<'_, [MirArg]>, String> {
    match crate::mir::bind_runtime_op_args(op, args) {
        Ok(None) => Ok(std::borrow::Cow::Borrowed(args)),
        Ok(Some(bound)) => Ok(std::borrow::Cow::Owned(bound)),
        Err(detail) => Err(format!("direct backend {detail}")),
    }
}

/// Refuses a resolved op's call whose arguments are not in the bound form
/// MIR gives them, as when a caller skips the module validator.
fn require_bound_args(op: RuntimeOp, args: &[MirArg]) -> std::result::Result<(), String> {
    match bind_member_call_args(op, args)? {
        std::borrow::Cow::Borrowed(_) => Ok(()),
        std::borrow::Cow::Owned(_) => Err(format!(
            "direct backend expected the arguments of `{op}` in parameter order"
        )),
    }
}

fn direct_internal_slice_args(args: &[MirArg]) -> std::result::Result<&[MirArg; 6], String> {
    args.try_into()
        .map_err(|_| DIRECT_INTERNAL_SLICE_ARITY_ERROR.to_string())
}

/// The word kinds of a slice's arguments: the start, whether it is
/// present, the end, whether it is present, and the source line and column.
const SLICE_WORD_KINDS: &[ScalarKind] = &[
    ScalarKind::Int64,
    ScalarKind::Bool,
    ScalarKind::Int64,
    ScalarKind::Bool,
    ScalarKind::Int32,
    ScalarKind::Int32,
];

fn required_direct_field_slice(
    ty: &DirectType,
    field: &str,
) -> std::result::Result<(usize, usize, DirectType), String> {
    ty.field_slice(field).ok_or(format!(
        "direct backend does not know field `{}` on `{}`",
        field,
        render_direct_type(ty)
    ))
}

impl<'a> NativeCodegen<'a> {
    #[cfg(test)]
    fn new(
        module: &'a MirModule,
        program_path: &str,
        program_source: &str,
    ) -> std::result::Result<Self, String> {
        crate::mir::validate_loan_flow(module)?;
        Self::with_validated_loans(module, program_path, program_source)
    }

    /// Prepares to emit `module`, whose loan flow the caller validated.
    fn with_validated_loans(
        module: &'a MirModule,
        program_path: &str,
        program_source: &str,
    ) -> std::result::Result<Self, String> {
        let reachable_blocks = validate_module(module)?;
        // A program with no task-start operation cannot have a runnable sibling
        // for a loop to starve. Keep explicit MIR markers for portability, but
        // elide their native fast-path cost for provably sequential programs.
        let safepoints_enabled = !collect_task_start_targets(module, &reachable_blocks).is_empty();
        let mut classes = HashMap::new();
        for class in &module.classes {
            classes.insert(class.name.clone(), class.clone());
        }
        let method_receivers = module
            .classes
            .iter()
            .flat_map(|class| &class.methods)
            .chain(
                module
                    .trait_impls
                    .iter()
                    .flat_map(|trait_impl| &trait_impl.methods),
            )
            .map(|method| (method.function_name.clone(), method.receiver))
            .collect();

        let mut object = crate::native_backend::host_module_backend()?;

        let mut runtime =
            RuntimeTable::try_build(|function| declare_runtime_helper(&mut *object, function))?;
        let ownership = declare_ownership_fast_paths(&mut *object, &mut runtime)?;

        let mut functions = HashMap::new();
        let mut function_thunks = HashMap::new();
        let mut function_default_binders = HashMap::new();
        let mut cleanup_thunks = HashMap::new();
        let mut function_return_types = HashMap::new();
        let mut function_param_types = HashMap::new();
        let mut function_writeback_types = HashMap::new();
        for function in module.functions.iter().chain(module.top_level.iter()) {
            let function_reachable = reachable_blocks.get(&function.name).ok_or_else(|| {
                format!(
                    "direct backend is missing reachable-block metadata for `{}`",
                    function.name
                )
            })?;
            let signature = signature_for(function, &classes)?;
            let func_id = try_or_string_error!(
                object.declare_function(&mangle_symbol(&function.name), Linkage::Local, &signature),
                "failed to declare function `{}`: {}",
                function.name
            );
            functions.insert(function.name.clone(), func_id);
            let thunk_signature = thunk_signature();
            let thunk_id = try_or_string_error!(
                object.declare_function(
                    &mangle_thunk_symbol(&function.name),
                    Linkage::Local,
                    &thunk_signature,
                ),
                "failed to declare function thunk `{}`: {}",
                function.name
            );
            function_thunks.insert(function.name.clone(), thunk_id);
            let binder_signature = default_binder_signature();
            let binder_id = try_or_string_error!(
                object.declare_function(
                    &mangle_default_binder_symbol(&function.name),
                    Linkage::Local,
                    &binder_signature,
                ),
                "failed to declare function default binder `{}`: {}",
                function.name
            );
            function_default_binders.insert(function.name.clone(), binder_id);
            for (cleanup_index, place) in collect_cleanup_places(function, function_reachable)
                .into_iter()
                .enumerate()
            {
                let cleanup_id = try_or_string_error!(
                    object.declare_function(
                        &mangle_cleanup_thunk_symbol(&function.name, &place, cleanup_index),
                        Linkage::Local,
                        &thunk_signature,
                    ),
                    "failed to declare cleanup thunk for `{}` in `{}`: {}",
                    place,
                    function.name
                );
                cleanup_thunks.insert((function.name.clone(), place), cleanup_id);
            }
            function_return_types.insert(
                function.name.clone(),
                ensure_direct_type(
                    &function.return_type,
                    &classes,
                    &format!("return type of `{}`", function.name),
                )?,
            );
            let mut params = Vec::new();
            let mut writebacks = Vec::new();
            if function.receiver == Some(MirReceiverKind::BorrowMut) {
                writebacks.push(receiver_type(function, &classes)?);
            }
            if function.receiver.is_some() {
                params.push(receiver_type(function, &classes)?);
            }
            for param in &function.params {
                if param.passing == MirReceiverKind::BorrowMut {
                    writebacks.push(ensure_direct_type(
                        &param.ty,
                        &classes,
                        &format!("parameter `{}` on `{}`", param.name, function.name),
                    )?);
                }
                params.push(ensure_direct_type(
                    &param.ty,
                    &classes,
                    &format!("parameter `{}` on `{}`", param.name, function.name),
                )?);
            }
            function_param_types.insert(function.name.clone(), params);
            function_writeback_types.insert(function.name.clone(), writebacks);
        }

        let callable_shapes = callables::declare_callable_shapes(
            module,
            &mut *object,
            &function_param_types,
            &classes,
        )?;
        Ok(Self {
            module,
            explicit_locals: crate::mir::ownership::explicit_locals(module),
            reachable_blocks,
            safepoints_enabled,
            program_path: program_path.to_string(),
            program_source: program_source.to_string(),
            object: Some(object),
            functions,
            function_thunks,
            function_default_binders,
            callable_shapes,
            contract_descriptors: HashMap::new(),
            cleanup_thunks,
            classes,
            enums: module
                .enums
                .iter()
                .map(|layout| (layout.name.clone(), layout.clone()))
                .collect(),
            union_layouts: module
                .unions
                .iter()
                .map(|plan| {
                    (
                        crate::union_layout::union_plan_key(&plan.union_type),
                        plan.clone(),
                    )
                })
                .collect(),
            method_receivers,
            function_return_types,
            function_param_types,
            function_writeback_types,
            runtime,
            string_data: HashMap::new(),
            tag_cells: HashMap::new(),
            frame_descriptors: Vec::new(),
            ownership,
        })
    }

    fn emit(mut self) -> std::result::Result<Vec<u8>, String> {
        for function in self
            .module
            .functions
            .iter()
            .chain(self.module.top_level.iter())
        {
            self.define_function(function)?;
            if function.receiver.is_none() {
                self.define_function_thunk(function)?;
                self.define_function_default_binder(function)?;
            }
        }
        self.define_callable_shapes()?;
        self.define_contract_descriptors()?;
        for function in self
            .module
            .functions
            .iter()
            .chain(self.module.top_level.iter())
        {
            self.define_cleanup_thunks(function)?;
        }
        self.define_ownership_fast_paths()?;
        self.define_main_wrapper()?;
        match self.take_module_backend()?.finish() {
            Ok(bytes) => Ok(bytes),
            Err(error) => Err(format!("failed to emit direct backend object: {}", error)),
        }
    }

    /// Defines the module's retain and release.
    fn define_ownership_fast_paths(&mut self) -> std::result::Result<(), String> {
        let paths = self.ownership;
        let frame_stack = self.runtime[RuntimeFn::FrameStack];
        self.define_body(
            paths.retain,
            &ownership_fast_path_signature(true),
            "the retain fast path",
            |_, builder| {
                emit_retain_fast_path(builder, frame_stack, paths.runtime_retain);
                Ok(())
            },
        )?;
        self.define_body(
            paths.release,
            &ownership_fast_path_signature(false),
            "the release fast path",
            |_, builder| {
                emit_release_fast_path(builder, frame_stack, paths);
                Ok(())
            },
        )
    }

    /// The module backend, out of `self`.
    fn take_module_backend(&mut self) -> std::result::Result<Box<dyn ModuleBackend>, String> {
        match self.object.take() {
            Some(object) => Ok(object),
            None => Err(MODULE_BACKEND_BUSY_ERROR.to_string()),
        }
    }

    /// The module backend, between function definitions.
    fn module_backend(&mut self) -> std::result::Result<&mut dyn ModuleBackend, String> {
        match self.object.as_deref_mut() {
            Some(object) => Ok(object),
            None => Err(MODULE_BACKEND_BUSY_ERROR.to_string()),
        }
    }

    /// Defines `id` with the body `emit` builds. The module backend is out of
    /// `self` while `emit` runs, which reaches it through the function
    /// backend. `what` names the function in a code generator error.
    fn define_body(
        &mut self,
        id: FuncHandle,
        signature: &Signature,
        what: &str,
        emit: impl FnOnce(&mut Self, &mut dyn FunctionBackend) -> std::result::Result<(), String>,
    ) -> std::result::Result<(), String> {
        let mut object = self.take_module_backend()?;
        let defined =
            object.define_function(id, signature, Box::new(|builder| emit(self, builder)));
        self.object = Some(object);
        match defined {
            Ok(()) => Ok(()),
            Err(DefineError::Body(error)) => Err(error),
            Err(DefineError::Backend(error)) => Err(format!("failed to define {what}: {error}")),
        }
    }

    fn define_function(&mut self, function: &MirFunction) -> std::result::Result<(), String> {
        let func_id = self.functions[&function.name];
        let signature = signature_for(function, &self.classes)?;
        self.define_body(
            func_id,
            &signature,
            &format!("direct function `{}`", function.name),
            |codegen, builder| codegen.emit_function(builder, function, func_id),
        )
    }

    fn emit_function(
        &mut self,
        builder: &mut dyn FunctionBackend,
        function: &MirFunction,
        func_id: FuncHandle,
    ) -> std::result::Result<(), String> {
        let reachable_blocks = self
            .reachable_blocks
            .get(&function.name)
            .cloned()
            .ok_or_else(|| {
                format!(
                    "direct backend is missing reachable-block metadata for `{}`",
                    function.name
                )
            })?;
        let mut blocks = HashMap::new();
        for block in &function.blocks {
            if !reachable_blocks.contains(&block.label) {
                continue;
            }
            blocks.insert(block.label.clone(), builder.create_block());
        }

        let entry = match blocks.get(&function.entry) {
            Some(entry) => *entry,
            None => {
                return Err(format!(
                    "direct backend could not find entry block `{}`",
                    function.entry
                ));
            }
        };
        builder.append_block_params_for_function_params(entry);
        builder.switch_to_block(entry);
        let runtime_refs = self.runtime.map(|id| builder.import_function(*id));
        let public_function_name = public_direct_function_name(&function.name);
        let function_path = function
            .source_path
            .as_deref()
            .unwrap_or(&self.program_path);
        let descriptor = declare_frame_descriptor(
            builder.module(),
            &mut self.string_data,
            &format!("aura_frame_{}", func_id.0),
            function.span,
            function_path.as_bytes(),
            public_function_name.as_bytes(),
        )?;
        self.frame_descriptors.push(descriptor);
        let descriptor = builder.import_data(descriptor);
        let descriptor = builder.symbol_value(types::I64, descriptor);
        // The entry block's code continues after the push, in the block
        // where both of its paths meet.
        let body = emit_frame_push(builder, &runtime_refs, descriptor);
        blocks.insert(function.entry.clone(), body);

        let mut variable_index = 0usize;
        let mut variables = HashMap::new();
        let mut variable_types = HashMap::new();
        let entry_values = builder.block_params(entry);
        let mut entry_index = 0usize;

        if function.receiver.is_some() {
            let receiver_ty = receiver_type(function, &self.classes)?;
            let end = entry_index + receiver_ty.value_count();
            declare_root_variables(
                builder,
                &mut variable_index,
                &mut variables,
                &mut variable_types,
                "self".to_string(),
                receiver_ty,
                Some(&entry_values[entry_index..end]),
            );
            entry_index = end;
        }

        for param in &function.params {
            let ty = ensure_direct_type(
                &param.ty,
                &self.classes,
                &format!("parameter `{}` on `{}`", param.name, function.name),
            )?;
            let end = entry_index + ty.value_count();
            declare_root_variables(
                builder,
                &mut variable_index,
                &mut variables,
                &mut variable_types,
                param.name.clone(),
                ty,
                Some(&entry_values[entry_index..end]),
            );
            entry_index = end;
        }

        for local in &function.local_types {
            if variables.contains_key(&local.name) {
                continue;
            }
            let ty = ensure_direct_type(
                &local.ty,
                &self.classes,
                &format!("local `{}` on `{}`", local.name, function.name),
            )?;
            declare_root_variables(
                builder,
                &mut variable_index,
                &mut variables,
                &mut variable_types,
                local.name.clone(),
                ty,
                None,
            );
        }

        // MIR records the type of every assigned local (Batch 7 step 8c), so
        // each whole-local assignment target is already declared above.
        for block in &function.blocks {
            if !reachable_blocks.contains(&block.label) {
                continue;
            }
            for instruction in &block.instructions {
                let Instruction::Assign { target, .. } = instruction else {
                    continue;
                };
                if !target.contains('.') && !variables.contains_key(target) {
                    return Err(format!(
                        "direct backend found no MIR type for local `{}` in `{}`",
                        target, function.name
                    ));
                }
            }
        }

        let mut cleanup_places = Vec::<String>::new();
        let mut cleanup_actions = HashMap::<String, CleanupAction>::new();
        let mut cleanup_callees = HashMap::<String, CleanupCallee>::new();
        for block in &function.blocks {
            if !reachable_blocks.contains(&block.label) {
                continue;
            }
            for instruction in &block.instructions {
                let Instruction::PushCleanup {
                    place,
                    action,
                    callee,
                } = instruction
                else {
                    continue;
                };
                if !cleanup_places.contains(place) {
                    cleanup_places.push(place.clone());
                }
                cleanup_actions.insert(place.clone(), action.clone());
                if let Some(callee) = callee {
                    cleanup_callees.insert(place.clone(), callee.clone());
                }
            }
        }
        let mut cleanup_active_vars = HashMap::new();
        let mut cleanup_registration_vars = HashMap::new();
        for place in &cleanup_places {
            let variable = Var(variable_index as u32);
            variable_index += 1;
            builder.declare_var(variable, types::I64);
            let zero = builder.iconst(types::I64, 0);
            builder.def_var(variable, zero);
            cleanup_active_vars.insert(place.clone(), variable);

            let registration_variable = Var(variable_index as u32);
            variable_index += 1;
            builder.declare_var(registration_variable, types::I64);
            let zero = builder.iconst(types::I64, 0);
            builder.def_var(registration_variable, zero);
            cleanup_registration_vars.insert(place.clone(), registration_variable);
        }

        let safepoint_fuel = if self.safepoints_enabled
            && function.blocks.iter().any(|block| {
                reachable_blocks.contains(&block.label)
                    && block
                        .instructions
                        .iter()
                        .any(|instruction| matches!(instruction, Instruction::Safepoint))
            }) {
            let variable = Var(variable_index as u32);
            variable_index += 1;
            builder.declare_var(variable, types::I64);
            let initial = builder.iconst(types::I64, NATIVE_LOOP_SAFEPOINT_INTERVAL as i64);
            builder.def_var(variable, initial);
            Some(variable)
        } else {
            None
        };

        let view_selector_tags = direct_view_selector_tags(function, &reachable_blocks);
        let mut view_selector_vars = HashMap::new();
        let mut selector_loans = view_selector_tags.keys().cloned().collect::<Vec<_>>();
        selector_loans.sort();
        for loan in selector_loans {
            let variable = Var(variable_index as u32);
            variable_index += 1;
            builder.declare_var(variable, types::I64);
            let zero = builder.iconst(types::I64, 0);
            builder.def_var(variable, zero);
            view_selector_vars.insert(loan, variable);
        }

        let mut closure_selector_tags = HashMap::new();
        let mut closure_selector_counts = HashMap::<String, i64>::new();
        for block in &function.blocks {
            if !reachable_blocks.contains(&block.label) {
                continue;
            }
            for (instruction_index, instruction) in block.instructions.iter().enumerate() {
                let Instruction::Assign {
                    target,
                    value: Rvalue::Closure { captures, .. },
                } = instruction
                else {
                    continue;
                };
                if !captures
                    .iter()
                    .any(|capture| capture.passing == MirReceiverKind::BorrowMut)
                {
                    continue;
                }
                let root = target.split('.').next().unwrap_or(target).to_string();
                let next = closure_selector_counts.entry(root).or_default();
                closure_selector_tags.insert((block.label.clone(), instruction_index), *next);
                *next = next.checked_add(1).ok_or_else(|| {
                    format!(
                        "direct backend closure selector count overflows in `{}`",
                        function.name
                    )
                })?;
            }
        }
        let mut closure_selector_vars = HashMap::new();
        let mut closure_roots = closure_selector_counts.into_keys().collect::<Vec<_>>();
        closure_roots.sort();
        for root in closure_roots {
            let variable = Var(variable_index as u32);
            variable_index += 1;
            builder.declare_var(variable, types::I64);
            let unset = builder.iconst(types::I64, -1);
            builder.def_var(variable, unset);
            closure_selector_vars.insert(root, variable);
        }

        let mut writeback_locals = Vec::new();
        let mut mutable_param_indices = HashMap::new();
        let mut call_slot = 0usize;
        if function.receiver.is_some() {
            if function.receiver == Some(MirReceiverKind::BorrowMut) {
                mutable_param_indices.insert("self".to_string(), call_slot);
            }
            call_slot += 1;
        }
        for param in &function.params {
            if param.passing == MirReceiverKind::BorrowMut {
                mutable_param_indices.insert(param.name.clone(), call_slot);
            }
            call_slot += 1;
        }
        if function.receiver == Some(MirReceiverKind::BorrowMut) {
            let receiver_ty = receiver_type(function, &self.classes)?;
            writeback_locals.push(("self".to_string(), receiver_ty));
        }
        for param in &function.params {
            if param.passing == MirReceiverKind::BorrowMut {
                let ty = ensure_direct_type(
                    &param.ty,
                    &self.classes,
                    &format!("parameter `{}` on `{}`", param.name, function.name),
                )?;
                writeback_locals.push((param.name.clone(), ty));
            }
        }

        let mut function_refs = HashMap::new();
        // Sorted, so each function declares its callees in the same order in
        // every build.
        let mut functions = self.functions.iter().collect::<Vec<_>>();
        functions.sort_by(|left, right| left.0.cmp(right.0));
        for (name, func_id) in functions {
            let func_ref = builder.import_function(*func_id);
            function_refs.insert(name.clone(), func_ref);
        }
        let mut function_thunk_refs = HashMap::new();
        let mut function_thunks = self.function_thunks.iter().collect::<Vec<_>>();
        function_thunks.sort_by(|left, right| left.0.cmp(right.0));
        for (name, func_id) in function_thunks {
            let func_ref = builder.import_function(*func_id);
            function_thunk_refs.insert(name.clone(), func_ref);
        }
        let mut cleanup_thunk_refs = HashMap::new();
        for place in &cleanup_places {
            if let Some(func_id) = self
                .cleanup_thunks
                .get(&(function.name.clone(), place.clone()))
            {
                let func_ref = builder.import_function(*func_id);
                cleanup_thunk_refs.insert(place.clone(), func_ref);
            }
        }

        let explicit_locals = self
            .explicit_locals
            .get(&function.name)
            .cloned()
            .unwrap_or_default();
        let current_function_name = public_direct_function_name(&function.name);
        let current_function_path = function
            .source_path
            .clone()
            .unwrap_or_else(|| self.program_path.clone());

        let lookup_slots = lookups::lookup_payload_slots(function)
            .into_iter()
            .filter(|name| {
                matches!(
                    variable_types.get(name),
                    Some(DirectType::Opaque(Type::Named(lookup, _))) if lookup == "Lookup"
                )
            })
            .collect::<HashSet<_>>();
        let lent_places = lookups::lent_lookup_places(
            function,
            &lookup_slots,
            // The MIR retains and releases every place that holds the value,
            // so each of its references is counted where the MIR says.
            |place| {
                explicit_locals.tracked.contains(place)
                    && matches!(variable_types.get(place), Some(DirectType::Opaque(_)))
            },
            |place| {
                matches!(
                    variable_types.get(place),
                    Some(DirectType::Scalar(
                        ScalarKind::Int32
                            | ScalarKind::Int64
                            | ScalarKind::Uint64
                            | ScalarKind::Bool
                    ))
                )
            },
        );
        let mut compiler = FunctionCompiler {
            builder,
            blocks,
            variables,
            variable_types,
            next_variable_index: variable_index,
            function_refs,
            function_thunk_refs,
            cleanup_thunk_refs,
            function_return_types: self.function_return_types.clone(),
            function_param_types: self.function_param_types.clone(),
            function_writeback_types: self.function_writeback_types.clone(),
            current_function_name,
            current_function_path,
            writeback_locals,
            mutable_param_indices,
            classes: self.classes.clone(),
            enums: self.enums.clone(),
            union_layouts: self.union_layouts.clone(),
            method_receivers: &self.method_receivers,
            return_type: function.return_type.clone(),
            layout_scratch: LayoutScratch::default(),
            tracked_roots: explicit_locals.tracked,
            loan_locals: explicit_locals.loans,
            closed_matches: explicit_locals.closed_matches,
            lookup_slots,
            lent_places,
            lend_lookup: false,
            lookup_payload: None,
            emptied_store_root: None,
            lent_temps: HashMap::new(),
            retain_calls: HashMap::new(),
            retain_releases: HashMap::new(),
            consumed_retains: HashSet::new(),
            callable_retains: HashMap::new(),
            retained_unions: HashSet::new(),
            last_runtime_type_tag: None,
            view_places: HashMap::new(),
            view_element_keys: HashMap::new(),
            view_selector_vars,
            view_selector_tags,
            closure_selector_vars,
            closure_selector_tags,
            closure_capture_writebacks: HashMap::new(),
            host_builtin_refs: HashMap::new(),
            string_data: &mut self.string_data,
            tag_cells: &mut self.tag_cells,
            callable_shapes: &self.callable_shapes,
            contract_descriptors: &mut self.contract_descriptors,
            cleanup_places,
            cleanup_actions,
            cleanup_callees,
            cleanup_active_vars,
            cleanup_registration_vars,
            safepoint_fuel,
            runtime: runtime_refs,
        };

        let return_ty = ensure_direct_type(
            &function.return_type,
            &self.classes,
            &format!("return type of `{}`", function.name),
        )?;
        compiler.compile_reachable_blocks(function, &return_ty)?;

        compiler.builder.seal_all_blocks();
        try_or_string_error!(
            compiler.builder.verify(),
            "failed to define direct function `{}`: {}",
            function.name
        );
        Ok(())
    }

    fn define_function_thunk(&mut self, function: &MirFunction) -> std::result::Result<(), String> {
        if function.receiver.is_some() {
            return Err(format!(
                "direct backend does not yet support task-start thunks for methods like `{}`",
                function.name
            ));
        }

        let thunk_id = self.function_thunks[&function.name];
        let target_id = self.functions[&function.name];
        self.define_body(
            thunk_id,
            &thunk_signature(),
            &format!("direct function thunk `{}`", function.name),
            |codegen, builder| codegen.emit_function_thunk(builder, function, target_id),
        )
    }

    fn emit_function_thunk(
        &mut self,
        builder: &mut dyn FunctionBackend,
        function: &MirFunction,
        target_id: FuncHandle,
    ) -> std::result::Result<(), String> {
        let entry = builder.create_block();
        builder.append_block_params_for_function_params(entry);
        builder.switch_to_block(entry);
        builder.seal_block(entry);

        let args_ptr = builder.block_params(entry)[0];
        let target_ref = builder.import_function(target_id);
        let unbox_i64 = builder.import_function(self.runtime[RuntimeFn::UnboxI64]);
        let unbox_int64 = builder.import_function(self.runtime[RuntimeFn::UnboxInt64]);
        let unbox_u64 = builder.import_function(self.runtime[RuntimeFn::UnboxU64]);
        let unbox_f64 = builder.import_function(self.runtime[RuntimeFn::UnboxF64]);
        let unbox_bool = builder.import_function(self.runtime[RuntimeFn::UnboxBool]);
        let release_value = builder.import_function(self.runtime[RuntimeFn::ReleaseValue]);

        if matches!(function.name.as_str(), "main" | "__script") {
            let module_constant = builder.import_function(self.runtime[RuntimeFn::ModuleConstant]);
            for constant in &self.module.constants {
                let initializer =
                    self.function_thunks
                        .get(&constant.initializer)
                        .ok_or_else(|| {
                            format!(
                                "direct backend cannot find module constant initializer `{}`",
                                constant.initializer
                            )
                        })?;
                let initializer = builder.import_function(*initializer);
                let initializer = builder.func_addr(types::I64, initializer);
                let (key_ptr, key_len) = declare_string_constant(
                    &mut self.string_data,
                    builder,
                    constant.key.as_bytes(),
                )?;
                let call = builder.call(module_constant, &[key_ptr, key_len, initializer]);
                let value = call.results[0];
                builder.call(release_value, &[value]);
            }
        }

        let mut lowered_args = Vec::new();
        let param_types = self.function_param_types[&function.name].clone();
        for (index, param_ty) in param_types.iter().enumerate() {
            let raw = builder.load(types::I64, MemFlags::new(), args_ptr, (index as i32) * 8);
            // The uniform function-value buffer is an in/out ownership
            // channel. Clear each incoming slot before transferring its
            // retained handle into the concrete function ABI; mutable
            // parameter writebacks are installed into their original slots
            // after the call.
            let zero = builder.iconst(types::I64, 0);
            builder.store(MemFlags::new(), zero, args_ptr, (index as i32) * 8);
            match param_ty {
                DirectType::Callable(_) => {
                    // The boxed handle is adopted into the callable's words.
                    lowered_args.extend(unbox_thunk_value(self, builder, raw, param_ty)?);
                }
                DirectType::Opaque(_) => lowered_args.push(raw),
                DirectType::Scalar(ScalarKind::Int32) => {
                    let inst = builder.call(unbox_i64, &[raw]);
                    lowered_args.push(inst.results[0]);
                    let _ = builder.call(release_value, &[raw]);
                }
                DirectType::Scalar(ScalarKind::Int64) => {
                    let inst = builder.call(unbox_int64, &[raw]);
                    lowered_args.push(inst.results[0]);
                    let _ = builder.call(release_value, &[raw]);
                }
                DirectType::Scalar(ScalarKind::Uint64) => {
                    let inst = builder.call(unbox_u64, &[raw]);
                    lowered_args.push(inst.results[0]);
                    let _ = builder.call(release_value, &[raw]);
                }
                DirectType::Scalar(ScalarKind::Float32)
                | DirectType::Scalar(ScalarKind::Float64) => {
                    let inst = builder.call(unbox_f64, &[raw]);
                    lowered_args.push(inst.results[0]);
                    let _ = builder.call(release_value, &[raw]);
                }
                DirectType::Scalar(ScalarKind::Bool) => {
                    let inst = builder.call(unbox_bool, &[raw]);
                    lowered_args.push(inst.results[0]);
                    let _ = builder.call(release_value, &[raw]);
                }
                DirectType::Scalar(ScalarKind::Unit) => {
                    lowered_args.push(builder.iconst(types::I64, 0));
                    let _ = builder.call(release_value, &[raw]);
                }
                DirectType::PlainClass(_) | DirectType::Union(_) => {
                    lowered_args.extend(unbox_thunk_value(self, builder, raw, param_ty)?);
                    let _ = builder.call(release_value, &[raw]);
                }
            }
        }

        let inst = builder.call(target_ref, &lowered_args);
        let results = inst.results;
        let return_ty = match self.function_return_types.get(&function.name).cloned() {
            Some(return_ty) => return_ty,
            None => {
                return Err(format!(
                    "direct backend does not know return type for `{}`",
                    function.name
                ));
            }
        };
        // `target_ref` uses `signature_for(function)`: its results are the
        // declared return ABI followed by mutable-parameter writebacks. The
        // cached direct types above are built from that same `MirFunction`.
        let return_count = return_ty.value_count();
        let mut cursor = return_count;
        for (index, param) in function.params.iter().enumerate() {
            if param.passing != MirReceiverKind::BorrowMut {
                continue;
            }
            let writeback_ty = &param_types[index];
            let writeback_count = writeback_ty.value_count();
            let boxed_writeback = callables::box_owned_thunk_value(
                self,
                builder,
                &results[cursor..cursor + writeback_count],
                writeback_ty,
            )?;
            builder.store(
                MemFlags::new(),
                boxed_writeback,
                args_ptr,
                (index as i32) * 8,
            );
            cursor += writeback_count;
        }
        let boxed =
            callables::box_owned_thunk_value(self, builder, &results[..return_count], &return_ty)?;
        builder.return_(&[boxed]);
        Ok(())
    }

    fn define_function_default_binder(
        &mut self,
        function: &MirFunction,
    ) -> std::result::Result<(), String> {
        if function.receiver.is_some() {
            return Err(format!(
                "direct backend cannot build a function-value default binder for method `{}`",
                function.name
            ));
        }
        let binder_id = self.function_default_binders[&function.name];
        self.define_body(
            binder_id,
            &default_binder_signature(),
            &format!("direct default binder `{}`", function.name),
            |codegen, builder| codegen.emit_function_default_binder(builder, function),
        )
    }

    fn emit_function_default_binder(
        &mut self,
        builder: &mut dyn FunctionBackend,
        function: &MirFunction,
    ) -> std::result::Result<(), String> {
        let entry = builder.create_block();
        builder.append_block_params_for_function_params(entry);
        builder.switch_to_block(entry);
        builder.seal_block(entry);
        let args_ptr = builder.block_params(entry)[0];
        let transfer_defaults = builder.block_params(entry)[2];
        let store_owned = builder.import_function(self.runtime[RuntimeFn::ArgBufferStoreOwned]);
        let param_types = self
            .function_param_types
            .get(&function.name)
            .cloned()
            .unwrap_or_default();

        for (index, param) in function.params.iter().enumerate() {
            let Some(default_name) = param.default_function.as_ref() else {
                continue;
            };
            let raw = builder.load(types::I64, MemFlags::new(), args_ptr, (index as i32) * 8);
            let missing = builder.icmp_imm(IntCC::Equal, raw, 0);
            let default_block = builder.create_block();
            let next_block = builder.create_block();
            builder.brif(missing, default_block, &[], next_block, &[]);
            builder.switch_to_block(default_block);
            let default_id = *self.functions.get(default_name).ok_or_else(|| {
                format!(
                    "direct backend is missing default function `{default_name}` for `{}`",
                    function.name
                )
            })?;
            let default_ref = builder.import_function(default_id);
            let call = builder.call(default_ref, &[]);
            let results = call.results;
            let param_ty = param_types.get(index).ok_or_else(|| {
                format!(
                    "direct backend is missing parameter {} metadata for `{}`",
                    index + 1,
                    function.name
                )
            })?;
            let boxed = box_thunk_value(self, builder, &results, param_ty)?;
            let transfer = builder.icmp_imm(IntCC::NotEqual, transfer_defaults, 0);
            let transfer_block = builder.create_block();
            let retain_block = builder.create_block();
            builder.brif(transfer, transfer_block, &[], retain_block, &[]);
            builder.switch_to_block(transfer_block);
            let index_value = builder.iconst(types::I64, index as i64);
            builder.call(store_owned, &[args_ptr, index_value, boxed]);
            builder.jump(next_block, &[]);
            builder.seal_block(transfer_block);
            builder.switch_to_block(retain_block);
            builder.store(MemFlags::new(), boxed, args_ptr, (index as i32) * 8);
            builder.jump(next_block, &[]);
            builder.seal_block(retain_block);
            builder.seal_block(default_block);
            builder.switch_to_block(next_block);
            builder.seal_block(next_block);
        }
        builder.return_(&[]);
        Ok(())
    }

    fn define_cleanup_thunks(&mut self, function: &MirFunction) -> std::result::Result<(), String> {
        let reachable = self
            .reachable_blocks
            .get(&function.name)
            .cloned()
            .ok_or_else(|| {
                format!(
                    "direct backend is missing reachable-block metadata for `{}`",
                    function.name
                )
            })?;
        for place in collect_cleanup_places(function, &reachable) {
            self.define_cleanup_thunk(function, &place)?;
        }
        Ok(())
    }

    fn define_cleanup_thunk(
        &mut self,
        function: &MirFunction,
        place: &str,
    ) -> std::result::Result<(), String> {
        let thunk_id = *self
            .cleanup_thunks
            .get(&(function.name.clone(), place.to_string()))
            .ok_or({
                format!(
                    "direct backend could not find cleanup thunk for `{}` in `{}`",
                    place, function.name
                )
            })?;

        self.define_body(
            thunk_id,
            &thunk_signature(),
            &format!("cleanup thunk for `{}` in `{}`", place, function.name),
            |codegen, builder| codegen.emit_cleanup_thunk(builder, function, place),
        )
    }

    fn emit_cleanup_thunk(
        &mut self,
        builder: &mut dyn FunctionBackend,
        function: &MirFunction,
        place: &str,
    ) -> std::result::Result<(), String> {
        let place_ty = cleanup_place_type(function, &self.classes, place)?;

        let action = cleanup_action_for_place(function, place);
        // The class method the cleanup calls, if it calls one.
        let exit_function = match cleanup_callee_for_place(function, place) {
            Some(CleanupCallee::Method { function }) => Some(function),
            _ => None,
        };

        let entry = builder.create_block();
        builder.append_block_params_for_function_params(entry);
        builder.switch_to_block(entry);
        builder.seal_block(entry);

        let args_ptr = builder.block_params(entry)[0];
        let raw = builder.load(types::I64, MemFlags::new(), args_ptr, 0);
        match &place_ty {
            DirectType::Callable(_) => {}
            DirectType::Union(_) => {}
            DirectType::PlainClass(class_ty) => {
                if let Some(exit_function) = &exit_function {
                    let target_id = *self.functions.get(exit_function).ok_or({
                        format!(
                            "direct backend could not find cleanup close method `{exit_function}`"
                        )
                    })?;
                    let target_ref = builder.import_function(target_id);
                    let lowered = unbox_thunk_value(self, builder, raw, &place_ty)?;
                    let inst = builder.call(target_ref, &lowered);
                    let results = inst.results;
                    release_cleanup_thunk_results(
                        self,
                        builder,
                        action.as_ref(),
                        &class_ty.class_name,
                        exit_function,
                        &results,
                    )?;
                }
                let zero = builder.iconst(types::I64, 0);
                let unit = box_thunk_value(
                    self,
                    builder,
                    &[zero],
                    &DirectType::Scalar(ScalarKind::Unit),
                )?;
                builder.return_(&[unit]);
            }
            DirectType::Opaque(ty) => {
                if let Some(exit_function) = &exit_function {
                    let target_id = *self.functions.get(exit_function).ok_or({
                        format!(
                            "direct backend could not find cleanup close method `{exit_function}`"
                        )
                    })?;
                    let target_ref = builder.import_function(target_id);
                    let retain_value =
                        builder.import_function(self.runtime[RuntimeFn::RetainValue]);
                    let retained = builder.call(retain_value, &[raw]);
                    let retained = retained.results[0];
                    let inst = builder.call(target_ref, &[retained]);
                    let results = inst.results;
                    let class_name = match ty {
                        Type::Named(class_name, _) => class_name.clone(),
                        _ => String::new(),
                    };
                    release_cleanup_thunk_results(
                        self,
                        builder,
                        action.as_ref(),
                        &class_name,
                        exit_function,
                        &results,
                    )?;
                    let zero = builder.iconst(types::I64, 0);
                    let unit = box_thunk_value(
                        self,
                        builder,
                        &[zero],
                        &DirectType::Scalar(ScalarKind::Unit),
                    )?;
                    builder.return_(&[unit]);
                } else {
                    // A resource whose static type is known closes through its
                    // own entry, so the program links only that close path.
                    let typed_close = match ty {
                        Type::Named(name, _) => crate::native_runtime::typed_close_symbol(name),
                        _ => None,
                    };
                    let close_id = match typed_close {
                        Some(symbol) => declare_runtime_function(
                            builder.module(),
                            symbol,
                            &[types::I64, types::I64],
                            Some(types::I64),
                        )?,
                        None => self.runtime[RuntimeFn::CloseValue],
                    };
                    let close_value = builder.import_function(close_id);
                    let cancel_before = builder.iconst(types::I64, 1);
                    let inst = builder.call(close_value, &[raw, cancel_before]);
                    let result = inst.results[0];
                    builder.return_(&[result]);
                }
            }
            DirectType::Scalar(_) => {
                let zero = builder.iconst(types::I64, 0);
                let unit = box_thunk_value(
                    self,
                    builder,
                    &[zero],
                    &DirectType::Scalar(ScalarKind::Unit),
                )?;
                builder.return_(&[unit]);
            }
        }
        Ok(())
    }

    fn define_main_wrapper(&mut self) -> std::result::Result<(), String> {
        let entry_name = if self.functions.contains_key("main") {
            "main".to_string()
        } else if self.functions.contains_key("__script") {
            "__script".to_string()
        } else {
            return Err(
                "direct backend requires a `main` function or top-level script".to_string(),
            );
        };
        let entry_thunk_id = self.function_thunks[&entry_name];

        let signature = main_signature();
        let wrapper_id = try_or_string_error!(
            self.module_backend()?
                .declare_function("main", Linkage::Export, &signature),
            "failed to declare main wrapper: {}"
        );
        self.define_body(
            wrapper_id,
            &signature,
            "main wrapper",
            |codegen, builder| codegen.emit_main_wrapper(builder, entry_thunk_id),
        )
    }

    fn emit_main_wrapper(
        &mut self,
        builder: &mut dyn FunctionBackend,
        entry_thunk_id: FuncHandle,
    ) -> std::result::Result<(), String> {
        let entry_block = builder.create_block();
        builder.switch_to_block(entry_block);
        builder.seal_block(entry_block);

        let runtime_init = builder.import_function(self.runtime[RuntimeFn::RuntimeInit]);
        let run_root = builder.import_function(self.runtime[RuntimeFn::RunRoot]);
        let entry_thunk_ref = builder.import_function(entry_thunk_id);
        let (path_ptr, path_len) =
            declare_string_constant(&mut self.string_data, builder, self.program_path.as_bytes())?;
        let (source_ptr, source_len) = declare_string_constant(
            &mut self.string_data,
            builder,
            self.program_source.as_bytes(),
        )?;
        builder.call(runtime_init, &[path_ptr, path_len, source_ptr, source_len]);
        // Generated code reads list elements at fixed offsets, so a runtime
        // built with another layout stops here instead of misreading them.
        let check_list_layout = builder.import_function(self.runtime[RuntimeFn::CheckListLayout]);
        let fingerprint = builder.iconst(
            types::I64,
            aura_runtime::native_runtime::direct_list_layout().fingerprint(),
        );
        builder.call(check_list_layout, &[fingerprint]);
        // Generated functions push and pop frames at fixed offsets too.
        let check_frame_stack_layout =
            builder.import_function(self.runtime[RuntimeFn::CheckFrameStackLayout]);
        let fingerprint = builder.iconst(types::I64, frame_stack_layout().fingerprint());
        builder.call(check_frame_stack_layout, &[fingerprint]);
        // Retains and releases update reference counts at fixed offsets.
        let check_ownership_layout =
            builder.import_function(self.runtime[RuntimeFn::CheckOwnershipLayout]);
        let fingerprint = builder.iconst(types::I64, ownership_layout().fingerprint());
        builder.call(check_ownership_layout, &[fingerprint]);
        // Validating every descriptor now lets each function push its own
        // without checking it.
        let descriptors =
            declare_frame_descriptor_table(builder.module(), &self.frame_descriptors)?;
        let descriptors = builder.import_data(descriptors);
        let descriptors = builder.symbol_value(types::I64, descriptors);
        let count = builder.iconst(types::I64, self.frame_descriptors.len() as i64);
        let validate = builder.import_function(self.runtime[RuntimeFn::ValidateFrameDescriptors]);
        builder.call(validate, &[descriptors, count]);
        let display_wrappers = self
            .module
            .functions
            .iter()
            .filter_map(|function| {
                crate::mir::display_wrapper_type_name(&function.name)
                    .map(|type_name| (type_name.to_string(), function.name.clone()))
            })
            .collect::<Vec<_>>();
        if !display_wrappers.is_empty() {
            let register_display =
                builder.import_function(self.runtime[RuntimeFn::RegisterDisplay]);
            for (type_name, function_name) in display_wrappers {
                let thunk_id = self.function_thunks[&function_name];
                let thunk_ref = builder.import_function(thunk_id);
                let thunk_addr = builder.func_addr(types::I64, thunk_ref);
                let (name_ptr, name_len) =
                    declare_string_constant(&mut self.string_data, builder, type_name.as_bytes())?;
                builder.call(register_display, &[name_ptr, name_len, thunk_addr]);
            }
        }
        let thunk_ptr = builder.func_addr(types::I64, entry_thunk_ref);
        let result = builder.call(run_root, &[thunk_ptr]);
        let return_code = result.results[0];
        builder.return_(&[return_code]);
        Ok(())
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct DirectViewPlace {
    alternatives: Vec<DirectViewAlternative>,
}

/// A list element a loan lends, read in place by a runtime helper (see
/// `FunctionCompiler::borrowed_list_element`).
struct BorrowedListElement {
    /// The local holding the list.
    list: String,
    /// The variable holding the element's position.
    index: Var,
    element_type: Type,
    /// The index expression a bounds trap names.
    span: Span,
}

/// How the emitter passes one argument of a handle runtime operation.
#[derive(Clone, Copy)]
enum RuntimeOpArgLayout {
    /// One boxed value word.
    Boxed,
    /// One boxed value word, stored back to the argument's writeback place
    /// after the call.
    BoxedWriteback,
    /// One boxed word of the receiver's payload type: a `Queue` put value.
    BoxedPayload,
    /// An `int32`, boxed.
    BoxedInt32,
    /// A scalar `int64` word.
    Int64,
}

fn runtime_op_arg_layout(op: RuntimeOp, name: &str) -> RuntimeOpArgLayout {
    use crate::runtime_ops::{ResourceOp, RngOp};
    match (op, name) {
        (RuntimeOp::Rng(RngOp::NextInt), _) => RuntimeOpArgLayout::Int64,
        (RuntimeOp::Rng(RngOp::Shuffle), _) => RuntimeOpArgLayout::BoxedWriteback,
        (RuntimeOp::Resource(ResourceOp::ProcessSupervisorStart), _) => RuntimeOpArgLayout::Boxed,
        (RuntimeOp::Resource(_), "max_bytes" | "count" | "status") => {
            RuntimeOpArgLayout::BoxedInt32
        }
        (RuntimeOp::Queue(_), "value") => RuntimeOpArgLayout::BoxedPayload,
        _ => RuntimeOpArgLayout::Boxed,
    }
}

/// Whether an op's result is its receiver's payload, which the helper
/// returns boxed whatever its type: `Queue.get_or` and `Task.result_or`.
fn runtime_op_returns_boxed_payload(op: RuntimeOp) -> bool {
    use crate::runtime_ops::{QueueOp, TaskOp};
    matches!(
        op,
        RuntimeOp::Queue(QueueOp::GetOr | QueueOp::GetOrTimeout)
            | RuntimeOp::Task(TaskOp::ResultOr | TaskOp::ResultOrTimeout)
    )
}

/// One lowered argument of a handle runtime operation.
enum RuntimeOpArg {
    Boxed(ValueRef),
    Word(Val),
}

/// What a member call on a receiver runs: a member and the dispatch MIR
/// resolved for it, or a runtime operation lowering resolved.
#[derive(Clone, Copy)]
enum MemberCallee<'a> {
    Method {
        field: &'a str,
        dispatch: &'a MethodDispatch,
    },
    Runtime(RuntimeOp),
}

impl MemberCallee<'_> {
    /// The runtime op the call makes, if it makes one.
    fn op(self) -> Option<RuntimeOp> {
        match self {
            MemberCallee::Runtime(op) => Some(op),
            MemberCallee::Method {
                dispatch: MethodDispatch::Runtime(op),
                ..
            } => Some(*op),
            MemberCallee::Method { .. } => None,
        }
    }
}

/// The dispatch MIR names for a member call `.field`.
fn required_dispatch<'a>(
    dispatch: &'a Option<MethodDispatch>,
    field: &str,
) -> std::result::Result<&'a MethodDispatch, String> {
    dispatch
        .as_ref()
        .ok_or_else(|| format!("direct backend received member call `.{field}` without a dispatch"))
}

/// How the emitter passes one argument of a named function op.
#[derive(Clone, Copy)]
enum FunctionOpArgLayout {
    /// One boxed value word.
    Boxed,
    /// A scalar `int64` word.
    Int64,
}

fn function_op_arg_layout(op: FunctionOp) -> FunctionOpArgLayout {
    match op {
        FunctionOp::RngNew
        | FunctionOp::RandomSecureInt
        | FunctionOp::RandomSecureBytes
        | FunctionOp::DurationMilliseconds
        | FunctionOp::DurationSeconds
        | FunctionOp::DurationMinutes => FunctionOpArgLayout::Int64,
        _ => FunctionOpArgLayout::Boxed,
    }
}

/// How a named function op's helper returns its value.
enum FunctionOpResultLayout {
    /// The helper returns nothing.
    Unit,
    /// One scalar word.
    Scalar(ScalarKind),
    /// A boxed `bool`, unboxed after the call.
    BoxedBool,
    /// An owned box of this type, or of the op's result type.
    Boxed(Option<Type>),
    /// A box laid out as the first argument was.
    FirstArgument,
}

fn function_op_result_layout(op: FunctionOp) -> FunctionOpResultLayout {
    let unknown = || Type::named("Unknown");
    match op {
        FunctionOp::YieldNow | FunctionOp::Sleep => FunctionOpResultLayout::Unit,
        FunctionOp::Cancelled => FunctionOpResultLayout::Scalar(ScalarKind::Bool),
        FunctionOp::RandomSecureInt | FunctionOp::MonotonicTimeMs => {
            FunctionOpResultLayout::Scalar(ScalarKind::Int64)
        }
        FunctionOp::FsExists => FunctionOpResultLayout::BoxedBool,
        FunctionOp::Abs
        | FunctionOp::Min
        | FunctionOp::Max
        | FunctionOp::Sqrt
        | FunctionOp::Round => FunctionOpResultLayout::FirstArgument,
        // A new queue or empty collection has no element type yet; the
        // assignment retypes it.
        FunctionOp::QueueNew => {
            FunctionOpResultLayout::Boxed(Some(Type::Named("Queue".to_string(), vec![unknown()])))
        }
        FunctionOp::ListEmpty => {
            FunctionOpResultLayout::Boxed(Some(Type::Named("list".to_string(), vec![unknown()])))
        }
        FunctionOp::SetEmpty => {
            FunctionOpResultLayout::Boxed(Some(Type::Named("set".to_string(), vec![unknown()])))
        }
        FunctionOp::DictEmpty => FunctionOpResultLayout::Boxed(Some(Type::Named(
            "dict".to_string(),
            vec![unknown(), unknown()],
        ))),
        _ => FunctionOpResultLayout::Boxed(None),
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct DirectViewAlternative {
    place: String,
    conditions: Vec<(Var, i64)>,
    union_payloads: Vec<DirectUnionPayload>,
    /// Element or entry selections applied, in order, after the static
    /// place is loaded: a view of `items[i]` names the list place plus one
    /// selector whose value was evaluated once when the loan began
    /// (ADR-0061, 2026-09-21 section).
    elements: Vec<DirectElementSelector>,
}

/// The runtime operands of one selection chain (see
/// `FunctionCompiler::element_path_operands`).
struct DirectElementPathOperands {
    path_ptr: Val,
    path_len: Val,
    selectors_ptr: Val,
    count: Val,
    line: Val,
    column: Val,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum DirectElementKind {
    List,
    Dict,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct DirectElementSelector {
    variable: Var,
    selector_ty: DirectType,
    /// The type the selection reaches: the element, or its `projection`.
    element_type: Type,
    kind: DirectElementKind,
    /// Fields, tuple positions, and payload projections read or written
    /// inside the selected element, as a dotted path (empty for the
    /// element itself).
    projection: String,
    /// The index expression a bounds or presence trap names.
    span: Span,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct DirectUnionPayload {
    base: String,
    union_type: Type,
    member_index: usize,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct DirectClosureCaptureWriteback {
    index: usize,
    place: DirectViewPlace,
    ty: DirectType,
    /// The capture's first word in the callable's environment.
    env_offset: usize,
    /// Whether the environment lives in the callable's own words.
    inline: bool,
}

impl DirectViewPlace {
    fn static_place(place: String) -> Self {
        Self {
            alternatives: vec![DirectViewAlternative {
                place,
                conditions: Vec::new(),
                union_payloads: Vec::new(),
                elements: Vec::new(),
            }],
        }
    }

    fn project(mut self, projection: &str) -> Self {
        if projection.is_empty() {
            return self;
        }
        for alternative in &mut self.alternatives {
            // A projection of an element view reaches inside the selected
            // element, not past the collection place.
            if let Some(selector) = alternative.elements.last_mut() {
                if selector.projection.is_empty() {
                    selector.projection = projection.to_string();
                } else {
                    selector.projection = format!("{}.{projection}", selector.projection);
                }
            } else {
                alternative.place = format!("{}.{}", alternative.place, projection);
            }
        }
        self
    }

    fn conditioned(mut self, selector: Var, expected: i64) -> Self {
        for alternative in &mut self.alternatives {
            let condition = (selector, expected);
            if !alternative.conditions.contains(&condition) {
                alternative.conditions.push(condition);
            }
        }
        self
    }
}

fn direct_terminator_successors(terminator: &Terminator) -> Vec<&str> {
    match terminator {
        Terminator::Goto(label) => vec![label],
        Terminator::Branch {
            then_label,
            else_label,
            ..
        } => vec![then_label, else_label],
        Terminator::ForRange {
            body_label,
            exit_label,
            ..
        } => vec![body_label, exit_label],
        Terminator::Match {
            arms, otherwise, ..
        } => arms
            .iter()
            .map(|arm| arm.label.as_str())
            .chain(std::iter::once(otherwise.as_str()))
            .collect(),
        Terminator::Return(_) | Terminator::AssertFail { .. } | Terminator::Unreachable => {
            Vec::new()
        }
    }
}

fn reachable_direct_block_labels(
    function: &MirFunction,
) -> std::result::Result<HashSet<String>, String> {
    let mut blocks = HashMap::new();
    for block in &function.blocks {
        if blocks.insert(block.label.as_str(), block).is_some() {
            return Err(format!(
                "direct backend found duplicate MIR block `{}` in `{}`",
                block.label, function.name
            ));
        }
    }
    if !blocks.contains_key(function.entry.as_str()) {
        return Err(format!(
            "direct backend could not find entry block `{}` in `{}`",
            function.entry, function.name
        ));
    }
    for block in &function.blocks {
        for successor in direct_terminator_successors(&block.terminator) {
            if !blocks.contains_key(successor) {
                return Err(format!(
                    "direct backend MIR block `{}` in `{}` targets unknown block `{successor}`",
                    block.label, function.name
                ));
            }
        }
    }

    let mut reachable = HashSet::new();
    let mut pending = vec![function.entry.as_str()];
    while let Some(label) = pending.pop() {
        if !reachable.insert(label.to_string()) {
            continue;
        }
        let block = blocks.get(label).ok_or_else(|| {
            format!(
                "direct backend could not resolve MIR block `{label}` in `{}`",
                function.name
            )
        })?;
        pending.extend(direct_terminator_successors(&block.terminator));
    }
    Ok(reachable)
}

fn direct_view_selector_tags(
    function: &MirFunction,
    reachable: &HashSet<String>,
) -> HashMap<String, HashMap<String, i64>> {
    let mut projections = HashMap::<String, BTreeSet<String>>::new();
    for block in &function.blocks {
        if !reachable.contains(&block.label) {
            continue;
        }
        for instruction in &block.instructions {
            let Instruction::BeginReturnedLoan {
                loan,
                projections: loan_projections,
                ..
            } = instruction
            else {
                continue;
            };
            projections
                .entry(loan.clone())
                .or_default()
                .extend(loan_projections.iter().cloned());
        }
    }
    projections
        .into_iter()
        .map(|(loan, projections)| {
            let tags = projections
                .into_iter()
                .enumerate()
                .map(|(index, projection)| (projection, index as i64))
                .collect();
            (loan, tags)
        })
        .collect()
}

fn direct_view_maps_equivalent(
    left: &HashMap<String, DirectViewPlace>,
    right: &HashMap<String, DirectViewPlace>,
) -> bool {
    left.len() == right.len()
        && left.iter().all(|(loan, left_place)| {
            right.get(loan).is_some_and(|right_place| {
                left_place.alternatives.len() == right_place.alternatives.len()
                    && left_place.alternatives.iter().all(|left_alternative| {
                        right_place.alternatives.iter().any(|right_alternative| {
                            left_alternative.place == right_alternative.place
                                && left_alternative.union_payloads
                                    == right_alternative.union_payloads
                                && left_alternative.conditions.len()
                                    == right_alternative.conditions.len()
                                && left_alternative.conditions.iter().all(|condition| {
                                    right_alternative.conditions.contains(condition)
                                })
                        })
                    })
            })
        })
}

fn merge_direct_closure_writebacks(
    left: &HashMap<String, Vec<DirectClosureCaptureWriteback>>,
    right: &HashMap<String, Vec<DirectClosureCaptureWriteback>>,
) -> HashMap<String, Vec<DirectClosureCaptureWriteback>> {
    let mut merged = left.clone();
    for (closure, writebacks) in right {
        let merged_writebacks = merged.entry(closure.clone()).or_default();
        for writeback in writebacks {
            if let Some(existing) = merged_writebacks
                .iter_mut()
                .find(|existing| existing.index == writeback.index && existing.ty == writeback.ty)
            {
                for alternative in &writeback.place.alternatives {
                    if !existing.place.alternatives.contains(alternative) {
                        existing.place.alternatives.push(alternative.clone());
                    }
                }
            } else {
                merged_writebacks.push(writeback.clone());
            }
        }
    }
    merged
}

fn direct_closure_metadata_uses(value: &Rvalue) -> impl Iterator<Item = &str> {
    let transferred = match value {
        Rvalue::Use(Operand::Place(place) | Operand::MovePlace(place))
        | Rvalue::CallableAdapt {
            value: Operand::Place(place) | Operand::MovePlace(place),
            ..
        } => Some(place.as_str()),
        _ => None,
    };
    let called = match value {
        Rvalue::Call {
            callee: CallTarget::Value(Operand::Place(place) | Operand::MovePlace(place)),
            ..
        } => Some(place.as_str()),
        _ => None,
    };
    transferred
        .into_iter()
        .chain(called)
        .map(|place| place.split('.').next().unwrap_or(place))
}

/// Computes which closure values still need mutable-capture writeback
/// descriptors on entry to each reachable block. The descriptors are compiler
/// metadata, not part of the runtime closure value, and are only observed when
/// an indirect call is compiled or when a closure value is transferred to
/// another local. Keeping dead descriptors across a loop backedge makes a
/// loop-local closure look like a new loop-carried identity after the header
/// has already been compiled.
fn direct_closure_writeback_live_ins(
    function: &MirFunction,
    reachable: &HashSet<String>,
) -> HashMap<String, HashSet<String>> {
    let closure_roots = function
        .local_types
        .iter()
        .filter(|local| matches!(&local.ty, Type::Closure { .. }))
        .map(|local| {
            local
                .name
                .split('.')
                .next()
                .unwrap_or(&local.name)
                .to_string()
        })
        .collect::<HashSet<_>>();
    let blocks = function
        .blocks
        .iter()
        .filter(|block| reachable.contains(&block.label))
        .map(|block| (block.label.as_str(), block))
        .collect::<HashMap<_, _>>();
    let mut block_uses = HashMap::<String, HashSet<String>>::new();
    let mut block_defs = HashMap::<String, HashSet<String>>::new();

    for block in blocks.values() {
        let mut uses = HashSet::new();
        let mut defs = HashSet::new();
        for instruction in &block.instructions {
            let value = match instruction {
                Instruction::Assign { value, .. } | Instruction::WriteLoan { value, .. } => {
                    Some(value)
                }
                _ => None,
            };
            if let Some(value) = value {
                for root in direct_closure_metadata_uses(value) {
                    if closure_roots.contains(root) && !defs.contains(root) {
                        uses.insert(root.to_string());
                    }
                }
            }
            if let Instruction::Assign { target, .. } = instruction {
                let root = target.split('.').next().unwrap_or(target);
                if closure_roots.contains(root) {
                    defs.insert(root.to_string());
                }
            }
        }
        block_uses.insert(block.label.clone(), uses);
        block_defs.insert(block.label.clone(), defs);
    }

    let mut live_ins = reachable
        .iter()
        .map(|label| (label.clone(), HashSet::new()))
        .collect::<HashMap<_, _>>();
    loop {
        let mut changed = false;
        for block in blocks.values() {
            let mut live_out = HashSet::new();
            for successor in direct_terminator_successors(&block.terminator) {
                if let Some(successor_live_in) = live_ins.get(successor) {
                    live_out.extend(successor_live_in.iter().cloned());
                }
            }
            let defs = &block_defs[&block.label];
            live_out.retain(|root| !defs.contains(root));
            live_out.extend(block_uses[&block.label].iter().cloned());
            let entry = live_ins
                .get_mut(&block.label)
                .expect("reachable block should have closure liveness state");
            if *entry != live_out {
                *entry = live_out;
                changed = true;
            }
        }
        if !changed {
            break;
        }
    }
    live_ins
}

fn direct_reverse_postorder(
    function: &MirFunction,
    reachable: &HashSet<String>,
) -> std::result::Result<Vec<String>, String> {
    let blocks = function
        .blocks
        .iter()
        .map(|block| (block.label.as_str(), block))
        .collect::<HashMap<_, _>>();
    let mut discovered = HashSet::new();
    let mut postorder = Vec::with_capacity(reachable.len());
    let mut pending = vec![(function.entry.clone(), false)];
    while let Some((label, expanded)) = pending.pop() {
        if expanded {
            postorder.push(label);
            continue;
        }
        if !discovered.insert(label.clone()) {
            continue;
        }
        let block = blocks.get(label.as_str()).ok_or_else(|| {
            format!(
                "direct backend could not resolve reachable block `{label}` in `{}`",
                function.name
            )
        })?;
        pending.push((label, true));
        for successor in direct_terminator_successors(&block.terminator)
            .into_iter()
            .rev()
        {
            if reachable.contains(successor) && !discovered.contains(successor) {
                pending.push((successor.to_string(), false));
            }
        }
    }
    postorder.reverse();
    Ok(postorder)
}

struct FunctionCompiler<'a> {
    builder: &'a mut dyn FunctionBackend,
    blocks: HashMap<String, Blk>,
    variables: HashMap<String, Vec<Var>>,
    variable_types: HashMap<String, DirectType>,
    next_variable_index: usize,
    function_refs: HashMap<String, FuncRef>,
    function_thunk_refs: HashMap<String, FuncRef>,
    cleanup_thunk_refs: HashMap<String, FuncRef>,
    function_return_types: HashMap<String, DirectType>,
    function_param_types: HashMap<String, Vec<DirectType>>,
    function_writeback_types: HashMap<String, Vec<DirectType>>,
    current_function_name: String,
    current_function_path: String,
    writeback_locals: Vec<(String, DirectType)>,
    mutable_param_indices: HashMap<String, usize>,
    classes: HashMap<String, MirClass>,
    enums: HashMap<String, MirEnum>,
    union_layouts: HashMap<String, crate::union_layout::MirUnionLayout>,
    method_receivers: &'a HashMap<String, Option<MirReceiverKind>>,
    return_type: Type,
    layout_scratch: LayoutScratch,
    /// The roots whose values the MIR retains and releases. Any other root
    /// whose layout owns a handle holds a boxed Copy value, which the
    /// backend drops itself: see `release_boxed_copy_roots`.
    tracked_roots: HashSet<String>,
    /// The loans, whose locals hold nothing.
    loan_locals: HashSet<String>,
    /// The match blocks whose fall-through the MIR treats as unreachable.
    /// It releases nothing there, so the edge traps.
    closed_matches: HashSet<String>,
    /// The `Lookup` temporaries kept as the found value's box or null. See
    /// `lookups::lookup_payload_slots`.
    lookup_slots: HashSet<String>,
    /// The lookup slots whose found value is lent, and the locals that hold
    /// that value. See `lookups::lent_lookup_places`. Each holds null, a
    /// box, or a value's address with its low bit set, so a retain or a
    /// release masks the address out.
    lent_places: HashSet<String>,
    /// Set while a `get` for a lent slot compiles.
    lend_lookup: bool,
    /// Set while a `get` for one of `lookup_slots` compiles: `Some(false)`
    /// asks for the payload form, and the helper that emits it sets
    /// `Some(true)`.
    lookup_payload: Option<bool>,
    /// A tracked root the current instruction assigns. The MIR released its
    /// old value, so the store adds no release.
    emptied_store_root: Option<String>,
    /// The temporaries of the function's lends, each with whether its lend
    /// is mutable. The call that writes back a mutable one takes its value,
    /// as MIR states by giving it no retain. A builtin operation changes a
    /// lent receiver in place, so it stores nothing back.
    lent_temps: HashMap<String, bool>,
    /// Each retained handle with the retain that made it and the handle it
    /// retained. A consumer that copies the value into its slot turns the
    /// retain into the copy.
    retain_calls: HashMap<Val, (Inst, Val)>,
    /// The releases of each retained handle, and the handles a consumer
    /// took. A handle no consumer took was only read: its retain and its
    /// releases cancel at the end of the instruction.
    retain_releases: HashMap<Val, Vec<Inst>>,
    consumed_retains: HashSet<Val>,
    /// The retain adapter call of each callable a `Retain` made, by its
    /// first word, with the drop calls that release it. A consumer that
    /// takes the callable removes it; the rest cancel like opaque retains.
    callable_retains: HashMap<Val, (Inst, Vec<Inst>)>,
    /// The tag words of inline unions a `Retain` made. Their handle stays
    /// shared with the root, so boxing one copies the payload.
    retained_unions: HashSet<Val>,
    /// The latest `tag_value_type` call: its instruction, value, and encoded
    /// type, so an identical tag that no other call follows is not repeated.
    last_runtime_type_tag: Option<(Inst, Val, String)>,
    view_places: HashMap<String, DirectViewPlace>,
    view_selector_vars: HashMap<String, Var>,
    /// Owned dictionary keys held by entry loans, released at `EndLoan`.
    view_element_keys: HashMap<String, Vec<Var>>,
    view_selector_tags: HashMap<String, HashMap<String, i64>>,
    closure_selector_vars: HashMap<String, Var>,
    closure_selector_tags: HashMap<(String, usize), i64>,
    closure_capture_writebacks: HashMap<String, Vec<DirectClosureCaptureWriteback>>,
    /// Host builtin entries this function calls, declared on first use so the
    /// object imports only the builtins the program calls.
    host_builtin_refs: HashMap<&'static str, FuncRef>,
    string_data: &'a mut HashMap<Vec<u8>, DataHandle>,
    tag_cells: &'a mut HashMap<String, DataHandle>,
    callable_shapes: &'a HashMap<CallableShapeKey, CallableShape>,
    contract_descriptors: &'a mut HashMap<String, callables::ContractDescriptor>,
    cleanup_places: Vec<String>,
    /// The typed exit action each cleanup place registers.
    cleanup_actions: HashMap<String, CleanupAction>,
    /// What each cleanup place's action runs, as MIR names it.
    cleanup_callees: HashMap<String, CleanupCallee>,
    cleanup_active_vars: HashMap<String, Var>,
    cleanup_registration_vars: HashMap<String, Var>,
    safepoint_fuel: Option<Var>,
    runtime: RuntimeTable<FuncRef>,
}

impl<'a> FunctionCompiler<'a> {
    fn compiled_block(&self, label: &str) -> std::result::Result<Blk, String> {
        self.blocks.get(label).copied().ok_or_else(|| {
            format!(
                "direct backend could not find MIR block `{label}` in `{}`",
                self.current_function_name
            )
        })
    }

    fn snapshot_view_place_selectors(
        &mut self,
        mut place: DirectViewPlace,
    ) -> std::result::Result<DirectViewPlace, String> {
        let mut snapshots = Vec::<(Var, Var)>::new();
        for alternative in &mut place.alternatives {
            for (selector, _) in &mut alternative.conditions {
                let snapshot = if let Some((_, snapshot)) =
                    snapshots.iter().find(|(original, _)| original == selector)
                {
                    *snapshot
                } else {
                    let snapshot = Var(self.next_variable_index as u32);
                    self.next_variable_index =
                        self.next_variable_index.checked_add(1).ok_or_else(|| {
                            "direct backend exhausted selector snapshot variables".to_string()
                        })?;
                    self.builder.declare_var(snapshot, types::I64);
                    let current = self.builder.use_var(*selector);
                    self.builder.def_var(snapshot, current);
                    snapshots.push((*selector, snapshot));
                    snapshot
                };
                *selector = snapshot;
            }
        }
        Ok(place)
    }

    fn local_type(&self, name: &str) -> std::result::Result<DirectType, String> {
        self.variable_types.get(name).cloned().ok_or(format!(
            "direct backend does not know local type for `{}`",
            name
        ))
    }

    fn local_vars(&self, name: &str) -> std::result::Result<Vec<Var>, String> {
        self.variables
            .get(name)
            .cloned()
            .ok_or(format!("direct backend does not know local `{}`", name))
    }

    fn is_opaque_value(&self, value: &ValueRef) -> bool {
        matches!(value.ty, DirectType::Opaque(_))
    }

    fn temporary_owns_opaque(&self, value: &ValueRef) -> bool {
        self.is_opaque_value(value) && self.layout_scratch.contains(&value.values[0])
    }

    fn mark_temporary_opaque_owned(&mut self, value: &ValueRef) {
        if self.is_opaque_value(value) {
            self.layout_scratch.insert(value.values[0]);
        }
    }

    fn clear_temporary_opaque_owned(&mut self, value: &ValueRef) {
        if self.is_opaque_value(value) {
            self.layout_scratch.remove(&value.values[0]);
            if self.retain_calls.contains_key(&value.values[0]) {
                self.consumed_retains.insert(value.values[0]);
            }
        }
    }

    fn retain_opaque_handle(&mut self, value: Val) -> Val {
        let call = self
            .builder
            .call(self.runtime[RuntimeFn::RetainValue], &[value]);
        call.results[0]
    }

    fn release_opaque_handle(&mut self, value: Val) {
        let call = self
            .builder
            .call(self.runtime[RuntimeFn::ReleaseValue], &[value]);
        if self.retain_calls.contains_key(&value) {
            self.retain_releases
                .entry(value)
                .or_default()
                .push(call.inst);
        }
    }

    fn release_all_temporary_owned(&mut self) {
        self.release_unconsumed_retains();
        let owned = self
            .layout_scratch
            .opaque
            .iter()
            .copied()
            .collect::<Vec<_>>();
        for value in owned {
            self.release_opaque_handle(value);
        }
        let unions = std::mem::take(&mut self.layout_scratch.unions);
        for (words, union) in unions.into_values() {
            self.release_union_words(&union, &words);
        }
        let callables = std::mem::take(&mut self.layout_scratch.callables);
        for words in callables.into_values() {
            self.release_callable_words(&words);
        }
        self.layout_scratch.clear();
    }

    fn release_temporary_owned_since(&mut self, baseline: &LayoutScratch) {
        let created = self
            .layout_scratch
            .opaque
            .difference(&baseline.opaque)
            .copied()
            .collect::<Vec<_>>();
        for value in created {
            self.release_opaque_handle(value);
            self.layout_scratch.remove(&value);
        }
        let created = self
            .layout_scratch
            .unions
            .keys()
            .filter(|tag| !baseline.unions.contains_key(tag))
            .copied()
            .collect::<Vec<_>>();
        for tag in created {
            if let Some((words, union)) = self.layout_scratch.unions.remove(&tag) {
                self.release_union_words(&union, &words);
            }
        }
        let created = self
            .layout_scratch
            .callables
            .keys()
            .filter(|descriptor| !baseline.callables.contains_key(descriptor))
            .copied()
            .collect::<Vec<_>>();
        for descriptor in created {
            if let Some(words) = self.layout_scratch.callables.remove(&descriptor) {
                self.release_callable_words(&words);
            }
        }
    }

    fn temporary_owns_union(&self, value: &ValueRef) -> bool {
        matches!(value.ty, DirectType::Union(_))
            && self.layout_scratch.unions.contains_key(&value.values[0])
    }

    fn mark_temporary_union_owned(&mut self, value: &ValueRef) {
        if let DirectType::Union(union) = &value.ty {
            if union.owns_handles() {
                self.layout_scratch
                    .unions
                    .insert(value.values[0], (value.values.clone(), union.clone()));
            }
        }
    }

    fn clear_temporary_union_owned(&mut self, value: &ValueRef) {
        self.layout_scratch.unions.remove(&value.values[0]);
    }

    /// Releases the active member's handle of an inline union, if the active
    /// member owns one; a null handle (a moved-from local) is skipped by the
    /// runtime.
    fn release_union_words(&mut self, union: &DirectUnionType, words: &[Val]) {
        for &index in &union.owning {
            let active = self.builder.icmp_imm(IntCC::Equal, words[0], index as i64);
            let release_block = self.builder.create_block();
            let continue_block = self.builder.create_block();
            self.builder
                .brif(active, release_block, &[], continue_block, &[]);
            self.builder.switch_to_block(release_block);
            self.builder.seal_block(release_block);
            if matches!(union.member(index), Ok(DirectType::Callable(_))) {
                let callable_words = words[1..2 + CALLABLE_ENVIRONMENT_WORDS].to_vec();
                self.call_callable_adapter_on_words(&callable_words, callables::DESCRIPTOR_DROP);
            } else {
                self.release_opaque_handle(words[1]);
            }
            self.builder.jump(continue_block, &[]);
            self.builder.switch_to_block(continue_block);
            self.builder.seal_block(continue_block);
        }
    }

    /// Retains the active member's handle of an inline union, if the active
    /// member owns one.
    fn retain_union_words(&mut self, union: &DirectUnionType, words: &[Val]) {
        for &index in &union.owning {
            let active = self.builder.icmp_imm(IntCC::Equal, words[0], index as i64);
            let retain_block = self.builder.create_block();
            let continue_block = self.builder.create_block();
            self.builder
                .brif(active, retain_block, &[], continue_block, &[]);
            self.builder.switch_to_block(retain_block);
            self.builder.seal_block(retain_block);
            if matches!(union.member(index), Ok(DirectType::Callable(_))) {
                let callable_words = words[1..2 + CALLABLE_ENVIRONMENT_WORDS].to_vec();
                self.call_callable_adapter_on_words(&callable_words, callables::DESCRIPTOR_RETAIN);
            } else {
                let _ = self.retain_opaque_handle(words[1]);
            }
            self.builder.jump(continue_block, &[]);
            self.builder.switch_to_block(continue_block);
            self.builder.seal_block(continue_block);
        }
    }

    /// The words of an inline union handed to a new owner: an owned
    /// temporary moves, a borrowed value retains its active handle.
    fn transfer_union_words(&mut self, value: &ValueRef) -> Vec<Val> {
        if self.temporary_owns_union(value) {
            self.clear_temporary_union_owned(value);
            return value.values.clone();
        }
        if let DirectType::Union(union) = &value.ty {
            let union = union.clone();
            self.retain_union_words(&union, &value.values);
        }
        value.values.clone()
    }

    fn temporary_owns_callable(&self, value: &ValueRef) -> bool {
        matches!(value.ty, DirectType::Callable(_))
            && self.layout_scratch.callables.contains_key(&value.values[0])
    }

    fn mark_temporary_callable_owned(&mut self, value: &ValueRef) {
        if matches!(value.ty, DirectType::Callable(_)) {
            self.layout_scratch
                .callables
                .insert(value.values[0], value.values.clone());
        }
    }

    fn clear_temporary_callable_owned(&mut self, value: &ValueRef) {
        if matches!(value.ty, DirectType::Callable(_)) {
            self.layout_scratch.callables.remove(&value.values[0]);
            self.callable_retains.remove(&value.values[0]);
        }
    }

    /// Calls the `drop` or `retain` adapter named by an inline callable's
    /// descriptor on its four words; a zero descriptor (a moved-from or
    /// never-assigned value) names no shape and is skipped.
    fn call_callable_adapter_on_words(&mut self, words: &[Val], descriptor_offset: i32) -> Inst {
        let live = self.builder.icmp_imm(IntCC::NotEqual, words[0], 0);
        let call_block = self.builder.create_block();
        let continue_block = self.builder.create_block();
        self.builder
            .brif(live, call_block, &[], continue_block, &[]);
        self.builder.switch_to_block(call_block);
        self.builder.seal_block(call_block);
        let call = self.call_callable_adapter_on_live_words(words, descriptor_offset);
        self.builder.jump(continue_block, &[]);
        self.builder.switch_to_block(continue_block);
        self.builder.seal_block(continue_block);
        call
    }

    fn call_callable_adapter_on_live_words(
        &mut self,
        words: &[Val],
        descriptor_offset: i32,
    ) -> Inst {
        let slot = self
            .builder
            .create_sized_stack_slot((8 * (1 + CALLABLE_ENVIRONMENT_WORDS)) as u32, 3);
        let callable_ptr = self.builder.stack_addr(types::I64, slot, 0);
        for (index, word) in words.iter().enumerate() {
            self.builder
                .store(MemFlags::new(), *word, callable_ptr, (index as i32) * 8);
        }
        let adapter = self
            .builder
            .load(types::I64, MemFlags::new(), words[0], descriptor_offset);
        let signature = self
            .builder
            .import_signature(callables::callable_unary_signature());
        self.builder
            .call_indirect(signature, adapter, &[callable_ptr])
            .inst
    }

    /// Boxes an inline callable through its descriptor's `box` adapter and
    /// returns the owned handle; the inline value stays alive.
    fn call_callable_box_on_words(&mut self, words: &[Val]) -> Val {
        let slot = self
            .builder
            .create_sized_stack_slot((8 * (1 + CALLABLE_ENVIRONMENT_WORDS)) as u32, 3);
        let callable_ptr = self.builder.stack_addr(types::I64, slot, 0);
        for (index, word) in words.iter().enumerate() {
            self.builder
                .store(MemFlags::new(), *word, callable_ptr, (index as i32) * 8);
        }
        let adapter = self.builder.load(
            types::I64,
            MemFlags::new(),
            words[0],
            callables::DESCRIPTOR_BOX,
        );
        let signature = self
            .builder
            .import_signature(callables::callable_box_signature());
        let call = self
            .builder
            .call_indirect(signature, adapter, &[callable_ptr]);
        call.results[0]
    }

    /// The four words of a boxed function value re-entering as an inline
    /// callable of `signature`.
    fn boxed_callable_words(
        &mut self,
        handle: Val,
        signature: &Type,
    ) -> std::result::Result<Vec<Val>, String> {
        let contract = callables::declare_contract_descriptor(
            self.builder.module(),
            &mut *self.contract_descriptors,
            &self.classes,
            signature,
        )?;
        let global = self.builder.import_data(contract.descriptor);
        let descriptor = self.builder.symbol_value(types::I64, global);
        let zero = self.builder.iconst(types::I64, 0);
        Ok(vec![descriptor, handle, zero, zero])
    }

    /// Releases the captures an inline callable owns.
    fn release_callable_words(&mut self, words: &[Val]) {
        let release = self.call_callable_adapter_on_words(words, callables::DESCRIPTOR_DROP);
        if let Some((_, releases)) = self.callable_retains.get_mut(&words[0]) {
            releases.push(release);
        }
    }

    fn retain_callable_words(&mut self, words: &[Val]) {
        self.call_callable_adapter_on_words(words, callables::DESCRIPTOR_RETAIN);
    }

    /// The words of an inline callable handed to a new owner: an owned
    /// temporary moves, a borrowed value retains its captures.
    fn transfer_callable_words(&mut self, value: &ValueRef) -> Vec<Val> {
        if self.temporary_owns_callable(value) {
            self.clear_temporary_callable_owned(value);
            return value.values.clone();
        }
        self.retain_callable_words(&value.values);
        value.values.clone()
    }

    /// Argument or field words for any direct value: opaque handles,
    /// owning unions, and inline callables transfer, everything else copies.
    fn transfer_values(&mut self, value: &ValueRef) -> Vec<Val> {
        match &value.ty {
            DirectType::Opaque(_) => vec![self.transfer_opaque_arg(value)],
            DirectType::Union(union) if union.owns_handles() => self.transfer_union_words(value),
            DirectType::Callable(_) => self.transfer_callable_words(value),
            _ => value.values.clone(),
        }
    }

    /// Releases the handles the words of root `name` own, by its layout:
    /// an opaque handle, the active member of an inline union, or the
    /// captures of an inline callable. A plain class owns no handle. The
    /// zero words of an empty root release nothing.
    fn release_root_words(&mut self, name: &str) -> std::result::Result<(), String> {
        let ty = self.local_type(name)?;
        if !layout_owns_handles(&ty) {
            return Ok(());
        }
        let words = self
            .local_vars(name)?
            .iter()
            .map(|var| self.builder.use_var(*var))
            .collect::<Vec<_>>();
        if words
            .iter()
            .all(|word| self.integer_constant(*word) == Some(0))
        {
            return Ok(());
        }
        match &ty {
            DirectType::Union(union) => {
                let union = union.clone();
                self.release_union_words(&union, &words);
            }
            DirectType::Callable(_) => self.release_callable_words(&words),
            _ if self.lent_places.contains(name) => {
                let owned = self.owned_lent_word(words[0]);
                self.release_opaque_handle(owned);
            }
            _ => self.release_opaque_handle(words[0]),
        }
        Ok(())
    }

    /// The box a lent lookup result owns: the word itself, or null for a
    /// value's address, which has its low bit set.
    fn owned_lent_word(&mut self, word: Val) -> Val {
        let bit = self.builder.band_imm(word, 1);
        let lent = self.builder.icmp_imm(IntCC::NotEqual, bit, 0);
        let null = self.builder.iconst(types::I64, 0);
        self.builder.select(lent, null, word)
    }

    /// A new reference to a lent lookup result. Only a box counts it.
    fn retain_lent_word(&mut self, word: Val) {
        let owned = self.owned_lent_word(word);
        self.builder
            .call(self.runtime[RuntimeFn::RetainValue], &[owned]);
    }

    /// Stores a lent lookup result in root `name` as it is.
    fn store_lent_word(&mut self, name: &str, word: Val) -> std::result::Result<(), String> {
        if self.emptied_store_root.as_deref() == Some(name) {
            self.emptied_store_root = None;
        } else {
            self.release_root_words(name)?;
        }
        let vars = self.local_vars(name)?;
        self.builder.def_var(vars[0], word);
        Ok(())
    }

    /// Assigns the found value of a lent lookup, or a copy of one, to
    /// another of the places that hold it. The words move as they are,
    /// with the reference `Retain` counts or the payload read adds.
    fn compile_lent_assign(
        &mut self,
        target: &str,
        value: &Rvalue,
    ) -> std::result::Result<(), String> {
        let (source, payload, moved) = match value {
            Rvalue::VariantPayload {
                scrutinee: Operand::Place(source),
                ..
            } => (source, true, false),
            Rvalue::VariantPayload {
                scrutinee: Operand::MovePlace(source),
                ..
            } => (source, true, true),
            Rvalue::Use(Operand::Place(source)) => (source, false, false),
            Rvalue::Use(Operand::MovePlace(source)) => (source, false, true),
            other => {
                return Err(format!(
                    "direct backend cannot lend `{target}` from `{other:?}`"
                ))
            }
        };
        let word = self.load_place(source)?.values[0];
        if moved {
            let vars = self.local_vars(source)?;
            let null = self.builder.iconst(types::I64, 0);
            self.builder.def_var(vars[0], null);
        } else if payload {
            // The payload is a new reference to the slot's value.
            self.retain_lent_word(word);
        }
        self.store_lent_word(target, word)
    }

    /// Lowers MIR `Release`: drops the root's value by its layout and
    /// leaves the root empty, with all-zero words.
    fn release_root(&mut self, name: &str) -> std::result::Result<(), String> {
        self.release_root_words(name)?;
        let ty = self.local_type(name)?;
        if !layout_owns_handles(&ty) {
            return Ok(());
        }
        for (var, abi) in self.local_vars(name)?.into_iter().zip(ty.abi_types()) {
            let zero = if abi == types::F64 {
                self.builder.f64const(0.0)
            } else {
                self.builder.iconst(abi, 0)
            };
            self.builder.def_var(var, zero);
        }
        Ok(())
    }

    /// Releases, in name order, the roots the MIR does not track whose
    /// layout owns a handle. They hold boxed Copy values, such as an
    /// `int8`, a `Duration`, a Copy enum, or a tuple of Copy values: the
    /// MIR copies them freely and never releases them, and this backend
    /// boxes them. `mut` parameters go back to the caller instead.
    fn release_boxed_copy_roots(&mut self) -> std::result::Result<(), String> {
        let mut names = self
            .variable_types
            .iter()
            .filter(|(name, ty)| {
                layout_owns_handles(ty)
                    && !self.tracked_roots.contains(*name)
                    && !self.loan_locals.contains(*name)
                    && !self
                        .writeback_locals
                        .iter()
                        .any(|(writeback, _)| writeback == *name)
            })
            .map(|(name, _)| name.clone())
            .collect::<Vec<_>>();
        names.sort();
        for name in names {
            self.release_root_words(&name)?;
        }
        Ok(())
    }

    fn transfer_opaque_arg(&mut self, value: &ValueRef) -> Val {
        if self.temporary_owns_opaque(value) {
            self.clear_temporary_opaque_owned(value);
            value.values[0]
        } else {
            self.retain_opaque_handle(value.values[0])
        }
    }

    fn transfer_owned_opaque_value(&mut self, value: &ValueRef) -> Val {
        if self.temporary_owns_opaque(value) {
            self.clear_temporary_opaque_owned(value);
            // A slot that holds its value inline needs a copy, not a second
            // reference, so the retain becomes the copy.
            if let Some((inst, handle)) = self.retain_calls.remove(&value.values[0]) {
                let clone = self.runtime[RuntimeFn::CloneValue];
                self.builder.replace_with_call(inst, clone, &[handle]);
            }
            value.values[0]
        } else {
            let inst = self
                .builder
                .call(self.runtime[RuntimeFn::CloneValue], &[value.values[0]]);
            inst.results[0]
        }
    }

    fn owned_opaque_result(&mut self, values: Vec<Val>, ty: Type) -> ValueRef {
        let value = ValueRef {
            values,
            ty: DirectType::Opaque(ty),
        };
        self.mark_temporary_opaque_owned(&value);
        value
    }

    fn runtime_call_results(&mut self, callee: FuncRef, args: &[Val]) -> Vec<Val> {
        let inst = self.builder.call(callee, args);
        inst.results
    }

    fn compile_reachable_blocks(
        &mut self,
        function: &MirFunction,
        return_ty: &DirectType,
    ) -> std::result::Result<(), String> {
        let reachable = self.blocks.keys().cloned().collect::<HashSet<_>>();
        let order = direct_reverse_postorder(function, &reachable)?;
        let closure_writeback_live_ins = direct_closure_writeback_live_ins(function, &reachable);
        let block_by_label = function
            .blocks
            .iter()
            .filter(|block| self.blocks.contains_key(&block.label))
            .map(|block| (block.label.as_str(), block))
            .collect::<HashMap<_, _>>();
        let mut incoming_views = HashMap::<String, HashMap<String, DirectViewPlace>>::new();
        incoming_views.insert(function.entry.clone(), HashMap::new());
        let mut incoming_writebacks =
            HashMap::<String, HashMap<String, Vec<DirectClosureCaptureWriteback>>>::new();
        incoming_writebacks.insert(function.entry.clone(), HashMap::new());
        let mut compiled = HashSet::new();

        for label in order {
            let block = block_by_label.get(label.as_str()).ok_or_else(|| {
                format!(
                    "direct backend could not resolve reachable block `{label}` in `{}`",
                    function.name
                )
            })?;
            self.view_places = incoming_views.get(&label).cloned().ok_or_else(|| {
                format!(
                    "direct backend has no incoming view state for block `{label}` in `{}`",
                    function.name
                )
            })?;
            self.closure_capture_writebacks =
                incoming_writebacks.get(&label).cloned().ok_or_else(|| {
                    format!(
                        "direct backend has no incoming closure-writeback state for block `{label}` in `{}`",
                        function.name
                    )
                })?;
            self.compile_block(block, return_ty)?;
            compiled.insert(label.clone());
            let outgoing_views = self.view_places.clone();
            let outgoing_writebacks = self.closure_capture_writebacks.clone();

            for successor in direct_terminator_successors(&block.terminator) {
                match incoming_views.get(successor) {
                    Some(existing) if !direct_view_maps_equivalent(existing, &outgoing_views) => {
                        return Err(format!(
                            "direct backend reaches MIR block `{successor}` in `{}` with inconsistent view identities",
                            function.name
                        ));
                    }
                    Some(_) => {}
                    None => {
                        incoming_views.insert(successor.to_string(), outgoing_views.clone());
                    }
                }
                let live_writebacks = closure_writeback_live_ins.get(successor).ok_or_else(|| {
                    format!(
                        "direct backend has no closure-writeback liveness for block `{successor}` in `{}`",
                        function.name
                    )
                })?;
                let successor_writebacks = outgoing_writebacks
                    .iter()
                    .filter(|(closure, _)| live_writebacks.contains(*closure))
                    .map(|(closure, writebacks)| (closure.clone(), writebacks.clone()))
                    .collect::<HashMap<_, _>>();
                match incoming_writebacks.get(successor).cloned() {
                    Some(existing) => {
                        let merged =
                            merge_direct_closure_writebacks(&existing, &successor_writebacks);
                        if compiled.contains(successor) && merged != existing {
                            return Err(format!(
                                "direct backend reaches already-compiled MIR block `{successor}` in `{}` with new closure writeback identities",
                                function.name
                            ));
                        }
                        incoming_writebacks.insert(successor.to_string(), merged);
                    }
                    None => {
                        incoming_writebacks.insert(successor.to_string(), successor_writebacks);
                    }
                }
            }
        }

        if compiled.len() != self.blocks.len() {
            return Err(format!(
                "direct backend did not compile every reachable MIR block in `{}`",
                function.name
            ));
        }
        Ok(())
    }

    fn compile_block(
        &mut self,
        block: &BasicBlock,
        return_ty: &DirectType,
    ) -> std::result::Result<(), String> {
        let block_id = *self.blocks.get(&block.label).ok_or_else(|| {
            format!(
                "direct backend could not find compiled block `{}`",
                block.label
            )
        })?;
        if self.builder.current_block() != Some(block_id) {
            self.builder.switch_to_block(block_id);
        }
        debug_assert!(
            self.layout_scratch.is_empty(),
            "layout temporaries outlived the block before `{}`",
            block.label
        );

        for (instruction_index, instruction) in block.instructions.iter().enumerate() {
            // A fact emits nothing. The accesses it covers say so, and a
            // retain before it stays for the read that follows.
            if matches!(instruction, Instruction::IndexInRange { .. }) {
                continue;
            }
            self.compile_instruction(&block.label, instruction_index, instruction)?;
        }
        // A retain before the terminator pays for the value a `Return`
        // hands back.
        self.layout_scratch.consumers =
            consuming_operands(crate::mir::ownership::terminator_roles(&block.terminator));
        if !matches!(block.terminator, Terminator::Return(_)) {
            self.release_unconsumed_retains();
        }
        let compiled = self.compile_terminator(&block.label, &block.terminator, return_ty);
        self.layout_scratch.consumers.clear();
        debug_assert!(
            self.layout_scratch.retained.is_empty(),
            "a retain outlived the end of block `{}`",
            block.label
        );
        compiled
    }

    fn compile_instruction(
        &mut self,
        block_label: &str,
        instruction_index: usize,
        instruction: &Instruction,
    ) -> std::result::Result<(), String> {
        debug_assert!(
            self.layout_scratch.is_empty(),
            "layout temporaries outlived the instruction before {instruction:?}"
        );
        if let Instruction::Retain { place } = instruction {
            if self.lent_places.contains(place.as_str()) {
                let word = self.load_place(place)?.values[0];
                self.retain_lent_word(word);
                return Ok(());
            }
            return self.retain_place(place);
        }
        let erased = crate::mir::ownership::borrowed_receiver(instruction, |place| {
            self.type_of_place(place)
                .ok()
                .map(|ty| direct_type_to_type(&ty))
        });
        self.layout_scratch.consumers = consuming_operands(
            crate::mir::ownership::operand_roles_with(instruction, erased),
        );
        let compiled = self.compile_instruction_body(block_label, instruction_index, instruction);
        self.emptied_store_root = None;
        self.layout_scratch.consumers.clear();
        compiled?;
        self.release_all_temporary_owned();
        self.cancel_unconsumed_retains();
        Ok(())
    }

    /// Lowers MIR `Retain`: makes the new reference by the value's layout
    /// and keeps it for the consumer that follows. A copy the load already
    /// made is the new reference.
    fn retain_place(&mut self, place: &str) -> std::result::Result<(), String> {
        let value = self.load_place(place)?;
        let retained = match &value.ty {
            DirectType::Opaque(_) if self.temporary_owns_opaque(&value) => {
                self.clear_temporary_opaque_owned(&value);
                value
            }
            DirectType::Opaque(ty) => {
                let ty = ty.clone();
                let call = self
                    .builder
                    .call(self.runtime[RuntimeFn::RetainValue], &[value.values[0]]);
                let handle = call.results[0];
                self.retain_calls
                    .insert(handle, (call.inst, value.values[0]));
                ValueRef {
                    values: vec![handle],
                    ty: DirectType::Opaque(ty),
                }
            }
            DirectType::Union(union) if union.owns_handles() => {
                if self.temporary_owns_union(&value) {
                    self.clear_temporary_union_owned(&value);
                    value
                } else {
                    let union = union.clone();
                    self.retain_union_words(&union, &value.values);
                    let fresh = self.fresh_words(value);
                    self.retained_unions.insert(fresh.values[0]);
                    fresh
                }
            }
            DirectType::Callable(_) => {
                if self.temporary_owns_callable(&value) {
                    self.clear_temporary_callable_owned(&value);
                    value
                } else {
                    let retain = self.call_callable_adapter_on_words(
                        &value.values,
                        callables::DESCRIPTOR_RETAIN,
                    );
                    let fresh = self.fresh_words(value);
                    self.callable_retains
                        .insert(fresh.values[0], (retain, Vec::new()));
                    fresh
                }
            }
            _ => value,
        };
        // The load may leave temporaries behind; the new reference is kept
        // apart from them.
        self.release_all_temporary_owned();
        self.layout_scratch
            .retained
            .push((place.to_string(), retained));
        Ok(())
    }

    /// `value` in new SSA words, so the layout scratch can own it apart
    /// from the root it was read from.
    fn fresh_words(&mut self, value: ValueRef) -> ValueRef {
        let values = value
            .values
            .iter()
            .map(|word| {
                if self.builder.value_type(*word) == types::I64 {
                    self.builder.iadd_imm(*word, 0)
                } else {
                    *word
                }
            })
            .collect();
        ValueRef {
            values,
            ty: value.ty,
        }
    }

    /// The reference a `Retain` made for `operand`, when it is a consuming
    /// operand of the instruction. The value is then an owned temporary.
    fn take_retained(&mut self, operand: &Operand) -> Option<ValueRef> {
        let Operand::Place(place) = operand else {
            return None;
        };
        let consumer = self
            .layout_scratch
            .consumers
            .iter()
            .position(|consumer| std::ptr::eq(*consumer, operand))?;
        let position = self
            .layout_scratch
            .retained
            .iter()
            .position(|(retained, _)| retained == place)?;
        self.layout_scratch.consumers.remove(consumer);
        let (_, value) = self.layout_scratch.retained.remove(position);
        self.mark_temporary_opaque_owned(&value);
        self.mark_temporary_union_owned(&value);
        self.mark_temporary_callable_owned(&value);
        Some(value)
    }

    /// Cancels each retain of the instruction that no consumer took against
    /// its releases: the consumer only read the value, which its root holds
    /// throughout the instruction.
    fn cancel_unconsumed_retains(&mut self) {
        let retains = std::mem::take(&mut self.retain_calls);
        let mut releases = std::mem::take(&mut self.retain_releases);
        let consumed = std::mem::take(&mut self.consumed_retains);
        self.retained_unions.clear();
        // A callable a consumer took left this map; the rest were only read.
        for (retain, releases) in std::mem::take(&mut self.callable_retains).into_values() {
            for inst in std::iter::once(retain).chain(releases) {
                self.builder.replace_with_nop(inst);
            }
        }
        for (handle, (retain, retained)) in retains {
            if consumed.contains(&handle) {
                continue;
            }
            self.builder.replace_with_copy(retain, retained);
            for release in releases.remove(&handle).unwrap_or_default() {
                self.builder.replace_with_nop(release);
            }
        }
    }

    /// Restores the layout temporaries from before a branch at its merge.
    /// Every arm takes the same retained references, so the ones the last
    /// arm left stay for the rest of the instruction.
    fn restore_scratch_after_branches(&mut self, caller: LayoutScratch) {
        let retained = std::mem::take(&mut self.layout_scratch.retained);
        let consumers = std::mem::take(&mut self.layout_scratch.consumers);
        self.layout_scratch = caller;
        self.layout_scratch.retained = retained;
        self.layout_scratch.consumers = consumers;
    }

    /// Makes the references no consumer took owned temporaries, which the
    /// end of the instruction releases: their consumer only borrowed.
    fn release_unconsumed_retains(&mut self) {
        for (_, value) in std::mem::take(&mut self.layout_scratch.retained) {
            self.mark_temporary_opaque_owned(&value);
            self.mark_temporary_union_owned(&value);
            self.mark_temporary_callable_owned(&value);
        }
    }

    fn compile_instruction_body(
        &mut self,
        block_label: &str,
        instruction_index: usize,
        instruction: &Instruction,
    ) -> std::result::Result<(), String> {
        match instruction {
            // A fact emits nothing: the accesses it covers say so. A retain
            // pays for the consumer that follows. The consumer adds the
            // reference by layout when it hands the value on, so a consumer
            // that only borrows, such as a call dispatched on a boxed
            // generic receiver, adds none.
            Instruction::IndexInRange { .. } | Instruction::Retain { .. } => {}
            Instruction::Release { place } => self.release_root(place)?,
            Instruction::Lend {
                place,
                temp,
                mutable,
            } => {
                self.lent_temps.insert(temp.clone(), *mutable);
                let value = self.take_lent_place(place)?;
                self.mark_emptied_store_root(temp);
                self.store_root(temp, value)?;
            }
            Instruction::EndLend { place, temp, .. } => {
                let value = self.take_place(temp)?;
                self.store_place(place, value)?;
            }
            Instruction::Safepoint => {
                let Some(fuel_variable) = self.safepoint_fuel else {
                    return Ok(());
                };
                let fuel = self.builder.use_var(fuel_variable);
                let remaining = self.builder.iadd_imm(fuel, -1);
                self.builder.def_var(fuel_variable, remaining);
                let exhausted = self.builder.icmp_imm(IntCC::Equal, remaining, 0);
                let slow_block = self.builder.create_block();
                let continue_block = self.builder.create_block();
                self.builder
                    .brif(exhausted, slow_block, &[], continue_block, &[]);

                self.builder.switch_to_block(slow_block);
                let reset = self
                    .builder
                    .iconst(types::I64, NATIVE_LOOP_SAFEPOINT_INTERVAL as i64);
                self.builder.def_var(fuel_variable, reset);
                self.builder.call(self.runtime[RuntimeFn::YieldNow], &[]);
                self.builder.jump(continue_block, &[]);

                self.builder.switch_to_block(continue_block);
            }
            Instruction::BeginLoan { loan, source, .. } => {
                let source = self.resolve_view_place(source)?;
                self.view_places.insert(loan.clone(), source);
            }
            Instruction::BeginElementLoan {
                loan,
                source,
                selector,
                projection,
                span,
                checked,
                ..
            } => {
                self.begin_element_loan(loan, source, selector, projection, *span, *checked)?;
            }
            Instruction::ReborrowElement {
                loan,
                parent: source,
                selector,
                projection,
                span,
                ..
            } => {
                self.begin_element_loan(loan, source, selector, projection, *span, true)?;
            }
            Instruction::BeginReturnedLoan {
                loan,
                origin,
                projections,
                ..
            } => {
                let mut encoded = Vec::new();
                for (index, projection) in projections.iter().enumerate() {
                    if index != 0 {
                        encoded.push(0);
                    }
                    encoded.extend_from_slice(projection.as_bytes());
                }
                let (projections_ptr, projections_len) = self.string_constant(&encoded)?;
                let selected = self.builder.call(
                    self.runtime[RuntimeFn::TakeReturnedViewProjection],
                    &[projections_ptr, projections_len],
                );
                let selected = selected.results[0];
                let selector_var = *self.view_selector_vars.get(loan).ok_or_else(|| {
                    format!(
                        "direct backend has no returned-view selector storage for loan `{loan}`"
                    )
                })?;
                let selector_tags =
                    self.view_selector_tags.get(loan).cloned().ok_or_else(|| {
                        format!(
                            "direct backend has no returned-view selector tags for loan `{loan}`"
                        )
                    })?;
                let mut canonical_selector = self.builder.iconst(types::I64, -1);
                for (index, projection) in projections.iter().enumerate() {
                    let tag = *selector_tags.get(projection).ok_or_else(|| {
                        format!(
                            "direct backend has no selector tag for projection `{projection}` on loan `{loan}`"
                        )
                    })?;
                    let matches = self.builder.icmp_imm(
                        IntCC::Equal,
                        selected,
                        i64::try_from(index).map_err(|_| {
                            format!(
                                "direct backend returned-view projection index overflows for loan `{loan}`"
                            )
                        })?,
                    );
                    let tag = self.builder.iconst(types::I64, tag);
                    canonical_selector = self.builder.select(matches, tag, canonical_selector);
                }
                self.builder.def_var(selector_var, canonical_selector);
                let origin = self.resolve_view_place(origin)?;
                let loan_type = direct_type_to_type(&self.local_type(loan)?);
                // Element steps take their selectors from the callee once, in
                // order; an unselected projection's extra words read `0`.
                let element_steps = projections
                    .iter()
                    .map(|projection| {
                        projection
                            .split('.')
                            .filter(|segment| *segment == crate::mir::RETURNED_ELEMENT_STEP)
                            .count()
                    })
                    .max()
                    .unwrap_or(0);
                let selector_words = (0..element_steps)
                    .map(|step| {
                        let index = self.builder.iconst(types::I64, step as i64);
                        let inst = self
                            .builder
                            .call(self.runtime[RuntimeFn::TakeReturnedViewSelector], &[index]);
                        inst.results[0]
                    })
                    .collect::<Vec<_>>();
                let mut alternatives = Vec::new();
                for origin in origin.alternatives {
                    for projection in projections {
                        let mut conditions = origin.conditions.clone();
                        let tag = *selector_tags.get(projection).ok_or_else(|| {
                            format!(
                                "direct backend has no selector tag for projection `{projection}` on loan `{loan}`"
                            )
                        })?;
                        conditions.push((selector_var, tag));
                        if projection
                            .split('.')
                            .any(|segment| segment == crate::mir::RETURNED_ELEMENT_STEP)
                        {
                            let alternative = self.returned_element_alternative(
                                loan,
                                &origin,
                                projection,
                                conditions,
                                &selector_words,
                            )?;
                            if !alternatives.contains(&alternative) {
                                alternatives.push(alternative);
                            }
                            continue;
                        }
                        // Through an element origin, the returned projection
                        // lies inside the selected element.
                        let mut elements = origin.elements.clone();
                        let place = match elements.last_mut() {
                            Some(selector) if !projection.is_empty() => {
                                selector.projection = if selector.projection.is_empty() {
                                    projection.clone()
                                } else {
                                    format!("{}.{projection}", selector.projection)
                                };
                                selector.element_type = loan_type.clone();
                                origin.place.clone()
                            }
                            _ if projection.is_empty() => origin.place.clone(),
                            _ => format!("{}.{}", origin.place, projection),
                        };
                        let alternative = DirectViewAlternative {
                            place,
                            conditions,
                            union_payloads: origin.union_payloads.clone(),
                            elements,
                        };
                        if !alternatives.contains(&alternative) {
                            alternatives.push(alternative);
                        }
                    }
                }
                self.view_places
                    .insert(loan.clone(), DirectViewPlace { alternatives });
            }
            Instruction::Reborrow {
                loan,
                parent,
                projection,
                ..
            } => {
                let mut source = self.resolve_view_place(parent)?.project(projection);
                // A projection inside a selected element reaches a value of
                // the reborrow's own type, such as a union member's payload.
                if !projection.is_empty() {
                    let reached = direct_type_to_type(&self.local_type(loan)?);
                    for alternative in &mut source.alternatives {
                        if let Some(selector) = alternative.elements.last_mut() {
                            selector.element_type = reached.clone();
                        }
                    }
                }
                self.view_places.insert(loan.clone(), source);
            }
            Instruction::ReadLoan { target, loan } => {
                self.mark_emptied_store_root(target);
                let loaded = self.load_place(loan)?;
                let target_ty = self.type_of_place(target)?;
                let loaded = self.coerce_value(loaded, &target_ty)?;
                self.store_place(target, loaded)?;
            }
            Instruction::WriteLoan { loan, value } => {
                let target_ty = self.type_of_place(loan)?;
                let compiled = self.compile_rvalue_for_target(value, &target_ty)?;
                let coerced = self.coerce_value(compiled, &target_ty)?;
                self.store_place(loan, coerced)?;
            }
            Instruction::EndLoan { loan } => {
                self.view_places.remove(loan);
                // A loan that ends in both arms of a branch has one EndLoan
                // per arm, and each must release the keys, so keep them.
                for variable in self
                    .view_element_keys
                    .get(loan)
                    .cloned()
                    .unwrap_or_default()
                {
                    let key = self.builder.use_var(variable);
                    self.release_opaque_handle(key);
                }
            }
            Instruction::ReturnLoan { loan, origin } => {
                self.emit_returned_view_projection(loan, origin)?;
                let loan_root = loan.split('.').next().unwrap_or(loan);
                self.view_places.remove(loan_root);
                // The caller received retained copies of any string keys; this
                // path has no EndLoan, so release the loan's own references.
                // Other paths still end the loan, so the map keeps them.
                for variable in self
                    .view_element_keys
                    .get(loan_root)
                    .cloned()
                    .unwrap_or_default()
                {
                    let key = self.builder.use_var(variable);
                    self.release_opaque_handle(key);
                }
            }
            Instruction::Assign { target, value } => {
                if let Rvalue::Closure { captures, .. } = value {
                    let root = target.split('.').next().unwrap_or(target).to_string();
                    let selector = if captures
                        .iter()
                        .any(|capture| capture.passing == MirReceiverKind::BorrowMut)
                    {
                        let selector = *self.closure_selector_vars.get(&root).ok_or_else(|| {
                            format!("direct backend has no closure writeback selector for `{root}`")
                        })?;
                        let tag = *self
                            .closure_selector_tags
                            .get(&(block_label.to_string(), instruction_index))
                            .ok_or_else(|| {
                                format!(
                                    "direct backend has no closure writeback tag for instruction {instruction_index} in `{block_label}`"
                                )
                            })?;
                        let selected = self.builder.iconst(types::I64, tag);
                        self.builder.def_var(selector, selected);
                        Some((selector, tag))
                    } else {
                        None
                    };
                    let capture_widths = captures
                        .iter()
                        .map(|capture| {
                            ensure_direct_type(&capture.ty, &self.classes, "closure capture")
                                .map(|ty| ty.value_count())
                        })
                        .collect::<std::result::Result<Vec<_>, String>>()?;
                    let inline_environment =
                        capture_widths.iter().sum::<usize>() <= CALLABLE_ENVIRONMENT_WORDS;
                    let writebacks = captures
                        .iter()
                        .enumerate()
                        .filter_map(|(index, capture)| {
                            (capture.passing == MirReceiverKind::BorrowMut)
                                .then_some((index, capture))
                        })
                        .map(|(index, capture)| {
                            let source = capture.source_place.clone().ok_or_else(|| {
                                format!(
                                    "direct backend mutable closure capture `{}` has no source place",
                                    capture.name
                                )
                            })?;
                            let ty = ensure_direct_type(
                                &capture.ty,
                                &self.classes,
                                &format!("mutable closure capture `{}`", capture.name),
                            )?;
                            let place = if capture.resolve_source_at_capture {
                                let resolved = self.resolve_view_place(&source)?;
                                self.snapshot_view_place_selectors(resolved)?
                            } else {
                                DirectViewPlace::static_place(source)
                            };
                            let place = if let Some((selector, tag)) = selector {
                                place.conditioned(selector, tag)
                            } else {
                                place
                            };
                            Ok(DirectClosureCaptureWriteback {
                                index,
                                place,
                                ty,
                                env_offset: capture_widths[..index].iter().sum(),
                                inline: inline_environment,
                            })
                        })
                        .collect::<std::result::Result<Vec<_>, String>>()?;
                    if writebacks.is_empty() {
                        self.closure_capture_writebacks.remove(&root);
                    } else {
                        self.closure_capture_writebacks.insert(root, writebacks);
                    }
                } else if let Rvalue::Use(Operand::Place(source) | Operand::MovePlace(source))
                | Rvalue::CallableAdapt {
                    value: Operand::Place(source) | Operand::MovePlace(source),
                    ..
                } = value
                {
                    let source_root = source.split('.').next().unwrap_or(source);
                    let target_root = target.split('.').next().unwrap_or(target).to_string();
                    match self.closure_capture_writebacks.get(source_root).cloned() {
                        Some(writebacks) => {
                            self.closure_capture_writebacks
                                .insert(target_root, writebacks);
                        }
                        None => {
                            self.closure_capture_writebacks.remove(&target_root);
                        }
                    }
                }
                self.mark_emptied_store_root(target);
                if let Rvalue::Try {
                    value: try_value,
                    releases_on_error,
                    from,
                } = value
                {
                    let target_ty = self.type_of_place(target)?;
                    self.compile_try_assign(
                        target,
                        target_ty,
                        try_value,
                        releases_on_error,
                        from.as_deref(),
                    )?;
                } else if self.lent_places.contains(target.as_str())
                    && !self.lookup_slots.contains(target.as_str())
                {
                    self.compile_lent_assign(target, value)?;
                } else if self.lookup_slots.contains(target.as_str()) {
                    self.compile_lookup_slot_assign(target, value)?;
                } else if let Some(scalar) = self.compile_scalar_variant_payload(target, value)? {
                    self.store_place(target, scalar)?;
                } else {
                    let target_ty = self.type_of_place(target)?;
                    let compiled = self.compile_rvalue_for_target(value, &target_ty)?;
                    let assignment_span = match value {
                        Rvalue::Unary { span, .. }
                        | Rvalue::Cast { span, .. }
                        | Rvalue::Binary { span, .. } => Some(*span),
                        _ => None,
                    };
                    let coerced = self.coerce_value_at(compiled, &target_ty, assignment_span)?;
                    self.store_place(target, coerced)?;
                }
            }
            Instruction::Eval { value } => {
                let _ = self.load_operand(value)?;
            }
            Instruction::PushCleanup { place, .. } => {
                self.set_cleanup_active(place, true)?;
                self.register_cleanup_for_place(place)?;
            }
            Instruction::PopCleanup {
                place,
                cancel_before_cleanup,
                run,
            } => {
                self.unregister_cleanup_for_place(place)?;
                self.set_cleanup_active(place, false)?;
                if *run {
                    self.emit_cleanup_for_place(place, *cancel_before_cleanup)?;
                }
            }
        }
        Ok(())
    }

    /// Notes that the instruction assigns `target`, a tracked root the MIR
    /// left empty, so its store releases nothing.
    fn mark_emptied_store_root(&mut self, target: &str) {
        if !target.contains('.') && self.tracked_roots.contains(target) {
            self.emptied_store_root = Some(target.to_string());
        }
    }

    fn compile_terminator(
        &mut self,
        block_label: &str,
        terminator: &Terminator,
        return_ty: &DirectType,
    ) -> std::result::Result<(), String> {
        match terminator {
            Terminator::Return(operand) => {
                let value = self.load_operand_for_target(operand, return_ty)?;
                self.release_unconsumed_retains();
                let coerced = self.coerce_value(value, return_ty)?;
                // A return releases the root it returns from, unless the
                // root moved out whole or goes back to the caller.
                let returned_root = match operand {
                    Operand::Place(place) => Some(place.as_str()),
                    Operand::MovePlace(place) if place.contains('.') => Some(place.as_str()),
                    _ => None,
                }
                .map(|place| place.split('.').next().unwrap_or(place))
                .filter(|root| {
                    self.tracked_roots.contains(*root)
                        && !self
                            .writeback_locals
                            .iter()
                            .any(|(writeback, _)| writeback == root)
                })
                .map(str::to_string);
                self.emit_return_value(coerced, returned_root.as_slice())?;
            }
            Terminator::Goto(label) => {
                self.release_all_temporary_owned();
                let block = self.compiled_block(label)?;
                self.builder.jump(block, &[]);
            }
            Terminator::Branch {
                condition,
                then_label,
                else_label,
            } => {
                let condition = self.load_operand(condition)?;
                let condition = self.as_bool_value(condition)?;
                let then_block = self.compiled_block(then_label)?;
                let else_block = self.compiled_block(else_label)?;
                self.release_all_temporary_owned();
                self.builder
                    .brif(condition, then_block, &[], else_block, &[]);
            }
            Terminator::Match {
                scrutinee,
                arms,
                otherwise,
            } => {
                let lookup_slot = matches!(
                    scrutinee,
                    Operand::Place(place) if self.lookup_slots.contains(place.as_str())
                );
                let lent = matches!(
                    scrutinee,
                    Operand::Place(place) if self.lent_places.contains(place.as_str())
                );
                let scrutinee = self.load_operand(scrutinee)?;
                let DirectType::Opaque(scrutinee_ty) = &scrutinee.ty else {
                    return Err(
                        "direct backend expected enum matches to use opaque scrutinees".to_string(),
                    );
                };
                let scrutinee_enum_name = match scrutinee_ty {
                    Type::Named(name, _) => name.as_str(),
                    other => {
                        return Err(format!(
                            "direct backend expected match scrutinee to carry an enum type name, found `{}`",
                            other
                        ))
                    }
                };
                for arm in arms {
                    if arm.wildcard {
                        self.release_all_temporary_owned();
                        let arm_block = self.compiled_block(&arm.label)?;
                        self.builder.jump(arm_block, &[]);
                        return Ok(());
                    }
                    let next_block = self.builder.create_block();
                    let matched = if lookup_slot {
                        // A lookup slot is null exactly when the key was
                        // missing.
                        let found = arm.variant_name.as_deref() == Some("Found");
                        let cc = if found { IntCC::NotEqual } else { IntCC::Equal };
                        self.builder.icmp_imm(cc, scrutinee.values[0], 0)
                    } else {
                        let test = if lent {
                            RuntimeFn::LentVariantMatches
                        } else {
                            RuntimeFn::VariantMatches
                        };
                        self.variant_matches_with(
                            test,
                            scrutinee.values[0],
                            arm.enum_name.as_deref().unwrap_or(scrutinee_enum_name),
                            arm.variant_name.as_deref().unwrap_or_default(),
                        )?
                    };
                    let arm_block = self.compiled_block(&arm.label)?;
                    let matched_cleanup = self.builder.create_block();
                    let pending_owned = self.layout_scratch.clone();
                    self.builder
                        .brif(matched, matched_cleanup, &[], next_block, &[]);
                    self.builder.switch_to_block(matched_cleanup);
                    self.release_all_temporary_owned();
                    self.builder.jump(arm_block, &[]);
                    self.builder.seal_block(matched_cleanup);
                    self.builder.switch_to_block(next_block);
                    self.layout_scratch = pending_owned;
                }
                self.release_all_temporary_owned();
                if self.closed_matches.contains(block_label) {
                    self.builder.trap(TrapCode::User(2));
                } else {
                    let otherwise = self.compiled_block(otherwise)?;
                    self.builder.jump(otherwise, &[]);
                }
            }
            Terminator::ForRange {
                binding,
                iterable,
                body_label,
                exit_label,
            } => {
                self.compile_for_range(binding, iterable, body_label, exit_label)?;
                self.release_all_temporary_owned();
            }
            Terminator::AssertFail {
                message,
                captures,
                span,
            } => {
                let message = match message {
                    Some(message) => {
                        let message = self.load_operand(message)?;
                        match &message.ty {
                            DirectType::Opaque(Type::Named(name, args))
                                if name == "str" && args.is_empty() => {}
                            other => {
                                return Err(format!(
                                    "direct backend expected an assertion message to be `str`, found `{}`",
                                    render_direct_type(other)
                                ))
                            }
                        }
                        message.values[0]
                    }
                    None => self.builder.iconst(types::I64, 0),
                };
                let (line, column) = self.span_values(Some(*span));
                match captures.as_slice() {
                    [] => {
                        self.builder
                            .call(self.runtime[RuntimeFn::AssertFail], &[message, line, column]);
                    }
                    [left, right] => {
                        let left_label = self.string_value(&left.label)?.values[0];
                        let left_type = self.string_value(&left.ty.to_string())?.values[0];
                        let left_value = self.load_assertion_capture_value(&left.value)?;
                        let right_label = self.string_value(&right.label)?.values[0];
                        let right_type = self.string_value(&right.ty.to_string())?.values[0];
                        let right_value = self.load_assertion_capture_value(&right.value)?;
                        self.builder.call(
                            self.runtime[RuntimeFn::AssertFailDetailed],
                            &[
                                message,
                                line,
                                column,
                                left_label,
                                left_type,
                                left_value,
                                right_label,
                                right_type,
                                right_value,
                            ],
                        );
                    }
                    _ => {
                        return Err(
                            "direct backend requires exactly two assertion captures when captures are present"
                                .to_string(),
                        )
                    }
                }
                self.builder.trap(TrapCode::User(1));
                // The trap unwinds the task, whose ledger reclaims what the
                // assertion's operands held.
                self.layout_scratch.clear();
            }
            // The checker proved this path cannot run (for example the
            // impossible arm of a narrowed module-constant read).
            Terminator::Unreachable => {
                self.builder.trap(TrapCode::User(2));
            }
        }
        Ok(())
    }

    fn load_assertion_capture_value(
        &mut self,
        operand: &Operand,
    ) -> std::result::Result<Val, String> {
        let value = self.load_operand(operand)?;
        match &value.ty {
            DirectType::Opaque(Type::Named(name, args)) if name == "str" && args.is_empty() => {
                Ok(value.values[0])
            }
            other => Err(format!(
                "direct backend expected a rendered assertion capture to be `str`, found `{}`",
                render_direct_type(other)
            )),
        }
    }

    /// Returns `value` to the caller with the `mut` parameters' values, after
    /// the pending cleanups. The MIR released the other tracked roots
    /// before the exit, except `releases`, which go after the cleanups.
    fn emit_return_value(
        &mut self,
        value: ValueRef,
        releases: &[String],
    ) -> std::result::Result<(), String> {
        let mut return_values = self.transfer_values(&value);
        self.emit_pending_cleanups_for(true, false)?;
        self.append_writeback_return_values(&mut return_values)?;
        self.release_all_temporary_owned();
        for root in releases {
            self.release_root_words(root)?;
        }
        self.release_boxed_copy_roots()?;
        emit_frame_pop(self.builder, &self.runtime);
        self.builder.return_(&return_values);
        Ok(())
    }

    fn compile_rvalue_for_target(
        &mut self,
        rvalue: &Rvalue,
        target: &DirectType,
    ) -> std::result::Result<ValueRef, String> {
        match rvalue {
            Rvalue::Use(operand) | Rvalue::CallableAdapt { value: operand, .. } => {
                let integer_hint = target.scalar_kind().filter(|kind| kind.is_integer());
                self.load_operand_with_integer_hint(operand, integer_hint)
            }
            Rvalue::NoneTest { value } => {
                // Lowering tests a union's `None` member through
                // `UnionTagTest`; this dynamic form only reaches runtime values.
                let loaded = self.load_operand(value)?;
                let loaded = self.ensure_opaque(loaded)?;
                let call = self
                    .builder
                    .call(self.runtime[RuntimeFn::NoneTest], &[loaded.values[0]]);
                Ok(ValueRef {
                    values: call.results,
                    ty: DirectType::Scalar(ScalarKind::Bool),
                })
            }
            Rvalue::ModuleConstant { key, initializer } => {
                let thunk = *self.function_thunk_refs.get(initializer).ok_or_else(|| {
                    format!(
                        "direct backend cannot find module constant initializer `{initializer}`"
                    )
                })?;
                let thunk_ptr = self.builder.func_addr(types::I64, thunk);
                let (key_ptr, key_len) = self.string_constant(key.as_bytes())?;
                let call = self.builder.call(
                    self.runtime[RuntimeFn::ModuleConstant],
                    &[key_ptr, key_len, thunk_ptr],
                );
                let value = self.owned_opaque_result(call.results, Type::named("Unknown"));
                self.coerce_value(value, target)
            }
            Rvalue::Closure {
                function,
                signature,
                captures,
                consuming,
                mutable: _,
            } => self.compile_closure(function, signature, captures, *consuming),
            Rvalue::FormatString { parts } => self.compile_format_string(parts),
            Rvalue::Unary {
                op,
                value,
                span,
                trap,
            } => {
                let integer_hint = target.scalar_kind().filter(|kind| kind.is_integer());
                if matches!(op, UnaryOp::Neg)
                    && matches!(integer_hint, Some(ScalarKind::Int64))
                    && matches!(value, Operand::Int(magnitude) if *magnitude == (i64::MAX as u128) + 1)
                {
                    return Ok(ValueRef {
                        values: vec![self.builder.iconst(types::I64, i64::MIN)],
                        ty: DirectType::Scalar(ScalarKind::Int64),
                    });
                }
                let value = self.load_operand_with_integer_hint(value, integer_hint)?;
                self.compile_unary(*op, value, Some(*span), *trap)
            }
            Rvalue::Cast { value, ty, span } => {
                let cast_target = ensure_direct_type(ty, &self.classes, "cast target")?;
                let value = self.load_operand_for_target(value, &cast_target)?;
                self.compile_cast(value, ty, &cast_target, Some(*span))
            }
            Rvalue::Binary {
                op,
                left,
                right,
                span,
                trap,
            } => {
                let target_integer_hint = target.scalar_kind().filter(|kind| kind.is_integer());
                let left_integer_hint = target_integer_hint
                    .or_else(|| self.operand_integer_kind(left))
                    .or_else(|| {
                        matches!(left, Operand::Int(_))
                            .then(|| self.operand_integer_kind(right))
                            .flatten()
                    });
                let right_integer_hint = target_integer_hint
                    .or_else(|| self.operand_integer_kind(right))
                    .or_else(|| {
                        matches!(right, Operand::Int(_))
                            .then(|| self.operand_integer_kind(left))
                            .flatten()
                    });
                if is_comparison_op(*op) {
                    if let Some(result) =
                        self.compile_borrowed_element_comparison(*op, left, right, *span)?
                    {
                        return Ok(result);
                    }
                    if let Some(result) =
                        self.compile_str_literal_comparison(*op, left, right, *span)?
                    {
                        return Ok(result);
                    }
                }
                let left = self.load_operand_with_integer_hint(left, left_integer_hint)?;
                let right = self.load_operand_with_integer_hint(right, right_integer_hint)?;
                self.compile_binary(*op, left, right, Some(*span), *trap)
            }
            Rvalue::Call {
                callee,
                args,
                type_args,
            } => self.compile_call(callee, args, type_args, target),
            Rvalue::VecLiteral {
                elements,
                element_type,
            } => self.compile_vec_literal_for_target(elements, element_type, Some(target)),
            Rvalue::TupleLiteral {
                elements,
                element_types,
            } => {
                let tuple_type = match target {
                    DirectType::Opaque(Type::Tuple(target_elements)) => {
                        Type::Tuple(target_elements.clone())
                    }
                    _ => Type::Tuple(element_types.clone()),
                };
                self.compile_tuple_literal(elements, element_types, tuple_type)
            }
            Rvalue::TupleElement {
                tuple,
                index,
                element_type,
            } => self.compile_tuple_element(tuple, *index, element_type),
            Rvalue::TupleTakeElement {
                place,
                index,
                element_type,
            } => self.compile_take_tuple_element(place, *index, element_type),
            Rvalue::MapLiteral {
                entries,
                key_type,
                value_type,
            } => self.compile_map_literal_for_target(entries, key_type, value_type, Some(target)),
            Rvalue::SetLiteral {
                elements,
                element_type,
            } => self.compile_set_literal_for_target(elements, element_type, Some(target)),
            Rvalue::Construct { class_name, fields } => {
                self.compile_construct(class_name, fields, target)
            }
            Rvalue::Member { object, field } => {
                let object = self.load_operand(object)?;
                self.extract_field(object, field)
            }
            Rvalue::UnionTagTest {
                place,
                union_type,
                member_index,
            } => {
                self.require_union_plan(union_type)?;
                let value = self.load_operand(&Operand::Place(place.clone()))?;
                if let DirectType::Union(_) = &value.ty {
                    let matched =
                        self.builder
                            .icmp_imm(IntCC::Equal, value.values[0], *member_index as i64);
                    let matched = self.builder.uextend(types::I64, matched);
                    return Ok(ValueRef {
                        values: vec![matched],
                        ty: DirectType::Scalar(ScalarKind::Bool),
                    });
                }
                let value = self.ensure_opaque(value)?;
                let encoded = crate::native_runtime::canonical_runtime_type_name(union_type);
                let (ptr, len) = self.string_constant(encoded.as_bytes())?;
                let index = self.builder.iconst(types::I64, *member_index as i64);
                let call = self.builder.call(
                    self.runtime[RuntimeFn::UnionTagTest],
                    &[value.values[0], ptr, len, index],
                );
                Ok(ValueRef {
                    values: call.results,
                    ty: DirectType::Scalar(ScalarKind::Bool),
                })
            }
            Rvalue::UnionTakePayload {
                place,
                union_type,
                member_type,
                member_index,
            } => {
                self.require_union_plan(union_type)?;
                // A consuming take only reaches non-Copy payloads, which stay
                // runtime values; an inline union's members are all Copy.
                let value = self.load_operand(&Operand::Place(place.clone()))?;
                let value = self.ensure_opaque(value)?;
                let encoded = crate::native_runtime::canonical_runtime_type_name(union_type);
                let (ptr, len) = self.string_constant(encoded.as_bytes())?;
                let index = self.builder.iconst(types::I64, *member_index as i64);
                let call = self.builder.call(
                    self.runtime[RuntimeFn::UnionTakePayload],
                    &[value.values[0], ptr, len, index],
                );
                let payload = self.owned_opaque_result(call.results, member_type.clone());
                // The take moves the payload out and the MIR counts the
                // root as moved, so the emptied box goes now.
                if !place.contains('.') && self.tracked_roots.contains(place) {
                    self.release_root(place)?;
                }
                let member = ensure_direct_type(member_type, &self.classes, "union payload")?;
                self.coerce_value(payload, &member)
            }
            Rvalue::UnionInject {
                value,
                union_type,
                member_type,
                member_index,
            } => {
                self.require_union_plan(union_type)?;
                let member_target = ensure_direct_type(member_type, &self.classes, "union member")?;
                let loaded = self.load_operand_for_target(value, &member_target)?;
                let loaded = self.coerce_value(loaded, &member_target)?;
                if let DirectType::Union(union) =
                    ensure_direct_type(union_type, &self.classes, "union")?
                {
                    let words = self.union_words_from_member(&union, *member_index, loaded)?;
                    return Ok(ValueRef {
                        values: words,
                        ty: DirectType::Union(union),
                    });
                }
                let payload = self.ensure_opaque(loaded)?;
                let transferred = self.transfer_owned_opaque_value(&payload);
                let encoded = crate::native_runtime::canonical_runtime_type_name(union_type);
                let (ptr, len) = self.string_constant(encoded.as_bytes())?;
                let tag = self.builder.iconst(types::I64, *member_index as i64);
                let call = self.builder.call(
                    self.runtime[RuntimeFn::UnionInject],
                    &[ptr, len, tag, transferred],
                );
                Ok(self.owned_opaque_result(call.results, union_type.clone()))
            }
            Rvalue::EnumVariant {
                enum_name,
                variant_name,
                payloads,
            } => self.compile_enum_variant_for_target(
                enum_name,
                variant_name,
                payloads,
                Some(target),
            ),
            Rvalue::VariantPayload {
                scrutinee,
                variant_name: _,
                index,
            } => {
                if let Operand::Place(place) | Operand::MovePlace(place) = scrutinee {
                    if self.lookup_slots.contains(place.as_str()) {
                        let moved = matches!(scrutinee, Operand::MovePlace(_));
                        return self.compile_lookup_slot_payload(place, moved);
                    }
                }
                if let Operand::MovePlace(place) = scrutinee {
                    return self.compile_take_variant_payload(place, *index);
                }
                let scrutinee = self.load_operand(scrutinee)?;
                self.compile_variant_payload(scrutinee, *index)
            }
            Rvalue::StartTask {
                returns_handle,
                result_is_copy,
                stack_size,
                task_group,
                function,
                args,
                span,
            } => self.compile_start_task(TaskStart {
                mode: TaskStartMode {
                    returns_handle: *returns_handle,
                    result_is_copy: *result_is_copy,
                },
                stack_size: stack_size.as_ref(),
                task_group,
                function,
                args,
                spawn_span: *span,
                target,
            }),
            Rvalue::Try { .. } => unreachable!("try rvalues are handled before target lowering"),
        }
    }

    /// An inline callable: the shape's descriptor word and the captures'
    /// direct words, inline when they fit in three words and otherwise in
    /// one checked environment block (checkpoint Q15 A / Q16 A).
    fn compile_closure(
        &mut self,
        function: &str,
        signature: &Type,
        captures: &[crate::mir::MirClosureCapture],
        consuming: bool,
    ) -> std::result::Result<ValueRef, String> {
        let key = CallableShapeKey {
            function: function.to_string(),
            captures: captures.len(),
            consuming,
            contract: signature.to_string(),
        };
        let shape = self.callable_shapes.get(&key).cloned().ok_or(format!(
            "direct backend does not know a callable shape for `{function}` with {} captures",
            captures.len()
        ))?;
        let mut words = Vec::new();
        for (capture, capture_ty) in captures.iter().zip(&shape.capture_types) {
            let value = self.load_operand_for_target(&capture.value, capture_ty)?;
            let value = self.coerce_value(value, capture_ty)?;
            let transferred = self.transfer_values(&value);
            for (word, abi) in transferred.iter().zip(capture_ty.abi_types()) {
                words.push(if abi == types::F64 {
                    self.builder.bitcast(types::I64, *word)
                } else {
                    *word
                });
            }
        }
        let global = self.builder.import_data(shape.descriptor);
        let descriptor = self.builder.symbol_value(types::I64, global);
        let zero = self.builder.iconst(types::I64, 0);
        let mut values = vec![descriptor];
        if shape.inline() {
            values.extend(words.iter().copied());
            while values.len() < 1 + CALLABLE_ENVIRONMENT_WORDS {
                values.push(zero);
            }
        } else {
            let size = self.builder.iconst(types::I64, shape.env_words as i64);
            let call = self
                .builder
                .call(self.runtime[RuntimeFn::CallableEnvAlloc], &[size]);
            let block = call.results[0];
            for (index, word) in words.iter().enumerate() {
                self.builder
                    .store(MemFlags::new(), *word, block, (index as i32) * 8);
            }
            values.push(block);
            values.push(zero);
            values.push(zero);
        }
        let value = ValueRef {
            values,
            ty: DirectType::Callable(DirectCallableType::new(signature.clone())),
        };
        self.mark_temporary_callable_owned(&value);
        Ok(value)
    }

    fn compile_vec_literal_for_target(
        &mut self,
        elements: &[Operand],
        element_type: &Type,
        target: Option<&DirectType>,
    ) -> std::result::Result<ValueRef, String> {
        let resolved_element_type = match target {
            Some(DirectType::Opaque(Type::Named(name, args)))
                if name == "list" && args.len() == 1 =>
            {
                args[0].clone()
            }
            _ => element_type.clone(),
        };
        let element_direct_ty =
            ensure_direct_type(&resolved_element_type, &self.classes, "Vec element")?;
        let init = self.builder.call(self.runtime[RuntimeFn::VecEmpty], &[]);
        let vector = self.owned_opaque_result(
            init.results,
            Type::Named("list".to_string(), vec![resolved_element_type]),
        );
        for element in elements {
            let value = self.load_operand_as_opaque_direct(element, &element_direct_ty)?;
            let value = self.transfer_owned_opaque_value(&value);
            let result = self.builder.call(
                self.runtime[RuntimeFn::VecPushInPlace],
                &[vector.values[0], value],
            );
            self.release_opaque_handle(result.results[0]);
        }
        Ok(vector)
    }

    fn compile_map_literal_for_target(
        &mut self,
        entries: &[MirMapEntry],
        key_type: &Type,
        value_type: &Type,
        target: Option<&DirectType>,
    ) -> std::result::Result<ValueRef, String> {
        let (resolved_key_type, resolved_value_type) = match target {
            Some(DirectType::Opaque(Type::Named(name, args)))
                if name == "dict" && args.len() == 2 =>
            {
                (args[0].clone(), args[1].clone())
            }
            _ => (key_type.clone(), value_type.clone()),
        };
        let key_direct_ty = ensure_direct_type(&resolved_key_type, &self.classes, "dict key")?;
        let value_direct_ty = ensure_direct_type(&resolved_value_type, &self.classes, "Map value")?;
        let init = self.builder.call(self.runtime[RuntimeFn::MapEmpty], &[]);
        let map = self.owned_opaque_result(
            init.results,
            Type::Named(
                "dict".to_string(),
                vec![resolved_key_type, resolved_value_type],
            ),
        );
        for entry in entries {
            let key = self.load_operand_as_opaque_direct(&entry.key, &key_direct_ty)?;
            let value = self.load_operand_as_opaque_direct(&entry.value, &value_direct_ty)?;
            let key = self.transfer_owned_opaque_value(&key);
            let value = self.transfer_owned_opaque_value(&value);
            let result = self.builder.call(
                self.runtime[RuntimeFn::MapSetInPlace],
                &[map.values[0], key, value],
            );
            self.release_opaque_handle(result.results[0]);
        }
        Ok(map)
    }

    fn compile_set_literal_for_target(
        &mut self,
        elements: &[Operand],
        element_type: &Type,
        target: Option<&DirectType>,
    ) -> std::result::Result<ValueRef, String> {
        let resolved_element_type = match target {
            Some(DirectType::Opaque(Type::Named(name, args)))
                if name == "set" && args.len() == 1 =>
            {
                args[0].clone()
            }
            _ => element_type.clone(),
        };
        let element_direct_ty =
            ensure_direct_type(&resolved_element_type, &self.classes, "set element")?;
        let init = self.builder.call(self.runtime[RuntimeFn::SetEmpty], &[]);
        let set = self.owned_opaque_result(
            init.results,
            Type::Named("set".to_string(), vec![resolved_element_type]),
        );
        for element in elements {
            let value = self.load_operand_as_opaque_direct(element, &element_direct_ty)?;
            let value = self.transfer_owned_opaque_value(&value);
            let _ = self.builder.call(
                self.runtime[RuntimeFn::SetInsertInPlace],
                &[set.values[0], value],
            );
        }
        Ok(set)
    }

    /// Lowers `op value` with the check `trap` names. A boxed operand goes
    /// to the runtime helper, which checks it. An `int32` value is checked
    /// where it is stored, since it lives in a 64-bit word.
    fn compile_unary(
        &mut self,
        op: UnaryOp,
        value: ValueRef,
        span: Option<Span>,
        trap: ArithmeticTrap,
    ) -> std::result::Result<ValueRef, String> {
        if matches!(value.ty, DirectType::Opaque(_)) || op == UnaryOp::BitNot {
            let target_ty = value.ty.clone();
            let value = self.ensure_opaque(value)?;
            let opcode = match op {
                UnaryOp::Neg => 0,
                UnaryOp::Not => 1,
                UnaryOp::BitNot => 2,
            };
            let opcode = self.builder.iconst(types::I64, opcode);
            let (line, column) = self.span_values(span);
            let inst = self.builder.call(
                self.runtime[RuntimeFn::UnaryValue],
                &[opcode, value.values[0], line, column],
            );
            let result = self.owned_opaque_result(inst.results, Type::named("Unknown"));
            return if matches!(target_ty, DirectType::Opaque(_)) {
                Ok(result)
            } else {
                self.coerce_value(result, &target_ty)
            };
        }
        match (op, value.ty.scalar_kind()) {
            (UnaryOp::Neg, Some(ScalarKind::Int32)) => Ok(ValueRef {
                values: vec![self.builder.ineg(value.values[0])],
                ty: DirectType::Scalar(ScalarKind::Int32),
            }),
            (UnaryOp::Neg, Some(ScalarKind::Int64)) => self.compile_wide_integer_negation(
                WideIntegerKind::Int64,
                value.values[0],
                span,
                trap,
            ),
            (UnaryOp::Neg, Some(ScalarKind::Uint64)) => self.compile_wide_integer_negation(
                WideIntegerKind::Uint64,
                value.values[0],
                span,
                trap,
            ),
            (UnaryOp::Neg, Some(kind)) if kind.is_float() => Ok(ValueRef {
                values: vec![self.builder.fneg(value.values[0])],
                ty: DirectType::Scalar(kind),
            }),
            (UnaryOp::Not, Some(ScalarKind::Bool)) => {
                let zero = self.builder.iconst(types::I64, 0);
                let cmp = self.builder.icmp(IntCC::Equal, value.values[0], zero);
                Ok(ValueRef {
                    values: vec![self.builder.uextend(types::I64, cmp)],
                    ty: DirectType::Scalar(ScalarKind::Bool),
                })
            }
            _ => Err(format!(
                "direct backend does not support unary operation `{:?}` for the current operand type",
                op
            )),
        }
    }

    fn compile_wide_integer_negation(
        &mut self,
        kind: WideIntegerKind,
        value: Val,
        span: Option<Span>,
        trap: ArithmeticTrap,
    ) -> std::result::Result<ValueRef, String> {
        let zero = self.builder.iconst(types::I64, 0);
        let result = self.compile_wide_integer_overflowing(
            kind,
            WideOverflowOp::Sub,
            zero,
            value,
            span,
            trap,
        )?;
        Ok(ValueRef {
            values: vec![result],
            ty: DirectType::Scalar(kind.scalar_kind()),
        })
    }

    /// `left op right` for `+`, `-`, or `*` on a 64-bit integer, with the
    /// overflow check when `trap` names one.
    fn compile_wide_integer_overflowing(
        &mut self,
        kind: WideIntegerKind,
        op: WideOverflowOp,
        left: Val,
        right: Val,
        span: Option<Span>,
        trap: ArithmeticTrap,
    ) -> std::result::Result<Val, String> {
        if !trap.checks_overflow() {
            return Ok(match op {
                WideOverflowOp::Add => self.builder.iadd(left, right),
                WideOverflowOp::Sub => self.builder.isub(left, right),
                // `Div` names only a floor division's failure.
                WideOverflowOp::Mul | WideOverflowOp::Div => self.builder.imul(left, right),
            });
        }
        let (result, overflow) = match (op, kind.is_signed()) {
            (WideOverflowOp::Add, true) => self.builder.sadd_overflow(left, right),
            (WideOverflowOp::Add, false) => self.builder.uadd_overflow(left, right),
            (WideOverflowOp::Sub, true) => self.builder.ssub_overflow(left, right),
            (WideOverflowOp::Sub, false) => self.builder.usub_overflow(left, right),
            (WideOverflowOp::Mul | WideOverflowOp::Div, true) => {
                self.builder.smul_overflow(left, right)
            }
            (WideOverflowOp::Mul | WideOverflowOp::Div, false) => {
                self.builder.umul_overflow(left, right)
            }
        };
        self.emit_integer_overflow_failure_branch(overflow, kind, op, left, right, span)?;
        Ok(result)
    }

    fn compile_cast(
        &mut self,
        value: ValueRef,
        target: &Type,
        target_ty: &DirectType,
        span: Option<Span>,
    ) -> std::result::Result<ValueRef, String> {
        if matches!(value.ty, DirectType::Opaque(_)) || matches!(target_ty, DirectType::Opaque(_)) {
            let boxed = self.ensure_opaque(value)?;
            let (target_ptr, target_len) = self.string_constant(target.to_string().as_bytes())?;
            let (line, column) = self.span_values(span);
            let inst = self.builder.call(
                self.runtime[RuntimeFn::CastValue],
                &[boxed.values[0], target_ptr, target_len, line, column],
            );
            let boxed = self.owned_opaque_result(inst.results, target.clone());
            return self.coerce_value(boxed, target_ty);
        }
        let Some(target_kind) = target_ty.scalar_kind() else {
            return Err(format!(
                "direct backend only supports numeric casts, found target `{}`",
                target
            ));
        };
        let Some(source_kind) = value.ty.scalar_kind() else {
            return Err(format!(
                "direct backend only supports numeric casts from scalar values, found `{}`",
                render_direct_type(&value.ty)
            ));
        };

        let source = value.values[0];
        let result = match (source_kind, target_kind) {
            (_, _) if source_kind == target_kind => source,
            (source_kind, target_kind) if source_kind.is_integer() && target_kind.is_integer() => {
                let source_code = self.builder.iconst(
                    types::I64,
                    if matches!(source_kind, ScalarKind::Uint64) {
                        1
                    } else {
                        0
                    },
                );
                let target_code = self.builder.iconst(
                    types::I64,
                    match target_kind {
                        ScalarKind::Int32 => 0,
                        ScalarKind::Int64 => 1,
                        ScalarKind::Uint64 => 2,
                        _ => unreachable!("integer cast target should be exhaustive"),
                    },
                );
                let (line, column) = self.span_values(span);
                let inst = self.builder.call(
                    self.runtime[RuntimeFn::CastIntegerToInteger],
                    &[source, source_code, target_code, line, column],
                );
                inst.results[0]
            }
            (source_kind, target_kind) if source_kind.is_integer() && target_kind.is_float() => {
                let source_code = self.builder.iconst(
                    types::I64,
                    if matches!(source_kind, ScalarKind::Uint64) {
                        1
                    } else {
                        0
                    },
                );
                let target_code = self.builder.iconst(
                    types::I64,
                    if matches!(target_kind, ScalarKind::Float32) {
                        0
                    } else {
                        1
                    },
                );
                let (line, column) = self.span_values(span);
                let inst = self.builder.call(
                    self.runtime[RuntimeFn::CastIntegerToFloat],
                    &[source, source_code, target_code, line, column],
                );
                inst.results[0]
            }
            (source_kind, target_kind) if source_kind.is_float() && target_kind.is_integer() => {
                let target_code = self.builder.iconst(
                    types::I64,
                    match target_kind {
                        ScalarKind::Int32 => 0,
                        ScalarKind::Int64 => 1,
                        ScalarKind::Uint64 => 2,
                        _ => unreachable!("integer cast target should be exhaustive"),
                    },
                );
                let (line, column) = self.span_values(span);
                let inst = self.builder.call(
                    self.runtime[RuntimeFn::CastFloatToInteger],
                    &[source, target_code, line, column],
                );
                inst.results[0]
            }
            (lhs, rhs) if lhs.is_float() && rhs.is_float() => source,
            _ => {
                return Err(format!(
                    "direct backend only supports numeric casts, found `{}` to `{}`",
                    render_direct_type(&value.ty),
                    target
                ));
            }
        };

        Ok(ValueRef {
            values: vec![result],
            ty: DirectType::Scalar(target_kind),
        })
    }

    /// Equality of two inline unions of one type: the tags must agree and
    /// the active members must compare equal under their own rule; `None`
    /// equals `None`.
    fn compile_inline_union_equality(
        &mut self,
        left: ValueRef,
        right: ValueRef,
        op: BinaryOp,
        span: Option<Span>,
    ) -> std::result::Result<ValueRef, String> {
        let DirectType::Union(union) = left.ty.clone() else {
            unreachable!("caller checked an inline union");
        };
        let same_tag = self
            .builder
            .icmp(IntCC::Equal, left.values[0], right.values[0]);
        let bool_ty = DirectType::Scalar(ScalarKind::Bool);
        let right_words = right.values.clone();
        // The members are compared only when the tags agree: with different
        // tags the other side's payload words belong to another member (a
        // runtime-object member's word would be a null or foreign handle).
        let compare_block = self.builder.create_block();
        let differ_block = self.builder.create_block();
        let merge_block = self.builder.create_block();
        self.builder.append_block_param(merge_block, types::I64);
        self.builder
            .brif(same_tag, compare_block, &[], differ_block, &[]);
        self.builder.switch_to_block(differ_block);
        self.builder.seal_block(differ_block);
        let zero = self.builder.iconst(types::I64, 0);
        self.builder.jump(merge_block, &[zero]);
        self.builder.switch_to_block(compare_block);
        self.builder.seal_block(compare_block);
        let members_equal =
            self.switch_on_union_tag(&union, &left.values, &bool_ty, |codegen, index, member| {
                if matches!(member.ty, DirectType::Scalar(ScalarKind::Unit)) {
                    return Ok(ValueRef {
                        values: vec![codegen.builder.iconst(types::I64, 1)],
                        ty: DirectType::Scalar(ScalarKind::Bool),
                    });
                }
                let other = codegen.union_member_from_words(&union, index, &right_words)?;
                if matches!(member.ty, DirectType::Scalar(_)) {
                    return codegen.compile_binary(
                        BinaryOp::Eq,
                        member,
                        other,
                        span,
                        ArithmeticTrap::None,
                    );
                }
                // A plain-class member compares under the runtime's shared
                // structural rule, exactly as the interpreter compares it.
                let member = codegen.ensure_opaque(member)?;
                let other = codegen.ensure_opaque(other)?;
                codegen.compile_binary(BinaryOp::Eq, member, other, span, ArithmeticTrap::None)
            })?;
        self.builder.jump(merge_block, &[members_equal.values[0]]);
        self.builder.switch_to_block(merge_block);
        self.builder.seal_block(merge_block);
        let equal = self.builder.block_params(merge_block)[0];
        let result = if op == BinaryOp::Eq {
            equal
        } else {
            self.builder.bxor_imm(equal, 1)
        };
        Ok(ValueRef {
            values: vec![result],
            ty: bool_ty,
        })
    }

    /// Compares a lent list element where it lies (see
    /// `borrowed_list_element`). Returns `None` when neither operand is one.
    fn compile_borrowed_element_comparison(
        &mut self,
        op: BinaryOp,
        left: &Operand,
        right: &Operand,
        span: Span,
    ) -> std::result::Result<Option<ValueRef>, String> {
        let (selection, other, element_on_left) =
            if let Some(selection) = self.borrowed_list_element(left) {
                (selection, right, 1)
            } else if let Some(selection) = self.borrowed_list_element(right) {
                (selection, left, 0)
            } else {
                return Ok(None);
            };
        let opcode = self.builder.iconst(types::I64, Self::binary_opcode(op));
        let (line, column) = self.span_values(Some(span));
        let inst = match other {
            Operand::String(literal) => {
                let (list, index) = self.load_borrowed_list_element(&selection)?;
                let (literal_ptr, literal_len) = self.string_constant(literal.as_bytes())?;
                let literal_on_left = self.builder.iconst(types::I64, 1 - element_on_left);
                let slow_args = [
                    opcode,
                    list,
                    index,
                    literal_ptr,
                    literal_len,
                    literal_on_left,
                    line,
                    column,
                ];
                if matches!(op, BinaryOp::Eq | BinaryOp::NotEq)
                    && selection.element_type == Type::named("str")
                {
                    let equal = self.emit_str_view_equality(
                        list,
                        index,
                        (literal_ptr, literal.len()),
                        &slow_args,
                    );
                    let result = if op == BinaryOp::Eq {
                        equal
                    } else {
                        self.builder.bxor_imm(equal, 1)
                    };
                    return Ok(Some(ValueRef {
                        values: vec![result],
                        ty: DirectType::Scalar(ScalarKind::Bool),
                    }));
                }
                self.builder
                    .call(self.runtime[RuntimeFn::CompareStrLiteralAt], &slow_args)
            }
            other => {
                let other = self.load_operand(other)?;
                let other = self.ensure_opaque(other)?;
                let (list, index) = self.load_borrowed_list_element(&selection)?;
                let element_on_left = self.builder.iconst(types::I64, element_on_left);
                self.builder.call(
                    self.runtime[RuntimeFn::CompareValuesAt],
                    &[
                        opcode,
                        list,
                        index,
                        other.values[0],
                        element_on_left,
                        line,
                        column,
                    ],
                )
            }
        };
        Ok(Some(ValueRef {
            values: vec![inst.results[0]],
            ty: DirectType::Scalar(ScalarKind::Bool),
        }))
    }

    /// Compares a `str` value with a string literal without boxing the
    /// literal. Returns `None` when neither operand fits that shape.
    fn compile_str_literal_comparison(
        &mut self,
        op: BinaryOp,
        left: &Operand,
        right: &Operand,
        span: Span,
    ) -> std::result::Result<Option<ValueRef>, String> {
        let (value, literal, literal_on_left) = match (left, right) {
            (Operand::String(_), Operand::String(_)) => return Ok(None),
            (value, Operand::String(literal)) => (value, literal, 0),
            (Operand::String(literal), value) => (value, literal, 1),
            _ => return Ok(None),
        };
        let loaded = self.load_operand(value)?;
        if !matches!(&loaded.ty, DirectType::Opaque(ty) if *ty == Type::named("str")) {
            let literal_value = self.load_operand(&Operand::String(literal.clone()))?;
            let (left, right) = if literal_on_left == 1 {
                (literal_value, loaded)
            } else {
                (loaded, literal_value)
            };
            // A comparison makes no check.
            return self
                .compile_binary(op, left, right, Some(span), ArithmeticTrap::None)
                .map(Some);
        }
        let (literal_ptr, literal_len) = self.string_constant(literal.as_bytes())?;
        let opcode = self.builder.iconst(types::I64, Self::binary_opcode(op));
        let literal_on_left = self.builder.iconst(types::I64, literal_on_left);
        let (line, column) = self.span_values(Some(span));
        let inst = self.builder.call(
            self.runtime[RuntimeFn::CompareStrLiteral],
            &[
                opcode,
                loaded.values[0],
                literal_ptr,
                literal_len,
                literal_on_left,
                line,
                column,
            ],
        );
        Ok(Some(ValueRef {
            values: vec![inst.results[0]],
            ty: DirectType::Scalar(ScalarKind::Bool),
        }))
    }

    /// Lowers `left op right` with the check `trap` names. A boxed operand,
    /// a power, a bitwise operator, and a shift go to a runtime helper,
    /// which checks them.
    fn compile_binary(
        &mut self,
        op: BinaryOp,
        left: ValueRef,
        right: ValueRef,
        span: Option<Span>,
        trap: ArithmeticTrap,
    ) -> std::result::Result<ValueRef, String> {
        if let (DirectType::Union(left_union), DirectType::Union(right_union)) =
            (&left.ty, &right.ty)
        {
            if left_union == right_union && matches!(op, BinaryOp::Eq | BinaryOp::NotEq) {
                // The checker injects the compared member into the union, so
                // `value == None` and `value == 5` arrive as two inline unions
                // of one type: equal tags and equal active members, without
                // a runtime box (ADR-0052 union equality).
                return self.compile_inline_union_equality(left, right, op, span);
            }
        }
        if matches!(left.ty, DirectType::Union(_)) || matches!(right.ty, DirectType::Union(_)) {
            let left = self.ensure_opaque(left)?;
            let right = self.ensure_opaque(right)?;
            return self.compile_binary(op, left, right, span, trap);
        }
        if matches!(
            op,
            BinaryOp::Pow
                | BinaryOp::BitAnd
                | BinaryOp::BitOr
                | BinaryOp::BitXor
                | BinaryOp::Shl
                | BinaryOp::Shr
        ) {
            let target_ty = left.ty.clone();
            let float_width = match &target_ty {
                DirectType::Scalar(ScalarKind::Float32) => 32,
                DirectType::Scalar(ScalarKind::Float64) => 64,
                _ => 0,
            };
            let left = self.ensure_opaque(left)?;
            let right = self.ensure_opaque(right)?;
            let opcode_value = Self::binary_opcode(op);
            let opcode = self.builder.iconst(types::I64, opcode_value);
            let float_width = self.builder.iconst(types::I64, float_width);
            let (line, column) = self.span_values(span);
            let inst = self.builder.call(
                self.runtime[RuntimeFn::BinaryValue],
                &[
                    opcode,
                    left.values[0],
                    right.values[0],
                    float_width,
                    line,
                    column,
                ],
            );
            let result = self.owned_opaque_result(inst.results, Type::named("Unknown"));
            return if matches!(target_ty, DirectType::Opaque(_)) {
                Ok(result)
            } else {
                self.coerce_value(result, &target_ty)
            };
        }
        let left_array_element = direct_array_element_type(&left.ty).cloned();
        let right_array_element = direct_array_element_type(&right.ty).cloned();
        if left_array_element.is_some() || right_array_element.is_some() {
            let array_ty = if left_array_element.is_some() {
                direct_type_to_type(&left.ty)
            } else {
                direct_type_to_type(&right.ty)
            };
            let element_type = left_array_element
                .as_ref()
                .or(right_array_element.as_ref())
                .expect("Array operation has an Array dtype");
            let element_direct =
                ensure_direct_type(element_type, &self.classes, "Array scalar operand")?;
            let scalar_left = i64::from(left_array_element.is_none());
            let left = if left_array_element.is_some() {
                left
            } else {
                self.coerce_value_at(left, &element_direct, span)?
            };
            let right = if right_array_element.is_some() {
                right
            } else {
                self.coerce_value_at(right, &element_direct, span)?
            };
            let left = self.ensure_opaque(left)?;
            let right = self.ensure_opaque(right)?;
            let scalar_left = self.builder.iconst(types::I64, scalar_left);
            let operation = self
                .builder
                .iconst(types::I64, direct_array_binary_opcode(op)?);
            let checked_mode = self.builder.iconst(types::I64, 0);
            let (line, column) = self.span_values(span);
            let inst = self.builder.call(
                self.runtime[RuntimeFn::ArrayBinary],
                &[
                    left.values[0],
                    right.values[0],
                    scalar_left,
                    operation,
                    checked_mode,
                    line,
                    column,
                ],
            );
            return Ok(self.owned_opaque_result(inst.results, array_ty));
        }
        if matches!(left.ty, DirectType::Opaque(_)) || matches!(right.ty, DirectType::Opaque(_)) {
            let left = self.ensure_opaque(left)?;
            let right = self.ensure_opaque(right)?;
            let binary_opcode = Self::binary_opcode(op);
            let opcode = self.builder.iconst(types::I64, binary_opcode);
            if is_comparison_op(op) {
                // The result is a machine `bool`, so no box is built for it.
                let (line, column) = self.span_values(span);
                let inst = self.builder.call(
                    self.runtime[RuntimeFn::CompareValues],
                    &[opcode, left.values[0], right.values[0], line, column],
                );
                return Ok(ValueRef {
                    values: vec![inst.results[0]],
                    ty: DirectType::Scalar(ScalarKind::Bool),
                });
            }
            let float_width = self.builder.iconst(types::I64, 0);
            let (line, column) = self.span_values(span);
            let inst = self.builder.call(
                self.runtime[RuntimeFn::BinaryValue],
                &[
                    opcode,
                    left.values[0],
                    right.values[0],
                    float_width,
                    line,
                    column,
                ],
            );
            return Ok(self.owned_opaque_result(inst.results, Type::named("Unknown")));
        }
        match (left.ty.scalar_kind(), right.ty.scalar_kind()) {
            (Some(ScalarKind::Int32), Some(ScalarKind::Int32)) => {
                self.compile_int32_binary(op, left.values[0], right.values[0], span, trap)
            }
            (Some(ScalarKind::Int64), Some(ScalarKind::Int64)) => self
                .compile_wide_integer_binary(
                    WideIntegerKind::Int64,
                    op,
                    left.values[0],
                    right.values[0],
                    span,
                    trap,
                ),
            (Some(ScalarKind::Uint64), Some(ScalarKind::Uint64)) => self
                .compile_wide_integer_binary(
                    WideIntegerKind::Uint64,
                    op,
                    left.values[0],
                    right.values[0],
                    span,
                    trap,
                ),
            (Some(lhs), Some(rhs)) if lhs.is_float() && rhs.is_float() => {
                self.compile_float_binary(op, left.values[0], right.values[0], lhs, span, trap)
            }
            (Some(ScalarKind::Bool), Some(ScalarKind::Bool)) => {
                self.compile_bool_binary(op, left.values[0], right.values[0])
            }
            (Some(ScalarKind::Unit), Some(ScalarKind::Unit))
                if matches!(op, BinaryOp::Eq | BinaryOp::NotEq) =>
            {
                self.compile_bool_binary(op, left.values[0], right.values[0])
            }
            _ => Err(format!(
                "direct backend does not support binary operation `{:?}` for the current operand types",
                op
            )),
        }
    }

    /// `int32` arithmetic. Every `int32` result is range-checked, since the
    /// value lives in a 64-bit word: that check is how `trap`'s overflow
    /// is raised. The divisor is checked when `trap` names it.
    fn compile_int32_binary(
        &mut self,
        op: BinaryOp,
        left: Val,
        right: Val,
        span: Option<Span>,
        trap: ArithmeticTrap,
    ) -> std::result::Result<ValueRef, String> {
        let ty = DirectType::Scalar(ScalarKind::Int32);
        let value = match op {
            BinaryOp::Add => ValueRef {
                values: vec![self.builder.iadd(left, right)],
                ty,
            },
            BinaryOp::Sub => ValueRef {
                values: vec![self.builder.isub(left, right)],
                ty,
            },
            BinaryOp::Mul => ValueRef {
                values: vec![self.builder.imul(left, right)],
                ty,
            },
            BinaryOp::FloorDiv => {
                self.emit_int_division_guard_for(trap, right, span)?;
                ValueRef {
                    values: vec![self.compile_signed_floor_divmod(left, right, true)],
                    ty,
                }
            }
            BinaryOp::Mod => {
                self.emit_int_division_guard_for(trap, right, span)?;
                ValueRef {
                    values: vec![self.compile_signed_floor_divmod(left, right, false)],
                    ty,
                }
            }
            BinaryOp::Eq => self.boolean_from_icmp(IntCC::Equal, left, right),
            BinaryOp::NotEq => self.boolean_from_icmp(IntCC::NotEqual, left, right),
            BinaryOp::Less => self.boolean_from_icmp(IntCC::SignedLessThan, left, right),
            BinaryOp::LessEq => self.boolean_from_icmp(IntCC::SignedLessThanOrEqual, left, right),
            BinaryOp::Greater => self.boolean_from_icmp(IntCC::SignedGreaterThan, left, right),
            BinaryOp::GreaterEq => {
                self.boolean_from_icmp(IntCC::SignedGreaterThanOrEqual, left, right)
            }
            other => {
                return Err(format!(
                    "direct backend does not support integer binary operation `{:?}`",
                    other
                ))
            }
        };

        if matches!(value.ty.scalar_kind(), Some(ScalarKind::Int32)) {
            self.emit_int32_bounds_check(value.values[0], span)?;
        }
        Ok(value)
    }

    fn compile_signed_floor_divmod(&mut self, left: Val, right: Val, quotient: bool) -> Val {
        let truncating_quotient = self.builder.sdiv(left, right);
        let truncating_remainder = self.builder.srem(left, right);
        let zero = self.builder.iconst(types::I64, 0);
        let remainder_nonzero = self
            .builder
            .icmp(IntCC::NotEqual, truncating_remainder, zero);
        let left_negative = self.builder.icmp(IntCC::SignedLessThan, left, zero);
        let right_negative = self.builder.icmp(IntCC::SignedLessThan, right, zero);
        let signs_differ = self.builder.bxor(left_negative, right_negative);
        let adjust = self.builder.band(remainder_nonzero, signs_differ);

        if quotient {
            let one = self.builder.iconst(types::I64, 1);
            let adjusted = self.builder.isub(truncating_quotient, one);
            self.builder.select(adjust, adjusted, truncating_quotient)
        } else {
            let adjusted = self.builder.iadd(truncating_remainder, right);
            self.builder.select(adjust, adjusted, truncating_remainder)
        }
    }

    fn compile_wide_integer_binary(
        &mut self,
        kind: WideIntegerKind,
        op: BinaryOp,
        left: Val,
        right: Val,
        span: Option<Span>,
        trap: ArithmeticTrap,
    ) -> std::result::Result<ValueRef, String> {
        let ty = DirectType::Scalar(kind.scalar_kind());
        let arithmetic = |value| ValueRef {
            values: vec![value],
            ty: ty.clone(),
        };
        let value = match op {
            BinaryOp::Add | BinaryOp::Sub | BinaryOp::Mul => {
                let overflow_op = match op {
                    BinaryOp::Add => WideOverflowOp::Add,
                    BinaryOp::Sub => WideOverflowOp::Sub,
                    _ => WideOverflowOp::Mul,
                };
                arithmetic(self.compile_wide_integer_overflowing(
                    kind,
                    overflow_op,
                    left,
                    right,
                    span,
                    trap,
                )?)
            }
            BinaryOp::FloorDiv => {
                self.emit_int_division_guard_for(trap, right, span)?;
                if kind.is_signed() {
                    if trap.checks_overflow() {
                        let min = self.builder.iconst(types::I64, i64::MIN);
                        let negative_one = self.builder.iconst(types::I64, -1);
                        let is_min = self.builder.icmp(IntCC::Equal, left, min);
                        let is_negative_one = self.builder.icmp(IntCC::Equal, right, negative_one);
                        let overflow = self.builder.band(is_min, is_negative_one);
                        self.emit_integer_overflow_failure_branch(
                            overflow,
                            kind,
                            WideOverflowOp::Div,
                            left,
                            right,
                            span,
                        )?;
                    }
                    arithmetic(self.compile_signed_floor_divmod(left, right, true))
                } else {
                    arithmetic(self.builder.udiv(left, right))
                }
            }
            BinaryOp::Mod => {
                self.emit_int_division_guard_for(trap, right, span)?;
                if kind.is_signed() {
                    let min = self.builder.iconst(types::I64, i64::MIN);
                    let negative_one = self.builder.iconst(types::I64, -1);
                    let is_min = self.builder.icmp(IntCC::Equal, left, min);
                    let is_negative_one = self.builder.icmp(IntCC::Equal, right, negative_one);
                    let exceptional = self.builder.band(is_min, is_negative_one);
                    let normal_block = self.builder.create_block();
                    let continue_block = self.builder.create_block();
                    self.builder.append_block_param(continue_block, types::I64);
                    let zero_remainder = self.builder.iconst(types::I64, 0);
                    self.builder.brif(
                        exceptional,
                        continue_block,
                        &[zero_remainder],
                        normal_block,
                        &[],
                    );
                    self.builder.switch_to_block(normal_block);
                    let remainder = self.compile_signed_floor_divmod(left, right, false);
                    self.builder.jump(continue_block, &[remainder]);
                    self.builder.seal_block(normal_block);
                    self.builder.switch_to_block(continue_block);
                    self.builder.seal_block(continue_block);
                    arithmetic(self.builder.block_params(continue_block)[0])
                } else {
                    arithmetic(self.builder.urem(left, right))
                }
            }
            BinaryOp::Eq => self.boolean_from_icmp(IntCC::Equal, left, right),
            BinaryOp::NotEq => self.boolean_from_icmp(IntCC::NotEqual, left, right),
            BinaryOp::Less => self.boolean_from_icmp(
                if kind.is_signed() {
                    IntCC::SignedLessThan
                } else {
                    IntCC::UnsignedLessThan
                },
                left,
                right,
            ),
            BinaryOp::LessEq => self.boolean_from_icmp(
                if kind.is_signed() {
                    IntCC::SignedLessThanOrEqual
                } else {
                    IntCC::UnsignedLessThanOrEqual
                },
                left,
                right,
            ),
            BinaryOp::Greater => self.boolean_from_icmp(
                if kind.is_signed() {
                    IntCC::SignedGreaterThan
                } else {
                    IntCC::UnsignedGreaterThan
                },
                left,
                right,
            ),
            BinaryOp::GreaterEq => self.boolean_from_icmp(
                if kind.is_signed() {
                    IntCC::SignedGreaterThanOrEqual
                } else {
                    IntCC::UnsignedGreaterThanOrEqual
                },
                left,
                right,
            ),
            other => return Err(format!("{WIDE_INTEGER_BINARY_ERROR} `{other:?}`")),
        };
        Ok(value)
    }

    fn compile_float_binary(
        &mut self,
        op: BinaryOp,
        left: Val,
        right: Val,
        kind: ScalarKind,
        span: Option<Span>,
        trap: ArithmeticTrap,
    ) -> std::result::Result<ValueRef, String> {
        let ty = DirectType::Scalar(kind);
        match op {
            BinaryOp::Add => Ok(ValueRef {
                values: vec![self.builder.fadd(left, right)],
                ty,
            }),
            BinaryOp::Sub => Ok(ValueRef {
                values: vec![self.builder.fsub(left, right)],
                ty,
            }),
            BinaryOp::Mul => Ok(ValueRef {
                values: vec![self.builder.fmul(left, right)],
                ty,
            }),
            BinaryOp::Div => {
                if trap.checks_divisor() {
                    self.emit_float_division_guard(right, span)?;
                }
                Ok(ValueRef {
                    values: vec![self.builder.fdiv(left, right)],
                    ty,
                })
            }
            BinaryOp::FloorDiv | BinaryOp::Mod => {
                let opcode_value = Self::binary_opcode(op);
                let left_box = self.builder.call(self.runtime[RuntimeFn::BoxF64], &[left]);
                let right_box = self.builder.call(self.runtime[RuntimeFn::BoxF64], &[right]);
                let left_boxed = left_box.results[0];
                let right_boxed = right_box.results[0];
                let opcode = self.builder.iconst(types::I64, opcode_value);
                let float_width = self.builder.iconst(types::I64, 0);
                let (line, column) = self.span_values(span);
                let result = self.builder.call(
                    self.runtime[RuntimeFn::BinaryValue],
                    &[opcode, left_boxed, right_boxed, float_width, line, column],
                );
                let result_boxed = result.results[0];
                self.release_opaque_handle(left_boxed);
                self.release_opaque_handle(right_boxed);
                let unboxed = self
                    .builder
                    .call(self.runtime[RuntimeFn::UnboxF64], &[result_boxed]);
                let value = unboxed.results[0];
                self.release_opaque_handle(result_boxed);
                Ok(ValueRef {
                    values: vec![value],
                    ty,
                })
            }
            BinaryOp::Eq => Ok(self.boolean_from_fcmp(FloatCC::Equal, left, right)),
            BinaryOp::NotEq => Ok(self.boolean_from_fcmp(FloatCC::NotEqual, left, right)),
            BinaryOp::Less => Ok(self.boolean_from_fcmp(FloatCC::LessThan, left, right)),
            BinaryOp::LessEq => Ok(self.boolean_from_fcmp(FloatCC::LessThanOrEqual, left, right)),
            BinaryOp::Greater => Ok(self.boolean_from_fcmp(FloatCC::GreaterThan, left, right)),
            BinaryOp::GreaterEq => {
                Ok(self.boolean_from_fcmp(FloatCC::GreaterThanOrEqual, left, right))
            }
            other => Err(format!(
                "direct backend does not support float binary operation `{:?}`",
                other
            )),
        }
    }

    fn compile_bool_binary(
        &mut self,
        op: BinaryOp,
        left: Val,
        right: Val,
    ) -> std::result::Result<ValueRef, String> {
        match op {
            BinaryOp::Eq => Ok(self.boolean_from_icmp(IntCC::Equal, left, right)),
            BinaryOp::NotEq => Ok(self.boolean_from_icmp(IntCC::NotEqual, left, right)),
            BinaryOp::And => Ok(ValueRef {
                values: vec![self.builder.band(left, right)],
                ty: DirectType::Scalar(ScalarKind::Bool),
            }),
            BinaryOp::Or => Ok(ValueRef {
                values: vec![self.builder.bor(left, right)],
                ty: DirectType::Scalar(ScalarKind::Bool),
            }),
            other => Err(format!(
                "direct backend does not support boolean binary operation `{:?}`",
                other
            )),
        }
    }

    fn boolean_from_icmp(&mut self, cc: IntCC, left: Val, right: Val) -> ValueRef {
        let cmp = self.builder.icmp(cc, left, right);
        ValueRef {
            values: vec![self.builder.uextend(types::I64, cmp)],
            ty: DirectType::Scalar(ScalarKind::Bool),
        }
    }

    fn boolean_from_fcmp(&mut self, cc: FloatCC, left: Val, right: Val) -> ValueRef {
        let cmp = self.builder.fcmp(cc, left, right);
        ValueRef {
            values: vec![self.builder.uextend(types::I64, cmp)],
            ty: DirectType::Scalar(ScalarKind::Bool),
        }
    }

    /// The zero-divisor check of an integer division, when `trap` names it.
    fn emit_int_division_guard_for(
        &mut self,
        trap: ArithmeticTrap,
        divisor: Val,
        span: Option<Span>,
    ) -> std::result::Result<(), String> {
        if !trap.checks_divisor() {
            return Ok(());
        }
        let zero = self.builder.iconst(types::I64, 0);
        let is_zero = self.builder.icmp(IntCC::Equal, divisor, zero);
        self.emit_division_failure_branch(is_zero, span)
    }

    fn emit_integer_overflow_failure_branch(
        &mut self,
        overflow: Val,
        kind: WideIntegerKind,
        op: WideOverflowOp,
        left: Val,
        right: Val,
        span: Option<Span>,
    ) -> std::result::Result<(), String> {
        let fail_block = self.builder.create_block();
        let continue_block = self.builder.create_block();
        self.builder
            .brif(overflow, fail_block, &[], continue_block, &[]);
        self.builder.switch_to_block(fail_block);
        self.emit_pending_cleanups(true)?;
        let kind_code = self.builder.iconst(types::I64, kind as i64);
        let op_code = self.builder.iconst(types::I64, op as i64);
        let (line, column) = self.span_values(span);
        self.builder.call(
            self.runtime[RuntimeFn::FailIntegerOverflow],
            &[kind_code, op_code, left, right, line, column],
        );
        self.builder.trap(TrapCode::IntegerOverflow);
        self.builder.seal_block(fail_block);
        self.builder.switch_to_block(continue_block);
        self.builder.seal_block(continue_block);
        Ok(())
    }

    fn emit_float_division_guard(
        &mut self,
        divisor: Val,
        span: Option<Span>,
    ) -> std::result::Result<(), String> {
        let zero = self.builder.f64const(0.0);
        let is_zero = self.builder.fcmp(FloatCC::Equal, divisor, zero);
        self.emit_division_failure_branch(is_zero, span)
    }

    fn emit_division_failure_branch(
        &mut self,
        is_zero: Val,
        span: Option<Span>,
    ) -> std::result::Result<(), String> {
        let fail_block = self.builder.create_block();
        let continue_block = self.builder.create_block();
        self.builder
            .brif(is_zero, fail_block, &[], continue_block, &[]);
        self.builder.switch_to_block(fail_block);
        self.emit_pending_cleanups(true)?;
        let (line, column) = self.span_values(span);
        self.builder
            .call(self.runtime[RuntimeFn::FailDivisionByZero], &[line, column]);
        self.builder.trap(TrapCode::IntegerDivisionByZero);
        self.builder.seal_block(fail_block);
        self.builder.switch_to_block(continue_block);
        self.builder.seal_block(continue_block);
        Ok(())
    }

    fn compile_call(
        &mut self,
        callee: &CallTarget,
        args: &[MirArg],
        type_args: &BTreeMap<String, Type>,
        target: &DirectType,
    ) -> std::result::Result<ValueRef, String> {
        match callee {
            CallTarget::Name(name) => self.compile_named_call(name, args, type_args),
            CallTarget::Value(function) => self.compile_function_value_call(function, args, target),
            CallTarget::Extern(call) => self.compile_extern_call(call, args, target),
            CallTarget::Member {
                object,
                field,
                receiver_place,
                dispatch,
            } => self.compile_member_call(
                object,
                MemberCallee::Method {
                    field,
                    dispatch: required_dispatch(dispatch, field)?,
                },
                receiver_place.as_deref(),
                args,
            ),
            CallTarget::Runtime {
                op,
                receiver: Some(object),
                receiver_place,
            } => self.compile_member_call(
                object,
                MemberCallee::Runtime(*op),
                receiver_place.as_deref(),
                args,
            ),
            CallTarget::Runtime {
                op, receiver: None, ..
            } => self.compile_named_runtime_op(*op, args, target),
            CallTarget::TraitMember {
                object,
                field,
                receiver_place,
                dispatch,
                ..
            } => {
                let dispatch = required_dispatch(dispatch, field)?;
                let object = self.load_operand(object)?;
                self.compile_dispatch(object, field, dispatch, receiver_place.as_deref(), args)
            }
        }
    }

    fn compile_extern_call(
        &mut self,
        call: &MirExternCall,
        args: &[MirArg],
        target: &DirectType,
    ) -> std::result::Result<ValueRef, String> {
        if call.abi != "C" {
            return Err(format!(
                "direct backend received unsupported FFI ABI `{}`",
                call.abi
            ));
        }
        if args.len() != call.params.len() {
            return Err(format!(
                "direct backend expected {} argument(s) for extern `{}`, found {}",
                call.params.len(),
                call.symbol,
                args.len()
            ));
        }

        let mut spec_params = Vec::with_capacity(call.params.len());
        let mut expected_types = Vec::with_capacity(call.params.len());
        for param in &call.params {
            spec_params.push(DirectFfiParam {
                passing: match param.passing {
                    MirReceiverKind::Value => ReceiverKind::Value,
                    MirReceiverKind::Borrow => ReceiverKind::Borrow,
                    MirReceiverKind::BorrowMut => ReceiverKind::BorrowMut,
                },
                ty: direct_ffi_type_for_source(&param.ty, Some(param.passing))?,
            });
            expected_types.push(ensure_direct_type(
                &param.ty,
                &self.classes,
                &format!("extern parameter `{}`", param.name),
            )?);
        }
        let result_spec = direct_ffi_type_for_source(&call.return_type, None)?;
        let encoded_spec = encode_direct_ffi_call_spec(&DirectFfiCallSpec {
            symbol: call.symbol.clone(),
            params: spec_params,
            result: result_spec,
        });
        let (spec_ptr, spec_len) = self.string_constant(&encoded_spec)?;
        let buffer_size = match u32::try_from(args.len().max(1).saturating_mul(8)) {
            Ok(buffer_size) => buffer_size,
            Err(_) => return Err("direct backend FFI argument buffer is too large".to_string()),
        };
        let buffer_slot = self.builder.create_sized_stack_slot(buffer_size, 3);
        let buffer = self.builder.stack_addr(types::I64, buffer_slot, 0);
        let mut writebacks = Vec::new();
        for (index, ((argument, param), expected)) in args
            .iter()
            .zip(&call.params)
            .zip(&expected_types)
            .enumerate()
        {
            let loaded = self.load_operand_for_target(&argument.value, expected)?;
            let coerced = self.coerce_value(loaded, expected)?;
            let boxed = self.ensure_opaque(coerced)?;
            self.builder
                .store(MemFlags::new(), boxed.values[0], buffer, (index as i32) * 8);
            match (param.passing, argument.writeback_place.as_ref()) {
                (MirReceiverKind::BorrowMut, Some(place)) => {
                    writebacks.push((place.clone(), boxed, expected.clone()));
                }
                (MirReceiverKind::BorrowMut, None) => {
                    return Err(format!(
                        "direct backend extern mutable argument {} has no writeback place",
                        index + 1
                    ));
                }
                (_, Some(_)) => {
                    return Err(format!(
                        "direct backend extern argument {} unexpectedly requests writeback",
                        index + 1
                    ));
                }
                (_, None) => {}
            }
        }
        let count = self.builder.iconst(types::I64, call.params.len() as i64);
        let invocation = self.builder.call(
            self.runtime[RuntimeFn::FfiCall],
            &[spec_ptr, spec_len, buffer, count],
        );
        let raw_result = invocation.results[0];

        for (place, boxed, expected) in writebacks {
            let writeback = self.coerce_value(boxed, &expected)?;
            self.store_place(&place, writeback)?;
        }

        let boxed_result = self.owned_opaque_result(vec![raw_result], call.return_type.clone());
        let return_direct = ensure_direct_type(
            &call.return_type,
            &self.classes,
            &format!("extern return from `{}`", call.symbol),
        )?;
        let result = self.coerce_value(boxed_result, &return_direct)?;
        self.coerce_value(result, target)
    }

    fn compile_function_value_call(
        &mut self,
        function: &Operand,
        args: &[MirArg],
        target: &DirectType,
    ) -> std::result::Result<ValueRef, String> {
        let function_type = infer_operand_type(function, &self.variable_types, &self.classes)
            .ok_or("direct backend could not infer the indirect callee type".to_string())?;
        if let DirectType::Callable(callable) = &function_type {
            let signature = callable.signature.clone();
            return self.compile_inline_callable_call(function, args, target, &signature);
        }
        Err(format!(
            "direct backend expected an indirect function value, found `{}`",
            render_direct_type(&function_type)
        ))
    }

    /// A call through an inline callable: the arguments are coerced to the
    /// contract's direct parameters and passed with a supplied mask to the
    /// `invoke` adapter named by the descriptor, which fills defaults and
    /// writes mutated captures back into the environment words.
    fn compile_inline_callable_call(
        &mut self,
        function: &Operand,
        args: &[MirArg],
        target: &DirectType,
        signature: &Type,
    ) -> std::result::Result<ValueRef, String> {
        let (params, return_type) = callables::contract_parts(signature)?;
        if args.len() > params.len() {
            return Err(format!(
                "direct backend expected at most {} indirect-call arguments, found {}",
                params.len(),
                args.len()
            ));
        }
        let binding = bind_function_value_args(
            &params,
            args,
            "direct backend function value has no parameter named",
            "direct backend received duplicate indirect-call arguments",
        )?;
        let param_types =
            function_value_param_types(&params, &self.classes, "indirect-call parameter")?;
        let mutable_params = params
            .iter()
            .map(|param| param.passing == ReceiverKind::BorrowMut)
            .collect::<Vec<_>>();
        let return_direct = ensure_direct_type(
            crate::sema::returned_view_pointee(&return_type),
            &self.classes,
            "indirect-call return type",
        )?;
        let callee_root = match function {
            Operand::Place(place) if !place.contains('.') => Some(place.clone()),
            _ => None,
        };
        let closure_writebacks = match function {
            Operand::Place(place) | Operand::MovePlace(place) => self
                .closure_capture_writebacks
                .get(place.split('.').next().unwrap_or(place))
                .cloned()
                .unwrap_or_default(),
            _ => Vec::new(),
        };
        let loaded = self.load_operand(function)?;
        let callee_ty = DirectType::Callable(DirectCallableType::new(signature.clone()));
        let callee = self.coerce_value(loaded, &callee_ty)?;
        let callee_owned = self.temporary_owns_callable(&callee);
        let slot = self
            .builder
            .create_sized_stack_slot((8 * (1 + CALLABLE_ENVIRONMENT_WORDS)) as u32, 3);
        let callable_ptr = self.builder.stack_addr(types::I64, slot, 0);
        for (index, word) in callee.values.iter().enumerate() {
            self.builder
                .store(MemFlags::new(), *word, callable_ptr, (index as i32) * 8);
        }

        let mut supplied: Vec<Option<Vec<Val>>> = vec![None; params.len()];
        let mut mask_bits = 0i64;
        let mut writebacks = Vec::new();
        for (argument, index) in args.iter().zip(&binding.source_slots) {
            let index = *index;
            let expected = &param_types[index];
            let loaded_value = self.load_operand_for_target(&argument.value, expected)?;
            let value = self.coerce_value(loaded_value, expected)?;
            let transferred = self.transfer_values(&value);
            supplied[index] = Some(transferred);
            mask_bits |= 1i64 << index;
            match (params[index].passing, argument.writeback_place.as_ref()) {
                (ReceiverKind::BorrowMut, Some(place)) => {
                    writebacks.push((index, place.clone(), expected.clone()));
                }
                (ReceiverKind::BorrowMut, None) => {
                    return Err(format!(
                        "direct backend indirect mutable argument {} has no writeback place",
                        index + 1
                    ));
                }
                (_, Some(_)) => {
                    return Err(format!(
                        "direct backend indirect argument {} unexpectedly requests writeback",
                        index + 1
                    ));
                }
                (_, None) => {}
            }
        }
        let mut public_sinks = Vec::new();
        let mut capture_sinks = Vec::new();
        if !writebacks.is_empty() || !closure_writebacks.is_empty() {
            public_sinks = (0..params.len())
                .map(|_| self.builder.iconst(types::I64, 0))
                .collect();
            for (index, place, _) in &writebacks {
                public_sinks[*index] = self.mutable_sink_for_place(place)?;
            }
            for writeback in &closure_writebacks {
                capture_sinks.push((
                    writeback.index,
                    self.mutable_sink_for_resolved_place(writeback.place.clone())?,
                ));
            }
            self.install_indirect_mutable_sinks(&public_sinks, &capture_sinks)?;
        }
        let mask = self.builder.iconst(types::I64, mask_bits);
        let mut lowered = vec![callable_ptr, mask];
        for (index, expected) in param_types.iter().enumerate() {
            match supplied[index].take() {
                Some(values) => lowered.extend(values),
                None => lowered.extend(expected.zero_values(&mut *self.builder)),
            }
        }
        let invoke = self.builder.load(
            types::I64,
            MemFlags::new(),
            callee.values[0],
            callables::DESCRIPTOR_INVOKE,
        );
        let invoke_signature = self
            .builder
            .import_signature(callables::contract_invoke_signature(
                &param_types,
                &mutable_params,
                &return_direct,
            ));
        let inst = self
            .builder
            .call_indirect(invoke_signature, invoke, &lowered);
        let results = inst.results;
        self.release_mutable_sinks(public_sinks.iter().copied());
        self.release_mutable_sinks(capture_sinks.iter().map(|(_, sink)| *sink));

        // The environment words may have changed (mutated captures), so the
        // callee's words are read back into its local or owned temporary.
        let reloaded = (0..1 + CALLABLE_ENVIRONMENT_WORDS)
            .map(|index| {
                self.builder.load(
                    types::I64,
                    MemFlags::new(),
                    callable_ptr,
                    (index as i32) * 8,
                )
            })
            .collect::<Vec<_>>();
        if let Some(root) = callee_root.as_deref() {
            if matches!(self.local_type(root)?, DirectType::Callable(_)) {
                let vars = self.local_vars(root)?;
                for (var, word) in vars.into_iter().zip(reloaded.iter().copied()) {
                    self.builder.def_var(var, word);
                }
            }
        } else if callee_owned {
            self.clear_temporary_callable_owned(&callee);
            let refreshed = ValueRef {
                values: reloaded.clone(),
                ty: callee_ty.clone(),
            };
            self.mark_temporary_callable_owned(&refreshed);
        }

        let result_count = return_direct.value_count();
        let result = ValueRef {
            values: results[..result_count].to_vec(),
            ty: return_direct.clone(),
        };
        self.mark_call_result_owned(&result);
        let mut cursor = result_count;
        for (_, place, writeback_ty) in writebacks {
            let count = writeback_ty.value_count();
            let writeback = ValueRef {
                values: results[cursor..cursor + count].to_vec(),
                ty: writeback_ty,
            };
            cursor += count;
            self.mark_call_result_owned(&writeback);
            self.store_place(&place, writeback)?;
        }
        for writeback in closure_writebacks {
            let env_base = if writeback.inline {
                self.builder.iadd_imm(callable_ptr, 8)
            } else {
                reloaded[1]
            };
            let mut values = Vec::new();
            for (offset, abi) in writeback.ty.abi_types().into_iter().enumerate() {
                let word = self.builder.load(
                    types::I64,
                    MemFlags::new(),
                    env_base,
                    ((writeback.env_offset + offset) as i32) * 8,
                );
                values.push(if abi == types::F64 {
                    self.builder.bitcast(types::F64, word)
                } else {
                    word
                });
            }
            let value = ValueRef {
                values,
                ty: writeback.ty.clone(),
            };
            let value = self.coerce_value(value, &writeback.ty)?;
            self.store_resolved_view_place(writeback.place, value)?;
        }
        self.coerce_value(result, target)
    }

    /// Marks a call result or writeback owned by its direct type.
    fn mark_call_result_owned(&mut self, value: &ValueRef) {
        match &value.ty {
            DirectType::Opaque(_) => self.mark_temporary_opaque_owned(value),
            DirectType::Union(union) if union.owns_handles() => {
                self.mark_temporary_union_owned(value)
            }
            DirectType::Callable(_) => self.mark_temporary_callable_owned(value),
            _ => {}
        }
    }

    /// `print(value)`. Lowering picked the helper from the argument's type;
    /// a scalar argument passes its word, and any other value its box.
    fn compile_print(
        &mut self,
        kind: PrintKind,
        args: &[MirArg],
    ) -> std::result::Result<ValueRef, String> {
        let op = RuntimeOp::Print(kind);
        let [argument] = args else {
            return Err("direct backend expected `print` to receive one argument".to_string());
        };
        let argument = self.load_operand(&argument.value)?;
        let scalar_matches = matches!(
            (kind, argument.ty.scalar_kind()),
            (PrintKind::Int, Some(ScalarKind::Int32 | ScalarKind::Int64))
                | (PrintKind::Uint64, Some(ScalarKind::Uint64))
                | (PrintKind::Float32, Some(ScalarKind::Float32))
                | (PrintKind::Float64, Some(ScalarKind::Float64))
                | (PrintKind::Bool, Some(ScalarKind::Bool))
        );
        let (function, word) = if scalar_matches {
            (op.runtime_call().function, argument.values[0])
        } else {
            // A boxed value, or a scalar whose layout the kind does not
            // name, prints through its box.
            let argument = self.ensure_opaque(argument)?;
            (RuntimeFn::PrintValue, argument.values[0])
        };
        self.builder.call(self.runtime[function], &[word]);
        Ok(unit_value(&mut *self.builder))
    }

    /// Builds an f-string in one `str` box: the leading literal starts it and
    /// each later part appends in place, so no part needs its own box or a
    /// concatenation. `int64` parts append without being boxed.
    fn compile_format_string(
        &mut self,
        parts: &[MirFormatPart],
    ) -> std::result::Result<ValueRef, String> {
        let (leading, rest) = match parts.split_first() {
            Some((MirFormatPart::Literal(text), rest)) => (text.as_str(), rest),
            _ => ("", parts),
        };
        let (ptr, len) = self.string_constant(leading.as_bytes())?;
        // A leading `int64` part joins the start, which also sizes the
        // buffer for the parts after it.
        let mut rest = rest;
        let mut boxed_first = None;
        let call = match rest.split_first() {
            Some((MirFormatPart::Int64(value), after)) => {
                rest = after;
                let value = self.load_operand(value)?;
                if value.ty == DirectType::Scalar(ScalarKind::Int64) {
                    let reserve = self
                        .builder
                        .iconst(types::I64, format_reserve_hint(after) as i64);
                    self.builder.call(
                        self.runtime[RuntimeFn::FormatBeginI64],
                        &[ptr, len, value.values[0], reserve],
                    )
                } else {
                    boxed_first = Some(value);
                    self.builder
                        .call(self.runtime[RuntimeFn::FormatBegin], &[ptr, len])
                }
            }
            _ => self
                .builder
                .call(self.runtime[RuntimeFn::FormatBegin], &[ptr, len]),
        };
        let builder = self.owned_opaque_result(call.results, Type::named("str"));
        let handle = builder.values[0];
        if let Some(value) = boxed_first {
            self.push_format_value(handle, value)?;
        }
        for part in rest {
            match part {
                MirFormatPart::Literal(text) => {
                    let (ptr, len) = self.string_constant(text.as_bytes())?;
                    self.builder
                        .call(self.runtime[RuntimeFn::FormatPushStr], &[handle, ptr, len]);
                }
                MirFormatPart::Int64(value) => {
                    let value = self.load_operand(value)?;
                    if value.ty == DirectType::Scalar(ScalarKind::Int64) {
                        self.builder.call(
                            self.runtime[RuntimeFn::FormatPushI64],
                            &[handle, value.values[0]],
                        );
                    } else {
                        self.push_format_value(handle, value)?;
                    }
                }
                MirFormatPart::Value(value) => {
                    let value = self.load_operand(value)?;
                    self.push_format_value(handle, value)?;
                }
                MirFormatPart::Formatted {
                    value,
                    spec,
                    value_type,
                } => {
                    let value = self.load_operand(value)?;
                    let value = self.ensure_opaque(value)?;
                    let (spec_ptr, spec_len) = self.string_constant(spec.as_bytes())?;
                    let type_name = value_type.to_string();
                    let (type_ptr, type_len) = self.string_constant(type_name.as_bytes())?;
                    self.builder.call(
                        self.runtime[RuntimeFn::FormatPushFormatted],
                        &[
                            handle,
                            value.values[0],
                            spec_ptr,
                            spec_len,
                            type_ptr,
                            type_len,
                        ],
                    );
                }
            }
        }
        Ok(builder)
    }

    /// Appends a boxed f-string part.
    fn push_format_value(
        &mut self,
        handle: Val,
        value: ValueRef,
    ) -> std::result::Result<(), String> {
        let value = self.ensure_opaque(value)?;
        self.builder.call(
            self.runtime[RuntimeFn::FormatPushValue],
            &[handle, value.values[0]],
        );
        Ok(())
    }

    fn string_value(&mut self, text: &str) -> std::result::Result<ValueRef, String> {
        let (ptr, len) = self.string_constant(text.as_bytes())?;
        let call = self
            .builder
            .call(self.runtime[RuntimeFn::StringLiteral], &[ptr, len]);
        Ok(self.owned_opaque_result(call.results, Type::named("str")))
    }

    /// A direct call to a function of the program. Lowering resolves calls
    /// to built-in functions to runtime ops. An argument whose parameter
    /// names a type parameter loads as its type at this call, which MIR
    /// records, and passes in the parameter's boxed layout.
    fn compile_named_call(
        &mut self,
        name: &str,
        args: &[MirArg],
        type_args: &BTreeMap<String, Type>,
    ) -> std::result::Result<ValueRef, String> {
        let func_ref = *self
            .function_refs
            .get(name)
            .ok_or(format!("direct backend does not know function `{}`", name))?;
        let mut lowered_args = Vec::new();
        let expected = self
            .function_param_types
            .get(name)
            .cloned()
            .unwrap_or_default();
        let substitutions = type_args
            .iter()
            .map(|(name, ty)| (name.clone(), ty.clone()))
            .collect::<HashMap<_, _>>();
        let mut writeback_places = Vec::new();
        let mut mutable_sink_places = vec![None; expected.len()];
        // The shared validator binds one argument to each parameter.
        debug_assert_eq!(args.len(), expected.len(), "arguments of `{name}`");
        for (index, (argument, abi_expected)) in args.iter().zip(&expected).enumerate() {
            let specialized = substitute_type(&direct_type_to_type(abi_expected), &substitutions);
            let semantic_expected = ensure_direct_type(
                &specialized,
                &self.classes,
                &format!("specialized argument {} for `{name}`", index + 1),
            )?;
            let loaded = match &argument.value {
                Operand::Place(place)
                    if self.moves_lent_temp(place, argument.writeback_place.as_deref()) =>
                {
                    self.take_place(place)?
                }
                operand => self.load_operand_for_target(operand, &semantic_expected)?,
            };
            let semantic_coerced = self.coerce_value(loaded, &semantic_expected)?;
            let coerced = self.coerce_value(semantic_coerced, abi_expected)?;
            if let Some(place) = &argument.writeback_place {
                writeback_places.push(place.clone());
                if let Some(slot) = mutable_sink_places.get_mut(index) {
                    *slot = Some(place.clone());
                }
            }
            let transferred = self.transfer_values(&coerced);
            lowered_args.extend(transferred);
        }
        let mutable_sinks = if mutable_sink_places.iter().any(Option::is_some) {
            let mut sinks = Vec::with_capacity(mutable_sink_places.len());
            for place in &mutable_sink_places {
                sinks.push(match place {
                    Some(place) => self.mutable_sink_for_place(place)?,
                    None => self.builder.iconst(types::I64, 0),
                });
            }
            self.install_direct_mutable_sinks(&sinks)?;
            sinks
        } else {
            Vec::new()
        };
        let inst = self.builder.call(func_ref, &lowered_args);
        let results = inst.results;
        self.release_mutable_sinks(mutable_sinks);
        let (result, writebacks) = self.split_call_results(name, results)?;
        self.apply_writeback_places(&writeback_places, writebacks)?;
        Ok(result)
    }

    /// Emits a resolved op with no receiver: a named built-in function,
    /// `print`, or a host builtin.
    fn compile_named_runtime_op(
        &mut self,
        op: RuntimeOp,
        args: &[MirArg],
        target: &DirectType,
    ) -> std::result::Result<ValueRef, String> {
        if matches!(
            op,
            RuntimeOp::Print(_) | RuntimeOp::Host(_) | RuntimeOp::Function(_)
        ) {
            require_bound_args(op, args)?;
        }
        match op {
            RuntimeOp::Print(kind) => self.compile_print(kind, args),
            RuntimeOp::Host(builtin) => self.compile_host_builtin_call(builtin, args),
            RuntimeOp::Function(function) => self.compile_function_op(function, args, target),
            RuntimeOp::Resource(_)
            | RuntimeOp::Rng(_)
            | RuntimeOp::Scalar(_)
            | RuntimeOp::Value(_)
            | RuntimeOp::Str(_)
            | RuntimeOp::Queue(_)
            | RuntimeOp::Task(_)
            | RuntimeOp::TaskGroup(_)
            | RuntimeOp::List(_)
            | RuntimeOp::Dict(_)
            | RuntimeOp::Set(_)
            | RuntimeOp::Array(_)
            | RuntimeOp::Span(_) => Err(format!(
                "direct backend received runtime operation `{op}` without a receiver"
            )),
        }
    }

    /// A named built-in function. The op names the helper; the arms here
    /// decide how arguments and results are laid out in words.
    fn compile_function_op(
        &mut self,
        op: FunctionOp,
        args: &[MirArg],
        target: &DirectType,
    ) -> std::result::Result<ValueRef, String> {
        match op {
            FunctionOp::ArrayZeros | FunctionOp::ArrayFull | FunctionOp::ArrayFromList => {
                self.compile_array_constructor(op, args, target)
            }
            FunctionOp::ListWithCapacity
            | FunctionOp::DictWithCapacity
            | FunctionOp::SetWithCapacity => {
                self.compile_collection_with_capacity(op, args, target)
            }
            FunctionOp::Range => self.compile_range(args),
            FunctionOp::Select => self.compile_select(args, target),
            FunctionOp::WaitAny
            | FunctionOp::WaitAnyTimeout
            | FunctionOp::WaitAll
            | FunctionOp::WaitAllTimeout => self.compile_wait(op, args),
            FunctionOp::Divmod => self.compile_divmod(args),
            _ => self.compile_plain_function_op(op, args),
        }
    }

    /// A named op whose arguments each pass as one word, in parameter order,
    /// and whose helper returns one word or nothing.
    fn compile_plain_function_op(
        &mut self,
        op: FunctionOp,
        args: &[MirArg],
    ) -> std::result::Result<ValueRef, String> {
        let runtime_op = RuntimeOp::Function(op);
        let call = runtime_op.runtime_call();
        let params = runtime_op.params();
        let mut words = Vec::with_capacity(params.len() + call.immediates.len());
        // `abs`, `min`, `max`, `sqrt`, and `round` return their first
        // argument's layout.
        let mut first_layout = None;
        // MIR binds one argument to each parameter, in order. An omitted
        // timeout or queue capacity passes no value.
        for argument in args {
            if argument.value == Operand::Default(ArgDefault::Omitted) {
                words.push(self.builder.iconst(types::I64, 0));
                continue;
            }
            let word = match function_op_arg_layout(op) {
                FunctionOpArgLayout::Int64 => {
                    let value = self
                        .load_operand_with_integer_hint(&argument.value, Some(ScalarKind::Int64))?;
                    let value = self.coerce_value(value, &DirectType::Scalar(ScalarKind::Int64))?;
                    value.values[0]
                }
                FunctionOpArgLayout::Boxed => {
                    let loaded = self.load_operand(&argument.value)?;
                    first_layout.get_or_insert(loaded.ty.clone());
                    let value = self.ensure_opaque(loaded)?;
                    value.values[0]
                }
            };
            words.push(word);
        }
        self.push_runtime_call_words(call, &mut words);
        let results = self.runtime_call_results(self.runtime[call.function], &words);
        match function_op_result_layout(op) {
            FunctionOpResultLayout::Unit => Ok(unit_value(&mut *self.builder)),
            FunctionOpResultLayout::Scalar(kind) => Ok(ValueRef {
                values: results,
                ty: DirectType::Scalar(kind),
            }),
            FunctionOpResultLayout::BoxedBool => {
                let result = self.owned_opaque_result(results, Type::named("bool"));
                self.coerce_value(result, &DirectType::Scalar(ScalarKind::Bool))
            }
            FunctionOpResultLayout::Boxed(result_ty) => {
                let result_ty = match result_ty {
                    Some(result_ty) => result_ty,
                    // Every other op is a module function with a declared
                    // result type.
                    None => runtime_op
                        .result_type(None, &[])
                        .expect("a module function declares its result type"),
                };
                Ok(self.owned_opaque_result(results, result_ty))
            }
            FunctionOpResultLayout::FirstArgument => {
                // The shared validator binds the required first argument.
                let layout = first_layout.expect("the first argument is bound");
                // `round` returns an `int64` for a float argument.
                let return_ty = match (op, layout.scalar_kind()) {
                    (FunctionOp::Round, Some(ScalarKind::Float32 | ScalarKind::Float64)) => {
                        DirectType::Scalar(ScalarKind::Int64)
                    }
                    _ => layout,
                };
                let result = self.owned_opaque_result(results, Type::named("Unknown"));
                self.coerce_value(result, &return_ty)
            }
        }
    }

    /// `Array.zeros`, `Array.full`, or `Array.from_list`. The dtype code
    /// comes from the call's `Array` result type.
    fn compile_array_constructor(
        &mut self,
        op: FunctionOp,
        args: &[MirArg],
        target: &DirectType,
    ) -> std::result::Result<ValueRef, String> {
        let runtime_op = RuntimeOp::Function(op);
        let (array_ty, element_type) = match target {
            DirectType::Opaque(ty @ Type::Named(owner, arguments))
                if owner == "Array" && arguments.len() == 1 =>
            {
                (ty.clone(), arguments[0].clone())
            }
            other => {
                return Err(format!(
                    "direct backend requires an Array result type for `{}`, found {}",
                    op.name(),
                    render_direct_type(other)
                ));
            }
        };
        let dtype = self
            .builder
            .iconst(types::I64, direct_array_dtype_code(&element_type)?);
        let zero = self.builder.iconst(types::I64, 0);
        let shape_ty =
            DirectType::Opaque(Type::Named("list".to_string(), vec![Type::named("int64")]));
        let ordered = args;
        let mut words = vec![dtype];
        match op {
            FunctionOp::ArrayZeros => {
                let shape = self.load_operand_for_target(&ordered[0].value, &shape_ty)?;
                let shape = self.ensure_opaque(shape)?;
                words.push(shape.values[0]);
            }
            FunctionOp::ArrayFull => {
                let shape = self.load_operand_for_target(&ordered[0].value, &shape_ty)?;
                let shape = self.ensure_opaque(shape)?;
                let element_direct =
                    ensure_direct_type(&element_type, &self.classes, "Array dtype")?;
                let value =
                    self.load_operand_as_opaque_direct(&ordered[1].value, &element_direct)?;
                words.extend([shape.values[0], value.values[0]]);
            }
            _ => {
                debug_assert_eq!(op, FunctionOp::ArrayFromList);
                let values_ty =
                    DirectType::Opaque(Type::Named("list".to_string(), vec![element_type]));
                let values = self.load_operand_for_target(&ordered[0].value, &values_ty)?;
                let values = self.ensure_opaque(values)?;
                let shape = self.load_operand_for_target(&ordered[1].value, &shape_ty)?;
                let shape = self.ensure_opaque(shape)?;
                words.extend([values.values[0], shape.values[0]]);
            }
        }
        // The span words are zero, so the dtype's zero constant serves.
        words.extend([zero, zero]);
        let function = runtime_op.runtime_call().function;
        let results = self.runtime_call_results(self.runtime[function], &words);
        Ok(self.owned_opaque_result(results, array_ty))
    }

    /// `list.with_capacity`, `dict.with_capacity`, or `set.with_capacity`:
    /// an empty collection that reserves room for `minimum` elements.
    fn compile_collection_with_capacity(
        &mut self,
        op: FunctionOp,
        args: &[MirArg],
        target: &DirectType,
    ) -> std::result::Result<ValueRef, String> {
        let ordered = args;
        let minimum =
            self.load_operand_with_integer_hint(&ordered[0].value, Some(ScalarKind::Int64))?;
        let minimum = self.coerce_value(minimum, &DirectType::Scalar(ScalarKind::Int64))?;
        let empty = op
            .empty_collection_fn()
            .expect("a capacity constructor names its empty collection");
        let empty_call = self.builder.call(self.runtime[empty], &[]);
        let collection = empty_call.results[0];
        let call = RuntimeOp::Function(op).runtime_call();
        let zero = self.builder.iconst(types::I64, 0);
        let mut words = vec![collection, zero, minimum.values[0]];
        self.push_runtime_call_words(call, &mut words);
        let reserve = self.builder.call(self.runtime[call.function], &words);
        self.release_opaque_handle(reserve.results[0]);
        let result_ty = direct_type_to_type(target);
        Ok(self.owned_opaque_result(vec![collection], result_ty))
    }

    /// `select(source, ...)`: the sources pass as one tuple. The shared
    /// validator requires one or more positional sources.
    fn compile_select(
        &mut self,
        args: &[MirArg],
        target: &DirectType,
    ) -> std::result::Result<ValueRef, String> {
        let mut operands = Vec::with_capacity(args.len());
        let mut element_types = Vec::with_capacity(args.len());
        let mut queue_payload: Option<Type> = None;
        let mut task_payload: Option<Type> = None;
        for argument in args {
            let direct =
                infer_operand_type(&argument.value, &self.variable_types, &self.classes)
                    .ok_or("direct backend could not infer a `select` source type".to_string())?;
            let ty = direct_type_to_type(&direct);
            match &ty {
                Type::Named(name, type_args) if name == "Queue" => {
                    let [payload] = type_args.as_slice() else {
                        return Err(
                            "direct backend expected `Queue[Q]` as a `select` source".to_string()
                        );
                    };
                    if let Some(previous) = &queue_payload {
                        if previous != payload {
                            return Err(
                                "direct backend received inconsistent Queue payload types in \
                                 `select`"
                                    .to_string(),
                            );
                        }
                    } else {
                        queue_payload = Some(payload.clone());
                    }
                }
                Type::Named(name, type_args) if name == "Task" => {
                    let [payload] = type_args.as_slice() else {
                        return Err(
                            "direct backend expected `Task[T]` as a `select` source".to_string()
                        );
                    };
                    if let Some(previous) = &task_payload {
                        if previous != payload {
                            return Err(
                                "direct backend received inconsistent Task result types in \
                                 `select`"
                                    .to_string(),
                            );
                        }
                    } else {
                        task_payload = Some(payload.clone());
                    }
                }
                Type::Named(name, type_args) if name == "Duration" && type_args.is_empty() => {}
                _ => {
                    return Err(format!(
                        "direct backend expected a Queue, Task, or Duration `select` source, \
                         found `{}`",
                        render_direct_type(&direct)
                    ));
                }
            }
            operands.push(argument.value.clone());
            element_types.push(ty);
        }

        let tuple_type = Type::Tuple(element_types.clone());
        let tuple = self.compile_tuple_literal(&operands, &element_types, tuple_type)?;
        let tuple = self.transfer_opaque_arg(&tuple);
        let function = RuntimeOp::Function(FunctionOp::Select)
            .runtime_call()
            .function;
        let inst = self.builder.call(self.runtime[function], &[tuple]);
        let inferred_result_type = Type::Named(
            "SelectOutcome".to_string(),
            vec![
                queue_payload.unwrap_or(Type::Unit),
                task_payload.unwrap_or(Type::Unit),
            ],
        );
        let result_type = match target {
            DirectType::Opaque(Type::Named(name, type_args))
                if name == "SelectOutcome" && type_args.len() == 2 =>
            {
                Type::Named(name.clone(), type_args.clone())
            }
            _ => inferred_result_type,
        };
        Ok(self.owned_opaque_result(inst.results, result_type))
    }

    /// `wait_any` or `wait_all`, with or without a timeout.
    fn compile_wait(
        &mut self,
        op: FunctionOp,
        args: &[MirArg],
    ) -> std::result::Result<ValueRef, String> {
        let runtime_op = RuntimeOp::Function(op);
        let tasks_arg = args.first().ok_or(format!(
            "direct backend expected `{}(tasks, timeout=...)`",
            op.name()
        ))?;
        let tasks = self.load_operand(&tasks_arg.value)?;
        let tasks = self.ensure_opaque(tasks)?;
        let task_payload_ty =
            infer_operand_type(&tasks_arg.value, &self.variable_types, &self.classes)
                .map(|direct| match direct {
                    DirectType::Opaque(Type::Named(vec_name, args)) if vec_name == "list" => {
                        match args.as_slice() {
                            [Type::Named(task_name, task_args)] if task_name == "Task" => {
                                task_args.first().cloned().unwrap_or(Type::Unit)
                            }
                            _ => Type::named("Unknown"),
                        }
                    }
                    _ => Type::named("Unknown"),
                })
                .unwrap_or(Type::named("Unknown"));
        let mut words = vec![tasks.values[0]];
        if let Some(timeout_arg) = args.get(1) {
            let timeout = self.load_operand(&timeout_arg.value)?;
            let timeout = self.ensure_opaque(timeout)?;
            words.push(timeout.values[0]);
        }
        let function = runtime_op.runtime_call().function;
        let inst = self.builder.call(self.runtime[function], &words);
        let enum_name = match op.unspecialized() {
            FunctionOp::WaitAny => "WaitAny",
            _ => "WaitAll",
        };
        Ok(self.owned_opaque_result(
            inst.results,
            Type::Named(enum_name.to_string(), vec![task_payload_ty]),
        ))
    }

    /// `divmod(left, right)`: the left operand is tagged with its static
    /// type, which the helper's quotient and remainder take.
    fn compile_divmod(&mut self, args: &[MirArg]) -> std::result::Result<ValueRef, String> {
        let ordered = args;
        let left = self.load_operand(&ordered[0].value)?;
        let operand_ty = direct_type_to_type(&left.ty);
        let left = self.ensure_opaque(left)?;
        self.tag_opaque_runtime_type(&left, &operand_ty)?;
        let right = self.load_operand(&ordered[1].value)?;
        let right = self.ensure_opaque(right)?;
        let function = RuntimeOp::Function(FunctionOp::Divmod)
            .runtime_call()
            .function;
        let inst = self
            .builder
            .call(self.runtime[function], &[left.values[0], right.values[0]]);
        let result_ty = Type::Tuple(vec![operand_ty.clone(), operand_ty]);
        Ok(self.owned_opaque_result(inst.results, result_ty))
    }

    /// A host builtin: its arguments pass boxed in an argument buffer.
    fn compile_host_builtin_call(
        &mut self,
        builtin: HostBuiltin,
        args: &[MirArg],
    ) -> std::result::Result<ValueRef, String> {
        let name = builtin.name();
        // Checked MIR materializes builtin defaults before direct code generation, so
        // optional metadata parameters are present in this argument list too.
        let mut loaded_args = Vec::with_capacity(args.len());
        for (index, argument) in args.iter().enumerate() {
            if argument.value == Operand::Default(ArgDefault::Omitted) {
                return Err(format!(
                    "direct backend is missing argument {} for `{name}`",
                    index + 1
                ));
            }
            let loaded = self.load_operand(&argument.value)?;
            let loaded = self.ensure_opaque(loaded)?;
            loaded_args.push(loaded);
        }
        let count = self.builder.iconst(types::I64, loaded_args.len() as i64);
        let buffer_call = self
            .builder
            .call(self.runtime[RuntimeFn::ArgBufferNew], &[count]);
        let buffer = buffer_call.results[0];
        for (index, loaded) in loaded_args.iter().enumerate() {
            let index = self.builder.iconst(types::I64, index as i64);
            self.builder.call(
                self.runtime[RuntimeFn::ArgBufferStore],
                &[buffer, index, loaded.values[0]],
            );
        }
        let entry = self.host_builtin_ref(builtin)?;
        let call = self.builder.call(entry, &[buffer, count]);

        let semantic_type = RuntimeOp::Host(builtin)
            .result_type(None, &[])
            .expect("a host builtin names its result type");
        let result = self.owned_opaque_result(call.results, semantic_type.clone());
        let target = ensure_direct_type(
            &semantic_type,
            &self.classes,
            &format!("return value of `{name}`"),
        )?;
        self.coerce_value(result, &target)
    }

    /// The runtime entry for one host builtin, imported on first use.
    fn host_builtin_ref(&mut self, builtin: HostBuiltin) -> std::result::Result<FuncRef, String> {
        let RuntimeTarget::Host(symbol) = RuntimeOp::Host(builtin).runtime_target() else {
            unreachable!("a host builtin calls its own entry");
        };
        if let Some(entry) = self.host_builtin_refs.get(symbol) {
            return Ok(*entry);
        }
        let id = declare_runtime_function(
            self.builder.module(),
            symbol,
            &[types::I64, types::I64],
            Some(types::I64),
        )?;
        let entry = self.builder.import_function(id);
        self.host_builtin_refs.insert(symbol, entry);
        Ok(entry)
    }

    fn compile_range(&mut self, args: &[MirArg]) -> std::result::Result<ValueRef, String> {
        let int_ty = DirectType::Scalar(ScalarKind::Int64);
        // MIR binds one argument to `stop` and two to `start, stop`.
        let (start_arg, stop_arg) = match args {
            [stop] => (None, stop),
            [start, stop] => (Some(start), stop),
            _ => {
                return Err(
                    "direct backend expected `range()` to receive a `stop` argument".to_string(),
                )
            }
        };

        let start = if let Some(argument) = start_arg {
            let loaded = self.load_operand(&argument.value)?;
            self.coerce_value(loaded, &int_ty)?
        } else {
            ValueRef {
                values: vec![self.builder.iconst(types::I64, 0)],
                ty: int_ty.clone(),
            }
        };
        let stop = self.load_operand(&stop_arg.value)?;
        let stop = self.coerce_value(stop, &int_ty)?;
        let inst = self.builder.call(
            self.runtime[RuntimeFn::RangeNew],
            &[start.values[0], stop.values[0]],
        );
        Ok(self.owned_opaque_result(inst.results, Type::named("Range")))
    }

    fn compile_for_range(
        &mut self,
        binding: &str,
        iterable: &Operand,
        body_label: &str,
        exit_label: &str,
    ) -> std::result::Result<(), String> {
        let Operand::Place(iterable_place) = iterable else {
            return Err(
                "direct backend requires `for range` iterables to live in a place".to_string(),
            );
        };
        let range = self.load_place(iterable_place)?;
        let range = self.ensure_opaque(range)?;
        let current_inst = self
            .builder
            .call(self.runtime[RuntimeFn::RangeCurrent], &[range.values[0]]);
        let current = current_inst.results[0];
        let end_inst = self
            .builder
            .call(self.runtime[RuntimeFn::RangeEnd], &[range.values[0]]);
        let end = end_inst.results[0];
        let has_next = self.builder.icmp(IntCC::SignedLessThan, current, end);

        let next_block = self.builder.create_block();
        let body_block = self.compiled_block(body_label)?;
        let exit_block = self.compiled_block(exit_label)?;
        self.builder
            .brif(has_next, next_block, &[], exit_block, &[]);

        self.builder.switch_to_block(next_block);
        let binding_ty = self.type_of_place(binding)?;
        self.store_place(
            binding,
            ValueRef {
                values: vec![current],
                ty: DirectType::Scalar(ScalarKind::Int64),
            },
        )?;
        let advanced_inst = self
            .builder
            .call(self.runtime[RuntimeFn::RangeAdvance], &[range.values[0]]);
        let advanced = self.owned_opaque_result(advanced_inst.results, Type::named("Range"));
        self.store_place(iterable_place, advanced)?;
        self.builder.jump(body_block, &[]);
        self.builder.seal_block(next_block);
        let _ = binding_ty;
        Ok(())
    }

    /// The direct backend refuses a union operation whose module carries no
    /// validated layout plan for that union (ADR-0052 A9).
    fn require_union_plan(&self, union_type: &Type) -> std::result::Result<(), String> {
        if self
            .union_layouts
            .contains_key(&crate::union_layout::union_plan_key(union_type))
        {
            Ok(())
        } else {
            Err(format!(
                "direct backend has no layout plan for union `{union_type}`"
            ))
        }
    }

    fn compile_member_call(
        &mut self,
        object: &Operand,
        callee: MemberCallee<'_>,
        receiver_place: Option<&str>,
        args: &[MirArg],
    ) -> std::result::Result<ValueRef, String> {
        if let Some(result) = self.compile_borrowed_element_member_call(object, callee, args)? {
            return Ok(result);
        }
        let object = match (object, callee) {
            (Operand::Place(place), MemberCallee::Method { .. })
                if self.moves_lent_temp(place, receiver_place) =>
            {
                self.take_place(place)?
            }
            (object, _) => self.load_operand(object)?,
        };
        let receiver_place = receiver_place.filter(|place| {
            !(matches!(callee, MemberCallee::Runtime(_)) && self.lent_temps.contains_key(*place))
        });
        self.compile_member_callee_on_receiver(object, callee, receiver_place, args)
    }

    /// Whether a call operand reading `place` with `writeback` moves the
    /// temporary of a mutable lend in.
    fn moves_lent_temp(&self, place: &str, writeback: Option<&str>) -> bool {
        writeback == Some(place) && self.lent_temps.get(place) == Some(&true)
    }

    fn compile_member_callee_on_receiver(
        &mut self,
        object: ValueRef,
        callee: MemberCallee<'_>,
        receiver_place: Option<&str>,
        args: &[MirArg],
    ) -> std::result::Result<ValueRef, String> {
        match callee {
            MemberCallee::Method { field, dispatch } => {
                self.compile_dispatch(object, field, dispatch, receiver_place, args)
            }
            MemberCallee::Runtime(op) => self.compile_runtime_op(op, object, receiver_place, args),
        }
    }

    /// The loan behind `operand` when it lends one element of a local list
    /// that a runtime helper can read where it lies: a `str`, `list`, `dict`,
    /// or `set` selected by position, with no projection or condition. Such
    /// an element is compared, measured, or searched without a copy.
    fn borrowed_list_element(&self, operand: &Operand) -> Option<BorrowedListElement> {
        let Operand::Place(place) = operand else {
            return None;
        };
        let resolved = self.view_places.get(place)?;
        let [alternative] = resolved.alternatives.as_slice() else {
            return None;
        };
        let [selector] = alternative.elements.as_slice() else {
            return None;
        };
        let lendable = alternative.conditions.is_empty()
            && alternative.union_payloads.is_empty()
            && !alternative.place.contains('.')
            && selector.kind == DirectElementKind::List
            && selector.projection.is_empty()
            && crate::mir_copies::lendable_loop_element(&selector.element_type)
            && matches!(
                self.variable_types.get(&alternative.place),
                Some(DirectType::Opaque(Type::Named(name, _))) if name == "list"
            );
        lendable.then(|| BorrowedListElement {
            list: alternative.place.clone(),
            index: selector.variable,
            element_type: selector.element_type.clone(),
            span: selector.span,
        })
    }

    /// The list handle and position words of a lent element.
    fn load_borrowed_list_element(
        &mut self,
        selection: &BorrowedListElement,
    ) -> std::result::Result<(Val, Val), String> {
        let list = self.load_root(&selection.list)?;
        let index = self.builder.use_var(selection.index);
        Ok((list.values[0], index))
    }

    /// `len()` or `get(key)` on a lent list element, read where it lies (see
    /// `borrowed_list_element`): a peephole on the resolved `len` of a
    /// `str`, `list`, `dict`, or `set` and the resolved `get` of a `dict`.
    /// Returns `None` for any other call.
    fn compile_borrowed_element_member_call(
        &mut self,
        object: &Operand,
        callee: MemberCallee<'_>,
        args: &[MirArg],
    ) -> std::result::Result<Option<ValueRef>, String> {
        let Some(selection) = self.borrowed_list_element(object) else {
            return Ok(None);
        };
        let op = callee.op();
        let key = match (op, args) {
            (Some(RuntimeOp::Dict(DictOp::Get | DictOp::GetStr | DictOp::GetInt64)), [key]) => {
                Some(key)
            }
            (
                Some(
                    RuntimeOp::Str(StrOp::Len)
                    | RuntimeOp::List(ListOp::Len)
                    | RuntimeOp::Dict(DictOp::Len)
                    | RuntimeOp::Set(SetOp::Len),
                ),
                [],
            ) => None,
            _ => return Ok(None),
        };
        // MIR resolved the call to the built-in op, so a trait method of
        // the same name on the element's type does not run.
        if args
            .iter()
            .any(|argument| argument.name.is_some() || argument.writeback_place.is_some())
        {
            return Ok(None);
        }
        let (line, column) = self.span_operands(selection.span);
        let Some(key) = key else {
            let (list, index) = self.load_borrowed_list_element(&selection)?;
            let inst = self.builder.call(
                self.runtime[RuntimeFn::VecElementLen],
                &[list, index, line, column],
            );
            return Ok(Some(ValueRef {
                values: vec![inst.results[0]],
                ty: DirectType::Scalar(ScalarKind::Int64),
            }));
        };
        let Type::Named(_, entry_types) = selection.element_type.clone() else {
            return Ok(None);
        };
        let inst = match (op, &key.value) {
            (Some(RuntimeOp::Dict(DictOp::GetStr)), Operand::String(text)) => {
                // A literal `str` key is looked up without a box.
                let (key_ptr, key_len) = self.string_constant(text.as_bytes())?;
                let (list, index) = self.load_borrowed_list_element(&selection)?;
                let function = self.lookup_function(
                    self.runtime[RuntimeFn::VecElementMapGetStr],
                    RuntimeFn::VecElementMapGetStrPayload,
                    RuntimeFn::VecElementMapGetStrLent,
                    true,
                );
                self.builder
                    .call(function, &[list, index, key_ptr, key_len, line, column])
            }
            _ => {
                let key_direct_ty = ensure_direct_type(&entry_types[0], &self.classes, "dict key")?;
                let key = self.load_operand_as_opaque_direct(&key.value, &key_direct_ty)?;
                let (list, index) = self.load_borrowed_list_element(&selection)?;
                let function = self.lookup_function(
                    self.runtime[RuntimeFn::VecElementMapGet],
                    RuntimeFn::VecElementMapGetPayload,
                    RuntimeFn::VecElementMapGetLent,
                    true,
                );
                self.builder
                    .call(function, &[list, index, key.values[0], line, column])
            }
        };
        Ok(Some(self.owned_opaque_result(
            inst.results,
            optional_type(entry_types[1].clone()),
        )))
    }

    /// Lowers the move out of MIR `Lend`: the value at `place` moves into
    /// its temporary. A field of an instance is taken by name, and an
    /// element a loan selects is taken from its collection. Any other place
    /// is copied, and `EndLend` stores the copy back.
    fn take_lent_place(&mut self, place: &str) -> std::result::Result<ValueRef, String> {
        let resolved = self.resolve_view_place(place)?;
        let alternative = match resolved.alternatives.as_slice() {
            [alternative]
                if alternative.conditions.is_empty() && alternative.union_payloads.is_empty() =>
            {
                alternative
            }
            _ => return self.load_place(place),
        };
        if alternative.elements.is_empty() {
            return self.take_place(&alternative.place.clone());
        }
        let element_type = alternative
            .elements
            .last()
            .map(|selector| selector.element_type.clone())
            .ok_or("direct backend lent an element view without a selection")?;
        let elements = alternative.elements.clone();
        let (collection, projection) = self.load_element_path_root(&alternative.place.clone())?;
        let path = self.element_path_operands(&projection, &elements)?;
        let inst = self.builder.call(
            self.runtime[RuntimeFn::ElementPathTake],
            &[
                collection.values[0],
                path.path_ptr,
                path.path_len,
                path.selectors_ptr,
                path.count,
                path.line,
                path.column,
            ],
        );
        let taken = self.owned_opaque_result(inst.results, element_type);
        let ty = self.type_of_place(place)?;
        self.coerce_value(taken, &ty)
    }

    /// Runs the dispatch MIR resolved for `.field` on a loaded receiver.
    fn compile_dispatch(
        &mut self,
        object: ValueRef,
        field: &str,
        dispatch: &MethodDispatch,
        receiver_place: Option<&str>,
        args: &[MirArg],
    ) -> std::result::Result<ValueRef, String> {
        match dispatch {
            MethodDispatch::Static { function } => {
                self.compile_method_call(function, object, receiver_place, args)
            }
            MethodDispatch::Runtime(op) => {
                let args = bind_member_call_args(*op, args)?;
                self.compile_runtime_op(*op, object, receiver_place, &args)
            }
            MethodDispatch::Nothing => {
                let _ = self.ensure_opaque(object)?;
                Ok(unit_value(&mut *self.builder))
            }
            MethodDispatch::Union(arms) => {
                let DirectType::Union(union) = object.ty.clone() else {
                    return Err(format!(
                        "direct backend received a union dispatch of `.{field}` on `{}`",
                        render_direct_type(&object.ty)
                    ));
                };
                self.compile_union_member_call(
                    &union,
                    &object.values,
                    field,
                    arms,
                    receiver_place,
                    args,
                )
            }
            MethodDispatch::UnionClone => {
                let DirectType::Union(union) = object.ty.clone() else {
                    return Err(format!(
                        "direct backend received a union clone on `{}`",
                        render_direct_type(&object.ty)
                    ));
                };
                if !args.is_empty() {
                    return Err("`clone` does not take arguments".to_string());
                }
                self.compile_union_clone(&union, &object.values)
            }
            MethodDispatch::Candidates { candidates, erased } => {
                if *erased {
                    self.compile_borrowed_receiver_dispatch(
                        object,
                        receiver_place,
                        args,
                        candidates,
                    )
                } else {
                    self.compile_candidate_dispatch(object, receiver_place, args, candidates)
                }
            }
        }
    }

    fn compile_construct(
        &mut self,
        class_name: &str,
        fields: &[crate::mir::MirFieldInit],
        target: &DirectType,
    ) -> std::result::Result<ValueRef, String> {
        let ty = match target {
            DirectType::PlainClass(class) if class.class_name == class_name => target.clone(),
            DirectType::Opaque(Type::Named(name, _)) if name == class_name => target.clone(),
            _ => ensure_direct_type(
                &Type::named(class_name),
                &self.classes,
                &format!("class `{}`", class_name),
            )?,
        };
        match &ty {
            DirectType::Union(_) | DirectType::Callable(_) => Err(format!(
                "direct backend cannot construct `{}` as a class: it is not a class type",
                class_name
            )),
            DirectType::PlainClass(class_ty) => {
                let mut by_name = HashMap::new();
                for field in fields {
                    by_name.insert(field.name.clone(), field.value.clone());
                }

                let mut values = Vec::new();
                for field in &class_ty.fields {
                    // An initializer's object starts with only its defaulted
                    // fields; definite initialization proves that the body
                    // writes every other field before any read, so the slot
                    // holds inert zero words until then (ADR-0059).
                    let Some(operand) = by_name.get(&field.name) else {
                        values.extend(field.ty.zero_values(&mut *self.builder));
                        continue;
                    };
                    let value = self.load_operand_for_target(operand, &field.ty)?;
                    let coerced = self.coerce_value(value, &field.ty)?;
                    values.extend(coerced.values);
                }

                Ok(ValueRef {
                    values,
                    ty: ty.clone(),
                })
            }
            DirectType::Opaque(target_ty) => {
                self.compile_opaque_construct(class_name, fields, target_ty)
            }
            DirectType::Scalar(_) => Err(format!(
                "direct backend could not construct non-class type `{}`",
                class_name
            )),
        }
    }

    fn call_result_type(&self, name: &str) -> std::result::Result<DirectType, String> {
        self.function_return_types.get(name).cloned().ok_or(format!(
            "direct backend does not know return type for `{}`",
            name
        ))
    }

    fn projected_element_type(&self, element_type: &Type, projection: &str) -> Option<Type> {
        let mut ty = ensure_direct_type(element_type, &self.classes, "element projection").ok()?;
        for field in projection.split('.') {
            ty = direct_field_type_with_enums(&ty, field, &self.classes, &self.enums)?;
        }
        Some(direct_type_to_type(&ty))
    }

    fn resolve_view_place(&self, place: &str) -> std::result::Result<DirectViewPlace, String> {
        let (root, projection) = place.split_once('.').unwrap_or((place, ""));
        let mut resolved = if let Some(source) = self.view_places.get(root) {
            let mut projected = source.clone().project(projection);
            // A field reached through an element view has the field's type,
            // not the element's: `t.members.len()` must dispatch on the list.
            if !projection.is_empty() {
                for selector in projected
                    .alternatives
                    .iter_mut()
                    .filter_map(|alternative| alternative.elements.last_mut())
                {
                    if let Some(ty) =
                        self.projected_element_type(&selector.element_type, projection)
                    {
                        selector.element_type = ty;
                    }
                }
            }
            projected
        } else {
            DirectViewPlace::static_place(place.to_string())
        };
        for alternative in &mut resolved.alternatives {
            let mut segments = alternative.place.split('.');
            let mut base = segments.next().unwrap_or_default().to_string();
            let Ok(mut ty) = self.local_type(&base) else {
                continue;
            };
            alternative.union_payloads.clear();
            for segment in segments {
                if let DirectType::Opaque(Type::Union(union)) = &ty {
                    if segment == crate::native_runtime::DIRECT_UNION_ACTIVE_PAYLOAD_PROJECTION {
                        // A trait method's receiver write-back selects the
                        // active member at run time; there is no static tag
                        // metadata to retain past this point.
                        break;
                    }
                    let index = crate::mir::union_payload_projection_index(segment)
                        .filter(|index| *index < union.members.len())
                        .ok_or_else(|| format!("invalid union payload projection `{segment}`"))?;
                    alternative.union_payloads.push(DirectUnionPayload {
                        base: base.clone(),
                        union_type: Type::Union(union.clone()),
                        member_index: index,
                    });
                }
                let Some(projected) =
                    direct_field_type_with_enums(&ty, segment, &self.classes, &self.enums)
                else {
                    break;
                };
                ty = projected;
                base.push('.');
                base.push_str(segment);
            }
        }
        Ok(resolved)
    }

    fn type_of_place(&self, place: &str) -> std::result::Result<DirectType, String> {
        let (virtual_root, virtual_projection) = place.split_once('.').unwrap_or((place, ""));
        if self.view_places.contains_key(virtual_root) {
            if let Some(mut ty) = self.variable_types.get(virtual_root).cloned() {
                for field in virtual_projection
                    .split('.')
                    .filter(|segment| !segment.is_empty())
                {
                    ty = direct_field_type_with_enums(&ty, field, &self.classes, &self.enums)
                        .ok_or(format!(
                            "direct backend does not know field `{}` on `{}`",
                            field,
                            render_direct_type(&ty)
                        ))?;
                }
                return Ok(ty);
            }
        }
        let resolved = self.resolve_view_place(place)?;
        let place = resolved
            .alternatives
            .first()
            .ok_or_else(|| format!("direct backend view `{place}` has no place alternatives"))?
            .place
            .as_str();
        let mut segments = place.split('.');
        let root = segments
            .next()
            .ok_or("direct backend encountered an empty place".to_string())?;
        let mut ty = self.local_type(root)?;
        for field in segments {
            ty = direct_field_type_with_enums(&ty, field, &self.classes, &self.enums).ok_or(
                format!(
                    "direct backend does not know field `{}` on `{}`",
                    field,
                    render_direct_type(&ty)
                ),
            )?;
        }
        Ok(ty)
    }

    fn load_operand(&mut self, operand: &Operand) -> std::result::Result<ValueRef, String> {
        if let Some(retained) = self.take_retained(operand) {
            return Ok(retained);
        }
        match operand {
            Operand::Place(place) => self.load_place(place),
            Operand::MovePlace(place) => self.take_place(place),
            Operand::Function { name, signature } => {
                let key = CallableShapeKey {
                    function: name.clone(),
                    captures: 0,
                    consuming: false,
                    contract: signature.to_string(),
                };
                let shape = self.callable_shapes.get(&key).cloned().ok_or(format!(
                    "direct backend does not know a callable shape for `{name}`"
                ))?;
                let global = self.builder.import_data(shape.descriptor);
                let descriptor = self.builder.symbol_value(types::I64, global);
                let zero = self.builder.iconst(types::I64, 0);
                Ok(ValueRef {
                    values: vec![descriptor, zero, zero, zero],
                    ty: DirectType::Callable(DirectCallableType::new(signature.as_ref().clone())),
                })
            }
            Operand::Int(value) => {
                if let Ok(narrowed) = i64::try_from(*value) {
                    return Ok(ValueRef {
                        values: vec![self.builder.iconst(types::I64, narrowed)],
                        ty: DirectType::Scalar(ScalarKind::Int64),
                    });
                }
                let (ptr, len) = self.string_constant(value.to_string().as_bytes())?;
                let inst = self
                    .builder
                    .call(self.runtime[RuntimeFn::BoxUintLiteral], &[ptr, len]);
                Ok(self.owned_opaque_result(inst.results, Type::named("Unknown")))
            }
            Operand::Float(value) => Ok(ValueRef {
                values: vec![self.builder.f64const(*value)],
                ty: DirectType::Scalar(ScalarKind::Float64),
            }),
            Operand::String(value) => {
                let (ptr, len) = self.string_constant(value.as_bytes())?;
                let inst = self
                    .builder
                    .call(self.runtime[RuntimeFn::StringLiteral], &[ptr, len]);
                Ok(self.owned_opaque_result(inst.results, Type::named("str")))
            }
            Operand::Duration(value) => {
                let low = self.builder.iconst(types::I64, *value as i64);
                let high = self.builder.iconst(types::I64, (*value >> 64) as i64);
                let inst = self
                    .builder
                    .call(self.runtime[RuntimeFn::DurationLiteral], &[low, high]);
                Ok(self.owned_opaque_result(inst.results, Type::named("Duration")))
            }
            Operand::Bool(value) => Ok(ValueRef {
                values: vec![self.builder.iconst(types::I64, if *value { 1 } else { 0 })],
                ty: DirectType::Scalar(ScalarKind::Bool),
            }),
            Operand::Unit => Ok(unit_value(&mut *self.builder)),
            Operand::Default(default) => self.load_default_arg(*default),
        }
    }

    fn operand_integer_kind(&self, operand: &Operand) -> Option<ScalarKind> {
        let kind = match operand {
            Operand::Place(place) | Operand::MovePlace(place) => {
                self.type_of_place(place).ok()?.scalar_kind()?
            }
            Operand::Int(_)
            | Operand::Function { .. }
            | Operand::Float(_)
            | Operand::String(_)
            | Operand::Duration(_)
            | Operand::Bool(_)
            | Operand::Unit
            | Operand::Default(_) => return None,
        };
        kind.is_integer().then_some(kind)
    }

    fn load_operand_with_integer_hint(
        &mut self,
        operand: &Operand,
        hint: Option<ScalarKind>,
    ) -> std::result::Result<ValueRef, String> {
        if let (Operand::Int(value), Some(kind)) = (operand, hint) {
            let raw = match kind {
                ScalarKind::Int32 | ScalarKind::Int64 => i64::try_from(*value).ok(),
                ScalarKind::Uint64 => u64::try_from(*value).ok().map(|value| value as i64),
                ScalarKind::Float32 | ScalarKind::Float64 | ScalarKind::Bool | ScalarKind::Unit => {
                    None
                }
            };
            if let Some(raw) = raw {
                return Ok(ValueRef {
                    values: vec![self.builder.iconst(types::I64, raw)],
                    ty: DirectType::Scalar(kind),
                });
            }
        }
        self.load_operand(operand)
    }

    fn load_operand_for_target(
        &mut self,
        operand: &Operand,
        target: &DirectType,
    ) -> std::result::Result<ValueRef, String> {
        let hint = target.scalar_kind().filter(|kind| kind.is_integer());
        self.load_operand_with_integer_hint(operand, hint)
    }

    fn load_operand_as_opaque_direct(
        &mut self,
        operand: &Operand,
        expected_ty: &DirectType,
    ) -> std::result::Result<ValueRef, String> {
        let loaded = self.load_operand_for_target(operand, expected_ty)?;
        let coerced = self.coerce_value(loaded, expected_ty)?;
        self.ensure_opaque(coerced)
    }

    fn load_place(&mut self, place: &str) -> std::result::Result<ValueRef, String> {
        let resolved = self.resolve_view_place(place)?;
        if resolved.alternatives.len() == 1 && resolved.alternatives[0].conditions.is_empty() {
            return self.load_alternative(&resolved.alternatives[0]);
        }
        self.load_selected_view_place(place, resolved)
    }

    /// Begins an element or entry loan on the direct backend: the selector
    /// is evaluated once into its own variable (a non-Copy key is owned by
    /// the loan and released with it), the collection is probed so a
    /// missing position or key traps now, and every alternative of the
    /// collection's view place gains the selection. An unchecked loan of a
    /// local list element skips the probe.
    fn begin_element_loan(
        &mut self,
        loan: &str,
        source: &str,
        selector: &Operand,
        projection: &str,
        span: Span,
        checked: bool,
    ) -> std::result::Result<(), String> {
        let collection_ty = direct_type_to_type(&self.type_of_place(source)?);
        let (kind, selector_ty) = match &collection_ty {
            Type::Named(name, args) if name == "list" && args.len() == 1 => (
                DirectElementKind::List,
                DirectType::Scalar(ScalarKind::Int64),
            ),
            Type::Named(name, args) if name == "dict" && args.len() == 2 => (
                DirectElementKind::Dict,
                ensure_direct_type(&args[0], &self.classes, "dictionary key")?,
            ),
            other => {
                return Err(format!(
                    "direct backend cannot begin element loan `{loan}` on `{other}`"
                ));
            }
        };
        // The loan's local carries the type the selection reaches.
        let element_type = direct_type_to_type(&self.local_type(loan)?);
        let loaded = self.load_operand_for_target(selector, &selector_ty)?;
        let selected = self.coerce_value(loaded, &selector_ty)?;
        let words = self.transfer_values(&selected);
        let abi = selector_ty.abi_types()[0];
        let variable = Var(self.next_variable_index as u32);
        self.next_variable_index += 1;
        self.builder.declare_var(variable, abi);
        self.builder.def_var(variable, words[0]);
        if matches!(selector_ty, DirectType::Opaque(_)) {
            self.view_element_keys
                .entry(loan.to_string())
                .or_default()
                .push(variable);
        }
        let selector = DirectElementSelector {
            variable,
            selector_ty,
            element_type,
            kind,
            projection: projection.to_string(),
            span,
        };
        let mut resolved = self.resolve_view_place(source)?;
        for alternative in &mut resolved.alternatives {
            alternative.elements.push(selector.clone());
        }
        self.view_places.insert(loan.to_string(), resolved);
        // Bounds or presence are checked once at creation. A single
        // unconditional selection is probed in place; otherwise probe the
        // selected slot through the loan and drop the copy.
        if let Some(selection) = self.borrowed_list_element(&Operand::Place(loan.to_string())) {
            if !checked {
                return Ok(());
            }
            let (list, index) = self.load_borrowed_list_element(&selection)?;
            let (line, column) = self.span_operands(selection.span);
            self.builder.call(
                self.runtime[RuntimeFn::VecCheckIndex],
                &[list, index, line, column],
            );
            return Ok(());
        }
        let resolved = self.resolve_view_place(loan)?;
        if let [alternative] = resolved.alternatives.as_slice() {
            if alternative.conditions.is_empty() && !alternative.elements.is_empty() {
                let (collection, projection) = self.load_element_path_root(&alternative.place)?;
                let path = self.element_path_operands(&projection, &alternative.elements)?;
                let inst = self.builder.call(
                    self.runtime[RuntimeFn::ElementPathProbe],
                    &[
                        collection.values[0],
                        path.path_ptr,
                        path.path_len,
                        path.selectors_ptr,
                        path.count,
                        path.line,
                        path.column,
                    ],
                );
                self.release_opaque_handle(inst.results[0]);
                return Ok(());
            }
        }
        let baseline = self.layout_scratch.clone();
        let probe = self.load_place(loan)?;
        drop(probe);
        self.release_temporary_owned_since(&baseline);
        Ok(())
    }

    /// A span as the line and column words the runtime's indexing helpers
    /// take.
    fn span_operands(&mut self, span: Span) -> (Val, Val) {
        let line = self.builder.iconst(types::I64, span.line as i64);
        let column = self.builder.iconst(types::I64, span.column as i64);
        (line, column)
    }

    /// Loads one view alternative: its static place, then the chain of
    /// element and entry selections the loan applied to it, in one runtime
    /// walk that clones only the value reached.
    fn load_alternative(
        &mut self,
        alternative: &DirectViewAlternative,
    ) -> std::result::Result<ValueRef, String> {
        let Some(last) = alternative.elements.last() else {
            return self.load_static_place(&alternative.place);
        };
        let element_type = last.element_type.clone();
        let (collection, projection) = self.load_element_path_root(&alternative.place)?;
        let path = self.element_path_operands(&projection, &alternative.elements)?;
        let inst = self.builder.call(
            self.runtime[RuntimeFn::ElementPathLoad],
            &[
                collection.values[0],
                path.path_ptr,
                path.path_len,
                path.selectors_ptr,
                path.count,
                path.line,
                path.column,
            ],
        );
        Ok(self.owned_opaque_result(inst.results, element_type))
    }

    /// Stores through one view alternative: a static place directly, or the
    /// value its selection chain reaches inside the collection, replaced in
    /// place (never a clone written back).
    fn store_alternative(
        &mut self,
        alternative: &DirectViewAlternative,
        value: ValueRef,
    ) -> std::result::Result<(), String> {
        if alternative.elements.is_empty() {
            return self.store_static_place(&alternative.place, value);
        }
        let (collection, projection) = self.load_element_path_root(&alternative.place)?;
        let path = self.element_path_operands(&projection, &alternative.elements)?;
        let stored = self.ensure_opaque(value)?;
        let stored = self.transfer_owned_opaque_value(&stored);
        let result = self.builder.call(
            self.runtime[RuntimeFn::ElementPathStore],
            &[
                collection.values[0],
                path.path_ptr,
                path.path_len,
                path.selectors_ptr,
                path.count,
                stored,
                path.line,
                path.column,
            ],
        );
        self.release_opaque_handle(result.results[0]);
        Ok(())
    }

    /// Keeps the first runtime-object handle in a collection place and
    /// leaves its child projections for the element-path walker. Extracting
    /// an opaque field or tuple slot first would clone that collection, so a
    /// later element store would update a detached value. Inline union and
    /// plain-class projections only select words and may be traversed here.
    fn load_element_path_root(
        &mut self,
        place: &str,
    ) -> std::result::Result<(ValueRef, String), String> {
        let (root, mut projection) = place.split_once('.').unwrap_or((place, ""));
        let mut value = self.load_root(root)?;
        while !projection.is_empty() && !matches!(value.ty, DirectType::Opaque(_)) {
            let (field, rest) = projection.split_once('.').unwrap_or((projection, ""));
            value = self.extract_field(value, field)?;
            projection = rest;
        }
        Ok((self.ensure_opaque(value)?, projection.to_string()))
    }

    /// The runtime operands of a selection chain: its path (`[i]`, `[k]`,
    /// and projection steps), a stack buffer of the selector words (list
    /// positions, and dictionary keys as borrowed handles, a scalar key
    /// boxed into a statement temporary), and the innermost index
    /// expression's position for traps.
    fn element_path_operands(
        &mut self,
        projection: &str,
        elements: &[DirectElementSelector],
    ) -> std::result::Result<DirectElementPathOperands, String> {
        let mut path = projection.to_string();
        for selector in elements {
            if !path.is_empty() {
                path.push('.');
            }
            path.push_str(match selector.kind {
                DirectElementKind::List => "[i]",
                DirectElementKind::Dict => "[k]",
            });
            if !selector.projection.is_empty() {
                path.push('.');
                path.push_str(&selector.projection);
            }
        }
        let (path_ptr, path_len) = self.string_constant(path.as_bytes())?;
        let slot = self
            .builder
            .create_sized_stack_slot((8 * elements.len()) as u32, 3);
        let selectors_ptr = self.builder.stack_addr(types::I64, slot, 0);
        for (index, selector) in elements.iter().enumerate() {
            let selected = self.builder.use_var(selector.variable);
            let word = match selector.kind {
                DirectElementKind::List => selected,
                DirectElementKind::Dict => {
                    let key = ValueRef {
                        values: vec![selected],
                        ty: selector.selector_ty.clone(),
                    };
                    self.ensure_opaque(key)?.values[0]
                }
            };
            self.builder
                .store(MemFlags::new(), word, selectors_ptr, (index as i32) * 8);
        }
        let count = self.builder.iconst(types::I64, elements.len() as i64);
        let span = elements
            .last()
            .map(|selector| selector.span)
            .unwrap_or(Span::new(0, 0));
        let (line, column) = self.span_operands(span);
        Ok(DirectElementPathOperands {
            path_ptr,
            path_len,
            selectors_ptr,
            count,
            line,
            column,
        })
    }

    fn load_static_place(&mut self, place: &str) -> std::result::Result<ValueRef, String> {
        let mut segments = place.split('.');
        let root = segments
            .next()
            .ok_or("direct backend encountered an empty place".to_string())?;
        let mut value = self.load_root(root)?;
        for field in segments {
            value = self.extract_field(value, field)?;
        }
        Ok(value)
    }

    fn view_alternative_condition(&mut self, alternative: &DirectViewAlternative) -> Val {
        let mut conditions = alternative.conditions.iter();
        let Some((selector, expected)) = conditions.next() else {
            return self.builder.iconst(types::I64, 1);
        };
        let selector = self.builder.use_var(*selector);
        let mut matches = self.builder.icmp_imm(IntCC::Equal, selector, *expected);
        for (selector, expected) in conditions {
            let selector = self.builder.use_var(*selector);
            let next = self.builder.icmp_imm(IntCC::Equal, selector, *expected);
            matches = self.builder.band(matches, next);
        }
        matches
    }

    fn emit_returned_view_projection(
        &mut self,
        loan: &str,
        origin: &str,
    ) -> std::result::Result<(), String> {
        let sources = self.resolve_view_place(loan)?;
        let origins = self.resolve_view_place(origin)?;
        let mut alternatives = Vec::<(String, Vec<(Var, i64)>, Vec<DirectElementSelector>)>::new();
        for source in &sources.alternatives {
            for origin in &origins.alternatives {
                let projection = if source.place == origin.place {
                    Some(String::new())
                } else {
                    source
                        .place
                        .strip_prefix(&format!("{}.", origin.place))
                        .map(str::to_string)
                };
                let Some(mut projection) = projection else {
                    continue;
                };
                // A returned element or entry view spells each selection
                // made inside the origin as `[*]` and hands its selector
                // over at run time (ADR-0061 A6).
                if !source.elements.starts_with(&origin.elements) {
                    continue;
                }
                let steps = source.elements[origin.elements.len()..].to_vec();
                for step in &steps {
                    for segment in std::iter::once(crate::mir::RETURNED_ELEMENT_STEP)
                        .chain(step.projection.split('.').filter(|part| !part.is_empty()))
                    {
                        if !projection.is_empty() {
                            projection.push('.');
                        }
                        projection.push_str(segment);
                    }
                }
                let mut conditions = origin.conditions.clone();
                let mut compatible = true;
                for condition in &source.conditions {
                    if let Some((_, expected)) = conditions
                        .iter()
                        .find(|(selector, _)| selector == &condition.0)
                    {
                        if *expected != condition.1 {
                            compatible = false;
                            break;
                        }
                    } else {
                        conditions.push(*condition);
                    }
                }
                if compatible
                    && !alternatives
                        .iter()
                        .any(|candidate| candidate.0 == projection && candidate.1 == conditions)
                {
                    alternatives.push((projection, conditions, steps));
                }
            }
        }
        if alternatives.is_empty() {
            return Err(format!(
                "direct returned loan `{loan}` has no projection within origin `{origin}`"
            ));
        }

        let merge = self.builder.create_block();
        let alternative_count = alternatives.len();
        for (index, (projection, conditions, steps)) in alternatives.into_iter().enumerate() {
            if index + 1 < alternative_count {
                let selected = self.view_alternative_condition(&DirectViewAlternative {
                    place: String::new(),
                    conditions,
                    union_payloads: Vec::new(),
                    elements: Vec::new(),
                });
                let set_block = self.builder.create_block();
                let next_block = self.builder.create_block();
                self.builder.brif(selected, set_block, &[], next_block, &[]);
                self.builder.switch_to_block(set_block);
                self.set_returned_view_projection_value(&projection)?;
                self.push_returned_view_selectors(&steps);
                self.builder.jump(merge, &[]);
                self.builder.seal_block(set_block);
                self.builder.switch_to_block(next_block);
                self.builder.seal_block(next_block);
            } else {
                self.set_returned_view_projection_value(&projection)?;
                self.push_returned_view_selectors(&steps);
                self.builder.jump(merge, &[]);
            }
        }
        self.builder.switch_to_block(merge);
        self.builder.seal_block(merge);
        Ok(())
    }

    /// Hands the selector of each returned element step to the caller as one
    /// word. A string key is retained, so the caller owns its own reference.
    fn push_returned_view_selectors(&mut self, steps: &[DirectElementSelector]) {
        for step in steps {
            let mut word = self.builder.use_var(step.variable);
            if matches!(step.selector_ty, DirectType::Opaque(_)) {
                word = self.retain_opaque_handle(word);
            } else {
                // A float key travels as its bit pattern.
                let word_ty = self.builder.value_type(word);
                if word_ty == types::F64 {
                    word = self.builder.bitcast(types::I64, word);
                } else if word_ty == types::F32 {
                    word = self.builder.bitcast(types::I32, word);
                }
                if self.builder.value_type(word) != types::I64 {
                    word = self.builder.uextend(types::I64, word);
                }
            }
            self.builder
                .call(self.runtime[RuntimeFn::PushReturnedViewSelector], &[word]);
        }
    }

    /// Rebuilds a returned element or entry view in the caller: each `[*]`
    /// step of `projection` becomes an element selection whose selector is
    /// the word the callee handed over.
    fn returned_element_alternative(
        &mut self,
        loan: &str,
        origin: &DirectViewAlternative,
        projection: &str,
        conditions: Vec<(Var, i64)>,
        selector_words: &[Val],
    ) -> std::result::Result<DirectViewAlternative, String> {
        let mut groups = projection.split(crate::mir::RETURNED_ELEMENT_STEP);
        let prefix = groups
            .next()
            .unwrap_or_default()
            .trim_end_matches('.')
            .to_string();
        let mut place = origin.place.clone();
        let mut elements = origin.elements.clone();
        let mut ty = match elements.last() {
            Some(selector) => ensure_direct_type(
                &selector.element_type,
                &self.classes,
                "returned element origin",
            )?,
            None => self.type_of_place(&place)?,
        };
        for field in prefix.split('.').filter(|part| !part.is_empty()) {
            ty = direct_field_type_with_enums(&ty, field, &self.classes, &self.enums).ok_or(
                format!("direct backend does not know field `{field}` on a returned view origin"),
            )?;
        }
        if !prefix.is_empty() {
            match elements.last_mut() {
                Some(selector) if selector.projection.is_empty() => {
                    selector.projection = prefix.clone()
                }
                Some(selector) => selector.projection = format!("{}.{prefix}", selector.projection),
                None => place = format!("{place}.{prefix}"),
            }
            if let Some(selector) = elements.last_mut() {
                selector.element_type = direct_type_to_type(&ty);
            }
        }
        for (step, fields) in groups.enumerate() {
            let collection = direct_type_to_type(&ty);
            let (kind, selector_ty, element) = match &collection {
                Type::Named(name, args) if name == "list" && args.len() == 1 => (
                    DirectElementKind::List,
                    DirectType::Scalar(ScalarKind::Int64),
                    args[0].clone(),
                ),
                Type::Named(name, args) if name == "dict" && args.len() == 2 => (
                    DirectElementKind::Dict,
                    ensure_direct_type(&args[0], &self.classes, "dictionary key")?,
                    args[1].clone(),
                ),
                other => {
                    return Err(format!(
                        "direct returned loan `{loan}` selects an element inside `{other}`"
                    ))
                }
            };
            let fields = fields.trim_matches('.').to_string();
            ty = ensure_direct_type(&element, &self.classes, "returned element")?;
            for field in fields.split('.').filter(|part| !part.is_empty()) {
                ty = direct_field_type_with_enums(&ty, field, &self.classes, &self.enums).ok_or(
                    format!("direct backend does not know field `{field}` in a returned element"),
                )?;
            }
            let word = *selector_words.get(step).ok_or(format!(
                "direct returned loan `{loan}` has no selector for element step {step}"
            ))?;
            let abi = selector_ty.abi_types()[0];
            let word = if abi == types::I64 {
                word
            } else if abi == types::F64 {
                self.builder.bitcast(types::F64, word)
            } else if abi == types::F32 {
                let bits = self.builder.ireduce(types::I32, word);
                self.builder.bitcast(types::F32, bits)
            } else {
                self.builder.ireduce(abi, word)
            };
            let variable = Var(self.next_variable_index as u32);
            self.next_variable_index += 1;
            self.builder.declare_var(variable, abi);
            self.builder.def_var(variable, word);
            if matches!(selector_ty, DirectType::Opaque(_)) {
                self.view_element_keys
                    .entry(loan.to_string())
                    .or_default()
                    .push(variable);
            }
            elements.push(DirectElementSelector {
                variable,
                selector_ty,
                element_type: direct_type_to_type(&ty),
                kind,
                projection: fields,
                span: Span::new(0, 0),
            });
        }
        Ok(DirectViewAlternative {
            place,
            conditions,
            union_payloads: origin.union_payloads.clone(),
            elements,
        })
    }

    fn set_returned_view_projection_value(
        &mut self,
        projection: &str,
    ) -> std::result::Result<(), String> {
        let (projection_ptr, projection_len) = self.string_constant(projection.as_bytes())?;
        self.builder.call(
            self.runtime[RuntimeFn::SetReturnedViewProjection],
            &[projection_ptr, projection_len],
        );
        Ok(())
    }

    fn load_selected_view_place(
        &mut self,
        view: &str,
        resolved: DirectViewPlace,
    ) -> std::result::Result<ValueRef, String> {
        let target_ty = self.type_of_place(view)?;
        let merge = self.builder.create_block();
        for abi in target_ty.abi_types() {
            self.builder.append_block_param(merge, abi);
        }

        let caller_owned = self.layout_scratch.clone();
        let alternative_count = resolved.alternatives.len();
        let mut merged_owned = None;
        for (index, alternative) in resolved.alternatives.into_iter().enumerate() {
            if index + 1 < alternative_count {
                let selected = self.view_alternative_condition(&alternative);
                let load_block = self.builder.create_block();
                let next_block = self.builder.create_block();
                self.builder
                    .brif(selected, load_block, &[], next_block, &[]);
                self.builder.switch_to_block(load_block);
                self.layout_scratch = caller_owned.clone();
                let value = self.load_alternative(&alternative)?;
                let value = self.coerce_value(value, &target_ty)?;
                let value_is_owned = self.temporary_owns_opaque(&value);
                if let Some(expected) = merged_owned {
                    if expected != value_is_owned {
                        return Err(format!(
                            "direct view `{view}` alternatives disagree on value ownership"
                        ));
                    }
                } else {
                    merged_owned = Some(value_is_owned);
                }
                self.clear_temporary_opaque_owned(&value);
                self.release_temporary_owned_since(&caller_owned);
                self.builder.jump(merge, &value.values);
                self.builder.seal_block(load_block);
                self.builder.switch_to_block(next_block);
                self.builder.seal_block(next_block);
            } else {
                self.layout_scratch = caller_owned.clone();
                let value = self.load_alternative(&alternative)?;
                let value = self.coerce_value(value, &target_ty)?;
                let value_is_owned = self.temporary_owns_opaque(&value);
                if let Some(expected) = merged_owned {
                    if expected != value_is_owned {
                        return Err(format!(
                            "direct view `{view}` alternatives disagree on value ownership"
                        ));
                    }
                } else {
                    merged_owned = Some(value_is_owned);
                }
                self.clear_temporary_opaque_owned(&value);
                self.release_temporary_owned_since(&caller_owned);
                self.builder.jump(merge, &value.values);
            }
        }
        self.builder.switch_to_block(merge);
        self.builder.seal_block(merge);
        self.restore_scratch_after_branches(caller_owned);
        let value = ValueRef {
            values: self.builder.block_params(merge),
            ty: target_ty,
        };
        if merged_owned == Some(true) {
            self.mark_temporary_opaque_owned(&value);
        }
        Ok(value)
    }

    fn take_place(&mut self, place: &str) -> std::result::Result<ValueRef, String> {
        let segments = place.split('.').collect::<Vec<_>>();
        let Some(root) = segments.first().copied() else {
            return Err("direct backend encountered an empty move place".to_string());
        };
        let root_ty = self.local_type(root)?;
        if segments.len() == 1 {
            let vars = self.local_vars(root)?;
            let values = vars
                .iter()
                .map(|var| self.builder.use_var(*var))
                .collect::<Vec<_>>();
            for (var, abi) in vars.into_iter().zip(root_ty.abi_types()) {
                let zero = if abi == types::F64 {
                    self.builder.f64const(0.0)
                } else {
                    self.builder.iconst(abi, 0)
                };
                self.builder.def_var(var, zero);
            }
            let moved = ValueRef {
                values,
                ty: root_ty,
            };
            self.mark_temporary_opaque_owned(&moved);
            self.mark_temporary_union_owned(&moved);
            self.mark_temporary_callable_owned(&moved);
            return Ok(moved);
        }

        let moved_ty = self.type_of_place(place)?;
        if matches!(&root_ty, DirectType::Opaque(_)) {
            let root_value = self.load_root(root)?;
            let path = segments[1..].join(".");
            let (field_ptr, field_len) = self.string_constant(path.as_bytes())?;
            let inst = self.builder.call(
                self.runtime[RuntimeFn::InstanceTakeField],
                &[root_value.values[0], field_ptr, field_len],
            );
            let moved = ValueRef {
                values: inst.results,
                ty: DirectType::Opaque(direct_type_to_type(&moved_ty)),
            };
            self.mark_temporary_opaque_owned(&moved);
            return self.coerce_value(moved, &moved_ty);
        }

        let vars = self.local_vars(root)?;
        let mut current_ty = root_ty;
        let mut start = 0usize;
        let mut end = vars.len();
        for field in &segments[1..] {
            let (field_start, field_end, field_ty) =
                required_direct_field_slice(&current_ty, field)?;
            start += field_start;
            end = start + (field_end - field_start);
            current_ty = field_ty;
        }
        let values = vars[start..end]
            .iter()
            .map(|var| self.builder.use_var(*var))
            .collect::<Vec<_>>();
        for (var, abi) in vars[start..end].iter().copied().zip(current_ty.abi_types()) {
            let zero = if abi == types::F64 {
                self.builder.f64const(0.0)
            } else {
                self.builder.iconst(abi, 0)
            };
            self.builder.def_var(var, zero);
        }
        let moved = ValueRef {
            values,
            ty: current_ty,
        };
        self.mark_temporary_opaque_owned(&moved);
        Ok(moved)
    }

    fn load_root(&mut self, name: &str) -> std::result::Result<ValueRef, String> {
        let vars = self.local_vars(name)?;
        let ty = self.local_type(name)?;
        let values = vars
            .into_iter()
            .map(|var| self.builder.use_var(var))
            .collect::<Vec<_>>();
        let value = ValueRef { values, ty };
        if matches!(value.ty, DirectType::Opaque(_)) {
            self.clear_temporary_opaque_owned(&value);
        }
        Ok(value)
    }

    fn extract_field(
        &mut self,
        object: ValueRef,
        field: &str,
    ) -> std::result::Result<ValueRef, String> {
        match &object.ty {
            DirectType::PlainClass(_) => {
                let (start, end, field_ty) = required_direct_field_slice(&object.ty, field)?;
                Ok(ValueRef {
                    values: object.values[start..end].to_vec(),
                    ty: field_ty,
                })
            }
            DirectType::Union(union) => {
                let union = union.clone();
                let index = crate::mir::union_payload_projection_index(field)
                    .ok_or_else(|| format!("invalid union payload projection `{field}`"))?;
                self.union_member_from_words(&union, index, &object.values)
            }
            DirectType::Opaque(Type::Tuple(elements)) => {
                let index = field.parse::<usize>().map_err(|_| {
                    format!("direct backend tuple projection `{field}` is not a fixed position")
                })?;
                let element_type = elements.get(index).cloned().ok_or_else(|| {
                    format!(
                        "direct backend tuple of length {} has no element at index {index}",
                        elements.len()
                    )
                })?;
                let index_value = self.builder.iconst(types::I64, index as i64);
                let inst = self.builder.call(
                    self.runtime[RuntimeFn::TupleElement],
                    &[object.values[0], index_value],
                );
                let element = self.owned_opaque_result(inst.results, element_type.clone());
                let direct_type =
                    ensure_direct_type(&element_type, &self.classes, "tuple view element")?;
                self.coerce_value(element, &direct_type)
            }
            DirectType::Opaque(_) => {
                let (field_ptr, field_len) = self.string_constant(field.as_bytes())?;
                let inst = self.builder.call(
                    self.runtime[RuntimeFn::InstanceGetField],
                    &[object.values[0], field_ptr, field_len],
                );
                let loaded = ValueRef {
                    values: inst.results,
                    ty: DirectType::Opaque(Type::named("Unknown")),
                };
                self.mark_temporary_opaque_owned(&loaded);
                if let Some(field_ty) =
                    direct_field_type_with_enums(&object.ty, field, &self.classes, &self.enums)
                {
                    self.coerce_value(loaded, &field_ty)
                } else {
                    Ok(loaded)
                }
            }
            DirectType::Scalar(_) | DirectType::Callable(_) => Err(format!(
                "direct backend does not know field `{}` on `{}`",
                field,
                render_direct_type(&object.ty)
            )),
        }
    }

    fn coerce_value(
        &mut self,
        value: ValueRef,
        target: &DirectType,
    ) -> std::result::Result<ValueRef, String> {
        self.coerce_value_at(value, target, None)
    }

    fn coerce_value_at(
        &mut self,
        value: ValueRef,
        target: &DirectType,
        span: Option<Span>,
    ) -> std::result::Result<ValueRef, String> {
        if &value.ty == target {
            let value = self.normalize_scalar_value(value)?;
            if matches!(target.scalar_kind(), Some(ScalarKind::Int32)) {
                self.emit_int32_bounds_check(value.values[0], span)?;
            }
            if let DirectType::Opaque(target_ty) = target {
                // A leaf tag such as `str` only records the name, and the
                // value already carries the same static type, so retagging
                // it would change nothing.
                if !is_name_only_runtime_type(target_ty) {
                    self.tag_opaque_runtime_type(&value, target_ty)?;
                }
            }
            return Ok(value);
        }

        if let (DirectType::Scalar(ScalarKind::Unit), DirectType::Callable(_)) = (&value.ty, target)
        {
            // The unit placeholder of an unreachable fall-through (every
            // checked path returns a callable) is the absent callable: all
            // words zero, which releases and retains skip.
            return Ok(ValueRef {
                values: target.zero_values(&mut *self.builder),
                ty: target.clone(),
            });
        }

        if let (DirectType::Callable(source), DirectType::Callable(destination)) =
            (&value.ty, target)
        {
            // The words do not depend on the contract's spelling; the call
            // ABI does, so two contracts must agree on their direct parts.
            let source_parts = callables::contract_direct_parts(&source.signature, &self.classes)?;
            let destination_parts =
                callables::contract_direct_parts(&destination.signature, &self.classes)?;
            if source_parts == destination_parts {
                let owned = self.temporary_owns_callable(&value);
                let adapted = ValueRef {
                    values: value.values,
                    ty: target.clone(),
                };
                if owned {
                    self.mark_temporary_callable_owned(&adapted);
                }
                return Ok(adapted);
            }
            // Contracts with different direct parts (a generic frame's
            // symbolic `T | None` against a concrete union) meet through the
            // runtime: the value is boxed once and re-enters under the
            // destination contract's boxed descriptor.
            let destination_signature = destination.signature.clone();
            let boxed = self.ensure_opaque(value)?;
            let handle = self.transfer_opaque_arg(&boxed);
            let words = self.boxed_callable_words(handle, &destination_signature)?;
            let adapted = ValueRef {
                values: words,
                ty: target.clone(),
            };
            self.mark_temporary_callable_owned(&adapted);
            return Ok(adapted);
        }

        if let DirectType::Union(union) = target {
            let union = union.clone();
            if let DirectType::Opaque(source_ty) = &value.ty {
                if let Some(index) = union
                    .members
                    .iter()
                    .position(|member| member.ty == value.ty)
                {
                    // A runtime-object member lifts into the union as its
                    // owned handle word: an owned temporary moves in, a
                    // borrowed value is retained, and the union owns it.
                    let handle = self.transfer_opaque_arg(&value);
                    let member = ValueRef {
                        values: vec![handle],
                        ty: value.ty.clone(),
                    };
                    let words = self.union_words_from_member(&union, index, member)?;
                    let lifted = ValueRef {
                        values: words,
                        ty: target.clone(),
                    };
                    self.mark_temporary_union_owned(&lifted);
                    return Ok(lifted);
                }
                if matches!(source_ty, Type::Union(_))
                    || matches!(source_ty, Type::Named(name, _) if name == "Unknown")
                    || matches!(source_ty, Type::TypeParam(_))
                {
                    return self.unbox_inline_union(&union, value.values[0]);
                }
            }
            // A bare member value lifts into the union it is a member of
            // (the checker's comparison-only injection and the bare `None`).
            let member_index = union
                .members
                .iter()
                .position(|member| member.ty == value.ty)
                .ok_or_else(|| {
                    format!(
                        "direct backend encountered an unsupported value coercion from `{}` to `{}`",
                        render_direct_type(&value.ty),
                        render_direct_type(target)
                    )
                })?;
            let words = self.union_words_from_member(&union, member_index, value)?;
            return Ok(ValueRef {
                values: words,
                ty: target.clone(),
            });
        }

        if let DirectType::Opaque(target_ty) = target {
            if is_numeric_type_name(target_ty) {
                let boxed = self.ensure_opaque(value)?;
                let (target_ptr, target_len) =
                    self.string_constant(target_ty.to_string().as_bytes())?;
                let (line, column) = self.span_values(span);
                let inst = self.builder.call(
                    self.runtime[RuntimeFn::CastValue],
                    &[boxed.values[0], target_ptr, target_len, line, column],
                );
                let casted = self.owned_opaque_result(inst.results, direct_type_to_type(target));
                self.tag_opaque_runtime_type(&casted, target_ty)?;
                return Ok(casted);
            }
            let mut boxed = self.ensure_opaque(value)?;
            self.tag_opaque_runtime_type(&boxed, target_ty)?;
            boxed.ty = target.clone();
            return Ok(boxed);
        }

        if matches!(value.ty, DirectType::Opaque(_)) {
            let result = match target {
                DirectType::Callable(callable) => {
                    let handle = self.transfer_opaque_arg(&value);
                    let words = self.boxed_callable_words(handle, &callable.signature)?;
                    let lifted = ValueRef {
                        values: words,
                        ty: target.clone(),
                    };
                    self.mark_temporary_callable_owned(&lifted);
                    return Ok(lifted);
                }
                DirectType::Scalar(ScalarKind::Int32) => {
                    let inst = self
                        .builder
                        .call(self.runtime[RuntimeFn::UnboxI64], &[value.values[0]]);
                    ValueRef {
                        values: inst.results,
                        ty: target.clone(),
                    }
                }
                DirectType::Scalar(ScalarKind::Int64) => {
                    let inst = self
                        .builder
                        .call(self.runtime[RuntimeFn::UnboxInt64], &[value.values[0]]);
                    ValueRef {
                        values: inst.results,
                        ty: target.clone(),
                    }
                }
                DirectType::Scalar(ScalarKind::Uint64) => {
                    let inst = self
                        .builder
                        .call(self.runtime[RuntimeFn::UnboxU64], &[value.values[0]]);
                    ValueRef {
                        values: inst.results,
                        ty: target.clone(),
                    }
                }
                DirectType::Scalar(ScalarKind::Float32)
                | DirectType::Scalar(ScalarKind::Float64) => {
                    let inst = self
                        .builder
                        .call(self.runtime[RuntimeFn::UnboxF64], &[value.values[0]]);
                    self.normalize_scalar_value(ValueRef {
                        values: inst.results,
                        ty: target.clone(),
                    })?
                }
                DirectType::Scalar(ScalarKind::Bool) => {
                    let inst = self
                        .builder
                        .call(self.runtime[RuntimeFn::UnboxBool], &[value.values[0]]);
                    ValueRef {
                        values: inst.results,
                        ty: target.clone(),
                    }
                }
                DirectType::Scalar(ScalarKind::Unit) => unit_value(&mut *self.builder),
                DirectType::Union(union) => {
                    let union = union.clone();
                    self.unbox_inline_union(&union, value.values[0])?
                }
                DirectType::PlainClass(class) => {
                    let mut values = Vec::new();
                    for field in &class.fields {
                        let (field_ptr, field_len) = self.string_constant(field.name.as_bytes())?;
                        let inst = self.builder.call(
                            self.runtime[RuntimeFn::InstanceGetField],
                            &[value.values[0], field_ptr, field_len],
                        );
                        let field_value = ValueRef {
                            values: inst.results,
                            ty: DirectType::Opaque(Type::named("Unknown")),
                        };
                        self.mark_temporary_opaque_owned(&field_value);
                        let coerced = self.coerce_value_at(field_value, &field.ty, span)?;
                        values.extend(coerced.values);
                    }
                    ValueRef {
                        values,
                        ty: target.clone(),
                    }
                }
                DirectType::Opaque(_) => unreachable!("opaque target handled earlier"),
            };
            if matches!(target.scalar_kind(), Some(ScalarKind::Int32)) {
                self.emit_int32_bounds_check(result.values[0], span)?;
            }
            return Ok(result);
        }

        match (value.ty.scalar_kind(), target.scalar_kind()) {
            (Some(ScalarKind::Bool), Some(ScalarKind::Int32)) => Ok(ValueRef {
                values: value.values,
                ty: target.clone(),
            }),
            (Some(ScalarKind::Int32), Some(ScalarKind::Int64)) => Ok(ValueRef {
                values: value.values,
                ty: target.clone(),
            }),
            (Some(ScalarKind::Int64), Some(ScalarKind::Int32)) => {
                self.emit_int32_bounds_check(value.values[0], span)?;
                Ok(ValueRef {
                    values: value.values,
                    ty: target.clone(),
                })
            }
            (Some(lhs), Some(rhs)) if lhs.is_float() && rhs.is_float() => self
                .normalize_scalar_value(ValueRef {
                    values: value.values,
                    ty: target.clone(),
                }),
            (Some(ScalarKind::Int32), Some(ScalarKind::Bool)) => Ok(ValueRef {
                values: value.values,
                ty: target.clone(),
            }),
            (Some(ScalarKind::Unit), Some(ScalarKind::Int32)) => Ok(ValueRef {
                values: vec![self.builder.iconst(types::I64, 0)],
                ty: target.clone(),
            }),
            _ => Err(format!(
                "direct backend encountered an unsupported value coercion from `{}` to `{}`",
                render_direct_type(&value.ty),
                render_direct_type(target)
            )),
        }
    }

    fn normalize_scalar_value(&mut self, value: ValueRef) -> std::result::Result<ValueRef, String> {
        match value.ty.scalar_kind() {
            Some(ScalarKind::Float32) => {
                let narrowed = self.builder.fdemote(types::F32, value.values[0]);
                let widened = self.builder.fpromote(types::F64, narrowed);
                Ok(ValueRef {
                    values: vec![widened],
                    ty: value.ty,
                })
            }
            _ => Ok(value),
        }
    }

    fn tag_opaque_runtime_type(
        &mut self,
        value: &ValueRef,
        ty: &Type,
    ) -> std::result::Result<(), String> {
        if runtime_type_is_wildcard(ty) {
            return Ok(());
        }
        let [raw] = value.values.as_slice() else {
            return Err(format!(
                "direct backend expected opaque `{ty}` to use one runtime value"
            ));
        };
        self.emit_runtime_type_tag(*raw, ty)
    }

    /// Tags `raw` with `ty`. A tag repeats nothing when the same value got the
    /// same tag earlier in this block and no call ran since, because only a
    /// call can change a box.
    fn emit_runtime_type_tag(&mut self, raw: Val, ty: &Type) -> std::result::Result<(), String> {
        let encoded = crate::native_runtime::canonical_runtime_type_name(ty);
        if let Some((inst, tagged, tagged_type)) = &self.last_runtime_type_tag {
            // A retained handle names the box it retained.
            let same_box = *tagged == raw
                || self
                    .retain_calls
                    .get(&raw)
                    .is_some_and(|(_, retained)| retained == tagged);
            if same_box && *tagged_type == encoded && self.no_call_since(*inst) {
                return Ok(());
            }
        }
        let (type_ptr, type_len) = self.string_constant(encoded.as_bytes())?;
        let cell = self.tag_cell(&encoded)?;
        let call = self.builder.call(
            self.runtime[RuntimeFn::TagValueType],
            &[raw, cell, type_ptr, type_len],
        );
        self.last_runtime_type_tag = Some((call.inst, raw, encoded));
        Ok(())
    }

    /// The address of the module's tag cell for `encoded`. The runtime
    /// stores the decoded tag there on first use and reads it back after.
    fn tag_cell(&mut self, encoded: &str) -> std::result::Result<Val, String> {
        let id = match self.tag_cells.get(encoded) {
            Some(id) => *id,
            None => {
                let name = format!("aura_tag_cell_{}", self.tag_cells.len());
                let module = self.builder.module();
                let id = try_or_string_error!(
                    module.declare_data(&name, Linkage::Local, true),
                    "failed to declare a type tag cell: {}"
                );
                let mut data = DataDescription::new(vec![0u8; 8]);
                data.align = Some(8);
                try_or_string_error!(
                    module.define_data(id, data),
                    "failed to define a type tag cell: {}"
                );
                self.tag_cells.insert(encoded.to_string(), id);
                id
            }
        };
        let global = self.builder.import_data(id);
        Ok(self.builder.symbol_value(types::I64, global))
    }

    /// Whether `inst` is in the current block and no call that could change
    /// a box follows it there. A retain or a release changes only the count
    /// of the value it names, and a tag of `raw` is never followed by a
    /// release of `raw` that frees it. A call that only boxes a new value
    /// from its words reaches no other box.
    fn no_call_since(&self, inst: Inst) -> bool {
        self.builder.only_calls_since(
            inst,
            &[
                self.runtime[RuntimeFn::RetainValue],
                self.runtime[RuntimeFn::ReleaseValue],
                self.runtime[RuntimeFn::FormatBegin],
                self.runtime[RuntimeFn::StringLiteral],
                self.runtime[RuntimeFn::BoxI64],
                self.runtime[RuntimeFn::BoxBool],
            ],
        )
    }

    fn tag_raw_opaque_runtime_type(
        &mut self,
        raw: Val,
        ty: &DirectType,
    ) -> std::result::Result<(), String> {
        let DirectType::Opaque(ty) = ty else {
            return Ok(());
        };
        if runtime_type_is_wildcard(ty) {
            return Ok(());
        }
        self.emit_runtime_type_tag(raw, ty)
    }

    /// The value of `value` when an `iconst` defines it.
    fn integer_constant(&self, value: Val) -> Option<i64> {
        self.builder.integer_constant(value)
    }

    fn emit_int32_bounds_check(
        &mut self,
        value: Val,
        span: Option<Span>,
    ) -> std::result::Result<(), String> {
        // `value` is an `int32` iff biasing it by `-i32::MIN` lands inside the
        // unsigned 32-bit range. That is one add and one compare, where the
        // signed two-sided form needs two constants, two compares, and an or.
        // This check runs on the result of every narrow arithmetic operation,
        // so its cost is what separated `int32` loops from `int64` ones, where
        // the overflow flag comes free with the add.
        if let Some(constant) = self.integer_constant(value) {
            if i32::try_from(constant).is_ok() {
                return Ok(());
            }
        }
        let biased = self.builder.iadd_imm(value, -(i32::MIN as i64));
        let overflow = self
            .builder
            .icmp_imm(IntCC::UnsignedGreaterThan, biased, u32::MAX as i64);
        let fail_block = self.builder.create_block();
        let continue_block = self.builder.create_block();
        self.builder
            .brif(overflow, fail_block, &[], continue_block, &[]);
        self.builder.switch_to_block(fail_block);
        self.emit_pending_cleanups(true)?;
        let (line, column) = self.span_values(span);
        self.builder.call(
            self.runtime[RuntimeFn::FailInt32Overflow],
            &[value, line, column],
        );
        self.builder.trap(TrapCode::IntegerOverflow);
        self.builder.seal_block(fail_block);
        self.builder.switch_to_block(continue_block);
        self.builder.seal_block(continue_block);
        Ok(())
    }

    /// The length of `list`, read from its list view when the value is
    /// confined to this thread (see `aura_runtime::native_runtime::DirectListLayout`).
    fn emit_list_len(&mut self, list: Val) -> Val {
        let layout = aura_runtime::native_runtime::direct_list_layout();
        let slow_block = self.builder.create_block();
        let fast_block = self.builder.create_block();
        let join_block = self.builder.create_block();
        self.builder.append_block_param(join_block, types::I64);
        // Every list form keeps its length in the view.
        let access = self.emit_list_access(list, &layout);
        let form = self.builder.iadd_imm(
            access,
            -(aura_runtime::native_runtime::DirectListAccess::Values as i64),
        );
        let is_list = self.builder.icmp_imm(
            IntCC::UnsignedLessThan,
            form,
            i64::from(aura_runtime::native_runtime::DirectListAccess::LIST_FORMS),
        );
        self.builder.brif(is_list, fast_block, &[], slow_block, &[]);

        self.builder.switch_to_block(fast_block);
        self.builder.seal_block(fast_block);
        let len = self
            .builder
            .load(types::I64, MemFlags::trusted(), list, layout.len_offset);
        self.builder.jump(join_block, &[len]);

        self.builder.switch_to_block(slow_block);
        self.builder.seal_block(slow_block);
        let inst = self.builder.call(self.runtime[RuntimeFn::VecLen], &[list]);
        let len = inst.results[0];
        self.builder.jump(join_block, &[len]);

        self.builder.switch_to_block(join_block);
        self.builder.seal_block(join_block);
        self.builder.block_params(join_block)[0]
    }

    /// Whether element `index` of a `list[str]` equals a literal, as 1 or 0.
    /// A list whose view is `DirectListAccess::Str` is read in place: the
    /// lengths are compared first, and the bytes only when they match. Any
    /// other list, or an index out of range, takes
    /// `aura_direct_compare_str_literal_at` with `slow_args`, which asks
    /// for `==` and reports the same diagnostics as before.
    fn emit_str_view_equality(
        &mut self,
        list: Val,
        index: Val,
        (literal_ptr, literal_len): (Val, usize),
        slow_args: &[Val; 8],
    ) -> Val {
        use aura_runtime::native_runtime::{
            DirectListAccess, DIRECT_STR_VIEW_ENTRY_BYTES, DIRECT_STR_VIEW_LEN_OFFSET,
            DIRECT_STR_VIEW_PTR_OFFSET,
        };
        let layout = aura_runtime::native_runtime::direct_list_layout();
        let slow_block = self.builder.create_block();
        let fast_block = self.builder.create_block();
        let join_block = self.builder.create_block();
        self.builder.append_block_param(join_block, types::I64);
        self.emit_list_access_check(list, DirectListAccess::Str, &layout, slow_block, fast_block);

        self.builder.switch_to_block(fast_block);
        self.builder.seal_block(fast_block);
        let flags = MemFlags::trusted();
        let entries = self
            .builder
            .load(types::I64, flags, list, layout.elements_offset);
        let position = self.emit_list_view_bounds_check(list, index, &layout, slow_block);
        let offset = self
            .builder
            .imul_imm(position, i64::from(DIRECT_STR_VIEW_ENTRY_BYTES));
        let entry = self.builder.iadd(entries, offset);
        let len = self
            .builder
            .load(types::I64, flags, entry, DIRECT_STR_VIEW_LEN_OFFSET);
        let same_len = self.builder.icmp_imm(IntCC::Equal, len, literal_len as i64);
        if literal_len == 0 {
            let empty = self.builder.uextend(types::I64, same_len);
            self.builder.jump(join_block, &[empty]);
        } else {
            let bytes_block = self.builder.create_block();
            let zero = self.builder.iconst(types::I64, 0);
            self.builder
                .brif(same_len, bytes_block, &[], join_block, &[zero]);
            self.builder.switch_to_block(bytes_block);
            self.builder.seal_block(bytes_block);
            let bytes = self
                .builder
                .load(types::I64, flags, entry, DIRECT_STR_VIEW_PTR_OFFSET);
            let literal_len = self.builder.iconst(types::I64, literal_len as i64);
            let inst = self.builder.call(
                self.runtime[RuntimeFn::BytesEqual],
                &[bytes, literal_ptr, literal_len],
            );
            let equal = inst.results[0];
            self.builder.jump(join_block, &[equal]);
        }

        self.builder.switch_to_block(slow_block);
        self.builder.seal_block(slow_block);
        let mut args = *slow_args;
        args[0] = self
            .builder
            .iconst(types::I64, Self::binary_opcode(BinaryOp::Eq));
        let inst = self
            .builder
            .call(self.runtime[RuntimeFn::CompareStrLiteralAt], &args);
        let equal = inst.results[0];
        self.builder.jump(join_block, &[equal]);

        self.builder.switch_to_block(join_block);
        self.builder.seal_block(join_block);
        self.builder.block_params(join_block)[0]
    }

    /// Loads the `DirectListAccess` code of `list`, widened to 64 bits.
    /// Only the creating thread changes the code of a confined value, and it
    /// clears the code before it hands the value on, so a plain load is
    /// enough.
    fn emit_list_access(
        &mut self,
        list: Val,
        layout: &aura_runtime::native_runtime::DirectListLayout,
    ) -> Val {
        self.builder
            .uload32(MemFlags::trusted(), list, layout.access_offset)
    }

    /// Branches to `fast_block` when the list view of `list` has the layout
    /// `access` names, which also means the value is confined to this
    /// thread, and to `slow_block` otherwise.
    fn emit_list_access_check(
        &mut self,
        list: Val,
        access: aura_runtime::native_runtime::DirectListAccess,
        layout: &aura_runtime::native_runtime::DirectListLayout,
        slow_block: Blk,
        fast_block: Blk,
    ) {
        let actual = self.emit_list_access(list, layout);
        let matches = self.builder.icmp_imm(IntCC::Equal, actual, access as i64);
        self.builder.brif(matches, fast_block, &[], slow_block, &[]);
    }

    /// Reads one element of an `Array` indexed by one `int64`. A confined
    /// rank-1 array in range is read in place from its list view. Any other
    /// array or position takes the runtime call, which reports the rank or
    /// range diagnostic.
    fn emit_array_element_read(
        &mut self,
        array: Val,
        coordinate: ValueRef,
        kind: ScalarKind,
        element_ty: Type,
        line: Val,
        column: Val,
    ) -> std::result::Result<ValueRef, String> {
        use aura_runtime::native_runtime::DirectListAccess;
        let (size, load_ty, access) = match kind {
            ScalarKind::Int32 => (4, types::I32, DirectListAccess::ArrayInt32),
            ScalarKind::Int64 => (8, types::I64, DirectListAccess::ArrayInt64),
            ScalarKind::Float32 => (4, types::F32, DirectListAccess::ArrayFloat32),
            ScalarKind::Float64 => (8, types::F64, DirectListAccess::ArrayFloat64),
            other => return Err(format!("direct backend has no Array dtype `{other:?}`")),
        };
        let layout = aura_runtime::native_runtime::direct_list_layout();
        let element_direct_ty = DirectType::Scalar(kind);
        let slow_block = self.builder.create_block();
        let fast_block = self.builder.create_block();
        let join_block = self.builder.create_block();
        self.builder
            .append_block_param(join_block, kind.signature_type());
        self.emit_list_access_check(array, access, &layout, slow_block, fast_block);
        self.builder.switch_to_block(fast_block);
        self.builder.seal_block(fast_block);
        let flags = MemFlags::trusted();
        let elements = self
            .builder
            .load(types::I64, flags, array, layout.elements_offset);
        let position =
            self.emit_list_view_bounds_check(array, coordinate.values[0], &layout, slow_block);
        let offset = self.builder.imul_imm(position, size);
        let element = self.builder.iadd(elements, offset);
        let value = match kind {
            ScalarKind::Int32 => self.builder.sload32(flags, element, 0),
            ScalarKind::Float32 => {
                let narrow = self.builder.load(load_ty, flags, element, 0);
                self.builder.fpromote(types::F64, narrow)
            }
            _ => self.builder.load(load_ty, flags, element, 0),
        };
        self.builder.jump(join_block, &[value]);

        self.builder.switch_to_block(slow_block);
        self.builder.seal_block(slow_block);
        let caller_owned = self.layout_scratch.clone();
        let coordinates = self.ensure_opaque(coordinate)?;
        let inst = self.builder.call(
            self.runtime[RuntimeFn::ArrayIndex],
            &[array, coordinates.values[0], line, column],
        );
        let result = self.owned_opaque_result(inst.results, element_ty);
        let result = self.coerce_value(result, &element_direct_ty)?;
        self.release_temporary_owned_since(&caller_owned);
        self.builder.jump(join_block, &[result.values[0]]);
        self.layout_scratch = caller_owned;

        self.builder.switch_to_block(join_block);
        self.builder.seal_block(join_block);
        Ok(ValueRef {
            values: vec![self.builder.block_params(join_block)[0]],
            ty: element_direct_ty,
        })
    }

    /// Reads a scalar list element as raw bits. `kind` is 0 for `int64`,
    /// 1 for `float64`, and 2 for `bool`. A confined list that keeps the
    /// scalars unboxed is read in place with one bounds check, or none when
    /// the loop proves the index. Any other list, or an index out of range,
    /// takes the runtime call, which reports the same diagnostic as before.
    fn emit_scalar_list_read(
        &mut self,
        list: Val,
        index: Val,
        kind: i64,
        proven: bool,
        line: Val,
        column: Val,
    ) -> std::result::Result<Val, String> {
        use aura_runtime::native_runtime::DirectListAccess;
        let layout = aura_runtime::native_runtime::direct_list_layout();
        let (access, stride) = match kind {
            0 => (DirectListAccess::Int64, 8),
            1 => (DirectListAccess::Float64, 8),
            _ => (DirectListAccess::Bool, 1),
        };
        let slow_block = self.builder.create_block();
        let fast_block = self.builder.create_block();
        let join_block = self.builder.create_block();
        self.builder.append_block_param(join_block, types::I64);
        self.emit_list_access_check(list, access, &layout, slow_block, fast_block);
        self.builder.switch_to_block(fast_block);
        self.builder.seal_block(fast_block);
        let flags = MemFlags::trusted();
        let elements = self
            .builder
            .load(types::I64, flags, list, layout.elements_offset);
        let position = if proven {
            index
        } else {
            self.emit_list_view_bounds_check(list, index, &layout, slow_block)
        };
        let offset = if stride == 1 {
            position
        } else {
            self.builder.imul_imm(position, stride)
        };
        let element = self.builder.iadd(elements, offset);
        let bits = if stride == 1 {
            self.builder.uload8(types::I64, flags, element, 0)
        } else {
            self.builder.load(types::I64, flags, element, 0)
        };
        self.builder.jump(join_block, &[bits]);

        self.builder.switch_to_block(slow_block);
        self.builder.seal_block(slow_block);
        let normalized = self.emit_vec_index_failure_guard(list, index, line, column)?;
        let kind = self.builder.iconst(types::I64, kind);
        let inst = self.builder.call(
            self.runtime[RuntimeFn::VecIndexScalar],
            &[list, normalized, kind],
        );
        let bits = inst.results[0];
        self.builder.jump(join_block, &[bits]);

        self.builder.switch_to_block(join_block);
        self.builder.seal_block(join_block);
        Ok(self.builder.block_params(join_block)[0])
    }

    /// Appends a scalar's `bits` to `list`. A confined list that keeps the
    /// scalars unboxed and has room stores the element past the end and
    /// raises the view's length, and the runtime takes the element in at
    /// its next access. Any other list, or a full one, calls `function`
    /// with `words`, which grows the storage, converts the form, or refreshes
    /// the view, as every write does.
    fn emit_scalar_list_push(
        &mut self,
        list: Val,
        bits: Val,
        kind: ScalarElement,
        function: FuncRef,
        words: &[Val],
    ) {
        use aura_runtime::native_runtime::DirectListAccess;
        let layout = aura_runtime::native_runtime::direct_list_layout();
        let (access, stride) = match kind {
            ScalarElement::Int64 => (DirectListAccess::Int64, 8),
            ScalarElement::Float64 => (DirectListAccess::Float64, 8),
            ScalarElement::Bool => (DirectListAccess::Bool, 1),
        };
        let slow_block = self.builder.create_block();
        let fast_block = self.builder.create_block();
        let room_block = self.builder.create_block();
        let join_block = self.builder.create_block();
        self.emit_list_access_check(list, access, &layout, slow_block, fast_block);
        self.builder.switch_to_block(fast_block);
        self.builder.seal_block(fast_block);
        let flags = MemFlags::trusted();
        let len = self
            .builder
            .load(types::I64, flags, list, layout.len_offset);
        let capacity = self.builder.uload32(flags, list, layout.capacity_offset);
        let full = self
            .builder
            .icmp(IntCC::UnsignedGreaterThanOrEqual, len, capacity);
        self.builder.brif(full, slow_block, &[], room_block, &[]);

        self.builder.switch_to_block(room_block);
        self.builder.seal_block(room_block);
        let elements = self
            .builder
            .load(types::I64, flags, list, layout.elements_offset);
        if stride == 1 {
            // A `bool` element is one byte that holds 0 or 1.
            let element = self.builder.iadd(elements, len);
            let flag = self.builder.icmp_imm(IntCC::NotEqual, bits, 0);
            self.builder.store(flags, flag, element, 0);
        } else {
            let offset = self.builder.imul_imm(len, stride);
            let element = self.builder.iadd(elements, offset);
            self.builder.store(flags, bits, element, 0);
        }
        let grown = self.builder.iadd_imm(len, 1);
        self.builder.store(flags, grown, list, layout.len_offset);
        self.builder.jump(join_block, &[]);

        self.builder.switch_to_block(slow_block);
        self.builder.seal_block(slow_block);
        self.builder.call(function, words);
        self.builder.jump(join_block, &[]);

        self.builder.switch_to_block(join_block);
        self.builder.seal_block(join_block);
    }

    /// Normalizes a negative `index` against a list view's length, and
    /// continues in a new block when the position is in range or in `slow`
    /// when it is not.
    fn emit_list_view_bounds_check(
        &mut self,
        list: Val,
        index: Val,
        layout: &aura_runtime::native_runtime::DirectListLayout,
        slow: Blk,
    ) -> Val {
        let len = self
            .builder
            .load(types::I64, MemFlags::trusted(), list, layout.len_offset);
        let zero = self.builder.iconst(types::I64, 0);
        let negative = self.builder.icmp(IntCC::SignedLessThan, index, zero);
        let from_end = self.builder.iadd(len, index);
        let position = self.builder.select(negative, from_end, index);
        // One unsigned compare also rejects a position below zero.
        let out_of_range = self
            .builder
            .icmp(IntCC::UnsignedGreaterThanOrEqual, position, len);
        let in_range = self.builder.create_block();
        self.builder.brif(out_of_range, slow, &[], in_range, &[]);
        self.builder.switch_to_block(in_range);
        self.builder.seal_block(in_range);
        position
    }

    fn emit_vec_index_failure_guard(
        &mut self,
        object: Val,
        index: Val,
        line: Val,
        column: Val,
    ) -> std::result::Result<Val, String> {
        let len_inst = self
            .builder
            .call(self.runtime[RuntimeFn::VecLen], &[object]);
        let len = len_inst.results[0];
        let zero = self.builder.iconst(types::I64, 0);
        let negative = self.builder.icmp(IntCC::SignedLessThan, index, zero);
        let from_end = self.builder.iadd(len, index);
        let normalized = self.builder.select(negative, from_end, index);
        let below = self.builder.icmp(IntCC::SignedLessThan, normalized, zero);
        let above = self
            .builder
            .icmp(IntCC::SignedGreaterThanOrEqual, normalized, len);
        let out_of_bounds = self.builder.bor(below, above);
        let fail_block = self.builder.create_block();
        let continue_block = self.builder.create_block();
        self.builder
            .brif(out_of_bounds, fail_block, &[], continue_block, &[]);
        self.builder.switch_to_block(fail_block);
        self.emit_pending_cleanups(true)?;
        self.builder.call(
            self.runtime[RuntimeFn::VecIndex],
            &[object, index, line, column],
        );
        self.builder.trap(TrapCode::User(2));
        self.builder.seal_block(fail_block);
        self.builder.switch_to_block(continue_block);
        self.builder.seal_block(continue_block);
        Ok(normalized)
    }

    fn as_bool_value(&mut self, value: ValueRef) -> std::result::Result<Val, String> {
        match value.ty.scalar_kind() {
            Some(ScalarKind::Bool) | Some(ScalarKind::Int32) | Some(ScalarKind::Unit) => {
                let zero = self.builder.iconst(types::I64, 0);
                Ok(self.builder.icmp(IntCC::NotEqual, value.values[0], zero))
            }
            None if matches!(value.ty, DirectType::Opaque(_)) => {
                let inst = self.builder.call(
                    self.runtime[RuntimeFn::ValueAsCondition],
                    &[value.values[0]],
                );
                Ok(inst.results[0])
            }
            other => Err(format!(
                "direct backend cannot use `{}` as a branch condition",
                match other {
                    Some(kind) => render_direct_type(&DirectType::Scalar(kind)),
                    None => render_direct_type(&value.ty),
                }
            )),
        }
    }

    /// Whether storing a receiver back into `place` after an in-place write
    /// would change nothing: `place` is a plain local holding a handle, so
    /// the write already changed the value it holds, and no view, mutable
    /// parameter, or cleanup registration follows it.
    fn receiver_store_is_redundant(&self, place: &str) -> bool {
        !place.contains('.')
            && !self.view_places.contains_key(place)
            && !self.mutable_param_indices.contains_key(place)
            && !self
                .cleanup_places
                .iter()
                .any(|cleanup| direct_place_paths_overlap(cleanup, place))
            && matches!(self.variable_types.get(place), Some(DirectType::Opaque(_)))
    }

    fn store_place(&mut self, place: &str, value: ValueRef) -> std::result::Result<(), String> {
        let resolved = self.resolve_view_place(place)?;
        self.store_resolved_view_place(resolved, value)
    }

    fn store_resolved_view_place(
        &mut self,
        resolved: DirectViewPlace,
        value: ValueRef,
    ) -> std::result::Result<(), String> {
        if resolved.alternatives.len() == 1 && resolved.alternatives[0].conditions.is_empty() {
            return self.store_alternative(&resolved.alternatives[0], value);
        }
        self.store_selected_view_place(resolved, value)
    }

    fn store_static_place(
        &mut self,
        place: &str,
        value: ValueRef,
    ) -> std::result::Result<(), String> {
        let mut segments = place.split('.').collect::<Vec<_>>();
        let root = segments.remove(0);
        let result = if segments.is_empty() {
            self.store_root(root, value)
        } else if matches!(self.variable_types.get(root), Some(DirectType::Opaque(_))) {
            let current = self.load_root(root)?;
            let current = self.ensure_opaque(current)?;
            let updated_value = self.ensure_opaque(value)?;
            let updated_value = self.transfer_owned_opaque_value(&updated_value);
            let field_path = segments.join(".");
            let (field_ptr, field_len) = self.string_constant(field_path.as_bytes())?;
            self.builder.call(
                self.runtime[RuntimeFn::InstanceSetFieldOwned],
                &[current.values[0], field_ptr, field_len, updated_value],
            );
            Ok(())
        } else {
            let root_value = self.load_root(root)?;
            let updated = self.replace_nested_field(root_value, &segments, value)?;
            self.store_root(root, updated)
        };
        result?;
        self.publish_mutable_root_write_through(root)?;
        self.refresh_cleanup_registrations_for_mutation(place)
    }

    fn store_selected_view_place(
        &mut self,
        resolved: DirectViewPlace,
        value: ValueRef,
    ) -> std::result::Result<(), String> {
        let merge = self.builder.create_block();
        let caller_owned = self.layout_scratch.clone();
        let value_was_owned = self.temporary_owns_opaque(&value);
        let alternative_count = resolved.alternatives.len();
        for (index, alternative) in resolved.alternatives.into_iter().enumerate() {
            if index + 1 < alternative_count {
                let selected = self.view_alternative_condition(&alternative);
                let store_block = self.builder.create_block();
                let next_block = self.builder.create_block();
                self.builder
                    .brif(selected, store_block, &[], next_block, &[]);
                self.builder.switch_to_block(store_block);
                self.layout_scratch = caller_owned.clone();
                self.store_alternative(&alternative, value.clone())?;
                self.release_temporary_owned_since(&caller_owned);
                self.builder.jump(merge, &[]);
                self.builder.seal_block(store_block);
                self.builder.switch_to_block(next_block);
                self.builder.seal_block(next_block);
            } else {
                self.layout_scratch = caller_owned.clone();
                self.store_alternative(&alternative, value.clone())?;
                self.release_temporary_owned_since(&caller_owned);
                self.builder.jump(merge, &[]);
            }
        }
        self.builder.switch_to_block(merge);
        self.builder.seal_block(merge);
        self.restore_scratch_after_branches(caller_owned);
        if value_was_owned {
            self.clear_temporary_opaque_owned(&value);
        }
        Ok(())
    }

    fn replace_nested_field(
        &mut self,
        current: ValueRef,
        segments: &[&str],
        new_value: ValueRef,
    ) -> std::result::Result<ValueRef, String> {
        let (head, rest) = split_field_path_segments(segments)?;
        if let DirectType::Union(union) = &current.ty {
            // Writing a payload projection selects that member: the tag and
            // the payload words are replaced together.
            let union = union.clone();
            let index = crate::mir::union_payload_projection_index(head)
                .ok_or_else(|| format!("invalid union payload projection `{head}`"))?;
            let member_ty = union.member(index)?.clone();
            if !rest.is_empty() && matches!(member_ty, DirectType::Opaque(_)) {
                // A projection into a runtime-object member writes through
                // the runtime, which resolves enum payloads and nested unions
                // inside the object; the union's own words do not change.
                let member = self.union_member_from_words(&union, index, &current.values)?;
                let updated_value = self.ensure_opaque(new_value)?;
                let updated_value = self.transfer_owned_opaque_value(&updated_value);
                let field_path = rest.join(".");
                let (field_ptr, field_len) = self.string_constant(field_path.as_bytes())?;
                self.builder.call(
                    self.runtime[RuntimeFn::InstanceSetFieldOwned],
                    &[member.values[0], field_ptr, field_len, updated_value],
                );
                return Ok(current);
            }
            let replacement = if rest.is_empty() {
                self.coerce_value(new_value, &member_ty)?
            } else {
                let nested = self.union_member_from_words(&union, index, &current.values)?;
                self.replace_nested_field(nested, rest, new_value)?
            };
            let owns_written_handle = matches!(member_ty, DirectType::Opaque(_));
            let replacement = if owns_written_handle {
                // The written handle is owned by the new union words: adopt
                // an owned temporary or retain a borrowed value. The handle
                // the union held before is released once, by the store that
                // replaces the root's words.
                let stored = self.transfer_opaque_arg(&replacement);
                ValueRef {
                    values: vec![stored],
                    ty: member_ty.clone(),
                }
            } else {
                replacement
            };
            let values = self.union_words_from_member(&union, index, replacement)?;
            let updated = ValueRef {
                values,
                ty: current.ty,
            };
            if owns_written_handle {
                self.mark_temporary_union_owned(&updated);
            }
            return Ok(updated);
        }
        let (start, end, field_ty) = required_direct_field_slice(&current.ty, head)?;

        let replacement = if rest.is_empty() {
            self.coerce_value(new_value, &field_ty)?
        } else {
            let nested = ValueRef {
                values: current.values[start..end].to_vec(),
                ty: field_ty.clone(),
            };
            self.replace_nested_field(nested, rest, new_value)?
        };

        let mut values = Vec::with_capacity(current.values.len());
        values.extend_from_slice(&current.values[..start]);
        values.extend(replacement.values);
        values.extend_from_slice(&current.values[end..]);
        Ok(ValueRef {
            values,
            ty: current.ty,
        })
    }

    /// Stores `value` into root `name`. The root takes an owned layout
    /// temporary as it is and copies a value read in place by its layout.
    /// A store the MIR assigns to an emptied root adds no release; any
    /// other store, such as a writeback or a field written into an inline
    /// union, drops the value the root held.
    fn store_root(&mut self, name: &str, value: ValueRef) -> std::result::Result<(), String> {
        let expected = self.local_type(name)?;
        // A lookup slot holds a payload's box, which its `Lookup` type
        // must not tag.
        let value = if self.lookup_slots.contains(name) {
            value
        } else {
            self.coerce_value(value, &expected)?
        };
        let vars = self.local_vars(name)?;
        let stored = self.transfer_values(&value);
        if self.emptied_store_root.as_deref() == Some(name) {
            self.emptied_store_root = None;
        } else {
            self.release_root_words(name)?;
        }
        for (var, compiled) in vars.into_iter().zip(stored) {
            self.builder.def_var(var, compiled);
        }
        Ok(())
    }

    /// A union clones as its active member (ADR-0052 A6): inline members
    /// copy their words, a runtime-object member's handle is cloned by the
    /// runtime, and the copy owns its handle.
    fn compile_union_clone(
        &mut self,
        union: &DirectUnionType,
        words: &[Val],
    ) -> std::result::Result<ValueRef, String> {
        let result_ty = DirectType::Union(union.clone());
        let cloned =
            self.switch_on_union_tag(union, words, &result_ty, |codegen, index, member| {
                let member = if matches!(member.ty, DirectType::Opaque(_)) {
                    let inst = codegen
                        .builder
                        .call(codegen.runtime[RuntimeFn::CloneValue], &[member.values[0]]);
                    ValueRef {
                        values: inst.results,
                        ty: member.ty.clone(),
                    }
                } else {
                    member
                };
                let member_words = codegen.union_words_from_member(union, index, member)?;
                Ok(ValueRef {
                    values: member_words,
                    ty: result_ty.clone(),
                })
            })?;
        self.mark_temporary_union_owned(&cloned);
        Ok(cloned)
    }

    /// A method on an inline union receiver dispatches on the tag, and each
    /// member runs its arm of the dispatch. The arms share the result type
    /// of the first method one of them calls. A `mut self` arm writes its
    /// receiver back through the member's payload projection.
    fn compile_union_member_call(
        &mut self,
        union: &DirectUnionType,
        words: &[Val],
        field: &str,
        arms: &[MethodDispatch],
        receiver_place: Option<&str>,
        args: &[MirArg],
    ) -> std::result::Result<ValueRef, String> {
        if arms.len() != union.members.len() {
            return Err(format!(
                "direct backend received {} dispatch arms for the {} members of `{}`",
                arms.len(),
                union.members.len(),
                union.union_type
            ));
        }
        let result_ty = arms
            .iter()
            .find_map(|arm| match arm {
                MethodDispatch::Static { function } => {
                    self.function_return_types.get(function).cloned()
                }
                _ => None,
            })
            .ok_or_else(|| {
                format!(
                    "direct backend does not know the result of `.{field}` on `{}`",
                    union.union_type
                )
            })?;
        let words = words.to_vec();
        // A receiver the call owns, as MIR's `Retain` made it, hands its
        // active member to the member's call.
        let receiver = ValueRef {
            values: words.clone(),
            ty: DirectType::Union(union.clone()),
        };
        let owned = self.temporary_owns_union(&receiver);
        if owned {
            self.clear_temporary_union_owned(&receiver);
        }
        let receiver_place = receiver_place.map(str::to_string);
        let args = args.to_vec();
        let field = field.to_string();
        let result =
            self.switch_on_union_tag(union, &words, &result_ty, |codegen, index, member| {
                if owned {
                    codegen.mark_call_result_owned(&member);
                }
                let projected = receiver_place.as_ref().map(|place| {
                    format!(
                        "{place}.{}{index}",
                        crate::mir::UNION_PAYLOAD_PROJECTION_PREFIX
                    )
                });
                codegen.compile_dispatch(member, &field, &arms[index], projected.as_deref(), &args)
            })?;
        if matches!(result.ty, DirectType::Opaque(_)) {
            self.mark_temporary_opaque_owned(&result);
        }
        Ok(result)
    }

    /// The tag word and payload words of `member` injected as member
    /// `index`: floats are bit-cast into integer words, and the unused
    /// words are zero.
    fn union_words_from_member(
        &mut self,
        union: &DirectUnionType,
        index: usize,
        member: ValueRef,
    ) -> std::result::Result<Vec<Val>, String> {
        let member_ty = union.member(index)?.clone();
        let member = self.coerce_value(member, &member_ty)?;
        let mut words = vec![self.builder.iconst(types::I64, index as i64)];
        for (value, abi) in member.values.iter().zip(member_ty.abi_types()) {
            words.push(if abi == types::F64 {
                self.builder.bitcast(types::I64, *value)
            } else {
                *value
            });
        }
        while words.len() < union.payload_words + 1 {
            words.push(self.builder.iconst(types::I64, 0));
        }
        Ok(words)
    }

    /// Member `index` read back from an inline union's words.
    fn union_member_from_words(
        &mut self,
        union: &DirectUnionType,
        index: usize,
        words: &[Val],
    ) -> std::result::Result<ValueRef, String> {
        let member_ty = union.member(index)?.clone();
        let mut values = Vec::new();
        for (word, abi) in words[1..].iter().zip(member_ty.abi_types()) {
            values.push(if abi == types::F64 {
                self.builder.bitcast(types::F64, *word)
            } else {
                *word
            });
        }
        Ok(ValueRef {
            values,
            ty: member_ty,
        })
    }

    /// Runs `body` once per member on the inline union's words, selected by
    /// the tag at run time, and merges the bodies' results, which must share
    /// `result_ty`.
    fn switch_on_union_tag(
        &mut self,
        union: &DirectUnionType,
        words: &[Val],
        result_ty: &DirectType,
        mut body: impl FnMut(&mut Self, usize, ValueRef) -> std::result::Result<ValueRef, String>,
    ) -> std::result::Result<ValueRef, String> {
        let merge = self.builder.create_block();
        for abi in result_ty.abi_types() {
            self.builder.append_block_param(merge, abi);
        }
        let tag = words[0];
        let caller_owned = self.layout_scratch.clone();
        let member_count = union.members.len();
        let mut merged_union_owned: Option<bool> = None;
        for index in 0..member_count {
            let matched = self.builder.create_block();
            let next = if index + 1 < member_count {
                Some(self.builder.create_block())
            } else {
                None
            };
            if let Some(next) = next {
                let selected = self.builder.icmp_imm(IntCC::Equal, tag, index as i64);
                self.builder.brif(selected, matched, &[], next, &[]);
            } else {
                self.builder.jump(matched, &[]);
            }
            self.builder.switch_to_block(matched);
            self.builder.seal_block(matched);
            self.layout_scratch = caller_owned.clone();
            let member = self.union_member_from_words(union, index, words)?;
            let result = body(self, index, member)?;
            let result = self.coerce_value(result, result_ty)?;
            // A result the body owns is handed to the merge block: an opaque
            // result's ownership is the caller's decision once for every
            // arm, and an owned union result (a member call returning
            // `str | None`) stays owned across the merge, so the arm must
            // not release its handle with the other temporaries it made.
            let owns_union = self.temporary_owns_union(&result);
            match merged_union_owned {
                Some(expected) if expected != owns_union => {
                    return Err(format!(
                        "direct union switch arms disagree on result ownership for `{}`",
                        union.union_type
                    ));
                }
                Some(_) => {}
                None => merged_union_owned = Some(owns_union),
            }
            self.clear_temporary_opaque_owned(&result);
            self.clear_temporary_union_owned(&result);
            self.release_temporary_owned_since(&caller_owned);
            self.builder.jump(merge, &result.values);
            if let Some(next) = next {
                self.builder.switch_to_block(next);
                self.builder.seal_block(next);
            }
        }
        self.builder.switch_to_block(merge);
        self.builder.seal_block(merge);
        self.restore_scratch_after_branches(caller_owned);
        let merged = ValueRef {
            values: self.builder.block_params(merge),
            ty: result_ty.clone(),
        };
        if merged_union_owned == Some(true) {
            self.mark_temporary_union_owned(&merged);
        }
        Ok(merged)
    }

    /// Boxes an inline union into the runtime's `Value::Union` for a
    /// container, task, queue, or helper boundary.
    fn box_inline_union(
        &mut self,
        union: &DirectUnionType,
        words: &[Val],
    ) -> std::result::Result<ValueRef, String> {
        let union_type = union.union_type.clone();
        let encoded = crate::native_runtime::canonical_runtime_type_name(&union_type);
        let (ptr, len) = self.string_constant(encoded.as_bytes())?;
        let result_ty = DirectType::Opaque(union_type.clone());
        let source = ValueRef {
            values: words.to_vec(),
            ty: DirectType::Union(union.clone()),
        };
        let owned = self.temporary_owns_union(&source);
        if owned {
            self.clear_temporary_union_owned(&source);
        }
        // A retained union already holds the reference the copy takes.
        let retained = owned && self.retained_unions.contains(&words[0]);
        let boxed =
            self.switch_on_union_tag(union, words, &result_ty, |codegen, index, member| {
                // A handle payload of a borrowed union stays owned by the
                // union's place, so the injection copies the value behind
                // the retained handle, as the interpreter copies a place read
                // into an injection. A moved union's payload or a freshly
                // boxed member moves into the boxed value.
                let handle = matches!(member.ty, DirectType::Opaque(_));
                let borrowed_handle = !owned && handle;
                let payload = codegen.ensure_opaque(member)?;
                let (inject, transferred) = if retained && handle {
                    (
                        codegen.runtime[RuntimeFn::UnionInjectCopy],
                        payload.values[0],
                    )
                } else if borrowed_handle {
                    (
                        codegen.runtime[RuntimeFn::UnionInjectCopy],
                        codegen.retain_opaque_handle(payload.values[0]),
                    )
                } else {
                    codegen.mark_temporary_opaque_owned(&payload);
                    (
                        codegen.runtime[RuntimeFn::UnionInject],
                        codegen.transfer_owned_opaque_value(&payload),
                    )
                };
                let tag = codegen.builder.iconst(types::I64, index as i64);
                let call = codegen.builder.call(inject, &[ptr, len, tag, transferred]);
                Ok(ValueRef {
                    values: call.results,
                    ty: DirectType::Opaque(union_type.clone()),
                })
            })?;
        self.mark_temporary_opaque_owned(&boxed);
        Ok(boxed)
    }

    /// Reads a boxed runtime union back into inline words.
    fn unbox_inline_union(
        &mut self,
        union: &DirectUnionType,
        handle: Val,
    ) -> std::result::Result<ValueRef, String> {
        let tag_call = self
            .builder
            .call(self.runtime[RuntimeFn::UnionTag], &[handle]);
        let tag = tag_call.results[0];
        let mut probe = vec![tag];
        for _ in 0..union.payload_words {
            probe.push(self.builder.iconst(types::I64, 0));
        }
        let result_ty = DirectType::Union(union.clone());
        let unboxed =
            self.switch_on_union_tag(union, &probe, &result_ty, |codegen, index, _| {
                let member_ty = union.member(index)?.clone();
                let index_value = codegen.builder.iconst(types::I64, index as i64);
                let call = codegen.builder.call(
                    codegen.runtime[RuntimeFn::UnionPayloadCopy],
                    &[handle, index_value],
                );
                let payload =
                    codegen.owned_opaque_result(call.results, direct_type_to_type(&member_ty));
                let member = codegen.coerce_value(payload, &member_ty)?;
                // The payload copy is owned; an opaque member's handle (or a
                // callable member's boxed words) leaves the statement's
                // temporaries and travels with the union words.
                codegen.clear_temporary_opaque_owned(&member);
                codegen.clear_temporary_callable_owned(&member);
                let words = codegen.union_words_from_member(union, index, member)?;
                Ok(ValueRef {
                    values: words,
                    ty: DirectType::Union(union.clone()),
                })
            })?;
        self.mark_temporary_union_owned(&unboxed);
        Ok(unboxed)
    }

    fn ensure_opaque(&mut self, value: ValueRef) -> std::result::Result<ValueRef, String> {
        match value.ty {
            DirectType::Callable(ref callable) => {
                let signature = callable.signature.clone();
                let handle = self.call_callable_box_on_words(&value.values);
                let boxed = ValueRef {
                    values: vec![handle],
                    ty: DirectType::Opaque(signature),
                };
                self.mark_temporary_opaque_owned(&boxed);
                Ok(boxed)
            }
            DirectType::Opaque(_) => Ok(value),
            DirectType::Union(ref union) => {
                let union = union.clone();
                self.box_inline_union(&union, &value.values)
            }
            DirectType::Scalar(ScalarKind::Int32) => {
                let inst = self
                    .builder
                    .call(self.runtime[RuntimeFn::BoxI32], &[value.values[0]]);
                let boxed = ValueRef {
                    values: inst.results,
                    ty: DirectType::Opaque(Type::named("int32")),
                };
                self.mark_temporary_opaque_owned(&boxed);
                Ok(boxed)
            }
            DirectType::Scalar(ScalarKind::Int64) => {
                let inst = self
                    .builder
                    .call(self.runtime[RuntimeFn::BoxI64], &[value.values[0]]);
                let boxed = ValueRef {
                    values: inst.results,
                    ty: DirectType::Opaque(Type::named("int64")),
                };
                self.mark_temporary_opaque_owned(&boxed);
                Ok(boxed)
            }
            DirectType::Scalar(ScalarKind::Uint64) => {
                let inst = self
                    .builder
                    .call(self.runtime[RuntimeFn::BoxU64], &[value.values[0]]);
                let boxed = ValueRef {
                    values: inst.results,
                    ty: DirectType::Opaque(Type::named("uint64")),
                };
                self.mark_temporary_opaque_owned(&boxed);
                Ok(boxed)
            }
            DirectType::Scalar(ScalarKind::Float32) | DirectType::Scalar(ScalarKind::Float64) => {
                let inst = self
                    .builder
                    .call(self.runtime[RuntimeFn::BoxF64], &[value.values[0]]);
                let boxed = ValueRef {
                    values: inst.results,
                    ty: DirectType::Opaque(Type::named("float64")),
                };
                self.mark_temporary_opaque_owned(&boxed);
                Ok(boxed)
            }
            DirectType::Scalar(ScalarKind::Bool) => {
                let inst = self
                    .builder
                    .call(self.runtime[RuntimeFn::BoxBool], &[value.values[0]]);
                let boxed = ValueRef {
                    values: inst.results,
                    ty: DirectType::Opaque(Type::named("bool")),
                };
                self.mark_temporary_opaque_owned(&boxed);
                Ok(boxed)
            }
            DirectType::Scalar(ScalarKind::Unit) => {
                let inst = self.builder.call(self.runtime[RuntimeFn::BoxUnit], &[]);
                let boxed = ValueRef {
                    values: inst.results,
                    ty: DirectType::Opaque(Type::Unit),
                };
                self.mark_temporary_opaque_owned(&boxed);
                Ok(boxed)
            }
            DirectType::PlainClass(class) => {
                let (class_ptr, class_len) = self.string_constant(class.class_name.as_bytes())?;
                let init = self.builder.call(
                    self.runtime[RuntimeFn::InstanceEmpty],
                    &[class_ptr, class_len],
                );
                let current = init.results[0];
                let mut start = 0usize;
                for field in &class.fields {
                    let end = start + field.ty.value_count();
                    let field_value = ValueRef {
                        values: value.values[start..end].to_vec(),
                        ty: field.ty.clone(),
                    };
                    let field_value = self.ensure_opaque(field_value)?;
                    let field_value = self.transfer_owned_opaque_value(&field_value);
                    let (field_ptr, field_len) = self.string_constant(field.name.as_bytes())?;
                    self.builder.call(
                        self.runtime[RuntimeFn::InstanceSetFieldOwned],
                        &[current, field_ptr, field_len, field_value],
                    );
                    start = end;
                }
                let boxed = ValueRef {
                    values: vec![current],
                    ty: DirectType::Opaque(Type::named(&class.class_name)),
                };
                self.mark_temporary_opaque_owned(&boxed);
                Ok(boxed)
            }
        }
    }

    fn binary_opcode(op: BinaryOp) -> i64 {
        match op {
            BinaryOp::Add => 0,
            BinaryOp::Sub => 1,
            BinaryOp::Mul => 2,
            BinaryOp::Div => 3,
            BinaryOp::Mod => 4,
            BinaryOp::Eq => 5,
            BinaryOp::NotEq => 6,
            BinaryOp::Less => 7,
            BinaryOp::LessEq => 8,
            BinaryOp::Greater => 9,
            BinaryOp::GreaterEq => 10,
            BinaryOp::And => 11,
            BinaryOp::Or => 12,
            BinaryOp::FloorDiv => 13,
            BinaryOp::Pow => 14,
            BinaryOp::BitAnd => 15,
            BinaryOp::BitOr => 16,
            BinaryOp::BitXor => 17,
            BinaryOp::Shl => 18,
            BinaryOp::Shr => 19,
        }
    }

    fn string_constant(&mut self, bytes: &[u8]) -> std::result::Result<(Val, Val), String> {
        declare_string_constant(self.string_data, &mut *self.builder, bytes)
    }

    /// Compiles a tuple literal as one opaque aggregate handle.
    ///
    /// Frontend integration calls this from `Rvalue::TupleLiteral`; the
    /// semantic tuple type is passed explicitly so the opaque result keeps its
    /// exact structural runtime type. Elements are evaluated in source order,
    /// coerced to their semantic element types, then transferred into the
    /// aggregate. Destructive extraction is kept in a separate private-temp
    /// helper so this path cannot create user-visible partial moves.
    fn compile_tuple_literal(
        &mut self,
        elements: &[Operand],
        element_types: &[Type],
        tuple_type: Type,
    ) -> std::result::Result<ValueRef, String> {
        debug_assert_eq!(elements.len(), element_types.len());
        let count = self.builder.iconst(types::I64, elements.len() as i64);
        let buffer = if elements.is_empty() {
            self.builder.iconst(types::I64, 0)
        } else {
            let buffer_inst = self
                .builder
                .call(self.runtime[RuntimeFn::ArgBufferNew], &[count]);
            let buffer = buffer_inst.results[0];
            for (index, (element, element_type)) in elements.iter().zip(element_types).enumerate() {
                let direct_type = ensure_direct_type(element_type, &self.classes, "tuple element")?;
                let value = self.load_operand_for_target(element, &direct_type)?;
                let value = self.coerce_value(value, &direct_type)?;
                let value = self.ensure_opaque(value)?;
                let transferred = self.transfer_owned_opaque_value(&value);
                let index = self.builder.iconst(types::I64, index as i64);
                self.builder.call(
                    self.runtime[RuntimeFn::ArgBufferStoreOwned],
                    &[buffer, index, transferred],
                );
            }
            buffer
        };
        let inst = self
            .builder
            .call(self.runtime[RuntimeFn::TupleNew], &[buffer, count]);
        let tuple = self.owned_opaque_result(inst.results, tuple_type.clone());
        self.tag_opaque_runtime_type(&tuple, &tuple_type)?;
        Ok(tuple)
    }

    /// Compiles a constant-index read of a Copy tuple element.
    ///
    /// The checker must reject projection when the selected element is
    /// non-Copy. The runtime returns an independently owned boxed clone, which
    /// is then coerced back to the element's direct representation.
    fn compile_tuple_element(
        &mut self,
        tuple: &Operand,
        index: usize,
        element_type: &Type,
    ) -> std::result::Result<ValueRef, String> {
        validate_tuple_projection_operand(tuple)?;
        let tuple = self.load_operand(tuple)?;
        let tuple = self.ensure_opaque(tuple)?;
        let index = self.builder.iconst(types::I64, index as i64);
        let inst = self.builder.call(
            self.runtime[RuntimeFn::TupleElement],
            &[tuple.values[0], index],
        );
        let element = self.owned_opaque_result(inst.results, element_type.clone());
        let direct_type = ensure_direct_type(element_type, &self.classes, "tuple element")?;
        self.coerce_value(element, &direct_type)
    }

    /// Destructively extracts one element from a private captured tuple.
    ///
    /// Lowering must first move the entire user-visible source into a `%t...`
    /// temporary. This operation may then empty selected slots while recursive
    /// destructuring proceeds; no public source projection remains live.
    fn compile_take_tuple_element(
        &mut self,
        place: &str,
        index: usize,
        element_type: &Type,
    ) -> std::result::Result<ValueRef, String> {
        validate_tuple_take_place(place)?;
        let tuple = self.load_place(place)?;
        let tuple = self.ensure_opaque(tuple)?;
        let index = self.builder.iconst(types::I64, index as i64);
        let inst = self.builder.call(
            self.runtime[RuntimeFn::TupleTakeElement],
            &[tuple.values[0], index],
        );
        let element = self.owned_opaque_result(inst.results, element_type.clone());
        let direct_type = ensure_direct_type(element_type, &self.classes, "tuple element")?;
        self.coerce_value(element, &direct_type)
    }

    fn compile_enum_variant_for_target(
        &mut self,
        enum_name: &str,
        variant_name: &str,
        payloads: &[Operand],
        target: Option<&DirectType>,
    ) -> std::result::Result<ValueRef, String> {
        let (enum_ptr, enum_len) = self.string_constant(enum_name.as_bytes())?;
        let (variant_ptr, variant_len) = self.string_constant(variant_name.as_bytes())?;
        let expected_payload_types = target.and_then(|target_ty| {
            enum_variant_payload_types_for_target(enum_name, variant_name, target_ty, &self.classes)
        });
        let payload_buffer = if payloads.is_empty() {
            self.builder.iconst(types::I64, 0)
        } else {
            let count = self.builder.iconst(types::I64, payloads.len() as i64);
            let buffer_inst = self
                .builder
                .call(self.runtime[RuntimeFn::ArgBufferNew], &[count]);
            let buffer = buffer_inst.results[0];
            for (index, payload) in payloads.iter().enumerate() {
                let loaded = if let Some(expected_ty) = expected_payload_types
                    .as_ref()
                    .and_then(|types| types.get(index))
                {
                    self.load_operand_for_target(payload, expected_ty)?
                } else {
                    self.load_operand(payload)?
                };
                let loaded = if let Some(expected_payload_types) = expected_payload_types.as_ref() {
                    if let Some(expected_ty) = expected_payload_types.get(index) {
                        self.coerce_value(loaded, expected_ty)?
                    } else {
                        loaded
                    }
                } else {
                    loaded
                };
                let payload = self.ensure_opaque(loaded)?;
                let transferred = self.transfer_owned_opaque_value(&payload);
                let index_value = self.builder.iconst(types::I64, index as i64);
                self.builder.call(
                    self.runtime[RuntimeFn::ArgBufferStoreOwned],
                    &[buffer, index_value, transferred],
                );
            }
            buffer
        };
        let payload_count = self.builder.iconst(types::I64, payloads.len() as i64);
        let inst = self.builder.call(
            self.runtime[RuntimeFn::EnumVariant],
            &[
                enum_ptr,
                enum_len,
                variant_ptr,
                variant_len,
                payload_buffer,
                payload_count,
            ],
        );
        Ok(self.owned_opaque_result(
            inst.results,
            target
                .map(direct_type_to_type)
                .unwrap_or_else(|| Type::named(enum_name)),
        ))
    }

    fn compile_enum_variant_from_values(
        &mut self,
        enum_name: &str,
        variant_name: &str,
        payloads: Vec<ValueRef>,
        target: Type,
    ) -> std::result::Result<ValueRef, String> {
        let (enum_ptr, enum_len) = self.string_constant(enum_name.as_bytes())?;
        let (variant_ptr, variant_len) = self.string_constant(variant_name.as_bytes())?;
        let count = self.builder.iconst(types::I64, payloads.len() as i64);
        let buffer = if payloads.is_empty() {
            self.builder.iconst(types::I64, 0)
        } else {
            let buffer_inst = self
                .builder
                .call(self.runtime[RuntimeFn::ArgBufferNew], &[count]);
            let buffer = buffer_inst.results[0];
            for (index, payload) in payloads.into_iter().enumerate() {
                let payload = self.ensure_opaque(payload)?;
                let transferred = self.transfer_owned_opaque_value(&payload);
                let index = self.builder.iconst(types::I64, index as i64);
                self.builder.call(
                    self.runtime[RuntimeFn::ArgBufferStoreOwned],
                    &[buffer, index, transferred],
                );
            }
            buffer
        };
        let inst = self.builder.call(
            self.runtime[RuntimeFn::EnumVariant],
            &[enum_ptr, enum_len, variant_ptr, variant_len, buffer, count],
        );
        Ok(self.owned_opaque_result(inst.results, target))
    }

    /// A payload bound to a scalar local, read where it lies instead of
    /// copied into a box and unboxed. `None` for any other assignment.
    fn compile_scalar_variant_payload(
        &mut self,
        target: &str,
        value: &Rvalue,
    ) -> std::result::Result<Option<ValueRef>, String> {
        let Rvalue::VariantPayload {
            scrutinee: scrutinee @ Operand::Place(place),
            index,
            ..
        } = value
        else {
            return Ok(None);
        };
        if self.lookup_slots.contains(place.as_str()) {
            return Ok(None);
        }
        let target_ty = self.type_of_place(target)?;
        let lent = self.lent_places.contains(place.as_str());
        let function = match (&target_ty, lent) {
            (DirectType::Scalar(ScalarKind::Int32), false) => RuntimeFn::VariantPayloadI64,
            (DirectType::Scalar(ScalarKind::Int64), false) => RuntimeFn::VariantPayloadInt64,
            (DirectType::Scalar(ScalarKind::Uint64), false) => RuntimeFn::VariantPayloadU64,
            (DirectType::Scalar(ScalarKind::Bool), false) => RuntimeFn::VariantPayloadBool,
            (DirectType::Scalar(ScalarKind::Int32), true) => RuntimeFn::LentVariantPayloadI64,
            (DirectType::Scalar(ScalarKind::Int64), true) => RuntimeFn::LentVariantPayloadInt64,
            (DirectType::Scalar(ScalarKind::Uint64), true) => RuntimeFn::LentVariantPayloadU64,
            (DirectType::Scalar(ScalarKind::Bool), true) => RuntimeFn::LentVariantPayloadBool,
            (_, true) => {
                return Err(format!(
                    "direct backend cannot read a lent payload into `{target}`"
                ))
            }
            _ => return Ok(None),
        };
        let scrutinee = self.load_operand(scrutinee)?;
        let scrutinee = self.ensure_opaque(scrutinee)?;
        let index = self.builder.iconst(types::I64, *index as i64);
        let inst = self
            .builder
            .call(self.runtime[function], &[scrutinee.values[0], index]);
        let scalar = ValueRef {
            values: inst.results,
            ty: target_ty.clone(),
        };
        // The same checks as unboxing a copy: an `int32` stays in range.
        self.coerce_value(scalar, &target_ty).map(Some)
    }

    /// `payload` in place of `lookup` while a lookup slot asks for the
    /// payload form, which this records. Otherwise `lookup`. A slot whose
    /// found value is lent takes `lent` instead when the collection handle
    /// is the one its place holds, not a copy this instruction frees.
    fn lookup_function(
        &mut self,
        lookup: FuncRef,
        payload: RuntimeFn,
        lent: RuntimeFn,
        in_place: bool,
    ) -> FuncRef {
        if self.lookup_payload == Some(false) {
            self.lookup_payload = Some(true);
            if self.lend_lookup && in_place {
                self.runtime[lent]
            } else {
                self.runtime[payload]
            }
        } else {
            lookup
        }
    }

    /// Assigns a `get` to a lookup slot: the found value's box, or null.
    /// A `get` that compiled to a path that builds the `Lookup` converts it.
    fn compile_lookup_slot_assign(
        &mut self,
        target: &str,
        value: &Rvalue,
    ) -> std::result::Result<(), String> {
        let target_ty = self.type_of_place(target)?;
        self.lookup_payload = Some(false);
        self.lend_lookup = self.lent_places.contains(target);
        let compiled = self.compile_rvalue_for_target(value, &target_ty);
        self.lend_lookup = false;
        let payload_form = self.lookup_payload.take() == Some(true);
        let compiled = self.ensure_opaque(compiled?)?;
        let payload = if payload_form {
            compiled
        } else {
            let lookup = self.transfer_owned_opaque_value(&compiled);
            let inst = self
                .builder
                .call(self.runtime[RuntimeFn::LookupPayload], &[lookup]);
            self.owned_opaque_result(inst.results, direct_type_to_type(&target_ty))
        };
        self.store_place(
            target,
            ValueRef {
                values: payload.values,
                ty: target_ty,
            },
        )
    }

    /// The payload of a lookup slot: a new reference to the found value's
    /// box, or the box itself when the slot moves out.
    fn compile_lookup_slot_payload(
        &mut self,
        place: &str,
        moved: bool,
    ) -> std::result::Result<ValueRef, String> {
        let payload = if moved {
            self.take_place(place)?
        } else {
            let slot = self.load_place(place)?;
            let inst = self
                .builder
                .call(self.runtime[RuntimeFn::RetainValue], &[slot.values[0]]);
            let retained = ValueRef {
                values: inst.results,
                ty: slot.ty,
            };
            self.mark_temporary_opaque_owned(&retained);
            retained
        };
        Ok(ValueRef {
            values: payload.values,
            ty: DirectType::Opaque(Type::named("Unknown")),
        })
    }

    fn variant_matches_value(
        &mut self,
        value: Val,
        enum_name: &str,
        variant_name: &str,
    ) -> std::result::Result<Val, String> {
        self.variant_matches_with(RuntimeFn::VariantMatches, value, enum_name, variant_name)
    }

    /// A variant test by `test`, which reads a box or a lent value.
    fn variant_matches_with(
        &mut self,
        test: RuntimeFn,
        value: Val,
        enum_name: &str,
        variant_name: &str,
    ) -> std::result::Result<Val, String> {
        let (enum_ptr, enum_len) = self.string_constant(enum_name.as_bytes())?;
        let (variant_ptr, variant_len) = self.string_constant(variant_name.as_bytes())?;
        let inst = self.builder.call(
            self.runtime[test],
            &[value, enum_ptr, enum_len, variant_ptr, variant_len],
        );
        Ok(inst.results[0])
    }

    fn compile_variant_payload(
        &mut self,
        scrutinee: ValueRef,
        index: usize,
    ) -> std::result::Result<ValueRef, String> {
        let scrutinee = self.ensure_opaque(scrutinee)?;
        let index = self.builder.iconst(types::I64, index as i64);
        let inst = self.builder.call(
            self.runtime[RuntimeFn::VariantPayload],
            &[scrutinee.values[0], index],
        );
        let payload = ValueRef {
            values: inst.results,
            ty: DirectType::Opaque(Type::named("Unknown")),
        };
        self.mark_temporary_opaque_owned(&payload);
        Ok(payload)
    }

    fn compile_take_variant_payload(
        &mut self,
        place: &str,
        index: usize,
    ) -> std::result::Result<ValueRef, String> {
        let scrutinee = self.load_place(place)?;
        self.compile_take_variant_payload_from_value(scrutinee, index)
    }

    fn compile_take_variant_payload_from_value(
        &mut self,
        scrutinee: ValueRef,
        index: usize,
    ) -> std::result::Result<ValueRef, String> {
        let scrutinee = self.ensure_opaque(scrutinee)?;
        let index = self.builder.iconst(types::I64, index as i64);
        let inst = self.builder.call(
            self.runtime[RuntimeFn::VariantTakePayload],
            &[scrutinee.values[0], index],
        );
        Ok(self.owned_opaque_result(inst.results, Type::named("Unknown")))
    }

    fn compile_try_assign(
        &mut self,
        target: &str,
        target_ty: DirectType,
        try_value: &Operand,
        releases_on_error: &[String],
        from: Option<&str>,
    ) -> std::result::Result<(), String> {
        let loaded = self.load_operand(try_value)?;
        let (source_error_ty, target_error_ty) = match (&loaded.ty, &self.return_type) {
            (
                DirectType::Opaque(Type::Named(source_name, source_args)),
                Type::Named(target_name, target_args),
            ) if source_name == "Result"
                && source_args.len() == 2
                && target_name == "Result"
                && target_args.len() == 2 =>
            {
                (source_args[1].clone(), target_args[1].clone())
            }
            _ => {
                return Err(format!(
                    "direct backend `try` for `{target}` requires Result types, found operand `{}` and return `{}`",
                    render_direct_type(&loaded.ty),
                    self.return_type
                ))
            }
        };
        let value = self.ensure_opaque(loaded)?;
        let source_is_owned = self.temporary_owns_opaque(&value);
        if source_is_owned {
            // Ownership must be modeled independently in each CFG successor. Keeping
            // this value in the compile-time temporary set while emitting the first
            // successor would make the second successor inherit the first one's
            // release/transfer decision.
            self.clear_temporary_opaque_owned(&value);
        }
        let ok = self.variant_matches_value(value.values[0], "Result", "Ok")?;
        let ok_block = self.builder.create_block();
        let err_block = self.builder.create_block();
        let join_block = self.builder.create_block();
        self.builder.brif(ok, ok_block, &[], err_block, &[]);

        self.builder.switch_to_block(ok_block);
        if source_is_owned {
            self.mark_temporary_opaque_owned(&value);
        }
        let payload = if source_is_owned {
            self.compile_take_variant_payload_from_value(value.clone(), 0)?
        } else {
            self.compile_variant_payload(value.clone(), 0)?
        };
        if source_is_owned {
            self.clear_temporary_opaque_owned(&value);
            self.release_opaque_handle(value.values[0]);
        }
        let coerced = self.coerce_value(payload, &target_ty)?;
        self.store_place(target, coerced)?;
        self.release_all_temporary_owned();
        self.builder.jump(join_block, &[]);
        self.builder.seal_block(ok_block);

        self.builder.switch_to_block(err_block);
        if source_is_owned {
            self.mark_temporary_opaque_owned(&value);
        }
        let error_result = if source_error_ty == target_error_ty {
            value.clone()
        } else {
            let payload = if source_is_owned {
                self.compile_take_variant_payload_from_value(value.clone(), 0)?
            } else {
                self.compile_variant_payload(value.clone(), 0)?
            };
            if source_is_owned {
                self.clear_temporary_opaque_owned(&value);
                self.release_opaque_handle(value.values[0]);
            }
            let from = from.ok_or_else(|| {
                format!(
                    "direct backend `try` names no `From[{source_error_ty}] for {target_error_ty}` conversion"
                )
            })?;
            let converted = self.convert_try_error_via_from(payload, from, &source_error_ty)?;
            self.compile_enum_variant_from_values(
                "Result",
                "Err",
                vec![converted],
                self.return_type.clone(),
            )?
        };
        self.emit_return_value(error_result, releases_on_error)?;
        self.builder.seal_block(err_block);

        self.builder.switch_to_block(join_block);
        self.builder.seal_block(join_block);
        Ok(())
    }

    /// Converts a `try` error payload with the `From` method MIR names.
    fn convert_try_error_via_from(
        &mut self,
        payload: ValueRef,
        function_name: &str,
        source_ty: &Type,
    ) -> std::result::Result<ValueRef, String> {
        let function_name = function_name.to_string();
        let expected = self
            .function_param_types
            .get(&function_name)
            .and_then(|parameters| parameters.first())
            .cloned()
            .unwrap_or(DirectType::Opaque(source_ty.clone()));
        let payload = self.coerce_value(payload, &expected)?;
        let arguments = if matches!(payload.ty, DirectType::Opaque(_)) {
            vec![self.transfer_opaque_arg(&payload)]
        } else {
            payload.values
        };
        let Some(function) = self.function_refs.get(&function_name).copied() else {
            return Err(format!(
                "direct backend does not know From function `{function_name}`"
            ));
        };
        let inst = self.builder.call(function, &arguments);
        let results = inst.results;
        let (converted, writebacks) = self.split_call_results(&function_name, results)?;
        if !writebacks.is_empty() {
            return Err(
                "direct backend From conversion unexpectedly returned writebacks".to_string(),
            );
        }
        Ok(converted)
    }

    fn mutable_sink_for_static_place(&mut self, place: &str) -> std::result::Result<Val, String> {
        let (root, projection) = place.split_once('.').unwrap_or((place, ""));
        if let Some(index) = self.mutable_param_indices.get(root).copied() {
            let index = self.builder.iconst(types::I64, index as i64);
            let current = self
                .builder
                .call(self.runtime[RuntimeFn::CurrentMutableSink], &[index]);
            let parent = current.results[0];
            let (projection_ptr, projection_len) = self.string_constant(projection.as_bytes())?;
            let projected = self.builder.call(
                self.runtime[RuntimeFn::MutableSinkProject],
                &[parent, projection_ptr, projection_len],
            );
            return Ok(projected.results[0]);
        }

        let cleanup_place = self
            .cleanup_places
            .iter()
            .filter_map(|cleanup_place| {
                if place == cleanup_place {
                    Some((cleanup_place.clone(), String::new()))
                } else {
                    place
                        .strip_prefix(&format!("{cleanup_place}."))
                        .map(|projection| (cleanup_place.clone(), projection.to_string()))
                }
            })
            .max_by_key(|(cleanup_place, _)| cleanup_place.len());
        let Some((cleanup_place, projection)) = cleanup_place else {
            return Ok(self.builder.iconst(types::I64, 0));
        };
        let registration_variable = self
            .cleanup_registration_vars
            .get(&cleanup_place)
            .copied()
            .ok_or_else(|| {
                format!("direct backend has no cleanup registration for mutable place `{place}`")
            })?;
        let registration_id = self.builder.use_var(registration_variable);
        let root = self.load_static_place(&cleanup_place)?;
        let root = self.ensure_opaque(root)?;
        let root = self.transfer_owned_opaque_value(&root);
        let (projection_ptr, projection_len) = self.string_constant(projection.as_bytes())?;
        let sink = self.builder.call(
            self.runtime[RuntimeFn::MutableSinkNew],
            &[registration_id, root, projection_ptr, projection_len],
        );
        Ok(sink.results[0])
    }

    fn mutable_sink_for_resolved_place(
        &mut self,
        resolved: DirectViewPlace,
    ) -> std::result::Result<Val, String> {
        if resolved.alternatives.is_empty() {
            return Ok(self.builder.iconst(types::I64, 0));
        }
        if resolved.alternatives.len() == 1 && resolved.alternatives[0].conditions.is_empty() {
            return self.mutable_sink_for_static_place(&resolved.alternatives[0].place);
        }

        let sink_slot = self.builder.create_sized_stack_slot(8, 3);
        let sink_ptr = self.builder.stack_addr(types::I64, sink_slot, 0);
        let zero = self.builder.iconst(types::I64, 0);
        self.builder.store(MemFlags::new(), zero, sink_ptr, 0);
        let merge = self.builder.create_block();
        let alternative_count = resolved.alternatives.len();
        for (index, alternative) in resolved.alternatives.into_iter().enumerate() {
            if index + 1 < alternative_count {
                let selected = self.view_alternative_condition(&alternative);
                let create_block = self.builder.create_block();
                let next_block = self.builder.create_block();
                self.builder
                    .brif(selected, create_block, &[], next_block, &[]);
                self.builder.switch_to_block(create_block);
                let sink = self.mutable_sink_for_static_place(&alternative.place)?;
                self.builder.store(MemFlags::new(), sink, sink_ptr, 0);
                self.builder.jump(merge, &[]);
                self.builder.seal_block(create_block);
                self.builder.switch_to_block(next_block);
                self.builder.seal_block(next_block);
            } else {
                let sink = self.mutable_sink_for_static_place(&alternative.place)?;
                self.builder.store(MemFlags::new(), sink, sink_ptr, 0);
                self.builder.jump(merge, &[]);
            }
        }
        self.builder.switch_to_block(merge);
        self.builder.seal_block(merge);
        Ok(self.builder.load(types::I64, MemFlags::new(), sink_ptr, 0))
    }

    fn mutable_sink_for_place(&mut self, place: &str) -> std::result::Result<Val, String> {
        let resolved = self.resolve_view_place(place)?;
        self.mutable_sink_for_resolved_place(resolved)
    }

    fn install_direct_mutable_sinks(&mut self, sinks: &[Val]) -> std::result::Result<(), String> {
        let count = self.builder.iconst(types::I64, sinks.len() as i64);
        let pointer = if sinks.is_empty() {
            self.builder.iconst(types::I64, 0)
        } else {
            let byte_len = u32::try_from(sinks.len().saturating_mul(8))
                .map_err(|_| "direct mutable sink buffer is too large".to_string())?;
            let slot = self.builder.create_sized_stack_slot(byte_len, 3);
            let pointer = self.builder.stack_addr(types::I64, slot, 0);
            for (index, sink) in sinks.iter().copied().enumerate() {
                self.builder
                    .store(MemFlags::new(), sink, pointer, (index as i32) * 8);
            }
            pointer
        };
        self.builder.call(
            self.runtime[RuntimeFn::SetNextMutableSinks],
            &[pointer, count],
        );
        Ok(())
    }

    /// Hands the caller's mutable sinks to the runtime for a call through a
    /// callable value: public sinks by parameter index and capture sinks by
    /// capture index, which the callee's claim combines into its own
    /// parameter order. A trap inside the callee publishes its mutable
    /// parameters through the sinks before cleanup.
    fn install_indirect_mutable_sinks(
        &mut self,
        public_sinks: &[Val],
        capture_sinks: &[(usize, Val)],
    ) -> std::result::Result<(), String> {
        let store_buffer =
            |compiler: &mut Self, values: &[Val]| -> std::result::Result<Val, String> {
                if values.is_empty() {
                    return Ok(compiler.builder.iconst(types::I64, 0));
                }
                let byte_len = u32::try_from(values.len().saturating_mul(8))
                    .map_err(|_| "direct mutable sink buffer is too large".to_string())?;
                let slot = compiler.builder.create_sized_stack_slot(byte_len, 3);
                let pointer = compiler.builder.stack_addr(types::I64, slot, 0);
                for (index, value) in values.iter().copied().enumerate() {
                    compiler
                        .builder
                        .store(MemFlags::new(), value, pointer, (index as i32) * 8);
                }
                Ok(pointer)
            };
        let public_ptr = store_buffer(self, public_sinks)?;
        let capture_indices = capture_sinks
            .iter()
            .map(|(index, _)| self.builder.iconst(types::I64, *index as i64))
            .collect::<Vec<_>>();
        let capture_values = capture_sinks
            .iter()
            .map(|(_, sink)| *sink)
            .collect::<Vec<_>>();
        let capture_indices_ptr = store_buffer(self, &capture_indices)?;
        let capture_values_ptr = store_buffer(self, &capture_values)?;
        let public_count = self.builder.iconst(types::I64, public_sinks.len() as i64);
        let capture_count = self.builder.iconst(types::I64, capture_sinks.len() as i64);
        self.builder.call(
            self.runtime[RuntimeFn::SetNextIndirectMutableSinks],
            &[
                public_ptr,
                public_count,
                capture_indices_ptr,
                capture_values_ptr,
                capture_count,
            ],
        );
        Ok(())
    }

    fn release_mutable_sinks(&mut self, sinks: impl IntoIterator<Item = Val>) {
        for sink in sinks {
            self.builder
                .call(self.runtime[RuntimeFn::MutableSinkRelease], &[sink]);
        }
    }

    fn publish_mutable_root_write_through(
        &mut self,
        root: &str,
    ) -> std::result::Result<(), String> {
        let Some(index) = self.mutable_param_indices.get(root).copied() else {
            return Ok(());
        };
        let index = self.builder.iconst(types::I64, index as i64);
        let sink = self
            .builder
            .call(self.runtime[RuntimeFn::CurrentMutableSink], &[index]);
        let sink = sink.results[0];
        let zero = self.builder.iconst(types::I64, 0);
        let active = self.builder.icmp(IntCC::NotEqual, sink, zero);
        let publish = self.builder.create_block();
        let done = self.builder.create_block();
        self.builder.brif(active, publish, &[], done, &[]);
        self.builder.switch_to_block(publish);
        let current = self.load_root(root)?;
        let current = self.ensure_opaque(current)?;
        let current = self.transfer_owned_opaque_value(&current);
        self.builder.call(
            self.runtime[RuntimeFn::MutableSinkStoreOwned],
            &[sink, current],
        );
        self.builder.jump(done, &[]);
        self.builder.seal_block(publish);
        self.builder.switch_to_block(done);
        self.builder.seal_block(done);
        Ok(())
    }

    fn set_cleanup_active(&mut self, place: &str, active: bool) -> std::result::Result<(), String> {
        let Some(variable) = self.cleanup_active_vars.get(place).copied() else {
            return Err(format!(
                "direct backend does not know cleanup place `{}`",
                place
            ));
        };
        let value = self.builder.iconst(types::I64, if active { 1 } else { 0 });
        self.builder.def_var(variable, value);
        Ok(())
    }

    fn register_cleanup_for_place(&mut self, place: &str) -> std::result::Result<(), String> {
        let Some(registration_variable) = self.cleanup_registration_vars.get(place).copied() else {
            return Err(format!(
                "direct backend does not know cleanup registration for `{}`",
                place
            ));
        };
        let Some(thunk_ref) = self.cleanup_thunk_refs.get(place).copied() else {
            return Err(format!(
                "direct backend does not know cleanup thunk for `{}`",
                place
            ));
        };

        let loaded = self.load_place(place)?;
        let boxed = self.ensure_opaque(loaded)?;
        let count = self.builder.iconst(types::I64, 1);
        let buffer = self
            .builder
            .call(self.runtime[RuntimeFn::ArgBufferNew], &[count]);
        let buffer = buffer.results[0];
        let zero = self.builder.iconst(types::I64, 0);
        self.builder.call(
            self.runtime[RuntimeFn::ArgBufferStore],
            &[buffer, zero, boxed.values[0]],
        );
        if self.temporary_owns_opaque(&boxed) {
            self.clear_temporary_opaque_owned(&boxed);
            self.release_opaque_handle(boxed.values[0]);
        }
        let thunk_ptr = self.builder.func_addr(types::I64, thunk_ref);
        let registration = self.builder.call(
            self.runtime[RuntimeFn::RegisterCleanup],
            &[thunk_ptr, buffer, count],
        );
        let registration_id = registration.results[0];
        self.builder.def_var(registration_variable, registration_id);
        Ok(())
    }

    fn unregister_cleanup_for_place(&mut self, place: &str) -> std::result::Result<(), String> {
        let Some(variable) = self.cleanup_registration_vars.get(place).copied() else {
            return Err(format!(
                "direct backend does not know cleanup registration for `{}`",
                place
            ));
        };
        let registration_id = self.builder.use_var(variable);
        self.builder.call(
            self.runtime[RuntimeFn::UnregisterCleanup],
            &[registration_id],
        );
        let zero = self.builder.iconst(types::I64, 0);
        self.builder.def_var(variable, zero);
        Ok(())
    }

    fn refresh_cleanup_registration_for_place(
        &mut self,
        place: &str,
    ) -> std::result::Result<(), String> {
        let Some(active_variable) = self.cleanup_active_vars.get(place).copied() else {
            return Err(format!(
                "direct backend does not know cleanup place `{}`",
                place
            ));
        };
        let Some(registration_variable) = self.cleanup_registration_vars.get(place).copied() else {
            return Err(format!(
                "direct backend does not know cleanup registration for `{}`",
                place
            ));
        };
        let Some(thunk_ref) = self.cleanup_thunk_refs.get(place).copied() else {
            return Err(format!(
                "direct backend does not know cleanup thunk for `{}`",
                place
            ));
        };

        let loaded = self.load_place(place)?;
        let boxed = self.ensure_opaque(loaded)?;
        let count = self.builder.iconst(types::I64, 1);
        let buffer = self
            .builder
            .call(self.runtime[RuntimeFn::ArgBufferNew], &[count]);
        let buffer = buffer.results[0];
        let zero = self.builder.iconst(types::I64, 0);
        self.builder.call(
            self.runtime[RuntimeFn::ArgBufferStore],
            &[buffer, zero, boxed.values[0]],
        );
        if self.temporary_owns_opaque(&boxed) {
            self.clear_temporary_opaque_owned(&boxed);
            self.release_opaque_handle(boxed.values[0]);
        }
        let active = self.builder.use_var(active_variable);
        let registration_id = self.builder.use_var(registration_variable);
        let thunk_ptr = self.builder.func_addr(types::I64, thunk_ref);
        let refreshed = self.builder.call(
            self.runtime[RuntimeFn::RefreshCleanup],
            &[active, registration_id, thunk_ptr, buffer, count],
        );
        let refreshed_id = refreshed.results[0];
        self.builder.def_var(registration_variable, refreshed_id);
        Ok(())
    }

    fn refresh_cleanup_registrations_for_mutation(
        &mut self,
        mutated_place: &str,
    ) -> std::result::Result<(), String> {
        let cleanup_places = self
            .cleanup_places
            .clone()
            .into_iter()
            .filter(|place| direct_place_paths_overlap(place, mutated_place))
            .collect::<Vec<_>>();
        for place in cleanup_places {
            self.refresh_cleanup_registration_for_place(&place)?;
        }
        Ok(())
    }

    /// Runs every still-registered cleanup inline before `try` propagation
    /// or a trap. On a trap path, a failing manager exit is recorded for the
    /// trap's notes; on the `try` path its `Result.Err` is dropped (F5).
    fn emit_pending_cleanups(
        &mut self,
        cancel_before_cleanup: bool,
    ) -> std::result::Result<(), String> {
        self.emit_pending_cleanups_for(cancel_before_cleanup, true)
    }

    fn emit_pending_cleanups_for(
        &mut self,
        cancel_before_cleanup: bool,
        trapping: bool,
    ) -> std::result::Result<(), String> {
        // Cleanup branches are generated off the caller's current control-flow
        // path. Temporaries that were already owned by that caller remain live
        // on its continuation and must not disappear from compile-time tracking
        // merely because the cleanup branch releases its own results. Track only
        // cleanup-created temporaries while emitting each run block, then restore
        // the caller's owners at the merge.
        let caller_owned_temporaries = std::mem::take(&mut self.layout_scratch);
        for place in self.cleanup_places.clone().into_iter().rev() {
            let Some(variable) = self.cleanup_active_vars.get(&place).copied() else {
                continue;
            };
            let active = self.builder.use_var(variable);
            let zero = self.builder.iconst(types::I64, 0);
            let should_run = self.builder.icmp(IntCC::NotEqual, active, zero);
            let run_block = self.builder.create_block();
            let next_block = self.builder.create_block();
            self.builder
                .brif(should_run, run_block, &[], next_block, &[]);
            self.builder.switch_to_block(run_block);
            self.unregister_cleanup_for_place(&place)?;
            self.builder.def_var(variable, zero);
            self.emit_cleanup_for_place_recording(&place, cancel_before_cleanup, trapping)?;
            self.release_all_temporary_owned();
            self.builder.jump(next_block, &[]);
            self.builder.seal_block(run_block);
            self.builder.switch_to_block(next_block);
            self.builder.seal_block(next_block);
            self.layout_scratch.clear();
        }
        self.layout_scratch = caller_owned_temporaries;
        Ok(())
    }

    fn append_writeback_return_values(
        &mut self,
        values: &mut Vec<Val>,
    ) -> std::result::Result<(), String> {
        // Each `mut` parameter's words move back to the caller as they are.
        for (name, ty) in self.writeback_locals.clone() {
            let current = self.load_root(&name)?;
            debug_assert_eq!(current.ty, ty, "writeback `{name}`");
            values.extend(current.values);
        }
        Ok(())
    }

    fn split_call_results(
        &mut self,
        function_name: &str,
        results: Vec<Val>,
    ) -> std::result::Result<(ValueRef, Vec<ValueRef>), String> {
        let result_ty = self.call_result_type(function_name)?;
        let result_count = result_ty.value_count();
        if results.len() < result_count {
            return Err(format!(
                "direct backend received too few call results for `{}`",
                function_name
            ));
        }
        let mut cursor = result_count;
        let mut writebacks = Vec::new();
        for ty in self
            .function_writeback_types
            .get(function_name)
            .cloned()
            .unwrap_or_default()
        {
            let count = ty.value_count();
            if results.len() < cursor + count {
                return Err(format!(
                    "direct backend received incomplete writeback results for `{}`",
                    function_name
                ));
            }
            writebacks.push(ValueRef {
                values: results[cursor..cursor + count].to_vec(),
                ty,
            });
            let value = writebacks.last().expect("just pushed writeback");
            match &value.ty {
                DirectType::Opaque(_) => self.mark_temporary_opaque_owned(value),
                DirectType::Union(union) if union.owns_handles() => {
                    self.mark_temporary_union_owned(value)
                }
                DirectType::Callable(_) => self.mark_temporary_callable_owned(value),
                _ => {}
            }
            cursor += count;
        }
        let result = ValueRef {
            values: results[..result_count].to_vec(),
            ty: result_ty,
        };
        // The callee exported one owned reference; the caller adopts it.
        match &result.ty {
            DirectType::Callable(_) => self.mark_temporary_callable_owned(&result),
            DirectType::Opaque(_) => self.mark_temporary_opaque_owned(&result),
            DirectType::Union(union) if union.owns_handles() => {
                self.mark_temporary_union_owned(&result)
            }
            _ => {}
        }
        Ok((result, writebacks))
    }

    fn apply_writeback_places(
        &mut self,
        places: &[String],
        values: Vec<ValueRef>,
    ) -> std::result::Result<(), String> {
        if places.len() != values.len() {
            return Err(format!(
                "direct backend expected {} writeback values but received {}",
                places.len(),
                values.len()
            ));
        }
        for (place, value) in places.iter().zip(values) {
            self.store_place(place, value)?;
        }
        Ok(())
    }

    fn emit_cleanup_for_place(
        &mut self,
        place: &str,
        cancel_before_cleanup: bool,
    ) -> std::result::Result<(), String> {
        self.emit_cleanup_for_place_recording(place, cancel_before_cleanup, false)
    }

    /// Runs one cleanup inline. With `record_exit_failure`, a `Result`
    /// manager exit's value is handed to the runtime so a `Result.Err` is
    /// attached to the trap that is unwinding this frame.
    fn emit_cleanup_for_place_recording(
        &mut self,
        place: &str,
        cancel_before_cleanup: bool,
        record_exit_failure: bool,
    ) -> std::result::Result<(), String> {
        let record_exit_failure = record_exit_failure
            && matches!(
                self.cleanup_actions.get(place),
                Some(CleanupAction::ManagerExit {
                    exit_result: crate::mir::ManagerExitResult::Result
                })
            );
        let receiver_ty = self.type_of_place(place)?;
        let callee =
            self.cleanup_callees.get(place).cloned().ok_or_else(|| {
                format!("direct backend received cleanup `{place}` without a callee")
            })?;
        let operand = Operand::Place(place.to_string());
        match (&receiver_ty, &callee) {
            (DirectType::PlainClass(class_ty), CleanupCallee::Method { function }) => {
                let dispatch = MethodDispatch::Static {
                    function: function.clone(),
                };
                let result = self.compile_member_call(
                    &operand,
                    MemberCallee::Method {
                        field: function,
                        dispatch: &dispatch,
                    },
                    Some(place),
                    &[],
                )?;
                if record_exit_failure {
                    let class_name = class_ty.class_name.clone();
                    self.record_manager_exit_failure_value(&class_name, result)?;
                }
            }
            (DirectType::Opaque(ty), _) => {
                let loaded = self.load_operand(&operand)?;
                match &callee {
                    CleanupCallee::Method { function } => {
                        let result =
                            self.compile_method_call(function, loaded, Some(place), &[])?;
                        if record_exit_failure {
                            if let Type::Named(class_name, _) = ty {
                                self.record_manager_exit_failure_value(class_name, result)?;
                            }
                        }
                    }
                    CleanupCallee::Runtime(op) => {
                        let args = bind_member_call_args(*op, &[])?;
                        self.compile_runtime_op(*op, loaded, Some(place), &args)?;
                    }
                    CleanupCallee::CloseTaskGroup => {
                        let loaded = self.ensure_opaque(loaded)?;
                        let cancel_before = self
                            .builder
                            .iconst(types::I64, if cancel_before_cleanup { 1 } else { 0 });
                        let result = self.builder.call(
                            self.runtime[RuntimeFn::TaskGroupClose],
                            &[loaded.values[0], cancel_before],
                        );
                        self.release_opaque_handle(result.results[0]);
                    }
                    CleanupCallee::Nothing => {
                        let _ = self.ensure_opaque(loaded)?;
                    }
                }
            }
            _ => {}
        }
        Ok(())
    }

    fn record_manager_exit_failure_value(
        &mut self,
        class_name: &str,
        result: ValueRef,
    ) -> std::result::Result<(), String> {
        let boxed = self.ensure_opaque(result)?;
        let (class_ptr, class_len) = self.string_constant(class_name.as_bytes())?;
        self.builder.call(
            self.runtime[RuntimeFn::RecordManagerExitFailure],
            &[boxed.values[0], class_ptr, class_len],
        );
        Ok(())
    }

    /// Calls the method `function` MIR resolved for a member call, with
    /// the receiver first. A `mut self` receiver writes back to its place.
    fn compile_method_call(
        &mut self,
        function: &str,
        object: ValueRef,
        receiver_place: Option<&str>,
        args: &[MirArg],
    ) -> std::result::Result<ValueRef, String> {
        let method_function_name = function.to_string();
        let mutable_receiver =
            self.method_receivers.get(function) == Some(&Some(MirReceiverKind::BorrowMut));
        if mutable_receiver && receiver_place.is_none() {
            return Err(format!(
                "direct backend does not yet support temporary mutable receiver method `{function}`"
            ));
        }
        let func_ref = *self.function_refs.get(&method_function_name).ok_or({
            format!(
                "direct backend does not know function `{}`",
                method_function_name
            )
        })?;
        let expected = self
            .function_param_types
            .get(&method_function_name)
            .cloned()
            .unwrap_or_default();
        let mut lowered_args = Vec::new();
        let mut writeback_places = Vec::new();
        let mut mutable_sink_places = vec![None; expected.len()];
        let receiver_expected = expected.first().cloned().unwrap_or(object.ty.clone());
        let receiver = self.coerce_value(object.clone(), &receiver_expected)?;
        let transferred = self.transfer_values(&receiver);
        lowered_args.extend(transferred);
        if mutable_receiver {
            let Some(place) = receiver_place else {
                return Err(format!(
                    "direct backend does not yet support temporary mutable receiver method `{function}`"
                ));
            };
            writeback_places.push(place.to_string());
            if let Some(slot) = mutable_sink_places.first_mut() {
                *slot = Some(place.to_string());
            }
        }
        for (index, argument) in args.iter().enumerate() {
            let loaded = if let Some(expected_ty) = expected.get(index + 1) {
                self.load_operand_for_target(&argument.value, expected_ty)?
            } else {
                self.load_operand(&argument.value)?
            };
            let coerced = if let Some(expected_ty) = expected.get(index + 1) {
                self.coerce_value(loaded, expected_ty)?
            } else {
                loaded
            };
            if let Some(place) = &argument.writeback_place {
                writeback_places.push(place.clone());
                if let Some(slot) = mutable_sink_places.get_mut(index + 1) {
                    *slot = Some(place.clone());
                }
            }
            let transferred = self.transfer_values(&coerced);
            lowered_args.extend(transferred);
        }
        let mutable_sinks = if mutable_sink_places.iter().any(Option::is_some) {
            let mut sinks = Vec::with_capacity(mutable_sink_places.len());
            for place in &mutable_sink_places {
                sinks.push(match place {
                    Some(place) => self.mutable_sink_for_place(place)?,
                    None => self.builder.iconst(types::I64, 0),
                });
            }
            self.install_direct_mutable_sinks(&sinks)?;
            sinks
        } else {
            Vec::new()
        };
        let inst = self.builder.call(func_ref, &lowered_args);
        let results = inst.results;
        self.release_mutable_sinks(mutable_sinks);
        let (result, writebacks) = self.split_call_results(&method_function_name, results)?;
        self.apply_writeback_places(&writeback_places, writebacks)?;
        Ok(result)
    }

    /// Emits a resolved runtime operation on its loaded receiver. The op
    /// names the helper and its immediate words; this code decides only how
    /// each value is laid out in machine words.
    fn compile_runtime_op(
        &mut self,
        op: RuntimeOp,
        object: ValueRef,
        receiver_place: Option<&str>,
        args: &[MirArg],
    ) -> std::result::Result<ValueRef, String> {
        if !matches!(
            op,
            RuntimeOp::Print(_) | RuntimeOp::Host(_) | RuntimeOp::Function(_)
        ) {
            require_bound_args(op, args)?;
        }
        match op {
            RuntimeOp::Scalar(scalar) => self.compile_scalar_runtime_op(scalar, object, args),
            RuntimeOp::Value(value) => self.compile_value_runtime_op(value, object, args),
            RuntimeOp::Str(StrOp::Slice) => self.compile_slice_runtime_op(op, object, args),
            RuntimeOp::Str(text) if text.is_text_method() => {
                self.compile_str_text_runtime_op(op, object, args)
            }
            RuntimeOp::List(list) => {
                self.compile_list_runtime_op(list, object, receiver_place, args)
            }
            RuntimeOp::Dict(dict) => {
                self.compile_dict_runtime_op(dict, object, receiver_place, args)
            }
            RuntimeOp::Set(set) => self.compile_set_runtime_op(set, object, receiver_place, args),
            RuntimeOp::Array(array) => {
                self.compile_array_runtime_op(array, object, receiver_place, args)
            }
            RuntimeOp::Span(span) => {
                self.compile_span_runtime_op(span, object, receiver_place, args)
            }
            RuntimeOp::Resource(_)
            | RuntimeOp::Rng(_)
            | RuntimeOp::Str(_)
            | RuntimeOp::Queue(_)
            | RuntimeOp::Task(_)
            | RuntimeOp::TaskGroup(_) => self.compile_handle_runtime_op(op, object, args),
            RuntimeOp::Function(_) | RuntimeOp::Print(_) | RuntimeOp::Host(_) => Err(format!(
                "direct backend received named runtime operation `{op}` with a receiver"
            )),
        }
    }

    /// Appends an op's immediate words, then its source line and column
    /// words when the helper takes them. A member call has no source span,
    /// so both are one zero word.
    fn push_runtime_call_words(
        &mut self,
        call: crate::runtime_ops::RuntimeCall,
        words: &mut Vec<Val>,
    ) {
        for immediate in call.immediates {
            words.push(self.builder.iconst(types::I64, *immediate));
        }
        if call.span {
            let zero = self.builder.iconst(types::I64, 0);
            words.extend([zero, zero]);
        }
    }

    /// A method of a resource handle, `random.Rng`, `str`, `Queue`, `Task`,
    /// or `TaskGroup`: the receiver and each argument pass as one word, and
    /// the helper's result word is the value.
    fn compile_handle_runtime_op(
        &mut self,
        op: RuntimeOp,
        object: ValueRef,
        args: &[MirArg],
    ) -> std::result::Result<ValueRef, String> {
        let call = op.runtime_call();
        let receiver_ty = direct_type_to_type(&object.ty);
        let object = self.ensure_opaque(object)?;
        let params = op.params();
        let mut lowered = Vec::with_capacity(params.len());
        let mut writeback = None;
        // MIR binds one argument to each parameter, in order.
        for (param, argument) in params.iter().zip(args) {
            if argument.value == Operand::Default(ArgDefault::Omitted) {
                lowered.push(RuntimeOpArg::Word(self.builder.iconst(types::I64, 0)));
                continue;
            }
            let value = match runtime_op_arg_layout(op, param.name) {
                RuntimeOpArgLayout::Boxed => {
                    let loaded = self.load_operand(&argument.value)?;
                    RuntimeOpArg::Boxed(self.ensure_opaque(loaded)?)
                }
                RuntimeOpArgLayout::BoxedPayload => {
                    let payload = crate::runtime_ops::first_type_arg(Some(&receiver_ty))
                        .unwrap_or(Type::named("Unknown"));
                    let target = ensure_direct_type(&payload, &self.classes, "Queue element")?;
                    RuntimeOpArg::Boxed(
                        self.load_operand_as_opaque_direct(&argument.value, &target)?,
                    )
                }
                RuntimeOpArgLayout::BoxedWriteback => {
                    let loaded = self.load_operand(&argument.value)?;
                    let value = self.ensure_opaque(loaded)?;
                    writeback = Some((value.clone(), argument));
                    RuntimeOpArg::Boxed(value)
                }
                RuntimeOpArgLayout::BoxedInt32 => {
                    let loaded = self.load_operand(&argument.value)?;
                    let value =
                        self.coerce_value(loaded, &DirectType::Scalar(ScalarKind::Int32))?;
                    RuntimeOpArg::Boxed(self.ensure_opaque(value)?)
                }
                RuntimeOpArgLayout::Int64 => {
                    let loaded = self
                        .load_operand_with_integer_hint(&argument.value, Some(ScalarKind::Int64))?;
                    let value =
                        self.coerce_value(loaded, &DirectType::Scalar(ScalarKind::Int64))?;
                    RuntimeOpArg::Word(value.values[0])
                }
            };
            lowered.push(value);
        }
        // An `own` parameter takes its argument: each one is handed over
        // after every argument is loaded.
        let mut words = vec![object.values[0]];
        for (index, value) in lowered.iter().enumerate() {
            words.push(match value {
                RuntimeOpArg::Boxed(value)
                    if op.argument_passing(index) == Some(ReceiverKind::Value) =>
                {
                    self.transfer_owned_opaque_value(value)
                }
                RuntimeOpArg::Boxed(value) => value.values[0],
                RuntimeOpArg::Word(word) => *word,
            });
        }
        self.push_runtime_call_words(call, &mut words);
        let results = self.runtime_call_results(self.runtime[call.function], &words);
        if let Some((value, argument)) = writeback {
            let Some(place) = &argument.writeback_place else {
                return Err(format!(
                    "direct backend expected `{}()` to carry a mutable argument writeback place",
                    op.name()
                ));
            };
            self.store_place(place, value)?;
            return Ok(unit_value(&mut *self.builder));
        }
        let result_ty = op
            .result_type(Some(&receiver_ty), &[])
            .ok_or_else(|| format!("direct backend does not know the result type of `{op}`"))?;
        if runtime_op_returns_boxed_payload(op) {
            return Ok(self.owned_opaque_result(results, result_ty));
        }
        let scalar = match &result_ty {
            // A close helper returns the handle's last reference.
            Type::Unit => {
                self.release_opaque_handle(results[0]);
                return Ok(unit_value(&mut *self.builder));
            }
            Type::Named(name, type_args) if type_args.is_empty() => match name.as_str() {
                "bool" => Some(ScalarKind::Bool),
                "int64" => Some(ScalarKind::Int64),
                "float64" => Some(ScalarKind::Float64),
                "int32" => {
                    self.emit_int32_bounds_check(results[0], None)?;
                    Some(ScalarKind::Int32)
                }
                _ => None,
            },
            _ => None,
        };
        Ok(match scalar {
            Some(kind) => ValueRef {
                values: results,
                ty: DirectType::Scalar(kind),
            },
            None => self.owned_opaque_result(results, result_ty),
        })
    }

    /// A `str` text-convenience method, through the shared text dispatcher.
    /// The dispatcher takes the op's opcode word first, then the receiver,
    /// a text argument word, and a count word, and leaves unused words zero.
    /// The receiver stays shared; a text argument is moved into the call
    /// and a count travels as a scalar.
    fn compile_str_text_runtime_op(
        &mut self,
        op: RuntimeOp,
        object: ValueRef,
        args: &[MirArg],
    ) -> std::result::Result<ValueRef, String> {
        let call = op.runtime_call();
        let object = self.ensure_opaque(object)?;
        let zero = self.builder.iconst(types::I64, 0);
        let (argument, count) = match (op, args.first()) {
            (RuntimeOp::Str(StrOp::Repeat), Some(argument)) => {
                let count =
                    self.load_operand_with_integer_hint(&argument.value, Some(ScalarKind::Int64))?;
                let count = self.coerce_value(count, &DirectType::Scalar(ScalarKind::Int64))?;
                (zero, count.values[0])
            }
            (_, Some(argument)) => {
                let loaded = self.load_operand(&argument.value)?;
                let text = self.ensure_opaque(loaded)?;
                (text.values[0], zero)
            }
            (_, None) => (zero, zero),
        };
        let mut words = Vec::with_capacity(4);
        self.push_runtime_call_words(call, &mut words);
        words.extend([object.values[0], argument, count]);
        let results = self.runtime_call_results(self.runtime[call.function], &words);
        let semantic_type = op
            .result_type(None, &[])
            .expect("text-convenience methods have a result type");
        let result = self.owned_opaque_result(results, semantic_type.clone());
        let context = format!("return value of `str.{}`", op.name());
        let target = ensure_direct_type(&semantic_type, &self.classes, &context)?;
        self.coerce_value(result, &target)
    }

    /// The value of a default argument MIR names. An omitted one has no
    /// value: the op passes a zero word in its place.
    fn load_default_arg(&mut self, default: ArgDefault) -> std::result::Result<ValueRef, String> {
        let stdio = || Type::Named("process.Stdio".to_string(), Vec::new());
        let int = |builder: &mut dyn FunctionBackend, value: i64, kind| ValueRef {
            values: vec![builder.iconst(types::I64, value)],
            ty: DirectType::Scalar(kind),
        };
        Ok(match default {
            ArgDefault::Omitted => {
                return Err(
                    "direct backend received an omitted argument outside a runtime operation"
                        .to_string(),
                )
            }
            ArgDefault::Int64(value) => int(&mut *self.builder, value, ScalarKind::Int64),
            ArgDefault::Int32(value) => int(&mut *self.builder, value.into(), ScalarKind::Int32),
            ArgDefault::EmptyStrDict => {
                let results = self.runtime_call_results(self.runtime[RuntimeFn::MapEmpty], &[]);
                self.owned_opaque_result(
                    results,
                    Type::Named(
                        "dict".to_string(),
                        vec![Type::named("str"), Type::named("str")],
                    ),
                )
            }
            ArgDefault::StdioNull => {
                let results = self.runtime_call_results(self.runtime[RuntimeFn::ProcessNull], &[]);
                self.owned_opaque_result(results, stdio())
            }
            ArgDefault::StdioInherit => {
                let results =
                    self.runtime_call_results(self.runtime[RuntimeFn::ProcessInherit], &[]);
                self.owned_opaque_result(results, stdio())
            }
            ArgDefault::RestartOnFailure => self.compile_enum_variant_for_target(
                "process.RestartPolicy",
                "OnFailure",
                &[],
                None,
            )?,
            ArgDefault::Milliseconds(value) => {
                let milliseconds = self.builder.iconst(types::I64, value);
                let unit_nanoseconds = self.builder.iconst(
                    types::I64,
                    crate::runtime_value::NANOS_PER_MILLISECOND as i64,
                );
                let results = self.runtime_call_results(
                    self.runtime[RuntimeFn::DurationFromI64],
                    &[milliseconds, unit_nanoseconds],
                );
                self.owned_opaque_result(results, Type::named("Duration"))
            }
        })
    }

    /// An integer, `float64`, or `Duration` method.
    fn compile_scalar_runtime_op(
        &mut self,
        op: ScalarOp,
        object: ValueRef,
        args: &[MirArg],
    ) -> std::result::Result<ValueRef, String> {
        let runtime_op = RuntimeOp::Scalar(op);
        let call = runtime_op.runtime_call();
        // The shared validator binds a resolved op's arguments; a member
        // call on an integer literal reaches here unresolved.
        if runtime_op.params().is_empty() && !args.is_empty() {
            return Err(format!(
                "direct backend expected `{}()` to take no arguments",
                runtime_op.name()
            ));
        }
        match op {
            ScalarOp::Sqrt => {
                let results =
                    self.runtime_call_results(self.runtime[call.function], &[object.values[0]]);
                Ok(ValueRef {
                    values: results,
                    ty: object.ty,
                })
            }
            ScalarOp::DurationToMilliseconds | ScalarOp::DurationToSeconds => {
                let mut words = vec![object.values[0]];
                self.push_runtime_call_words(call, &mut words);
                let results = self.runtime_call_results(self.runtime[call.function], &words);
                Ok(ValueRef {
                    values: results,
                    ty: DirectType::Scalar(ScalarKind::Float64),
                })
            }
            ScalarOp::ToFloat => {
                // A scalar integer converts inline; a boxed one calls the
                // helper.
                if let Some(kind) = object.ty.scalar_kind() {
                    let value = if kind == ScalarKind::Uint64 {
                        self.builder.fcvt_from_uint(types::F64, object.values[0])
                    } else {
                        self.builder.fcvt_from_sint(types::F64, object.values[0])
                    };
                    return Ok(ValueRef {
                        values: vec![value],
                        ty: DirectType::Scalar(ScalarKind::Float64),
                    });
                }
                let object = self.ensure_opaque(object)?;
                let results =
                    self.runtime_call_results(self.runtime[call.function], &[object.values[0]]);
                Ok(ValueRef {
                    values: results,
                    ty: DirectType::Scalar(ScalarKind::Float64),
                })
            }
            ScalarOp::WrappingAdd
            | ScalarOp::WrappingSub
            | ScalarOp::WrappingMul
            | ScalarOp::SaturatingAdd
            | ScalarOp::SaturatingSub
            | ScalarOp::SaturatingMul
            | ScalarOp::WrappingShl
            | ScalarOp::WrappingShr
            | ScalarOp::SaturatingShl
            | ScalarOp::SaturatingShr => {
                let argument = &args[0];
                let (target, result_ty) = match &object.ty {
                    DirectType::Opaque(ty) => (
                        ensure_direct_type(ty, &self.classes, "integer receiver")?,
                        ty.clone(),
                    ),
                    other => (other.clone(), direct_type_to_type(other)),
                };
                let left = self.ensure_opaque(object)?;
                let right = self.load_operand_as_opaque_direct(&argument.value, &target)?;
                let mut words = vec![left.values[0], right.values[0]];
                self.push_runtime_call_words(call, &mut words);
                let results = self.runtime_call_results(self.runtime[call.function], &words);
                let result = self.owned_opaque_result(results, result_ty);
                self.coerce_value(result, &target)
            }
        }
    }

    /// `to_string`, `clone`, or `copy`: the boxed receiver is the one word.
    fn compile_value_runtime_op(
        &mut self,
        op: ValueOp,
        object: ValueRef,
        args: &[MirArg],
    ) -> std::result::Result<ValueRef, String> {
        let runtime_op = RuntimeOp::Value(op);
        if !args.is_empty() {
            return Err(format!(
                "direct backend expected `{}()` to take no arguments",
                runtime_op.name()
            ));
        }
        let receiver_ty = direct_type_to_type(&object.ty);
        let result_ty = runtime_op
            .result_type(Some(&receiver_ty), &[])
            .ok_or_else(|| format!("direct backend does not know the result type of `{op:?}`"))?;
        let object = self.ensure_opaque(object)?;
        let results = self.runtime_call_results(
            self.runtime[runtime_op.runtime_call().function],
            &[object.values[0]],
        );
        Ok(self.owned_opaque_result(results, result_ty))
    }

    /// Loads `argument` as one scalar word of `kind`. With `hint`, an
    /// integer literal loads directly as `kind`.
    fn load_word(
        &mut self,
        argument: &MirArg,
        kind: ScalarKind,
        hint: bool,
    ) -> std::result::Result<Val, String> {
        let loaded = if hint {
            self.load_operand_with_integer_hint(&argument.value, Some(kind))?
        } else {
            self.load_operand(&argument.value)?
        };
        Ok(self.coerce_value(loaded, &DirectType::Scalar(kind))?.values[0])
    }

    /// Stores a receiver a method changed in place back to its place.
    fn store_receiver_back(
        &mut self,
        receiver_place: Option<&str>,
        object: &ValueRef,
    ) -> std::result::Result<(), String> {
        if let Some(place) = receiver_place {
            self.store_place(place, object.clone())?;
        }
        Ok(())
    }

    /// A call to the shared collection operation: the receiver, a value
    /// word, an index word, then the op's operation word. A call that
    /// passes only one of the two leaves the other zero.
    fn emit_collection_operation(
        &mut self,
        call: crate::runtime_ops::RuntimeCall,
        collection: Val,
        value: Option<Val>,
        index: Option<Val>,
    ) -> Val {
        let zero = self.builder.iconst(types::I64, 0);
        let mut words = vec![collection, value.unwrap_or(zero), index.unwrap_or(zero)];
        self.push_runtime_call_words(call, &mut words);
        self.runtime_call_results(self.runtime[call.function], &words)[0]
    }

    /// A slice `items[a:b]` of a `str`, `list`, or `Array`. The bounds and
    /// their presence flags pass as words, then the source line and column,
    /// and the result has the receiver's type.
    fn compile_slice_runtime_op(
        &mut self,
        op: RuntimeOp,
        object: ValueRef,
        args: &[MirArg],
    ) -> std::result::Result<ValueRef, String> {
        let call = op.runtime_call();
        let receiver_ty = direct_type_to_type(&object.ty);
        let object = self.ensure_opaque(object)?;
        let args = args.iter().collect::<Vec<_>>();
        let mut words = vec![object.values[0]];
        for (argument, kind) in args.iter().zip(SLICE_WORD_KINDS) {
            words.push(self.load_word(argument, *kind, true)?);
        }
        let results = self.runtime_call_results(self.runtime[call.function], &words);
        let result_ty = op
            .result_type(Some(&receiver_ty), &[])
            .unwrap_or(receiver_ty);
        Ok(self.owned_opaque_result(results, result_ty))
    }

    /// `reserve(additional)` on a `list`, `dict`, or `set`, through the
    /// shared collection operation.
    fn compile_collection_reserve(
        &mut self,
        call: crate::runtime_ops::RuntimeCall,
        object: &ValueRef,
        receiver_place: Option<&str>,
        additional: &MirArg,
    ) -> std::result::Result<ValueRef, String> {
        let additional = self.load_word(additional, ScalarKind::Int64, true)?;
        let result = self.emit_collection_operation(call, object.values[0], None, Some(additional));
        self.release_opaque_handle(result);
        self.store_receiver_back(receiver_place, object)?;
        Ok(unit_value(&mut *self.builder))
    }

    /// A `list` method, or a private read, write, or slice that indexing
    /// and iteration lower to. The op names the helper and its immediate
    /// words. The arms here lay out the arguments and the result, and keep
    /// the inline length and scalar element reads.
    fn compile_list_runtime_op(
        &mut self,
        op: ListOp,
        object: ValueRef,
        receiver_place: Option<&str>,
        mir_args: &[MirArg],
    ) -> std::result::Result<ValueRef, String> {
        let runtime_op = RuntimeOp::List(op);
        let call = runtime_op.runtime_call();
        let function = self.runtime[call.function];
        let list_ty = direct_type_to_type(&object.ty);
        let object = self.ensure_opaque(object)?;
        let list = object.values[0];
        let element_ty = first_type_arg(Some(&list_ty)).unwrap_or(Type::named("Unknown"));
        let element_direct_ty = ensure_direct_type(&element_ty, &self.classes, "Vec element")?;
        let args = mir_args.iter().collect::<Vec<_>>();
        let int64 = ScalarKind::Int64;
        let int32 = ScalarKind::Int32;
        match op {
            ListOp::Slice => self.compile_slice_runtime_op(runtime_op, object, mir_args),
            ListOp::Len => {
                let len = self.emit_list_len(list);
                Ok(ValueRef {
                    values: vec![len],
                    ty: DirectType::Scalar(int64),
                })
            }
            ListOp::IsEmpty => Ok(ValueRef {
                values: self.runtime_call_results(function, &[list]),
                ty: DirectType::Scalar(ScalarKind::Bool),
            }),
            ListOp::PushScalar(kind) => {
                // A scalar is pushed as its bits, with no box.
                let loaded = self.load_operand_with_integer_hint(
                    &args[0].value,
                    element_direct_ty.scalar_kind(),
                )?;
                let value = self.coerce_value(loaded, &element_direct_ty)?.values[0];
                let bits = if kind == ScalarElement::Float64 {
                    self.builder.bitcast(types::I64, value)
                } else {
                    value
                };
                let mut words = vec![list, bits];
                self.push_runtime_call_words(call, &mut words);
                self.emit_scalar_list_push(list, bits, kind, function, &words);
                if let Some(place) = receiver_place {
                    if !self.receiver_store_is_redundant(place) {
                        self.store_place(place, object.clone())?;
                    }
                }
                Ok(unit_value(&mut *self.builder))
            }
            ListOp::Push => {
                let value =
                    self.load_operand_as_opaque_direct(&args[0].value, &element_direct_ty)?;
                let value = self.transfer_owned_opaque_value(&value);
                let results = self.runtime_call_results(function, &[list, value]);
                self.release_opaque_handle(results[0]);
                self.store_receiver_back(receiver_place, &object)?;
                Ok(unit_value(&mut *self.builder))
            }
            ListOp::Pop => {
                let index = self.load_word(args[0], int64, true)?;
                let result = self.emit_collection_operation(call, list, None, Some(index));
                self.store_receiver_back(receiver_place, &object)?;
                Ok(self.owned_opaque_result(vec![result], element_ty))
            }
            ListOp::Get | ListOp::IndexOption => {
                let index = self.load_word(args[0], int64, false)?;
                let results = self.runtime_call_results(function, &[list, index]);
                Ok(self.owned_opaque_result(results, element_ty))
            }
            ListOp::TakeIndexOption => {
                let Some(receiver_place) = receiver_place else {
                    return Err(
                        "direct backend expected internal consuming vector indexing to provide its private receiver place"
                            .to_string(),
                    );
                };
                let index = self.load_word(args[0], int64, false)?;
                let results = self.runtime_call_results(function, &[list, index]);
                self.store_place(receiver_place, object.clone())?;
                Ok(self.owned_opaque_result(results, optional_type(element_ty)))
            }
            ListOp::Index
            | ListOp::IndexScalar(_)
            | ListOp::IndexUnchecked
            | ListOp::IndexScalarUnchecked(_) => {
                let index = self.load_word(args[0], int64, false)?;
                let line = self.load_word(args[1], int32, false)?;
                let column = self.load_word(args[2], int32, false)?;
                let proven = op.is_unchecked();
                if let ListOp::IndexScalar(kind) | ListOp::IndexScalarUnchecked(kind) = op {
                    // A scalar element comes back as raw bits, with no box.
                    let bits = self.emit_scalar_list_read(
                        list,
                        index,
                        call.immediates[0],
                        proven,
                        line,
                        column,
                    )?;
                    let value = if kind == ScalarElement::Float64 {
                        self.builder.bitcast(types::F64, bits)
                    } else {
                        bits
                    };
                    return Ok(ValueRef {
                        values: vec![value],
                        ty: element_direct_ty,
                    });
                }
                let index = if proven {
                    index
                } else {
                    self.emit_vec_index_failure_guard(list, index, line, column)?
                };
                let results = self.runtime_call_results(function, &[list, index, line, column]);
                let indexed = self.owned_opaque_result(results, element_ty);
                self.coerce_value(indexed, &element_direct_ty)
            }
            ListOp::Set | ListOp::Insert => {
                let index = self.load_word(args[0], int64, false)?;
                let value =
                    self.load_operand_as_opaque_direct(&args[1].value, &element_direct_ty)?;
                let value = self.transfer_owned_opaque_value(&value);
                let results = self.runtime_call_results(function, &[list, index, value]);
                self.store_receiver_back(receiver_place, &object)?;
                if op == ListOp::Insert {
                    return Ok(unit_value(&mut *self.builder));
                }
                Ok(self.owned_opaque_result(results, optional_type(element_ty)))
            }
            ListOp::SetIndex => {
                let index = self.load_word(args[0], int64, false)?;
                let value =
                    self.load_operand_as_opaque_direct(&args[1].value, &element_direct_ty)?;
                let line = self.load_word(args[2], int32, false)?;
                let column = self.load_word(args[3], int32, false)?;
                let index = self.emit_vec_index_failure_guard(list, index, line, column)?;
                let value = self.transfer_owned_opaque_value(&value);
                let results =
                    self.runtime_call_results(function, &[list, index, value, line, column]);
                self.release_opaque_handle(results[0]);
                self.store_receiver_back(receiver_place, &object)?;
                Ok(unit_value(&mut *self.builder))
            }
            ListOp::Remove | ListOp::IndexOf | ListOp::Count => {
                let value =
                    self.load_operand_as_opaque_direct(&args[0].value, &element_direct_ty)?;
                let result =
                    self.emit_collection_operation(call, list, Some(value.values[0]), None);
                if op == ListOp::Remove {
                    self.store_receiver_back(receiver_place, &object)?;
                    self.release_opaque_handle(result);
                    return Ok(unit_value(&mut *self.builder));
                }
                let boxed = self.owned_opaque_result(vec![result], Type::named("int64"));
                self.coerce_value(boxed, &DirectType::Scalar(int64))
            }
            ListOp::Swap => {
                let first = self.load_word(args[0], int64, false)?;
                let second = self.load_word(args[1], int64, false)?;
                self.builder.call(function, &[list, first, second]);
                self.store_receiver_back(receiver_place, &object)?;
                Ok(unit_value(&mut *self.builder))
            }
            ListOp::Contains => {
                let value =
                    self.load_operand_as_opaque_direct(&args[0].value, &element_direct_ty)?;
                Ok(ValueRef {
                    values: self.runtime_call_results(function, &[list, value.values[0]]),
                    ty: DirectType::Scalar(ScalarKind::Bool),
                })
            }
            ListOp::Extend => {
                let loaded = self.load_operand(&args[0].value)?;
                let other = self.ensure_opaque(loaded)?;
                let other = self.transfer_owned_opaque_value(&other);
                let results = self.runtime_call_results(function, &[list, other]);
                self.release_opaque_handle(results[0]);
                self.store_receiver_back(receiver_place, &object)?;
                Ok(unit_value(&mut *self.builder))
            }
            ListOp::Clear | ListOp::Reverse => {
                let results = self.runtime_call_results(function, &[list]);
                self.release_opaque_handle(results[0]);
                self.store_receiver_back(receiver_place, &object)?;
                Ok(unit_value(&mut *self.builder))
            }
            ListOp::Reserve => {
                self.compile_collection_reserve(call, &object, receiver_place, args[0])
            }
        }
    }

    /// A `dict` method, or the private element read and write that
    /// indexing lowers to.
    fn compile_dict_runtime_op(
        &mut self,
        op: DictOp,
        object: ValueRef,
        receiver_place: Option<&str>,
        args: &[MirArg],
    ) -> std::result::Result<ValueRef, String> {
        let runtime_op = RuntimeOp::Dict(op);
        let call = runtime_op.runtime_call();
        let function = self.runtime[call.function];
        let dict_ty = direct_type_to_type(&object.ty);
        // A lookup lends from the map only through the handle its place
        // holds, never a copy this instruction frees.
        let in_place = !self.temporary_owns_opaque(&object);
        let object = self.ensure_opaque(object)?;
        let map = object.values[0];
        let (key_ty, value_ty) = match &dict_ty {
            Type::Named(_, entry_types) => (
                entry_types
                    .first()
                    .cloned()
                    .unwrap_or(Type::named("Unknown")),
                entry_types
                    .get(1)
                    .cloned()
                    .unwrap_or(Type::named("Unknown")),
            ),
            _ => (Type::named("Unknown"), Type::named("Unknown")),
        };
        let key_direct_ty = ensure_direct_type(&key_ty, &self.classes, "dict key")?;
        let value_direct_ty = ensure_direct_type(&value_ty, &self.classes, "Map value")?;
        let args = args.iter().collect::<Vec<_>>();
        let int32 = ScalarKind::Int32;
        let list = |element: Type| Type::Named("list".to_string(), vec![element]);
        match op {
            DictOp::Len => Ok(ValueRef {
                values: self.runtime_call_results(function, &[map]),
                ty: DirectType::Scalar(ScalarKind::Int64),
            }),
            DictOp::ContainsKeyInt64 | DictOp::GetInt64 => {
                // An `int64` key is searched for without a box.
                let loaded = self.load_operand_for_target(&args[0].value, &key_direct_ty)?;
                let key = self.coerce_value(loaded, &key_direct_ty)?;
                if op == DictOp::ContainsKeyInt64 {
                    return Ok(ValueRef {
                        values: self.runtime_call_results(function, &[map, key.values[0]]),
                        ty: DirectType::Scalar(ScalarKind::Bool),
                    });
                }
                let function = self.lookup_function(
                    function,
                    RuntimeFn::MapGetInt64Payload,
                    RuntimeFn::MapGetInt64Lent,
                    in_place,
                );
                let results = self.runtime_call_results(function, &[map, key.values[0]]);
                Ok(self.owned_opaque_result(results, optional_type(value_ty)))
            }
            DictOp::IsEmpty | DictOp::ContainsKey => {
                let mut words = vec![map];
                if let Some(key) = args.first() {
                    let key = self.load_operand_as_opaque_direct(&key.value, &key_direct_ty)?;
                    words.push(key.values[0]);
                }
                Ok(ValueRef {
                    values: self.runtime_call_results(function, &words),
                    ty: DirectType::Scalar(ScalarKind::Bool),
                })
            }
            DictOp::GetStr => {
                // A literal `str` key is looked up without a box.
                let Operand::String(text) = &args[0].value else {
                    return Err(format!(
                        "direct backend expected `{runtime_op}` to look up a literal key"
                    ));
                };
                let (key_ptr, key_len) = self.string_constant(text.as_bytes())?;
                let function = self.lookup_function(
                    function,
                    RuntimeFn::MapGetStrPayload,
                    RuntimeFn::MapGetStrLent,
                    in_place,
                );
                let results = self.runtime_call_results(function, &[map, key_ptr, key_len]);
                Ok(self.owned_opaque_result(results, optional_type(value_ty)))
            }
            DictOp::Get | DictOp::Remove => {
                let key = self.load_operand_as_opaque_direct(&args[0].value, &key_direct_ty)?;
                let function = if op == DictOp::Get {
                    self.lookup_function(
                        function,
                        RuntimeFn::MapGetPayload,
                        RuntimeFn::MapGetLent,
                        in_place,
                    )
                } else {
                    function
                };
                let results = self.runtime_call_results(function, &[map, key.values[0]]);
                if op == DictOp::Remove {
                    self.store_receiver_back(receiver_place, &object)?;
                }
                Ok(self.owned_opaque_result(results, optional_type(value_ty)))
            }
            DictOp::Index => {
                let key = self.load_operand_as_opaque_direct(&args[0].value, &key_direct_ty)?;
                let line = self.load_word(args[1], int32, false)?;
                let column = self.load_word(args[2], int32, false)?;
                let results =
                    self.runtime_call_results(function, &[map, key.values[0], line, column]);
                let indexed = self.owned_opaque_result(results, value_ty);
                self.coerce_value(indexed, &value_direct_ty)
            }
            DictOp::Set | DictOp::SetIndex => {
                let key = self.load_operand_as_opaque_direct(&args[0].value, &key_direct_ty)?;
                let value = self.load_operand_as_opaque_direct(&args[1].value, &value_direct_ty)?;
                let mut line_column = Vec::with_capacity(2);
                for argument in &args[2..] {
                    line_column.push(self.load_word(argument, int32, false)?);
                }
                let key = self.transfer_owned_opaque_value(&key);
                let value = self.transfer_owned_opaque_value(&value);
                let mut words = vec![map, key, value];
                words.extend(line_column);
                let results = self.runtime_call_results(function, &words);
                if op == DictOp::SetIndex {
                    self.release_opaque_handle(results[0]);
                    self.store_receiver_back(receiver_place, &object)?;
                    return Ok(unit_value(&mut *self.builder));
                }
                self.store_receiver_back(receiver_place, &object)?;
                Ok(self.owned_opaque_result(results, optional_type(value_ty)))
            }
            DictOp::Keys | DictOp::Values | DictOp::Items => {
                let result_ty = match op {
                    DictOp::Keys => list(key_ty),
                    DictOp::Values => list(value_ty),
                    _ => list(Type::Tuple(vec![key_ty, value_ty])),
                };
                let results = self.runtime_call_results(function, &[map]);
                Ok(self.owned_opaque_result(results, result_ty))
            }
            DictOp::Clear | DictOp::Update => {
                let mut words = vec![map];
                if let Some(other) = args.first() {
                    let loaded = self.load_operand(&other.value)?;
                    let other = self.ensure_opaque(loaded)?;
                    words.push(self.transfer_owned_opaque_value(&other));
                }
                let results = self.runtime_call_results(function, &words);
                self.release_opaque_handle(results[0]);
                self.store_receiver_back(receiver_place, &object)?;
                Ok(unit_value(&mut *self.builder))
            }
            DictOp::Reserve => {
                self.compile_collection_reserve(call, &object, receiver_place, args[0])
            }
        }
    }

    /// A `set` method, or a private iteration read.
    fn compile_set_runtime_op(
        &mut self,
        op: SetOp,
        object: ValueRef,
        receiver_place: Option<&str>,
        args: &[MirArg],
    ) -> std::result::Result<ValueRef, String> {
        let runtime_op = RuntimeOp::Set(op);
        let call = runtime_op.runtime_call();
        let function = self.runtime[call.function];
        let set_ty = direct_type_to_type(&object.ty);
        let object = self.ensure_opaque(object)?;
        let set = object.values[0];
        let element_ty = first_type_arg(Some(&set_ty)).unwrap_or(Type::named("Unknown"));
        let element_direct_ty = ensure_direct_type(&element_ty, &self.classes, "set element")?;
        let args = args.iter().collect::<Vec<_>>();
        match op {
            SetOp::Len => Ok(ValueRef {
                values: self.runtime_call_results(function, &[set]),
                ty: DirectType::Scalar(ScalarKind::Int64),
            }),
            SetOp::IsEmpty => Ok(ValueRef {
                values: self.runtime_call_results(function, &[set]),
                ty: DirectType::Scalar(ScalarKind::Bool),
            }),
            SetOp::Contains | SetOp::ContainsInt64 => {
                let loaded = self.load_operand_for_target(&args[0].value, &element_direct_ty)?;
                let value = self.coerce_value(loaded, &element_direct_ty)?;
                // An `int64` needle is compared without a box.
                let value = if op == SetOp::ContainsInt64 {
                    value
                } else {
                    self.ensure_opaque(value)?
                };
                Ok(ValueRef {
                    values: self.runtime_call_results(function, &[set, value.values[0]]),
                    ty: DirectType::Scalar(ScalarKind::Bool),
                })
            }
            SetOp::Add => {
                let value =
                    self.load_operand_as_opaque_direct(&args[0].value, &element_direct_ty)?;
                let value = self.transfer_owned_opaque_value(&value);
                self.builder.call(function, &[set, value]);
                self.store_receiver_back(receiver_place, &object)?;
                Ok(unit_value(&mut *self.builder))
            }
            // An `int64` element is added without a box.
            SetOp::AddInt64 => {
                let loaded = self.load_operand_for_target(&args[0].value, &element_direct_ty)?;
                let value = self.coerce_value(loaded, &element_direct_ty)?;
                self.builder.call(function, &[set, value.values[0]]);
                self.store_receiver_back(receiver_place, &object)?;
                Ok(unit_value(&mut *self.builder))
            }
            SetOp::Remove | SetOp::Discard => {
                let value =
                    self.load_operand_as_opaque_direct(&args[0].value, &element_direct_ty)?;
                let result = self.emit_collection_operation(call, set, Some(value.values[0]), None);
                self.store_receiver_back(receiver_place, &object)?;
                self.release_opaque_handle(result);
                Ok(unit_value(&mut *self.builder))
            }
            SetOp::Clear => {
                let result = self.emit_collection_operation(call, set, None, None);
                self.release_opaque_handle(result);
                self.store_receiver_back(receiver_place, &object)?;
                Ok(unit_value(&mut *self.builder))
            }
            SetOp::Reserve => {
                self.compile_collection_reserve(call, &object, receiver_place, args[0])
            }
            SetOp::IndexOption | SetOp::TakeIndexOption => {
                let take_place = match (op, receiver_place) {
                    (SetOp::IndexOption, _) => None,
                    (_, Some(place)) => Some(place),
                    (_, None) => {
                        return Err(
                            "direct backend expected internal consuming set indexing to provide its private receiver place"
                                .to_string(),
                        )
                    }
                };
                let index = self.load_word(args[0], ScalarKind::Int64, false)?;
                let results = self.runtime_call_results(function, &[set, index]);
                if let Some(place) = take_place {
                    self.store_place(place, object.clone())?;
                }
                Ok(self.owned_opaque_result(results, optional_type(element_ty)))
            }
        }
    }

    /// An `Array` method, or the private element read, write, or slice that
    /// indexing lowers to. A public method passes a zero source line and
    /// column after its words.
    fn compile_array_runtime_op(
        &mut self,
        op: ArrayOp,
        object: ValueRef,
        receiver_place: Option<&str>,
        mir_args: &[MirArg],
    ) -> std::result::Result<ValueRef, String> {
        let runtime_op = RuntimeOp::Array(op);
        let call = runtime_op.runtime_call();
        let function = self.runtime[call.function];
        let receiver_ty = direct_type_to_type(&object.ty);
        let object = self.ensure_opaque(object)?;
        let array = object.values[0];
        let element_ty = first_type_arg(Some(&receiver_ty))
            .ok_or("direct backend expected Array to carry one dtype".to_string())?;
        let element_direct_ty = ensure_direct_type(&element_ty, &self.classes, "Array dtype")?;
        let array_ty = Type::Named("Array".to_string(), vec![element_ty.clone()]);
        let args = mir_args.iter().collect::<Vec<_>>();
        let int32 = ScalarKind::Int32;
        let mut words = vec![array];
        match op {
            ArrayOp::Slice => self.compile_slice_runtime_op(runtime_op, object, mir_args),
            ArrayOp::Shape => {
                let results = self.runtime_call_results(function, &words);
                Ok(self.owned_opaque_result(
                    results,
                    Type::Named("list".to_string(), vec![Type::named("int64")]),
                ))
            }
            ArrayOp::Len => Ok(ValueRef {
                values: self.runtime_call_results(function, &words),
                ty: DirectType::Scalar(ScalarKind::Int64),
            }),
            ArrayOp::Clone => {
                self.push_runtime_call_words(call, &mut words);
                let results = self.runtime_call_results(function, &words);
                Ok(self.owned_opaque_result(results, receiver_ty))
            }
            ArrayOp::Get | ArrayOp::Set | ArrayOp::Fill => {
                for (position, argument) in args.iter().enumerate() {
                    // `get` and `set` take coordinates first; `set` and
                    // `fill` take an element value last.
                    let value = if matches!((op, position), (ArrayOp::Set, 1) | (ArrayOp::Fill, 0))
                    {
                        self.load_operand_as_opaque_direct(&argument.value, &element_direct_ty)?
                    } else {
                        let loaded = self.load_operand(&argument.value)?;
                        self.ensure_opaque(loaded)?
                    };
                    words.push(value.values[0]);
                }
                self.push_runtime_call_words(call, &mut words);
                let results = self.runtime_call_results(function, &words);
                if op == ArrayOp::Fill {
                    self.release_opaque_handle(results[0]);
                    self.store_receiver_back(receiver_place, &object)?;
                    return Ok(unit_value(&mut *self.builder));
                }
                if op == ArrayOp::Set {
                    self.store_receiver_back(receiver_place, &object)?;
                }
                Ok(self.owned_opaque_result(results, optional_type(element_ty)))
            }
            ArrayOp::Index => {
                let coordinates = self.load_operand(&args[0].value)?;
                let line = self.load_word(args[1], int32, true)?;
                let column = self.load_word(args[2], int32, true)?;
                if coordinates.ty == DirectType::Scalar(ScalarKind::Int64) {
                    if let DirectType::Scalar(kind) = element_direct_ty {
                        return self.emit_array_element_read(
                            array,
                            coordinates,
                            kind,
                            element_ty,
                            line,
                            column,
                        );
                    }
                }
                let coordinates = self.ensure_opaque(coordinates)?;
                let results = self
                    .runtime_call_results(function, &[array, coordinates.values[0], line, column]);
                let result = self.owned_opaque_result(results, element_ty);
                self.coerce_value(result, &element_direct_ty)
            }
            ArrayOp::SetIndex => {
                let coordinates = self.load_operand(&args[0].value)?;
                let coordinates = self.ensure_opaque(coordinates)?;
                let value =
                    self.load_operand_as_opaque_direct(&args[1].value, &element_direct_ty)?;
                let line = self.load_word(args[2], int32, true)?;
                let column = self.load_word(args[3], int32, true)?;
                let results = self.runtime_call_results(
                    function,
                    &[array, coordinates.values[0], value.values[0], line, column],
                );
                self.release_opaque_handle(results[0]);
                self.store_receiver_back(receiver_place, &object)?;
                Ok(unit_value(&mut *self.builder))
            }
            ArrayOp::Map => {
                let callback = self.load_operand(&args[0].value)?;
                let result_type = match direct_type_to_type(&callback.ty) {
                    Type::Function { return_type, .. } | Type::Closure { return_type, .. } => {
                        *return_type
                    }
                    other => {
                        return Err(format!(
                            "direct backend expected `Array.map` callback, found `{other}`"
                        ))
                    }
                };
                let result_dtype = self
                    .builder
                    .iconst(types::I64, direct_array_dtype_code(&result_type)?);
                let callback = self.ensure_opaque(callback)?;
                words.extend([callback.values[0], result_dtype]);
                self.push_runtime_call_words(call, &mut words);
                let results = self.runtime_call_results(function, &words);
                Ok(self.owned_opaque_result(
                    results,
                    Type::Named("Array".to_string(), vec![result_type]),
                ))
            }
            ArrayOp::Sum | ArrayOp::Min | ArrayOp::Max | ArrayOp::Mean => {
                self.push_runtime_call_words(call, &mut words);
                let results = self.runtime_call_results(function, &words);
                let target = if op == ArrayOp::Mean {
                    DirectType::Scalar(ScalarKind::Float64)
                } else {
                    element_direct_ty
                };
                let boxed = self.owned_opaque_result(results, direct_type_to_type(&target));
                self.coerce_value(boxed, &target)
            }
            ArrayOp::WrappingAdd
            | ArrayOp::WrappingSub
            | ArrayOp::WrappingMul
            | ArrayOp::SaturatingAdd
            | ArrayOp::SaturatingSub
            | ArrayOp::SaturatingMul => {
                let rhs = self.load_operand(&args[0].value)?;
                let rhs = if direct_array_element_type(&rhs.ty).is_some() {
                    rhs
                } else {
                    self.coerce_value(rhs, &element_direct_ty)?
                };
                let rhs = self.ensure_opaque(rhs)?;
                words.push(rhs.values[0]);
                self.push_runtime_call_words(call, &mut words);
                let results = self.runtime_call_results(function, &words);
                Ok(self.owned_opaque_result(results, array_ty))
            }
        }
    }

    /// A private `Span[T]` range operation on a list (ADR-0061, B2): one
    /// runtime entry point keyed by the op's opcode word, with the `int64`
    /// operands in order, padded with zero words, then the value word.
    fn compile_span_runtime_op(
        &mut self,
        op: SpanOp,
        object: ValueRef,
        receiver_place: Option<&str>,
        args: &[MirArg],
    ) -> std::result::Result<ValueRef, String> {
        let runtime_op = RuntimeOp::Span(op);
        let call = runtime_op.runtime_call();
        let list_ty = direct_type_to_type(&object.ty);
        let object = self.ensure_opaque(object)?;
        let element_ty = first_type_arg(Some(&list_ty)).unwrap_or(Type::named("Unknown"));
        let element_direct_ty = ensure_direct_type(&element_ty, &self.classes, "Vec element")?;
        let args = args.iter().collect::<Vec<_>>();
        let mut operands = Vec::with_capacity(args.len());
        let mut value_operand = None;
        for (position, argument) in args.iter().enumerate() {
            if op.value_position() != Some(position) {
                operands.push(self.load_word(argument, ScalarKind::Int64, true)?);
                continue;
            }
            value_operand = Some(match op {
                SpanOp::Eq => {
                    let loaded = self.load_operand(&argument.value)?;
                    self.ensure_opaque(loaded)?.values[0]
                }
                SpanOp::Restore => {
                    let loaded = self.load_operand(&argument.value)?;
                    let loaded = self.ensure_opaque(loaded)?;
                    self.transfer_owned_opaque_value(&loaded)
                }
                _ => {
                    self.load_operand_as_opaque_direct(&argument.value, &element_direct_ty)?
                        .values[0]
                }
            });
        }
        let mut words = vec![object.values[0]];
        self.push_runtime_call_words(call, &mut words);
        words.extend(operands);
        while words.len() < 10 {
            words.push(self.builder.iconst(types::I64, 0));
        }
        let value_operand = match value_operand {
            Some(value) => value,
            None => self.builder.iconst(types::I64, 0),
        };
        words.push(value_operand);
        let results = self.runtime_call_results(self.runtime[call.function], &words);
        if op.mutates_receiver() {
            self.store_receiver_back(receiver_place, &object)?;
        }
        match op {
            SpanOp::Extract => Ok(self
                .owned_opaque_result(results, Type::Named("list".to_string(), vec![element_ty]))),
            SpanOp::Restore | SpanOp::Fill => {
                self.release_opaque_handle(results[0]);
                Ok(unit_value(&mut *self.builder))
            }
            SpanOp::Contains | SpanOp::Eq => {
                let boxed = self.owned_opaque_result(results, Type::named("bool"));
                self.coerce_value(boxed, &DirectType::Scalar(ScalarKind::Bool))
            }
            SpanOp::Start | SpanOp::Stop | SpanOp::Index | SpanOp::Find | SpanOp::Count => {
                let boxed = self.owned_opaque_result(results, Type::named("int64"));
                self.coerce_value(boxed, &DirectType::Scalar(ScalarKind::Int64))
            }
        }
    }

    /// Tests the receiver's runtime type against each candidate in turn and
    /// runs the first match's method. Every branch joins on one result.
    fn compile_candidate_dispatch(
        &mut self,
        object: ValueRef,
        receiver_place: Option<&str>,
        args: &[MirArg],
        candidates: &[MethodCandidate],
    ) -> std::result::Result<ValueRef, String> {
        let result_ty = self.dynamic_result_type(candidates)?;
        let join_block = self.builder.create_block();
        let result_vars = self.declare_temporary_result_storage(&result_ty)?;
        self.emit_candidate_chain(
            object,
            receiver_place,
            candidates,
            args,
            &result_ty,
            &result_vars,
            join_block,
        )?;
        self.builder.switch_to_block(join_block);
        self.builder.seal_block(join_block);
        // Every successful candidate released its branch-local temporaries
        // before jumping here. The restored fallthrough ledger belongs only to
        // the trapping no-match edge and must not be released a second time at
        // the join.
        self.layout_scratch.clear();
        self.load_result_vars(&result_vars, result_ty)
    }

    /// The common result type of every runtime candidate, or an opaque
    /// unknown when the candidates disagree.
    fn dynamic_result_type(
        &self,
        candidates: &[MethodCandidate],
    ) -> std::result::Result<DirectType, String> {
        let [first, ..] = candidates else {
            return Err("direct backend received a dispatch without candidates".to_string());
        };
        let result_types = candidates
            .iter()
            .map(|candidate| self.call_result_type(&candidate.function))
            .collect::<std::result::Result<Vec<_>, _>>()?;
        if result_types.windows(2).all(|window| window[0] == window[1]) {
            self.call_result_type(&first.function)
        } else {
            Ok(DirectType::Opaque(Type::named("Unknown")))
        }
    }

    /// Dispatches an erased receiver on its runtime value: a union routes
    /// through its active payload (with the active-payload write-back
    /// projection for the receiver place), any other value dispatches
    /// directly. Both paths join on the same result storage.
    fn compile_borrowed_receiver_dispatch(
        &mut self,
        object: ValueRef,
        receiver_place: Option<&str>,
        args: &[MirArg],
        candidates: &[MethodCandidate],
    ) -> std::result::Result<ValueRef, String> {
        let raw = self.ensure_opaque(object)?;
        let result_ty = self.dynamic_result_type(candidates)?;
        let join_block = self.builder.create_block();
        let result_vars = self.declare_temporary_result_storage(&result_ty)?;
        let probe = self
            .builder
            .call(self.runtime[RuntimeFn::ValueIsUnion], &[raw.values[0]]);
        let is_union = probe.results[0];
        let union_block = self.builder.create_block();
        let plain_block = self.builder.create_block();
        self.builder
            .brif(is_union, union_block, &[], plain_block, &[]);
        let caller_owned = self.layout_scratch.clone();

        self.builder.switch_to_block(union_block);
        self.builder.seal_block(union_block);
        self.layout_scratch = caller_owned.clone();
        let receiver_kind = |candidate: &MethodCandidate| {
            self.method_receivers
                .get(&candidate.function)
                .copied()
                .flatten()
        };
        let mutates_receiver = candidates
            .iter()
            .any(|candidate| receiver_kind(candidate) == Some(MirReceiverKind::BorrowMut));
        let consumes = receiver_kind(&candidates[0]) == Some(MirReceiverKind::Value);
        if mutates_receiver {
            // No static write-back path exists for the payload of a union
            // behind an erased place; refuse at run time rather than mutate
            // a detached copy.
            self.emit_pending_cleanups(true)?;
            let line = self.builder.iconst(types::I64, 0);
            let column = self.builder.iconst(types::I64, 0);
            self.builder.call(
                self.runtime[RuntimeFn::FailErasedUnionMutableReceiver],
                &[line, column],
            );
            self.builder.trap(TrapCode::User(1));
        } else {
            let consume_flag = self.builder.iconst(types::I64, i64::from(consumes));
            let inst = self.builder.call(
                self.runtime[RuntimeFn::UnionActivePayload],
                &[raw.values[0], consume_flag],
            );
            let payload = self.owned_opaque_result(inst.results, Type::named("Unknown"));
            let union_receiver_place = receiver_place.map(|place| {
                format!(
                    "{place}.{}",
                    crate::native_runtime::DIRECT_UNION_ACTIVE_PAYLOAD_PROJECTION
                )
            });
            self.emit_candidate_chain(
                payload,
                union_receiver_place.as_deref(),
                candidates,
                args,
                &result_ty,
                &result_vars,
                join_block,
            )?;
        }

        self.builder.switch_to_block(plain_block);
        self.builder.seal_block(plain_block);
        self.layout_scratch = caller_owned;
        self.emit_candidate_chain(
            raw,
            receiver_place,
            candidates,
            args,
            &result_ty,
            &result_vars,
            join_block,
        )?;

        self.builder.switch_to_block(join_block);
        self.builder.seal_block(join_block);
        self.layout_scratch.clear();
        self.load_result_vars(&result_vars, result_ty)
    }

    /// Emits the runtime-type candidate chain: each candidate tests the
    /// receiver's runtime type, calls its method on a match, stores the
    /// coerced result, and jumps to `join_block`; the final fallthrough traps.
    #[allow(clippy::too_many_arguments)]
    fn emit_candidate_chain(
        &mut self,
        object: ValueRef,
        receiver_place: Option<&str>,
        candidates: &[MethodCandidate],
        args: &[MirArg],
        result_ty: &DirectType,
        result_vars: &[Var],
        join_block: Blk,
    ) -> std::result::Result<(), String> {
        let mut current_fallthrough = None;
        // Every runtime candidate starts from the same ownership state. Emitting
        // one candidate's taken edge can consume or release an owned projected
        // receiver, but that edge is skipped when control falls through to the
        // next candidate. Keep the compile-time ledger path-local so a later
        // candidate still emits the release owed by its runtime path.
        let caller_owned = self.layout_scratch.clone();
        for candidate in candidates {
            self.layout_scratch = caller_owned.clone();
            let matched = self.value_matches_runtime_type(object.values[0], &candidate.ty)?;
            let then_block = self.builder.create_block();
            let else_block = self.builder.create_block();
            self.builder.brif(matched, then_block, &[], else_block, &[]);
            self.builder.switch_to_block(then_block);
            self.layout_scratch = caller_owned.clone();
            let call_result = self.compile_method_call(
                &candidate.function,
                object.clone(),
                receiver_place,
                args,
            )?;
            let coerced_result = self.coerce_value(call_result, result_ty)?;
            self.store_result_vars(result_vars, &coerced_result)?;
            self.release_all_temporary_owned();
            self.builder.jump(join_block, &[]);
            self.builder.seal_block(then_block);
            self.builder.switch_to_block(else_block);
            self.layout_scratch = caller_owned.clone();
            current_fallthrough = Some(else_block);
        }
        if let Some(else_block) = current_fallthrough {
            self.builder.switch_to_block(else_block);
            self.emit_pending_cleanups(true)?;
            self.builder.trap(TrapCode::User(1));
            self.builder.seal_block(else_block);
        }
        Ok(())
    }

    fn compile_opaque_construct(
        &mut self,
        class_name: &str,
        fields: &[crate::mir::MirFieldInit],
        target_ty: &Type,
    ) -> std::result::Result<ValueRef, String> {
        let class = self.classes.get(class_name).cloned().ok_or(format!(
            "direct backend does not know class `{}`",
            class_name
        ))?;
        let (class_ptr, class_len) = self.string_constant(class_name.as_bytes())?;
        let init = self.builder.call(
            self.runtime[RuntimeFn::InstanceEmpty],
            &[class_ptr, class_len],
        );
        let current = self.owned_opaque_result(init.results, target_ty.clone());
        let substitutions = match target_ty {
            Type::Named(name, args) if name == class_name => class
                .type_params
                .iter()
                .cloned()
                .zip(args.iter().cloned())
                .collect::<HashMap<_, _>>(),
            _ => HashMap::new(),
        };
        for field in fields {
            let field_ty = class
                .fields
                .iter()
                .find(|candidate| candidate.name == field.name)
                .map(|candidate| substitute_type(&candidate.ty, &substitutions))
                .ok_or({
                    format!(
                        "direct backend construction for `{}` is missing field metadata for `{}`",
                        class_name, field.name
                    )
                })?;
            let field_ty = ensure_direct_type(
                &field_ty,
                &self.classes,
                &format!("field `{}` on class `{}`", field.name, class_name),
            )?;
            let loaded = self.load_operand_for_target(&field.value, &field_ty)?;
            let loaded = self.coerce_value(loaded, &field_ty)?;
            let loaded = self.ensure_opaque(loaded)?;
            let loaded = self.transfer_owned_opaque_value(&loaded);
            let (field_ptr, field_len) = self.string_constant(field.name.as_bytes())?;
            self.builder.call(
                self.runtime[RuntimeFn::InstanceSetFieldOwned],
                &[current.values[0], field_ptr, field_len, loaded],
            );
        }
        Ok(current)
    }

    fn compile_start_task(&mut self, task: TaskStart<'_>) -> std::result::Result<ValueRef, String> {
        let TaskStart {
            mode,
            stack_size,
            task_group,
            function,
            args,
            spawn_span,
            target,
        } = task;
        let function_value = self.load_operand(function)?;
        let function_value = self.ensure_opaque(function_value)?;
        let function_params =
            match infer_operand_type(function, &self.variable_types, &self.classes) {
                Some(DirectType::Callable(callable)) => {
                    callables::contract_parts(&callable.signature)?.0
                }
                _ => Vec::new(),
            };
        let arg_count_value = self
            .builder
            .iconst(types::I64, function_params.len() as i64);
        let buffer_call = self
            .builder
            .call(self.runtime[RuntimeFn::ArgBufferNew], &[arg_count_value]);
        let buffer = buffer_call.results[0];
        let guard_call = self.builder.call(
            self.runtime[RuntimeFn::TaskArgBufferGuard],
            &[buffer, arg_count_value],
        );
        let buffer_guard = guard_call.results[0];
        let expected = function_value_param_types(
            &function_params,
            &self.classes,
            "task function-value parameter",
        )?;
        let binding = bind_function_value_args(
            &function_params,
            args,
            "task function value has no parameter named",
            "duplicate task function-value argument",
        )?;
        for (arg, index) in args.iter().zip(&binding.source_slots) {
            let index = *index;
            if arg.writeback_place.is_some() {
                return Err(
                    "direct backend does not yet support borrowed task-start arguments".to_string(),
                );
            }
            let expected_ty = &expected[index];
            let loaded = self.load_operand_for_target(&arg.value, expected_ty)?;
            let value = self.coerce_value(loaded, expected_ty)?;
            let value = self.ensure_opaque(value)?;
            let index_value = self.builder.iconst(types::I64, index as i64);
            self.builder.call(
                self.runtime[RuntimeFn::ArgBufferStore],
                &[buffer, index_value, value.values[0]],
            );
        }
        let transfer_defaults = self.builder.iconst(types::I64, 1);
        self.builder.call(
            self.runtime[RuntimeFn::FunctionBindDefaults],
            &[
                function_value.values[0],
                buffer,
                arg_count_value,
                transfer_defaults,
            ],
        );
        for (index, supplied) in binding.slots.iter().enumerate() {
            if supplied.is_some() {
                continue;
            }
            let raw = self
                .builder
                .load(types::I64, MemFlags::new(), buffer, (index as i32) * 8);
            self.tag_raw_opaque_runtime_type(raw, &expected[index])?;
        }
        let returns_handle_value = self
            .builder
            .iconst(types::I64, if mode.returns_handle { 1 } else { 0 });
        let result_is_copy_value = self
            .builder
            .iconst(types::I64, if mode.result_is_copy { 1 } else { 0 });
        let (stack_size_present_value, stack_size_value) = match stack_size {
            Some(stack_size) => {
                let target = DirectType::Scalar(ScalarKind::Int64);
                let value = self.load_operand_for_target(stack_size, &target)?;
                (
                    self.builder.iconst(types::I64, 1),
                    self.coerce_value(value, &target)?.values[0],
                )
            }
            None => (
                self.builder.iconst(types::I64, 0),
                self.builder.iconst(types::I64, 0),
            ),
        };
        let group = self.load_operand(task_group)?;
        let group = self.ensure_opaque(group)?;
        let task_group_value = group.values[0];
        let current_function_name = self.current_function_name.clone();
        let current_function_path = self.current_function_path.clone();
        let (parent_function_ptr, parent_function_len) =
            self.string_constant(current_function_name.as_bytes())?;
        let (spawn_path_ptr, spawn_path_len) =
            self.string_constant(current_function_path.as_bytes())?;
        let spawn_line = self.builder.iconst(types::I64, spawn_span.line as i64);
        let spawn_column = self.builder.iconst(types::I64, spawn_span.column as i64);
        // Every operation that can trap after raw allocation has completed.
        // Transfer the buffer from the direct cleanup stack to the task runtime
        // only across this allocation-free call boundary.
        self.builder.call(
            self.runtime[RuntimeFn::TaskArgBufferDisarm],
            &[buffer_guard],
        );
        let call = self.builder.call(
            self.runtime[RuntimeFn::StartTaskCall],
            &[
                function_value.values[0],
                buffer,
                arg_count_value,
                returns_handle_value,
                task_group_value,
                result_is_copy_value,
                stack_size_present_value,
                stack_size_value,
                parent_function_ptr,
                parent_function_len,
                spawn_path_ptr,
                spawn_path_len,
                spawn_line,
                spawn_column,
            ],
        );
        // MIR checking/lowering has already assigned every handle-returning
        // start to its concrete `Task[T]` target. Carry that exact target
        // through codegen instead of independently reconstructing or
        // defensively revalidating the result type here.
        let ty = if mode.returns_handle {
            target.clone()
        } else {
            DirectType::Scalar(ScalarKind::Unit)
        };
        match ty {
            DirectType::Opaque(ty) => Ok(self.owned_opaque_result(call.results, ty)),
            _ => {
                self.release_opaque_handle(call.results[0]);
                Ok(unit_value(&mut *self.builder))
            }
        }
    }

    fn value_matches_type(
        &mut self,
        value: Val,
        type_name: &str,
    ) -> std::result::Result<Val, String> {
        let (ptr, len) = self.string_constant(type_name.as_bytes())?;
        let inst = self.builder.call(
            self.runtime[RuntimeFn::ValueTypeMatches],
            &[value, ptr, len],
        );
        Ok(inst.results[0])
    }

    fn value_matches_runtime_type(
        &mut self,
        value: Val,
        ty: &Type,
    ) -> std::result::Result<Val, String> {
        match ty {
            Type::Union(_) => {
                let pattern = crate::native_runtime::canonical_runtime_type_name(ty);
                self.value_matches_type(value, &pattern)
            }
            Type::TypeParam(_) => Ok(self.builder.iconst(types::I64, 1)),
            Type::ReturnedView(_) => Err(
                "direct backend cannot test a value against a returned-view contract".to_string(),
            ),
            Type::Unit => self.value_matches_type(value, "None"),
            Type::Module(path) => self.value_matches_type(value, &format!("module {}", path)),
            Type::Tuple(_) => {
                let pattern = crate::native_runtime::canonical_runtime_type_name(ty);
                self.value_matches_type(value, &pattern)
            }
            Type::Function { .. } | Type::Closure { .. } | Type::Callable(_) => {
                let pattern = crate::native_runtime::canonical_runtime_type_name(ty);
                self.value_matches_type(value, &pattern)
            }
            Type::Named(name, args) => {
                if args.is_empty() {
                    return self.value_matches_type(value, name);
                }

                let pattern = crate::native_runtime::canonical_runtime_type_name(ty);
                let exact = self.value_matches_type(value, &pattern)?;
                let base_matches = self.value_matches_type(value, name)?;

                let Some(class) = self.classes.get(name).cloned() else {
                    return Ok(exact);
                };
                let inspect_block = self.builder.create_block();
                let join_block = self.builder.create_block();
                self.builder.append_block_param(join_block, types::I64);
                let structural_block = self.builder.create_block();
                let tagged = self
                    .builder
                    .call(self.runtime[RuntimeFn::ValueHasRuntimeType], &[value]);
                let tagged = tagged.results[0];
                self.builder
                    .brif(tagged, join_block, &[exact], structural_block, &[]);
                self.builder.switch_to_block(structural_block);
                self.builder
                    .brif(base_matches, inspect_block, &[], join_block, &[exact]);
                self.builder.seal_block(structural_block);
                self.builder.switch_to_block(inspect_block);

                let mut structural = base_matches;
                let substitutions = class
                    .type_params
                    .iter()
                    .cloned()
                    .zip(args.iter().cloned())
                    .collect::<HashMap<_, _>>();
                for field in &class.fields {
                    let field_ty = substitute_type(&field.ty, &substitutions);
                    if runtime_type_is_wildcard(&field_ty) {
                        continue;
                    }
                    let (field_ptr, field_len) = self.string_constant(field.name.as_bytes())?;
                    let inst = self.builder.call(
                        self.runtime[RuntimeFn::InstanceGetField],
                        &[value, field_ptr, field_len],
                    );
                    let field_value = inst.results[0];
                    let field_matches = self.value_matches_runtime_type(field_value, &field_ty)?;
                    self.release_opaque_handle(field_value);
                    structural = self.builder.band(structural, field_matches);
                }
                let matched = self.builder.bor(exact, structural);
                self.builder.jump(join_block, &[matched]);
                self.builder.seal_block(inspect_block);
                self.builder.switch_to_block(join_block);
                self.builder.seal_block(join_block);
                Ok(self.builder.block_params(join_block)[0])
            }
        }
    }

    fn span_values(&mut self, span: Option<Span>) -> (Val, Val) {
        let (line, column) = span
            .map(|span| (span.line as i64, span.column as i64))
            .unwrap_or((0, 0));
        (
            self.builder.iconst(types::I64, line),
            self.builder.iconst(types::I64, column),
        )
    }

    fn declare_temporary_result_storage(
        &mut self,
        ty: &DirectType,
    ) -> std::result::Result<Vec<Var>, String> {
        let mut vars = Vec::new();
        for abi in ty.abi_types() {
            let variable = Var(self.next_variable_index as u32);
            self.next_variable_index += 1;
            self.builder.declare_var(variable, abi);
            let zero = match abi {
                t if t == types::F64 => self.builder.f64const(0.0),
                _ => self.builder.iconst(abi, 0),
            };
            self.builder.def_var(variable, zero);
            vars.push(variable);
        }
        Ok(vars)
    }

    fn store_result_vars(
        &mut self,
        vars: &[Var],
        value: &ValueRef,
    ) -> std::result::Result<(), String> {
        if matches!(value.ty, DirectType::Opaque(_)) {
            let stored = if self.temporary_owns_opaque(value) {
                self.clear_temporary_opaque_owned(value);
                value.values[0]
            } else {
                self.retain_opaque_handle(value.values[0])
            };
            let Some(var) = vars.first() else {
                return Err("direct backend expected opaque temporary result storage".to_string());
            };
            self.builder.def_var(*var, stored);
            return Ok(());
        }

        for (var, compiled) in vars.iter().zip(value.values.iter()) {
            self.builder.def_var(*var, *compiled);
        }
        Ok(())
    }

    fn load_result_vars(
        &mut self,
        vars: &[Var],
        ty: DirectType,
    ) -> std::result::Result<ValueRef, String> {
        let values = vars.iter().map(|var| self.builder.use_var(*var)).collect();
        let value = ValueRef { values, ty };
        if matches!(value.ty, DirectType::Opaque(_)) {
            self.mark_temporary_opaque_owned(&value);
        }
        Ok(value)
    }
}

fn unit_value(builder: &mut dyn FunctionBackend) -> ValueRef {
    ValueRef {
        values: vec![builder.iconst(types::I64, 0)],
        ty: DirectType::Scalar(ScalarKind::Unit),
    }
}

/// The addresses of the consuming `Place` operands among `roles`.
fn consuming_operands(
    roles: Vec<(&Operand, crate::mir::ownership::OperandRole)>,
) -> Vec<*const Operand> {
    roles
        .into_iter()
        .filter(|(operand, role)| {
            *role == crate::mir::ownership::OperandRole::Consume
                && matches!(operand, Operand::Place(_))
        })
        .map(|(operand, _)| operand as *const Operand)
        .collect()
}

fn declare_runtime_helper(
    module: &mut dyn ModuleBackend,
    function: RuntimeFn,
) -> std::result::Result<FuncHandle, String> {
    let params = function
        .params()
        .iter()
        .map(|param| abi_type(*param))
        .collect::<Vec<_>>();
    let handle = declare_runtime_function(
        module,
        function.symbol(),
        &params,
        function.result().map(abi_type),
    )?;
    if function.is_task_invariant() {
        module.mark_task_invariant(handle);
    }
    Ok(handle)
}

/// Pushes a call's frame record the way the runtime's frame stack allows:
/// store the descriptor's address after the active entries and count it,
/// unless the stack is full or its flags ask for the runtime. Then
/// `aura_direct_enter_frame` does the whole push. The main wrapper has
/// validated every descriptor, so the inline path stores only validated
/// ones. Returns the block both paths continue in.
fn emit_frame_push(
    builder: &mut dyn FunctionBackend,
    runtime: &RuntimeTable<FuncRef>,
    descriptor: Val,
) -> Blk {
    let layout = frame_stack_layout();
    let trusted = MemFlags::trusted();
    let stack = builder.call(runtime[RuntimeFn::FrameStack], &[]).results[0];
    let len = builder.load(types::I64, trusted, stack, layout.len_offset);
    let limit = builder.load(types::I64, trusted, stack, layout.limit_offset);
    let flags = builder.load(types::I64, trusted, stack, layout.flags_offset);
    let runtime_work = builder.icmp_imm(IntCC::NotEqual, flags, 0);
    let full = builder.icmp(IntCC::UnsignedGreaterThanOrEqual, len, limit);
    let slow = builder.bor(full, runtime_work);
    let (fast_block, slow_block, done) = emit_frame_branch(builder, slow);

    builder.switch_to_block(fast_block);
    let entries = builder.load(types::I64, trusted, stack, layout.entries_offset);
    let offset = builder.imul_imm(len, FRAME_STACK_ENTRY_BYTES);
    let slot = builder.iadd(entries, offset);
    builder.store(trusted, descriptor, slot, 0);
    let pushed = builder.iadd_imm(len, 1);
    builder.store(trusted, pushed, stack, layout.len_offset);
    builder.jump(done, &[]);

    builder.switch_to_block(slow_block);
    builder.call(runtime[RuntimeFn::EnterCall], &[descriptor]);
    builder.jump(done, &[]);

    builder.seal_block(done);
    builder.switch_to_block(done);
    done
}

fn ownership_fast_path_signature(returns_value: bool) -> Signature {
    let mut signature = Signature::new(CallConv::C);
    signature.params.push(types::I64);
    if returns_value {
        signature.returns.push(types::I64);
    }
    signature
}

/// Declares the module's retain and release, and makes `runtime` name them
/// instead of the runtime's exports, which they call for the cold cases.
/// LLVM inlines both into every caller.
fn declare_ownership_fast_paths(
    module: &mut dyn ModuleBackend,
    runtime: &mut RuntimeTable<FuncHandle>,
) -> std::result::Result<OwnershipFastPaths, String> {
    let retain = try_or_string_error!(
        module.declare_function(
            "aura_owned_retain",
            Linkage::Local,
            &ownership_fast_path_signature(true),
        ),
        "failed to declare the retain fast path: {}"
    );
    let release = try_or_string_error!(
        module.declare_function(
            "aura_owned_release",
            Linkage::Local,
            &ownership_fast_path_signature(false),
        ),
        "failed to declare the release fast path: {}"
    );
    module.mark_inline(retain);
    module.mark_inline(release);
    Ok(OwnershipFastPaths {
        retain,
        release,
        runtime_retain: runtime.replace(RuntimeFn::RetainValue, retain),
        runtime_release: runtime.replace(RuntimeFn::ReleaseValue, release),
        release_last: runtime[RuntimeFn::ReleaseLastOwnedValue],
    })
}

/// What both fast paths share: the value, the cold block that calls the
/// runtime, and the block that returns.
struct OwnershipBlocks {
    value: Val,
    slow: Blk,
    done: Blk,
}

/// Emits the checks both fast paths make before they touch the counts: the
/// value is not null, is confined to this thread, and is linked into the
/// ledger of the running task. Leaves the builder in the block that may
/// touch the counts.
fn emit_ownership_checks(
    builder: &mut dyn FunctionBackend,
    frame_stack: FuncHandle,
) -> OwnershipBlocks {
    let layout = ownership_layout();
    let trusted = MemFlags::trusted();
    let entry = builder.create_block();
    builder.append_block_params_for_function_params(entry);
    builder.switch_to_block(entry);
    builder.seal_block(entry);
    let value = builder.block_params(entry)[0];
    let check = builder.create_block();
    let linked = builder.create_block();
    let counts = builder.create_block();
    let slow = builder.create_block();
    let done = builder.create_block();
    builder.set_cold_block(slow);
    let null = builder.icmp_imm(IntCC::Equal, value, 0);
    builder.brif(null, done, &[], check, &[]);
    builder.seal_block(check);

    // Another thread may write the header of a shared value, so the owner
    // is read only once the lock word shows the value is confined.
    builder.switch_to_block(check);
    let stack_ref = builder.import_function(frame_stack);
    let stack = builder.call(stack_ref, &[]).results[0];
    let lock = builder.load(types::I64, trusted, value, layout.shared_lock_offset);
    let shared = builder.icmp_imm(IntCC::NotEqual, lock, 0);
    builder.brif(shared, slow, &[], linked, &[]);
    builder.seal_block(linked);

    builder.switch_to_block(linked);
    let owner = builder.load(types::I64, trusted, value, layout.owner_offset);
    let elsewhere = builder.icmp(IntCC::NotEqual, owner, stack);
    builder.brif(elsewhere, slow, &[], counts, &[]);
    builder.seal_block(counts);
    builder.switch_to_block(counts);
    OwnershipBlocks { value, slow, done }
}

/// Retains a value: adds one to its reference count and to its ledger's,
/// unless the value is shared, belongs to another task's ledger, or its
/// count is full. Then `aura_direct_retain_value` does it.
fn emit_retain_fast_path(
    builder: &mut dyn FunctionBackend,
    frame_stack: FuncHandle,
    runtime_retain: FuncHandle,
) {
    let layout = ownership_layout();
    let trusted = MemFlags::trusted();
    let blocks = emit_ownership_checks(builder, frame_stack);
    let ref_count = builder.load(types::I32, trusted, blocks.value, layout.ref_count_offset);
    let full = builder.icmp_imm(IntCC::Equal, ref_count, -1);
    let fast = builder.create_block();
    builder.brif(full, blocks.slow, &[], fast, &[]);
    builder.seal_block(fast);
    builder.seal_block(blocks.slow);

    builder.switch_to_block(fast);
    let retained = builder.iadd_imm(ref_count, 1);
    builder.store(trusted, retained, blocks.value, layout.ref_count_offset);
    let owned = builder.load(types::I32, trusted, blocks.value, layout.owned_offset);
    let owned = builder.iadd_imm(owned, 1);
    builder.store(trusted, owned, blocks.value, layout.owned_offset);
    builder.jump(blocks.done, &[]);

    builder.switch_to_block(blocks.slow);
    let slow_ref = builder.import_function(runtime_retain);
    builder.call(slow_ref, &[blocks.value]);
    builder.jump(blocks.done, &[]);

    builder.seal_block(blocks.done);
    builder.switch_to_block(blocks.done);
    builder.return_(&[blocks.value]);
}

/// Releases a value: subtracts one from its reference count and from its
/// ledger's, while both stay above zero. When the running task's ledger
/// holds the last reference it links, `aura_direct_release_last_owned_value`
/// unlinks the value and frees it at zero without a lookup. A shared value,
/// or one another task's ledger links, goes to `aura_direct_release_value`.
fn emit_release_fast_path(
    builder: &mut dyn FunctionBackend,
    frame_stack: FuncHandle,
    paths: OwnershipFastPaths,
) {
    let layout = ownership_layout();
    let trusted = MemFlags::trusted();
    let blocks = emit_ownership_checks(builder, frame_stack);
    let owned = builder.load(types::I32, trusted, blocks.value, layout.owned_offset);
    let ref_count = builder.load(types::I32, trusted, blocks.value, layout.ref_count_offset);
    let last_owned = builder.icmp_imm(IntCC::UnsignedLessThanOrEqual, owned, 1);
    let last_reference = builder.icmp_imm(IntCC::UnsignedLessThanOrEqual, ref_count, 1);
    let last = builder.bor(last_owned, last_reference);
    let fast = builder.create_block();
    // Most releases drop the last reference, so this call is not cold.
    let last_block = builder.create_block();
    builder.brif(last, last_block, &[], fast, &[]);
    builder.seal_block(fast);
    builder.seal_block(last_block);
    builder.seal_block(blocks.slow);

    builder.switch_to_block(fast);
    let released = builder.iadd_imm(ref_count, -1);
    builder.store(trusted, released, blocks.value, layout.ref_count_offset);
    let owned = builder.iadd_imm(owned, -1);
    builder.store(trusted, owned, blocks.value, layout.owned_offset);
    builder.jump(blocks.done, &[]);

    builder.switch_to_block(last_block);
    let last_ref = builder.import_function(paths.release_last);
    builder.call(last_ref, &[blocks.value]);
    builder.jump(blocks.done, &[]);

    builder.switch_to_block(blocks.slow);
    let slow_ref = builder.import_function(paths.runtime_release);
    builder.call(slow_ref, &[blocks.value]);
    builder.jump(blocks.done, &[]);

    builder.seal_block(blocks.done);
    builder.switch_to_block(blocks.done);
    builder.return_(&[]);
}

/// Pops the innermost call's frame record: count one entry fewer, unless
/// the stack's flags ask for the runtime or no call is above its floor.
/// Then `aura_direct_exit_call` does the whole pop.
fn emit_frame_pop(builder: &mut dyn FunctionBackend, runtime: &RuntimeTable<FuncRef>) {
    let layout = frame_stack_layout();
    let trusted = MemFlags::trusted();
    // Calls since the push may have yielded, but the task's stack keeps its
    // address, so the code generator may reuse the push's address here.
    let stack = builder.call(runtime[RuntimeFn::FrameStack], &[]).results[0];
    let len = builder.load(types::I64, trusted, stack, layout.len_offset);
    let floor = builder.load(types::I64, trusted, stack, layout.floor_offset);
    let flags = builder.load(types::I64, trusted, stack, layout.flags_offset);
    let runtime_work = builder.icmp_imm(IntCC::NotEqual, flags, 0);
    let bottom = builder.icmp(IntCC::UnsignedLessThanOrEqual, len, floor);
    let slow = builder.bor(bottom, runtime_work);
    let (fast_block, slow_block, done) = emit_frame_branch(builder, slow);

    builder.switch_to_block(fast_block);
    let popped = builder.iadd_imm(len, -1);
    builder.store(trusted, popped, stack, layout.len_offset);
    builder.jump(done, &[]);

    builder.switch_to_block(slow_block);
    builder.call(runtime[RuntimeFn::ExitCall], &[]);
    builder.jump(done, &[]);

    builder.seal_block(done);
    builder.switch_to_block(done);
}

/// Branches on `slow` to a cold runtime block or the inline block, and
/// returns both with the block they rejoin in.
fn emit_frame_branch(builder: &mut dyn FunctionBackend, slow: Val) -> (Blk, Blk, Blk) {
    let fast_block = builder.create_block();
    let slow_block = builder.create_block();
    let done = builder.create_block();
    builder.set_cold_block(slow_block);
    builder.brif(slow, slow_block, &[], fast_block, &[]);
    builder.seal_block(fast_block);
    builder.seal_block(slow_block);
    (fast_block, slow_block, done)
}

fn declare_runtime_function(
    module: &mut dyn ModuleBackend,
    name: &str,
    params: &[ScalarType],
    result: Option<ScalarType>,
) -> std::result::Result<FuncHandle, String> {
    let mut sig = Signature::new(CallConv::C);
    sig.params.extend_from_slice(params);
    if let Some(result) = result {
        sig.returns.push(result);
    }
    match module.declare_function(name, Linkage::Import, &sig) {
        Ok(id) => Ok(id),
        Err(error) => Err(format!(
            "failed to declare runtime function `{}`: {}",
            name, error
        )),
    }
}

fn abi_type(ty: AbiType) -> ScalarType {
    match ty {
        AbiType::I32 => types::I32,
        AbiType::I64 => types::I64,
        AbiType::F64 => types::F64,
    }
}

/// About how many bytes `parts` add to an f-string: each literal's length,
/// the widest `int64`, and a short guess for any other value.
fn format_reserve_hint(parts: &[MirFormatPart]) -> usize {
    parts
        .iter()
        .map(|part| match part {
            MirFormatPart::Literal(text) => text.len(),
            MirFormatPart::Int64(_) => 20,
            MirFormatPart::Value(_) | MirFormatPart::Formatted { .. } => 16,
        })
        .sum()
}

fn declare_string_constant(
    string_data: &mut HashMap<Vec<u8>, DataHandle>,
    builder: &mut dyn FunctionBackend,
    bytes: &[u8],
) -> std::result::Result<(Val, Val), String> {
    let id = string_data_id(builder.module(), string_data, bytes)?;
    let global = builder.import_data(id);
    let ptr = builder.symbol_value(types::I64, global);
    let len = builder.iconst(types::I64, bytes.len() as i64);
    Ok((ptr, len))
}

fn string_data_id(
    object: &mut dyn ModuleBackend,
    string_data: &mut HashMap<Vec<u8>, DataHandle>,
    bytes: &[u8],
) -> std::result::Result<DataHandle, String> {
    if let Some(id) = string_data.get(bytes) {
        return Ok(*id);
    }
    let name = format!("aura_data_{}", string_data.len());
    let id = try_or_string_error!(
        object.declare_data(&name, Linkage::Local, false),
        "failed to declare string data: {}"
    );
    try_or_string_error!(
        object.define_data(id, DataDescription::new(bytes.to_vec())),
        "failed to define string data: {}"
    );
    string_data.insert(bytes.to_vec(), id);
    Ok(id)
}

// Field offsets of the runtime's `DirectFrameDescriptor`: a validation flag,
// the line and column, then the path and function name as pointer and length.
const FRAME_DESCRIPTOR_LINE: usize = 8;
const FRAME_DESCRIPTOR_COLUMN: usize = 16;
const FRAME_DESCRIPTOR_PATH_PTR: u32 = 24;
const FRAME_DESCRIPTOR_PATH_LEN: usize = 32;
const FRAME_DESCRIPTOR_FUNCTION_PTR: u32 = 40;
const FRAME_DESCRIPTOR_FUNCTION_LEN: usize = 48;
const FRAME_DESCRIPTOR_BYTES: usize = 56;

/// Defines one function's static frame descriptor. It is writable because the
/// runtime marks it validated when the main wrapper starts.
fn declare_frame_descriptor(
    object: &mut dyn ModuleBackend,
    string_data: &mut HashMap<Vec<u8>, DataHandle>,
    name: &str,
    span: Span,
    path: &[u8],
    function: &[u8],
) -> std::result::Result<DataHandle, String> {
    let path_id = string_data_id(object, string_data, path)?;
    let function_id = string_data_id(object, string_data, function)?;
    let id = try_or_string_error!(
        object.declare_data(name, Linkage::Local, true),
        "failed to declare frame descriptor: {}"
    );
    let mut bytes = vec![0u8; FRAME_DESCRIPTOR_BYTES];
    for (offset, value) in [
        (FRAME_DESCRIPTOR_LINE, span.line as u64),
        (FRAME_DESCRIPTOR_COLUMN, span.column as u64),
        (FRAME_DESCRIPTOR_PATH_LEN, path.len() as u64),
        (FRAME_DESCRIPTOR_FUNCTION_LEN, function.len() as u64),
    ] {
        bytes[offset..offset + 8].copy_from_slice(&value.to_le_bytes());
    }
    let mut data = DataDescription::new(bytes);
    data.align = Some(8);
    data.write_data_addr(FRAME_DESCRIPTOR_PATH_PTR, path_id);
    data.write_data_addr(FRAME_DESCRIPTOR_FUNCTION_PTR, function_id);
    try_or_string_error!(
        object.define_data(id, data),
        "failed to define frame descriptor: {}"
    );
    Ok(id)
}

/// Defines the table of every frame descriptor's address.
fn declare_frame_descriptor_table(
    object: &mut dyn ModuleBackend,
    descriptors: &[DataHandle],
) -> std::result::Result<DataHandle, String> {
    let id = try_or_string_error!(
        object.declare_data("aura_frame_descriptors", Linkage::Local, false),
        "failed to declare the frame descriptor table: {}"
    );
    let mut data = DataDescription::new(vec![0u8; 8 * descriptors.len().max(1)]);
    data.align = Some(8);
    for (index, descriptor) in descriptors.iter().enumerate() {
        data.write_data_addr(8 * index as u32, *descriptor);
    }
    try_or_string_error!(
        object.define_data(id, data),
        "failed to define the frame descriptor table: {}"
    );
    Ok(id)
}

fn signature_for(
    function: &MirFunction,
    classes: &HashMap<String, MirClass>,
) -> std::result::Result<Signature, String> {
    let mut signature = Signature::new(CallConv::Aura);
    let mut writeback_types = Vec::new();
    if function.receiver.is_some() {
        let receiver_ty = receiver_type(function, classes)?;
        for abi in receiver_ty.abi_types() {
            signature.params.push(abi);
        }
        if function.receiver == Some(MirReceiverKind::BorrowMut) {
            writeback_types.push(receiver_ty);
        }
    }
    for param in &function.params {
        let ty = ensure_direct_type(
            &param.ty,
            classes,
            &format!("parameter `{}` on `{}`", param.name, function.name),
        )?;
        for abi in ty.abi_types() {
            signature.params.push(abi);
        }
        if param.passing == MirReceiverKind::BorrowMut {
            writeback_types.push(ty);
        }
    }
    let return_ty = ensure_direct_type(
        &function.return_type,
        classes,
        &format!("return type of `{}`", function.name),
    )?;
    for abi in return_ty.abi_types() {
        signature.returns.push(abi);
    }
    for ty in writeback_types {
        for abi in ty.abi_types() {
            signature.returns.push(abi);
        }
    }
    Ok(signature)
}

fn receiver_type(
    function: &MirFunction,
    classes: &HashMap<String, MirClass>,
) -> std::result::Result<DirectType, String> {
    let mut receiver_ty = None;
    for local in &function.local_types {
        if local.name == "self" {
            receiver_ty = Some(&local.ty);
            break;
        }
    }
    let Some(receiver_ty) = receiver_ty else {
        return Err(format!(
            "direct backend could not find receiver local type for `{}`",
            function.name
        ));
    };
    ensure_direct_type(
        receiver_ty,
        classes,
        &format!("receiver of `{}`", function.name),
    )
}

/// The direct type of a cleanup place, read from the MIR types of its root
/// and the fields it projects.
fn cleanup_place_type(
    function: &MirFunction,
    classes: &HashMap<String, MirClass>,
    place: &str,
) -> std::result::Result<DirectType, String> {
    let mut root_types = HashMap::new();
    if function.receiver.is_some() {
        if let Some(local) = function
            .local_types
            .iter()
            .find(|local| local.name == "self")
        {
            root_types.insert(
                "self".to_string(),
                ensure_direct_type(&local.ty, classes, "cleanup receiver")?,
            );
        }
    }
    for param in &function.params {
        root_types.insert(
            param.name.clone(),
            ensure_direct_type(
                &param.ty,
                classes,
                &format!("cleanup parameter `{}`", param.name),
            )?,
        );
    }
    for local in &function.local_types {
        root_types
            .entry(local.name.clone())
            .or_insert(ensure_direct_type(
                &local.ty,
                classes,
                &format!("cleanup local `{}`", local.name),
            )?);
    }
    let mut segments = place.split('.');
    let root = segments
        .next()
        .ok_or("direct backend encountered an empty cleanup place".to_string())?;
    let mut ty = root_types.get(root).cloned().ok_or({
        format!(
            "direct backend does not know cleanup place `{}` in `{}`",
            place, function.name
        )
    })?;
    for field in segments {
        ty = direct_field_type(&ty, field, classes).ok_or({
            format!(
                "direct backend does not know cleanup field `{}` on `{}`",
                field,
                render_direct_type(&ty)
            )
        })?;
    }
    Ok(ty)
}

fn declare_root_variables(
    builder: &mut dyn FunctionBackend,
    variable_index: &mut usize,
    variables: &mut HashMap<String, Vec<Var>>,
    variable_types: &mut HashMap<String, DirectType>,
    name: String,
    ty: DirectType,
    initial: Option<&[Val]>,
) {
    let initial_values = if let Some(values) = initial {
        values.to_vec()
    } else {
        ty.zero_values(builder)
    };
    let abi_types = ty.abi_types();
    let mut declared = Vec::new();
    for (offset, abi_ty) in abi_types.into_iter().enumerate() {
        let variable = Var(*variable_index as u32);
        *variable_index += 1;
        builder.declare_var(variable, abi_ty);
        builder.def_var(variable, initial_values[offset]);
        declared.push(variable);
    }
    variables.insert(name.clone(), declared);
    variable_types.insert(name, ty);
}

/// The action registered for `place` in `function`. Every push of one place
/// in a function carries the same action, because lowering derives it from
/// the place's type.
fn cleanup_action_for_place(function: &MirFunction, place: &str) -> Option<CleanupAction> {
    function.blocks.iter().find_map(|block| {
        block
            .instructions
            .iter()
            .find_map(|instruction| match instruction {
                Instruction::PushCleanup {
                    place: pushed,
                    action,
                    ..
                } if pushed == place => Some(action.clone()),
                _ => None,
            })
    })
}

/// The callee MIR names for the cleanup of `place` in `function`. Every
/// push of one place carries the same callee.
fn cleanup_callee_for_place(function: &MirFunction, place: &str) -> Option<CleanupCallee> {
    function.blocks.iter().find_map(|block| {
        block
            .instructions
            .iter()
            .find_map(|instruction| match instruction {
                Instruction::PushCleanup {
                    place: pushed,
                    callee,
                    ..
                } if pushed == place => callee.clone(),
                _ => None,
            })
    })
}

/// Releases a cleanup thunk's call results. A `Result`-returning manager
/// exit first hands its result to the runtime, which records a `Result.Err`
/// for the trap being unwound (checkpoint F5).
fn release_cleanup_thunk_results(
    codegen: &mut NativeCodegen<'_>,
    builder: &mut dyn FunctionBackend,
    action: Option<&CleanupAction>,
    class_name: &str,
    function_name: &str,
    results: &[Val],
) -> std::result::Result<(), String> {
    let returns_result = matches!(
        action,
        Some(CleanupAction::ManagerExit {
            exit_result: crate::mir::ManagerExitResult::Result
        })
    );
    if returns_result {
        let return_ty = codegen
            .function_return_types
            .get(function_name)
            .cloned()
            .ok_or_else(|| {
                format!(
                    "direct backend does not know return type for `{}`",
                    function_name
                )
            })?;
        let return_count = return_ty.value_count().min(results.len());
        let boxed = box_thunk_value(codegen, builder, &results[..return_count], &return_ty)?;
        let (class_ptr, class_len) =
            thunk_string_constant(codegen, builder, class_name.as_bytes())?;
        let record = builder.import_function(codegen.runtime[RuntimeFn::RecordManagerExitFailure]);
        builder.call(record, &[boxed, class_ptr, class_len]);
        let release = builder.import_function(codegen.runtime[RuntimeFn::ReleaseValue]);
        builder.call(release, &[boxed]);
        let mut cursor = return_count;
        for writeback_ty in codegen
            .function_writeback_types
            .get(function_name)
            .cloned()
            .unwrap_or_default()
        {
            let count = writeback_ty.value_count();
            let end = (cursor + count).min(results.len());
            release_direct_values(codegen, builder, &results[cursor..end], &writeback_ty)?;
            cursor = end;
        }
        return Ok(());
    }
    release_direct_call_results(codegen, builder, function_name, results)
}

fn collect_cleanup_places(function: &MirFunction, reachable: &HashSet<String>) -> Vec<String> {
    let mut cleanup_places = Vec::<String>::new();
    for block in &function.blocks {
        if !reachable.contains(&block.label) {
            continue;
        }
        for instruction in &block.instructions {
            let Instruction::PushCleanup { place, .. } = instruction else {
                continue;
            };
            if !cleanup_places.contains(place) {
                cleanup_places.push(place.clone());
            }
        }
    }
    cleanup_places
}

fn direct_place_paths_overlap(left: &str, right: &str) -> bool {
    let left_segments = left.split('.').collect::<Vec<_>>();
    let right_segments = right.split('.').collect::<Vec<_>>();
    if left_segments.first() != right_segments.first() {
        return false;
    }
    let shared = left_segments
        .iter()
        .zip(right_segments.iter())
        .take_while(|(lhs, rhs)| lhs == rhs)
        .count();
    shared == left_segments.len() || shared == right_segments.len()
}

fn validate_module(
    module: &MirModule,
) -> std::result::Result<HashMap<String, HashSet<String>>, String> {
    let mut classes = HashMap::new();
    for class in &module.classes {
        classes.insert(class.name.clone(), class.clone());
    }
    for class in &module.classes {
        for field in &class.fields {
            ensure_direct_type(
                &field.ty,
                &classes,
                &format!("field `{}.{}`", class.name, field.name),
            )?;
        }
    }
    let mut reachable_by_function = HashMap::new();
    for function in module.functions.iter().chain(module.top_level.iter()) {
        let reachable = reachable_direct_block_labels(function)?;
        validate_function_in_reachable(function, &classes, &reachable)?;
        reachable_by_function.insert(function.name.clone(), reachable);
    }
    Ok(reachable_by_function)
}

#[cfg(test)]
fn validate_function(
    function: &MirFunction,
    classes: &HashMap<String, MirClass>,
) -> std::result::Result<(), String> {
    let reachable = reachable_direct_block_labels(function)?;
    validate_function_in_reachable(function, classes, &reachable)
}

fn validate_function_in_reachable(
    function: &MirFunction,
    classes: &HashMap<String, MirClass>,
    reachable: &HashSet<String>,
) -> std::result::Result<(), String> {
    if function.receiver.is_some() {
        receiver_type(function, classes)?;
    }
    for param in &function.params {
        ensure_direct_type(
            &param.ty,
            classes,
            &format!("parameter `{}` on `{}`", param.name, function.name),
        )?;
    }
    ensure_direct_type(
        &function.return_type,
        classes,
        &format!("return type of `{}`", function.name),
    )?;
    for local in &function.local_types {
        ensure_direct_type(
            &local.ty,
            classes,
            &format!("local `{}` on `{}`", local.name, function.name),
        )?;
    }
    for block in &function.blocks {
        if !reachable.contains(&block.label) {
            continue;
        }
        for instruction in &block.instructions {
            match instruction {
                Instruction::Safepoint
                | Instruction::IndexInRange { .. }
                | Instruction::Retain { .. }
                | Instruction::Release { .. }
                | Instruction::Lend { .. }
                | Instruction::EndLend { .. } => {}
                Instruction::BeginElementLoan { selector, .. }
                | Instruction::ReborrowElement { selector, .. } => validate_operand(selector)?,
                Instruction::BeginLoan { .. }
                | Instruction::BeginReturnedLoan { .. }
                | Instruction::Reborrow { .. }
                | Instruction::ReadLoan { .. }
                | Instruction::EndLoan { .. }
                | Instruction::ReturnLoan { .. } => {}
                Instruction::WriteLoan { value, .. } => validate_rvalue(value, classes)?,
                Instruction::Assign { value, .. } => validate_rvalue(value, classes)?,
                Instruction::Eval { value } => validate_operand(value)?,
                Instruction::PushCleanup { .. } | Instruction::PopCleanup { .. } => {}
            }
        }
        match &block.terminator {
            Terminator::Return(operand) => validate_operand(operand)?,
            Terminator::Goto(_) | Terminator::Unreachable => {}
            Terminator::Branch { condition, .. } => {
                validate_non_consuming_operand(condition, "a branch condition")?
            }
            Terminator::ForRange { iterable, .. } => validate_operand(iterable)?,
            Terminator::Match { scrutinee, .. } => validate_operand(scrutinee)?,
            Terminator::AssertFail {
                message, captures, ..
            } => {
                if let Some(message) = message {
                    validate_operand(message)?;
                }
                if !captures.is_empty() && captures.len() != 2 {
                    return Err(
                        "direct backend requires exactly two assertion captures when captures are present"
                            .to_string(),
                    );
                }
                for capture in captures {
                    validate_operand(&capture.value)?;
                }
            }
        }
    }
    Ok(())
}

fn validate_rvalue(
    rvalue: &Rvalue,
    classes: &HashMap<String, MirClass>,
) -> std::result::Result<(), String> {
    match rvalue {
        Rvalue::UnionTagTest { .. } | Rvalue::UnionTakePayload { .. } => Ok(()),
        Rvalue::Use(operand)
        | Rvalue::CallableAdapt { value: operand, .. }
        | Rvalue::NoneTest { value: operand }
        | Rvalue::UnionInject { value: operand, .. } => validate_operand(operand),
        Rvalue::ModuleConstant { .. } => Ok(()),
        Rvalue::Closure {
            signature,
            captures,
            ..
        } => {
            ensure_direct_type(signature, classes, "closure signature")?;
            for capture in captures.iter() {
                validate_operand(&capture.value)?;
                ensure_direct_type(&capture.ty, classes, "closure capture")?;
            }
            Ok(())
        }
        Rvalue::FormatString { parts } => {
            for value in parts.iter().filter_map(MirFormatPart::operand) {
                validate_non_consuming_operand(value, "format-string interpolation")?;
            }
            Ok(())
        }
        Rvalue::Unary { value, .. } => validate_non_consuming_operand(value, "a unary expression"),
        Rvalue::Cast { value, ty, .. } => {
            validate_non_consuming_operand(value, "a cast expression")?;
            ensure_direct_type(ty, classes, "cast target")?;
            Ok(())
        }
        Rvalue::Binary { left, right, .. } => {
            validate_non_consuming_operand(left, "the left side of a binary expression")?;
            validate_non_consuming_operand(right, "the right side of a binary expression")
        }
        Rvalue::Call { callee, args, .. } => {
            if matches!(callee, CallTarget::Member { field, .. } if field == "__slice") {
                direct_internal_slice_args(args)?;
            }
            match callee {
                CallTarget::Name(_) => {}
                CallTarget::Value(function) => {
                    validate_non_consuming_operand(function, "an indirect-call target")?
                }
                CallTarget::Extern(call) => {
                    for param in &call.params {
                        ensure_direct_type(&param.ty, classes, "extern parameter")?;
                        direct_ffi_type_for_source(&param.ty, Some(param.passing))?;
                    }
                    ensure_direct_type(&call.return_type, classes, "extern return")?;
                    direct_ffi_type_for_source(&call.return_type, None)?;
                }
                CallTarget::Member { object, .. } | CallTarget::TraitMember { object, .. } => {
                    validate_operand(object)?
                }
                CallTarget::Runtime { receiver, .. } => {
                    if let Some(object) = receiver {
                        validate_operand(object)?
                    }
                }
            }
            for argument in args {
                validate_operand(&argument.value)?;
            }
            Ok(())
        }
        Rvalue::VecLiteral {
            elements,
            element_type,
        } => {
            ensure_direct_type(element_type, classes, "Vec element")?;
            for element in elements {
                validate_operand(element)?;
            }
            Ok(())
        }
        Rvalue::TupleLiteral {
            elements,
            element_types,
        } => {
            if elements.len() != element_types.len() {
                return Err(format!(
                    "direct backend received tuple literal arity {} with {} element types",
                    elements.len(),
                    element_types.len()
                ));
            }
            for (element, element_type) in elements.iter().zip(element_types) {
                ensure_direct_type(element_type, classes, "tuple element")?;
                validate_operand(element)?;
            }
            Ok(())
        }
        Rvalue::TupleElement {
            tuple,
            element_type,
            ..
        } => {
            validate_tuple_projection_operand(tuple)?;
            ensure_direct_type(element_type, classes, "tuple element")?;
            Ok(())
        }
        Rvalue::TupleTakeElement {
            place,
            element_type,
            ..
        } => {
            validate_tuple_take_place(place)?;
            ensure_direct_type(element_type, classes, "tuple element")?;
            Ok(())
        }
        Rvalue::MapLiteral {
            entries,
            key_type,
            value_type,
        } => {
            ensure_direct_type(key_type, classes, "dict key")?;
            ensure_direct_type(value_type, classes, "Map value")?;
            for entry in entries {
                validate_operand(&entry.key)?;
                validate_operand(&entry.value)?;
            }
            Ok(())
        }
        Rvalue::SetLiteral {
            elements,
            element_type,
        } => {
            ensure_direct_type(element_type, classes, "set element")?;
            for element in elements {
                validate_operand(element)?;
            }
            Ok(())
        }
        Rvalue::Construct { class_name, fields } => {
            ensure_direct_type(
                &Type::named(class_name),
                classes,
                &format!("class `{}`", class_name),
            )?;
            for field in fields {
                validate_operand(&field.value)?;
            }
            Ok(())
        }
        Rvalue::Member { object, .. } => validate_non_consuming_operand(object, "a field read"),
        Rvalue::EnumVariant { payloads, .. } => {
            for payload in payloads {
                validate_operand(payload)?;
            }
            Ok(())
        }
        Rvalue::VariantPayload { scrutinee, .. } => validate_operand(scrutinee),
        Rvalue::Try { value, .. } => validate_operand(value),
        Rvalue::StartTask {
            stack_size,
            task_group,
            function,
            args,
            ..
        } => {
            if let Some(stack_size) = stack_size {
                validate_non_consuming_operand(stack_size, "a task stack size")?;
            }
            validate_non_consuming_operand(task_group, "a task-group receiver")?;
            validate_non_consuming_operand(function, "a task function value")?;
            for argument in args {
                validate_operand(&argument.value)?;
            }
            Ok(())
        }
    }
}

fn validate_operand(operand: &Operand) -> std::result::Result<(), String> {
    match operand {
        Operand::Place(_)
        | Operand::MovePlace(_)
        | Operand::Function { .. }
        | Operand::Int(_)
        | Operand::Bool(_)
        | Operand::Unit
        | Operand::Float(_)
        | Operand::String(_)
        | Operand::Duration(_)
        | Operand::Default(_) => Ok(()),
    }
}

fn validate_non_consuming_operand(
    operand: &Operand,
    context: &str,
) -> std::result::Result<(), String> {
    validate_operand(operand)?;
    if matches!(operand, Operand::MovePlace(_)) {
        return Err(format!(
            "direct backend only permits `MovePlace` in consuming contexts, not in {context}"
        ));
    }
    Ok(())
}

fn validate_tuple_projection_operand(operand: &Operand) -> std::result::Result<(), String> {
    validate_non_consuming_operand(operand, "tuple indexed access").map_err(|_| {
        "direct backend refuses consuming tuple projection; indexed access only reads Copy elements"
            .to_string()
    })
}

fn validate_tuple_take_place(place: &str) -> std::result::Result<(), String> {
    if place.starts_with("%t") {
        return Ok(());
    }
    Err(
        "destructive tuple extraction is internal to whole-tuple destructuring and requires a private captured temporary"
            .to_string(),
    )
}

fn ensure_direct_type(
    ty: &Type,
    classes: &HashMap<String, MirClass>,
    context: &str,
) -> std::result::Result<DirectType, String> {
    direct_type(ty, classes).ok_or({
        format!(
            "direct backend does not yet support {} with type `{}`",
            context, ty
        )
    })
}

fn direct_ffi_type_for_source(
    ty: &Type,
    passing: Option<MirReceiverKind>,
) -> std::result::Result<DirectFfiType, String> {
    let Type::Named(name, args) = ty else {
        if *ty == Type::Unit && passing.is_none() {
            return Ok(DirectFfiType::scalar(FfiType::Unit));
        }
        // Semantic analysis admits exactly `Handle | None` as a result
        // (ADR-0052 A10); it is one nullable C pointer.
        if let (Type::Union(union), None) = (ty, passing) {
            if union.members.len() == 2 && union.members.contains(&Type::Unit) {
                if let Some(Type::Named(handle, handle_args)) =
                    union.members.iter().find(|member| **member != Type::Unit)
                {
                    if handle_args.is_empty() {
                        return Ok(DirectFfiType::nullable(handle.clone(), ty.clone()));
                    }
                }
            }
        }
        return Err(format!("direct backend cannot lower `{ty}` through FFI v0"));
    };
    if args.is_empty() {
        let scalar = match name.as_str() {
            "bool" => Some(FfiType::Bool),
            "int8" => Some(FfiType::I8),
            "int16" => Some(FfiType::I16),
            "int32" => Some(FfiType::I32),
            "int" | "int64" => Some(FfiType::I64),
            "uint8" => Some(FfiType::U8),
            "uint16" => Some(FfiType::U16),
            "uint32" => Some(FfiType::U32),
            "uint64" => Some(FfiType::U64),
            "float32" => Some(FfiType::F32),
            "float64" => Some(FfiType::F64),
            "str" if passing == Some(MirReceiverKind::Borrow) => Some(FfiType::StringView),
            _ => None,
        };
        if let Some(ffi_type) = scalar {
            if ffi_type != FfiType::StringView {
                if let Some(passing) = passing {
                    if passing != MirReceiverKind::Borrow {
                        return Err(format!(
                            "direct backend cannot pass `{ty}` with ownership mode `{passing:?}` through FFI v0"
                        ));
                    }
                }
            }
            return Ok(DirectFfiType::scalar(ffi_type));
        }
        if name == "str" {
            return Err(
                "direct backend can pass `str` through FFI v0 only as a shared view".to_string(),
            );
        }
        if passing == Some(MirReceiverKind::BorrowMut) {
            return Err(format!(
                "direct backend cannot mutably borrow opaque handle `{ty}` through FFI v0"
            ));
        }
        return Ok(DirectFfiType::opaque(name.clone()));
    }
    if name == "list" && args.as_slice() == [Type::named("uint8")] {
        return Ok(DirectFfiType::scalar(match passing {
            Some(MirReceiverKind::Borrow) => FfiType::BytesView,
            Some(MirReceiverKind::BorrowMut) => FfiType::BytesViewMut,
            Some(MirReceiverKind::Value) => {
                return Err(
                    "direct backend cannot pass `own list[uint8]` through FFI v0".to_string(),
                )
            }
            None => {
                return Err("direct backend cannot return `list[uint8]` through FFI v0".to_string())
            }
        }));
    }
    Err(format!("direct backend cannot lower `{ty}` through FFI v0"))
}

/// Whether a value of `ty` owns a runtime handle: an opaque value, an inline
/// callable, or an inline union with an opaque or callable member.
fn layout_owns_handles(ty: &DirectType) -> bool {
    match ty {
        DirectType::Opaque(_) | DirectType::Callable(_) => true,
        DirectType::Union(union) => union.owns_handles(),
        DirectType::Scalar(_) | DirectType::PlainClass(_) => false,
    }
}

fn direct_type(ty: &Type, classes: &HashMap<String, MirClass>) -> Option<DirectType> {
    let mut visiting = BTreeSet::new();
    direct_type_inner(ty, classes, &mut visiting)
}

fn direct_type_inner(
    ty: &Type,
    classes: &HashMap<String, MirClass>,
    visiting: &mut BTreeSet<String>,
) -> Option<DirectType> {
    match ty {
        Type::Union(union) => {
            // Every concrete union is inline: a scalar, `None`, or plain-class
            // member occupies its own words, and a runtime-object member
            // occupies one owned handle word that the tag switches retain and
            // release. A union naming a type parameter keeps the generic
            // frame's symbolic runtime layout.
            let mut type_params = BTreeSet::new();
            collect_type_params_from_type(ty, &mut type_params);
            if !type_params.is_empty() {
                return Some(DirectType::Opaque(ty.clone()));
            }
            // A member whose canonical key names a class this module knows
            // under its module-qualified name resolves through that key, so
            // a local spelling and its qualified import share one layout;
            // an entry-module class keeps its own spelling, which is how the
            // class table names it.
            let mut members = Vec::new();
            for (index, member) in union.members.iter().enumerate() {
                let resolved = union
                    .keys
                    .get(index)
                    .and_then(|key| crate::mir::methods::canonical_named_type_from_key(key))
                    .filter(|candidate| {
                        matches!(candidate, Type::Named(name, _) if classes.contains_key(name))
                    })
                    .unwrap_or_else(|| member.clone());
                members.push(direct_type_inner(&resolved, classes, visiting)?);
            }
            Some(DirectType::Union(DirectUnionType::new(ty.clone(), members)))
        }
        Type::Unit => Some(DirectType::Scalar(ScalarKind::Unit)),
        Type::TypeParam(name) => Some(DirectType::Opaque(Type::TypeParam(name.clone()))),
        Type::ReturnedView(_) => None,
        Type::Module(path) => Some(DirectType::Opaque(Type::Module(path.clone()))),
        Type::Tuple(elements) => {
            for element in elements {
                direct_type_inner(element, classes, visiting)?;
            }
            Some(DirectType::Opaque(Type::Tuple(elements.clone())))
        }
        Type::Function { .. } | Type::Closure { .. } | Type::Callable(_) => {
            Some(DirectType::Callable(DirectCallableType::new(ty.clone())))
        }
        Type::Named(name, args) if args.is_empty() && name == "int32" => {
            Some(DirectType::Scalar(ScalarKind::Int32))
        }
        Type::Named(name, args) if args.is_empty() && name == "int64" => {
            Some(DirectType::Scalar(ScalarKind::Int64))
        }
        Type::Named(name, args) if args.is_empty() && name == "uint64" => {
            Some(DirectType::Scalar(ScalarKind::Uint64))
        }
        Type::Named(name, args) if args.is_empty() && name == "bool" => {
            Some(DirectType::Scalar(ScalarKind::Bool))
        }
        Type::Named(name, args) if args.is_empty() && name == "float32" => {
            Some(DirectType::Scalar(ScalarKind::Float32))
        }
        Type::Named(name, args) if args.is_empty() && name == "float64" => {
            Some(DirectType::Scalar(ScalarKind::Float64))
        }
        Type::Named(name, args) if args.is_empty() && name == "random.Rng" => {
            Some(DirectType::Opaque(Type::named(name)))
        }
        Type::Named(name, args) if args.is_empty() => {
            if let Some(class) = classes.get(name) {
                if !visiting.insert(name.clone()) {
                    return Some(DirectType::Opaque(Type::Named(name.clone(), vec![])));
                }
                let mut fields = Vec::new();
                for field in &class.fields {
                    let Some(field_ty) = direct_type_inner(&field.ty, classes, visiting) else {
                        visiting.remove(name);
                        return Some(DirectType::Opaque(Type::Named(name.clone(), vec![])));
                    };
                    let owns_handles = match &field_ty {
                        DirectType::Opaque(_) | DirectType::Callable(_) => true,
                        DirectType::Union(union) => union.owns_handles(),
                        _ => false,
                    };
                    if owns_handles {
                        visiting.remove(name);
                        return Some(DirectType::Opaque(Type::Named(name.clone(), vec![])));
                    }
                    fields.push(PlainClassField {
                        name: field.name.clone(),
                        ty: field_ty,
                    });
                }
                visiting.remove(name);
                return Some(DirectType::PlainClass(PlainClassType {
                    class_name: name.clone(),
                    fields,
                }));
            }
            Some(DirectType::Opaque(Type::Named(name.clone(), vec![])))
        }
        Type::Named(name, args) => {
            Some(DirectType::Opaque(Type::Named(name.clone(), args.clone())))
        }
    }
}

fn collect_type_params_from_type(ty: &Type, collected: &mut BTreeSet<String>) {
    match ty {
        Type::Union(union) => {
            for member in &union.members {
                collect_type_params_from_type(member, collected);
            }
        }
        Type::TypeParam(name) => {
            collected.insert(name.clone());
        }
        Type::Named(_, args) => {
            for arg in args {
                collect_type_params_from_type(arg, collected);
            }
        }
        Type::Tuple(elements) => {
            for element in elements {
                collect_type_params_from_type(element, collected);
            }
        }
        Type::Function {
            params,
            return_type,
            ..
        } => {
            for param in params.iter() {
                collect_type_params_from_type(&param.ty, collected);
            }
            collect_type_params_from_type(return_type, collected);
        }
        Type::ReturnedView(view) => collect_type_params_from_type(&view.pointee, collected),
        Type::Callable(callable) => {
            for param in &callable.params {
                collect_type_params_from_type(&param.ty, collected);
            }
            collect_type_params_from_type(&callable.return_type, collected);
        }
        Type::Closure {
            params,
            return_type,
            captures,
            ..
        } => {
            for param in params.iter() {
                collect_type_params_from_type(&param.ty, collected);
            }
            for capture in captures.iter() {
                collect_type_params_from_type(&capture.ty, collected);
            }
            collect_type_params_from_type(return_type, collected);
        }
        Type::Unit | Type::Module(_) => {}
    }
}

fn enum_variant_payload_types_for_target(
    enum_name: &str,
    variant_name: &str,
    target: &DirectType,
    classes: &HashMap<String, MirClass>,
) -> Option<Vec<DirectType>> {
    let DirectType::Opaque(Type::Named(name, args)) = target else {
        return None;
    };
    if name != enum_name {
        return None;
    }
    let payload_types = match (name.as_str(), args.as_slice(), variant_name) {
        ("Lookup", [inner], "Found") => vec![inner.clone()],
        ("Lookup", [_], "Missing") => Vec::new(),
        ("Poll", [inner], "Ready") => vec![inner.clone()],
        ("Poll", [_], "Unavailable") => Vec::new(),
        ("Result", [ok, _], "Ok") => vec![ok.clone()],
        ("Result", [_, err], "Err") => vec![err.clone()],
        ("SendError", [inner], "Closed" | "Cancelled" | "TimedOut" | "Full") => {
            vec![inner.clone()]
        }
        ("QueueReceive", [inner], "Item") => vec![inner.clone()],
        ("QueueReceive", [_], "Closed" | "TimedOut" | "Cancelled") => Vec::new(),
        ("TaskResult", [inner], "Ready") => vec![inner.clone()],
        ("TaskResult", [_], "Error") => vec![Type::named("str")],
        ("TaskResult", [_], "TimedOut" | "Cancelled") => Vec::new(),
        ("WaitAny", [inner], "Ready") => vec![Type::named("int64"), inner.clone()],
        ("WaitAny", [_], "Error") => vec![Type::named("int64"), Type::named("str")],
        ("WaitAny", [_], "TimedOut" | "Cancelled") => Vec::new(),
        ("WaitAll", [inner], "Ready") => vec![Type::Named("list".to_string(), vec![inner.clone()])],
        ("WaitAll", [_], "Error") => vec![Type::named("int64"), Type::named("str")],
        ("WaitAll", [_], "TimedOut" | "Cancelled") => Vec::new(),
        ("SelectOutcome", [queue, _], "Queue") => vec![
            Type::named("int64"),
            Type::Named("QueueReceive".to_string(), vec![queue.clone()]),
        ],
        ("SelectOutcome", [_, task], "Task") => vec![
            Type::named("int64"),
            Type::Named("TaskResult".to_string(), vec![task.clone()]),
        ],
        ("SelectOutcome", [_, _], "Deadline") => vec![Type::named("int64")],
        ("SelectOutcome", [_, _], "Cancelled") => Vec::new(),
        _ => return None,
    };
    Some(
        payload_types
            .iter()
            .map(|payload_ty| {
                direct_type(payload_ty, classes).unwrap_or(DirectType::Opaque(payload_ty.clone()))
            })
            .collect::<Vec<_>>(),
    )
}

fn direct_field_type_with_enums(
    ty: &DirectType,
    field: &str,
    classes: &HashMap<String, MirClass>,
    enums: &HashMap<String, MirEnum>,
) -> Option<DirectType> {
    if let Some((variant, index)) = crate::mir::enum_payload_projection(field) {
        let DirectType::Opaque(Type::Named(name, args)) = ty else {
            return None;
        };
        let layout = enums.get(name)?;
        if args.len() != layout.type_params.len() {
            return None;
        }
        let payload = layout
            .variants
            .iter()
            .find(|candidate| candidate.name == variant)?
            .payloads
            .get(index)?;
        let substitutions = layout
            .type_params
            .iter()
            .cloned()
            .zip(args.iter().cloned())
            .collect::<HashMap<_, _>>();
        return direct_type(&substitute_type(payload, &substitutions), classes);
    }
    direct_field_type(ty, field, classes)
}

fn direct_field_type(
    ty: &DirectType,
    field: &str,
    classes: &HashMap<String, MirClass>,
) -> Option<DirectType> {
    if let Some((_, _, field_ty)) = ty.field_slice(field) {
        return Some(field_ty);
    }
    if let DirectType::Opaque(Type::Tuple(elements)) = ty {
        let index = field.parse::<usize>().ok()?;
        return direct_type(elements.get(index)?, classes);
    }
    if let DirectType::Opaque(Type::Union(union)) = ty {
        let index = crate::mir::union_payload_projection_index(field)?;
        return direct_type(union.members.get(index)?, classes);
    }
    let DirectType::Opaque(Type::Named(class_name, args)) = ty else {
        return None;
    };
    let class = classes.get(class_name)?;
    if args.len() != class.type_params.len() {
        return None;
    }
    let field_info = class
        .fields
        .iter()
        .find(|candidate| candidate.name == field)?;
    let substitutions = class
        .type_params
        .iter()
        .cloned()
        .zip(args.iter().cloned())
        .collect::<HashMap<_, _>>();
    direct_type(&substitute_type(&field_info.ty, &substitutions), classes)
}

fn infer_operand_type(
    operand: &Operand,
    variable_types: &HashMap<String, DirectType>,
    classes: &HashMap<String, MirClass>,
) -> Option<DirectType> {
    match operand {
        Operand::Place(place) | Operand::MovePlace(place) => {
            let mut segments = place.split('.');
            let root = segments.next()?;
            let mut ty = variable_types.get(root)?.clone();
            for field in segments {
                ty = direct_field_type(&ty, field, classes)?;
            }
            Some(ty)
        }
        Operand::Int(value) => {
            if i64::try_from(*value).is_ok() {
                Some(DirectType::Scalar(ScalarKind::Int64))
            } else {
                Some(DirectType::Opaque(Type::named("Unknown")))
            }
        }
        Operand::Float(_) => Some(DirectType::Scalar(ScalarKind::Float64)),
        Operand::Bool(_) => Some(DirectType::Scalar(ScalarKind::Bool)),
        Operand::Unit => Some(DirectType::Scalar(ScalarKind::Unit)),
        Operand::String(_) => Some(DirectType::Opaque(Type::named("str"))),
        Operand::Duration(_) => Some(DirectType::Opaque(Type::named("Duration"))),
        Operand::Function { signature, .. } => Some(DirectType::Callable(DirectCallableType::new(
            signature.as_ref().clone(),
        ))),
        Operand::Default(default) => direct_type(&default.ty()?, classes),
    }
}

fn render_direct_type(ty: &DirectType) -> String {
    match ty {
        DirectType::Scalar(ScalarKind::Int32) => "int32".to_string(),
        DirectType::Scalar(ScalarKind::Int64) => "int64".to_string(),
        DirectType::Scalar(ScalarKind::Uint64) => "uint64".to_string(),
        DirectType::Scalar(ScalarKind::Float32) => "float32".to_string(),
        DirectType::Scalar(ScalarKind::Float64) => "float64".to_string(),
        DirectType::Scalar(ScalarKind::Bool) => "bool".to_string(),
        DirectType::Scalar(ScalarKind::Unit) => "None".to_string(),
        DirectType::PlainClass(class) => class.class_name.clone(),
        DirectType::Union(union) => union.union_type.to_string(),
        DirectType::Callable(callable) => callable.signature.to_string(),
        DirectType::Opaque(ty) => ty.to_string(),
    }
}

fn thunk_string_constant(
    codegen: &mut NativeCodegen<'_>,
    builder: &mut dyn FunctionBackend,
    bytes: &[u8],
) -> std::result::Result<(Val, Val), String> {
    let id = if let Some(id) = codegen.string_data.get(bytes) {
        *id
    } else {
        let name = format!("aura_data_{}", codegen.string_data.len());
        let id = match builder.module().declare_data(&name, Linkage::Local, false) {
            Ok(id) => id,
            Err(error) => return Err(format!("failed to declare string data: {}", error)),
        };
        if let Err(error) = builder
            .module()
            .define_data(id, DataDescription::new(bytes.to_vec()))
        {
            return Err(format!("failed to define string data: {}", error));
        }
        codegen.string_data.insert(bytes.to_vec(), id);
        id
    };
    let global = builder.import_data(id);
    let ptr = builder.symbol_value(types::I64, global);
    let len = builder.iconst(types::I64, bytes.len() as i64);
    Ok((ptr, len))
}

/// A tag switch in a thunk's own builder: `body` runs once per member and
/// its results, typed `result_types`, merge into one block.
fn thunk_switch_on_tag(
    builder: &mut dyn FunctionBackend,
    tag: Val,
    member_count: usize,
    result_types: &[ScalarType],
    mut body: impl FnMut(&mut dyn FunctionBackend, usize) -> std::result::Result<Vec<Val>, String>,
) -> std::result::Result<Vec<Val>, String> {
    let merge = builder.create_block();
    for abi in result_types {
        builder.append_block_param(merge, *abi);
    }
    for index in 0..member_count {
        let matched = builder.create_block();
        let next = (index + 1 < member_count).then(|| builder.create_block());
        if let Some(next) = next {
            let selected = builder.icmp_imm(IntCC::Equal, tag, index as i64);
            builder.brif(selected, matched, &[], next, &[]);
        } else {
            builder.jump(matched, &[]);
        }
        builder.switch_to_block(matched);
        builder.seal_block(matched);
        let results = body(builder, index)?;
        builder.jump(merge, &results);
        if let Some(next) = next {
            builder.switch_to_block(next);
            builder.seal_block(next);
        }
    }
    builder.switch_to_block(merge);
    builder.seal_block(merge);
    Ok(builder.block_params(merge))
}

fn box_thunk_value(
    codegen: &mut NativeCodegen<'_>,
    builder: &mut dyn FunctionBackend,
    values: &[Val],
    ty: &DirectType,
) -> std::result::Result<Val, String> {
    match ty {
        DirectType::Callable(_) => callables::call_callable_box_adapter(builder, values),
        DirectType::Union(union) => {
            let union_inject = builder.import_function(codegen.runtime[RuntimeFn::UnionInject]);
            let encoded = crate::native_runtime::canonical_runtime_type_name(&union.union_type);
            let (ptr, len) = thunk_string_constant(codegen, builder, encoded.as_bytes())?;
            let tag = values[0];
            let words = values[1..].to_vec();
            thunk_switch_on_tag(
                builder,
                tag,
                union.members.len(),
                &[types::I64],
                |builder, index| {
                    let member_ty = &union.members[index].ty;
                    let mut member_values = Vec::new();
                    for (word, abi) in words.iter().zip(member_ty.abi_types()) {
                        member_values.push(if abi == types::F64 {
                            builder.bitcast(types::F64, *word)
                        } else {
                            *word
                        });
                    }
                    let payload = box_thunk_value(codegen, builder, &member_values, member_ty)?;
                    let index_value = builder.iconst(types::I64, index as i64);
                    let inst = builder.call(union_inject, &[ptr, len, index_value, payload]);
                    Ok(vec![inst.results[0]])
                },
            )
            .map(|values| values[0])
        }
        DirectType::Opaque(_) => values.first().copied().ok_or({
            format!(
                "task-start thunk expected an opaque value for `{}`",
                render_direct_type(ty)
            )
        }),
        DirectType::Scalar(ScalarKind::Int32) => {
            let box_i32 = builder.import_function(codegen.runtime[RuntimeFn::BoxI32]);
            let inst = builder.call(box_i32, &[values[0]]);
            Ok(inst.results[0])
        }
        DirectType::Scalar(ScalarKind::Int64) => {
            let box_i64 = builder.import_function(codegen.runtime[RuntimeFn::BoxI64]);
            let inst = builder.call(box_i64, &[values[0]]);
            Ok(inst.results[0])
        }
        DirectType::Scalar(ScalarKind::Uint64) => {
            let box_u64 = builder.import_function(codegen.runtime[RuntimeFn::BoxU64]);
            let inst = builder.call(box_u64, &[values[0]]);
            Ok(inst.results[0])
        }
        DirectType::Scalar(ScalarKind::Float32) | DirectType::Scalar(ScalarKind::Float64) => {
            let box_f64 = builder.import_function(codegen.runtime[RuntimeFn::BoxF64]);
            let inst = builder.call(box_f64, &[values[0]]);
            Ok(inst.results[0])
        }
        DirectType::Scalar(ScalarKind::Bool) => {
            let box_bool = builder.import_function(codegen.runtime[RuntimeFn::BoxBool]);
            let inst = builder.call(box_bool, &[values[0]]);
            Ok(inst.results[0])
        }
        DirectType::Scalar(ScalarKind::Unit) => {
            let box_unit = builder.import_function(codegen.runtime[RuntimeFn::BoxUnit]);
            let inst = builder.call(box_unit, &[]);
            Ok(inst.results[0])
        }
        DirectType::PlainClass(class) => {
            let instance_empty = builder.import_function(codegen.runtime[RuntimeFn::InstanceEmpty]);
            let instance_set_field_owned =
                builder.import_function(codegen.runtime[RuntimeFn::InstanceSetFieldOwned]);
            let (class_ptr, class_len) =
                thunk_string_constant(codegen, builder, class.class_name.as_bytes())?;
            let init = builder.call(instance_empty, &[class_ptr, class_len]);
            let current = init.results[0];
            let mut start = 0usize;
            for field in &class.fields {
                let end = start + field.ty.value_count();
                let field_value =
                    box_thunk_value(codegen, builder, &values[start..end], &field.ty)?;
                let (field_ptr, field_len) =
                    thunk_string_constant(codegen, builder, field.name.as_bytes())?;
                builder.call(
                    instance_set_field_owned,
                    &[current, field_ptr, field_len, field_value],
                );
                start = end;
            }
            Ok(current)
        }
    }
}

fn unbox_thunk_value(
    codegen: &mut NativeCodegen<'_>,
    builder: &mut dyn FunctionBackend,
    raw: Val,
    ty: &DirectType,
) -> std::result::Result<Vec<Val>, String> {
    match ty {
        DirectType::Callable(callable) => {
            callables::boxed_callable_words(codegen, builder, raw, &callable.signature)
        }
        DirectType::Union(union) => {
            let union_tag = builder.import_function(codegen.runtime[RuntimeFn::UnionTag]);
            let union_payload_copy =
                builder.import_function(codegen.runtime[RuntimeFn::UnionPayloadCopy]);
            let release_value = builder.import_function(codegen.runtime[RuntimeFn::ReleaseValue]);
            let tag_inst = builder.call(union_tag, &[raw]);
            let tag = tag_inst.results[0];
            let result_types = ty.abi_types();
            thunk_switch_on_tag(
                builder,
                tag,
                union.members.len(),
                &result_types,
                |builder, index| {
                    let member_ty = &union.members[index].ty;
                    let index_value = builder.iconst(types::I64, index as i64);
                    let copy = builder.call(union_payload_copy, &[raw, index_value]);
                    let payload = copy.results[0];
                    let member_values = unbox_thunk_value(codegen, builder, payload, member_ty)?;
                    // A scalar or plain-class member was read out of the copy,
                    // so the copy is released; a runtime-object member's handle
                    // is the copy itself and travels with the union words.
                    if !matches!(member_ty, DirectType::Opaque(_) | DirectType::Callable(_)) {
                        let _ = builder.call(release_value, &[payload]);
                    }
                    let mut words = vec![builder.iconst(types::I64, index as i64)];
                    for (value, abi) in member_values.iter().zip(member_ty.abi_types()) {
                        words.push(if abi == types::F64 {
                            builder.bitcast(types::I64, *value)
                        } else {
                            *value
                        });
                    }
                    while words.len() < union.payload_words + 1 {
                        words.push(builder.iconst(types::I64, 0));
                    }
                    Ok(words)
                },
            )
        }
        DirectType::Opaque(_) => Ok(vec![raw]),
        DirectType::Scalar(ScalarKind::Int32) => {
            let unbox_i64 = builder.import_function(codegen.runtime[RuntimeFn::UnboxI64]);
            let inst = builder.call(unbox_i64, &[raw]);
            Ok(inst.results)
        }
        DirectType::Scalar(ScalarKind::Int64) => {
            let unbox_int64 = builder.import_function(codegen.runtime[RuntimeFn::UnboxInt64]);
            let inst = builder.call(unbox_int64, &[raw]);
            Ok(inst.results)
        }
        DirectType::Scalar(ScalarKind::Uint64) => {
            let unbox_u64 = builder.import_function(codegen.runtime[RuntimeFn::UnboxU64]);
            let inst = builder.call(unbox_u64, &[raw]);
            Ok(inst.results)
        }
        DirectType::Scalar(ScalarKind::Float32) | DirectType::Scalar(ScalarKind::Float64) => {
            let unbox_f64 = builder.import_function(codegen.runtime[RuntimeFn::UnboxF64]);
            let inst = builder.call(unbox_f64, &[raw]);
            Ok(inst.results)
        }
        DirectType::Scalar(ScalarKind::Bool) => {
            let unbox_bool = builder.import_function(codegen.runtime[RuntimeFn::UnboxBool]);
            let inst = builder.call(unbox_bool, &[raw]);
            Ok(inst.results)
        }
        DirectType::Scalar(ScalarKind::Unit) => Ok(vec![builder.iconst(types::I64, 0)]),
        DirectType::PlainClass(class) => {
            let instance_get_field =
                builder.import_function(codegen.runtime[RuntimeFn::InstanceGetField]);
            let mut values = Vec::new();
            for field in &class.fields {
                let (field_ptr, field_len) =
                    thunk_string_constant(codegen, builder, field.name.as_bytes())?;
                let inst = builder.call(instance_get_field, &[raw, field_ptr, field_len]);
                let field_raw = inst.results[0];
                values.extend(unbox_thunk_value(codegen, builder, field_raw, &field.ty)?);
                if !matches!(field.ty, DirectType::Opaque(_) | DirectType::Callable(_)) {
                    let release_value =
                        builder.import_function(codegen.runtime[RuntimeFn::ReleaseValue]);
                    builder.call(release_value, &[field_raw]);
                }
            }
            Ok(values)
        }
    }
}

fn release_direct_call_results(
    codegen: &mut NativeCodegen<'_>,
    builder: &mut dyn FunctionBackend,
    function_name: &str,
    results: &[Val],
) -> std::result::Result<(), String> {
    let return_ty = codegen
        .function_return_types
        .get(function_name)
        .cloned()
        .ok_or({
            format!(
                "direct backend does not know return type for `{}`",
                function_name
            )
        })?;
    let return_count = return_ty.value_count();
    if results.len() < return_count {
        return Err(format!(
            "direct backend cleanup call `{}` returned too few values",
            function_name
        ));
    }
    release_direct_values(codegen, builder, &results[..return_count], &return_ty)?;
    let mut cursor = return_count;
    for writeback_ty in codegen
        .function_writeback_types
        .get(function_name)
        .cloned()
        .unwrap_or_default()
    {
        let count = writeback_ty.value_count();
        if results.len() < cursor + count {
            return Err(format!(
                "direct backend cleanup call `{}` returned incomplete writeback values",
                function_name
            ));
        }
        release_direct_values(
            codegen,
            builder,
            &results[cursor..cursor + count],
            &writeback_ty,
        )?;
        cursor += count;
    }
    Ok(())
}

fn release_direct_values(
    codegen: &mut NativeCodegen<'_>,
    builder: &mut dyn FunctionBackend,
    values: &[Val],
    ty: &DirectType,
) -> std::result::Result<(), String> {
    match ty {
        DirectType::Callable(_) => {
            callables::call_callable_unary_adapter(builder, values, callables::DESCRIPTOR_DROP)?;
        }
        DirectType::Union(union) => {
            if union.owns_handles() {
                let release_value =
                    builder.import_function(codegen.runtime[RuntimeFn::ReleaseValue]);
                let tag = values[0];
                let handle = values[1];
                for &index in &union.owning {
                    let active = builder.icmp_imm(IntCC::Equal, tag, index as i64);
                    let release_block = builder.create_block();
                    let continue_block = builder.create_block();
                    builder.brif(active, release_block, &[], continue_block, &[]);
                    builder.switch_to_block(release_block);
                    builder.seal_block(release_block);
                    if matches!(union.member(index), Ok(DirectType::Callable(_))) {
                        callables::call_callable_unary_adapter(
                            builder,
                            &values[1..2 + CALLABLE_ENVIRONMENT_WORDS],
                            callables::DESCRIPTOR_DROP,
                        )?;
                    } else {
                        builder.call(release_value, &[handle]);
                    }
                    builder.jump(continue_block, &[]);
                    builder.switch_to_block(continue_block);
                    builder.seal_block(continue_block);
                }
            }
        }
        DirectType::Opaque(_) => {
            let Some(value) = values.first().copied() else {
                return Err(format!(
                    "direct backend cleanup expected an opaque `{}` result",
                    render_direct_type(ty)
                ));
            };
            let release_value = builder.import_function(codegen.runtime[RuntimeFn::ReleaseValue]);
            builder.call(release_value, &[value]);
        }
        DirectType::PlainClass(class) => {
            let mut start = 0usize;
            for field in &class.fields {
                let end = start + field.ty.value_count();
                if values.len() < end {
                    return Err(format!(
                        "direct backend cleanup expected `{}` values for `{}`",
                        end,
                        render_direct_type(ty)
                    ));
                }
                release_direct_values(codegen, builder, &values[start..end], &field.ty)?;
                start = end;
            }
        }
        DirectType::Scalar(_) => {}
    }
    Ok(())
}

fn main_signature() -> Signature {
    let mut signature = Signature::new(CallConv::C);
    signature.returns.push(types::I32);
    signature
}

fn thunk_signature() -> Signature {
    let mut signature = Signature::new(CallConv::C);
    signature.params.push(types::I64);
    signature.params.push(types::I64);
    signature.returns.push(types::I64);
    signature
}

fn default_binder_signature() -> Signature {
    let mut signature = Signature::new(CallConv::C);
    signature.params.push(types::I64);
    signature.params.push(types::I64);
    signature.params.push(types::I64);
    signature
}

fn mangle_symbol(name: &str) -> String {
    let mut mangled = String::from("aura_fn_");
    for ch in name.chars() {
        if ch.is_ascii_alphanumeric() {
            mangled.push(ch);
        } else {
            mangled.push('_');
        }
    }
    mangled
}

fn public_direct_function_name(name: &str) -> String {
    name.split_once("::__default_")
        .map_or_else(|| name.to_string(), |(public, _)| public.to_string())
}

fn mangle_thunk_symbol(name: &str) -> String {
    let mut mangled = String::from("aura_thunk_");
    for ch in name.chars() {
        if ch.is_ascii_alphanumeric() {
            mangled.push(ch);
        } else {
            mangled.push('_');
        }
    }
    mangled
}

fn mangle_default_binder_symbol(name: &str) -> String {
    let mut mangled = String::from("aura_default_binder_");
    for ch in name.chars() {
        if ch.is_ascii_alphanumeric() {
            mangled.push(ch);
        } else {
            mangled.push('_');
        }
    }
    mangled
}

fn mangle_cleanup_thunk_symbol(function_name: &str, place: &str, index: usize) -> String {
    let mut mangled = format!("aura_cleanup_{}_", index);
    for ch in function_name
        .chars()
        .chain(std::iter::once('_'))
        .chain(place.chars())
    {
        if ch.is_ascii_alphanumeric() {
            mangled.push(ch);
        } else {
            mangled.push('_');
        }
    }
    mangled
}

fn direct_type_to_type(ty: &DirectType) -> Type {
    match ty {
        DirectType::Scalar(ScalarKind::Int32) => Type::named("int32"),
        DirectType::Scalar(ScalarKind::Int64) => Type::named("int64"),
        DirectType::Scalar(ScalarKind::Uint64) => Type::named("uint64"),
        DirectType::Scalar(ScalarKind::Float32) => Type::named("float32"),
        DirectType::Scalar(ScalarKind::Float64) => Type::named("float64"),
        DirectType::Scalar(ScalarKind::Bool) => Type::named("bool"),
        DirectType::Scalar(ScalarKind::Unit) => Type::Unit,
        DirectType::PlainClass(class) => Type::named(&class.class_name),
        DirectType::Union(union) => union.union_type.clone(),
        DirectType::Callable(callable) => callable.signature.clone(),
        DirectType::Opaque(ty) => ty.clone(),
    }
}

fn is_numeric_type_name(ty: &Type) -> bool {
    match ty {
        Type::Named(name, args) if args.is_empty() => {
            name == "float32"
                || name == "float64"
                || name.starts_with("int")
                || name.starts_with("uint")
        }
        _ => false,
    }
}

fn runtime_type_is_wildcard(ty: &Type) -> bool {
    match ty {
        Type::Union(union) => union.members.iter().any(runtime_type_is_wildcard),
        Type::TypeParam(_) => true,
        Type::Named(name, _) if name == "Unknown" => true,
        Type::Named(_, args) => args.iter().any(runtime_type_is_wildcard),
        Type::Tuple(elements) => elements.iter().any(runtime_type_is_wildcard),
        Type::Function {
            params,
            return_type,
            ..
        } => {
            params
                .iter()
                .any(|param| runtime_type_is_wildcard(&param.ty))
                || runtime_type_is_wildcard(return_type)
        }
        Type::ReturnedView(view) => runtime_type_is_wildcard(&view.pointee),
        Type::Callable(callable) => {
            callable
                .params
                .iter()
                .any(|param| runtime_type_is_wildcard(&param.ty))
                || runtime_type_is_wildcard(&callable.return_type)
        }
        Type::Closure {
            params,
            return_type,
            captures,
            ..
        } => {
            params
                .iter()
                .any(|param| runtime_type_is_wildcard(&param.ty))
                || captures
                    .iter()
                    .any(|capture| runtime_type_is_wildcard(&capture.ty))
                || runtime_type_is_wildcard(return_type)
        }
        Type::Unit | Type::Module(_) => false,
    }
}

fn collect_task_start_targets(
    module: &MirModule,
    reachable_by_function: &HashMap<String, HashSet<String>>,
) -> BTreeSet<String> {
    let mut targets = BTreeSet::new();
    for function in module.functions.iter().chain(module.top_level.iter()) {
        let Some(reachable) = reachable_by_function.get(&function.name) else {
            continue;
        };
        for block in &function.blocks {
            if !reachable.contains(&block.label) {
                continue;
            }
            for instruction in &block.instructions {
                if let Instruction::Assign {
                    value: Rvalue::StartTask { function, .. },
                    ..
                } = instruction
                {
                    targets.insert(match function {
                        Operand::Function { name, .. } => name.clone(),
                        _ => "<dynamic-function-value>".to_string(),
                    });
                }
            }
        }
    }
    targets
}

#[path = "native_codegen_reachability.rs"]
mod reachability;

#[path = "native_codegen_callables.rs"]
mod callables;

#[path = "native_codegen_lookups.rs"]
mod lookups;

#[cfg(test)]
#[path = "native_codegen_tests.rs"]
mod tests;

fn is_comparison_op(op: BinaryOp) -> bool {
    matches!(
        op,
        BinaryOp::Eq
            | BinaryOp::NotEq
            | BinaryOp::Less
            | BinaryOp::LessEq
            | BinaryOp::Greater
            | BinaryOp::GreaterEq
    )
}

/// Leaf types whose runtime tag never changes the value itself. This
/// matches the runtime's name-only tags.
fn is_name_only_runtime_type(ty: &Type) -> bool {
    matches!(
        ty,
        Type::Named(name, args)
            if args.is_empty()
                && matches!(name.as_str(), "str" | "bool" | "float32" | "float64" | "Duration")
    )
}
