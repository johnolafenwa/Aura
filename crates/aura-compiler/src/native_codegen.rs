use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};

use cranelift_codegen::ir::condcodes::{FloatCC, IntCC};
use cranelift_codegen::ir::immediates::Ieee64;
use cranelift_codegen::ir::TrapCode;
use cranelift_codegen::ir::{
    types, AbiParam, FuncRef, InstBuilder, MemFlags, Signature, StackSlotData, StackSlotKind,
    UserFuncName, Value,
};
use cranelift_codegen::isa::CallConv;
use cranelift_codegen::settings::{self, Configurable};
use cranelift_frontend::{FunctionBuilder, FunctionBuilderContext, Variable};
use cranelift_module::{default_libcall_names, DataDescription, DataId, FuncId, Linkage, Module};
use cranelift_object::{ObjectBuilder, ObjectModule};

use crate::ast::{BinaryOp, ReceiverKind, UnaryOp};
use crate::call::BuiltinMember;
use crate::diag::Span;
use crate::ffi::FfiType;
use crate::mir::{
    BasicBlock, CallTarget, CleanupAction, Instruction, MirArg, MirClass, MirEnum, MirExternCall,
    MirFormatPart, MirFunction, MirMapEntry, MirMethod, MirModule, MirReceiverKind, MirTraitImpl,
    Operand, Rvalue, Terminator, NATIVE_LOOP_SAFEPOINT_INTERVAL,
};
use crate::native_runtime::{
    encode_direct_ffi_call_spec, DirectFfiCallSpec, DirectFfiParam, DirectFfiType,
};
use crate::runtime_abi::{AbiType, RuntimeFn, RuntimeTable};
use crate::runtime_ops::{
    FunctionOp, HostBuiltin, PrintKind, ResourceOp, RuntimeOp, RuntimeTarget, ScalarOp, ValueOp,
};
use crate::sema::{optional_type, text_convenience_result_type};
use crate::sema::{substitute_type, FunctionParamContract, Type};

use callables::{CallableShape, CallableShapeKey};

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
    let context = NativeCodegen::new(&module, program_path, program_source)?;
    context.emit()
}

fn set_native_codegen_flag(
    flag_builder: &mut settings::Builder,
    name: &str,
    value: &str,
) -> std::result::Result<(), String> {
    match flag_builder.set(name, value) {
        Ok(()) => Ok(()),
        Err(error) => Err(format!("failed to configure native backend: {error}")),
    }
}

fn native_codegen_flags() -> std::result::Result<settings::Flags, String> {
    let mut flag_builder = settings::builder();
    set_native_codegen_flag(&mut flag_builder, "opt_level", "speed")?;
    set_native_codegen_flag(&mut flag_builder, "is_pic", "true")?;
    set_native_codegen_flag(&mut flag_builder, "unwind_info", "true")?;
    // Aura's direct-call ABI is private to one generated object and flattens
    // mutable receiver/parameter writeback into additional result values. On
    // x86-64 that can exceed the two integer return registers. Cranelift must
    // lower the overflow through its implicit return area so every Aura
    // caller and callee keeps the same internal signature.
    set_native_codegen_flag(&mut flag_builder, "enable_multi_ret_implicit_sret", "true")?;
    Ok(settings::Flags::new(flag_builder))
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
    fn signature_type(self) -> cranelift_codegen::ir::Type {
        match self {
            ScalarKind::Int32
            | ScalarKind::Int64
            | ScalarKind::Uint64
            | ScalarKind::Bool
            | ScalarKind::Unit => types::I64,
            ScalarKind::Float32 | ScalarKind::Float64 => types::F64,
        }
    }

    fn zero_value(self, builder: &mut FunctionBuilder<'_>) -> Value {
        match self {
            ScalarKind::Int32
            | ScalarKind::Int64
            | ScalarKind::Uint64
            | ScalarKind::Bool
            | ScalarKind::Unit => builder.ins().iconst(types::I64, 0),
            ScalarKind::Float32 | ScalarKind::Float64 => {
                builder.ins().f64const(Ieee64::with_float(0.0))
            }
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

/// What an `IndexInRange { list, index }` fact proves about the
/// instruction after it.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ProvenAccess {
    None,
    /// `list.__index(index, ..)`: the element exists, so the read needs no
    /// bounds check.
    Read,
    /// `index = index + 1`: `index` is below a list length, so the sum
    /// cannot overflow.
    Advance,
    /// An element loan of `list[index]`: the element exists, so the loan
    /// begins without a bounds check.
    Lend,
}

impl ProvenAccess {
    fn of(instruction: &Instruction, list: &str, index: &str) -> Self {
        if let Instruction::BeginElementLoan {
            source,
            selector: Operand::Place(selected),
            projection,
            ..
        } = instruction
        {
            return if source == list && selected == index && projection.is_empty() {
                Self::Lend
            } else {
                Self::None
            };
        }
        let Instruction::Assign { target, value } = instruction else {
            return Self::None;
        };
        match value {
            Rvalue::Call {
                callee:
                    CallTarget::Member {
                        object: Operand::Place(object),
                        field,
                        ..
                    },
                args,
            } if object == list
                && field == "__index"
                && target != list
                && target != index
                && matches!(
                    args.first(),
                    Some(MirArg { value: Operand::Place(place), .. }) if place == index
                ) =>
            {
                Self::Read
            }
            Rvalue::Binary {
                op: BinaryOp::Add,
                left: Operand::Place(left),
                right: Operand::Int(1),
                ..
            } if target == index && left == index => Self::Advance,
            _ => Self::None,
        }
    }
}

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
    fn abi_types(&self) -> Vec<cranelift_codegen::ir::Type> {
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

    fn zero_values(&self, builder: &mut FunctionBuilder<'_>) -> Vec<Value> {
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
                let mut values = vec![builder.ins().iconst(types::I64, 0)];
                for _ in 0..union.payload_words {
                    values.push(builder.ins().iconst(types::I64, 0));
                }
                values
            }
            DirectType::Callable(_) => (0..1 + CALLABLE_ENVIRONMENT_WORDS)
                .map(|_| builder.ins().iconst(types::I64, 0))
                .collect(),
            DirectType::Opaque(_) => vec![builder.ins().iconst(types::I64, 0)],
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
    values: Vec<Value>,
    ty: DirectType,
}

/// The temporaries a statement owns: opaque handles, and inline unions
/// whose active member may hold a handle (keyed by the tag word). They are
/// ordered by value, so releases come out in the same order in every build.
#[derive(Clone, Default)]
struct OwnedTemporaries {
    opaque: BTreeSet<Value>,
    unions: BTreeMap<Value, (Vec<Value>, DirectUnionType)>,
    /// Inline callables the statement owns, keyed by the descriptor word.
    callables: BTreeMap<Value, Vec<Value>>,
}

impl OwnedTemporaries {
    fn contains(&self, value: &Value) -> bool {
        self.opaque.contains(value)
    }

    fn insert(&mut self, value: Value) -> bool {
        self.opaque.insert(value)
    }

    fn remove(&mut self, value: &Value) -> bool {
        self.opaque.remove(value)
    }

    fn clear(&mut self) {
        self.opaque.clear();
        self.unions.clear();
        self.callables.clear();
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
    reachable_blocks: HashMap<String, HashSet<String>>,
    safepoints_enabled: bool,
    program_path: String,
    program_source: String,
    object: ObjectModule,
    functions: HashMap<String, FuncId>,
    function_thunks: HashMap<String, FuncId>,
    function_default_binders: HashMap<String, FuncId>,
    callable_shapes: HashMap<CallableShapeKey, CallableShape>,
    contract_descriptors: HashMap<String, callables::ContractDescriptor>,
    cleanup_thunks: HashMap<(String, String), FuncId>,
    classes: HashMap<String, MirClass>,
    enums: HashMap<String, MirEnum>,
    union_layouts: HashMap<String, crate::union_layout::MirUnionLayout>,
    trait_impls: Vec<MirTraitImpl>,
    function_return_types: HashMap<String, DirectType>,
    function_param_types: HashMap<String, Vec<DirectType>>,
    /// Each parameter's passing mode, the receiver first, parallel to
    /// `function_param_types`.
    function_param_passing: HashMap<String, Vec<MirReceiverKind>>,
    function_writeback_types: HashMap<String, Vec<DirectType>>,
    call_conv: CallConv,
    runtime: RuntimeTable<FuncId>,
    string_data: HashMap<Vec<u8>, DataId>,
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

/// The opcode the direct runtime's text dispatcher expects for one
/// `str` text-convenience method. It must match
/// `native_runtime::aura_direct_string_text_method`.
pub(crate) use crate::text_methods::text_method_opcode;

fn ordered_named_args<'a>(
    expected_names: &[&str],
    args: &'a [MirArg],
) -> std::result::Result<Vec<&'a MirArg>, String> {
    let mut values = vec![None; expected_names.len()];
    let mut next_positional = 0usize;
    for argument in args {
        if let Some(name) = argument.name.as_deref() {
            let mut index = None;
            for (candidate_index, candidate) in expected_names.iter().enumerate() {
                if *candidate == name {
                    index = Some(candidate_index);
                    break;
                }
            }
            let Some(index) = index else {
                return Err(format!(
                    "direct backend does not recognize builtin argument `{}`",
                    name
                ));
            };
            if values[index].is_some() {
                return Err(format!(
                    "direct backend received duplicate builtin argument `{}`",
                    name
                ));
            }
            values[index] = Some(argument);
            continue;
        }
        while next_positional < values.len() && values[next_positional].is_some() {
            next_positional += 1;
        }
        if next_positional >= values.len() {
            return Err("direct backend received too many builtin arguments".to_string());
        }
        values[next_positional] = Some(argument);
        next_positional += 1;
    }
    let mut ordered = Vec::with_capacity(values.len());
    for value in values {
        let Some(value) = value else {
            return Err("direct backend is missing a builtin argument".to_string());
        };
        ordered.push(value);
    }
    Ok(ordered)
}

fn ordered_optional_named_args<'a>(
    expected_names: &[&str],
    args: &'a [MirArg],
) -> std::result::Result<Vec<Option<&'a MirArg>>, String> {
    let mut values = vec![None; expected_names.len()];
    let mut next_positional = 0usize;
    for argument in args {
        if let Some(name) = argument.name.as_deref() {
            let mut index = None;
            for (candidate_index, candidate) in expected_names.iter().enumerate() {
                if *candidate == name {
                    index = Some(candidate_index);
                    break;
                }
            }
            let Some(index) = index else {
                return Err(format!(
                    "direct backend does not recognize builtin argument `{}`",
                    name
                ));
            };
            if values[index].is_some() {
                return Err(format!(
                    "direct backend received duplicate builtin argument `{}`",
                    name
                ));
            }
            values[index] = Some(argument);
            continue;
        }
        while next_positional < values.len() && values[next_positional].is_some() {
            next_positional += 1;
        }
        if next_positional >= values.len() {
            return Err("direct backend received too many builtin arguments".to_string());
        }
        values[next_positional] = Some(argument);
        next_positional += 1;
    }
    Ok(values)
}

fn required_named_arg<'a>(
    argument: Option<&'a MirArg>,
    message: &str,
) -> std::result::Result<&'a MirArg, String> {
    argument.ok_or(message.to_string())
}

fn direct_internal_slice_args(args: &[MirArg]) -> std::result::Result<&[MirArg; 6], String> {
    args.try_into()
        .map_err(|_| DIRECT_INTERNAL_SLICE_ARITY_ERROR.to_string())
}

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
    fn new(
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
        let trait_impls = module.trait_impls.clone();

        let flags = native_codegen_flags()?;
        let isa_builder =
            try_or_string_error!(cranelift_native::builder(), "failed to detect host ISA: {}");
        let isa = try_or_string_error!(isa_builder.finish(flags), "failed to build host ISA: {}");
        let call_conv = isa.default_call_conv();
        let mut builder = try_or_string_error!(
            ObjectBuilder::new(isa, "aura_direct".to_string(), default_libcall_names()),
            "failed to initialize object builder: {}"
        );
        // One section per function lets ELF linkers drop unused generated
        // functions with `--gc-sections`; Mach-O already strips per symbol.
        builder.per_function_section(true);
        let mut object = ObjectModule::new(builder);

        let runtime =
            RuntimeTable::try_build(|function| declare_runtime_helper(&mut object, function))?;

        let mut functions = HashMap::new();
        let mut function_thunks = HashMap::new();
        let mut function_default_binders = HashMap::new();
        let mut cleanup_thunks = HashMap::new();
        let mut function_return_types = HashMap::new();
        let mut function_param_types = HashMap::new();
        let mut function_param_passing = HashMap::new();
        let mut function_writeback_types = HashMap::new();
        for function in module.functions.iter().chain(module.top_level.iter()) {
            let function_reachable = reachable_blocks.get(&function.name).ok_or_else(|| {
                format!(
                    "direct backend is missing reachable-block metadata for `{}`",
                    function.name
                )
            })?;
            let signature = signature_for(function, &classes, call_conv)?;
            let func_id = try_or_string_error!(
                object.declare_function(&mangle_symbol(&function.name), Linkage::Local, &signature),
                "failed to declare function `{}`: {}",
                function.name
            );
            functions.insert(function.name.clone(), func_id);
            let thunk_signature = thunk_signature(call_conv);
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
            let binder_signature = default_binder_signature(call_conv);
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
            function_param_passing.insert(
                function.name.clone(),
                function
                    .receiver
                    .iter()
                    .copied()
                    .chain(function.params.iter().map(|param| param.passing))
                    .collect::<Vec<_>>(),
            );
            function_writeback_types.insert(function.name.clone(), writebacks);
        }

        let callable_shapes = callables::declare_callable_shapes(
            module,
            &mut object,
            &function_param_types,
            &classes,
            call_conv,
        )?;
        Ok(Self {
            module,
            reachable_blocks,
            safepoints_enabled,
            program_path: program_path.to_string(),
            program_source: program_source.to_string(),
            object,
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
            trait_impls,
            function_return_types,
            function_param_types,
            function_param_passing,
            function_writeback_types,
            call_conv,
            runtime,
            string_data: HashMap::new(),
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
        self.define_main_wrapper()?;
        let product = self.object.finish();
        match product.emit() {
            Ok(bytes) => Ok(bytes),
            Err(error) => Err(format!("failed to emit direct backend object: {}", error)),
        }
    }

    fn define_function(&mut self, function: &MirFunction) -> std::result::Result<(), String> {
        let func_id = self.functions[&function.name];
        let mut ctx = self.object.make_context();
        ctx.func.signature = signature_for(function, &self.classes, self.call_conv)?;
        ctx.func.name = UserFuncName::user(0, func_id.as_u32());

        let mut builder_ctx = FunctionBuilderContext::new();
        let mut builder = FunctionBuilder::new(&mut ctx.func, &mut builder_ctx);

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
        let runtime_refs = self
            .runtime
            .map(|id| self.object.declare_func_in_func(*id, builder.func));
        let public_function_name = public_direct_function_name(&function.name);
        let function_path = function
            .source_path
            .as_deref()
            .unwrap_or(&self.program_path);
        let descriptor = declare_frame_descriptor(
            &mut self.object,
            &mut self.string_data,
            &format!("aura_frame_{}", func_id.as_u32()),
            function.span,
            function_path.as_bytes(),
            public_function_name.as_bytes(),
        )?;
        let descriptor = self.object.declare_data_in_func(descriptor, builder.func);
        let descriptor = builder.ins().symbol_value(types::I64, descriptor);
        builder
            .ins()
            .call(runtime_refs[RuntimeFn::EnterCall], &[descriptor]);

        let mut variable_index = 0usize;
        let mut variables = HashMap::new();
        let mut variable_types = HashMap::new();
        let entry_values = builder.block_params(entry).to_vec();
        let mut entry_index = 0usize;

        if function.receiver.is_some() {
            let receiver_ty = receiver_type(function, &self.classes)?;
            let end = entry_index + receiver_ty.value_count();
            declare_root_variables(
                &mut builder,
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
                &mut builder,
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
                &mut builder,
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
        for block in &function.blocks {
            if !reachable_blocks.contains(&block.label) {
                continue;
            }
            for instruction in &block.instructions {
                let Instruction::PushCleanup { place, action } = instruction else {
                    continue;
                };
                if !cleanup_places.contains(place) {
                    cleanup_places.push(place.clone());
                }
                cleanup_actions.insert(place.clone(), action.clone());
            }
        }
        let mut cleanup_active_vars = HashMap::new();
        let mut cleanup_registration_vars = HashMap::new();
        for place in &cleanup_places {
            let variable = Variable::from_u32(variable_index as u32);
            variable_index += 1;
            builder.declare_var(variable, types::I64);
            let zero = builder.ins().iconst(types::I64, 0);
            builder.def_var(variable, zero);
            cleanup_active_vars.insert(place.clone(), variable);

            let registration_variable = Variable::from_u32(variable_index as u32);
            variable_index += 1;
            builder.declare_var(registration_variable, types::I64);
            let zero = builder.ins().iconst(types::I64, 0);
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
            let variable = Variable::from_u32(variable_index as u32);
            variable_index += 1;
            builder.declare_var(variable, types::I64);
            let initial = builder
                .ins()
                .iconst(types::I64, NATIVE_LOOP_SAFEPOINT_INTERVAL as i64);
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
            let variable = Variable::from_u32(variable_index as u32);
            variable_index += 1;
            builder.declare_var(variable, types::I64);
            let zero = builder.ins().iconst(types::I64, 0);
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
            let variable = Variable::from_u32(variable_index as u32);
            variable_index += 1;
            builder.declare_var(variable, types::I64);
            let unset = builder.ins().iconst(types::I64, -1);
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
            let func_ref = self.object.declare_func_in_func(*func_id, builder.func);
            function_refs.insert(name.clone(), func_ref);
        }
        let mut function_thunk_refs = HashMap::new();
        let mut function_thunks = self.function_thunks.iter().collect::<Vec<_>>();
        function_thunks.sort_by(|left, right| left.0.cmp(right.0));
        for (name, func_id) in function_thunks {
            let func_ref = self.object.declare_func_in_func(*func_id, builder.func);
            function_thunk_refs.insert(name.clone(), func_ref);
        }
        let mut cleanup_thunk_refs = HashMap::new();
        for place in &cleanup_places {
            if let Some(func_id) = self
                .cleanup_thunks
                .get(&(function.name.clone(), place.clone()))
            {
                let func_ref = self.object.declare_func_in_func(*func_id, builder.func);
                cleanup_thunk_refs.insert(place.clone(), func_ref);
            }
        }

        let current_function_name = public_direct_function_name(&function.name);
        let current_function_path = function
            .source_path
            .clone()
            .unwrap_or_else(|| self.program_path.clone());

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
            function_param_passing: self.function_param_passing.clone(),
            function_writeback_types: self.function_writeback_types.clone(),
            current_function_name,
            current_function_path,
            writeback_locals,
            mutable_param_indices,
            classes: self.classes.clone(),
            enums: self.enums.clone(),
            union_layouts: self.union_layouts.clone(),
            trait_impls: self.trait_impls.clone(),
            return_type: function.return_type.clone(),
            owned_opaque_temporaries: OwnedTemporaries::default(),
            last_runtime_type_tag: None,
            proven_access: ProvenAccess::None,
            view_places: HashMap::new(),
            view_element_keys: HashMap::new(),
            view_selector_vars,
            view_selector_tags,
            closure_selector_vars,
            closure_selector_tags,
            closure_capture_writebacks: HashMap::new(),
            object: &mut self.object,
            host_builtin_refs: HashMap::new(),
            string_data: &mut self.string_data,
            callable_shapes: &self.callable_shapes,
            contract_descriptors: &mut self.contract_descriptors,
            call_conv: self.call_conv,
            cleanup_places,
            cleanup_actions,
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
        compiler.builder.finalize();
        try_or_string_error!(
            ctx.verify(self.object.isa()),
            "failed to define direct function `{}`: {}\n{}",
            function.name,
            ctx.func.display()
        );
        try_or_string_error!(
            self.object.define_function(func_id, &mut ctx),
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
        let mut ctx = self.object.make_context();
        ctx.func.signature = thunk_signature(self.call_conv);
        ctx.func.name = UserFuncName::user(0, thunk_id.as_u32());

        let mut builder_ctx = FunctionBuilderContext::new();
        let mut builder = FunctionBuilder::new(&mut ctx.func, &mut builder_ctx);
        let entry = builder.create_block();
        builder.append_block_params_for_function_params(entry);
        builder.switch_to_block(entry);
        builder.seal_block(entry);

        let args_ptr = builder.block_params(entry)[0];
        let target_ref = self.object.declare_func_in_func(target_id, builder.func);
        let unbox_i64 = self
            .object
            .declare_func_in_func(self.runtime[RuntimeFn::UnboxI64], builder.func);
        let unbox_int64 = self
            .object
            .declare_func_in_func(self.runtime[RuntimeFn::UnboxInt64], builder.func);
        let unbox_u64 = self
            .object
            .declare_func_in_func(self.runtime[RuntimeFn::UnboxU64], builder.func);
        let unbox_f64 = self
            .object
            .declare_func_in_func(self.runtime[RuntimeFn::UnboxF64], builder.func);
        let unbox_bool = self
            .object
            .declare_func_in_func(self.runtime[RuntimeFn::UnboxBool], builder.func);
        let release_value = self
            .object
            .declare_func_in_func(self.runtime[RuntimeFn::ReleaseValue], builder.func);

        if matches!(function.name.as_str(), "main" | "__script") {
            let module_constant = self
                .object
                .declare_func_in_func(self.runtime[RuntimeFn::ModuleConstant], builder.func);
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
                let initializer = self.object.declare_func_in_func(*initializer, builder.func);
                let initializer = builder.ins().func_addr(types::I64, initializer);
                let (key_ptr, key_len) = declare_string_constant(
                    &mut self.object,
                    &mut self.string_data,
                    &mut builder,
                    constant.key.as_bytes(),
                )?;
                let call = builder
                    .ins()
                    .call(module_constant, &[key_ptr, key_len, initializer]);
                let value = builder.inst_results(call)[0];
                builder.ins().call(release_value, &[value]);
            }
        }

        let mut lowered_args = Vec::new();
        let param_types = self.function_param_types[&function.name].clone();
        for (index, param_ty) in param_types.iter().enumerate() {
            let raw = builder
                .ins()
                .load(types::I64, MemFlags::new(), args_ptr, (index as i32) * 8);
            // The uniform function-value buffer is an in/out ownership
            // channel. Clear each incoming slot before transferring its
            // retained handle into the concrete function ABI; mutable
            // parameter writebacks are installed into their original slots
            // after the call.
            let zero = builder.ins().iconst(types::I64, 0);
            builder
                .ins()
                .store(MemFlags::new(), zero, args_ptr, (index as i32) * 8);
            match param_ty {
                DirectType::Callable(_) => {
                    // The boxed handle is adopted into the callable's words.
                    lowered_args.extend(unbox_thunk_value(self, &mut builder, raw, param_ty)?);
                }
                DirectType::Opaque(_) => lowered_args.push(raw),
                DirectType::Scalar(ScalarKind::Int32) => {
                    let inst = builder.ins().call(unbox_i64, &[raw]);
                    lowered_args.push(builder.inst_results(inst)[0]);
                    let _ = builder.ins().call(release_value, &[raw]);
                }
                DirectType::Scalar(ScalarKind::Int64) => {
                    let inst = builder.ins().call(unbox_int64, &[raw]);
                    lowered_args.push(builder.inst_results(inst)[0]);
                    let _ = builder.ins().call(release_value, &[raw]);
                }
                DirectType::Scalar(ScalarKind::Uint64) => {
                    let inst = builder.ins().call(unbox_u64, &[raw]);
                    lowered_args.push(builder.inst_results(inst)[0]);
                    let _ = builder.ins().call(release_value, &[raw]);
                }
                DirectType::Scalar(ScalarKind::Float32)
                | DirectType::Scalar(ScalarKind::Float64) => {
                    let inst = builder.ins().call(unbox_f64, &[raw]);
                    lowered_args.push(builder.inst_results(inst)[0]);
                    let _ = builder.ins().call(release_value, &[raw]);
                }
                DirectType::Scalar(ScalarKind::Bool) => {
                    let inst = builder.ins().call(unbox_bool, &[raw]);
                    lowered_args.push(builder.inst_results(inst)[0]);
                    let _ = builder.ins().call(release_value, &[raw]);
                }
                DirectType::Scalar(ScalarKind::Unit) => {
                    lowered_args.push(builder.ins().iconst(types::I64, 0));
                    let _ = builder.ins().call(release_value, &[raw]);
                }
                DirectType::PlainClass(_) | DirectType::Union(_) => {
                    lowered_args.extend(unbox_thunk_value(self, &mut builder, raw, param_ty)?);
                    let _ = builder.ins().call(release_value, &[raw]);
                }
            }
        }

        let inst = builder.ins().call(target_ref, &lowered_args);
        let results = builder.inst_results(inst).to_vec();
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
                &mut builder,
                &results[cursor..cursor + writeback_count],
                writeback_ty,
            )?;
            builder.ins().store(
                MemFlags::new(),
                boxed_writeback,
                args_ptr,
                (index as i32) * 8,
            );
            cursor += writeback_count;
        }
        let boxed = callables::box_owned_thunk_value(
            self,
            &mut builder,
            &results[..return_count],
            &return_ty,
        )?;
        builder.ins().return_(&[boxed]);
        builder.finalize();

        try_or_string_error!(
            self.object.define_function(thunk_id, &mut ctx),
            "failed to define direct function thunk `{}`: {}",
            function.name
        );
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
        let mut ctx = self.object.make_context();
        ctx.func.signature = default_binder_signature(self.call_conv);
        ctx.func.name = UserFuncName::user(0, binder_id.as_u32());

        let mut builder_ctx = FunctionBuilderContext::new();
        let mut builder = FunctionBuilder::new(&mut ctx.func, &mut builder_ctx);
        let entry = builder.create_block();
        builder.append_block_params_for_function_params(entry);
        builder.switch_to_block(entry);
        builder.seal_block(entry);
        let args_ptr = builder.block_params(entry)[0];
        let transfer_defaults = builder.block_params(entry)[2];
        let store_owned = self
            .object
            .declare_func_in_func(self.runtime[RuntimeFn::ArgBufferStoreOwned], builder.func);
        let param_types = self
            .function_param_types
            .get(&function.name)
            .cloned()
            .unwrap_or_default();

        for (index, param) in function.params.iter().enumerate() {
            let Some(default_name) = param.default_function.as_ref() else {
                continue;
            };
            let raw = builder
                .ins()
                .load(types::I64, MemFlags::new(), args_ptr, (index as i32) * 8);
            let missing = builder.ins().icmp_imm(IntCC::Equal, raw, 0);
            let default_block = builder.create_block();
            let next_block = builder.create_block();
            builder
                .ins()
                .brif(missing, default_block, &[], next_block, &[]);
            builder.switch_to_block(default_block);
            let default_id = *self.functions.get(default_name).ok_or_else(|| {
                format!(
                    "direct backend is missing default function `{default_name}` for `{}`",
                    function.name
                )
            })?;
            let default_ref = self.object.declare_func_in_func(default_id, builder.func);
            let call = builder.ins().call(default_ref, &[]);
            let results = builder.inst_results(call).to_vec();
            let param_ty = param_types.get(index).ok_or_else(|| {
                format!(
                    "direct backend is missing parameter {} metadata for `{}`",
                    index + 1,
                    function.name
                )
            })?;
            let boxed = box_thunk_value(self, &mut builder, &results, param_ty)?;
            let transfer = builder
                .ins()
                .icmp_imm(IntCC::NotEqual, transfer_defaults, 0);
            let transfer_block = builder.create_block();
            let retain_block = builder.create_block();
            builder
                .ins()
                .brif(transfer, transfer_block, &[], retain_block, &[]);
            builder.switch_to_block(transfer_block);
            let index_value = builder.ins().iconst(types::I64, index as i64);
            builder
                .ins()
                .call(store_owned, &[args_ptr, index_value, boxed]);
            builder.ins().jump(next_block, &[]);
            builder.seal_block(transfer_block);
            builder.switch_to_block(retain_block);
            builder
                .ins()
                .store(MemFlags::new(), boxed, args_ptr, (index as i32) * 8);
            builder.ins().jump(next_block, &[]);
            builder.seal_block(retain_block);
            builder.seal_block(default_block);
            builder.switch_to_block(next_block);
            builder.seal_block(next_block);
        }
        builder.ins().return_(&[]);
        builder.finalize();
        try_or_string_error!(
            self.object.define_function(binder_id, &mut ctx),
            "failed to define direct default binder `{}`: {}",
            function.name
        );
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
        let place_ty = cleanup_place_type(function, &self.classes, place)?;

        let action = cleanup_action_for_place(function, place);
        let exit_method = cleanup_exit_method(action.as_ref());

        let mut ctx = self.object.make_context();
        ctx.func.signature = thunk_signature(self.call_conv);
        ctx.func.name = UserFuncName::user(0, thunk_id.as_u32());

        let mut builder_ctx = FunctionBuilderContext::new();
        let mut builder = FunctionBuilder::new(&mut ctx.func, &mut builder_ctx);
        let entry = builder.create_block();
        builder.append_block_params_for_function_params(entry);
        builder.switch_to_block(entry);
        builder.seal_block(entry);

        let args_ptr = builder.block_params(entry)[0];
        let raw = builder.ins().load(types::I64, MemFlags::new(), args_ptr, 0);
        match &place_ty {
            DirectType::Callable(_) => {}
            DirectType::Union(_) => {}
            DirectType::PlainClass(class_ty) => {
                let close_method = self
                    .classes
                    .get(&class_ty.class_name)
                    .and_then(|class| {
                        class
                            .methods
                            .iter()
                            .find(|method| method.name == exit_method)
                    })
                    .cloned();
                if let Some(method) = close_method {
                    let target_id = *self.functions.get(&method.function_name).ok_or({
                        format!(
                            "direct backend could not find cleanup close method `{}`",
                            method.function_name
                        )
                    })?;
                    let target_ref = self.object.declare_func_in_func(target_id, builder.func);
                    let lowered = unbox_thunk_value(self, &mut builder, raw, &place_ty)?;
                    let inst = builder.ins().call(target_ref, &lowered);
                    let results = builder.inst_results(inst).to_vec();
                    release_cleanup_thunk_results(
                        self,
                        &mut builder,
                        action.as_ref(),
                        &class_ty.class_name,
                        &method.function_name,
                        &results,
                    )?;
                }
                let zero = builder.ins().iconst(types::I64, 0);
                let unit = box_thunk_value(
                    self,
                    &mut builder,
                    &[zero],
                    &DirectType::Scalar(ScalarKind::Unit),
                )?;
                builder.ins().return_(&[unit]);
            }
            DirectType::Opaque(ty) => {
                let close_method = match ty {
                    Type::Named(class_name, _) => self
                        .classes
                        .get(class_name)
                        .and_then(|class| {
                            class
                                .methods
                                .iter()
                                .find(|method| method.name == exit_method)
                        })
                        .cloned(),
                    _ => None,
                };
                if let Some(method) = close_method {
                    let target_id = *self.functions.get(&method.function_name).ok_or({
                        format!(
                            "direct backend could not find cleanup close method `{}`",
                            method.function_name
                        )
                    })?;
                    let target_ref = self.object.declare_func_in_func(target_id, builder.func);
                    let retain_value = self
                        .object
                        .declare_func_in_func(self.runtime[RuntimeFn::RetainValue], builder.func);
                    let retained = builder.ins().call(retain_value, &[raw]);
                    let retained = builder.inst_results(retained)[0];
                    let inst = builder.ins().call(target_ref, &[retained]);
                    let results = builder.inst_results(inst).to_vec();
                    let class_name = match ty {
                        Type::Named(class_name, _) => class_name.clone(),
                        _ => String::new(),
                    };
                    release_cleanup_thunk_results(
                        self,
                        &mut builder,
                        action.as_ref(),
                        &class_name,
                        &method.function_name,
                        &results,
                    )?;
                    let zero = builder.ins().iconst(types::I64, 0);
                    let unit = box_thunk_value(
                        self,
                        &mut builder,
                        &[zero],
                        &DirectType::Scalar(ScalarKind::Unit),
                    )?;
                    builder.ins().return_(&[unit]);
                } else {
                    // A resource whose static type is known closes through its
                    // own entry, so the program links only that close path.
                    let typed_close = match ty {
                        Type::Named(name, _) => crate::native_runtime::typed_close_symbol(name),
                        _ => None,
                    };
                    let close_id = match typed_close {
                        Some(symbol) => declare_runtime_function(
                            &mut self.object,
                            symbol,
                            &[types::I64, types::I64],
                            Some(types::I64),
                        )?,
                        None => self.runtime[RuntimeFn::CloseValue],
                    };
                    let close_value = self.object.declare_func_in_func(close_id, builder.func);
                    let cancel_before = builder.ins().iconst(types::I64, 1);
                    let inst = builder.ins().call(close_value, &[raw, cancel_before]);
                    let result = builder.inst_results(inst)[0];
                    builder.ins().return_(&[result]);
                }
            }
            DirectType::Scalar(_) => {
                let zero = builder.ins().iconst(types::I64, 0);
                let unit = box_thunk_value(
                    self,
                    &mut builder,
                    &[zero],
                    &DirectType::Scalar(ScalarKind::Unit),
                )?;
                builder.ins().return_(&[unit]);
            }
        }
        builder.finalize();

        try_or_string_error!(
            self.object.define_function(thunk_id, &mut ctx),
            "failed to define cleanup thunk for `{}` in `{}`: {}",
            place,
            function.name
        );
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

        let mut ctx = self.object.make_context();
        ctx.func.signature = main_signature(self.call_conv);
        let wrapper_id = try_or_string_error!(
            self.object
                .declare_function("main", Linkage::Export, &ctx.func.signature),
            "failed to declare main wrapper: {}"
        );
        ctx.func.name = UserFuncName::user(0, wrapper_id.as_u32());

        let mut builder_ctx = FunctionBuilderContext::new();
        let mut builder = FunctionBuilder::new(&mut ctx.func, &mut builder_ctx);
        let entry_block = builder.create_block();
        builder.switch_to_block(entry_block);
        builder.seal_block(entry_block);

        let runtime_init = self
            .object
            .declare_func_in_func(self.runtime[RuntimeFn::RuntimeInit], builder.func);
        let run_root = self
            .object
            .declare_func_in_func(self.runtime[RuntimeFn::RunRoot], builder.func);
        let entry_thunk_ref = self
            .object
            .declare_func_in_func(entry_thunk_id, builder.func);
        let (path_ptr, path_len) = declare_string_constant(
            &mut self.object,
            &mut self.string_data,
            &mut builder,
            self.program_path.as_bytes(),
        )?;
        let (source_ptr, source_len) = declare_string_constant(
            &mut self.object,
            &mut self.string_data,
            &mut builder,
            self.program_source.as_bytes(),
        )?;
        builder
            .ins()
            .call(runtime_init, &[path_ptr, path_len, source_ptr, source_len]);
        // Generated code reads list elements at fixed offsets, so a runtime
        // built with another layout stops here instead of misreading them.
        let check_list_layout = self
            .object
            .declare_func_in_func(self.runtime[RuntimeFn::CheckListLayout], builder.func);
        let fingerprint = builder.ins().iconst(
            types::I64,
            aura_runtime::native_runtime::direct_list_layout().fingerprint(),
        );
        builder.ins().call(check_list_layout, &[fingerprint]);
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
            let register_display = self
                .object
                .declare_func_in_func(self.runtime[RuntimeFn::RegisterDisplay], builder.func);
            for (type_name, function_name) in display_wrappers {
                let thunk_id = self.function_thunks[&function_name];
                let thunk_ref = self.object.declare_func_in_func(thunk_id, builder.func);
                let thunk_addr = builder.ins().func_addr(types::I64, thunk_ref);
                let (name_ptr, name_len) = declare_string_constant(
                    &mut self.object,
                    &mut self.string_data,
                    &mut builder,
                    type_name.as_bytes(),
                )?;
                builder
                    .ins()
                    .call(register_display, &[name_ptr, name_len, thunk_addr]);
            }
        }
        let thunk_ptr = builder.ins().func_addr(types::I64, entry_thunk_ref);
        let result = builder.ins().call(run_root, &[thunk_ptr]);
        let return_code = builder.inst_results(result)[0];
        builder.ins().return_(&[return_code]);
        builder.finalize();

        try_or_string_error!(
            self.object.define_function(wrapper_id, &mut ctx),
            "failed to define main wrapper: {}"
        );
        Ok(())
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct DirectViewPlace {
    alternatives: Vec<DirectViewAlternative>,
}

/// A call argument moved out of its owner for one call instead of cloned
/// (see `FunctionCompiler::lendable_arguments`).
struct LentArgument {
    place: String,
    /// Shared arguments are moved back after the call; `mut` arguments come
    /// back through the call's writeback.
    restore: bool,
}

/// A list element a loan lends, read in place by a runtime helper (see
/// `FunctionCompiler::borrowed_list_element`).
struct BorrowedListElement {
    /// The local holding the list.
    list: String,
    /// The variable holding the element's position.
    index: Variable,
    element_type: Type,
    /// The index expression a bounds trap names.
    span: Span,
}

/// A member-call receiver moved out of its owner for one call instead of
/// cloned (see `FunctionCompiler::lendable_receiver`).
struct LentReceiver {
    place: String,
    alternative: DirectViewAlternative,
    kind: LentReceiverKind,
}

enum LentReceiverKind {
    /// The call only borrows the receiver: it is moved back afterwards.
    Shared,
    /// A `mut self` method: its writeback stores the receiver back.
    MutableWriteback,
}

/// How the emitter passes one argument of a handle runtime operation.
#[derive(Clone, Copy)]
enum RuntimeOpArgLayout {
    /// One boxed value word.
    Boxed,
    /// One boxed value word, stored back to the argument's writeback place
    /// after the call.
    BoxedWriteback,
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
        _ => RuntimeOpArgLayout::Boxed,
    }
}

/// One lowered argument of a handle runtime operation.
enum RuntimeOpArg {
    Boxed(ValueRef),
    Word(Value),
}

/// What a member call on a receiver runs: a member named in MIR, or a
/// runtime operation lowering resolved.
#[derive(Clone, Copy)]
enum MemberCallee<'a> {
    Field(&'a str),
    Runtime(RuntimeOp),
}

impl<'a> MemberCallee<'a> {
    /// The source member name.
    fn field(self) -> &'a str {
        match self {
            MemberCallee::Field(field) => field,
            MemberCallee::Runtime(op) => op.name(),
        }
    }
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
    conditions: Vec<(Variable, i64)>,
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
    path_ptr: Value,
    path_len: Value,
    selectors_ptr: Value,
    count: Value,
    line: Value,
    column: Value,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum DirectElementKind {
    List,
    Dict,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct DirectElementSelector {
    variable: Variable,
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

    fn conditioned(mut self, selector: Variable, expected: i64) -> Self {
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
    builder: FunctionBuilder<'a>,
    blocks: HashMap<String, cranelift_codegen::ir::Block>,
    variables: HashMap<String, Vec<Variable>>,
    variable_types: HashMap<String, DirectType>,
    next_variable_index: usize,
    function_refs: HashMap<String, cranelift_codegen::ir::FuncRef>,
    function_thunk_refs: HashMap<String, cranelift_codegen::ir::FuncRef>,
    cleanup_thunk_refs: HashMap<String, cranelift_codegen::ir::FuncRef>,
    function_return_types: HashMap<String, DirectType>,
    function_param_types: HashMap<String, Vec<DirectType>>,
    /// Each parameter's passing mode, the receiver first, parallel to
    /// `function_param_types`.
    function_param_passing: HashMap<String, Vec<MirReceiverKind>>,
    function_writeback_types: HashMap<String, Vec<DirectType>>,
    current_function_name: String,
    current_function_path: String,
    writeback_locals: Vec<(String, DirectType)>,
    mutable_param_indices: HashMap<String, usize>,
    classes: HashMap<String, MirClass>,
    enums: HashMap<String, MirEnum>,
    union_layouts: HashMap<String, crate::union_layout::MirUnionLayout>,
    trait_impls: Vec<MirTraitImpl>,
    return_type: Type,
    owned_opaque_temporaries: OwnedTemporaries,
    /// The latest `tag_value_type` call: its instruction, value, and encoded
    /// type, so an identical tag that no other call follows is not repeated.
    last_runtime_type_tag: Option<(cranelift_codegen::ir::Inst, Value, String)>,
    /// What an `IndexInRange` fact proves about the instruction being
    /// compiled.
    proven_access: ProvenAccess,
    view_places: HashMap<String, DirectViewPlace>,
    view_selector_vars: HashMap<String, Variable>,
    /// Owned dictionary keys held by entry loans, released at `EndLoan`.
    view_element_keys: HashMap<String, Vec<Variable>>,
    view_selector_tags: HashMap<String, HashMap<String, i64>>,
    closure_selector_vars: HashMap<String, Variable>,
    closure_selector_tags: HashMap<(String, usize), i64>,
    closure_capture_writebacks: HashMap<String, Vec<DirectClosureCaptureWriteback>>,
    object: &'a mut ObjectModule,
    /// Host builtin entries this function calls, declared on first use so the
    /// object imports only the builtins the program calls.
    host_builtin_refs: HashMap<&'static str, cranelift_codegen::ir::FuncRef>,
    string_data: &'a mut HashMap<Vec<u8>, DataId>,
    callable_shapes: &'a HashMap<CallableShapeKey, CallableShape>,
    contract_descriptors: &'a mut HashMap<String, callables::ContractDescriptor>,
    call_conv: CallConv,
    cleanup_places: Vec<String>,
    /// The typed exit action each cleanup place registers.
    cleanup_actions: HashMap<String, CleanupAction>,
    cleanup_active_vars: HashMap<String, Variable>,
    cleanup_registration_vars: HashMap<String, Variable>,
    safepoint_fuel: Option<Variable>,
    runtime: RuntimeTable<cranelift_codegen::ir::FuncRef>,
}

impl<'a> FunctionCompiler<'a> {
    fn compiled_block(
        &self,
        label: &str,
    ) -> std::result::Result<cranelift_codegen::ir::Block, String> {
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
        let mut snapshots = Vec::<(Variable, Variable)>::new();
        for alternative in &mut place.alternatives {
            for (selector, _) in &mut alternative.conditions {
                let snapshot = if let Some((_, snapshot)) =
                    snapshots.iter().find(|(original, _)| original == selector)
                {
                    *snapshot
                } else {
                    let snapshot = Variable::from_u32(self.next_variable_index as u32);
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

    fn local_vars(&self, name: &str) -> std::result::Result<Vec<Variable>, String> {
        self.variables
            .get(name)
            .cloned()
            .ok_or(format!("direct backend does not know local `{}`", name))
    }

    fn is_opaque_value(&self, value: &ValueRef) -> bool {
        matches!(value.ty, DirectType::Opaque(_))
    }

    fn temporary_owns_opaque(&self, value: &ValueRef) -> bool {
        self.is_opaque_value(value) && self.owned_opaque_temporaries.contains(&value.values[0])
    }

    fn mark_temporary_opaque_owned(&mut self, value: &ValueRef) {
        if self.is_opaque_value(value) {
            self.owned_opaque_temporaries.insert(value.values[0]);
        }
    }

    fn clear_temporary_opaque_owned(&mut self, value: &ValueRef) {
        if self.is_opaque_value(value) {
            self.owned_opaque_temporaries.remove(&value.values[0]);
        }
    }

    fn retain_opaque_handle(&mut self, value: Value) -> Value {
        let inst = self
            .builder
            .ins()
            .call(self.runtime[RuntimeFn::RetainValue], &[value]);
        self.builder.inst_results(inst)[0]
    }

    fn release_opaque_handle(&mut self, value: Value) {
        let _ = self
            .builder
            .ins()
            .call(self.runtime[RuntimeFn::ReleaseValue], &[value]);
    }

    fn release_all_temporary_owned(&mut self) {
        let owned = self
            .owned_opaque_temporaries
            .opaque
            .iter()
            .copied()
            .collect::<Vec<_>>();
        for value in owned {
            self.release_opaque_handle(value);
        }
        let unions = std::mem::take(&mut self.owned_opaque_temporaries.unions);
        for (words, union) in unions.into_values() {
            self.release_union_words(&union, &words);
        }
        let callables = std::mem::take(&mut self.owned_opaque_temporaries.callables);
        for words in callables.into_values() {
            self.release_callable_words(&words);
        }
        self.owned_opaque_temporaries.clear();
    }

    fn release_temporary_owned_since(&mut self, baseline: &OwnedTemporaries) {
        let created = self
            .owned_opaque_temporaries
            .opaque
            .difference(&baseline.opaque)
            .copied()
            .collect::<Vec<_>>();
        for value in created {
            self.release_opaque_handle(value);
            self.owned_opaque_temporaries.remove(&value);
        }
        let created = self
            .owned_opaque_temporaries
            .unions
            .keys()
            .filter(|tag| !baseline.unions.contains_key(tag))
            .copied()
            .collect::<Vec<_>>();
        for tag in created {
            if let Some((words, union)) = self.owned_opaque_temporaries.unions.remove(&tag) {
                self.release_union_words(&union, &words);
            }
        }
        let created = self
            .owned_opaque_temporaries
            .callables
            .keys()
            .filter(|descriptor| !baseline.callables.contains_key(descriptor))
            .copied()
            .collect::<Vec<_>>();
        for descriptor in created {
            if let Some(words) = self.owned_opaque_temporaries.callables.remove(&descriptor) {
                self.release_callable_words(&words);
            }
        }
    }

    fn temporary_owns_union(&self, value: &ValueRef) -> bool {
        matches!(value.ty, DirectType::Union(_))
            && self
                .owned_opaque_temporaries
                .unions
                .contains_key(&value.values[0])
    }

    fn mark_temporary_union_owned(&mut self, value: &ValueRef) {
        if let DirectType::Union(union) = &value.ty {
            if union.owns_handles() {
                self.owned_opaque_temporaries
                    .unions
                    .insert(value.values[0], (value.values.clone(), union.clone()));
            }
        }
    }

    fn clear_temporary_union_owned(&mut self, value: &ValueRef) {
        self.owned_opaque_temporaries
            .unions
            .remove(&value.values[0]);
    }

    /// Releases the active member's handle of an inline union, if the active
    /// member owns one; a null handle (a moved-from local) is skipped by the
    /// runtime.
    fn release_union_words(&mut self, union: &DirectUnionType, words: &[Value]) {
        for &index in &union.owning {
            let active = self
                .builder
                .ins()
                .icmp_imm(IntCC::Equal, words[0], index as i64);
            let release_block = self.builder.create_block();
            let continue_block = self.builder.create_block();
            self.builder
                .ins()
                .brif(active, release_block, &[], continue_block, &[]);
            self.builder.switch_to_block(release_block);
            self.builder.seal_block(release_block);
            if matches!(union.member(index), Ok(DirectType::Callable(_))) {
                let callable_words = words[1..2 + CALLABLE_ENVIRONMENT_WORDS].to_vec();
                self.call_callable_adapter_on_words(&callable_words, callables::DESCRIPTOR_DROP);
            } else {
                self.release_opaque_handle(words[1]);
            }
            self.builder.ins().jump(continue_block, &[]);
            self.builder.switch_to_block(continue_block);
            self.builder.seal_block(continue_block);
        }
    }

    /// Retains the active member's handle of an inline union, if the active
    /// member owns one.
    fn retain_union_words(&mut self, union: &DirectUnionType, words: &[Value]) {
        for &index in &union.owning {
            let active = self
                .builder
                .ins()
                .icmp_imm(IntCC::Equal, words[0], index as i64);
            let retain_block = self.builder.create_block();
            let continue_block = self.builder.create_block();
            self.builder
                .ins()
                .brif(active, retain_block, &[], continue_block, &[]);
            self.builder.switch_to_block(retain_block);
            self.builder.seal_block(retain_block);
            if matches!(union.member(index), Ok(DirectType::Callable(_))) {
                let callable_words = words[1..2 + CALLABLE_ENVIRONMENT_WORDS].to_vec();
                self.call_callable_adapter_on_words(&callable_words, callables::DESCRIPTOR_RETAIN);
            } else {
                let _ = self.retain_opaque_handle(words[1]);
            }
            self.builder.ins().jump(continue_block, &[]);
            self.builder.switch_to_block(continue_block);
            self.builder.seal_block(continue_block);
        }
    }

    /// The words of an inline union handed to a new owner: an owned
    /// temporary moves, a borrowed value retains its active handle.
    fn transfer_union_words(&mut self, value: &ValueRef) -> Vec<Value> {
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
            && self
                .owned_opaque_temporaries
                .callables
                .contains_key(&value.values[0])
    }

    fn mark_temporary_callable_owned(&mut self, value: &ValueRef) {
        if matches!(value.ty, DirectType::Callable(_)) {
            self.owned_opaque_temporaries
                .callables
                .insert(value.values[0], value.values.clone());
        }
    }

    fn clear_temporary_callable_owned(&mut self, value: &ValueRef) {
        if matches!(value.ty, DirectType::Callable(_)) {
            self.owned_opaque_temporaries
                .callables
                .remove(&value.values[0]);
        }
    }

    /// Calls the `drop` or `retain` adapter named by an inline callable's
    /// descriptor on its four words; a zero descriptor (a moved-from or
    /// never-assigned value) names no shape and is skipped.
    fn call_callable_adapter_on_words(&mut self, words: &[Value], descriptor_offset: i32) {
        let live = self.builder.ins().icmp_imm(IntCC::NotEqual, words[0], 0);
        let call_block = self.builder.create_block();
        let continue_block = self.builder.create_block();
        self.builder
            .ins()
            .brif(live, call_block, &[], continue_block, &[]);
        self.builder.switch_to_block(call_block);
        self.builder.seal_block(call_block);
        self.call_callable_adapter_on_live_words(words, descriptor_offset);
        self.builder.ins().jump(continue_block, &[]);
        self.builder.switch_to_block(continue_block);
        self.builder.seal_block(continue_block);
    }

    fn call_callable_adapter_on_live_words(&mut self, words: &[Value], descriptor_offset: i32) {
        let slot = self.builder.create_sized_stack_slot(StackSlotData::new(
            StackSlotKind::ExplicitSlot,
            (8 * (1 + CALLABLE_ENVIRONMENT_WORDS)) as u32,
            3,
        ));
        let callable_ptr = self.builder.ins().stack_addr(types::I64, slot, 0);
        for (index, word) in words.iter().enumerate() {
            self.builder
                .ins()
                .store(MemFlags::new(), *word, callable_ptr, (index as i32) * 8);
        }
        let adapter =
            self.builder
                .ins()
                .load(types::I64, MemFlags::new(), words[0], descriptor_offset);
        let signature = self
            .builder
            .func
            .import_signature(callables::callable_unary_signature(self.call_conv));
        self.builder
            .ins()
            .call_indirect(signature, adapter, &[callable_ptr]);
    }

    /// Boxes an inline callable through its descriptor's `box` adapter and
    /// returns the owned handle; the inline value stays alive.
    fn call_callable_box_on_words(&mut self, words: &[Value]) -> Value {
        let slot = self.builder.create_sized_stack_slot(StackSlotData::new(
            StackSlotKind::ExplicitSlot,
            (8 * (1 + CALLABLE_ENVIRONMENT_WORDS)) as u32,
            3,
        ));
        let callable_ptr = self.builder.ins().stack_addr(types::I64, slot, 0);
        for (index, word) in words.iter().enumerate() {
            self.builder
                .ins()
                .store(MemFlags::new(), *word, callable_ptr, (index as i32) * 8);
        }
        let adapter = self.builder.ins().load(
            types::I64,
            MemFlags::new(),
            words[0],
            callables::DESCRIPTOR_BOX,
        );
        let signature = self
            .builder
            .func
            .import_signature(callables::callable_box_signature(self.call_conv));
        let call = self
            .builder
            .ins()
            .call_indirect(signature, adapter, &[callable_ptr]);
        self.builder.inst_results(call)[0]
    }

    /// The four words of a boxed function value re-entering as an inline
    /// callable of `signature`.
    fn boxed_callable_words(
        &mut self,
        handle: Value,
        signature: &Type,
    ) -> std::result::Result<Vec<Value>, String> {
        let contract = callables::declare_contract_descriptor(
            &mut *self.object,
            &mut *self.contract_descriptors,
            &self.classes,
            self.call_conv,
            signature,
        )?;
        let global = self
            .object
            .declare_data_in_func(contract.descriptor, self.builder.func);
        let descriptor = self.builder.ins().symbol_value(types::I64, global);
        let zero = self.builder.ins().iconst(types::I64, 0);
        Ok(vec![descriptor, handle, zero, zero])
    }

    /// Releases the captures an inline callable owns.
    fn release_callable_words(&mut self, words: &[Value]) {
        self.call_callable_adapter_on_words(words, callables::DESCRIPTOR_DROP);
    }

    fn retain_callable_words(&mut self, words: &[Value]) {
        self.call_callable_adapter_on_words(words, callables::DESCRIPTOR_RETAIN);
    }

    /// The words of an inline callable handed to a new owner: an owned
    /// temporary moves, a borrowed value retains its captures.
    fn transfer_callable_words(&mut self, value: &ValueRef) -> Vec<Value> {
        if self.temporary_owns_callable(value) {
            self.clear_temporary_callable_owned(value);
            return value.values.clone();
        }
        self.retain_callable_words(&value.values);
        value.values.clone()
    }

    /// Argument or field words for any direct value: opaque handles,
    /// owning unions, and inline callables transfer, everything else copies.
    fn transfer_values(&mut self, value: &ValueRef) -> Vec<Value> {
        match &value.ty {
            DirectType::Opaque(_) => vec![self.transfer_opaque_arg(value)],
            DirectType::Union(union) if union.owns_handles() => self.transfer_union_words(value),
            DirectType::Callable(_) => self.transfer_callable_words(value),
            _ => value.values.clone(),
        }
    }

    fn release_root_if_opaque(&mut self, name: &str) -> std::result::Result<(), String> {
        let ty = self.local_type(name)?;
        if let DirectType::Union(union) = &ty {
            if union.owns_handles() {
                let union = union.clone();
                let vars = self.local_vars(name)?;
                let words = vars
                    .iter()
                    .map(|var| self.builder.use_var(*var))
                    .collect::<Vec<_>>();
                self.release_union_words(&union, &words);
            }
            return Ok(());
        }
        if matches!(ty, DirectType::Callable(_)) {
            let vars = self.local_vars(name)?;
            let words = vars
                .iter()
                .map(|var| self.builder.use_var(*var))
                .collect::<Vec<_>>();
            self.release_callable_words(&words);
            return Ok(());
        }
        if !matches!(ty, DirectType::Opaque(_)) {
            return Ok(());
        }
        let vars = self.local_vars(name)?;
        let current = self.builder.use_var(vars[0]);
        self.release_opaque_handle(current);
        Ok(())
    }

    fn release_all_opaque_roots(&mut self) -> std::result::Result<(), String> {
        // Sorted, so the same program releases its roots in the same order
        // in every build.
        let mut names = self.variable_types.keys().cloned().collect::<Vec<_>>();
        names.sort();
        for name in names {
            self.release_root_if_opaque(&name)?;
        }
        Ok(())
    }

    fn transfer_opaque_arg(&mut self, value: &ValueRef) -> Value {
        if self.temporary_owns_opaque(value) {
            self.clear_temporary_opaque_owned(value);
            value.values[0]
        } else {
            self.retain_opaque_handle(value.values[0])
        }
    }

    fn transfer_owned_opaque_value(&mut self, value: &ValueRef) -> Value {
        if self.temporary_owns_opaque(value) {
            self.clear_temporary_opaque_owned(value);
            value.values[0]
        } else {
            let inst = self
                .builder
                .ins()
                .call(self.runtime[RuntimeFn::CloneValue], &[value.values[0]]);
            self.builder.inst_results(inst)[0]
        }
    }

    fn export_return_value(&mut self, value: ValueRef) -> Vec<Value> {
        if matches!(value.ty, DirectType::Callable(_)) {
            return self.transfer_callable_words(&value);
        }
        if let DirectType::Union(union) = &value.ty {
            if union.owns_handles() {
                return self.transfer_union_words(&value);
            }
            return value.values;
        }
        if !self.is_opaque_value(&value) {
            return value.values;
        }
        if self.temporary_owns_opaque(&value) {
            self.clear_temporary_opaque_owned(&value);
            value.values
        } else {
            vec![self.retain_opaque_handle(value.values[0])]
        }
    }

    fn owned_opaque_result(&mut self, values: Vec<Value>, ty: Type) -> ValueRef {
        let value = ValueRef {
            values,
            ty: DirectType::Opaque(ty),
        };
        self.mark_temporary_opaque_owned(&value);
        value
    }

    fn runtime_call_results(&mut self, callee: FuncRef, args: &[Value]) -> Vec<Value> {
        let inst = self.builder.ins().call(callee, args);
        self.builder.inst_results(inst).to_vec()
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
        self.owned_opaque_temporaries.clear();

        let mut in_range: Option<(&str, &str)> = None;
        for (instruction_index, instruction) in block.instructions.iter().enumerate() {
            if let Instruction::IndexInRange { list, index } = instruction {
                in_range = Some((list, index));
                continue;
            }
            self.proven_access = in_range.map_or(ProvenAccess::None, |(list, index)| {
                ProvenAccess::of(instruction, list, index)
            });
            // The fact covers the element read and then the one advance.
            if self.proven_access != ProvenAccess::Read {
                in_range = None;
            }
            let compiled = self.compile_instruction(&block.label, instruction_index, instruction);
            self.proven_access = ProvenAccess::None;
            compiled?;
        }
        self.compile_terminator(&block.terminator, return_ty)?;
        Ok(())
    }

    fn compile_instruction(
        &mut self,
        block_label: &str,
        instruction_index: usize,
        instruction: &Instruction,
    ) -> std::result::Result<(), String> {
        match instruction {
            // `compile_block` reads the fact for the instructions after it.
            Instruction::IndexInRange { .. } => {}
            Instruction::Safepoint => {
                let Some(fuel_variable) = self.safepoint_fuel else {
                    return Ok(());
                };
                let fuel = self.builder.use_var(fuel_variable);
                let remaining = self.builder.ins().iadd_imm(fuel, -1);
                self.builder.def_var(fuel_variable, remaining);
                let exhausted = self.builder.ins().icmp_imm(IntCC::Equal, remaining, 0);
                let slow_block = self.builder.create_block();
                let continue_block = self.builder.create_block();
                self.builder
                    .ins()
                    .brif(exhausted, slow_block, &[], continue_block, &[]);

                self.builder.switch_to_block(slow_block);
                let reset = self
                    .builder
                    .ins()
                    .iconst(types::I64, NATIVE_LOOP_SAFEPOINT_INTERVAL as i64);
                self.builder.def_var(fuel_variable, reset);
                self.builder
                    .ins()
                    .call(self.runtime[RuntimeFn::YieldNow], &[]);
                self.builder.ins().jump(continue_block, &[]);

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
                ..
            }
            | Instruction::ReborrowElement {
                loan,
                parent: source,
                selector,
                projection,
                span,
                ..
            } => {
                self.begin_element_loan(loan, source, selector, projection, *span)?;
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
                let selected = self.builder.ins().call(
                    self.runtime[RuntimeFn::TakeReturnedViewProjection],
                    &[projections_ptr, projections_len],
                );
                let selected = self.builder.inst_results(selected)[0];
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
                let mut canonical_selector = self.builder.ins().iconst(types::I64, -1);
                for (index, projection) in projections.iter().enumerate() {
                    let tag = *selector_tags.get(projection).ok_or_else(|| {
                        format!(
                            "direct backend has no selector tag for projection `{projection}` on loan `{loan}`"
                        )
                    })?;
                    let matches = self.builder.ins().icmp_imm(
                        IntCC::Equal,
                        selected,
                        i64::try_from(index).map_err(|_| {
                            format!(
                                "direct backend returned-view projection index overflows for loan `{loan}`"
                            )
                        })?,
                    );
                    let tag = self.builder.ins().iconst(types::I64, tag);
                    canonical_selector =
                        self.builder.ins().select(matches, tag, canonical_selector);
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
                        let index = self.builder.ins().iconst(types::I64, step as i64);
                        let inst = self
                            .builder
                            .ins()
                            .call(self.runtime[RuntimeFn::TakeReturnedViewSelector], &[index]);
                        self.builder.inst_results(inst)[0]
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
                        let selected = self.builder.ins().iconst(types::I64, tag);
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
                if let Rvalue::Try { value: try_value } = value {
                    let target_ty = self.type_of_place(target)?;
                    self.compile_try_assign(target, target_ty, try_value)?;
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
        self.release_all_temporary_owned();
        Ok(())
    }

    fn compile_terminator(
        &mut self,
        terminator: &Terminator,
        return_ty: &DirectType,
    ) -> std::result::Result<(), String> {
        match terminator {
            Terminator::Return(operand) => {
                let value = self.load_operand_for_target(operand, return_ty)?;
                let coerced = self.coerce_value(value, return_ty)?;
                self.emit_return_value(coerced)?;
            }
            Terminator::Goto(label) => {
                self.release_all_temporary_owned();
                let block = self.compiled_block(label)?;
                self.builder.ins().jump(block, &[]);
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
                    .ins()
                    .brif(condition, then_block, &[], else_block, &[]);
            }
            Terminator::Match {
                scrutinee,
                arms,
                otherwise,
            } => {
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
                        self.builder.ins().jump(arm_block, &[]);
                        return Ok(());
                    }
                    let next_block = self.builder.create_block();
                    let matched = self.variant_matches_value(
                        scrutinee.values[0],
                        arm.enum_name.as_deref().unwrap_or(scrutinee_enum_name),
                        arm.variant_name.as_deref().unwrap_or_default(),
                    )?;
                    let arm_block = self.compiled_block(&arm.label)?;
                    let matched_cleanup = self.builder.create_block();
                    let pending_owned = self.owned_opaque_temporaries.clone();
                    self.builder
                        .ins()
                        .brif(matched, matched_cleanup, &[], next_block, &[]);
                    self.builder.switch_to_block(matched_cleanup);
                    self.release_all_temporary_owned();
                    self.builder.ins().jump(arm_block, &[]);
                    self.builder.seal_block(matched_cleanup);
                    self.builder.switch_to_block(next_block);
                    self.owned_opaque_temporaries = pending_owned;
                }
                self.release_all_temporary_owned();
                let otherwise = self.compiled_block(otherwise)?;
                self.builder.ins().jump(otherwise, &[]);
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
                    None => self.builder.ins().iconst(types::I64, 0),
                };
                let (line, column) = self.span_values(Some(*span));
                match captures.as_slice() {
                    [] => {
                        self.builder
                            .ins()
                            .call(self.runtime[RuntimeFn::AssertFail], &[message, line, column]);
                    }
                    [left, right] => {
                        let left_label = self.string_value(&left.label)?.values[0];
                        let left_type = self.string_value(&left.ty.to_string())?.values[0];
                        let left_value = self.load_assertion_capture_value(&left.value)?;
                        let right_label = self.string_value(&right.label)?.values[0];
                        let right_type = self.string_value(&right.ty.to_string())?.values[0];
                        let right_value = self.load_assertion_capture_value(&right.value)?;
                        self.builder.ins().call(
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
                self.builder.ins().trap(TrapCode::unwrap_user(1));
            }
            // The checker proved this path cannot run (for example the
            // impossible arm of a narrowed module-constant read).
            Terminator::Unreachable => {
                self.builder.ins().trap(TrapCode::unwrap_user(2));
            }
        }
        Ok(())
    }

    fn load_assertion_capture_value(
        &mut self,
        operand: &Operand,
    ) -> std::result::Result<Value, String> {
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

    fn emit_return_value(&mut self, value: ValueRef) -> std::result::Result<(), String> {
        let mut return_values = self.export_return_value(value);
        self.emit_pending_cleanups_for(true, false)?;
        self.append_writeback_return_values(&mut return_values)?;
        self.release_all_temporary_owned();
        self.release_all_opaque_roots()?;
        self.builder
            .ins()
            .call(self.runtime[RuntimeFn::ExitCall], &[]);
        self.builder.ins().return_(&return_values);
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
                    .ins()
                    .call(self.runtime[RuntimeFn::NoneTest], &[loaded.values[0]]);
                Ok(ValueRef {
                    values: self.builder.inst_results(call).to_vec(),
                    ty: DirectType::Scalar(ScalarKind::Bool),
                })
            }
            Rvalue::ModuleConstant { key, initializer } => {
                let thunk = *self.function_thunk_refs.get(initializer).ok_or_else(|| {
                    format!(
                        "direct backend cannot find module constant initializer `{initializer}`"
                    )
                })?;
                let thunk_ptr = self.builder.ins().func_addr(types::I64, thunk);
                let (key_ptr, key_len) = self.string_constant(key.as_bytes())?;
                let call = self.builder.ins().call(
                    self.runtime[RuntimeFn::ModuleConstant],
                    &[key_ptr, key_len, thunk_ptr],
                );
                let value = self.owned_opaque_result(
                    self.builder.inst_results(call).to_vec(),
                    Type::named("Unknown"),
                );
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
            Rvalue::Unary { op, value, span } => {
                let integer_hint = target.scalar_kind().filter(|kind| kind.is_integer());
                if matches!(op, UnaryOp::Neg)
                    && matches!(integer_hint, Some(ScalarKind::Int64))
                    && matches!(value, Operand::Int(magnitude) if *magnitude == (i64::MAX as u128) + 1)
                {
                    return Ok(ValueRef {
                        values: vec![self.builder.ins().iconst(types::I64, i64::MIN)],
                        ty: DirectType::Scalar(ScalarKind::Int64),
                    });
                }
                let value = self.load_operand_with_integer_hint(value, integer_hint)?;
                self.compile_unary(*op, value, Some(*span))
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
                self.compile_binary(*op, left, right, Some(*span))
            }
            Rvalue::Call { callee, args } => self.compile_call(callee, args, target),
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
                    let matched = self.builder.ins().icmp_imm(
                        IntCC::Equal,
                        value.values[0],
                        *member_index as i64,
                    );
                    let matched = self.builder.ins().uextend(types::I64, matched);
                    return Ok(ValueRef {
                        values: vec![matched],
                        ty: DirectType::Scalar(ScalarKind::Bool),
                    });
                }
                let value = self.ensure_opaque(value)?;
                let encoded = crate::native_runtime::canonical_runtime_type_name(union_type);
                let (ptr, len) = self.string_constant(encoded.as_bytes())?;
                let index = self.builder.ins().iconst(types::I64, *member_index as i64);
                let call = self.builder.ins().call(
                    self.runtime[RuntimeFn::UnionTagTest],
                    &[value.values[0], ptr, len, index],
                );
                Ok(ValueRef {
                    values: self.builder.inst_results(call).to_vec(),
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
                let index = self.builder.ins().iconst(types::I64, *member_index as i64);
                let call = self.builder.ins().call(
                    self.runtime[RuntimeFn::UnionTakePayload],
                    &[value.values[0], ptr, len, index],
                );
                let payload = self.owned_opaque_result(
                    self.builder.inst_results(call).to_vec(),
                    member_type.clone(),
                );
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
                let tag = self.builder.ins().iconst(types::I64, *member_index as i64);
                let call = self.builder.ins().call(
                    self.runtime[RuntimeFn::UnionInject],
                    &[ptr, len, tag, transferred],
                );
                Ok(self.owned_opaque_result(
                    self.builder.inst_results(call).to_vec(),
                    union_type.clone(),
                ))
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
                    self.builder
                        .ins()
                        .bitcast(types::I64, MemFlags::new(), *word)
                } else {
                    *word
                });
            }
        }
        let global = self
            .object
            .declare_data_in_func(shape.descriptor, self.builder.func);
        let descriptor = self.builder.ins().symbol_value(types::I64, global);
        let zero = self.builder.ins().iconst(types::I64, 0);
        let mut values = vec![descriptor];
        if shape.inline() {
            values.extend(words.iter().copied());
            while values.len() < 1 + CALLABLE_ENVIRONMENT_WORDS {
                values.push(zero);
            }
        } else {
            let size = self
                .builder
                .ins()
                .iconst(types::I64, shape.env_words as i64);
            let call = self
                .builder
                .ins()
                .call(self.runtime[RuntimeFn::CallableEnvAlloc], &[size]);
            let block = self.builder.inst_results(call)[0];
            for (index, word) in words.iter().enumerate() {
                self.builder
                    .ins()
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
        let init = self
            .builder
            .ins()
            .call(self.runtime[RuntimeFn::VecEmpty], &[]);
        let vector = self.owned_opaque_result(
            self.builder.inst_results(init).to_vec(),
            Type::Named("list".to_string(), vec![resolved_element_type]),
        );
        for element in elements {
            let value = self.load_operand_as_opaque_direct(element, &element_direct_ty)?;
            let value = self.transfer_owned_opaque_value(&value);
            let result = self.builder.ins().call(
                self.runtime[RuntimeFn::VecPushInPlace],
                &[vector.values[0], value],
            );
            self.release_opaque_handle(self.builder.inst_results(result)[0]);
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
        let init = self
            .builder
            .ins()
            .call(self.runtime[RuntimeFn::MapEmpty], &[]);
        let map = self.owned_opaque_result(
            self.builder.inst_results(init).to_vec(),
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
            let result = self.builder.ins().call(
                self.runtime[RuntimeFn::MapSetInPlace],
                &[map.values[0], key, value],
            );
            self.release_opaque_handle(self.builder.inst_results(result)[0]);
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
        let init = self
            .builder
            .ins()
            .call(self.runtime[RuntimeFn::SetEmpty], &[]);
        let set = self.owned_opaque_result(
            self.builder.inst_results(init).to_vec(),
            Type::Named("set".to_string(), vec![resolved_element_type]),
        );
        for element in elements {
            let value = self.load_operand_as_opaque_direct(element, &element_direct_ty)?;
            let value = self.transfer_owned_opaque_value(&value);
            let _ = self.builder.ins().call(
                self.runtime[RuntimeFn::SetInsertInPlace],
                &[set.values[0], value],
            );
        }
        Ok(set)
    }

    fn compile_unary(
        &mut self,
        op: UnaryOp,
        value: ValueRef,
        span: Option<Span>,
    ) -> std::result::Result<ValueRef, String> {
        if matches!(value.ty, DirectType::Opaque(_)) || op == UnaryOp::BitNot {
            let target_ty = value.ty.clone();
            let value = self.ensure_opaque(value)?;
            let opcode = match op {
                UnaryOp::Neg => 0,
                UnaryOp::Not => 1,
                UnaryOp::BitNot => 2,
            };
            let opcode = self.builder.ins().iconst(types::I64, opcode);
            let (line, column) = self.span_values(span);
            let inst = self.builder.ins().call(
                self.runtime[RuntimeFn::UnaryValue],
                &[opcode, value.values[0], line, column],
            );
            let result = self.owned_opaque_result(
                self.builder.inst_results(inst).to_vec(),
                Type::named("Unknown"),
            );
            return if matches!(target_ty, DirectType::Opaque(_)) {
                Ok(result)
            } else {
                self.coerce_value(result, &target_ty)
            };
        }
        match (op, value.ty.scalar_kind()) {
            (UnaryOp::Neg, Some(ScalarKind::Int32)) => Ok(ValueRef {
                values: vec![self.builder.ins().ineg(value.values[0])],
                ty: DirectType::Scalar(ScalarKind::Int32),
            }),
            (UnaryOp::Neg, Some(ScalarKind::Int64)) => self.compile_wide_integer_negation(
                WideIntegerKind::Int64,
                value.values[0],
                span,
            ),
            (UnaryOp::Neg, Some(ScalarKind::Uint64)) => self.compile_wide_integer_negation(
                WideIntegerKind::Uint64,
                value.values[0],
                span,
            ),
            (UnaryOp::Neg, Some(kind)) if kind.is_float() => Ok(ValueRef {
                values: vec![self.builder.ins().fneg(value.values[0])],
                ty: DirectType::Scalar(kind),
            }),
            (UnaryOp::Not, Some(ScalarKind::Bool)) => {
                let zero = self.builder.ins().iconst(types::I64, 0);
                let cmp = self.builder.ins().icmp(IntCC::Equal, value.values[0], zero);
                Ok(ValueRef {
                    values: vec![self.builder.ins().uextend(types::I64, cmp)],
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
        value: Value,
        span: Option<Span>,
    ) -> std::result::Result<ValueRef, String> {
        let zero = self.builder.ins().iconst(types::I64, 0);
        let (result, overflow) = if kind.is_signed() {
            self.builder.ins().ssub_overflow(zero, value)
        } else {
            self.builder.ins().usub_overflow(zero, value)
        };
        self.emit_integer_overflow_failure_branch(
            overflow,
            kind,
            WideOverflowOp::Sub,
            zero,
            value,
            span,
        )?;
        Ok(ValueRef {
            values: vec![result],
            ty: DirectType::Scalar(kind.scalar_kind()),
        })
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
            let inst = self.builder.ins().call(
                self.runtime[RuntimeFn::CastValue],
                &[boxed.values[0], target_ptr, target_len, line, column],
            );
            let boxed =
                self.owned_opaque_result(self.builder.inst_results(inst).to_vec(), target.clone());
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
                let source_code = self.builder.ins().iconst(
                    types::I64,
                    if matches!(source_kind, ScalarKind::Uint64) {
                        1
                    } else {
                        0
                    },
                );
                let target_code = self.builder.ins().iconst(
                    types::I64,
                    match target_kind {
                        ScalarKind::Int32 => 0,
                        ScalarKind::Int64 => 1,
                        ScalarKind::Uint64 => 2,
                        _ => unreachable!("integer cast target should be exhaustive"),
                    },
                );
                let (line, column) = self.span_values(span);
                let inst = self.builder.ins().call(
                    self.runtime[RuntimeFn::CastIntegerToInteger],
                    &[source, source_code, target_code, line, column],
                );
                self.builder.inst_results(inst)[0]
            }
            (source_kind, target_kind) if source_kind.is_integer() && target_kind.is_float() => {
                let source_code = self.builder.ins().iconst(
                    types::I64,
                    if matches!(source_kind, ScalarKind::Uint64) {
                        1
                    } else {
                        0
                    },
                );
                let target_code = self.builder.ins().iconst(
                    types::I64,
                    if matches!(target_kind, ScalarKind::Float32) {
                        0
                    } else {
                        1
                    },
                );
                let (line, column) = self.span_values(span);
                let inst = self.builder.ins().call(
                    self.runtime[RuntimeFn::CastIntegerToFloat],
                    &[source, source_code, target_code, line, column],
                );
                self.builder.inst_results(inst)[0]
            }
            (source_kind, target_kind) if source_kind.is_float() && target_kind.is_integer() => {
                let target_code = self.builder.ins().iconst(
                    types::I64,
                    match target_kind {
                        ScalarKind::Int32 => 0,
                        ScalarKind::Int64 => 1,
                        ScalarKind::Uint64 => 2,
                        _ => unreachable!("integer cast target should be exhaustive"),
                    },
                );
                let (line, column) = self.span_values(span);
                let inst = self.builder.ins().call(
                    self.runtime[RuntimeFn::CastFloatToInteger],
                    &[source, target_code, line, column],
                );
                self.builder.inst_results(inst)[0]
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
            .ins()
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
            .ins()
            .brif(same_tag, compare_block, &[], differ_block, &[]);
        self.builder.switch_to_block(differ_block);
        self.builder.seal_block(differ_block);
        let zero = self.builder.ins().iconst(types::I64, 0);
        self.builder.ins().jump(merge_block, &[zero]);
        self.builder.switch_to_block(compare_block);
        self.builder.seal_block(compare_block);
        let members_equal =
            self.switch_on_union_tag(&union, &left.values, &bool_ty, |codegen, index, member| {
                if matches!(member.ty, DirectType::Scalar(ScalarKind::Unit)) {
                    return Ok(ValueRef {
                        values: vec![codegen.builder.ins().iconst(types::I64, 1)],
                        ty: DirectType::Scalar(ScalarKind::Bool),
                    });
                }
                let other = codegen.union_member_from_words(&union, index, &right_words)?;
                if matches!(member.ty, DirectType::Scalar(_)) {
                    return codegen.compile_binary(BinaryOp::Eq, member, other, span);
                }
                // A plain-class member compares under the runtime's shared
                // structural rule, exactly as the interpreter compares it.
                let member = codegen.ensure_opaque(member)?;
                let other = codegen.ensure_opaque(other)?;
                codegen.compile_binary(BinaryOp::Eq, member, other, span)
            })?;
        self.builder
            .ins()
            .jump(merge_block, &[members_equal.values[0]]);
        self.builder.switch_to_block(merge_block);
        self.builder.seal_block(merge_block);
        let equal = self.builder.block_params(merge_block)[0];
        let result = if op == BinaryOp::Eq {
            equal
        } else {
            self.builder.ins().bxor_imm(equal, 1)
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
        let opcode = self
            .builder
            .ins()
            .iconst(types::I64, Self::binary_opcode(op));
        let (line, column) = self.span_values(Some(span));
        let inst = match other {
            Operand::String(literal) => {
                let (list, index) = self.load_borrowed_list_element(&selection)?;
                let (literal_ptr, literal_len) = self.string_constant(literal.as_bytes())?;
                let literal_on_left = self.builder.ins().iconst(types::I64, 1 - element_on_left);
                self.builder.ins().call(
                    self.runtime[RuntimeFn::CompareStrLiteralAt],
                    &[
                        opcode,
                        list,
                        index,
                        literal_ptr,
                        literal_len,
                        literal_on_left,
                        line,
                        column,
                    ],
                )
            }
            other => {
                let other = self.load_operand(other)?;
                let other = self.ensure_opaque(other)?;
                let (list, index) = self.load_borrowed_list_element(&selection)?;
                let element_on_left = self.builder.ins().iconst(types::I64, element_on_left);
                self.builder.ins().call(
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
            values: vec![self.builder.inst_results(inst)[0]],
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
            return self.compile_binary(op, left, right, Some(span)).map(Some);
        }
        let (literal_ptr, literal_len) = self.string_constant(literal.as_bytes())?;
        let opcode = self
            .builder
            .ins()
            .iconst(types::I64, Self::binary_opcode(op));
        let literal_on_left = self.builder.ins().iconst(types::I64, literal_on_left);
        let (line, column) = self.span_values(Some(span));
        let inst = self.builder.ins().call(
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
            values: vec![self.builder.inst_results(inst)[0]],
            ty: DirectType::Scalar(ScalarKind::Bool),
        }))
    }

    fn compile_binary(
        &mut self,
        op: BinaryOp,
        left: ValueRef,
        right: ValueRef,
        span: Option<Span>,
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
            return self.compile_binary(op, left, right, span);
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
            let opcode = self.builder.ins().iconst(types::I64, opcode_value);
            let float_width = self.builder.ins().iconst(types::I64, float_width);
            let (line, column) = self.span_values(span);
            let inst = self.builder.ins().call(
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
            let result = self.owned_opaque_result(
                self.builder.inst_results(inst).to_vec(),
                Type::named("Unknown"),
            );
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
            let scalar_left = self.builder.ins().iconst(types::I64, scalar_left);
            let operation = self
                .builder
                .ins()
                .iconst(types::I64, direct_array_binary_opcode(op)?);
            let checked_mode = self.builder.ins().iconst(types::I64, 0);
            let (line, column) = self.span_values(span);
            let inst = self.builder.ins().call(
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
            return Ok(self.owned_opaque_result(self.builder.inst_results(inst).to_vec(), array_ty));
        }
        if matches!(left.ty, DirectType::Opaque(_)) || matches!(right.ty, DirectType::Opaque(_)) {
            let left = self.ensure_opaque(left)?;
            let right = self.ensure_opaque(right)?;
            let binary_opcode = Self::binary_opcode(op);
            let opcode = self.builder.ins().iconst(types::I64, binary_opcode);
            if is_comparison_op(op) {
                // The result is a machine `bool`, so no box is built for it.
                let (line, column) = self.span_values(span);
                let inst = self.builder.ins().call(
                    self.runtime[RuntimeFn::CompareValues],
                    &[opcode, left.values[0], right.values[0], line, column],
                );
                return Ok(ValueRef {
                    values: vec![self.builder.inst_results(inst)[0]],
                    ty: DirectType::Scalar(ScalarKind::Bool),
                });
            }
            let float_width = self.builder.ins().iconst(types::I64, 0);
            let (line, column) = self.span_values(span);
            let inst = self.builder.ins().call(
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
            return Ok(self.owned_opaque_result(
                self.builder.inst_results(inst).to_vec(),
                Type::named("Unknown"),
            ));
        }
        match (left.ty.scalar_kind(), right.ty.scalar_kind()) {
            (Some(ScalarKind::Int32), Some(ScalarKind::Int32)) => {
                self.compile_int32_binary(op, left.values[0], right.values[0], span)
            }
            (Some(ScalarKind::Int64), Some(ScalarKind::Int64)) => self
                .compile_wide_integer_binary(
                    WideIntegerKind::Int64,
                    op,
                    left.values[0],
                    right.values[0],
                    span,
                ),
            (Some(ScalarKind::Uint64), Some(ScalarKind::Uint64)) => self
                .compile_wide_integer_binary(
                    WideIntegerKind::Uint64,
                    op,
                    left.values[0],
                    right.values[0],
                    span,
                ),
            (Some(lhs), Some(rhs)) if lhs.is_float() && rhs.is_float() => {
                self.compile_float_binary(op, left.values[0], right.values[0], lhs, span)
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

    fn compile_int32_binary(
        &mut self,
        op: BinaryOp,
        left: Value,
        right: Value,
        span: Option<Span>,
    ) -> std::result::Result<ValueRef, String> {
        let ty = DirectType::Scalar(ScalarKind::Int32);
        let value = match op {
            BinaryOp::Add => ValueRef {
                values: vec![self.builder.ins().iadd(left, right)],
                ty,
            },
            BinaryOp::Sub => ValueRef {
                values: vec![self.builder.ins().isub(left, right)],
                ty,
            },
            BinaryOp::Mul => ValueRef {
                values: vec![self.builder.ins().imul(left, right)],
                ty,
            },
            BinaryOp::FloorDiv => {
                self.emit_int_division_guard(right, span)?;
                ValueRef {
                    values: vec![self.compile_signed_floor_divmod(left, right, true)],
                    ty,
                }
            }
            BinaryOp::Mod => {
                self.emit_int_division_guard(right, span)?;
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

    fn compile_signed_floor_divmod(&mut self, left: Value, right: Value, quotient: bool) -> Value {
        let truncating_quotient = self.builder.ins().sdiv(left, right);
        let truncating_remainder = self.builder.ins().srem(left, right);
        let zero = self.builder.ins().iconst(types::I64, 0);
        let remainder_nonzero =
            self.builder
                .ins()
                .icmp(IntCC::NotEqual, truncating_remainder, zero);
        let left_negative = self.builder.ins().icmp(IntCC::SignedLessThan, left, zero);
        let right_negative = self.builder.ins().icmp(IntCC::SignedLessThan, right, zero);
        let signs_differ = self.builder.ins().bxor(left_negative, right_negative);
        let adjust = self.builder.ins().band(remainder_nonzero, signs_differ);

        if quotient {
            let one = self.builder.ins().iconst(types::I64, 1);
            let adjusted = self.builder.ins().isub(truncating_quotient, one);
            self.builder
                .ins()
                .select(adjust, adjusted, truncating_quotient)
        } else {
            let adjusted = self.builder.ins().iadd(truncating_remainder, right);
            self.builder
                .ins()
                .select(adjust, adjusted, truncating_remainder)
        }
    }

    fn compile_wide_integer_binary(
        &mut self,
        kind: WideIntegerKind,
        op: BinaryOp,
        left: Value,
        right: Value,
        span: Option<Span>,
    ) -> std::result::Result<ValueRef, String> {
        let ty = DirectType::Scalar(kind.scalar_kind());
        let arithmetic = |value| ValueRef {
            values: vec![value],
            ty: ty.clone(),
        };
        let value = match op {
            // An index below a list length advances without overflow.
            BinaryOp::Add if self.proven_access == ProvenAccess::Advance => {
                arithmetic(self.builder.ins().iadd(left, right))
            }
            BinaryOp::Add => {
                let (result, overflow) = if kind.is_signed() {
                    self.builder.ins().sadd_overflow(left, right)
                } else {
                    self.builder.ins().uadd_overflow(left, right)
                };
                self.emit_integer_overflow_failure_branch(
                    overflow,
                    kind,
                    WideOverflowOp::Add,
                    left,
                    right,
                    span,
                )?;
                arithmetic(result)
            }
            BinaryOp::Sub => {
                let (result, overflow) = if kind.is_signed() {
                    self.builder.ins().ssub_overflow(left, right)
                } else {
                    self.builder.ins().usub_overflow(left, right)
                };
                self.emit_integer_overflow_failure_branch(
                    overflow,
                    kind,
                    WideOverflowOp::Sub,
                    left,
                    right,
                    span,
                )?;
                arithmetic(result)
            }
            BinaryOp::Mul => {
                let (result, overflow) = if kind.is_signed() {
                    self.builder.ins().smul_overflow(left, right)
                } else {
                    self.builder.ins().umul_overflow(left, right)
                };
                self.emit_integer_overflow_failure_branch(
                    overflow,
                    kind,
                    WideOverflowOp::Mul,
                    left,
                    right,
                    span,
                )?;
                arithmetic(result)
            }
            BinaryOp::FloorDiv => {
                self.emit_int_division_guard(right, span)?;
                if kind.is_signed() {
                    let min = self.builder.ins().iconst(types::I64, i64::MIN);
                    let negative_one = self.builder.ins().iconst(types::I64, -1);
                    let is_min = self.builder.ins().icmp(IntCC::Equal, left, min);
                    let is_negative_one =
                        self.builder.ins().icmp(IntCC::Equal, right, negative_one);
                    let overflow = self.builder.ins().band(is_min, is_negative_one);
                    self.emit_integer_overflow_failure_branch(
                        overflow,
                        kind,
                        WideOverflowOp::Div,
                        left,
                        right,
                        span,
                    )?;
                    arithmetic(self.compile_signed_floor_divmod(left, right, true))
                } else {
                    arithmetic(self.builder.ins().udiv(left, right))
                }
            }
            BinaryOp::Mod => {
                self.emit_int_division_guard(right, span)?;
                if kind.is_signed() {
                    let min = self.builder.ins().iconst(types::I64, i64::MIN);
                    let negative_one = self.builder.ins().iconst(types::I64, -1);
                    let is_min = self.builder.ins().icmp(IntCC::Equal, left, min);
                    let is_negative_one =
                        self.builder.ins().icmp(IntCC::Equal, right, negative_one);
                    let exceptional = self.builder.ins().band(is_min, is_negative_one);
                    let normal_block = self.builder.create_block();
                    let continue_block = self.builder.create_block();
                    self.builder.append_block_param(continue_block, types::I64);
                    let zero_remainder = self.builder.ins().iconst(types::I64, 0);
                    self.builder.ins().brif(
                        exceptional,
                        continue_block,
                        &[zero_remainder],
                        normal_block,
                        &[],
                    );
                    self.builder.switch_to_block(normal_block);
                    let remainder = self.compile_signed_floor_divmod(left, right, false);
                    self.builder.ins().jump(continue_block, &[remainder]);
                    self.builder.seal_block(normal_block);
                    self.builder.switch_to_block(continue_block);
                    self.builder.seal_block(continue_block);
                    arithmetic(self.builder.block_params(continue_block)[0])
                } else {
                    arithmetic(self.builder.ins().urem(left, right))
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
        left: Value,
        right: Value,
        kind: ScalarKind,
        span: Option<Span>,
    ) -> std::result::Result<ValueRef, String> {
        let ty = DirectType::Scalar(kind);
        match op {
            BinaryOp::Add => Ok(ValueRef {
                values: vec![self.builder.ins().fadd(left, right)],
                ty,
            }),
            BinaryOp::Sub => Ok(ValueRef {
                values: vec![self.builder.ins().fsub(left, right)],
                ty,
            }),
            BinaryOp::Mul => Ok(ValueRef {
                values: vec![self.builder.ins().fmul(left, right)],
                ty,
            }),
            BinaryOp::Div => {
                self.emit_float_division_guard(right, span)?;
                Ok(ValueRef {
                    values: vec![self.builder.ins().fdiv(left, right)],
                    ty,
                })
            }
            BinaryOp::FloorDiv | BinaryOp::Mod => {
                let opcode_value = Self::binary_opcode(op);
                let left_box = self
                    .builder
                    .ins()
                    .call(self.runtime[RuntimeFn::BoxF64], &[left]);
                let right_box = self
                    .builder
                    .ins()
                    .call(self.runtime[RuntimeFn::BoxF64], &[right]);
                let left_boxed = self.builder.inst_results(left_box)[0];
                let right_boxed = self.builder.inst_results(right_box)[0];
                let opcode = self.builder.ins().iconst(types::I64, opcode_value);
                let float_width = self.builder.ins().iconst(types::I64, 0);
                let (line, column) = self.span_values(span);
                let result = self.builder.ins().call(
                    self.runtime[RuntimeFn::BinaryValue],
                    &[opcode, left_boxed, right_boxed, float_width, line, column],
                );
                let result_boxed = self.builder.inst_results(result)[0];
                self.release_opaque_handle(left_boxed);
                self.release_opaque_handle(right_boxed);
                let unboxed = self
                    .builder
                    .ins()
                    .call(self.runtime[RuntimeFn::UnboxF64], &[result_boxed]);
                let value = self.builder.inst_results(unboxed)[0];
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
        left: Value,
        right: Value,
    ) -> std::result::Result<ValueRef, String> {
        match op {
            BinaryOp::Eq => Ok(self.boolean_from_icmp(IntCC::Equal, left, right)),
            BinaryOp::NotEq => Ok(self.boolean_from_icmp(IntCC::NotEqual, left, right)),
            BinaryOp::And => Ok(ValueRef {
                values: vec![self.builder.ins().band(left, right)],
                ty: DirectType::Scalar(ScalarKind::Bool),
            }),
            BinaryOp::Or => Ok(ValueRef {
                values: vec![self.builder.ins().bor(left, right)],
                ty: DirectType::Scalar(ScalarKind::Bool),
            }),
            other => Err(format!(
                "direct backend does not support boolean binary operation `{:?}`",
                other
            )),
        }
    }

    fn boolean_from_icmp(&mut self, cc: IntCC, left: Value, right: Value) -> ValueRef {
        let cmp = self.builder.ins().icmp(cc, left, right);
        ValueRef {
            values: vec![self.builder.ins().uextend(types::I64, cmp)],
            ty: DirectType::Scalar(ScalarKind::Bool),
        }
    }

    fn boolean_from_fcmp(&mut self, cc: FloatCC, left: Value, right: Value) -> ValueRef {
        let cmp = self.builder.ins().fcmp(cc, left, right);
        ValueRef {
            values: vec![self.builder.ins().uextend(types::I64, cmp)],
            ty: DirectType::Scalar(ScalarKind::Bool),
        }
    }

    fn emit_int_division_guard(
        &mut self,
        divisor: Value,
        span: Option<Span>,
    ) -> std::result::Result<(), String> {
        let zero = self.builder.ins().iconst(types::I64, 0);
        let is_zero = self.builder.ins().icmp(IntCC::Equal, divisor, zero);
        self.emit_division_failure_branch(is_zero, span)
    }

    fn emit_integer_overflow_failure_branch(
        &mut self,
        overflow: Value,
        kind: WideIntegerKind,
        op: WideOverflowOp,
        left: Value,
        right: Value,
        span: Option<Span>,
    ) -> std::result::Result<(), String> {
        let fail_block = self.builder.create_block();
        let continue_block = self.builder.create_block();
        self.builder
            .ins()
            .brif(overflow, fail_block, &[], continue_block, &[]);
        self.builder.switch_to_block(fail_block);
        self.emit_pending_cleanups(true)?;
        let kind_code = self.builder.ins().iconst(types::I64, kind as i64);
        let op_code = self.builder.ins().iconst(types::I64, op as i64);
        let (line, column) = self.span_values(span);
        self.builder.ins().call(
            self.runtime[RuntimeFn::FailIntegerOverflow],
            &[kind_code, op_code, left, right, line, column],
        );
        self.builder.ins().trap(TrapCode::INTEGER_OVERFLOW);
        self.builder.seal_block(fail_block);
        self.builder.switch_to_block(continue_block);
        self.builder.seal_block(continue_block);
        Ok(())
    }

    fn emit_float_division_guard(
        &mut self,
        divisor: Value,
        span: Option<Span>,
    ) -> std::result::Result<(), String> {
        let zero = self.builder.ins().f64const(Ieee64::with_float(0.0));
        let is_zero = self.builder.ins().fcmp(FloatCC::Equal, divisor, zero);
        self.emit_division_failure_branch(is_zero, span)
    }

    fn emit_division_failure_branch(
        &mut self,
        is_zero: Value,
        span: Option<Span>,
    ) -> std::result::Result<(), String> {
        let fail_block = self.builder.create_block();
        let continue_block = self.builder.create_block();
        self.builder
            .ins()
            .brif(is_zero, fail_block, &[], continue_block, &[]);
        self.builder.switch_to_block(fail_block);
        self.emit_pending_cleanups(true)?;
        let (line, column) = self.span_values(span);
        self.builder
            .ins()
            .call(self.runtime[RuntimeFn::FailDivisionByZero], &[line, column]);
        self.builder.ins().trap(TrapCode::INTEGER_DIVISION_BY_ZERO);
        self.builder.seal_block(fail_block);
        self.builder.switch_to_block(continue_block);
        self.builder.seal_block(continue_block);
        Ok(())
    }

    fn compile_call(
        &mut self,
        callee: &CallTarget,
        args: &[MirArg],
        target: &DirectType,
    ) -> std::result::Result<ValueRef, String> {
        match callee {
            CallTarget::Name(name) => self.compile_named_call(name, args, target),
            CallTarget::Value(function) => self.compile_function_value_call(function, args, target),
            CallTarget::Extern(call) => self.compile_extern_call(call, args, target),
            CallTarget::Member {
                object,
                field,
                receiver_place,
            } => self.compile_member_call(
                object,
                MemberCallee::Field(field),
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
                trait_name,
                field,
                receiver_place,
            } => self.compile_trait_member_call(
                object,
                trait_name,
                field,
                receiver_place.as_deref(),
                args,
            ),
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
        let buffer_slot = self.builder.create_sized_stack_slot(StackSlotData::new(
            StackSlotKind::ExplicitSlot,
            buffer_size,
            3,
        ));
        let buffer = self.builder.ins().stack_addr(types::I64, buffer_slot, 0);
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
                .ins()
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
        let count = self
            .builder
            .ins()
            .iconst(types::I64, call.params.len() as i64);
        let invocation = self.builder.ins().call(
            self.runtime[RuntimeFn::FfiCall],
            &[spec_ptr, spec_len, buffer, count],
        );
        let raw_result = self.builder.inst_results(invocation)[0];

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
        let slot = self.builder.create_sized_stack_slot(StackSlotData::new(
            StackSlotKind::ExplicitSlot,
            (8 * (1 + CALLABLE_ENVIRONMENT_WORDS)) as u32,
            3,
        ));
        let callable_ptr = self.builder.ins().stack_addr(types::I64, slot, 0);
        for (index, word) in callee.values.iter().enumerate() {
            self.builder
                .ins()
                .store(MemFlags::new(), *word, callable_ptr, (index as i32) * 8);
        }

        let mut supplied: Vec<Option<Vec<Value>>> = vec![None; params.len()];
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
                .map(|_| self.builder.ins().iconst(types::I64, 0))
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
        let mask = self.builder.ins().iconst(types::I64, mask_bits);
        let mut lowered = vec![callable_ptr, mask];
        for (index, expected) in param_types.iter().enumerate() {
            match supplied[index].take() {
                Some(values) => lowered.extend(values),
                None => lowered.extend(expected.zero_values(&mut self.builder)),
            }
        }
        let invoke = self.builder.ins().load(
            types::I64,
            MemFlags::new(),
            callee.values[0],
            callables::DESCRIPTOR_INVOKE,
        );
        let invoke_signature =
            self.builder
                .func
                .import_signature(callables::contract_invoke_signature(
                    &param_types,
                    &mutable_params,
                    &return_direct,
                    self.call_conv,
                ));
        let inst = self
            .builder
            .ins()
            .call_indirect(invoke_signature, invoke, &lowered);
        let results = self.builder.inst_results(inst).to_vec();
        self.release_mutable_sinks(public_sinks.iter().copied());
        self.release_mutable_sinks(capture_sinks.iter().map(|(_, sink)| *sink));

        // The environment words may have changed (mutated captures), so the
        // callee's words are read back into its local or owned temporary.
        let reloaded = (0..1 + CALLABLE_ENVIRONMENT_WORDS)
            .map(|index| {
                self.builder.ins().load(
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
                self.builder.ins().iadd_imm(callable_ptr, 8)
            } else {
                reloaded[1]
            };
            let mut values = Vec::new();
            for (offset, abi) in writeback.ty.abi_types().into_iter().enumerate() {
                let word = self.builder.ins().load(
                    types::I64,
                    MemFlags::new(),
                    env_base,
                    ((writeback.env_offset + offset) as i32) * 8,
                );
                values.push(if abi == types::F64 {
                    self.builder
                        .ins()
                        .bitcast(types::F64, MemFlags::new(), word)
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
        let bound = self.bind_named_runtime_op_args(op, args)?;
        let Some(argument) = bound[0] else {
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
        self.builder.ins().call(self.runtime[function], &[word]);
        Ok(unit_value(&mut self.builder))
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
        let call = self
            .builder
            .ins()
            .call(self.runtime[RuntimeFn::FormatBegin], &[ptr, len]);
        let builder =
            self.owned_opaque_result(self.builder.inst_results(call).to_vec(), Type::named("str"));
        let handle = builder.values[0];
        for part in rest {
            match part {
                MirFormatPart::Literal(text) => {
                    let (ptr, len) = self.string_constant(text.as_bytes())?;
                    self.builder
                        .ins()
                        .call(self.runtime[RuntimeFn::FormatPushStr], &[handle, ptr, len]);
                }
                MirFormatPart::Int64(value) => {
                    let value = self.load_operand(value)?;
                    if value.ty == DirectType::Scalar(ScalarKind::Int64) {
                        self.builder.ins().call(
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
                    self.builder.ins().call(
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
        handle: Value,
        value: ValueRef,
    ) -> std::result::Result<(), String> {
        let value = self.ensure_opaque(value)?;
        self.builder.ins().call(
            self.runtime[RuntimeFn::FormatPushValue],
            &[handle, value.values[0]],
        );
        Ok(())
    }

    fn string_value(&mut self, text: &str) -> std::result::Result<ValueRef, String> {
        let (ptr, len) = self.string_constant(text.as_bytes())?;
        let call = self
            .builder
            .ins()
            .call(self.runtime[RuntimeFn::StringLiteral], &[ptr, len]);
        Ok(self.owned_opaque_result(self.builder.inst_results(call).to_vec(), Type::named("str")))
    }

    /// A direct call to a function of the program. Lowering resolves calls
    /// to built-in functions to runtime ops.
    fn compile_named_call(
        &mut self,
        name: &str,
        args: &[MirArg],
        target: &DirectType,
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
        let mut substitutions = HashMap::new();
        if let Some(return_type) = self.function_return_types.get(name) {
            collect_direct_runtime_type_substitutions(
                &direct_type_to_type(return_type),
                &direct_type_to_type(target),
                &mut substitutions,
            );
        }
        for (expected_ty, argument) in expected.iter().zip(args.iter()) {
            if let Some(actual_ty) =
                infer_operand_type(&argument.value, &self.variable_types, &self.classes)
            {
                collect_direct_runtime_type_substitutions(
                    &direct_type_to_type(expected_ty),
                    &direct_type_to_type(&actual_ty),
                    &mut substitutions,
                );
            }
        }
        let mut writeback_places = Vec::new();
        let mut mutable_sink_places = vec![None; expected.len()];
        let lendable = self.lendable_arguments(name, args, &expected);
        let mut lent = Vec::new();
        // The shared validator binds one argument to each parameter.
        debug_assert_eq!(args.len(), expected.len(), "arguments of `{name}`");
        for (index, (argument, abi_expected)) in args.iter().zip(&expected).enumerate() {
            let specialized = substitute_type(&direct_type_to_type(abi_expected), &substitutions);
            let semantic_expected = ensure_direct_type(
                &specialized,
                &self.classes,
                &format!("specialized argument {} for `{name}`", index + 1),
            )?;
            let loaded = if let Some(lent_argument) = lendable.get(index).and_then(Option::as_ref) {
                // A shared argument stays owned here and is moved back after
                // the call; a `mut` argument is handed over and comes back
                // through the writeback, as a cloned one would.
                let value = self.take_place(&lent_argument.place)?;
                if lent_argument.restore {
                    self.clear_temporary_opaque_owned(&value);
                    lent.push((lent_argument.place.clone(), value.clone()));
                }
                value
            } else {
                self.load_operand_for_target(&argument.value, &semantic_expected)?
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
                    None => self.builder.ins().iconst(types::I64, 0),
                });
            }
            self.install_direct_mutable_sinks(&sinks)?;
            sinks
        } else {
            Vec::new()
        };
        let inst = self.builder.ins().call(func_ref, &lowered_args);
        let results = self.builder.inst_results(inst).to_vec();
        self.release_mutable_sinks(mutable_sinks);
        let (result, writebacks) = self.split_call_results(name, results)?;
        self.apply_writeback_places(&writeback_places, writebacks)?;
        for (place, value) in lent {
            self.mark_temporary_opaque_owned(&value);
            self.store_place(&place, value)?;
        }
        Ok(result)
    }

    /// The arguments of a direct call to `name` that can be lent instead of
    /// cloned: a collection field such as `self.by_id` given to a shared or
    /// `mut` parameter. The field is moved out of its instance for the call
    /// (see `lendable_receiver`). A shared argument is moved back afterwards;
    /// a `mut` argument comes back through the call's writeback. An argument
    /// is not lent when another argument overlaps its place, and nothing is
    /// lent when an argument could hold an instance or callable that
    /// aliases the owner.
    fn lendable_arguments(
        &self,
        name: &str,
        args: &[MirArg],
        expected: &[DirectType],
    ) -> Vec<Option<LentArgument>> {
        let mut lendable = (0..args.len()).map(|_| None).collect::<Vec<_>>();
        let Some(passing) = self.function_param_passing.get(name) else {
            return lendable;
        };
        let mut places = Vec::with_capacity(args.len());
        for argument in args {
            let place = match &argument.value {
                Operand::Place(place) | Operand::MovePlace(place) => place,
                _ => {
                    places.push(None);
                    continue;
                }
            };
            let Ok(resolved) = self.resolve_view_place(place) else {
                return lendable;
            };
            let Ok(argument_ty) = self.type_of_place(place) else {
                return lendable;
            };
            if self.type_may_alias_instance(&direct_type_to_type(&argument_ty)) {
                return lendable;
            }
            places.push(Some((resolved, argument_ty)));
        }
        for (index, argument) in args.iter().enumerate() {
            let Operand::Place(place) = &argument.value else {
                continue;
            };
            let restore = match (passing.get(index), &argument.writeback_place) {
                (Some(MirReceiverKind::Borrow), None) => true,
                (Some(MirReceiverKind::BorrowMut), Some(writeback)) if writeback == place => false,
                _ => continue,
            };
            if !matches!(expected.get(index), Some(DirectType::Opaque(_))) {
                continue;
            }
            let Some((resolved, DirectType::Opaque(_))) = &places[index] else {
                continue;
            };
            let [alternative] = resolved.alternatives.as_slice() else {
                continue;
            };
            if !alternative.conditions.is_empty()
                || !alternative.union_payloads.is_empty()
                || !alternative.elements.is_empty()
            {
                continue;
            }
            let Some((root, _)) = alternative.place.split_once('.') else {
                continue;
            };
            let is_instance = is_plain_field_path(&alternative.place)
                && matches!(
                    self.variable_types.get(root),
                    Some(DirectType::Opaque(Type::Named(class, _))) if self.classes.contains_key(class)
                );
            let overlaps = places.iter().enumerate().any(|(other, entry)| {
                other != index
                    && entry.as_ref().is_some_and(|(other_resolved, _)| {
                        other_resolved.alternatives.iter().any(|candidate| {
                            places_overlap(&candidate.place, &alternative.place)
                                || projects_from_other_root(
                                    &alternative.place,
                                    &candidate.place,
                                    !candidate.elements.is_empty(),
                                )
                        })
                    })
            });
            if is_instance && !overlaps {
                lendable[index] = Some(LentArgument {
                    place: alternative.place.clone(),
                    restore,
                });
            }
        }
        lendable
    }

    /// Lowers one `str` text-convenience method to the shared runtime
    /// dispatcher. The receiver stays shared; a text argument is moved into
    /// the call and a count travels as a scalar.
    fn compile_string_text_method(
        &mut self,
        receiver: Value,
        field: &str,
        args: &[MirArg],
    ) -> std::result::Result<ValueRef, String> {
        let opcode = text_method_opcode(field)
            .expect("only text-convenience methods reach the text dispatcher");
        let expected: &[&str] = match field {
            "split_once" | "rsplit_once" => &["separator"],
            "find" | "count" => &["text"],
            "repeat" => &["count"],
            _ => &[],
        };
        let ordered = ordered_named_args(expected, args)?;
        let zero = self.builder.ins().iconst(types::I64, 0);
        let (argument, count) = match (field, ordered.first()) {
            ("repeat", Some(argument)) => {
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
        let opcode = self.builder.ins().iconst(types::I64, opcode);
        let inst = self.builder.ins().call(
            self.runtime[RuntimeFn::StringTextMethod],
            &[opcode, receiver, argument, count],
        );
        let semantic_type = text_convenience_result_type(field)
            .expect("text-convenience methods have a result type");
        let result = self.owned_opaque_result(
            self.builder.inst_results(inst).to_vec(),
            semantic_type.clone(),
        );
        let target = ensure_direct_type(
            &semantic_type,
            &self.classes,
            &format!("return value of `str.{field}`"),
        )?;
        self.coerce_value(result, &target)
    }

    /// Binds the arguments of a named op to its parameters, in declaration
    /// order. The validator has already checked them.
    fn bind_named_runtime_op_args<'args>(
        &self,
        op: RuntimeOp,
        args: &'args [MirArg],
    ) -> std::result::Result<Vec<Option<&'args MirArg>>, String> {
        let params = op.params();
        let names = params.iter().map(|param| param.name).collect::<Vec<_>>();
        ordered_optional_named_args(&names, args)
    }

    /// Emits a resolved op with no receiver: a named built-in function,
    /// `print`, or a host builtin.
    fn compile_named_runtime_op(
        &mut self,
        op: RuntimeOp,
        args: &[MirArg],
        target: &DirectType,
    ) -> std::result::Result<ValueRef, String> {
        match op {
            RuntimeOp::Print(kind) => self.compile_print(kind, args),
            RuntimeOp::Host(builtin) => self.compile_host_builtin_call(builtin, args),
            RuntimeOp::Function(function) => self.compile_function_op(function, args, target),
            RuntimeOp::Resource(_)
            | RuntimeOp::Rng(_)
            | RuntimeOp::Scalar(_)
            | RuntimeOp::Value(_) => Err(format!(
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
        let bound = self.bind_named_runtime_op_args(runtime_op, args)?;
        let mut words = Vec::with_capacity(params.len() + call.immediates.len());
        // `abs`, `min`, `max`, `sqrt`, and `round` return their first
        // argument's layout.
        let mut first_layout = None;
        for (param, argument) in params.iter().zip(bound) {
            let Some(argument) = argument else {
                // The shared validator binds every required parameter, so
                // only an omitted timeout or queue capacity is absent. It
                // passes no value.
                debug_assert!(!param.required, "`{}` binds `{}`", op.name(), param.name);
                words.push(self.builder.ins().iconst(types::I64, 0));
                continue;
            };
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
            FunctionOpResultLayout::Unit => Ok(unit_value(&mut self.builder)),
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
            .ins()
            .iconst(types::I64, direct_array_dtype_code(&element_type)?);
        let zero = self.builder.ins().iconst(types::I64, 0);
        let shape_ty =
            DirectType::Opaque(Type::Named("list".to_string(), vec![Type::named("int64")]));
        let ordered = ordered_named_args(
            &runtime_op
                .params()
                .iter()
                .map(|param| param.name)
                .collect::<Vec<_>>(),
            args,
        )?;
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
        let ordered = ordered_named_args(&["minimum"], args)?;
        let minimum =
            self.load_operand_with_integer_hint(&ordered[0].value, Some(ScalarKind::Int64))?;
        let minimum = self.coerce_value(minimum, &DirectType::Scalar(ScalarKind::Int64))?;
        let empty = op
            .empty_collection_fn()
            .expect("a capacity constructor names its empty collection");
        let empty_call = self.builder.ins().call(self.runtime[empty], &[]);
        let collection = self.builder.inst_results(empty_call)[0];
        let call = RuntimeOp::Function(op).runtime_call();
        let zero = self.builder.ins().iconst(types::I64, 0);
        let mut words = vec![collection, zero, minimum.values[0]];
        self.push_runtime_call_words(call, &mut words);
        let reserve = self.builder.ins().call(self.runtime[call.function], &words);
        self.release_opaque_handle(self.builder.inst_results(reserve)[0]);
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
        let inst = self.builder.ins().call(self.runtime[function], &[tuple]);
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
        Ok(self.owned_opaque_result(self.builder.inst_results(inst).to_vec(), result_type))
    }

    /// `wait_any` or `wait_all`, with or without a timeout.
    fn compile_wait(
        &mut self,
        op: FunctionOp,
        args: &[MirArg],
    ) -> std::result::Result<ValueRef, String> {
        let runtime_op = RuntimeOp::Function(op);
        let bound = self.bind_named_runtime_op_args(runtime_op, args)?;
        let tasks_arg = required_named_arg(
            bound[0],
            &format!(
                "direct backend expected `{}(tasks, timeout=...)`",
                op.name()
            ),
        )?;
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
        if let Some(timeout_arg) = bound.get(1).copied().flatten() {
            let timeout = self.load_operand(&timeout_arg.value)?;
            let timeout = self.ensure_opaque(timeout)?;
            words.push(timeout.values[0]);
        }
        let function = runtime_op.runtime_call().function;
        let inst = self.builder.ins().call(self.runtime[function], &words);
        let enum_name = match op.unspecialized() {
            FunctionOp::WaitAny => "WaitAny",
            _ => "WaitAll",
        };
        Ok(self.owned_opaque_result(
            self.builder.inst_results(inst).to_vec(),
            Type::Named(enum_name.to_string(), vec![task_payload_ty]),
        ))
    }

    /// `divmod(left, right)`: the left operand is tagged with its static
    /// type, which the helper's quotient and remainder take.
    fn compile_divmod(&mut self, args: &[MirArg]) -> std::result::Result<ValueRef, String> {
        let ordered = ordered_named_args(&["left", "right"], args)?;
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
            .ins()
            .call(self.runtime[function], &[left.values[0], right.values[0]]);
        let result_ty = Type::Tuple(vec![operand_ty.clone(), operand_ty]);
        Ok(self.owned_opaque_result(self.builder.inst_results(inst).to_vec(), result_ty))
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
        let bound = self.bind_named_runtime_op_args(RuntimeOp::Host(builtin), args)?;
        let mut loaded_args = Vec::with_capacity(bound.len());
        for (index, argument) in bound.into_iter().enumerate() {
            let argument = required_named_arg(
                argument,
                &format!(
                    "direct backend is missing argument {} for `{name}`",
                    index + 1
                ),
            )?;
            let loaded = self.load_operand(&argument.value)?;
            let loaded = self.ensure_opaque(loaded)?;
            loaded_args.push(loaded);
        }
        let count = self
            .builder
            .ins()
            .iconst(types::I64, loaded_args.len() as i64);
        let buffer_call = self
            .builder
            .ins()
            .call(self.runtime[RuntimeFn::ArgBufferNew], &[count]);
        let buffer = self.builder.inst_results(buffer_call)[0];
        for (index, loaded) in loaded_args.iter().enumerate() {
            let index = self.builder.ins().iconst(types::I64, index as i64);
            self.builder.ins().call(
                self.runtime[RuntimeFn::ArgBufferStore],
                &[buffer, index, loaded.values[0]],
            );
        }
        let entry = self.host_builtin_ref(builtin)?;
        let call = self.builder.ins().call(entry, &[buffer, count]);

        let semantic_type = RuntimeOp::Host(builtin)
            .result_type(None, &[])
            .expect("a host builtin names its result type");
        let result = self.owned_opaque_result(
            self.builder.inst_results(call).to_vec(),
            semantic_type.clone(),
        );
        let target = ensure_direct_type(
            &semantic_type,
            &self.classes,
            &format!("return value of `{name}`"),
        )?;
        self.coerce_value(result, &target)
    }

    /// The runtime entry for one host builtin, imported on first use.
    fn host_builtin_ref(
        &mut self,
        builtin: HostBuiltin,
    ) -> std::result::Result<cranelift_codegen::ir::FuncRef, String> {
        let RuntimeTarget::Host(symbol) = RuntimeOp::Host(builtin).runtime_target() else {
            unreachable!("a host builtin calls its own entry");
        };
        if let Some(entry) = self.host_builtin_refs.get(symbol) {
            return Ok(*entry);
        }
        let id = declare_runtime_function(
            self.object,
            symbol,
            &[types::I64, types::I64],
            Some(types::I64),
        )?;
        let entry = self.object.declare_func_in_func(id, self.builder.func);
        self.host_builtin_refs.insert(symbol, entry);
        Ok(entry)
    }

    fn compile_range(&mut self, args: &[MirArg]) -> std::result::Result<ValueRef, String> {
        let int_ty = DirectType::Scalar(ScalarKind::Int64);
        // The shared validator binds one argument to `stop` and two to
        // `start, stop`.
        let (start_arg, stop_arg) = match args {
            [stop] if stop.name.as_deref() != Some("start") => (None, Some(stop)),
            _ => {
                let bound =
                    self.bind_named_runtime_op_args(RuntimeOp::Function(FunctionOp::Range), args)?;
                (bound[0], bound[1])
            }
        };

        let start = if let Some(argument) = start_arg {
            let loaded = self.load_operand(&argument.value)?;
            self.coerce_value(loaded, &int_ty)?
        } else {
            ValueRef {
                values: vec![self.builder.ins().iconst(types::I64, 0)],
                ty: int_ty.clone(),
            }
        };
        let stop_arg = required_named_arg(
            stop_arg,
            "direct backend expected `range()` to receive a `stop` argument",
        )?;
        let stop = self.load_operand(&stop_arg.value)?;
        let stop = self.coerce_value(stop, &int_ty)?;
        let inst = self.builder.ins().call(
            self.runtime[RuntimeFn::RangeNew],
            &[start.values[0], stop.values[0]],
        );
        Ok(self.owned_opaque_result(
            self.builder.inst_results(inst).to_vec(),
            Type::named("Range"),
        ))
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
            .ins()
            .call(self.runtime[RuntimeFn::RangeCurrent], &[range.values[0]]);
        let current = self.builder.inst_results(current_inst)[0];
        let end_inst = self
            .builder
            .ins()
            .call(self.runtime[RuntimeFn::RangeEnd], &[range.values[0]]);
        let end = self.builder.inst_results(end_inst)[0];
        let has_next = self.builder.ins().icmp(IntCC::SignedLessThan, current, end);

        let next_block = self.builder.create_block();
        let body_block = self.compiled_block(body_label)?;
        let exit_block = self.compiled_block(exit_label)?;
        self.builder
            .ins()
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
            .ins()
            .call(self.runtime[RuntimeFn::RangeAdvance], &[range.values[0]]);
        let advanced = self.owned_opaque_result(
            self.builder.inst_results(advanced_inst).to_vec(),
            Type::named("Range"),
        );
        self.store_place(iterable_place, advanced)?;
        self.builder.ins().jump(body_block, &[]);
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

    fn compile_trait_member_call(
        &mut self,
        object: &Operand,
        trait_name: &str,
        field: &str,
        receiver_place: Option<&str>,
        args: &[MirArg],
    ) -> std::result::Result<ValueRef, String> {
        let object = self.load_operand(object)?;
        if let DirectType::Union(union) = &object.ty {
            let union = union.clone();
            return self.compile_union_member_call(
                &union,
                &object.values,
                field,
                receiver_place,
                args,
                Some(trait_name),
            );
        }
        let receiver_ty = direct_type_to_type(&object.ty);
        if let Type::Named(receiver_name, _) = &receiver_ty {
            self.compile_class_member_call(
                receiver_name,
                Some(receiver_ty.clone()),
                object,
                field,
                receiver_place,
                args,
                Some(trait_name),
            )
        } else {
            self.compile_opaque_member_call(
                &receiver_ty,
                object,
                field,
                receiver_place,
                args,
                Some(trait_name),
            )
        }
    }

    fn compile_member_call(
        &mut self,
        object: &Operand,
        callee: MemberCallee<'_>,
        receiver_place: Option<&str>,
        args: &[MirArg],
    ) -> std::result::Result<ValueRef, String> {
        let field = callee.field();
        if let Some(result) = self.compile_borrowed_element_member_call(object, field, args)? {
            return Ok(result);
        }
        if let Some(lent) = self.lendable_receiver(object, field, args)? {
            return self.compile_lent_member_call(lent, callee, receiver_place, args);
        }
        let object = self.load_operand(object)?;
        self.compile_member_callee_on_receiver(object, callee, receiver_place, args)
    }

    fn compile_member_callee_on_receiver(
        &mut self,
        object: ValueRef,
        callee: MemberCallee<'_>,
        receiver_place: Option<&str>,
        args: &[MirArg],
    ) -> std::result::Result<ValueRef, String> {
        match callee {
            MemberCallee::Field(field) => {
                self.compile_member_call_on_receiver(object, field, receiver_place, args)
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
    ) -> std::result::Result<(Value, Value), String> {
        let list = self.load_root(&selection.list)?;
        let index = self.builder.use_var(selection.index);
        Ok((list.values[0], index))
    }

    /// `len()` or `get(key)` on a lent list element, read where it lies (see
    /// `borrowed_list_element`). Returns `None` for any other call.
    fn compile_borrowed_element_member_call(
        &mut self,
        object: &Operand,
        field: &str,
        args: &[MirArg],
    ) -> std::result::Result<Option<ValueRef>, String> {
        let Some(selection) = self.borrowed_list_element(object) else {
            return Ok(None);
        };
        if !crate::mir_copies::borrowed_element_method(&selection.element_type, field, args.len())
            || self
                .find_trait_method(&selection.element_type, field)
                .is_some()
            || args
                .iter()
                .any(|argument| argument.name.is_some() || argument.writeback_place.is_some())
        {
            return Ok(None);
        }
        let (line, column) = self.span_operands(selection.span);
        match (field, args, &selection.element_type) {
            ("get", [key], Type::Named(_, entry_types)) => {
                let inst = if let Some(text) = str_literal_key(&key.value, &entry_types[0]) {
                    // A literal `str` key is looked up without a box.
                    let (key_ptr, key_len) = self.string_constant(text.as_bytes())?;
                    let (list, index) = self.load_borrowed_list_element(&selection)?;
                    self.builder.ins().call(
                        self.runtime[RuntimeFn::VecElementMapGetStr],
                        &[list, index, key_ptr, key_len, line, column],
                    )
                } else {
                    let key_direct_ty =
                        ensure_direct_type(&entry_types[0], &self.classes, "dict key")?;
                    let key = self.load_operand_as_opaque_direct(&key.value, &key_direct_ty)?;
                    let (list, index) = self.load_borrowed_list_element(&selection)?;
                    self.builder.ins().call(
                        self.runtime[RuntimeFn::VecElementMapGet],
                        &[list, index, key.values[0], line, column],
                    )
                };
                Ok(Some(self.owned_opaque_result(
                    self.builder.inst_results(inst).to_vec(),
                    optional_type(entry_types[1].clone()),
                )))
            }
            _ => {
                let (list, index) = self.load_borrowed_list_element(&selection)?;
                let inst = self.builder.ins().call(
                    self.runtime[RuntimeFn::VecElementLen],
                    &[list, index, line, column],
                );
                Ok(Some(ValueRef {
                    values: vec![self.builder.inst_results(inst)[0]],
                    ty: DirectType::Scalar(ScalarKind::Int64),
                }))
            }
        }
    }

    /// Decides whether a member call's receiver can be lent out of its
    /// owner instead of cloned. A collection field such as `self.docs`, or
    /// an element or entry reached through a loan such as the `bucket` of
    /// `match self.buckets.lookup(name)`, otherwise costs a deep clone of
    /// the whole receiver on every call. Lending moves the receiver out of
    /// its slot for the one call and moves it back afterwards, so the call
    /// sees the same value without a copy (execution model: no backend
    /// inserts a hidden deep clone).
    ///
    /// The slot is empty while the call runs, so lending is refused when
    /// anything else the call can reach might read it: an argument rooted at
    /// the same place, an argument whose type can hold a class instance or
    /// a callable (it could alias the owner), or a mutable argument.
    fn lendable_receiver(
        &self,
        object: &Operand,
        field: &str,
        args: &[MirArg],
    ) -> std::result::Result<Option<LentReceiver>, String> {
        let Operand::Place(place) = object else {
            return Ok(None);
        };
        if !is_plain_field_path(place) {
            return Ok(None);
        }
        let Ok(resolved) = self.resolve_view_place(place) else {
            return Ok(None);
        };
        let [alternative] = resolved.alternatives.as_slice() else {
            return Ok(None);
        };
        if !alternative.conditions.is_empty() || !alternative.union_payloads.is_empty() {
            return Ok(None);
        }
        let root = alternative
            .place
            .split_once('.')
            .map_or(alternative.place.as_str(), |(root, _)| root);
        let Some(DirectType::Opaque(root_ty)) = self.variable_types.get(root) else {
            return Ok(None);
        };
        if alternative.elements.is_empty() {
            // A plain field place: the owner must be a class instance, so
            // the field can be moved out of it by name.
            let is_instance =
                matches!(root_ty, Type::Named(name, _) if self.classes.contains_key(name));
            if !is_instance
                || !alternative.place.contains('.')
                || !is_plain_field_path(&alternative.place)
            {
                return Ok(None);
            }
        }
        let Ok(DirectType::Opaque(receiver_ty)) = self.type_of_place(place) else {
            return Ok(None);
        };
        let Type::Named(receiver_name, _) = &receiver_ty else {
            return Ok(None);
        };
        let kind = if matches!(receiver_name.as_str(), "list" | "dict" | "set") {
            if field == "__take_index_option"
                || field.starts_with("__span_")
                || self.find_trait_method(&receiver_ty, field).is_some()
            {
                return Ok(None);
            }
            LentReceiverKind::Shared
        } else if !alternative.elements.is_empty() && self.classes.contains_key(receiver_name) {
            match find_method(self.classes.get(receiver_name), field).map(|method| method.receiver)
            {
                Some(Some(MirReceiverKind::Borrow)) => LentReceiverKind::Shared,
                Some(Some(MirReceiverKind::BorrowMut)) => LentReceiverKind::MutableWriteback,
                _ => return Ok(None),
            }
        } else {
            return Ok(None);
        };
        for argument in args {
            if argument.writeback_place.is_some() {
                return Ok(None);
            }
            let (Operand::Place(argument_place) | Operand::MovePlace(argument_place)) =
                &argument.value
            else {
                continue;
            };
            let Ok(argument_resolved) = self.resolve_view_place(argument_place) else {
                return Ok(None);
            };
            let overlaps = argument_resolved.alternatives.iter().any(|candidate| {
                places_overlap(&candidate.place, &alternative.place)
                    || projects_from_other_root(
                        &alternative.place,
                        &candidate.place,
                        !candidate.elements.is_empty(),
                    )
            });
            if overlaps {
                return Ok(None);
            }
            let Ok(argument_ty) = self.type_of_place(argument_place) else {
                return Ok(None);
            };
            if self.type_may_alias_instance(&direct_type_to_type(&argument_ty)) {
                return Ok(None);
            }
        }
        Ok(Some(LentReceiver {
            place: place.clone(),
            alternative: alternative.clone(),
            kind,
        }))
    }

    /// Whether a value of `ty` can hold a handle to a class instance or a
    /// callable environment, and so might alias a lent receiver's owner.
    fn type_may_alias_instance(&self, ty: &Type) -> bool {
        match ty {
            Type::Named(name, args) => {
                self.classes.contains_key(name)
                    || args.iter().any(|arg| self.type_may_alias_instance(arg))
            }
            Type::Tuple(items) => items.iter().any(|item| self.type_may_alias_instance(item)),
            Type::Union(union) => union
                .members
                .iter()
                .any(|member| self.type_may_alias_instance(member)),
            Type::Unit | Type::Module(_) => false,
            _ => true,
        }
    }

    /// Moves a lent receiver out of its slot (see `lendable_receiver`). The
    /// result is an owned temporary.
    fn take_lent_receiver(&mut self, lent: &LentReceiver) -> std::result::Result<ValueRef, String> {
        if lent.alternative.elements.is_empty() {
            return self.take_place(&lent.alternative.place);
        }
        let element_type = lent
            .alternative
            .elements
            .last()
            .map(|selector| selector.element_type.clone())
            .ok_or("direct backend lent an element view without a selection")?;
        let (collection, projection) = self.load_element_path_root(&lent.alternative.place)?;
        let path = self.element_path_operands(&projection, &lent.alternative.elements)?;
        let inst = self.builder.ins().call(
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
        Ok(self.owned_opaque_result(self.builder.inst_results(inst).to_vec(), element_type))
    }

    fn compile_lent_member_call(
        &mut self,
        lent: LentReceiver,
        callee: MemberCallee<'_>,
        receiver_place: Option<&str>,
        args: &[MirArg],
    ) -> std::result::Result<ValueRef, String> {
        let receiver = self.take_lent_receiver(&lent)?;
        match lent.kind {
            LentReceiverKind::Shared => {
                // The call borrows the receiver; ownership stays here so it
                // can be moved back into its slot afterwards. Without a
                // writeback place, a mutating builtin changes the lent value
                // in place and the move back publishes the change.
                self.clear_temporary_opaque_owned(&receiver);
                let result =
                    self.compile_member_callee_on_receiver(receiver.clone(), callee, None, args)?;
                self.mark_temporary_opaque_owned(&receiver);
                self.store_place(&lent.place, receiver)?;
                Ok(result)
            }
            // A `mut self` method's writeback stores the receiver back.
            LentReceiverKind::MutableWriteback => {
                self.compile_member_callee_on_receiver(receiver, callee, receiver_place, args)
            }
        }
    }

    fn compile_member_call_on_receiver(
        &mut self,
        object: ValueRef,
        field: &str,
        receiver_place: Option<&str>,
        args: &[MirArg],
    ) -> std::result::Result<ValueRef, String> {
        if object.ty.scalar_kind().is_some()
            && matches!(field, "add" | "sub" | "mul" | "div")
            && args.len() == 1
        {
            let right = self.load_operand(&args[0].value)?;
            if let Some(element_type) = direct_array_element_type(&right.ty).cloned() {
                let element_direct =
                    ensure_direct_type(&element_type, &self.classes, "Array scalar operand")?;
                let left = self.coerce_value(object, &element_direct)?;
                let left = self.ensure_opaque(left)?;
                let array_ty = direct_type_to_type(&right.ty);
                let right = self.ensure_opaque(right)?;
                let scalar_left = self.builder.ins().iconst(types::I64, 1);
                let operation_code = match field {
                    "add" => 0,
                    "sub" => 1,
                    "mul" => 2,
                    _ => {
                        debug_assert_eq!(field, "div");
                        3
                    }
                };
                let operation = self.builder.ins().iconst(types::I64, operation_code);
                let checked_mode = self.builder.ins().iconst(types::I64, 0);
                let zero = self.builder.ins().iconst(types::I64, 0);
                let inst = self.builder.ins().call(
                    self.runtime[RuntimeFn::ArrayBinary],
                    &[
                        left.values[0],
                        right.values[0],
                        scalar_left,
                        operation,
                        checked_mode,
                        zero,
                        zero,
                    ],
                );
                return Ok(
                    self.owned_opaque_result(self.builder.inst_results(inst).to_vec(), array_ty)
                );
            }
        }

        match object.ty.clone() {
            DirectType::Callable(_) => Err(format!(
                "direct backend cannot call `.{field}` on a callable value"
            )),
            DirectType::Union(union) => self.compile_union_member_call(
                &union,
                &object.values,
                field,
                receiver_place,
                args,
                None,
            ),
            DirectType::PlainClass(class_ty) => self.compile_class_member_call(
                class_ty.class_name.as_str(),
                Some(Type::named(&class_ty.class_name)),
                object,
                field,
                receiver_place,
                args,
                None,
            ),
            DirectType::Opaque(ty) => {
                if let Type::Named(_name, _type_args) = &ty {
                    return self.compile_opaque_member_call(
                        &ty,
                        object,
                        field,
                        receiver_place,
                        args,
                        None,
                    );
                }
                self.compile_opaque_member_call(&ty, object, field, receiver_place, args, None)
            }
            DirectType::Scalar(_) => {
                let receiver_ty = direct_type_to_type(&object.ty);
                if let Some(op) = RuntimeOp::resolve_member(&receiver_ty, field, |_| false) {
                    return self.compile_runtime_op(op, object, receiver_place, args);
                }
                if self.find_trait_method(&receiver_ty, field).is_some() {
                    return self.compile_class_member_call(
                        &receiver_ty.to_string(),
                        Some(receiver_ty),
                        object,
                        field,
                        receiver_place,
                        args,
                        None,
                    );
                }
                Err(format!(
                    "direct backend does not support member call `.{}` on `{}`",
                    field,
                    render_direct_type(&object.ty)
                ))
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
                        values.extend(field.ty.zero_values(&mut self.builder));
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
                let global = self
                    .object
                    .declare_data_in_func(shape.descriptor, self.builder.func);
                let descriptor = self.builder.ins().symbol_value(types::I64, global);
                let zero = self.builder.ins().iconst(types::I64, 0);
                Ok(ValueRef {
                    values: vec![descriptor, zero, zero, zero],
                    ty: DirectType::Callable(DirectCallableType::new(signature.as_ref().clone())),
                })
            }
            Operand::Int(value) => {
                if let Ok(narrowed) = i64::try_from(*value) {
                    return Ok(ValueRef {
                        values: vec![self.builder.ins().iconst(types::I64, narrowed)],
                        ty: DirectType::Scalar(ScalarKind::Int64),
                    });
                }
                let (ptr, len) = self.string_constant(value.to_string().as_bytes())?;
                let inst = self
                    .builder
                    .ins()
                    .call(self.runtime[RuntimeFn::BoxUintLiteral], &[ptr, len]);
                Ok(self.owned_opaque_result(
                    self.builder.inst_results(inst).to_vec(),
                    Type::named("Unknown"),
                ))
            }
            Operand::Float(value) => Ok(ValueRef {
                values: vec![self.builder.ins().f64const(Ieee64::with_float(*value))],
                ty: DirectType::Scalar(ScalarKind::Float64),
            }),
            Operand::String(value) => {
                let (ptr, len) = self.string_constant(value.as_bytes())?;
                let inst = self
                    .builder
                    .ins()
                    .call(self.runtime[RuntimeFn::StringLiteral], &[ptr, len]);
                Ok(self.owned_opaque_result(
                    self.builder.inst_results(inst).to_vec(),
                    Type::named("str"),
                ))
            }
            Operand::Duration(value) => {
                let low = self.builder.ins().iconst(types::I64, *value as i64);
                let high = self.builder.ins().iconst(types::I64, (*value >> 64) as i64);
                let inst = self
                    .builder
                    .ins()
                    .call(self.runtime[RuntimeFn::DurationLiteral], &[low, high]);
                Ok(self.owned_opaque_result(
                    self.builder.inst_results(inst).to_vec(),
                    Type::named("Duration"),
                ))
            }
            Operand::Bool(value) => Ok(ValueRef {
                values: vec![self
                    .builder
                    .ins()
                    .iconst(types::I64, if *value { 1 } else { 0 })],
                ty: DirectType::Scalar(ScalarKind::Bool),
            }),
            Operand::Unit => Ok(unit_value(&mut self.builder)),
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
            | Operand::Unit => return None,
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
                    values: vec![self.builder.ins().iconst(types::I64, raw)],
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
    /// collection's view place gains the selection.
    fn begin_element_loan(
        &mut self,
        loan: &str,
        source: &str,
        selector: &Operand,
        projection: &str,
        span: Span,
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
        let variable = Variable::from_u32(self.next_variable_index as u32);
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
            if self.proven_access == ProvenAccess::Lend {
                return Ok(());
            }
            let (list, index) = self.load_borrowed_list_element(&selection)?;
            let (line, column) = self.span_operands(selection.span);
            self.builder.ins().call(
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
                let inst = self.builder.ins().call(
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
                self.release_opaque_handle(self.builder.inst_results(inst)[0]);
                return Ok(());
            }
        }
        let baseline = self.owned_opaque_temporaries.clone();
        let probe = self.load_place(loan)?;
        drop(probe);
        self.release_temporary_owned_since(&baseline);
        Ok(())
    }

    /// A span as the line and column words the runtime's indexing helpers
    /// take.
    fn span_operands(&mut self, span: Span) -> (Value, Value) {
        let line = self.builder.ins().iconst(types::I64, span.line as i64);
        let column = self.builder.ins().iconst(types::I64, span.column as i64);
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
        let inst = self.builder.ins().call(
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
        Ok(self.owned_opaque_result(self.builder.inst_results(inst).to_vec(), element_type))
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
        let result = self.builder.ins().call(
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
        self.release_opaque_handle(self.builder.inst_results(result)[0]);
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
        let slot = self.builder.create_sized_stack_slot(StackSlotData::new(
            StackSlotKind::ExplicitSlot,
            (8 * elements.len()) as u32,
            3,
        ));
        let selectors_ptr = self.builder.ins().stack_addr(types::I64, slot, 0);
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
                .ins()
                .store(MemFlags::new(), word, selectors_ptr, (index as i32) * 8);
        }
        let count = self.builder.ins().iconst(types::I64, elements.len() as i64);
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

    fn view_alternative_condition(&mut self, alternative: &DirectViewAlternative) -> Value {
        let mut conditions = alternative.conditions.iter();
        let Some((selector, expected)) = conditions.next() else {
            return self.builder.ins().iconst(types::I64, 1);
        };
        let selector = self.builder.use_var(*selector);
        let mut matches = self
            .builder
            .ins()
            .icmp_imm(IntCC::Equal, selector, *expected);
        for (selector, expected) in conditions {
            let selector = self.builder.use_var(*selector);
            let next = self
                .builder
                .ins()
                .icmp_imm(IntCC::Equal, selector, *expected);
            matches = self.builder.ins().band(matches, next);
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
        let mut alternatives =
            Vec::<(String, Vec<(Variable, i64)>, Vec<DirectElementSelector>)>::new();
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
                self.builder
                    .ins()
                    .brif(selected, set_block, &[], next_block, &[]);
                self.builder.switch_to_block(set_block);
                self.set_returned_view_projection_value(&projection)?;
                self.push_returned_view_selectors(&steps);
                self.builder.ins().jump(merge, &[]);
                self.builder.seal_block(set_block);
                self.builder.switch_to_block(next_block);
                self.builder.seal_block(next_block);
            } else {
                self.set_returned_view_projection_value(&projection)?;
                self.push_returned_view_selectors(&steps);
                self.builder.ins().jump(merge, &[]);
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
                let word_ty = self.builder.func.dfg.value_type(word);
                if word_ty == types::F64 {
                    word = self
                        .builder
                        .ins()
                        .bitcast(types::I64, MemFlags::new(), word);
                } else if word_ty == types::F32 {
                    word = self
                        .builder
                        .ins()
                        .bitcast(types::I32, MemFlags::new(), word);
                }
                if self.builder.func.dfg.value_type(word) != types::I64 {
                    word = self.builder.ins().uextend(types::I64, word);
                }
            }
            self.builder
                .ins()
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
        conditions: Vec<(Variable, i64)>,
        selector_words: &[Value],
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
                self.builder
                    .ins()
                    .bitcast(types::F64, MemFlags::new(), word)
            } else if abi == types::F32 {
                let bits = self.builder.ins().ireduce(types::I32, word);
                self.builder
                    .ins()
                    .bitcast(types::F32, MemFlags::new(), bits)
            } else {
                self.builder.ins().ireduce(abi, word)
            };
            let variable = Variable::from_u32(self.next_variable_index as u32);
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
        self.builder.ins().call(
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

        let caller_owned = self.owned_opaque_temporaries.clone();
        let alternative_count = resolved.alternatives.len();
        let mut merged_owned = None;
        for (index, alternative) in resolved.alternatives.into_iter().enumerate() {
            if index + 1 < alternative_count {
                let selected = self.view_alternative_condition(&alternative);
                let load_block = self.builder.create_block();
                let next_block = self.builder.create_block();
                self.builder
                    .ins()
                    .brif(selected, load_block, &[], next_block, &[]);
                self.builder.switch_to_block(load_block);
                self.owned_opaque_temporaries = caller_owned.clone();
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
                self.builder.ins().jump(merge, &value.values);
                self.builder.seal_block(load_block);
                self.builder.switch_to_block(next_block);
                self.builder.seal_block(next_block);
            } else {
                self.owned_opaque_temporaries = caller_owned.clone();
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
                self.builder.ins().jump(merge, &value.values);
            }
        }
        self.builder.switch_to_block(merge);
        self.builder.seal_block(merge);
        self.owned_opaque_temporaries = caller_owned;
        let value = ValueRef {
            values: self.builder.block_params(merge).to_vec(),
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
                    self.builder.ins().f64const(Ieee64::with_float(0.0))
                } else {
                    self.builder.ins().iconst(abi, 0)
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
            let inst = self.builder.ins().call(
                self.runtime[RuntimeFn::InstanceTakeField],
                &[root_value.values[0], field_ptr, field_len],
            );
            let moved = ValueRef {
                values: self.builder.inst_results(inst).to_vec(),
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
                self.builder.ins().f64const(Ieee64::with_float(0.0))
            } else {
                self.builder.ins().iconst(abi, 0)
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
                let index_value = self.builder.ins().iconst(types::I64, index as i64);
                let inst = self.builder.ins().call(
                    self.runtime[RuntimeFn::TupleElement],
                    &[object.values[0], index_value],
                );
                let element = self.owned_opaque_result(
                    self.builder.inst_results(inst).to_vec(),
                    element_type.clone(),
                );
                let direct_type =
                    ensure_direct_type(&element_type, &self.classes, "tuple view element")?;
                self.coerce_value(element, &direct_type)
            }
            DirectType::Opaque(_) => {
                let (field_ptr, field_len) = self.string_constant(field.as_bytes())?;
                let inst = self.builder.ins().call(
                    self.runtime[RuntimeFn::InstanceGetField],
                    &[object.values[0], field_ptr, field_len],
                );
                let loaded = ValueRef {
                    values: self.builder.inst_results(inst).to_vec(),
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
                values: target.zero_values(&mut self.builder),
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
                let inst = self.builder.ins().call(
                    self.runtime[RuntimeFn::CastValue],
                    &[boxed.values[0], target_ptr, target_len, line, column],
                );
                let casted = self.owned_opaque_result(
                    self.builder.inst_results(inst).to_vec(),
                    direct_type_to_type(target),
                );
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
                        .ins()
                        .call(self.runtime[RuntimeFn::UnboxI64], &[value.values[0]]);
                    ValueRef {
                        values: self.builder.inst_results(inst).to_vec(),
                        ty: target.clone(),
                    }
                }
                DirectType::Scalar(ScalarKind::Int64) => {
                    let inst = self
                        .builder
                        .ins()
                        .call(self.runtime[RuntimeFn::UnboxInt64], &[value.values[0]]);
                    ValueRef {
                        values: self.builder.inst_results(inst).to_vec(),
                        ty: target.clone(),
                    }
                }
                DirectType::Scalar(ScalarKind::Uint64) => {
                    let inst = self
                        .builder
                        .ins()
                        .call(self.runtime[RuntimeFn::UnboxU64], &[value.values[0]]);
                    ValueRef {
                        values: self.builder.inst_results(inst).to_vec(),
                        ty: target.clone(),
                    }
                }
                DirectType::Scalar(ScalarKind::Float32)
                | DirectType::Scalar(ScalarKind::Float64) => {
                    let inst = self
                        .builder
                        .ins()
                        .call(self.runtime[RuntimeFn::UnboxF64], &[value.values[0]]);
                    self.normalize_scalar_value(ValueRef {
                        values: self.builder.inst_results(inst).to_vec(),
                        ty: target.clone(),
                    })?
                }
                DirectType::Scalar(ScalarKind::Bool) => {
                    let inst = self
                        .builder
                        .ins()
                        .call(self.runtime[RuntimeFn::UnboxBool], &[value.values[0]]);
                    ValueRef {
                        values: self.builder.inst_results(inst).to_vec(),
                        ty: target.clone(),
                    }
                }
                DirectType::Scalar(ScalarKind::Unit) => unit_value(&mut self.builder),
                DirectType::Union(union) => {
                    let union = union.clone();
                    self.unbox_inline_union(&union, value.values[0])?
                }
                DirectType::PlainClass(class) => {
                    let mut values = Vec::new();
                    for field in &class.fields {
                        let (field_ptr, field_len) = self.string_constant(field.name.as_bytes())?;
                        let inst = self.builder.ins().call(
                            self.runtime[RuntimeFn::InstanceGetField],
                            &[value.values[0], field_ptr, field_len],
                        );
                        let field_value = ValueRef {
                            values: self.builder.inst_results(inst).to_vec(),
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
                values: vec![self.builder.ins().iconst(types::I64, 0)],
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
                let narrowed = self.builder.ins().fdemote(types::F32, value.values[0]);
                let widened = self.builder.ins().fpromote(types::F64, narrowed);
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
    fn emit_runtime_type_tag(&mut self, raw: Value, ty: &Type) -> std::result::Result<(), String> {
        let encoded = crate::native_runtime::canonical_runtime_type_name(ty);
        if let Some((inst, tagged, tagged_type)) = &self.last_runtime_type_tag {
            if *tagged == raw && *tagged_type == encoded && self.no_call_since(*inst) {
                return Ok(());
            }
        }
        let (type_ptr, type_len) = self.string_constant(encoded.as_bytes())?;
        let inst = self.builder.ins().call(
            self.runtime[RuntimeFn::TagValueType],
            &[raw, type_ptr, type_len],
        );
        self.last_runtime_type_tag = Some((inst, raw, encoded));
        Ok(())
    }

    /// Whether `inst` is in the current block and no call that could change
    /// a box follows it there. A retain or a release changes only the count
    /// of the value it names, and a tag of `raw` is never followed by a
    /// release of `raw` that frees it.
    fn no_call_since(&self, inst: cranelift_codegen::ir::Inst) -> bool {
        let Some(block) = self.builder.current_block() else {
            return false;
        };
        let func = &self.builder.func;
        if func.layout.inst_block(inst) != Some(block) {
            return false;
        }
        let mut cursor = func.layout.last_inst(block);
        while let Some(current) = cursor {
            if current == inst {
                return true;
            }
            match func.dfg.insts[current] {
                cranelift_codegen::ir::InstructionData::Call { func_ref, .. }
                    if func_ref == self.runtime[RuntimeFn::RetainValue]
                        || func_ref == self.runtime[RuntimeFn::ReleaseValue] => {}
                ref data if data.opcode().is_call() => return false,
                _ => {}
            }
            cursor = func.layout.prev_inst(current);
        }
        false
    }

    fn tag_raw_opaque_runtime_type(
        &mut self,
        raw: Value,
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
    fn integer_constant(&self, value: Value) -> Option<i64> {
        let dfg = &self.builder.func.dfg;
        let cranelift_codegen::ir::ValueDef::Result(inst, _) = dfg.value_def(value) else {
            return None;
        };
        match dfg.insts[inst] {
            cranelift_codegen::ir::InstructionData::UnaryImm {
                opcode: cranelift_codegen::ir::Opcode::Iconst,
                imm,
            } => Some(imm.bits()),
            _ => None,
        }
    }

    fn emit_int32_bounds_check(
        &mut self,
        value: Value,
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
        let biased = self.builder.ins().iadd_imm(value, -(i32::MIN as i64));
        let overflow =
            self.builder
                .ins()
                .icmp_imm(IntCC::UnsignedGreaterThan, biased, u32::MAX as i64);
        let fail_block = self.builder.create_block();
        let continue_block = self.builder.create_block();
        self.builder
            .ins()
            .brif(overflow, fail_block, &[], continue_block, &[]);
        self.builder.switch_to_block(fail_block);
        self.emit_pending_cleanups(true)?;
        let (line, column) = self.span_values(span);
        self.builder.ins().call(
            self.runtime[RuntimeFn::FailInt32Overflow],
            &[value, line, column],
        );
        self.builder.ins().trap(TrapCode::INTEGER_OVERFLOW);
        self.builder.seal_block(fail_block);
        self.builder.switch_to_block(continue_block);
        self.builder.seal_block(continue_block);
        Ok(())
    }

    /// The length of `list`, read from its list view when the value is
    /// confined to this thread (see `aura_runtime::native_runtime::DirectListLayout`).
    fn emit_list_len(&mut self, list: Value) -> Value {
        let layout = aura_runtime::native_runtime::direct_list_layout();
        let slow_block = self.builder.create_block();
        let fast_block = self.builder.create_block();
        let join_block = self.builder.create_block();
        self.builder.append_block_param(join_block, types::I64);
        // Every list form keeps its length in the view.
        let access = self.emit_list_access(list, &layout);
        let form = self.builder.ins().iadd_imm(
            access,
            -(aura_runtime::native_runtime::DirectListAccess::Values as i64),
        );
        let is_list = self.builder.ins().icmp_imm(
            IntCC::UnsignedLessThan,
            form,
            i64::from(aura_runtime::native_runtime::DirectListAccess::LIST_FORMS),
        );
        self.builder
            .ins()
            .brif(is_list, fast_block, &[], slow_block, &[]);

        self.builder.switch_to_block(fast_block);
        self.builder.seal_block(fast_block);
        let len = self
            .builder
            .ins()
            .load(types::I64, MemFlags::trusted(), list, layout.len_offset);
        self.builder.ins().jump(join_block, &[len]);

        self.builder.switch_to_block(slow_block);
        self.builder.seal_block(slow_block);
        let inst = self
            .builder
            .ins()
            .call(self.runtime[RuntimeFn::VecLen], &[list]);
        let len = self.builder.inst_results(inst)[0];
        self.builder.ins().jump(join_block, &[len]);

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
        list: Value,
        layout: &aura_runtime::native_runtime::DirectListLayout,
    ) -> Value {
        self.builder
            .ins()
            .uload32(MemFlags::trusted(), list, layout.access_offset)
    }

    /// Branches to `fast_block` when the list view of `list` has the layout
    /// `access` names, which also means the value is confined to this
    /// thread, and to `slow_block` otherwise.
    fn emit_list_access_check(
        &mut self,
        list: Value,
        access: aura_runtime::native_runtime::DirectListAccess,
        layout: &aura_runtime::native_runtime::DirectListLayout,
        slow_block: cranelift_codegen::ir::Block,
        fast_block: cranelift_codegen::ir::Block,
    ) {
        let actual = self.emit_list_access(list, layout);
        let matches = self
            .builder
            .ins()
            .icmp_imm(IntCC::Equal, actual, access as i64);
        self.builder
            .ins()
            .brif(matches, fast_block, &[], slow_block, &[]);
    }

    /// Reads one element of an `Array` indexed by one `int64`. A confined
    /// rank-1 array in range is read in place from its list view. Any other
    /// array or position takes the runtime call, which reports the rank or
    /// range diagnostic.
    fn emit_array_element_read(
        &mut self,
        array: Value,
        coordinate: ValueRef,
        kind: ScalarKind,
        element_ty: Type,
        line: Value,
        column: Value,
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
            .ins()
            .load(types::I64, flags, array, layout.elements_offset);
        let position =
            self.emit_list_view_bounds_check(array, coordinate.values[0], &layout, slow_block);
        let offset = self.builder.ins().imul_imm(position, size);
        let element = self.builder.ins().iadd(elements, offset);
        let value = match kind {
            ScalarKind::Int32 => self.builder.ins().sload32(flags, element, 0),
            ScalarKind::Float32 => {
                let narrow = self.builder.ins().load(load_ty, flags, element, 0);
                self.builder.ins().fpromote(types::F64, narrow)
            }
            _ => self.builder.ins().load(load_ty, flags, element, 0),
        };
        self.builder.ins().jump(join_block, &[value]);

        self.builder.switch_to_block(slow_block);
        self.builder.seal_block(slow_block);
        let caller_owned = self.owned_opaque_temporaries.clone();
        let coordinates = self.ensure_opaque(coordinate)?;
        let inst = self.builder.ins().call(
            self.runtime[RuntimeFn::ArrayIndex],
            &[array, coordinates.values[0], line, column],
        );
        let result = self.owned_opaque_result(self.builder.inst_results(inst).to_vec(), element_ty);
        let result = self.coerce_value(result, &element_direct_ty)?;
        self.release_temporary_owned_since(&caller_owned);
        self.builder.ins().jump(join_block, &[result.values[0]]);
        self.owned_opaque_temporaries = caller_owned;

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
        list: Value,
        index: Value,
        kind: i64,
        proven: bool,
        line: Value,
        column: Value,
    ) -> std::result::Result<Value, String> {
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
            .ins()
            .load(types::I64, flags, list, layout.elements_offset);
        let position = if proven {
            index
        } else {
            self.emit_list_view_bounds_check(list, index, &layout, slow_block)
        };
        let offset = if stride == 1 {
            position
        } else {
            self.builder.ins().imul_imm(position, stride)
        };
        let element = self.builder.ins().iadd(elements, offset);
        let bits = if stride == 1 {
            self.builder.ins().uload8(types::I64, flags, element, 0)
        } else {
            self.builder.ins().load(types::I64, flags, element, 0)
        };
        self.builder.ins().jump(join_block, &[bits]);

        self.builder.switch_to_block(slow_block);
        self.builder.seal_block(slow_block);
        let normalized = self.emit_vec_index_failure_guard(list, index, line, column)?;
        let kind = self.builder.ins().iconst(types::I64, kind);
        let inst = self.builder.ins().call(
            self.runtime[RuntimeFn::VecIndexScalar],
            &[list, normalized, kind],
        );
        let bits = self.builder.inst_results(inst)[0];
        self.builder.ins().jump(join_block, &[bits]);

        self.builder.switch_to_block(join_block);
        self.builder.seal_block(join_block);
        Ok(self.builder.block_params(join_block)[0])
    }

    /// Normalizes a negative `index` against a list view's length, and
    /// continues in a new block when the position is in range or in `slow`
    /// when it is not.
    fn emit_list_view_bounds_check(
        &mut self,
        list: Value,
        index: Value,
        layout: &aura_runtime::native_runtime::DirectListLayout,
        slow: cranelift_codegen::ir::Block,
    ) -> Value {
        let len = self
            .builder
            .ins()
            .load(types::I64, MemFlags::trusted(), list, layout.len_offset);
        let zero = self.builder.ins().iconst(types::I64, 0);
        let negative = self.builder.ins().icmp(IntCC::SignedLessThan, index, zero);
        let from_end = self.builder.ins().iadd(len, index);
        let position = self.builder.ins().select(negative, from_end, index);
        // One unsigned compare also rejects a position below zero.
        let out_of_range =
            self.builder
                .ins()
                .icmp(IntCC::UnsignedGreaterThanOrEqual, position, len);
        let in_range = self.builder.create_block();
        self.builder
            .ins()
            .brif(out_of_range, slow, &[], in_range, &[]);
        self.builder.switch_to_block(in_range);
        self.builder.seal_block(in_range);
        position
    }

    fn emit_vec_index_failure_guard(
        &mut self,
        object: Value,
        index: Value,
        line: Value,
        column: Value,
    ) -> std::result::Result<Value, String> {
        let len_inst = self
            .builder
            .ins()
            .call(self.runtime[RuntimeFn::VecLen], &[object]);
        let len = self.builder.inst_results(len_inst)[0];
        let zero = self.builder.ins().iconst(types::I64, 0);
        let negative = self.builder.ins().icmp(IntCC::SignedLessThan, index, zero);
        let from_end = self.builder.ins().iadd(len, index);
        let normalized = self.builder.ins().select(negative, from_end, index);
        let below = self
            .builder
            .ins()
            .icmp(IntCC::SignedLessThan, normalized, zero);
        let above = self
            .builder
            .ins()
            .icmp(IntCC::SignedGreaterThanOrEqual, normalized, len);
        let out_of_bounds = self.builder.ins().bor(below, above);
        let fail_block = self.builder.create_block();
        let continue_block = self.builder.create_block();
        self.builder
            .ins()
            .brif(out_of_bounds, fail_block, &[], continue_block, &[]);
        self.builder.switch_to_block(fail_block);
        self.emit_pending_cleanups(true)?;
        self.builder.ins().call(
            self.runtime[RuntimeFn::VecIndex],
            &[object, index, line, column],
        );
        self.builder.ins().trap(TrapCode::unwrap_user(2));
        self.builder.seal_block(fail_block);
        self.builder.switch_to_block(continue_block);
        self.builder.seal_block(continue_block);
        Ok(normalized)
    }

    fn as_bool_value(&mut self, value: ValueRef) -> std::result::Result<Value, String> {
        match value.ty.scalar_kind() {
            Some(ScalarKind::Bool) | Some(ScalarKind::Int32) | Some(ScalarKind::Unit) => {
                let zero = self.builder.ins().iconst(types::I64, 0);
                Ok(self
                    .builder
                    .ins()
                    .icmp(IntCC::NotEqual, value.values[0], zero))
            }
            None if matches!(value.ty, DirectType::Opaque(_)) => {
                let inst = self.builder.ins().call(
                    self.runtime[RuntimeFn::ValueAsCondition],
                    &[value.values[0]],
                );
                Ok(self.builder.inst_results(inst)[0])
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
            self.builder.ins().call(
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
        let caller_owned = self.owned_opaque_temporaries.clone();
        let value_was_owned = self.temporary_owns_opaque(&value);
        let alternative_count = resolved.alternatives.len();
        for (index, alternative) in resolved.alternatives.into_iter().enumerate() {
            if index + 1 < alternative_count {
                let selected = self.view_alternative_condition(&alternative);
                let store_block = self.builder.create_block();
                let next_block = self.builder.create_block();
                self.builder
                    .ins()
                    .brif(selected, store_block, &[], next_block, &[]);
                self.builder.switch_to_block(store_block);
                self.owned_opaque_temporaries = caller_owned.clone();
                self.store_alternative(&alternative, value.clone())?;
                self.release_temporary_owned_since(&caller_owned);
                self.builder.ins().jump(merge, &[]);
                self.builder.seal_block(store_block);
                self.builder.switch_to_block(next_block);
                self.builder.seal_block(next_block);
            } else {
                self.owned_opaque_temporaries = caller_owned.clone();
                self.store_alternative(&alternative, value.clone())?;
                self.release_temporary_owned_since(&caller_owned);
                self.builder.ins().jump(merge, &[]);
            }
        }
        self.builder.switch_to_block(merge);
        self.builder.seal_block(merge);
        self.owned_opaque_temporaries = caller_owned;
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
                self.builder.ins().call(
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

    fn store_root(&mut self, name: &str, value: ValueRef) -> std::result::Result<(), String> {
        let expected = self.local_type(name)?;
        let value = self.coerce_value(value, &expected)?;
        let vars = self.local_vars(name)?;
        if matches!(&expected, DirectType::Union(union) if union.owns_handles()) {
            let stored = self.transfer_union_words(&value);
            self.release_root_if_opaque(name)?;
            for (var, compiled) in vars.into_iter().zip(stored) {
                self.builder.def_var(var, compiled);
            }
            return Ok(());
        }
        if matches!(expected, DirectType::Callable(_)) {
            let stored = self.transfer_callable_words(&value);
            self.release_root_if_opaque(name)?;
            for (var, compiled) in vars.into_iter().zip(stored) {
                self.builder.def_var(var, compiled);
            }
            return Ok(());
        }
        if matches!(expected, DirectType::Opaque(_)) {
            let stored = if self.temporary_owns_opaque(&value) {
                self.clear_temporary_opaque_owned(&value);
                value.values[0]
            } else {
                self.retain_opaque_handle(value.values[0])
            };
            self.release_root_if_opaque(name)?;
            self.builder.def_var(vars[0], stored);
            return Ok(());
        }
        for (var, compiled) in vars.into_iter().zip(value.values) {
            self.builder.def_var(var, compiled);
        }
        Ok(())
    }

    /// The direct result type of `.field` on an inline union receiver,
    /// taken from the first member that declares it.
    fn union_member_call_result_type(
        &self,
        union: &DirectUnionType,
        field: &str,
        trait_name: Option<&str>,
    ) -> Option<DirectType> {
        union.members.iter().find_map(|member| {
            let class_name = match &member.ty {
                DirectType::PlainClass(class_ty) => class_ty.class_name.clone(),
                DirectType::Opaque(Type::Named(name, args)) if args.is_empty() => name.clone(),
                _ => return None,
            };
            let method = match trait_name {
                Some(trait_name) => self
                    .find_trait_method_for_trait(&Type::named(&class_name), trait_name, field)
                    .cloned(),
                None => find_method(self.classes.get(&class_name), field)
                    .or_else(|| find_concrete_impl_method(&self.trait_impls, &class_name, field))
                    .cloned(),
            }?;
            self.function_return_types
                .get(&method.function_name)
                .cloned()
        })
    }

    /// A method on an inline union receiver dispatches on the tag; every
    /// member resolves the method through one trait, so the arms share the
    /// result type. A `mut self` arm writes its receiver back through the
    /// member's payload projection.
    fn compile_union_member_call(
        &mut self,
        union: &DirectUnionType,
        words: &[Value],
        field: &str,
        receiver_place: Option<&str>,
        args: &[MirArg],
        trait_name: Option<&str>,
    ) -> std::result::Result<ValueRef, String> {
        if field == "clone" && trait_name.is_none() {
            // A union clones as its active member (ADR-0052 A6): inline
            // members copy their words, a runtime-object member's handle is
            // cloned by the runtime, and the copy owns its handle.
            if !args.is_empty() {
                return Err("`clone` does not take arguments".to_string());
            }
            let result_ty = DirectType::Union(union.clone());
            let cloned =
                self.switch_on_union_tag(union, words, &result_ty, |codegen, index, member| {
                    let member = if matches!(member.ty, DirectType::Opaque(_)) {
                        let inst = codegen
                            .builder
                            .ins()
                            .call(codegen.runtime[RuntimeFn::CloneValue], &[member.values[0]]);
                        ValueRef {
                            values: codegen.builder.inst_results(inst).to_vec(),
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
            return Ok(cloned);
        }
        let result_ty = self
            .union_member_call_result_type(union, field, trait_name)
            .ok_or_else(|| {
                format!(
                    "direct backend does not know the result of `.{field}` on `{}`",
                    union.union_type
                )
            })?;
        let words = words.to_vec();
        let receiver_place = receiver_place.map(str::to_string);
        let args = args.to_vec();
        let field = field.to_string();
        let trait_name = trait_name.map(str::to_string);
        let result =
            self.switch_on_union_tag(union, &words, &result_ty, |codegen, index, member| {
                let projected = receiver_place.as_ref().map(|place| {
                    format!(
                        "{place}.{}{index}",
                        crate::mir::UNION_PAYLOAD_PROJECTION_PREFIX
                    )
                });
                codegen.compile_member_call_on_value(
                    member,
                    &field,
                    projected.as_deref(),
                    &args,
                    trait_name.as_deref(),
                )
            })?;
        if matches!(result.ty, DirectType::Opaque(_)) {
            self.mark_temporary_opaque_owned(&result);
        }
        Ok(result)
    }

    /// A member call whose receiver is an already loaded value.
    fn compile_member_call_on_value(
        &mut self,
        object: ValueRef,
        field: &str,
        receiver_place: Option<&str>,
        args: &[MirArg],
        trait_name: Option<&str>,
    ) -> std::result::Result<ValueRef, String> {
        match object.ty.clone() {
            DirectType::PlainClass(class_ty) => self.compile_class_member_call(
                class_ty.class_name.as_str(),
                Some(Type::named(&class_ty.class_name)),
                object,
                field,
                receiver_place,
                args,
                trait_name,
            ),
            DirectType::Opaque(ty) => self.compile_opaque_member_call(
                &ty,
                object,
                field,
                receiver_place,
                args,
                trait_name,
            ),
            other => Err(format!(
                "direct backend cannot call `.{field}` on `{}`",
                render_direct_type(&other)
            )),
        }
    }

    /// The tag word and payload words of `member` injected as member
    /// `index`: floats are bit-cast into integer words, and the unused
    /// words are zero.
    fn union_words_from_member(
        &mut self,
        union: &DirectUnionType,
        index: usize,
        member: ValueRef,
    ) -> std::result::Result<Vec<Value>, String> {
        let member_ty = union.member(index)?.clone();
        let member = self.coerce_value(member, &member_ty)?;
        let mut words = vec![self.builder.ins().iconst(types::I64, index as i64)];
        for (value, abi) in member.values.iter().zip(member_ty.abi_types()) {
            words.push(if abi == types::F64 {
                self.builder
                    .ins()
                    .bitcast(types::I64, MemFlags::new(), *value)
            } else {
                *value
            });
        }
        while words.len() < union.payload_words + 1 {
            words.push(self.builder.ins().iconst(types::I64, 0));
        }
        Ok(words)
    }

    /// Member `index` read back from an inline union's words.
    fn union_member_from_words(
        &mut self,
        union: &DirectUnionType,
        index: usize,
        words: &[Value],
    ) -> std::result::Result<ValueRef, String> {
        let member_ty = union.member(index)?.clone();
        let mut values = Vec::new();
        for (word, abi) in words[1..].iter().zip(member_ty.abi_types()) {
            values.push(if abi == types::F64 {
                self.builder
                    .ins()
                    .bitcast(types::F64, MemFlags::new(), *word)
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
        words: &[Value],
        result_ty: &DirectType,
        mut body: impl FnMut(&mut Self, usize, ValueRef) -> std::result::Result<ValueRef, String>,
    ) -> std::result::Result<ValueRef, String> {
        let merge = self.builder.create_block();
        for abi in result_ty.abi_types() {
            self.builder.append_block_param(merge, abi);
        }
        let tag = words[0];
        let caller_owned = self.owned_opaque_temporaries.clone();
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
                let selected = self.builder.ins().icmp_imm(IntCC::Equal, tag, index as i64);
                self.builder.ins().brif(selected, matched, &[], next, &[]);
            } else {
                self.builder.ins().jump(matched, &[]);
            }
            self.builder.switch_to_block(matched);
            self.builder.seal_block(matched);
            self.owned_opaque_temporaries = caller_owned.clone();
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
            self.builder.ins().jump(merge, &result.values);
            if let Some(next) = next {
                self.builder.switch_to_block(next);
                self.builder.seal_block(next);
            }
        }
        self.builder.switch_to_block(merge);
        self.builder.seal_block(merge);
        self.owned_opaque_temporaries = caller_owned;
        let merged = ValueRef {
            values: self.builder.block_params(merge).to_vec(),
            ty: result_ty.clone(),
        };
        if merged_union_owned == Some(true) {
            self.mark_temporary_union_owned(&merged);
        }
        Ok(merged)
    }

    /// A private `Span[T]` range operation on a list (ADR-0061, B2): one
    /// runtime entry point keyed by opcode, with the integer operands in
    /// order and at most one value operand.
    fn emit_vec_span_operation(
        &mut self,
        object: &ValueRef,
        field: &str,
        args: &[MirArg],
        receiver_place: Option<&str>,
        element_ty: &Type,
        element_direct_ty: &DirectType,
    ) -> std::result::Result<ValueRef, String> {
        let opcode = crate::runtime_value::SPAN_OPERATIONS
            .iter()
            .position(|operation| *operation == field)
            .ok_or_else(|| format!("direct backend does not know span operation `{field}`"))?;
        let value_position = match field {
            "__span_contains" | "__span_find" | "__span_count" | "__span_eq" | "__span_fill" => {
                Some(2)
            }
            "__span_restore" => Some(1),
            _ => None,
        };
        let mut operands = Vec::new();
        let mut value_operand = None;
        for (position, argument) in args.iter().enumerate() {
            if value_position == Some(position) {
                let handle = match field {
                    "__span_eq" => {
                        let loaded = self.load_operand(&argument.value)?;
                        self.ensure_opaque(loaded)?.values[0]
                    }
                    "__span_restore" => {
                        let loaded = self.load_operand(&argument.value)?;
                        let loaded = self.ensure_opaque(loaded)?;
                        self.transfer_owned_opaque_value(&loaded)
                    }
                    _ => {
                        self.load_operand_as_opaque_direct(&argument.value, element_direct_ty)?
                            .values[0]
                    }
                };
                value_operand = Some(handle);
                continue;
            }
            let loaded =
                self.load_operand_with_integer_hint(&argument.value, Some(ScalarKind::Int64))?;
            let operand = self.coerce_value(loaded, &DirectType::Scalar(ScalarKind::Int64))?;
            operands.push(operand.values[0]);
        }
        if operands.len() > 8 {
            return Err(format!(
                "direct backend span operation `{field}` has too many operands"
            ));
        }
        let mut call_args = vec![
            object.values[0],
            self.builder.ins().iconst(types::I64, opcode as i64),
        ];
        call_args.extend(operands.iter().copied());
        while call_args.len() < 10 {
            call_args.push(self.builder.ins().iconst(types::I64, 0));
        }
        let value_operand =
            value_operand.unwrap_or_else(|| self.builder.ins().iconst(types::I64, 0));
        call_args.push(value_operand);
        let inst = self
            .builder
            .ins()
            .call(self.runtime[RuntimeFn::VecSpan], &call_args);
        let result = self.builder.inst_results(inst).to_vec();
        if crate::mir::span_member_mutates_receiver(field) {
            if let Some(place) = receiver_place {
                self.store_place(place, object.clone())?;
            }
        }
        match field {
            "__span_extract" => Ok(self.owned_opaque_result(
                result,
                Type::Named("list".to_string(), vec![element_ty.clone()]),
            )),
            "__span_restore" | "__span_fill" => {
                self.release_opaque_handle(result[0]);
                Ok(unit_value(&mut self.builder))
            }
            "__span_contains" | "__span_eq" => {
                let boxed = self.owned_opaque_result(result, Type::named("bool"));
                self.coerce_value(boxed, &DirectType::Scalar(ScalarKind::Bool))
            }
            _ => {
                let boxed = self.owned_opaque_result(result, Type::named("int64"));
                self.coerce_value(boxed, &DirectType::Scalar(ScalarKind::Int64))
            }
        }
    }

    /// Boxes an inline union into the runtime's `Value::Union` for a
    /// container, task, queue, or helper boundary.
    fn box_inline_union(
        &mut self,
        union: &DirectUnionType,
        words: &[Value],
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
        let boxed =
            self.switch_on_union_tag(union, words, &result_ty, |codegen, index, member| {
                // A handle payload of a borrowed union stays owned by the
                // union's place, so the injection copies the value behind
                // the retained handle, as the interpreter copies a place read
                // into an injection. A moved union's payload or a freshly
                // boxed member moves into the boxed value.
                let borrowed_handle = !owned && matches!(member.ty, DirectType::Opaque(_));
                let payload = codegen.ensure_opaque(member)?;
                let (inject, transferred) = if borrowed_handle {
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
                let tag = codegen.builder.ins().iconst(types::I64, index as i64);
                let call = codegen
                    .builder
                    .ins()
                    .call(inject, &[ptr, len, tag, transferred]);
                Ok(ValueRef {
                    values: codegen.builder.inst_results(call).to_vec(),
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
        handle: Value,
    ) -> std::result::Result<ValueRef, String> {
        let tag_call = self
            .builder
            .ins()
            .call(self.runtime[RuntimeFn::UnionTag], &[handle]);
        let tag = self.builder.inst_results(tag_call)[0];
        let mut probe = vec![tag];
        for _ in 0..union.payload_words {
            probe.push(self.builder.ins().iconst(types::I64, 0));
        }
        let result_ty = DirectType::Union(union.clone());
        let unboxed =
            self.switch_on_union_tag(union, &probe, &result_ty, |codegen, index, _| {
                let member_ty = union.member(index)?.clone();
                let index_value = codegen.builder.ins().iconst(types::I64, index as i64);
                let call = codegen.builder.ins().call(
                    codegen.runtime[RuntimeFn::UnionPayloadCopy],
                    &[handle, index_value],
                );
                let payload = codegen.owned_opaque_result(
                    codegen.builder.inst_results(call).to_vec(),
                    direct_type_to_type(&member_ty),
                );
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
                    .ins()
                    .call(self.runtime[RuntimeFn::BoxI32], &[value.values[0]]);
                let boxed = ValueRef {
                    values: self.builder.inst_results(inst).to_vec(),
                    ty: DirectType::Opaque(Type::named("int32")),
                };
                self.mark_temporary_opaque_owned(&boxed);
                Ok(boxed)
            }
            DirectType::Scalar(ScalarKind::Int64) => {
                let inst = self
                    .builder
                    .ins()
                    .call(self.runtime[RuntimeFn::BoxI64], &[value.values[0]]);
                let boxed = ValueRef {
                    values: self.builder.inst_results(inst).to_vec(),
                    ty: DirectType::Opaque(Type::named("int64")),
                };
                self.mark_temporary_opaque_owned(&boxed);
                Ok(boxed)
            }
            DirectType::Scalar(ScalarKind::Uint64) => {
                let inst = self
                    .builder
                    .ins()
                    .call(self.runtime[RuntimeFn::BoxU64], &[value.values[0]]);
                let boxed = ValueRef {
                    values: self.builder.inst_results(inst).to_vec(),
                    ty: DirectType::Opaque(Type::named("uint64")),
                };
                self.mark_temporary_opaque_owned(&boxed);
                Ok(boxed)
            }
            DirectType::Scalar(ScalarKind::Float32) | DirectType::Scalar(ScalarKind::Float64) => {
                let inst = self
                    .builder
                    .ins()
                    .call(self.runtime[RuntimeFn::BoxF64], &[value.values[0]]);
                let boxed = ValueRef {
                    values: self.builder.inst_results(inst).to_vec(),
                    ty: DirectType::Opaque(Type::named("float64")),
                };
                self.mark_temporary_opaque_owned(&boxed);
                Ok(boxed)
            }
            DirectType::Scalar(ScalarKind::Bool) => {
                let inst = self
                    .builder
                    .ins()
                    .call(self.runtime[RuntimeFn::BoxBool], &[value.values[0]]);
                let boxed = ValueRef {
                    values: self.builder.inst_results(inst).to_vec(),
                    ty: DirectType::Opaque(Type::named("bool")),
                };
                self.mark_temporary_opaque_owned(&boxed);
                Ok(boxed)
            }
            DirectType::Scalar(ScalarKind::Unit) => {
                let inst = self
                    .builder
                    .ins()
                    .call(self.runtime[RuntimeFn::BoxUnit], &[]);
                let boxed = ValueRef {
                    values: self.builder.inst_results(inst).to_vec(),
                    ty: DirectType::Opaque(Type::Unit),
                };
                self.mark_temporary_opaque_owned(&boxed);
                Ok(boxed)
            }
            DirectType::PlainClass(class) => {
                let (class_ptr, class_len) = self.string_constant(class.class_name.as_bytes())?;
                let init = self.builder.ins().call(
                    self.runtime[RuntimeFn::InstanceEmpty],
                    &[class_ptr, class_len],
                );
                let current = self.builder.inst_results(init)[0];
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
                    self.builder.ins().call(
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

    fn string_constant(&mut self, bytes: &[u8]) -> std::result::Result<(Value, Value), String> {
        declare_string_constant(self.object, self.string_data, &mut self.builder, bytes)
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
        let count = self.builder.ins().iconst(types::I64, elements.len() as i64);
        let buffer = if elements.is_empty() {
            self.builder.ins().iconst(types::I64, 0)
        } else {
            let buffer_inst = self
                .builder
                .ins()
                .call(self.runtime[RuntimeFn::ArgBufferNew], &[count]);
            let buffer = self.builder.inst_results(buffer_inst)[0];
            for (index, (element, element_type)) in elements.iter().zip(element_types).enumerate() {
                let direct_type = ensure_direct_type(element_type, &self.classes, "tuple element")?;
                let value = self.load_operand_for_target(element, &direct_type)?;
                let value = self.coerce_value(value, &direct_type)?;
                let value = self.ensure_opaque(value)?;
                let transferred = self.transfer_owned_opaque_value(&value);
                let index = self.builder.ins().iconst(types::I64, index as i64);
                self.builder.ins().call(
                    self.runtime[RuntimeFn::ArgBufferStoreOwned],
                    &[buffer, index, transferred],
                );
            }
            buffer
        };
        let inst = self
            .builder
            .ins()
            .call(self.runtime[RuntimeFn::TupleNew], &[buffer, count]);
        let tuple =
            self.owned_opaque_result(self.builder.inst_results(inst).to_vec(), tuple_type.clone());
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
        let index = self.builder.ins().iconst(types::I64, index as i64);
        let inst = self.builder.ins().call(
            self.runtime[RuntimeFn::TupleElement],
            &[tuple.values[0], index],
        );
        let element = self.owned_opaque_result(
            self.builder.inst_results(inst).to_vec(),
            element_type.clone(),
        );
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
        let index = self.builder.ins().iconst(types::I64, index as i64);
        let inst = self.builder.ins().call(
            self.runtime[RuntimeFn::TupleTakeElement],
            &[tuple.values[0], index],
        );
        let element = self.owned_opaque_result(
            self.builder.inst_results(inst).to_vec(),
            element_type.clone(),
        );
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
            self.builder.ins().iconst(types::I64, 0)
        } else {
            let count = self.builder.ins().iconst(types::I64, payloads.len() as i64);
            let buffer_inst = self
                .builder
                .ins()
                .call(self.runtime[RuntimeFn::ArgBufferNew], &[count]);
            let buffer = self.builder.inst_results(buffer_inst)[0];
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
                let index_value = self.builder.ins().iconst(types::I64, index as i64);
                self.builder.ins().call(
                    self.runtime[RuntimeFn::ArgBufferStoreOwned],
                    &[buffer, index_value, transferred],
                );
            }
            buffer
        };
        let payload_count = self.builder.ins().iconst(types::I64, payloads.len() as i64);
        let inst = self.builder.ins().call(
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
            self.builder.inst_results(inst).to_vec(),
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
        let count = self.builder.ins().iconst(types::I64, payloads.len() as i64);
        let buffer = if payloads.is_empty() {
            self.builder.ins().iconst(types::I64, 0)
        } else {
            let buffer_inst = self
                .builder
                .ins()
                .call(self.runtime[RuntimeFn::ArgBufferNew], &[count]);
            let buffer = self.builder.inst_results(buffer_inst)[0];
            for (index, payload) in payloads.into_iter().enumerate() {
                let payload = self.ensure_opaque(payload)?;
                let transferred = self.transfer_owned_opaque_value(&payload);
                let index = self.builder.ins().iconst(types::I64, index as i64);
                self.builder.ins().call(
                    self.runtime[RuntimeFn::ArgBufferStoreOwned],
                    &[buffer, index, transferred],
                );
            }
            buffer
        };
        let inst = self.builder.ins().call(
            self.runtime[RuntimeFn::EnumVariant],
            &[enum_ptr, enum_len, variant_ptr, variant_len, buffer, count],
        );
        Ok(self.owned_opaque_result(self.builder.inst_results(inst).to_vec(), target))
    }

    fn variant_matches_value(
        &mut self,
        value: Value,
        enum_name: &str,
        variant_name: &str,
    ) -> std::result::Result<Value, String> {
        let (enum_ptr, enum_len) = self.string_constant(enum_name.as_bytes())?;
        let (variant_ptr, variant_len) = self.string_constant(variant_name.as_bytes())?;
        let inst = self.builder.ins().call(
            self.runtime[RuntimeFn::VariantMatches],
            &[value, enum_ptr, enum_len, variant_ptr, variant_len],
        );
        Ok(self.builder.inst_results(inst)[0])
    }

    fn compile_variant_payload(
        &mut self,
        scrutinee: ValueRef,
        index: usize,
    ) -> std::result::Result<ValueRef, String> {
        let scrutinee = self.ensure_opaque(scrutinee)?;
        let index = self.builder.ins().iconst(types::I64, index as i64);
        let inst = self.builder.ins().call(
            self.runtime[RuntimeFn::VariantPayload],
            &[scrutinee.values[0], index],
        );
        let payload = ValueRef {
            values: self.builder.inst_results(inst).to_vec(),
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
        let index = self.builder.ins().iconst(types::I64, index as i64);
        let inst = self.builder.ins().call(
            self.runtime[RuntimeFn::VariantTakePayload],
            &[scrutinee.values[0], index],
        );
        Ok(self.owned_opaque_result(
            self.builder.inst_results(inst).to_vec(),
            Type::named("Unknown"),
        ))
    }

    fn compile_try_assign(
        &mut self,
        target: &str,
        target_ty: DirectType,
        try_value: &Operand,
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
        self.builder.ins().brif(ok, ok_block, &[], err_block, &[]);

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
        self.builder.ins().jump(join_block, &[]);
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
            let converted =
                self.convert_try_error_via_from(payload, &source_error_ty, &target_error_ty)?;
            self.compile_enum_variant_from_values(
                "Result",
                "Err",
                vec![converted],
                self.return_type.clone(),
            )?
        };
        self.emit_return_value(error_result)?;
        self.builder.seal_block(err_block);

        self.builder.switch_to_block(join_block);
        self.builder.seal_block(join_block);
        Ok(())
    }

    fn convert_try_error_via_from(
        &mut self,
        payload: ValueRef,
        source_ty: &Type,
        target_ty: &Type,
    ) -> std::result::Result<ValueRef, String> {
        let method = self
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
                    target_ty,
                    &type_params,
                    &mut substitutions,
                ) || crate::sema::substitute_type(&trait_impl.trait_args[0], &substitutions)
                    != *source_ty
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
                    method.clone(),
                ))
            })
            .max_by_key(|(specificity, _)| *specificity)
            .map(|(_, method)| method);
        let Some(method) = method else {
            return Err(format!(
                "direct backend could not find `From[{source_ty}] for {target_ty}` required by `try`"
            ));
        };
        let function_name = method.function_name;
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
        let inst = self.builder.ins().call(function, &arguments);
        let results = self.builder.inst_results(inst).to_vec();
        let (converted, writebacks) = self.split_call_results(&function_name, results)?;
        if !writebacks.is_empty() {
            return Err(
                "direct backend From conversion unexpectedly returned writebacks".to_string(),
            );
        }
        Ok(converted)
    }

    fn mutable_sink_for_static_place(&mut self, place: &str) -> std::result::Result<Value, String> {
        let (root, projection) = place.split_once('.').unwrap_or((place, ""));
        if let Some(index) = self.mutable_param_indices.get(root).copied() {
            let index = self.builder.ins().iconst(types::I64, index as i64);
            let current = self
                .builder
                .ins()
                .call(self.runtime[RuntimeFn::CurrentMutableSink], &[index]);
            let parent = self.builder.inst_results(current)[0];
            let (projection_ptr, projection_len) = self.string_constant(projection.as_bytes())?;
            let projected = self.builder.ins().call(
                self.runtime[RuntimeFn::MutableSinkProject],
                &[parent, projection_ptr, projection_len],
            );
            return Ok(self.builder.inst_results(projected)[0]);
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
            return Ok(self.builder.ins().iconst(types::I64, 0));
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
        let sink = self.builder.ins().call(
            self.runtime[RuntimeFn::MutableSinkNew],
            &[registration_id, root, projection_ptr, projection_len],
        );
        Ok(self.builder.inst_results(sink)[0])
    }

    fn mutable_sink_for_resolved_place(
        &mut self,
        resolved: DirectViewPlace,
    ) -> std::result::Result<Value, String> {
        if resolved.alternatives.is_empty() {
            return Ok(self.builder.ins().iconst(types::I64, 0));
        }
        if resolved.alternatives.len() == 1 && resolved.alternatives[0].conditions.is_empty() {
            return self.mutable_sink_for_static_place(&resolved.alternatives[0].place);
        }

        let sink_slot = self.builder.create_sized_stack_slot(StackSlotData::new(
            StackSlotKind::ExplicitSlot,
            8,
            3,
        ));
        let sink_ptr = self.builder.ins().stack_addr(types::I64, sink_slot, 0);
        let zero = self.builder.ins().iconst(types::I64, 0);
        self.builder.ins().store(MemFlags::new(), zero, sink_ptr, 0);
        let merge = self.builder.create_block();
        let alternative_count = resolved.alternatives.len();
        for (index, alternative) in resolved.alternatives.into_iter().enumerate() {
            if index + 1 < alternative_count {
                let selected = self.view_alternative_condition(&alternative);
                let create_block = self.builder.create_block();
                let next_block = self.builder.create_block();
                self.builder
                    .ins()
                    .brif(selected, create_block, &[], next_block, &[]);
                self.builder.switch_to_block(create_block);
                let sink = self.mutable_sink_for_static_place(&alternative.place)?;
                self.builder.ins().store(MemFlags::new(), sink, sink_ptr, 0);
                self.builder.ins().jump(merge, &[]);
                self.builder.seal_block(create_block);
                self.builder.switch_to_block(next_block);
                self.builder.seal_block(next_block);
            } else {
                let sink = self.mutable_sink_for_static_place(&alternative.place)?;
                self.builder.ins().store(MemFlags::new(), sink, sink_ptr, 0);
                self.builder.ins().jump(merge, &[]);
            }
        }
        self.builder.switch_to_block(merge);
        self.builder.seal_block(merge);
        Ok(self
            .builder
            .ins()
            .load(types::I64, MemFlags::new(), sink_ptr, 0))
    }

    fn mutable_sink_for_place(&mut self, place: &str) -> std::result::Result<Value, String> {
        let resolved = self.resolve_view_place(place)?;
        self.mutable_sink_for_resolved_place(resolved)
    }

    fn install_direct_mutable_sinks(&mut self, sinks: &[Value]) -> std::result::Result<(), String> {
        let count = self.builder.ins().iconst(types::I64, sinks.len() as i64);
        let pointer = if sinks.is_empty() {
            self.builder.ins().iconst(types::I64, 0)
        } else {
            let byte_len = u32::try_from(sinks.len().saturating_mul(8))
                .map_err(|_| "direct mutable sink buffer is too large".to_string())?;
            let slot = self.builder.create_sized_stack_slot(StackSlotData::new(
                StackSlotKind::ExplicitSlot,
                byte_len,
                3,
            ));
            let pointer = self.builder.ins().stack_addr(types::I64, slot, 0);
            for (index, sink) in sinks.iter().copied().enumerate() {
                self.builder
                    .ins()
                    .store(MemFlags::new(), sink, pointer, (index as i32) * 8);
            }
            pointer
        };
        self.builder.ins().call(
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
        public_sinks: &[Value],
        capture_sinks: &[(usize, Value)],
    ) -> std::result::Result<(), String> {
        let store_buffer = |compiler: &mut Self,
                            values: &[Value]|
         -> std::result::Result<Value, String> {
            if values.is_empty() {
                return Ok(compiler.builder.ins().iconst(types::I64, 0));
            }
            let byte_len = u32::try_from(values.len().saturating_mul(8))
                .map_err(|_| "direct mutable sink buffer is too large".to_string())?;
            let slot = compiler.builder.create_sized_stack_slot(StackSlotData::new(
                StackSlotKind::ExplicitSlot,
                byte_len,
                3,
            ));
            let pointer = compiler.builder.ins().stack_addr(types::I64, slot, 0);
            for (index, value) in values.iter().copied().enumerate() {
                compiler
                    .builder
                    .ins()
                    .store(MemFlags::new(), value, pointer, (index as i32) * 8);
            }
            Ok(pointer)
        };
        let public_ptr = store_buffer(self, public_sinks)?;
        let capture_indices = capture_sinks
            .iter()
            .map(|(index, _)| self.builder.ins().iconst(types::I64, *index as i64))
            .collect::<Vec<_>>();
        let capture_values = capture_sinks
            .iter()
            .map(|(_, sink)| *sink)
            .collect::<Vec<_>>();
        let capture_indices_ptr = store_buffer(self, &capture_indices)?;
        let capture_values_ptr = store_buffer(self, &capture_values)?;
        let public_count = self
            .builder
            .ins()
            .iconst(types::I64, public_sinks.len() as i64);
        let capture_count = self
            .builder
            .ins()
            .iconst(types::I64, capture_sinks.len() as i64);
        self.builder.ins().call(
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

    fn release_mutable_sinks(&mut self, sinks: impl IntoIterator<Item = Value>) {
        for sink in sinks {
            self.builder
                .ins()
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
        let index = self.builder.ins().iconst(types::I64, index as i64);
        let sink = self
            .builder
            .ins()
            .call(self.runtime[RuntimeFn::CurrentMutableSink], &[index]);
        let sink = self.builder.inst_results(sink)[0];
        let zero = self.builder.ins().iconst(types::I64, 0);
        let active = self.builder.ins().icmp(IntCC::NotEqual, sink, zero);
        let publish = self.builder.create_block();
        let done = self.builder.create_block();
        self.builder.ins().brif(active, publish, &[], done, &[]);
        self.builder.switch_to_block(publish);
        let current = self.load_root(root)?;
        let current = self.ensure_opaque(current)?;
        let current = self.transfer_owned_opaque_value(&current);
        self.builder.ins().call(
            self.runtime[RuntimeFn::MutableSinkStoreOwned],
            &[sink, current],
        );
        self.builder.ins().jump(done, &[]);
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
        let value = self
            .builder
            .ins()
            .iconst(types::I64, if active { 1 } else { 0 });
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
        let count = self.builder.ins().iconst(types::I64, 1);
        let buffer = self
            .builder
            .ins()
            .call(self.runtime[RuntimeFn::ArgBufferNew], &[count]);
        let buffer = self.builder.inst_results(buffer)[0];
        let zero = self.builder.ins().iconst(types::I64, 0);
        self.builder.ins().call(
            self.runtime[RuntimeFn::ArgBufferStore],
            &[buffer, zero, boxed.values[0]],
        );
        if self.temporary_owns_opaque(&boxed) {
            self.clear_temporary_opaque_owned(&boxed);
            self.release_opaque_handle(boxed.values[0]);
        }
        let thunk_ptr = self.builder.ins().func_addr(types::I64, thunk_ref);
        let registration = self.builder.ins().call(
            self.runtime[RuntimeFn::RegisterCleanup],
            &[thunk_ptr, buffer, count],
        );
        let registration_id = self.builder.inst_results(registration)[0];
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
        self.builder.ins().call(
            self.runtime[RuntimeFn::UnregisterCleanup],
            &[registration_id],
        );
        let zero = self.builder.ins().iconst(types::I64, 0);
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
        let count = self.builder.ins().iconst(types::I64, 1);
        let buffer = self
            .builder
            .ins()
            .call(self.runtime[RuntimeFn::ArgBufferNew], &[count]);
        let buffer = self.builder.inst_results(buffer)[0];
        let zero = self.builder.ins().iconst(types::I64, 0);
        self.builder.ins().call(
            self.runtime[RuntimeFn::ArgBufferStore],
            &[buffer, zero, boxed.values[0]],
        );
        if self.temporary_owns_opaque(&boxed) {
            self.clear_temporary_opaque_owned(&boxed);
            self.release_opaque_handle(boxed.values[0]);
        }
        let active = self.builder.use_var(active_variable);
        let registration_id = self.builder.use_var(registration_variable);
        let thunk_ptr = self.builder.ins().func_addr(types::I64, thunk_ref);
        let refreshed = self.builder.ins().call(
            self.runtime[RuntimeFn::RefreshCleanup],
            &[active, registration_id, thunk_ptr, buffer, count],
        );
        let refreshed_id = self.builder.inst_results(refreshed)[0];
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
        let caller_owned_temporaries = std::mem::take(&mut self.owned_opaque_temporaries);
        for place in self.cleanup_places.clone().into_iter().rev() {
            let Some(variable) = self.cleanup_active_vars.get(&place).copied() else {
                continue;
            };
            let active = self.builder.use_var(variable);
            let zero = self.builder.ins().iconst(types::I64, 0);
            let should_run = self.builder.ins().icmp(IntCC::NotEqual, active, zero);
            let run_block = self.builder.create_block();
            let next_block = self.builder.create_block();
            self.builder
                .ins()
                .brif(should_run, run_block, &[], next_block, &[]);
            self.builder.switch_to_block(run_block);
            self.unregister_cleanup_for_place(&place)?;
            self.builder.def_var(variable, zero);
            self.emit_cleanup_for_place_recording(&place, cancel_before_cleanup, trapping)?;
            self.release_all_temporary_owned();
            self.builder.ins().jump(next_block, &[]);
            self.builder.seal_block(run_block);
            self.builder.switch_to_block(next_block);
            self.builder.seal_block(next_block);
            self.owned_opaque_temporaries.clear();
        }
        self.owned_opaque_temporaries = caller_owned_temporaries;
        Ok(())
    }

    fn append_writeback_return_values(
        &mut self,
        values: &mut Vec<Value>,
    ) -> std::result::Result<(), String> {
        for (name, ty) in self.writeback_locals.clone() {
            let current = self.load_root(&name)?;
            let coerced = self.coerce_value(current, &ty)?;
            values.extend(self.export_return_value(coerced));
        }
        Ok(())
    }

    fn split_call_results(
        &mut self,
        function_name: &str,
        results: Vec<Value>,
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
        let exit_method = cleanup_exit_method(self.cleanup_actions.get(place));
        match &receiver_ty {
            DirectType::PlainClass(class_ty) => {
                let has_exit = self
                    .classes
                    .get(&class_ty.class_name)
                    .and_then(|class| {
                        class
                            .methods
                            .iter()
                            .find(|method| method.name == exit_method)
                    })
                    .is_some();
                if has_exit {
                    let operand = Operand::Place(place.to_string());
                    let result = self.compile_member_call(
                        &operand,
                        MemberCallee::Field(exit_method),
                        Some(place),
                        &[],
                    )?;
                    if record_exit_failure {
                        let class_name = class_ty.class_name.clone();
                        self.record_manager_exit_failure_value(&class_name, result)?;
                    }
                }
            }
            DirectType::Opaque(ty) => {
                let operand = Operand::Place(place.to_string());
                let loaded = self.load_operand(&operand)?;
                if matches!(ty, Type::Named(name, _) if name == "TaskGroup") {
                    let loaded = self.ensure_opaque(loaded)?;
                    let cancel_before = self
                        .builder
                        .ins()
                        .iconst(types::I64, if cancel_before_cleanup { 1 } else { 0 });
                    let result = self.builder.ins().call(
                        self.runtime[RuntimeFn::TaskGroupClose],
                        &[loaded.values[0], cancel_before],
                    );
                    self.release_opaque_handle(self.builder.inst_results(result)[0]);
                    return Ok(());
                }
                if let Ok(result) =
                    self.compile_opaque_member_call(ty, loaded, exit_method, Some(place), &[], None)
                {
                    if record_exit_failure {
                        if let Type::Named(class_name, _) = ty {
                            self.record_manager_exit_failure_value(class_name, result)?;
                        }
                    }
                    return Ok(());
                }
            }
            DirectType::Scalar(_) | DirectType::Union(_) | DirectType::Callable(_) => {}
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
        self.builder.ins().call(
            self.runtime[RuntimeFn::RecordManagerExitFailure],
            &[boxed.values[0], class_ptr, class_len],
        );
        Ok(())
    }

    #[allow(clippy::too_many_arguments)]
    fn compile_class_member_call(
        &mut self,
        class_name: &str,
        receiver_type_hint: Option<Type>,
        object: ValueRef,
        field: &str,
        receiver_place: Option<&str>,
        args: &[MirArg],
        trait_name: Option<&str>,
    ) -> std::result::Result<ValueRef, String> {
        let mut method = trait_name.and_then(|trait_name| {
            receiver_type_hint
                .as_ref()
                .and_then(|ty| self.find_trait_method_for_trait(ty, trait_name, field))
                .or_else(|| {
                    self.find_trait_method_for_trait(&Type::named(class_name), trait_name, field)
                })
                .cloned()
        });
        if trait_name.is_none() {
            method = find_method(self.classes.get(class_name), field).cloned();
            if method.is_none() {
                method = receiver_type_hint
                    .as_ref()
                    .and_then(|ty| self.find_trait_method(ty, field).cloned());
            }
            if method.is_none() {
                method = self
                    .find_trait_method(&Type::named(class_name), field)
                    .cloned();
            }
        }
        let Some(method) = method else {
            return Err(format!(
                "direct backend does not know method `{}.{}`",
                class_name, field
            ));
        };
        let method_function_name = method.function_name.clone();
        if method.receiver == Some(MirReceiverKind::BorrowMut) && receiver_place.is_none() {
            return Err(format!(
                "direct backend does not yet support temporary mutable receiver method `{}.{}`",
                class_name, field
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
        if method.receiver == Some(MirReceiverKind::BorrowMut) {
            let Some(place) = receiver_place else {
                return Err(format!(
                    "direct backend does not yet support temporary mutable receiver method `{}.{}`",
                    class_name, field
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
                    None => self.builder.ins().iconst(types::I64, 0),
                });
            }
            self.install_direct_mutable_sinks(&sinks)?;
            sinks
        } else {
            Vec::new()
        };
        let inst = self.builder.ins().call(func_ref, &lowered_args);
        let results = self.builder.inst_results(inst).to_vec();
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
        _receiver_place: Option<&str>,
        args: &[MirArg],
    ) -> std::result::Result<ValueRef, String> {
        match op {
            RuntimeOp::Scalar(scalar) => self.compile_scalar_runtime_op(scalar, object, args),
            RuntimeOp::Value(value) => self.compile_value_runtime_op(value, object, args),
            RuntimeOp::Resource(_) | RuntimeOp::Rng(_) => {
                self.compile_handle_runtime_op(op, object, args)
            }
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
        words: &mut Vec<Value>,
    ) {
        for immediate in call.immediates {
            words.push(self.builder.ins().iconst(types::I64, *immediate));
        }
        if call.span {
            let zero = self.builder.ins().iconst(types::I64, 0);
            words.extend([zero, zero]);
        }
    }

    /// A resource handle or `random.Rng` method: the receiver and each
    /// argument pass as one word, and the helper's result word is the value.
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
        let names = params.iter().map(|param| param.name).collect::<Vec<_>>();
        let bound = ordered_optional_named_args(&names, args)?;
        let mut lowered = Vec::with_capacity(params.len());
        let mut writeback = None;
        for (index, param) in params.iter().enumerate() {
            let Some(argument) = bound[index] else {
                // The shared validator binds every required parameter of a
                // resolved op, so only an optional one is absent here.
                let default = self.default_runtime_op_arg(op, param.name)?;
                lowered.push(default);
                continue;
            };
            let value = match runtime_op_arg_layout(op, param.name) {
                RuntimeOpArgLayout::Boxed => {
                    let loaded = self.load_operand(&argument.value)?;
                    RuntimeOpArg::Boxed(self.ensure_opaque(loaded)?)
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
        let member = op
            .builtin_member()
            .expect("a handle op has a built-in member");
        let mut words = vec![object.values[0]];
        for (index, value) in lowered.iter().enumerate() {
            words.push(match value {
                RuntimeOpArg::Boxed(value)
                    if member.argument_passing(index) == Some(ReceiverKind::Value) =>
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
            return Ok(unit_value(&mut self.builder));
        }
        let result_ty = op
            .result_type(Some(&receiver_ty), &[])
            .ok_or_else(|| format!("direct backend does not know the result type of `{op}`"))?;
        let scalar = match &result_ty {
            // A close helper returns the handle's last reference.
            Type::Unit => {
                self.release_opaque_handle(results[0]);
                return Ok(unit_value(&mut self.builder));
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

    /// The value an omitted optional argument of a handle op passes.
    fn default_runtime_op_arg(
        &mut self,
        op: RuntimeOp,
        name: &str,
    ) -> std::result::Result<RuntimeOpArg, String> {
        let stdio = || Type::Named("process.Stdio".to_string(), Vec::new());
        Ok(match name {
            // An absent timeout waits without a deadline.
            "timeout" => RuntimeOpArg::Word(self.builder.ins().iconst(types::I64, 0)),
            "cwd" => {
                let unit = unit_value(&mut self.builder);
                RuntimeOpArg::Boxed(self.ensure_opaque(unit)?)
            }
            "env" => {
                let results = self.runtime_call_results(self.runtime[RuntimeFn::MapEmpty], &[]);
                RuntimeOpArg::Boxed(self.owned_opaque_result(
                    results,
                    Type::Named(
                        "dict".to_string(),
                        vec![Type::named("str"), Type::named("str")],
                    ),
                ))
            }
            "stdin" => {
                let results = self.runtime_call_results(self.runtime[RuntimeFn::ProcessNull], &[]);
                RuntimeOpArg::Boxed(self.owned_opaque_result(results, stdio()))
            }
            "stdout" | "stderr" => {
                let results =
                    self.runtime_call_results(self.runtime[RuntimeFn::ProcessInherit], &[]);
                RuntimeOpArg::Boxed(self.owned_opaque_result(results, stdio()))
            }
            "restart" => RuntimeOpArg::Boxed(self.compile_enum_variant_for_target(
                "process.RestartPolicy",
                "OnFailure",
                &[],
                None,
            )?),
            "backoff" => {
                let milliseconds = self.builder.ins().iconst(types::I64, 100);
                let unit_nanoseconds = self.builder.ins().iconst(
                    types::I64,
                    crate::runtime_value::NANOS_PER_MILLISECOND as i64,
                );
                let results = self.runtime_call_results(
                    self.runtime[RuntimeFn::DurationFromI64],
                    &[milliseconds, unit_nanoseconds],
                );
                RuntimeOpArg::Boxed(self.owned_opaque_result(results, Type::named("Duration")))
            }
            "max_restarts" => {
                let minus_one = self.builder.ins().iconst(types::I64, -1);
                RuntimeOpArg::Boxed(self.ensure_opaque(ValueRef {
                    values: vec![minus_one],
                    ty: DirectType::Scalar(ScalarKind::Int32),
                })?)
            }
            "group" => {
                let one = self.builder.ins().iconst(types::I64, 1);
                RuntimeOpArg::Boxed(self.ensure_opaque(ValueRef {
                    values: vec![one],
                    ty: DirectType::Scalar(ScalarKind::Bool),
                })?)
            }
            _ => {
                return Err(format!(
                    "direct backend has no default for argument `{name}` of `{op}`"
                ))
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
                        self.builder
                            .ins()
                            .fcvt_from_uint(types::F64, object.values[0])
                    } else {
                        self.builder
                            .ins()
                            .fcvt_from_sint(types::F64, object.values[0])
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
                let names = runtime_op
                    .params()
                    .iter()
                    .map(|param| param.name)
                    .collect::<Vec<_>>();
                let ordered = ordered_named_args(&names, args)?;
                let argument = ordered[0];
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

    fn compile_opaque_member_call(
        &mut self,
        object_ty: &Type,
        object: ValueRef,
        field: &str,
        receiver_place: Option<&str>,
        args: &[MirArg],
        trait_name: Option<&str>,
    ) -> std::result::Result<ValueRef, String> {
        if let Type::Named(name, _) = object_ty {
            let has_declared_class_method = find_method(self.classes.get(name), field).is_some();
            let has_noncolliding_trait_method = BuiltinMember::resolve(name, field).is_none()
                && self.find_trait_method(object_ty, field).is_some();
            if has_declared_class_method || has_noncolliding_trait_method {
                return self.compile_class_member_call(
                    name,
                    Some(object_ty.clone()),
                    object,
                    field,
                    receiver_place,
                    args,
                    None,
                );
            }
        }
        if let Some(op) = RuntimeOp::resolve_member(object_ty, field, |_| false) {
            return self.compile_runtime_op(op, object, receiver_place, args);
        }
        if let Type::Named(name, class_args) = object_ty {
            if name == "str" {
                let object = self.ensure_opaque(object)?;
                return match field {
                    "__slice" => {
                        let [start_arg, has_start_arg, end_arg, has_end_arg, line_arg, column_arg] =
                            direct_internal_slice_args(args)?;
                        let start = self.load_operand_with_integer_hint(
                            &start_arg.value,
                            Some(ScalarKind::Int64),
                        )?;
                        let start =
                            self.coerce_value(start, &DirectType::Scalar(ScalarKind::Int64))?;
                        let has_start = self.load_operand(&has_start_arg.value)?;
                        let has_start =
                            self.coerce_value(has_start, &DirectType::Scalar(ScalarKind::Bool))?;
                        let end = self.load_operand_with_integer_hint(
                            &end_arg.value,
                            Some(ScalarKind::Int64),
                        )?;
                        let end = self.coerce_value(end, &DirectType::Scalar(ScalarKind::Int64))?;
                        let has_end = self.load_operand(&has_end_arg.value)?;
                        let has_end =
                            self.coerce_value(has_end, &DirectType::Scalar(ScalarKind::Bool))?;
                        let line = self.load_operand_with_integer_hint(
                            &line_arg.value,
                            Some(ScalarKind::Int32),
                        )?;
                        let line =
                            self.coerce_value(line, &DirectType::Scalar(ScalarKind::Int32))?;
                        let column = self.load_operand_with_integer_hint(
                            &column_arg.value,
                            Some(ScalarKind::Int32),
                        )?;
                        let column =
                            self.coerce_value(column, &DirectType::Scalar(ScalarKind::Int32))?;
                        let inst = self.builder.ins().call(
                            self.runtime[RuntimeFn::StringSlice],
                            &[
                                object.values[0],
                                start.values[0],
                                has_start.values[0],
                                end.values[0],
                                has_end.values[0],
                                line.values[0],
                                column.values[0],
                            ],
                        );
                        Ok(self.owned_opaque_result(
                            self.builder.inst_results(inst).to_vec(),
                            Type::named("str"),
                        ))
                    }
                    "len" | "byte_len" => {
                        if !args.is_empty() {
                            return Err(format!(
                                "direct backend expected `{field}()` to take no arguments"
                            ));
                        }
                        let function = match field {
                            "len" => self.runtime[RuntimeFn::StringLen],
                            "byte_len" => self.runtime[RuntimeFn::StringByteLen],
                            _ => unreachable!(),
                        };
                        let inst = self.builder.ins().call(function, &[object.values[0]]);
                        let len = self.builder.inst_results(inst)[0];
                        Ok(ValueRef {
                            values: vec![len],
                            ty: DirectType::Scalar(ScalarKind::Int64),
                        })
                    }
                    "contains" | "starts_with" | "ends_with" => {
                        let [argument] = args else {
                            return Err(format!(
                                "direct backend expected `{}`() to receive one string argument",
                                field
                            ));
                        };
                        let loaded_value = self.load_operand(&argument.value)?;
                        let value = self.ensure_opaque(loaded_value)?;
                        let func = match field {
                            "contains" => self.runtime[RuntimeFn::StringContains],
                            "starts_with" => self.runtime[RuntimeFn::StringStartsWith],
                            "ends_with" => self.runtime[RuntimeFn::StringEndsWith],
                            _ => unreachable!(),
                        };
                        let inst = self
                            .builder
                            .ins()
                            .call(func, &[object.values[0], value.values[0]]);
                        Ok(ValueRef {
                            values: self.builder.inst_results(inst).to_vec(),
                            ty: DirectType::Scalar(ScalarKind::Bool),
                        })
                    }
                    "split" => {
                        let [argument] = args else {
                            return Err(
                                "direct backend expected `split()` to receive one string argument"
                                    .to_string(),
                            );
                        };
                        let loaded_value = self.load_operand(&argument.value)?;
                        let value = self.ensure_opaque(loaded_value)?;
                        let inst = self.builder.ins().call(
                            self.runtime[RuntimeFn::StringSplit],
                            &[object.values[0], value.values[0]],
                        );
                        Ok(self.owned_opaque_result(
                            self.builder.inst_results(inst).to_vec(),
                            Type::Named("list".to_string(), vec![Type::named("str")]),
                        ))
                    }
                    "replace" => {
                        let [from_arg, to_arg] = args else {
                            return Err(
                                "direct backend expected `replace()` to receive `from` and `to` string arguments"
                                    .to_string(),
                            );
                        };
                        let loaded_from = self.load_operand(&from_arg.value)?;
                        let from = self.ensure_opaque(loaded_from)?;
                        let loaded_to = self.load_operand(&to_arg.value)?;
                        let to = self.ensure_opaque(loaded_to)?;
                        let inst = self.builder.ins().call(
                            self.runtime[RuntimeFn::StringReplace],
                            &[object.values[0], from.values[0], to.values[0]],
                        );
                        Ok(self.owned_opaque_result(
                            self.builder.inst_results(inst).to_vec(),
                            Type::named("str"),
                        ))
                    }
                    "add" => {
                        let [argument] = args else {
                            return Err(
                                "direct backend expected `add()` to receive one string argument"
                                    .to_string(),
                            );
                        };
                        let loaded_value = self.load_operand(&argument.value)?;
                        let value = self.ensure_opaque(loaded_value)?;
                        let binary_opcode = Self::binary_opcode(BinaryOp::Add);
                        let opcode = self.builder.ins().iconst(types::I64, binary_opcode);
                        let zero = self.builder.ins().iconst(types::I64, 0);
                        let inst = self.builder.ins().call(
                            self.runtime[RuntimeFn::BinaryValue],
                            &[opcode, object.values[0], value.values[0], zero, zero, zero],
                        );
                        Ok(self.owned_opaque_result(
                            self.builder.inst_results(inst).to_vec(),
                            Type::named("str"),
                        ))
                    }
                    "to_lower" => {
                        if !args.is_empty() {
                            return Err(
                                "direct backend expected `to_lower()` to take no arguments"
                                    .to_string(),
                            );
                        }
                        let inst = self
                            .builder
                            .ins()
                            .call(self.runtime[RuntimeFn::StringToLower], &[object.values[0]]);
                        Ok(self.owned_opaque_result(
                            self.builder.inst_results(inst).to_vec(),
                            Type::named("str"),
                        ))
                    }
                    "to_upper" => {
                        if !args.is_empty() {
                            return Err(
                                "direct backend expected `to_upper()` to take no arguments"
                                    .to_string(),
                            );
                        }
                        let inst = self
                            .builder
                            .ins()
                            .call(self.runtime[RuntimeFn::StringToUpper], &[object.values[0]]);
                        Ok(self.owned_opaque_result(
                            self.builder.inst_results(inst).to_vec(),
                            Type::named("str"),
                        ))
                    }
                    "join" => {
                        let [argument] = args else {
                            return Err(
                                "direct backend expected `join()` to receive one list argument"
                                    .to_string(),
                            );
                        };
                        let loaded_value = self.load_operand(&argument.value)?;
                        let value = self.ensure_opaque(loaded_value)?;
                        let inst = self.builder.ins().call(
                            self.runtime[RuntimeFn::StringJoin],
                            &[object.values[0], value.values[0]],
                        );
                        Ok(self.owned_opaque_result(
                            self.builder.inst_results(inst).to_vec(),
                            Type::named("str"),
                        ))
                    }
                    "strip_prefix" | "strip_suffix" => {
                        let [argument] = args else {
                            return Err(format!(
                                "direct backend expected `{}`() to receive one string argument",
                                field
                            ));
                        };
                        let loaded_value = self.load_operand(&argument.value)?;
                        let value = self.ensure_opaque(loaded_value)?;
                        let func = match field {
                            "strip_prefix" => self.runtime[RuntimeFn::StringStripPrefix],
                            "strip_suffix" => self.runtime[RuntimeFn::StringStripSuffix],
                            _ => unreachable!(),
                        };
                        let inst = self
                            .builder
                            .ins()
                            .call(func, &[object.values[0], value.values[0]]);
                        Ok(self.owned_opaque_result(
                            self.builder.inst_results(inst).to_vec(),
                            optional_type(Type::named("str")),
                        ))
                    }
                    "trim_start" | "trim_end" | "lines" | "split_whitespace" | "split_once"
                    | "rsplit_once" | "find" | "count" | "repeat" => {
                        self.compile_string_text_method(object.values[0], field, args)
                    }
                    "trim" => {
                        if !args.is_empty() {
                            return Err(
                                "direct backend expected `trim()` to take no arguments".to_string()
                            );
                        }
                        let inst = self
                            .builder
                            .ins()
                            .call(self.runtime[RuntimeFn::StringTrim], &[object.values[0]]);
                        Ok(self.owned_opaque_result(
                            self.builder.inst_results(inst).to_vec(),
                            Type::named("str"),
                        ))
                    }
                    _ => Err(format!(
                        "direct backend does not know runtime member `{}.{}`",
                        name, field
                    )),
                };
            }
            if name == "Array" {
                let object = self.ensure_opaque(object)?;
                let element_ty = class_args
                    .first()
                    .cloned()
                    .ok_or("direct backend expected Array to carry one dtype".to_string())?;
                let element_direct_ty =
                    ensure_direct_type(&element_ty, &self.classes, "Array dtype")?;
                let array_ty = Type::Named("Array".to_string(), vec![element_ty.clone()]);
                let zero = self.builder.ins().iconst(types::I64, 0);
                return match field {
                    "clone" => {
                        if !args.is_empty() {
                            return Err("direct backend expected `clone()` to take no arguments"
                                .to_string());
                        }
                        let inst = self.builder.ins().call(
                            self.runtime[RuntimeFn::ArrayClone],
                            &[object.values[0], zero, zero],
                        );
                        Ok(self.owned_opaque_result(
                            self.builder.inst_results(inst).to_vec(),
                            object_ty.clone(),
                        ))
                    }
                    "__slice" => {
                        let [start_arg, has_start_arg, end_arg, has_end_arg, line_arg, column_arg] =
                            direct_internal_slice_args(args)?;
                        let integer_hint = Some(ScalarKind::Int64);
                        let start =
                            self.load_operand_with_integer_hint(&start_arg.value, integer_hint)?;
                        let start =
                            self.coerce_value(start, &DirectType::Scalar(ScalarKind::Int64))?;
                        let has_start = self.load_operand(&has_start_arg.value)?;
                        let has_start =
                            self.coerce_value(has_start, &DirectType::Scalar(ScalarKind::Bool))?;
                        let end =
                            self.load_operand_with_integer_hint(&end_arg.value, integer_hint)?;
                        let end = self.coerce_value(end, &DirectType::Scalar(ScalarKind::Int64))?;
                        let has_end = self.load_operand(&has_end_arg.value)?;
                        let has_end =
                            self.coerce_value(has_end, &DirectType::Scalar(ScalarKind::Bool))?;
                        let line = self.load_operand_with_integer_hint(
                            &line_arg.value,
                            Some(ScalarKind::Int32),
                        )?;
                        let line =
                            self.coerce_value(line, &DirectType::Scalar(ScalarKind::Int32))?;
                        let column = self.load_operand_with_integer_hint(
                            &column_arg.value,
                            Some(ScalarKind::Int32),
                        )?;
                        let column =
                            self.coerce_value(column, &DirectType::Scalar(ScalarKind::Int32))?;
                        let inst = self.builder.ins().call(
                            self.runtime[RuntimeFn::ArraySlice],
                            &[
                                object.values[0],
                                start.values[0],
                                has_start.values[0],
                                end.values[0],
                                has_end.values[0],
                                line.values[0],
                                column.values[0],
                            ],
                        );
                        Ok(self.owned_opaque_result(
                            self.builder.inst_results(inst).to_vec(),
                            array_ty,
                        ))
                    }
                    "shape" => {
                        ordered_named_args(&[], args)?;
                        let inst = self
                            .builder
                            .ins()
                            .call(self.runtime[RuntimeFn::ArrayShape], &[object.values[0]]);
                        Ok(self.owned_opaque_result(
                            self.builder.inst_results(inst).to_vec(),
                            Type::Named("list".to_string(), vec![Type::named("int64")]),
                        ))
                    }
                    "len" => {
                        ordered_named_args(&[], args)?;
                        let inst = self
                            .builder
                            .ins()
                            .call(self.runtime[RuntimeFn::ArrayLen], &[object.values[0]]);
                        Ok(ValueRef {
                            values: self.builder.inst_results(inst).to_vec(),
                            ty: DirectType::Scalar(ScalarKind::Int64),
                        })
                    }
                    "get" => {
                        let ordered = ordered_named_args(&["index"], args)?;
                        let coordinates = self.load_operand(&ordered[0].value)?;
                        let coordinates = self.ensure_opaque(coordinates)?;
                        let inst = self.builder.ins().call(
                            self.runtime[RuntimeFn::ArrayGet],
                            &[object.values[0], coordinates.values[0], zero, zero],
                        );
                        Ok(self.owned_opaque_result(
                            self.builder.inst_results(inst).to_vec(),
                            optional_type(element_ty),
                        ))
                    }
                    "set" => {
                        let ordered = ordered_named_args(&["index", "value"], args)?;
                        let coordinates = self.load_operand(&ordered[0].value)?;
                        let coordinates = self.ensure_opaque(coordinates)?;
                        let value = self
                            .load_operand_as_opaque_direct(&ordered[1].value, &element_direct_ty)?;
                        let inst = self.builder.ins().call(
                            self.runtime[RuntimeFn::ArraySetInPlace],
                            &[
                                object.values[0],
                                coordinates.values[0],
                                value.values[0],
                                zero,
                                zero,
                            ],
                        );
                        if let Some(place) = receiver_place {
                            self.store_place(place, object.clone())?;
                        }
                        Ok(self.owned_opaque_result(
                            self.builder.inst_results(inst).to_vec(),
                            optional_type(element_ty),
                        ))
                    }
                    "fill" => {
                        let ordered = ordered_named_args(&["value"], args)?;
                        let value = self
                            .load_operand_as_opaque_direct(&ordered[0].value, &element_direct_ty)?;
                        let inst = self.builder.ins().call(
                            self.runtime[RuntimeFn::ArrayFillInPlace],
                            &[object.values[0], value.values[0], zero, zero],
                        );
                        self.release_opaque_handle(self.builder.inst_results(inst)[0]);
                        if let Some(place) = receiver_place {
                            self.store_place(place, object.clone())?;
                        }
                        Ok(unit_value(&mut self.builder))
                    }
                    "__index" => {
                        let [coordinate_arg, line_arg, column_arg] = <&[MirArg; 3]>::try_from(args)
                            .expect("MIR lowering emits three arguments for Array indexing");
                        let coordinates = self.load_operand(&coordinate_arg.value)?;
                        let integer_hint = Some(ScalarKind::Int32);
                        let line =
                            self.load_operand_with_integer_hint(&line_arg.value, integer_hint)?;
                        let line =
                            self.coerce_value(line, &DirectType::Scalar(ScalarKind::Int32))?;
                        let column =
                            self.load_operand_with_integer_hint(&column_arg.value, integer_hint)?;
                        let column =
                            self.coerce_value(column, &DirectType::Scalar(ScalarKind::Int32))?;
                        if coordinates.ty == DirectType::Scalar(ScalarKind::Int64) {
                            if let DirectType::Scalar(kind) = element_direct_ty {
                                return self.emit_array_element_read(
                                    object.values[0],
                                    coordinates,
                                    kind,
                                    element_ty,
                                    line.values[0],
                                    column.values[0],
                                );
                            }
                        }
                        let coordinates = self.ensure_opaque(coordinates)?;
                        let inst = self.builder.ins().call(
                            self.runtime[RuntimeFn::ArrayIndex],
                            &[
                                object.values[0],
                                coordinates.values[0],
                                line.values[0],
                                column.values[0],
                            ],
                        );
                        let result = self.owned_opaque_result(
                            self.builder.inst_results(inst).to_vec(),
                            element_ty,
                        );
                        self.coerce_value(result, &element_direct_ty)
                    }
                    "__set_index" => {
                        let [coordinate_arg, value_arg, line_arg, column_arg] =
                            <&[MirArg; 4]>::try_from(args).expect(
                                "MIR lowering emits four arguments for Array indexed assignment",
                            );
                        let coordinates = self.load_operand(&coordinate_arg.value)?;
                        let coordinates = self.ensure_opaque(coordinates)?;
                        let value = self
                            .load_operand_as_opaque_direct(&value_arg.value, &element_direct_ty)?;
                        let integer_hint = Some(ScalarKind::Int32);
                        let line =
                            self.load_operand_with_integer_hint(&line_arg.value, integer_hint)?;
                        let line =
                            self.coerce_value(line, &DirectType::Scalar(ScalarKind::Int32))?;
                        let column =
                            self.load_operand_with_integer_hint(&column_arg.value, integer_hint)?;
                        let column =
                            self.coerce_value(column, &DirectType::Scalar(ScalarKind::Int32))?;
                        let inst = self.builder.ins().call(
                            self.runtime[RuntimeFn::ArraySetIndexInPlace],
                            &[
                                object.values[0],
                                coordinates.values[0],
                                value.values[0],
                                line.values[0],
                                column.values[0],
                            ],
                        );
                        self.release_opaque_handle(self.builder.inst_results(inst)[0]);
                        if let Some(place) = receiver_place {
                            self.store_place(place, object.clone())?;
                        }
                        Ok(unit_value(&mut self.builder))
                    }
                    "map" => {
                        let ordered = ordered_named_args(&["f"], args)?;
                        let callback = self.load_operand(&ordered[0].value)?;
                        let callback_type = direct_type_to_type(&callback.ty);
                        let result_type = match callback_type {
                            Type::Function { return_type, .. }
                            | Type::Closure { return_type, .. } => *return_type,
                            other => {
                                return Err(format!(
                                    "direct backend expected `Array.map` callback, found `{other}`"
                                ))
                            }
                        };
                        let result_dtype = self
                            .builder
                            .ins()
                            .iconst(types::I64, direct_array_dtype_code(&result_type)?);
                        let callback = self.ensure_opaque(callback)?;
                        let inst = self.builder.ins().call(
                            self.runtime[RuntimeFn::ArrayMap],
                            &[
                                object.values[0],
                                callback.values[0],
                                result_dtype,
                                zero,
                                zero,
                            ],
                        );
                        Ok(self.owned_opaque_result(
                            self.builder.inst_results(inst).to_vec(),
                            Type::Named("Array".to_string(), vec![result_type]),
                        ))
                    }
                    "sum" | "min" | "max" | "mean" => {
                        ordered_named_args(&[], args)?;
                        let reduction_code = match field {
                            "sum" => 0,
                            "min" => 1,
                            "max" => 2,
                            _ => {
                                debug_assert_eq!(field, "mean");
                                3
                            }
                        };
                        let reduction = self.builder.ins().iconst(types::I64, reduction_code);
                        let inst = self.builder.ins().call(
                            self.runtime[RuntimeFn::ArrayReduce],
                            &[object.values[0], reduction, zero, zero],
                        );
                        let target = if field == "mean" {
                            DirectType::Scalar(ScalarKind::Float64)
                        } else {
                            element_direct_ty
                        };
                        let boxed = self.owned_opaque_result(
                            self.builder.inst_results(inst).to_vec(),
                            direct_type_to_type(&target),
                        );
                        self.coerce_value(boxed, &target)
                    }
                    "wrapping_add" | "wrapping_sub" | "wrapping_mul" | "saturating_add"
                    | "saturating_sub" | "saturating_mul" => {
                        let ordered = ordered_named_args(&["rhs"], args)?;
                        let rhs = self.load_operand(&ordered[0].value)?;
                        let rhs = if direct_array_element_type(&rhs.ty).is_some() {
                            rhs
                        } else {
                            self.coerce_value(rhs, &element_direct_ty)?
                        };
                        let rhs = self.ensure_opaque(rhs)?;
                        let operation_code = match field {
                            "wrapping_add" | "saturating_add" => 0,
                            "wrapping_sub" | "saturating_sub" => 1,
                            _ => {
                                debug_assert!(matches!(field, "wrapping_mul" | "saturating_mul"));
                                2
                            }
                        };
                        let arithmetic_mode = if field.starts_with("wrapping_") { 1 } else { 2 };
                        let operation = self.builder.ins().iconst(types::I64, operation_code);
                        let arithmetic_mode =
                            self.builder.ins().iconst(types::I64, arithmetic_mode);
                        let inst = self.builder.ins().call(
                            self.runtime[RuntimeFn::ArrayBinary],
                            &[
                                object.values[0],
                                rhs.values[0],
                                zero,
                                operation,
                                arithmetic_mode,
                                zero,
                                zero,
                            ],
                        );
                        Ok(self.owned_opaque_result(
                            self.builder.inst_results(inst).to_vec(),
                            array_ty,
                        ))
                    }
                    _ => Err(format!(
                        "direct backend does not know runtime member `Array.{field}`"
                    )),
                };
            }
            if name == "list" {
                let object = self.ensure_opaque(object)?;
                let element_ty = class_args
                    .first()
                    .cloned()
                    .unwrap_or(Type::named("Unknown"));
                let element_direct_ty =
                    ensure_direct_type(&element_ty, &self.classes, "Vec element")?;
                if field.starts_with("__span_") {
                    return self.emit_vec_span_operation(
                        &object,
                        field,
                        args,
                        receiver_place,
                        &element_ty,
                        &element_direct_ty,
                    );
                }
                return match field {
                    "__slice" => {
                        let [start_arg, has_start_arg, end_arg, has_end_arg, line_arg, column_arg] =
                            direct_internal_slice_args(args)?;
                        let start = self.load_operand_with_integer_hint(
                            &start_arg.value,
                            Some(ScalarKind::Int64),
                        )?;
                        let start =
                            self.coerce_value(start, &DirectType::Scalar(ScalarKind::Int64))?;
                        let has_start = self.load_operand(&has_start_arg.value)?;
                        let has_start =
                            self.coerce_value(has_start, &DirectType::Scalar(ScalarKind::Bool))?;
                        let end = self.load_operand_with_integer_hint(
                            &end_arg.value,
                            Some(ScalarKind::Int64),
                        )?;
                        let end = self.coerce_value(end, &DirectType::Scalar(ScalarKind::Int64))?;
                        let has_end = self.load_operand(&has_end_arg.value)?;
                        let has_end =
                            self.coerce_value(has_end, &DirectType::Scalar(ScalarKind::Bool))?;
                        let line = self.load_operand_with_integer_hint(
                            &line_arg.value,
                            Some(ScalarKind::Int32),
                        )?;
                        let line =
                            self.coerce_value(line, &DirectType::Scalar(ScalarKind::Int32))?;
                        let column = self.load_operand_with_integer_hint(
                            &column_arg.value,
                            Some(ScalarKind::Int32),
                        )?;
                        let column =
                            self.coerce_value(column, &DirectType::Scalar(ScalarKind::Int32))?;
                        let inst = self.builder.ins().call(
                            self.runtime[RuntimeFn::VecSlice],
                            &[
                                object.values[0],
                                start.values[0],
                                has_start.values[0],
                                end.values[0],
                                has_end.values[0],
                                line.values[0],
                                column.values[0],
                            ],
                        );
                        Ok(self.owned_opaque_result(
                            self.builder.inst_results(inst).to_vec(),
                            Type::Named("list".to_string(), vec![element_ty]),
                        ))
                    }
                    "len" => {
                        if !args.is_empty() {
                            return Err(
                                "direct backend expected `len()` to take no arguments".to_string()
                            );
                        }
                        let len = self.emit_list_len(object.values[0]);
                        Ok(ValueRef {
                            values: vec![len],
                            ty: DirectType::Scalar(ScalarKind::Int64),
                        })
                    }
                    "is_empty" => {
                        if !args.is_empty() {
                            return Err(
                                "direct backend expected `is_empty()` to take no arguments"
                                    .to_string(),
                            );
                        }
                        let inst = self
                            .builder
                            .ins()
                            .call(self.runtime[RuntimeFn::VecIsEmpty], &[object.values[0]]);
                        Ok(ValueRef {
                            values: self.builder.inst_results(inst).to_vec(),
                            ty: DirectType::Scalar(ScalarKind::Bool),
                        })
                    }
                    "append" => {
                        let [argument] = args else {
                            return Err(
                                "direct backend expected `append()` to receive one argument"
                                    .to_string(),
                            );
                        };
                        // A scalar is pushed as its bits, with no box.
                        let scalar_kind = match element_direct_ty {
                            DirectType::Scalar(ScalarKind::Int64) => Some(0),
                            DirectType::Scalar(ScalarKind::Float64) => Some(1),
                            DirectType::Scalar(ScalarKind::Bool) => Some(2),
                            _ => None,
                        };
                        if let Some(kind) = scalar_kind {
                            let loaded = self.load_operand_with_integer_hint(
                                &argument.value,
                                element_direct_ty.scalar_kind(),
                            )?;
                            let value = self.coerce_value(loaded, &element_direct_ty)?.values[0];
                            let bits = if kind == 1 {
                                self.builder
                                    .ins()
                                    .bitcast(types::I64, MemFlags::new(), value)
                            } else {
                                value
                            };
                            let kind = self.builder.ins().iconst(types::I64, kind);
                            self.builder.ins().call(
                                self.runtime[RuntimeFn::VecPushScalar],
                                &[object.values[0], bits, kind],
                            );
                            if let Some(place) = receiver_place {
                                if !self.receiver_store_is_redundant(place) {
                                    self.store_place(place, object.clone())?;
                                }
                            }
                            return Ok(unit_value(&mut self.builder));
                        }
                        let value = self
                            .load_operand_as_opaque_direct(&argument.value, &element_direct_ty)?;
                        let value = self.transfer_owned_opaque_value(&value);
                        let result = self.builder.ins().call(
                            self.runtime[RuntimeFn::VecPushInPlace],
                            &[object.values[0], value],
                        );
                        self.release_opaque_handle(self.builder.inst_results(result)[0]);
                        if let Some(place) = receiver_place {
                            self.store_place(place, object.clone())?;
                        }
                        Ok(unit_value(&mut self.builder))
                    }
                    "pop" => {
                        let index = match args {
                            [] => self.builder.ins().iconst(types::I64, -1),
                            [argument] => {
                                let loaded = self.load_operand_with_integer_hint(
                                    &argument.value,
                                    Some(ScalarKind::Int64),
                                )?;
                                self.coerce_value(loaded, &DirectType::Scalar(ScalarKind::Int64))?
                                    .values[0]
                            }
                            _ => {
                                return Err(
                                    "direct backend expected `pop()` to receive at most one index"
                                        .to_string(),
                                )
                            }
                        };
                        let zero = self.builder.ins().iconst(types::I64, 0);
                        let opcode = self.builder.ins().iconst(types::I64, 0);
                        let inst = self.builder.ins().call(
                            self.runtime[RuntimeFn::CollectionOperation],
                            &[object.values[0], zero, index, opcode],
                        );
                        if let Some(place) = receiver_place {
                            self.store_place(place, object.clone())?;
                        }
                        Ok(self.owned_opaque_result(
                            self.builder.inst_results(inst).to_vec(),
                            element_ty,
                        ))
                    }
                    "get" => {
                        let [argument] = args else {
                            return Err(
                                "direct backend expected `get()` to receive one index argument"
                                    .to_string(),
                            );
                        };
                        let loaded_index = self.load_operand(&argument.value)?;
                        let index = self
                            .coerce_value(loaded_index, &DirectType::Scalar(ScalarKind::Int64))?;
                        let inst = self.builder.ins().call(
                            self.runtime[RuntimeFn::VecGet],
                            &[object.values[0], index.values[0]],
                        );
                        Ok(self.owned_opaque_result(
                            self.builder.inst_results(inst).to_vec(),
                            class_args
                                .first()
                                .cloned()
                                .unwrap_or(Type::named("Unknown")),
                        ))
                    }
                    "__index_option" => {
                        let [argument] = args else {
                            return Err(
                                "direct backend expected internal optional vector indexing to receive one argument"
                                    .to_string(),
                            );
                        };
                        let loaded_index = self.load_operand(&argument.value)?;
                        let index = self
                            .coerce_value(loaded_index, &DirectType::Scalar(ScalarKind::Int64))?;
                        let inst = self.builder.ins().call(
                            self.runtime[RuntimeFn::VecIndexOption],
                            &[object.values[0], index.values[0]],
                        );
                        Ok(self.owned_opaque_result(
                            self.builder.inst_results(inst).to_vec(),
                            element_ty.clone(),
                        ))
                    }
                    "__take_index_option" => {
                        let [argument] = args else {
                            return Err(
                                "direct backend expected internal consuming vector indexing to receive one argument"
                                    .to_string(),
                            );
                        };
                        let Some(receiver_place) = receiver_place else {
                            return Err(
                                "direct backend expected internal consuming vector indexing to provide its private receiver place"
                                    .to_string(),
                            );
                        };
                        let loaded_index = self.load_operand(&argument.value)?;
                        let index = self
                            .coerce_value(loaded_index, &DirectType::Scalar(ScalarKind::Int64))?;
                        let inst = self.builder.ins().call(
                            self.runtime[RuntimeFn::VecTakeIndexInPlace],
                            &[object.values[0], index.values[0]],
                        );
                        self.store_place(receiver_place, object.clone())?;
                        Ok(self.owned_opaque_result(
                            self.builder.inst_results(inst).to_vec(),
                            optional_type(element_ty),
                        ))
                    }
                    "__index" => {
                        let [argument, line_arg, column_arg] = args else {
                            return Err(
                                "direct backend expected internal vector indexing to receive index, line, and column"
                                    .to_string(),
                            );
                        };
                        let loaded_index = self.load_operand(&argument.value)?;
                        let index = self
                            .coerce_value(loaded_index, &DirectType::Scalar(ScalarKind::Int64))?;
                        let loaded_line = self.load_operand(&line_arg.value)?;
                        let line =
                            self.coerce_value(loaded_line, &DirectType::Scalar(ScalarKind::Int32))?;
                        let loaded_column = self.load_operand(&column_arg.value)?;
                        let column = self
                            .coerce_value(loaded_column, &DirectType::Scalar(ScalarKind::Int32))?;
                        // Scalar elements come back as raw bits, with no box.
                        let scalar_kind = match element_direct_ty {
                            DirectType::Scalar(ScalarKind::Int64) => Some(0),
                            DirectType::Scalar(ScalarKind::Float64) => Some(1),
                            DirectType::Scalar(ScalarKind::Bool) => Some(2),
                            _ => None,
                        };
                        let proven = self.proven_access == ProvenAccess::Read;
                        if let Some(kind) = scalar_kind {
                            let bits = self.emit_scalar_list_read(
                                object.values[0],
                                index.values[0],
                                kind,
                                proven,
                                line.values[0],
                                column.values[0],
                            )?;
                            let value = if scalar_kind == Some(1) {
                                self.builder
                                    .ins()
                                    .bitcast(types::F64, MemFlags::new(), bits)
                            } else {
                                bits
                            };
                            return Ok(ValueRef {
                                values: vec![value],
                                ty: element_direct_ty.clone(),
                            });
                        }
                        let normalized_index = if proven {
                            index.values[0]
                        } else {
                            self.emit_vec_index_failure_guard(
                                object.values[0],
                                index.values[0],
                                line.values[0],
                                column.values[0],
                            )?
                        };
                        let inst = self.builder.ins().call(
                            self.runtime[RuntimeFn::VecIndex],
                            &[
                                object.values[0],
                                normalized_index,
                                line.values[0],
                                column.values[0],
                            ],
                        );
                        let indexed = self.owned_opaque_result(
                            self.builder.inst_results(inst).to_vec(),
                            element_ty,
                        );
                        self.coerce_value(indexed, &element_direct_ty)
                    }
                    "set" => {
                        let [index_arg, value_arg] = args else {
                            return Err(
                                "direct backend expected `set()` to receive index and value"
                                    .to_string(),
                            );
                        };
                        let loaded_index = self.load_operand(&index_arg.value)?;
                        let index = self
                            .coerce_value(loaded_index, &DirectType::Scalar(ScalarKind::Int64))?;
                        let value = self
                            .load_operand_as_opaque_direct(&value_arg.value, &element_direct_ty)?;
                        let value = self.transfer_owned_opaque_value(&value);
                        let inst = self.builder.ins().call(
                            self.runtime[RuntimeFn::VecSetInPlace],
                            &[object.values[0], index.values[0], value],
                        );
                        if let Some(place) = receiver_place {
                            self.store_place(place, object.clone())?;
                        }
                        Ok(self.owned_opaque_result(
                            self.builder.inst_results(inst).to_vec(),
                            optional_type(
                                class_args
                                    .first()
                                    .cloned()
                                    .unwrap_or(Type::named("Unknown")),
                            ),
                        ))
                    }
                    "__set_index" => {
                        let [index_arg, value_arg, line_arg, column_arg] = args else {
                            return Err(
                                "direct backend expected internal indexed assignment to receive index, value, line, and column"
                                    .to_string(),
                            );
                        };
                        let loaded_index = self.load_operand(&index_arg.value)?;
                        let index = self
                            .coerce_value(loaded_index, &DirectType::Scalar(ScalarKind::Int64))?;
                        let value = self
                            .load_operand_as_opaque_direct(&value_arg.value, &element_direct_ty)?;
                        let loaded_line = self.load_operand(&line_arg.value)?;
                        let line =
                            self.coerce_value(loaded_line, &DirectType::Scalar(ScalarKind::Int32))?;
                        let loaded_column = self.load_operand(&column_arg.value)?;
                        let column = self
                            .coerce_value(loaded_column, &DirectType::Scalar(ScalarKind::Int32))?;
                        let normalized_index = self.emit_vec_index_failure_guard(
                            object.values[0],
                            index.values[0],
                            line.values[0],
                            column.values[0],
                        )?;
                        let value = self.transfer_owned_opaque_value(&value);
                        let result = self.builder.ins().call(
                            self.runtime[RuntimeFn::VecSetIndexInPlace],
                            &[
                                object.values[0],
                                normalized_index,
                                value,
                                line.values[0],
                                column.values[0],
                            ],
                        );
                        self.release_opaque_handle(self.builder.inst_results(result)[0]);
                        if let Some(place) = receiver_place {
                            self.store_place(place, object.clone())?;
                        }
                        Ok(unit_value(&mut self.builder))
                    }
                    "remove" => {
                        let [argument] = args else {
                            return Err(
                                "direct backend expected `remove()` to receive one value argument"
                                    .to_string(),
                            );
                        };
                        let value = self
                            .load_operand_as_opaque_direct(&argument.value, &element_direct_ty)?;
                        let opcode = self.builder.ins().iconst(types::I64, 1);
                        let zero = self.builder.ins().iconst(types::I64, 0);
                        let inst = self.builder.ins().call(
                            self.runtime[RuntimeFn::CollectionOperation],
                            &[object.values[0], value.values[0], zero, opcode],
                        );
                        if let Some(place) = receiver_place {
                            self.store_place(place, object.clone())?;
                        }
                        self.release_opaque_handle(self.builder.inst_results(inst)[0]);
                        Ok(unit_value(&mut self.builder))
                    }
                    "index" | "count" => {
                        let [argument] = args else {
                            return Err(format!(
                                "direct backend expected `{field}()` to receive one value argument"
                            ));
                        };
                        let value = self
                            .load_operand_as_opaque_direct(&argument.value, &element_direct_ty)?;
                        let opcode = self
                            .builder
                            .ins()
                            .iconst(types::I64, if field == "index" { 2 } else { 3 });
                        let zero = self.builder.ins().iconst(types::I64, 0);
                        let inst = self.builder.ins().call(
                            self.runtime[RuntimeFn::CollectionOperation],
                            &[object.values[0], value.values[0], zero, opcode],
                        );
                        let boxed = self.owned_opaque_result(
                            self.builder.inst_results(inst).to_vec(),
                            Type::named("int64"),
                        );
                        self.coerce_value(boxed, &DirectType::Scalar(ScalarKind::Int64))
                    }
                    "swap" => {
                        let [first_arg, second_arg] = args else {
                            return Err(
                                "direct backend expected `swap()` to receive two index arguments"
                                    .to_string(),
                            );
                        };
                        let loaded_first = self.load_operand(&first_arg.value)?;
                        let first = self
                            .coerce_value(loaded_first, &DirectType::Scalar(ScalarKind::Int64))?;
                        let loaded_second = self.load_operand(&second_arg.value)?;
                        let second = self
                            .coerce_value(loaded_second, &DirectType::Scalar(ScalarKind::Int64))?;
                        let inst = self.builder.ins().call(
                            self.runtime[RuntimeFn::VecSwapInPlace],
                            &[object.values[0], first.values[0], second.values[0]],
                        );
                        if let Some(place) = receiver_place {
                            self.store_place(place, object.clone())?;
                        }
                        let _ = inst;
                        Ok(unit_value(&mut self.builder))
                    }
                    "contains" => {
                        let [argument] = args else {
                            return Err(
                                "direct backend expected `contains()` to receive one value argument"
                                    .to_string(),
                            );
                        };
                        let value = self
                            .load_operand_as_opaque_direct(&argument.value, &element_direct_ty)?;
                        let inst = self.builder.ins().call(
                            self.runtime[RuntimeFn::VecContains],
                            &[object.values[0], value.values[0]],
                        );
                        Ok(ValueRef {
                            values: self.builder.inst_results(inst).to_vec(),
                            ty: DirectType::Scalar(ScalarKind::Bool),
                        })
                    }
                    "insert" => {
                        let [index_arg, value_arg] = args else {
                            return Err(
                                "direct backend expected `insert()` to receive index and value"
                                    .to_string(),
                            );
                        };
                        let loaded_index = self.load_operand(&index_arg.value)?;
                        let index = self
                            .coerce_value(loaded_index, &DirectType::Scalar(ScalarKind::Int64))?;
                        let value = self
                            .load_operand_as_opaque_direct(&value_arg.value, &element_direct_ty)?;
                        let value = self.transfer_owned_opaque_value(&value);
                        let inst = self.builder.ins().call(
                            self.runtime[RuntimeFn::VecInsertInPlace],
                            &[object.values[0], index.values[0], value],
                        );
                        if let Some(place) = receiver_place {
                            self.store_place(place, object.clone())?;
                        }
                        let _ = inst;
                        Ok(unit_value(&mut self.builder))
                    }
                    "clear" => {
                        if !args.is_empty() {
                            return Err("direct backend expected `clear()` to take no arguments"
                                .to_string());
                        }
                        let result = self.builder.ins().call(
                            self.runtime[RuntimeFn::VecClearInPlace],
                            &[object.values[0]],
                        );
                        self.release_opaque_handle(self.builder.inst_results(result)[0]);
                        if let Some(place) = receiver_place {
                            self.store_place(place, object.clone())?;
                        }
                        Ok(unit_value(&mut self.builder))
                    }
                    "reverse" => {
                        if !args.is_empty() {
                            return Err("direct backend expected `reverse()` to take no arguments"
                                .to_string());
                        }
                        let result = self.builder.ins().call(
                            self.runtime[RuntimeFn::VecReverseInPlace],
                            &[object.values[0]],
                        );
                        self.release_opaque_handle(self.builder.inst_results(result)[0]);
                        if let Some(place) = receiver_place {
                            self.store_place(place, object.clone())?;
                        }
                        Ok(unit_value(&mut self.builder))
                    }
                    "extend" => {
                        let [argument] = args else {
                            return Err(
                                "direct backend expected `extend()` to receive one list argument"
                                    .to_string(),
                            );
                        };
                        let loaded_value = self.load_operand(&argument.value)?;
                        let value = self.ensure_opaque(loaded_value)?;
                        let value = self.transfer_owned_opaque_value(&value);
                        let result = self.builder.ins().call(
                            self.runtime[RuntimeFn::VecExtendInPlace],
                            &[object.values[0], value],
                        );
                        self.release_opaque_handle(self.builder.inst_results(result)[0]);
                        if let Some(place) = receiver_place {
                            self.store_place(place, object.clone())?;
                        }
                        Ok(unit_value(&mut self.builder))
                    }
                    "reserve" => {
                        let [argument] = args else {
                            return Err(
                                "direct backend expected `reserve()` to receive one argument"
                                    .to_string(),
                            );
                        };
                        let additional = self.load_operand_with_integer_hint(
                            &argument.value,
                            Some(ScalarKind::Int64),
                        )?;
                        let additional =
                            self.coerce_value(additional, &DirectType::Scalar(ScalarKind::Int64))?;
                        let zero = self.builder.ins().iconst(types::I64, 0);
                        let opcode = self.builder.ins().iconst(types::I64, 4);
                        let inst = self.builder.ins().call(
                            self.runtime[RuntimeFn::CollectionOperation],
                            &[object.values[0], zero, additional.values[0], opcode],
                        );
                        self.release_opaque_handle(self.builder.inst_results(inst)[0]);
                        if let Some(place) = receiver_place {
                            self.store_place(place, object.clone())?;
                        }
                        Ok(unit_value(&mut self.builder))
                    }
                    _ => Err(format!(
                        "direct backend does not know runtime member `{}.{}`",
                        name, field
                    )),
                };
            }
            if name == "dict" {
                let object = self.ensure_opaque(object)?;
                let key_ty = class_args
                    .first()
                    .cloned()
                    .unwrap_or(Type::named("Unknown"));
                let value_ty = class_args.get(1).cloned().unwrap_or(Type::named("Unknown"));
                let key_direct_ty = ensure_direct_type(&key_ty, &self.classes, "dict key")?;
                let value_direct_ty = ensure_direct_type(&value_ty, &self.classes, "Map value")?;
                return match field {
                    "len" => {
                        if !args.is_empty() {
                            return Err(
                                "direct backend expected `len()` to take no arguments".to_string()
                            );
                        }
                        let inst = self
                            .builder
                            .ins()
                            .call(self.runtime[RuntimeFn::MapLen], &[object.values[0]]);
                        let len = self.builder.inst_results(inst)[0];
                        Ok(ValueRef {
                            values: vec![len],
                            ty: DirectType::Scalar(ScalarKind::Int64),
                        })
                    }
                    "is_empty" => {
                        if !args.is_empty() {
                            return Err(
                                "direct backend expected `is_empty()` to take no arguments"
                                    .to_string(),
                            );
                        }
                        let inst = self
                            .builder
                            .ins()
                            .call(self.runtime[RuntimeFn::MapIsEmpty], &[object.values[0]]);
                        Ok(ValueRef {
                            values: self.builder.inst_results(inst).to_vec(),
                            ty: DirectType::Scalar(ScalarKind::Bool),
                        })
                    }
                    "get" => {
                        let [argument] = args else {
                            return Err(
                                "direct backend expected `get()` to receive one key argument"
                                    .to_string(),
                            );
                        };
                        let inst = if let Some(text) = str_literal_key(&argument.value, &key_ty) {
                            // A literal `str` key is looked up without a box.
                            let (key_ptr, key_len) = self.string_constant(text.as_bytes())?;
                            self.builder.ins().call(
                                self.runtime[RuntimeFn::MapGetStr],
                                &[object.values[0], key_ptr, key_len],
                            )
                        } else {
                            let key = self
                                .load_operand_as_opaque_direct(&argument.value, &key_direct_ty)?;
                            self.builder.ins().call(
                                self.runtime[RuntimeFn::MapGet],
                                &[object.values[0], key.values[0]],
                            )
                        };
                        Ok(self.owned_opaque_result(
                            self.builder.inst_results(inst).to_vec(),
                            optional_type(value_ty.clone()),
                        ))
                    }
                    "__index" => {
                        let [key_arg, line_arg, column_arg] = args else {
                            return Err(
                                "direct backend expected internal map indexing to receive key, line, and column"
                                    .to_string(),
                            );
                        };
                        let key =
                            self.load_operand_as_opaque_direct(&key_arg.value, &key_direct_ty)?;
                        let loaded_line = self.load_operand(&line_arg.value)?;
                        let line =
                            self.coerce_value(loaded_line, &DirectType::Scalar(ScalarKind::Int32))?;
                        let loaded_column = self.load_operand(&column_arg.value)?;
                        let column = self
                            .coerce_value(loaded_column, &DirectType::Scalar(ScalarKind::Int32))?;
                        let inst = self.builder.ins().call(
                            self.runtime[RuntimeFn::MapIndex],
                            &[
                                object.values[0],
                                key.values[0],
                                line.values[0],
                                column.values[0],
                            ],
                        );
                        let indexed = self.owned_opaque_result(
                            self.builder.inst_results(inst).to_vec(),
                            value_ty.clone(),
                        );
                        self.coerce_value(indexed, &value_direct_ty)
                    }
                    "set" => {
                        let [key_arg, value_arg] = args else {
                            return Err("direct backend expected `set()` to receive key and value"
                                .to_string());
                        };
                        let key =
                            self.load_operand_as_opaque_direct(&key_arg.value, &key_direct_ty)?;
                        let value =
                            self.load_operand_as_opaque_direct(&value_arg.value, &value_direct_ty)?;
                        let key = self.transfer_owned_opaque_value(&key);
                        let value = self.transfer_owned_opaque_value(&value);
                        let inst = self.builder.ins().call(
                            self.runtime[RuntimeFn::MapSetInPlace],
                            &[object.values[0], key, value],
                        );
                        if let Some(place) = receiver_place {
                            self.store_place(place, object.clone())?;
                        }
                        Ok(self.owned_opaque_result(
                            self.builder.inst_results(inst).to_vec(),
                            optional_type(value_ty.clone()),
                        ))
                    }
                    "__set_index" => {
                        let [key_arg, value_arg, line_arg, column_arg] = args else {
                            return Err(
                                "direct backend expected internal map indexed assignment to receive key, value, line, and column"
                                    .to_string(),
                            );
                        };
                        let key =
                            self.load_operand_as_opaque_direct(&key_arg.value, &key_direct_ty)?;
                        let value =
                            self.load_operand_as_opaque_direct(&value_arg.value, &value_direct_ty)?;
                        let loaded_line = self.load_operand(&line_arg.value)?;
                        let line =
                            self.coerce_value(loaded_line, &DirectType::Scalar(ScalarKind::Int32))?;
                        let loaded_column = self.load_operand(&column_arg.value)?;
                        let column = self
                            .coerce_value(loaded_column, &DirectType::Scalar(ScalarKind::Int32))?;
                        let key = self.transfer_owned_opaque_value(&key);
                        let value = self.transfer_owned_opaque_value(&value);
                        let result = self.builder.ins().call(
                            self.runtime[RuntimeFn::MapSetIndexInPlace],
                            &[
                                object.values[0],
                                key,
                                value,
                                line.values[0],
                                column.values[0],
                            ],
                        );
                        self.release_opaque_handle(self.builder.inst_results(result)[0]);
                        if let Some(place) = receiver_place {
                            self.store_place(place, object.clone())?;
                        }
                        Ok(unit_value(&mut self.builder))
                    }
                    "remove" => {
                        let [argument] = args else {
                            return Err(
                                "direct backend expected `remove()` to receive one key argument"
                                    .to_string(),
                            );
                        };
                        let key =
                            self.load_operand_as_opaque_direct(&argument.value, &key_direct_ty)?;
                        let inst = self.builder.ins().call(
                            self.runtime[RuntimeFn::MapRemoveInPlace],
                            &[object.values[0], key.values[0]],
                        );
                        if let Some(place) = receiver_place {
                            self.store_place(place, object.clone())?;
                        }
                        Ok(self.owned_opaque_result(
                            self.builder.inst_results(inst).to_vec(),
                            optional_type(value_ty.clone()),
                        ))
                    }
                    "contains_key" => {
                        let [argument] = args else {
                            return Err(
                                "direct backend expected `contains_key()` to receive one key argument"
                                    .to_string(),
                            );
                        };
                        let key =
                            self.load_operand_as_opaque_direct(&argument.value, &key_direct_ty)?;
                        let inst = self.builder.ins().call(
                            self.runtime[RuntimeFn::MapContainsKey],
                            &[object.values[0], key.values[0]],
                        );
                        Ok(ValueRef {
                            values: self.builder.inst_results(inst).to_vec(),
                            ty: DirectType::Scalar(ScalarKind::Bool),
                        })
                    }
                    "keys" => {
                        if !args.is_empty() {
                            return Err(
                                "direct backend expected `keys()` to take no arguments".to_string()
                            );
                        }
                        let inst = self
                            .builder
                            .ins()
                            .call(self.runtime[RuntimeFn::MapKeys], &[object.values[0]]);
                        Ok(self.owned_opaque_result(
                            self.builder.inst_results(inst).to_vec(),
                            Type::Named("list".to_string(), vec![key_ty.clone()]),
                        ))
                    }
                    "values" => {
                        if !args.is_empty() {
                            return Err("direct backend expected `values()` to take no arguments"
                                .to_string());
                        }
                        let inst = self
                            .builder
                            .ins()
                            .call(self.runtime[RuntimeFn::MapValues], &[object.values[0]]);
                        Ok(self.owned_opaque_result(
                            self.builder.inst_results(inst).to_vec(),
                            Type::Named("list".to_string(), vec![value_ty.clone()]),
                        ))
                    }
                    "items" => {
                        if !args.is_empty() {
                            return Err("direct backend expected `items()` to take no arguments"
                                .to_string());
                        }
                        let inst = self
                            .builder
                            .ins()
                            .call(self.runtime[RuntimeFn::MapItems], &[object.values[0]]);
                        Ok(self.owned_opaque_result(
                            self.builder.inst_results(inst).to_vec(),
                            Type::Named(
                                "list".to_string(),
                                vec![Type::Tuple(vec![key_ty.clone(), value_ty.clone()])],
                            ),
                        ))
                    }
                    "clear" => {
                        if !args.is_empty() {
                            return Err("direct backend expected `clear()` to take no arguments"
                                .to_string());
                        }
                        let result = self.builder.ins().call(
                            self.runtime[RuntimeFn::MapClearInPlace],
                            &[object.values[0]],
                        );
                        self.release_opaque_handle(self.builder.inst_results(result)[0]);
                        if let Some(place) = receiver_place {
                            self.store_place(place, object.clone())?;
                        }
                        Ok(unit_value(&mut self.builder))
                    }
                    "update" => {
                        let [argument] = args else {
                            return Err(
                                "direct backend expected `update()` to receive one dict argument"
                                    .to_string(),
                            );
                        };
                        let loaded_value = self.load_operand(&argument.value)?;
                        let value = self.ensure_opaque(loaded_value)?;
                        let value = self.transfer_owned_opaque_value(&value);
                        let result = self.builder.ins().call(
                            self.runtime[RuntimeFn::MapExtendInPlace],
                            &[object.values[0], value],
                        );
                        self.release_opaque_handle(self.builder.inst_results(result)[0]);
                        if let Some(place) = receiver_place {
                            self.store_place(place, object.clone())?;
                        }
                        Ok(unit_value(&mut self.builder))
                    }
                    "reserve" => {
                        let [argument] = args else {
                            return Err(
                                "direct backend expected `reserve()` to receive one argument"
                                    .to_string(),
                            );
                        };
                        let additional = self.load_operand_with_integer_hint(
                            &argument.value,
                            Some(ScalarKind::Int64),
                        )?;
                        let additional =
                            self.coerce_value(additional, &DirectType::Scalar(ScalarKind::Int64))?;
                        let zero = self.builder.ins().iconst(types::I64, 0);
                        let opcode = self.builder.ins().iconst(types::I64, 4);
                        let inst = self.builder.ins().call(
                            self.runtime[RuntimeFn::CollectionOperation],
                            &[object.values[0], zero, additional.values[0], opcode],
                        );
                        self.release_opaque_handle(self.builder.inst_results(inst)[0]);
                        if let Some(place) = receiver_place {
                            self.store_place(place, object.clone())?;
                        }
                        Ok(unit_value(&mut self.builder))
                    }
                    _ => Err(format!(
                        "direct backend does not know runtime member `{}.{}`",
                        name, field
                    )),
                };
            }
            if name == "set" {
                let object = self.ensure_opaque(object)?;
                let element_ty = class_args
                    .first()
                    .cloned()
                    .unwrap_or(Type::named("Unknown"));
                let element_direct_ty =
                    ensure_direct_type(&element_ty, &self.classes, "set element")?;
                return match field {
                    "len" => {
                        if !args.is_empty() {
                            return Err(
                                "direct backend expected `len()` to take no arguments".to_string()
                            );
                        }
                        let inst = self
                            .builder
                            .ins()
                            .call(self.runtime[RuntimeFn::SetLen], &[object.values[0]]);
                        let len = self.builder.inst_results(inst)[0];
                        Ok(ValueRef {
                            values: vec![len],
                            ty: DirectType::Scalar(ScalarKind::Int64),
                        })
                    }
                    "is_empty" => {
                        if !args.is_empty() {
                            return Err(
                                "direct backend expected `is_empty()` to take no arguments"
                                    .to_string(),
                            );
                        }
                        let inst = self
                            .builder
                            .ins()
                            .call(self.runtime[RuntimeFn::SetIsEmpty], &[object.values[0]]);
                        Ok(ValueRef {
                            values: self.builder.inst_results(inst).to_vec(),
                            ty: DirectType::Scalar(ScalarKind::Bool),
                        })
                    }
                    "contains" => {
                        let [argument] = args else {
                            return Err(
                                "direct backend expected `contains()` to receive one value argument"
                                    .to_string(),
                            );
                        };
                        let loaded =
                            self.load_operand_for_target(&argument.value, &element_direct_ty)?;
                        let value = self.coerce_value(loaded, &element_direct_ty)?;
                        let inst = if value.ty == DirectType::Scalar(ScalarKind::Int64) {
                            // An `int64` needle is compared without a box.
                            self.builder.ins().call(
                                self.runtime[RuntimeFn::SetContainsInt64],
                                &[object.values[0], value.values[0]],
                            )
                        } else {
                            let value = self.ensure_opaque(value)?;
                            self.builder.ins().call(
                                self.runtime[RuntimeFn::SetContains],
                                &[object.values[0], value.values[0]],
                            )
                        };
                        Ok(ValueRef {
                            values: self.builder.inst_results(inst).to_vec(),
                            ty: DirectType::Scalar(ScalarKind::Bool),
                        })
                    }
                    "add" => {
                        let [argument] = args else {
                            return Err(
                                "direct backend expected `add()` to receive one value argument"
                                    .to_string(),
                            );
                        };
                        let value = self
                            .load_operand_as_opaque_direct(&argument.value, &element_direct_ty)?;
                        let value = self.transfer_owned_opaque_value(&value);
                        self.builder.ins().call(
                            self.runtime[RuntimeFn::SetInsertInPlace],
                            &[object.values[0], value],
                        );
                        if let Some(place) = receiver_place {
                            self.store_place(place, object.clone())?;
                        }
                        Ok(unit_value(&mut self.builder))
                    }
                    "remove" | "discard" => {
                        let [argument] = args else {
                            return Err(format!(
                                "direct backend expected `{field}()` to receive one value argument"
                            ));
                        };
                        let value = self
                            .load_operand_as_opaque_direct(&argument.value, &element_direct_ty)?;
                        let zero = self.builder.ins().iconst(types::I64, 0);
                        let opcode = self
                            .builder
                            .ins()
                            .iconst(types::I64, if field == "remove" { 5 } else { 6 });
                        let inst = self.builder.ins().call(
                            self.runtime[RuntimeFn::CollectionOperation],
                            &[object.values[0], value.values[0], zero, opcode],
                        );
                        if let Some(place) = receiver_place {
                            self.store_place(place, object.clone())?;
                        }
                        self.release_opaque_handle(self.builder.inst_results(inst)[0]);
                        Ok(unit_value(&mut self.builder))
                    }
                    "clear" => {
                        if !args.is_empty() {
                            return Err("direct backend expected `clear()` to take no arguments"
                                .to_string());
                        }
                        let zero = self.builder.ins().iconst(types::I64, 0);
                        let opcode = self.builder.ins().iconst(types::I64, 7);
                        let inst = self.builder.ins().call(
                            self.runtime[RuntimeFn::CollectionOperation],
                            &[object.values[0], zero, zero, opcode],
                        );
                        self.release_opaque_handle(self.builder.inst_results(inst)[0]);
                        if let Some(place) = receiver_place {
                            self.store_place(place, object.clone())?;
                        }
                        Ok(unit_value(&mut self.builder))
                    }
                    "reserve" => {
                        let [argument] = args else {
                            return Err(
                                "direct backend expected `reserve()` to receive one argument"
                                    .to_string(),
                            );
                        };
                        let additional = self.load_operand_with_integer_hint(
                            &argument.value,
                            Some(ScalarKind::Int64),
                        )?;
                        let additional =
                            self.coerce_value(additional, &DirectType::Scalar(ScalarKind::Int64))?;
                        let zero = self.builder.ins().iconst(types::I64, 0);
                        let opcode = self.builder.ins().iconst(types::I64, 4);
                        let inst = self.builder.ins().call(
                            self.runtime[RuntimeFn::CollectionOperation],
                            &[object.values[0], zero, additional.values[0], opcode],
                        );
                        self.release_opaque_handle(self.builder.inst_results(inst)[0]);
                        if let Some(place) = receiver_place {
                            self.store_place(place, object.clone())?;
                        }
                        Ok(unit_value(&mut self.builder))
                    }
                    "__index_option" => {
                        let [argument] = args else {
                            return Err(
                                "direct backend expected internal optional set indexing to receive one argument"
                                    .to_string(),
                            );
                        };
                        let loaded_index = self.load_operand(&argument.value)?;
                        let index = self
                            .coerce_value(loaded_index, &DirectType::Scalar(ScalarKind::Int64))?;
                        let inst = self.builder.ins().call(
                            self.runtime[RuntimeFn::SetIndexOption],
                            &[object.values[0], index.values[0]],
                        );
                        Ok(self.owned_opaque_result(
                            self.builder.inst_results(inst).to_vec(),
                            optional_type(element_ty),
                        ))
                    }
                    "__take_index_option" => {
                        let [argument] = args else {
                            return Err(
                                "direct backend expected internal consuming set indexing to receive one argument"
                                    .to_string(),
                            );
                        };
                        let Some(receiver_place) = receiver_place else {
                            return Err(
                                "direct backend expected internal consuming set indexing to provide its private receiver place"
                                    .to_string(),
                            );
                        };
                        let loaded_index = self.load_operand(&argument.value)?;
                        let index = self
                            .coerce_value(loaded_index, &DirectType::Scalar(ScalarKind::Int64))?;
                        let inst = self.builder.ins().call(
                            self.runtime[RuntimeFn::SetTakeIndexInPlace],
                            &[object.values[0], index.values[0]],
                        );
                        self.store_place(receiver_place, object.clone())?;
                        Ok(self.owned_opaque_result(
                            self.builder.inst_results(inst).to_vec(),
                            optional_type(element_ty),
                        ))
                    }
                    _ => Err(format!(
                        "direct backend does not know runtime member `{}.{}`",
                        name, field
                    )),
                };
            }
            // These handles own no host resource, so leaving their scope
            // closes nothing.
            if field == "close"
                && matches!(
                    name.as_str(),
                    "net.HttpExchange" | "net.HttpResponse" | "net.WebSocketListener"
                )
            {
                let _ = self.ensure_opaque(object)?;
                return Ok(unit_value(&mut self.builder));
            }
            if ResourceOp::ALL
                .iter()
                .any(|op| op.receiver_type_name() == name)
            {
                return Err(format!(
                    "direct backend does not know runtime member `{name}.{field}`"
                ));
            }
            if self.classes.contains_key(name)
                || (BuiltinMember::resolve(name, field).is_none()
                    && self.find_trait_method(object_ty, field).is_some())
            {
                if let Ok(result) = self.compile_class_member_call(
                    name,
                    Some(object_ty.clone()),
                    object.clone(),
                    field,
                    receiver_place,
                    args,
                    None,
                ) {
                    return Ok(result);
                }
            }
            if name == "Queue" {
                let object = self.ensure_opaque(object)?;
                let element_ty = class_args
                    .first()
                    .cloned()
                    .unwrap_or(Type::named("Unknown"));
                let element_direct_ty =
                    ensure_direct_type(&element_ty, &self.classes, "Queue element")?;
                return match field {
                    "put" => {
                        let mut value_arg: Option<&MirArg> = None;
                        let mut timeout_arg: Option<&MirArg> = None;
                        for argument in args {
                            match argument.name.as_deref() {
                                Some("value") => value_arg = Some(argument),
                                Some("timeout") => timeout_arg = Some(argument),
                                Some(other) => {
                                    return Err(format!(
                                        "direct backend expected `put()` arguments to use `value` and optional `timeout`, found `{}`",
                                        other
                                    ))
                                }
                                None if value_arg.is_none() => value_arg = Some(argument),
                                None if timeout_arg.is_none() => timeout_arg = Some(argument),
                                None => {
                                    return Err(
                                        "direct backend expected `put(value, timeout=...)`".to_string(),
                                    )
                                }
                            }
                        }
                        let Some(argument) = value_arg else {
                            return Err(
                                "direct backend expected `put()` to receive a value argument"
                                    .to_string(),
                            );
                        };
                        let value = self
                            .load_operand_as_opaque_direct(&argument.value, &element_direct_ty)?;
                        let inst = if let Some(timeout_arg) = timeout_arg {
                            let timeout = self.load_operand(&timeout_arg.value)?;
                            let timeout = self.ensure_opaque(timeout)?;
                            let value = self.transfer_owned_opaque_value(&value);
                            self.builder.ins().call(
                                self.runtime[RuntimeFn::ChannelSendTimeoutValue],
                                &[object.values[0], value, timeout.values[0]],
                            )
                        } else {
                            let value = self.transfer_owned_opaque_value(&value);
                            self.builder.ins().call(
                                self.runtime[RuntimeFn::ChannelSend],
                                &[object.values[0], value],
                            )
                        };
                        Ok(self.owned_opaque_result(
                            self.builder.inst_results(inst).to_vec(),
                            Type::Named(
                                "Result".to_string(),
                                vec![
                                    Type::Unit,
                                    Type::Named("SendError".to_string(), vec![element_ty.clone()]),
                                ],
                            ),
                        ))
                    }
                    "try_put" => {
                        let [argument] = args else {
                            return Err(
                                "direct backend expected `try_put()` to receive one argument"
                                    .to_string(),
                            );
                        };
                        if argument.name.as_deref() != Some("value") && argument.name.is_some() {
                            return Err(
                                "direct backend expected `try_put()` to receive only `value=`"
                                    .to_string(),
                            );
                        }
                        let value = self
                            .load_operand_as_opaque_direct(&argument.value, &element_direct_ty)?;
                        let value = self.transfer_owned_opaque_value(&value);
                        let inst = self.builder.ins().call(
                            self.runtime[RuntimeFn::ChannelTrySend],
                            &[object.values[0], value],
                        );
                        Ok(self.owned_opaque_result(
                            self.builder.inst_results(inst).to_vec(),
                            Type::Named(
                                "Result".to_string(),
                                vec![
                                    Type::Unit,
                                    Type::Named("SendError".to_string(), vec![element_ty.clone()]),
                                ],
                            ),
                        ))
                    }
                    "get" => {
                        let inst = match args {
                            [] => self
                                .builder
                                .ins()
                                .call(self.runtime[RuntimeFn::ChannelRecv], &[object.values[0]]),
                            [argument] => {
                                if argument.name.as_deref() != Some("timeout")
                                    && argument.name.is_some()
                                {
                                    return Err(
                                        "direct backend expected `get()` or `get(timeout=...)`"
                                            .to_string(),
                                    );
                                }
                                let timeout = self.load_operand(&argument.value)?;
                                let timeout = self.ensure_opaque(timeout)?;
                                self.builder.ins().call(
                                    self.runtime[RuntimeFn::ChannelRecvTimeoutValue],
                                    &[object.values[0], timeout.values[0]],
                                )
                            }
                            _ => {
                                return Err("direct backend expected `get()` or `get(timeout=...)`"
                                    .to_string())
                            }
                        };
                        Ok(self.owned_opaque_result(
                            self.builder.inst_results(inst).to_vec(),
                            Type::Named(
                                "QueueReceive".to_string(),
                                vec![class_args
                                    .first()
                                    .cloned()
                                    .unwrap_or(Type::named("Unknown"))],
                            ),
                        ))
                    }
                    "__get_in_task_group" => {
                        let [task_group] = args else {
                            return Err(
                                "direct backend expected internal `__get_in_task_group()` to receive one task-group argument"
                                    .to_string(),
                            );
                        };
                        let task_group = self.load_operand(&task_group.value)?;
                        let task_group = self.ensure_opaque(task_group)?;
                        let inst = self.builder.ins().call(
                            self.runtime[RuntimeFn::ChannelRecvInTaskGroup],
                            &[object.values[0], task_group.values[0]],
                        );
                        Ok(self.owned_opaque_result(
                            self.builder.inst_results(inst).to_vec(),
                            Type::Named(
                                "QueueReceive".to_string(),
                                vec![class_args
                                    .first()
                                    .cloned()
                                    .unwrap_or(Type::named("Unknown"))],
                            ),
                        ))
                    }
                    "__get_with_registered_producers" => {
                        if !args.is_empty() {
                            return Err(
                                "direct backend expected internal `__get_with_registered_producers()` to take no arguments"
                                    .to_string(),
                            );
                        }
                        let inst = self.builder.ins().call(
                            self.runtime[RuntimeFn::ChannelRecvWithRegisteredProducers],
                            &[object.values[0]],
                        );
                        Ok(self.owned_opaque_result(
                            self.builder.inst_results(inst).to_vec(),
                            Type::Named(
                                "QueueReceive".to_string(),
                                vec![class_args
                                    .first()
                                    .cloned()
                                    .unwrap_or(Type::named("Unknown"))],
                            ),
                        ))
                    }
                    "poll" => {
                        let inst = match args {
                            [] => self.builder.ins().call(
                                self.runtime[RuntimeFn::ChannelRecvOrNone],
                                &[object.values[0]],
                            ),
                            [argument] => {
                                if argument.name.as_deref() != Some("timeout")
                                    && argument.name.is_some()
                                {
                                    return Err(
                                        "direct backend expected `poll()` or `poll(timeout=...)`"
                                            .to_string(),
                                    );
                                }
                                let timeout = self.load_operand(&argument.value)?;
                                let timeout = self.ensure_opaque(timeout)?;
                                self.builder.ins().call(
                                    self.runtime[RuntimeFn::ChannelRecvOrNoneTimeoutValue],
                                    &[object.values[0], timeout.values[0]],
                                )
                            }
                            _ => {
                                return Err(
                                    "direct backend expected `poll()` or `poll(timeout=...)`"
                                        .to_string(),
                                )
                            }
                        };
                        Ok(self.owned_opaque_result(
                            self.builder.inst_results(inst).to_vec(),
                            optional_type(element_ty.clone()),
                        ))
                    }
                    "get_or" => {
                        let bound = ordered_optional_named_args(&["default", "timeout"], args)?;
                        let default = required_named_arg(
                            bound[0],
                            "direct backend expected `get_or()` to receive `default`",
                        )?;
                        let loaded = self.load_operand(&default.value)?;
                        let loaded = self.ensure_opaque(loaded)?;
                        let inst = match bound[1] {
                            Some(timeout) => {
                                let timeout = self.load_operand(&timeout.value)?;
                                let timeout = self.ensure_opaque(timeout)?;
                                let default = self.transfer_owned_opaque_value(&loaded);
                                self.builder.ins().call(
                                    self.runtime[RuntimeFn::ChannelRecvOrValueTimeoutValue],
                                    &[object.values[0], default, timeout.values[0]],
                                )
                            }
                            None => {
                                let default = self.transfer_owned_opaque_value(&loaded);
                                self.builder.ins().call(
                                    self.runtime[RuntimeFn::ChannelRecvOrValue],
                                    &[object.values[0], default],
                                )
                            }
                        };
                        Ok(self.owned_opaque_result(
                            self.builder.inst_results(inst).to_vec(),
                            class_args
                                .first()
                                .cloned()
                                .unwrap_or(Type::named("Unknown")),
                        ))
                    }
                    "close" => {
                        if !args.is_empty() {
                            return Err("direct backend expected `close()` to take no arguments"
                                .to_string());
                        }
                        let inst = self
                            .builder
                            .ins()
                            .call(self.runtime[RuntimeFn::ChannelClose], &[object.values[0]]);
                        self.release_opaque_handle(self.builder.inst_results(inst)[0]);
                        Ok(unit_value(&mut self.builder))
                    }
                    _ => Err(format!(
                        "direct backend does not know runtime member `{}.{}`",
                        name, field
                    )),
                };
            }
            if name == "Task" {
                let object = self.ensure_opaque(object)?;
                return match field {
                    "result" => {
                        let inst = match args {
                            [] => self
                                .builder
                                .ins()
                                .call(self.runtime[RuntimeFn::TaskJoin], &[object.values[0]]),
                            [argument] => {
                                if argument.name.as_deref() != Some("timeout")
                                    && argument.name.is_some()
                                {
                                    return Err(
                                        "direct backend expected `result()` or `result(timeout=...)`"
                                            .to_string(),
                                    );
                                }
                                let timeout = self.load_operand(&argument.value)?;
                                let timeout = self.ensure_opaque(timeout)?;
                                self.builder.ins().call(
                                    self.runtime[RuntimeFn::TaskJoinTimeoutValue],
                                    &[object.values[0], timeout.values[0]],
                                )
                            }
                            _ => {
                                return Err(
                                    "direct backend expected `result()` or `result(timeout=...)`"
                                        .to_string(),
                                )
                            }
                        };
                        Ok(self.owned_opaque_result(
                            self.builder.inst_results(inst).to_vec(),
                            Type::Named(
                                "TaskResult".to_string(),
                                vec![class_args.first().cloned().unwrap_or(Type::Unit)],
                            ),
                        ))
                    }
                    "poll" => {
                        let inst = match args {
                            [] => self
                                .builder
                                .ins()
                                .call(self.runtime[RuntimeFn::TaskJoinOrNone], &[object.values[0]]),
                            [argument] => {
                                if argument.name.as_deref() != Some("timeout")
                                    && argument.name.is_some()
                                {
                                    return Err(
                                        "direct backend expected `poll()` or `poll(timeout=...)`"
                                            .to_string(),
                                    );
                                }
                                let timeout = self.load_operand(&argument.value)?;
                                let timeout = self.ensure_opaque(timeout)?;
                                self.builder.ins().call(
                                    self.runtime[RuntimeFn::TaskJoinOrNoneTimeoutValue],
                                    &[object.values[0], timeout.values[0]],
                                )
                            }
                            _ => {
                                return Err(
                                    "direct backend expected `poll()` or `poll(timeout=...)`"
                                        .to_string(),
                                )
                            }
                        };
                        Ok(self.owned_opaque_result(
                            self.builder.inst_results(inst).to_vec(),
                            optional_type(class_args.first().cloned().unwrap_or(Type::Unit)),
                        ))
                    }
                    "result_or" => {
                        let bound = ordered_optional_named_args(&["default", "timeout"], args)?;
                        let default = required_named_arg(
                            bound[0],
                            "direct backend expected `result_or()` to receive `default`",
                        )?;
                        let loaded = self.load_operand(&default.value)?;
                        let loaded = self.ensure_opaque(loaded)?;
                        let inst = match bound[1] {
                            Some(timeout) => {
                                let timeout = self.load_operand(&timeout.value)?;
                                let timeout = self.ensure_opaque(timeout)?;
                                let default = self.transfer_owned_opaque_value(&loaded);
                                self.builder.ins().call(
                                    self.runtime[RuntimeFn::TaskJoinOrValueTimeoutValue],
                                    &[object.values[0], default, timeout.values[0]],
                                )
                            }
                            None => {
                                let default = self.transfer_owned_opaque_value(&loaded);
                                self.builder.ins().call(
                                    self.runtime[RuntimeFn::TaskJoinOrValue],
                                    &[object.values[0], default],
                                )
                            }
                        };
                        Ok(self.owned_opaque_result(
                            self.builder.inst_results(inst).to_vec(),
                            class_args.first().cloned().unwrap_or(Type::Unit),
                        ))
                    }
                    _ => Err(format!(
                        "direct backend does not know runtime member `{}.{}`",
                        name, field
                    )),
                };
            }
            if name == "TaskGroup" {
                let object = self.ensure_opaque(object)?;
                return match field {
                    "cancel" => {
                        if !args.is_empty() {
                            return Err("direct backend expected `cancel()` to take no arguments"
                                .to_string());
                        }
                        let inst = self.builder.ins().call(
                            self.runtime[RuntimeFn::TaskGroupCancel],
                            &[object.values[0]],
                        );
                        self.release_opaque_handle(self.builder.inst_results(inst)[0]);
                        Ok(unit_value(&mut self.builder))
                    }
                    "close" => {
                        if !args.is_empty() {
                            return Err("direct backend expected `close()` to take no arguments"
                                .to_string());
                        }
                        let cancel_before = self.builder.ins().iconst(types::I64, 0);
                        let inst = self.builder.ins().call(
                            self.runtime[RuntimeFn::TaskGroupClose],
                            &[object.values[0], cancel_before],
                        );
                        self.release_opaque_handle(self.builder.inst_results(inst)[0]);
                        Ok(unit_value(&mut self.builder))
                    }
                    _ => Err(format!(
                        "direct backend does not know runtime member `{}.{}`",
                        name, field
                    )),
                };
            }
            let _ = class_args;
        }

        let candidates = self.dynamic_method_candidates(field, trait_name);
        if candidates.is_empty() {
            return Err(format!(
                "direct backend does not know dynamic method `.{}` on `{}`",
                field, object_ty
            ));
        }
        // An erased receiver (a generic body's `T`) may hold a union whose
        // active member carries the method (ADR-0052 A8). The interpreter
        // inspects the runtime value; the direct backend branches on it.
        if matches!(object_ty, Type::TypeParam(_)) {
            return self.compile_erased_receiver_dispatch(
                object,
                field,
                receiver_place,
                args,
                trait_name,
                &candidates,
            );
        }
        if candidates.len() == 1 {
            let Type::Named(candidate_name, _) = &candidates[0].0 else {
                return Err(format!(
                    "direct backend does not know how to call dynamic method `.{}` for `{}`",
                    field, candidates[0].0
                ));
            };
            return self.compile_class_member_call(
                candidate_name,
                Some(candidates[0].0.clone()),
                object,
                field,
                receiver_place,
                args,
                trait_name,
            );
        }

        let result_ty = self.dynamic_result_type(&candidates)?;
        let join_block = self.builder.create_block();
        let result_vars = self.declare_temporary_result_storage(&result_ty)?;
        self.emit_candidate_chain(
            object,
            receiver_place,
            &candidates,
            field,
            args,
            trait_name,
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
        self.owned_opaque_temporaries.clear();
        self.load_result_vars(&result_vars, result_ty)
    }

    /// The common result type of every runtime candidate, or an opaque
    /// unknown when the candidates disagree.
    fn dynamic_result_type(
        &self,
        candidates: &[(Type, MirMethod)],
    ) -> std::result::Result<DirectType, String> {
        let result_types = candidates
            .iter()
            .map(|(_, method)| self.call_result_type(&method.function_name))
            .collect::<std::result::Result<Vec<_>, _>>()?;
        if result_types.windows(2).all(|window| window[0] == window[1]) {
            self.call_result_type(&candidates[0].1.function_name)
        } else {
            Ok(DirectType::Opaque(Type::named("Unknown")))
        }
    }

    /// Dispatches an erased receiver on its runtime value: a union routes
    /// through its active payload (with the active-payload write-back
    /// projection for the receiver place), any other value dispatches
    /// directly. Both paths join on the same result storage.
    fn compile_erased_receiver_dispatch(
        &mut self,
        object: ValueRef,
        field: &str,
        receiver_place: Option<&str>,
        args: &[MirArg],
        trait_name: Option<&str>,
        candidates: &[(Type, MirMethod)],
    ) -> std::result::Result<ValueRef, String> {
        let raw = self.ensure_opaque(object)?;
        let result_ty = self.dynamic_result_type(candidates)?;
        let join_block = self.builder.create_block();
        let result_vars = self.declare_temporary_result_storage(&result_ty)?;
        let probe = self
            .builder
            .ins()
            .call(self.runtime[RuntimeFn::ValueIsUnion], &[raw.values[0]]);
        let is_union = self.builder.inst_results(probe)[0];
        let union_block = self.builder.create_block();
        let plain_block = self.builder.create_block();
        self.builder
            .ins()
            .brif(is_union, union_block, &[], plain_block, &[]);
        let caller_owned = self.owned_opaque_temporaries.clone();

        self.builder.switch_to_block(union_block);
        self.builder.seal_block(union_block);
        self.owned_opaque_temporaries = caller_owned.clone();
        let mutates_receiver = candidates
            .iter()
            .any(|(_, method)| method.receiver == Some(MirReceiverKind::BorrowMut));
        if mutates_receiver {
            // No static write-back path exists for the payload of a union
            // behind an erased place; refuse at run time rather than mutate
            // a detached copy.
            self.emit_pending_cleanups(true)?;
            let line = self.builder.ins().iconst(types::I64, 0);
            let column = self.builder.ins().iconst(types::I64, 0);
            self.builder.ins().call(
                self.runtime[RuntimeFn::FailErasedUnionMutableReceiver],
                &[line, column],
            );
            self.builder.ins().trap(TrapCode::unwrap_user(1));
        } else {
            let consumes = candidates[0].1.receiver == Some(MirReceiverKind::Value);
            let consume_flag = self.builder.ins().iconst(types::I64, i64::from(consumes));
            let inst = self.builder.ins().call(
                self.runtime[RuntimeFn::UnionActivePayload],
                &[raw.values[0], consume_flag],
            );
            let payload = self.owned_opaque_result(
                self.builder.inst_results(inst).to_vec(),
                Type::named("Unknown"),
            );
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
                field,
                args,
                trait_name,
                &result_ty,
                &result_vars,
                join_block,
            )?;
        }

        self.builder.switch_to_block(plain_block);
        self.builder.seal_block(plain_block);
        self.owned_opaque_temporaries = caller_owned;
        self.emit_candidate_chain(
            raw,
            receiver_place,
            candidates,
            field,
            args,
            trait_name,
            &result_ty,
            &result_vars,
            join_block,
        )?;

        self.builder.switch_to_block(join_block);
        self.builder.seal_block(join_block);
        self.owned_opaque_temporaries.clear();
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
        candidates: &[(Type, MirMethod)],
        field: &str,
        args: &[MirArg],
        trait_name: Option<&str>,
        result_ty: &DirectType,
        result_vars: &[Variable],
        join_block: cranelift_codegen::ir::Block,
    ) -> std::result::Result<(), String> {
        let mut current_fallthrough = None;
        // Every runtime candidate starts from the same ownership state. Emitting
        // one candidate's taken edge can consume or release an owned projected
        // receiver, but that edge is skipped when control falls through to the
        // next candidate. Keep the compile-time ledger path-local so a later
        // candidate still emits the release owed by its runtime path.
        let caller_owned = self.owned_opaque_temporaries.clone();
        for (candidate_ty, _method) in candidates.iter() {
            // Every implementation target is a named type; anything else is
            // forged metadata, refused before a block is left unterminated.
            let Type::Named(candidate_name, _) = candidate_ty else {
                return Err(format!(
                    "direct backend does not know how to call dynamic method `.{}` for `{}`",
                    field, candidate_ty
                ));
            };
            self.owned_opaque_temporaries = caller_owned.clone();
            let matched = self.value_matches_runtime_type(object.values[0], candidate_ty)?;
            let then_block = self.builder.create_block();
            let else_block = self.builder.create_block();
            self.builder
                .ins()
                .brif(matched, then_block, &[], else_block, &[]);
            self.builder.switch_to_block(then_block);
            self.owned_opaque_temporaries = caller_owned.clone();
            let call_result = self.compile_class_member_call(
                candidate_name,
                Some(candidate_ty.clone()),
                object.clone(),
                field,
                receiver_place,
                args,
                trait_name,
            )?;
            let coerced_result = self.coerce_value(call_result, result_ty)?;
            self.store_result_vars(result_vars, &coerced_result)?;
            self.release_all_temporary_owned();
            self.builder.ins().jump(join_block, &[]);
            self.builder.seal_block(then_block);
            self.builder.switch_to_block(else_block);
            self.owned_opaque_temporaries = caller_owned.clone();
            current_fallthrough = Some(else_block);
        }
        if let Some(else_block) = current_fallthrough {
            self.builder.switch_to_block(else_block);
            self.emit_pending_cleanups(true)?;
            self.builder.ins().trap(TrapCode::unwrap_user(1));
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
        let init = self.builder.ins().call(
            self.runtime[RuntimeFn::InstanceEmpty],
            &[class_ptr, class_len],
        );
        let current =
            self.owned_opaque_result(self.builder.inst_results(init).to_vec(), target_ty.clone());
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
            self.builder.ins().call(
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
            .ins()
            .iconst(types::I64, function_params.len() as i64);
        let buffer_call = self
            .builder
            .ins()
            .call(self.runtime[RuntimeFn::ArgBufferNew], &[arg_count_value]);
        let buffer = self.builder.inst_results(buffer_call)[0];
        let guard_call = self.builder.ins().call(
            self.runtime[RuntimeFn::TaskArgBufferGuard],
            &[buffer, arg_count_value],
        );
        let buffer_guard = self.builder.inst_results(guard_call)[0];
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
            let index_value = self.builder.ins().iconst(types::I64, index as i64);
            self.builder.ins().call(
                self.runtime[RuntimeFn::ArgBufferStore],
                &[buffer, index_value, value.values[0]],
            );
        }
        let transfer_defaults = self.builder.ins().iconst(types::I64, 1);
        self.builder.ins().call(
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
            let raw =
                self.builder
                    .ins()
                    .load(types::I64, MemFlags::new(), buffer, (index as i32) * 8);
            self.tag_raw_opaque_runtime_type(raw, &expected[index])?;
        }
        let returns_handle_value = self
            .builder
            .ins()
            .iconst(types::I64, if mode.returns_handle { 1 } else { 0 });
        let result_is_copy_value = self
            .builder
            .ins()
            .iconst(types::I64, if mode.result_is_copy { 1 } else { 0 });
        let (stack_size_present_value, stack_size_value) = match stack_size {
            Some(stack_size) => {
                let target = DirectType::Scalar(ScalarKind::Int64);
                let value = self.load_operand_for_target(stack_size, &target)?;
                (
                    self.builder.ins().iconst(types::I64, 1),
                    self.coerce_value(value, &target)?.values[0],
                )
            }
            None => (
                self.builder.ins().iconst(types::I64, 0),
                self.builder.ins().iconst(types::I64, 0),
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
        let spawn_line = self
            .builder
            .ins()
            .iconst(types::I64, spawn_span.line as i64);
        let spawn_column = self
            .builder
            .ins()
            .iconst(types::I64, spawn_span.column as i64);
        // Every operation that can trap after raw allocation has completed.
        // Transfer the buffer from the direct cleanup stack to the task runtime
        // only across this allocation-free call boundary.
        self.builder.ins().call(
            self.runtime[RuntimeFn::TaskArgBufferDisarm],
            &[buffer_guard],
        );
        let call = self.builder.ins().call(
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
            DirectType::Opaque(ty) => {
                Ok(self.owned_opaque_result(self.builder.inst_results(call).to_vec(), ty))
            }
            _ => {
                self.release_opaque_handle(self.builder.inst_results(call)[0]);
                Ok(unit_value(&mut self.builder))
            }
        }
    }

    fn value_matches_type(
        &mut self,
        value: Value,
        type_name: &str,
    ) -> std::result::Result<Value, String> {
        let (ptr, len) = self.string_constant(type_name.as_bytes())?;
        let inst = self.builder.ins().call(
            self.runtime[RuntimeFn::ValueTypeMatches],
            &[value, ptr, len],
        );
        Ok(self.builder.inst_results(inst)[0])
    }

    fn value_matches_runtime_type(
        &mut self,
        value: Value,
        ty: &Type,
    ) -> std::result::Result<Value, String> {
        match ty {
            Type::Union(_) => {
                let pattern = crate::native_runtime::canonical_runtime_type_name(ty);
                self.value_matches_type(value, &pattern)
            }
            Type::TypeParam(_) => Ok(self.builder.ins().iconst(types::I64, 1)),
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
                    .ins()
                    .call(self.runtime[RuntimeFn::ValueHasRuntimeType], &[value]);
                let tagged = self.builder.inst_results(tagged)[0];
                self.builder
                    .ins()
                    .brif(tagged, join_block, &[exact], structural_block, &[]);
                self.builder.switch_to_block(structural_block);
                self.builder
                    .ins()
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
                    let inst = self.builder.ins().call(
                        self.runtime[RuntimeFn::InstanceGetField],
                        &[value, field_ptr, field_len],
                    );
                    let field_value = self.builder.inst_results(inst)[0];
                    let field_matches = self.value_matches_runtime_type(field_value, &field_ty)?;
                    self.release_opaque_handle(field_value);
                    structural = self.builder.ins().band(structural, field_matches);
                }
                let matched = self.builder.ins().bor(exact, structural);
                self.builder.ins().jump(join_block, &[matched]);
                self.builder.seal_block(inspect_block);
                self.builder.switch_to_block(join_block);
                self.builder.seal_block(join_block);
                Ok(self.builder.block_params(join_block)[0])
            }
        }
    }

    fn span_values(&mut self, span: Option<Span>) -> (Value, Value) {
        let (line, column) = span
            .map(|span| (span.line as i64, span.column as i64))
            .unwrap_or((0, 0));
        (
            self.builder.ins().iconst(types::I64, line),
            self.builder.ins().iconst(types::I64, column),
        )
    }

    fn dynamic_method_candidates(
        &self,
        field: &str,
        trait_name: Option<&str>,
    ) -> Vec<(Type, MirMethod)> {
        let mut candidates = Vec::new();
        if trait_name.is_none() {
            for class in self.classes.values() {
                if let Some(method) = class.methods.iter().find(|method| method.name == field) {
                    candidates.push((usize::MAX, Type::named(&class.name), method.clone()));
                }
            }
        }
        for trait_impl in &self.trait_impls {
            if trait_name.is_some_and(|name| trait_impl.trait_name != name) {
                continue;
            }
            if let Some(method) = trait_impl
                .methods
                .iter()
                .find(|method| method.name == field)
            {
                candidates.push((
                    crate::sema::trait_impl_specificity_parts(
                        &trait_impl.for_type,
                        &trait_impl.trait_args,
                    ),
                    trait_impl.for_type.clone(),
                    method.clone(),
                ));
            }
        }
        candidates.sort_by_key(|candidate| std::cmp::Reverse(candidate.0));
        candidates
            .into_iter()
            .map(|(_, ty, method)| (ty, method))
            .collect()
    }

    fn find_trait_method(&self, ty: &Type, field: &str) -> Option<&MirMethod> {
        self.find_trait_method_with_identity(ty, None, field)
    }

    fn find_trait_method_for_trait(
        &self,
        ty: &Type,
        trait_name: &str,
        field: &str,
    ) -> Option<&MirMethod> {
        self.find_trait_method_with_identity(ty, Some(trait_name), field)
    }

    fn find_trait_method_with_identity(
        &self,
        ty: &Type,
        trait_name: Option<&str>,
        field: &str,
    ) -> Option<&MirMethod> {
        let mut best = None;
        let mut best_specificity = 0usize;
        let mut ambiguous = false;
        for trait_impl in &self.trait_impls {
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
        if ambiguous {
            None
        } else {
            best
        }
    }

    fn declare_temporary_result_storage(
        &mut self,
        ty: &DirectType,
    ) -> std::result::Result<Vec<Variable>, String> {
        let mut vars = Vec::new();
        for abi in ty.abi_types() {
            let variable = Variable::from_u32(self.next_variable_index as u32);
            self.next_variable_index += 1;
            self.builder.declare_var(variable, abi);
            let zero = match abi {
                t if t == types::F64 => self.builder.ins().f64const(Ieee64::with_float(0.0)),
                _ => self.builder.ins().iconst(abi, 0),
            };
            self.builder.def_var(variable, zero);
            vars.push(variable);
        }
        Ok(vars)
    }

    fn store_result_vars(
        &mut self,
        vars: &[Variable],
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
        vars: &[Variable],
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

fn unit_value(builder: &mut FunctionBuilder<'_>) -> ValueRef {
    ValueRef {
        values: vec![builder.ins().iconst(types::I64, 0)],
        ty: DirectType::Scalar(ScalarKind::Unit),
    }
}

/// The trait-impl method a concrete class provides for `field`; the checker
/// has already rejected an ambiguous name, so the first impl is the one.
fn find_concrete_impl_method<'a>(
    trait_impls: &'a [MirTraitImpl],
    class_name: &str,
    field: &str,
) -> Option<&'a MirMethod> {
    trait_impls
        .iter()
        .filter(|trait_impl| {
            matches!(&trait_impl.for_type, Type::Named(name, args) if name == class_name && args.is_empty())
        })
        .find_map(|trait_impl| trait_impl.methods.iter().find(|method| method.name == field))
}

/// A dotted place whose every segment is a declared field. A narrowed union
/// local reaches its fields through compiler segments such as
/// `__union_payload_0`, and its slot holds the union, not the instance, so
/// the field cannot be moved out of it by name.
fn is_plain_field_path(place: &str) -> bool {
    !place.split('.').any(|segment| segment.starts_with("__"))
}

/// Whether `other` reaches into a different root than `lent` through a
/// field or element. Two borrowed parameters can name one instance, so
/// `x.items` and `y.items` may be the same slot even though the places
/// differ. A whole value such as `ys` stays readable while a field is lent:
/// it holds the field's value, not its slot.
fn projects_from_other_root(lent: &str, other: &str, other_has_elements: bool) -> bool {
    let root = |place: &str| {
        place
            .split_once('.')
            .map_or(place, |(root, _)| root)
            .to_string()
    };
    root(lent) != root(other) && (other.contains('.') || other_has_elements)
}

/// Whether one dotted place contains the other, or both are the same.
fn places_overlap(left: &str, right: &str) -> bool {
    let contains = |outer: &str, inner: &str| {
        inner == outer
            || inner
                .strip_prefix(outer)
                .is_some_and(|rest| rest.starts_with('.'))
    };
    contains(left, right) || contains(right, left)
}

fn find_method<'a>(class: Option<&'a MirClass>, field: &str) -> Option<&'a MirMethod> {
    let class = class?;
    class
        .methods
        .iter()
        .find(|&method| method.name == field)
        .map(|v| v as _)
}

fn declare_runtime_helper(
    module: &mut ObjectModule,
    function: RuntimeFn,
) -> std::result::Result<FuncId, String> {
    let params = function
        .params()
        .iter()
        .map(|param| abi_type(*param))
        .collect::<Vec<_>>();
    declare_runtime_function(
        module,
        function.symbol(),
        &params,
        function.result().map(abi_type),
    )
}

fn declare_runtime_function(
    module: &mut ObjectModule,
    name: &str,
    params: &[cranelift_codegen::ir::Type],
    result: Option<cranelift_codegen::ir::Type>,
) -> std::result::Result<FuncId, String> {
    let mut sig = module.make_signature();
    for param in params {
        sig.params.push(AbiParam::new(*param));
    }
    if let Some(result) = result {
        sig.returns.push(AbiParam::new(result));
    }
    match module.declare_function(name, Linkage::Import, &sig) {
        Ok(id) => Ok(id),
        Err(error) => Err(format!(
            "failed to declare runtime function `{}`: {}",
            name, error
        )),
    }
}

fn abi_type(ty: AbiType) -> cranelift_codegen::ir::Type {
    match ty {
        AbiType::I32 => types::I32,
        AbiType::I64 => types::I64,
        AbiType::F64 => types::F64,
    }
}

fn declare_string_constant(
    object: &mut ObjectModule,
    string_data: &mut HashMap<Vec<u8>, DataId>,
    builder: &mut FunctionBuilder<'_>,
    bytes: &[u8],
) -> std::result::Result<(Value, Value), String> {
    let id = string_data_id(object, string_data, bytes)?;
    let global = object.declare_data_in_func(id, builder.func);
    let ptr = builder.ins().symbol_value(types::I64, global);
    let len = builder.ins().iconst(types::I64, bytes.len() as i64);
    Ok((ptr, len))
}

fn string_data_id(
    object: &mut ObjectModule,
    string_data: &mut HashMap<Vec<u8>, DataId>,
    bytes: &[u8],
) -> std::result::Result<DataId, String> {
    if let Some(id) = string_data.get(bytes) {
        return Ok(*id);
    }
    let name = format!("aura_data_{}", string_data.len());
    let id = try_or_string_error!(
        object.declare_data(&name, Linkage::Local, false, false),
        "failed to declare string data: {}"
    );
    let mut data = DataDescription::new();
    data.define(bytes.to_vec().into_boxed_slice());
    try_or_string_error!(
        object.define_data(id, &data),
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
/// runtime marks it validated on the function's first call.
fn declare_frame_descriptor(
    object: &mut ObjectModule,
    string_data: &mut HashMap<Vec<u8>, DataId>,
    name: &str,
    span: Span,
    path: &[u8],
    function: &[u8],
) -> std::result::Result<DataId, String> {
    let path_id = string_data_id(object, string_data, path)?;
    let function_id = string_data_id(object, string_data, function)?;
    let id = try_or_string_error!(
        object.declare_data(name, Linkage::Local, true, false),
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
    let mut data = DataDescription::new();
    data.define(bytes.into_boxed_slice());
    data.set_align(8);
    let path_global = object.declare_data_in_data(path_id, &mut data);
    data.write_data_addr(FRAME_DESCRIPTOR_PATH_PTR, path_global, 0);
    let function_global = object.declare_data_in_data(function_id, &mut data);
    data.write_data_addr(FRAME_DESCRIPTOR_FUNCTION_PTR, function_global, 0);
    try_or_string_error!(
        object.define_data(id, &data),
        "failed to define frame descriptor: {}"
    );
    Ok(id)
}

fn signature_for(
    function: &MirFunction,
    classes: &HashMap<String, MirClass>,
    call_conv: CallConv,
) -> std::result::Result<Signature, String> {
    let mut signature = Signature::new(call_conv);
    let mut writeback_types = Vec::new();
    if function.receiver.is_some() {
        let receiver_ty = receiver_type(function, classes)?;
        for abi in receiver_ty.abi_types() {
            signature.params.push(AbiParam::new(abi));
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
            signature.params.push(AbiParam::new(abi));
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
        signature.returns.push(AbiParam::new(abi));
    }
    for ty in writeback_types {
        for abi in ty.abi_types() {
            signature.returns.push(AbiParam::new(abi));
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
    builder: &mut FunctionBuilder<'_>,
    variable_index: &mut usize,
    variables: &mut HashMap<String, Vec<Variable>>,
    variable_types: &mut HashMap<String, DirectType>,
    name: String,
    ty: DirectType,
    initial: Option<&[Value]>,
) {
    let initial_values = if let Some(values) = initial {
        values.to_vec()
    } else {
        ty.zero_values(builder)
    };
    let abi_types = ty.abi_types();
    let mut declared = Vec::new();
    for (offset, abi_ty) in abi_types.into_iter().enumerate() {
        let variable = Variable::from_u32(*variable_index as u32);
        *variable_index += 1;
        builder.declare_var(variable, abi_ty);
        builder.def_var(variable, initial_values[offset]);
        declared.push(variable);
    }
    variables.insert(name.clone(), declared);
    variable_types.insert(name, ty);
}

/// The method a cleanup action calls on a class place: `__exit__` for a
/// manager, `close` for a resource.
fn cleanup_exit_method(action: Option<&CleanupAction>) -> &'static str {
    match action {
        Some(CleanupAction::ManagerExit { .. }) => "__exit__",
        _ => "close",
    }
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
                } if pushed == place => Some(action.clone()),
                _ => None,
            })
    })
}

/// Releases a cleanup thunk's call results. A `Result`-returning manager
/// exit first hands its result to the runtime, which records a `Result.Err`
/// for the trap being unwound (checkpoint F5).
fn release_cleanup_thunk_results(
    codegen: &mut NativeCodegen<'_>,
    builder: &mut FunctionBuilder<'_>,
    action: Option<&CleanupAction>,
    class_name: &str,
    function_name: &str,
    results: &[Value],
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
        let record = codegen.object.declare_func_in_func(
            codegen.runtime[RuntimeFn::RecordManagerExitFailure],
            builder.func,
        );
        builder.ins().call(record, &[boxed, class_ptr, class_len]);
        let release = codegen
            .object
            .declare_func_in_func(codegen.runtime[RuntimeFn::ReleaseValue], builder.func);
        builder.ins().call(release, &[boxed]);
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
    crate::mir::validate_loan_flow(module)?;
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
                Instruction::Safepoint | Instruction::IndexInRange { .. } => {}
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
        Rvalue::Call { callee, args } => {
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
        Rvalue::Try { value } => validate_operand(value),
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
        | Operand::Duration(_) => Ok(()),
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
                    .and_then(|key| canonical_named_type_from_key(key))
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

/// The module-qualified nominal type a canonical key names, for a key of a
/// non-generic named type; every other key shape resolves through the
/// member's own spelling.
fn canonical_named_type_from_key(key: &str) -> Option<Type> {
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
    builder: &mut FunctionBuilder<'_>,
    bytes: &[u8],
) -> std::result::Result<(Value, Value), String> {
    let id = if let Some(id) = codegen.string_data.get(bytes) {
        *id
    } else {
        let name = format!("aura_data_{}", codegen.string_data.len());
        let id = match codegen
            .object
            .declare_data(&name, Linkage::Local, false, false)
        {
            Ok(id) => id,
            Err(error) => return Err(format!("failed to declare string data: {}", error)),
        };
        let mut data = DataDescription::new();
        data.define(bytes.to_vec().into_boxed_slice());
        if let Err(error) = codegen.object.define_data(id, &data) {
            return Err(format!("failed to define string data: {}", error));
        }
        codegen.string_data.insert(bytes.to_vec(), id);
        id
    };
    let global = codegen.object.declare_data_in_func(id, builder.func);
    let ptr = builder.ins().symbol_value(types::I64, global);
    let len = builder.ins().iconst(types::I64, bytes.len() as i64);
    Ok((ptr, len))
}

/// A tag switch in a thunk's own builder: `body` runs once per member and
/// its results, typed `result_types`, merge into one block.
fn thunk_switch_on_tag(
    builder: &mut FunctionBuilder<'_>,
    tag: Value,
    member_count: usize,
    result_types: &[cranelift_codegen::ir::Type],
    mut body: impl FnMut(&mut FunctionBuilder<'_>, usize) -> std::result::Result<Vec<Value>, String>,
) -> std::result::Result<Vec<Value>, String> {
    let merge = builder.create_block();
    for abi in result_types {
        builder.append_block_param(merge, *abi);
    }
    for index in 0..member_count {
        let matched = builder.create_block();
        let next = (index + 1 < member_count).then(|| builder.create_block());
        if let Some(next) = next {
            let selected = builder.ins().icmp_imm(IntCC::Equal, tag, index as i64);
            builder.ins().brif(selected, matched, &[], next, &[]);
        } else {
            builder.ins().jump(matched, &[]);
        }
        builder.switch_to_block(matched);
        builder.seal_block(matched);
        let results = body(builder, index)?;
        builder.ins().jump(merge, &results);
        if let Some(next) = next {
            builder.switch_to_block(next);
            builder.seal_block(next);
        }
    }
    builder.switch_to_block(merge);
    builder.seal_block(merge);
    Ok(builder.block_params(merge).to_vec())
}

fn box_thunk_value(
    codegen: &mut NativeCodegen<'_>,
    builder: &mut FunctionBuilder<'_>,
    values: &[Value],
    ty: &DirectType,
) -> std::result::Result<Value, String> {
    match ty {
        DirectType::Callable(_) => callables::call_callable_box_adapter(codegen, builder, values),
        DirectType::Union(union) => {
            let union_inject = codegen
                .object
                .declare_func_in_func(codegen.runtime[RuntimeFn::UnionInject], builder.func);
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
                            builder.ins().bitcast(types::F64, MemFlags::new(), *word)
                        } else {
                            *word
                        });
                    }
                    let payload = box_thunk_value(codegen, builder, &member_values, member_ty)?;
                    let index_value = builder.ins().iconst(types::I64, index as i64);
                    let inst = builder
                        .ins()
                        .call(union_inject, &[ptr, len, index_value, payload]);
                    Ok(vec![builder.inst_results(inst)[0]])
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
            let box_i32 = codegen
                .object
                .declare_func_in_func(codegen.runtime[RuntimeFn::BoxI32], builder.func);
            let inst = builder.ins().call(box_i32, &[values[0]]);
            Ok(builder.inst_results(inst)[0])
        }
        DirectType::Scalar(ScalarKind::Int64) => {
            let box_i64 = codegen
                .object
                .declare_func_in_func(codegen.runtime[RuntimeFn::BoxI64], builder.func);
            let inst = builder.ins().call(box_i64, &[values[0]]);
            Ok(builder.inst_results(inst)[0])
        }
        DirectType::Scalar(ScalarKind::Uint64) => {
            let box_u64 = codegen
                .object
                .declare_func_in_func(codegen.runtime[RuntimeFn::BoxU64], builder.func);
            let inst = builder.ins().call(box_u64, &[values[0]]);
            Ok(builder.inst_results(inst)[0])
        }
        DirectType::Scalar(ScalarKind::Float32) | DirectType::Scalar(ScalarKind::Float64) => {
            let box_f64 = codegen
                .object
                .declare_func_in_func(codegen.runtime[RuntimeFn::BoxF64], builder.func);
            let inst = builder.ins().call(box_f64, &[values[0]]);
            Ok(builder.inst_results(inst)[0])
        }
        DirectType::Scalar(ScalarKind::Bool) => {
            let box_bool = codegen
                .object
                .declare_func_in_func(codegen.runtime[RuntimeFn::BoxBool], builder.func);
            let inst = builder.ins().call(box_bool, &[values[0]]);
            Ok(builder.inst_results(inst)[0])
        }
        DirectType::Scalar(ScalarKind::Unit) => {
            let box_unit = codegen
                .object
                .declare_func_in_func(codegen.runtime[RuntimeFn::BoxUnit], builder.func);
            let inst = builder.ins().call(box_unit, &[]);
            Ok(builder.inst_results(inst)[0])
        }
        DirectType::PlainClass(class) => {
            let instance_empty = codegen
                .object
                .declare_func_in_func(codegen.runtime[RuntimeFn::InstanceEmpty], builder.func);
            let instance_set_field_owned = codegen.object.declare_func_in_func(
                codegen.runtime[RuntimeFn::InstanceSetFieldOwned],
                builder.func,
            );
            let (class_ptr, class_len) =
                thunk_string_constant(codegen, builder, class.class_name.as_bytes())?;
            let init = builder.ins().call(instance_empty, &[class_ptr, class_len]);
            let current = builder.inst_results(init)[0];
            let mut start = 0usize;
            for field in &class.fields {
                let end = start + field.ty.value_count();
                let field_value =
                    box_thunk_value(codegen, builder, &values[start..end], &field.ty)?;
                let (field_ptr, field_len) =
                    thunk_string_constant(codegen, builder, field.name.as_bytes())?;
                builder.ins().call(
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
    builder: &mut FunctionBuilder<'_>,
    raw: Value,
    ty: &DirectType,
) -> std::result::Result<Vec<Value>, String> {
    match ty {
        DirectType::Callable(callable) => {
            callables::boxed_callable_words(codegen, builder, raw, &callable.signature)
        }
        DirectType::Union(union) => {
            let union_tag = codegen
                .object
                .declare_func_in_func(codegen.runtime[RuntimeFn::UnionTag], builder.func);
            let union_payload_copy = codegen
                .object
                .declare_func_in_func(codegen.runtime[RuntimeFn::UnionPayloadCopy], builder.func);
            let release_value = codegen
                .object
                .declare_func_in_func(codegen.runtime[RuntimeFn::ReleaseValue], builder.func);
            let tag_inst = builder.ins().call(union_tag, &[raw]);
            let tag = builder.inst_results(tag_inst)[0];
            let result_types = ty.abi_types();
            thunk_switch_on_tag(
                builder,
                tag,
                union.members.len(),
                &result_types,
                |builder, index| {
                    let member_ty = &union.members[index].ty;
                    let index_value = builder.ins().iconst(types::I64, index as i64);
                    let copy = builder.ins().call(union_payload_copy, &[raw, index_value]);
                    let payload = builder.inst_results(copy)[0];
                    let member_values = unbox_thunk_value(codegen, builder, payload, member_ty)?;
                    // A scalar or plain-class member was read out of the copy,
                    // so the copy is released; a runtime-object member's handle
                    // is the copy itself and travels with the union words.
                    if !matches!(member_ty, DirectType::Opaque(_) | DirectType::Callable(_)) {
                        let _ = builder.ins().call(release_value, &[payload]);
                    }
                    let mut words = vec![builder.ins().iconst(types::I64, index as i64)];
                    for (value, abi) in member_values.iter().zip(member_ty.abi_types()) {
                        words.push(if abi == types::F64 {
                            builder.ins().bitcast(types::I64, MemFlags::new(), *value)
                        } else {
                            *value
                        });
                    }
                    while words.len() < union.payload_words + 1 {
                        words.push(builder.ins().iconst(types::I64, 0));
                    }
                    Ok(words)
                },
            )
        }
        DirectType::Opaque(_) => Ok(vec![raw]),
        DirectType::Scalar(ScalarKind::Int32) => {
            let unbox_i64 = codegen
                .object
                .declare_func_in_func(codegen.runtime[RuntimeFn::UnboxI64], builder.func);
            let inst = builder.ins().call(unbox_i64, &[raw]);
            Ok(builder.inst_results(inst).to_vec())
        }
        DirectType::Scalar(ScalarKind::Int64) => {
            let unbox_int64 = codegen
                .object
                .declare_func_in_func(codegen.runtime[RuntimeFn::UnboxInt64], builder.func);
            let inst = builder.ins().call(unbox_int64, &[raw]);
            Ok(builder.inst_results(inst).to_vec())
        }
        DirectType::Scalar(ScalarKind::Uint64) => {
            let unbox_u64 = codegen
                .object
                .declare_func_in_func(codegen.runtime[RuntimeFn::UnboxU64], builder.func);
            let inst = builder.ins().call(unbox_u64, &[raw]);
            Ok(builder.inst_results(inst).to_vec())
        }
        DirectType::Scalar(ScalarKind::Float32) | DirectType::Scalar(ScalarKind::Float64) => {
            let unbox_f64 = codegen
                .object
                .declare_func_in_func(codegen.runtime[RuntimeFn::UnboxF64], builder.func);
            let inst = builder.ins().call(unbox_f64, &[raw]);
            Ok(builder.inst_results(inst).to_vec())
        }
        DirectType::Scalar(ScalarKind::Bool) => {
            let unbox_bool = codegen
                .object
                .declare_func_in_func(codegen.runtime[RuntimeFn::UnboxBool], builder.func);
            let inst = builder.ins().call(unbox_bool, &[raw]);
            Ok(builder.inst_results(inst).to_vec())
        }
        DirectType::Scalar(ScalarKind::Unit) => Ok(vec![builder.ins().iconst(types::I64, 0)]),
        DirectType::PlainClass(class) => {
            let instance_get_field = codegen
                .object
                .declare_func_in_func(codegen.runtime[RuntimeFn::InstanceGetField], builder.func);
            let mut values = Vec::new();
            for field in &class.fields {
                let (field_ptr, field_len) =
                    thunk_string_constant(codegen, builder, field.name.as_bytes())?;
                let inst = builder
                    .ins()
                    .call(instance_get_field, &[raw, field_ptr, field_len]);
                let field_raw = builder.inst_results(inst)[0];
                values.extend(unbox_thunk_value(codegen, builder, field_raw, &field.ty)?);
                if !matches!(field.ty, DirectType::Opaque(_) | DirectType::Callable(_)) {
                    let release_value = codegen.object.declare_func_in_func(
                        codegen.runtime[RuntimeFn::ReleaseValue],
                        builder.func,
                    );
                    builder.ins().call(release_value, &[field_raw]);
                }
            }
            Ok(values)
        }
    }
}

fn release_direct_call_results(
    codegen: &mut NativeCodegen<'_>,
    builder: &mut FunctionBuilder<'_>,
    function_name: &str,
    results: &[Value],
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
    builder: &mut FunctionBuilder<'_>,
    values: &[Value],
    ty: &DirectType,
) -> std::result::Result<(), String> {
    match ty {
        DirectType::Callable(_) => {
            callables::call_callable_unary_adapter(
                codegen,
                builder,
                values,
                callables::DESCRIPTOR_DROP,
            )?;
        }
        DirectType::Union(union) => {
            if union.owns_handles() {
                let release_value = codegen
                    .object
                    .declare_func_in_func(codegen.runtime[RuntimeFn::ReleaseValue], builder.func);
                let tag = values[0];
                let handle = values[1];
                for &index in &union.owning {
                    let active = builder.ins().icmp_imm(IntCC::Equal, tag, index as i64);
                    let release_block = builder.create_block();
                    let continue_block = builder.create_block();
                    builder
                        .ins()
                        .brif(active, release_block, &[], continue_block, &[]);
                    builder.switch_to_block(release_block);
                    builder.seal_block(release_block);
                    if matches!(union.member(index), Ok(DirectType::Callable(_))) {
                        callables::call_callable_unary_adapter(
                            codegen,
                            builder,
                            &values[1..2 + CALLABLE_ENVIRONMENT_WORDS],
                            callables::DESCRIPTOR_DROP,
                        )?;
                    } else {
                        builder.ins().call(release_value, &[handle]);
                    }
                    builder.ins().jump(continue_block, &[]);
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
            let release_value = codegen
                .object
                .declare_func_in_func(codegen.runtime[RuntimeFn::ReleaseValue], builder.func);
            builder.ins().call(release_value, &[value]);
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

fn main_signature(call_conv: CallConv) -> Signature {
    let mut signature = Signature::new(call_conv);
    signature.returns.push(AbiParam::new(types::I32));
    signature
}

fn thunk_signature(call_conv: CallConv) -> Signature {
    let mut signature = Signature::new(call_conv);
    signature.params.push(AbiParam::new(types::I64));
    signature.params.push(AbiParam::new(types::I64));
    signature.returns.push(AbiParam::new(types::I64));
    signature
}

fn default_binder_signature(call_conv: CallConv) -> Signature {
    let mut signature = Signature::new(call_conv);
    signature.params.push(AbiParam::new(types::I64));
    signature.params.push(AbiParam::new(types::I64));
    signature.params.push(AbiParam::new(types::I64));
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

fn collect_direct_runtime_type_substitutions(
    pattern: &Type,
    actual: &Type,
    substitutions: &mut HashMap<String, Type>,
) {
    match pattern {
        Type::Union(_) => {}
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
                collect_direct_runtime_type_substitutions(pattern_arg, actual_arg, substitutions);
            }
        }
        Type::Tuple(pattern_elements) => {
            let Type::Tuple(actual_elements) = actual else {
                return;
            };
            if pattern_elements.len() != actual_elements.len() {
                return;
            }
            for (pattern_element, actual_element) in
                pattern_elements.iter().zip(actual_elements.iter())
            {
                collect_direct_runtime_type_substitutions(
                    pattern_element,
                    actual_element,
                    substitutions,
                );
            }
        }
        Type::Function {
            params: pattern_params,
            return_type: pattern_return,
        } => {
            let Type::Function {
                params: actual_params,
                return_type: actual_return,
            } = actual
            else {
                return;
            };
            if pattern_params.len() != actual_params.len() {
                return;
            }
            for (pattern_param, actual_param) in pattern_params.iter().zip(actual_params.iter()) {
                if pattern_param.passing != actual_param.passing {
                    return;
                }
                collect_direct_runtime_type_substitutions(
                    &pattern_param.ty,
                    &actual_param.ty,
                    substitutions,
                );
            }
            collect_direct_runtime_type_substitutions(pattern_return, actual_return, substitutions);
        }
        Type::ReturnedView(view) => {
            if let Type::ReturnedView(actual_view) = actual {
                collect_direct_runtime_type_substitutions(
                    &view.pointee,
                    &actual_view.pointee,
                    substitutions,
                );
            }
        }
        Type::Callable(pattern_callable) => {
            let Type::Callable(actual_callable) = actual else {
                return;
            };
            if pattern_callable.params.len() != actual_callable.params.len() {
                return;
            }
            for (pattern_param, actual_param) in
                pattern_callable.params.iter().zip(&actual_callable.params)
            {
                collect_direct_runtime_type_substitutions(
                    &pattern_param.ty,
                    &actual_param.ty,
                    substitutions,
                );
            }
            collect_direct_runtime_type_substitutions(
                &pattern_callable.return_type,
                &actual_callable.return_type,
                substitutions,
            );
        }
        Type::Closure {
            params: pattern_params,
            return_type: pattern_return,
            captures: pattern_captures,
            ..
        } => {
            let Type::Closure {
                params: actual_params,
                return_type: actual_return,
                captures: actual_captures,
                ..
            } = actual
            else {
                return;
            };
            if pattern_params.len() != actual_params.len()
                || pattern_captures.len() != actual_captures.len()
            {
                return;
            }
            for (pattern_param, actual_param) in pattern_params.iter().zip(actual_params.iter()) {
                collect_direct_runtime_type_substitutions(
                    &pattern_param.ty,
                    &actual_param.ty,
                    substitutions,
                );
            }
            for (pattern_capture, actual_capture) in
                pattern_captures.iter().zip(actual_captures.iter())
            {
                collect_direct_runtime_type_substitutions(
                    &pattern_capture.ty,
                    &actual_capture.ty,
                    substitutions,
                );
            }
            collect_direct_runtime_type_substitutions(pattern_return, actual_return, substitutions);
        }
        Type::Unit | Type::Module(_) => {}
    }
}

/// The text of a `str` literal key for a dictionary whose keys are `str`.
fn str_literal_key<'a>(key: &'a Operand, key_type: &Type) -> Option<&'a str> {
    match key {
        Operand::String(text) if *key_type == Type::named("str") => Some(text),
        _ => None,
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
