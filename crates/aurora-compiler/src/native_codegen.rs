use std::collections::{BTreeSet, HashMap, HashSet};

use cranelift_codegen::ir::condcodes::{FloatCC, IntCC};
use cranelift_codegen::ir::immediates::Ieee64;
use cranelift_codegen::ir::TrapCode;
use cranelift_codegen::ir::{
    types, AbiParam, FuncRef, InstBuilder, MemFlags, Signature, UserFuncName, Value,
};
use cranelift_codegen::isa::CallConv;
use cranelift_codegen::settings::{self, Configurable};
use cranelift_frontend::{FunctionBuilder, FunctionBuilderContext, Variable};
use cranelift_module::{default_libcall_names, DataDescription, DataId, FuncId, Linkage, Module};
use cranelift_object::{ObjectBuilder, ObjectModule};

use crate::ast::{BinaryOp, UnaryOp};
use crate::diag::Span;
use crate::mir::{
    BasicBlock, CallTarget, Instruction, MirArg, MirClass, MirFormatPart, MirFunction, MirMethod,
    MirModule, MirReceiverKind, MirSelectArm, MirSelectKind, MirTraitImpl, Operand, Rvalue,
    Terminator,
};
use crate::sema::{substitute_type, Type};

pub fn emit_host_object(module: &MirModule) -> std::result::Result<Vec<u8>, String> {
    emit_host_object_with_metadata(module, "<aurora>", "")
}

pub fn emit_host_object_with_metadata(
    module: &MirModule,
    program_path: &str,
    program_source: &str,
) -> std::result::Result<Vec<u8>, String> {
    let context = NativeCodegen::new(module, program_path, program_source)?;
    context.emit()
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ScalarKind {
    Int32,
    Float32,
    Float64,
    Bool,
    Unit,
}

impl ScalarKind {
    fn signature_type(self) -> cranelift_codegen::ir::Type {
        match self {
            ScalarKind::Int32 | ScalarKind::Bool | ScalarKind::Unit => types::I64,
            ScalarKind::Float32 | ScalarKind::Float64 => types::F64,
        }
    }

    fn zero_value(self, builder: &mut FunctionBuilder<'_>) -> Value {
        match self {
            ScalarKind::Int32 | ScalarKind::Bool | ScalarKind::Unit => {
                builder.ins().iconst(types::I64, 0)
            }
            ScalarKind::Float32 | ScalarKind::Float64 => {
                builder.ins().f64const(Ieee64::with_float(0.0))
            }
        }
    }

    fn is_float(self) -> bool {
        matches!(self, ScalarKind::Float32 | ScalarKind::Float64)
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
enum DirectType {
    Scalar(ScalarKind),
    PlainClass(PlainClassType),
    Opaque(Type),
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
            DirectType::PlainClass(class) => {
                let mut types = Vec::new();
                for field in &class.fields {
                    types.extend(field.ty.abi_types());
                }
                types
            }
            DirectType::Opaque(_) => vec![types::I64],
        }
    }

    fn value_count(&self) -> usize {
        self.abi_types().len()
    }

    fn scalar_kind(&self) -> Option<ScalarKind> {
        match self {
            DirectType::Scalar(kind) => Some(*kind),
            DirectType::PlainClass(_) | DirectType::Opaque(_) => None,
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
            DirectType::Opaque(_) => vec![builder.ins().iconst(types::I64, 0)],
        }
    }

    fn field_slice(&self, field_name: &str) -> Option<(usize, usize, DirectType)> {
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

struct NativeCodegen<'a> {
    module: &'a MirModule,
    program_path: String,
    program_source: String,
    object: ObjectModule,
    functions: HashMap<String, FuncId>,
    function_thunks: HashMap<String, FuncId>,
    classes: HashMap<String, MirClass>,
    trait_impls: Vec<MirTraitImpl>,
    function_return_types: HashMap<String, DirectType>,
    function_param_types: HashMap<String, Vec<DirectType>>,
    function_writeback_types: HashMap<String, Vec<DirectType>>,
    call_conv: CallConv,
    runtime_init: FuncId,
    run_root: FuncId,
    print_i64: FuncId,
    print_f64: FuncId,
    print_bool: FuncId,
    print_value: FuncId,
    sqrt_f64: FuncId,
    fail_division_by_zero: FuncId,
    fail_int32_overflow: FuncId,
    box_i64: FuncId,
    box_uint_literal: FuncId,
    box_f64: FuncId,
    box_bool: FuncId,
    box_unit: FuncId,
    string_literal: FuncId,
    string_len: FuncId,
    string_contains: FuncId,
    string_starts_with: FuncId,
    string_ends_with: FuncId,
    string_split: FuncId,
    string_replace: FuncId,
    string_to_lower: FuncId,
    string_to_upper: FuncId,
    string_strip_prefix: FuncId,
    string_strip_suffix: FuncId,
    string_trim: FuncId,
    string_join: FuncId,
    stringify_value: FuncId,
    abs_value: FuncId,
    min_value: FuncId,
    max_value: FuncId,
    sqrt_value: FuncId,
    parse_int32: FuncId,
    parse_int64: FuncId,
    parse_float64: FuncId,
    duration_literal: FuncId,
    range_new: FuncId,
    range_current: FuncId,
    range_end: FuncId,
    range_advance: FuncId,
    vec_empty: FuncId,
    vec_len: FuncId,
    vec_is_empty: FuncId,
    vec_push_in_place: FuncId,
    vec_pop_in_place: FuncId,
    vec_get: FuncId,
    vec_set_in_place: FuncId,
    vec_remove_in_place: FuncId,
    vec_swap_in_place: FuncId,
    vec_contains: FuncId,
    vec_extend_in_place: FuncId,
    vec_insert_in_place: FuncId,
    vec_clear_in_place: FuncId,
    vec_reverse_in_place: FuncId,
    vec_index: FuncId,
    vec_index_option: FuncId,
    vec_set_index_in_place: FuncId,
    map_empty: FuncId,
    map_len: FuncId,
    map_is_empty: FuncId,
    map_get: FuncId,
    map_set_in_place: FuncId,
    map_remove_in_place: FuncId,
    map_contains_key: FuncId,
    map_keys: FuncId,
    map_values: FuncId,
    map_items: FuncId,
    map_entries: FuncId,
    map_clear_in_place: FuncId,
    map_extend_in_place: FuncId,
    map_index: FuncId,
    map_set_index_in_place: FuncId,
    set_empty: FuncId,
    set_len: FuncId,
    set_is_empty: FuncId,
    set_contains: FuncId,
    set_insert_in_place: FuncId,
    set_remove_in_place: FuncId,
    set_index_option: FuncId,
    retain_value: FuncId,
    release_value: FuncId,
    clone_value: FuncId,
    unbox_i64: FuncId,
    unbox_f64: FuncId,
    unbox_bool: FuncId,
    value_as_condition: FuncId,
    unary_value: FuncId,
    binary_value: FuncId,
    cast_value: FuncId,
    value_type_matches: FuncId,
    enum_variant: FuncId,
    variant_matches: FuncId,
    variant_payload: FuncId,
    instance_empty: FuncId,
    instance_get_field: FuncId,
    instance_set_field: FuncId,
    arg_buffer_new: FuncId,
    arg_buffer_store: FuncId,
    i64_buffer_new: FuncId,
    i64_buffer_store: FuncId,
    channel_new: FuncId,
    channel_can_send: FuncId,
    channel_send: FuncId,
    channel_recv: FuncId,
    channel_recv_timeout_value: FuncId,
    channel_try_recv: FuncId,
    channel_close: FuncId,
    task_group_new: FuncId,
    task_group_cancel: FuncId,
    task_group_close: FuncId,
    task_join: FuncId,
    io_write: FuncId,
    io_flush: FuncId,
    io_read_line: FuncId,
    fs_exists: FuncId,
    fs_read_to_string: FuncId,
    fs_read_bytes: FuncId,
    fs_write_string: FuncId,
    fs_write_bytes: FuncId,
    fs_append_string: FuncId,
    fs_append_bytes: FuncId,
    fs_create_dir: FuncId,
    fs_read_dir: FuncId,
    fs_remove_file: FuncId,
    fs_open: FuncId,
    fs_create: FuncId,
    fs_append: FuncId,
    file_read_all: FuncId,
    file_read_bytes: FuncId,
    file_write_all: FuncId,
    file_write_bytes: FuncId,
    file_flush: FuncId,
    file_close: FuncId,
    net_connect: FuncId,
    net_connect_timeout: FuncId,
    net_listen: FuncId,
    net_udp_bind: FuncId,
    net_unix_listen: FuncId,
    net_unix_connect: FuncId,
    net_unix_connect_timeout: FuncId,
    net_tls_listen: FuncId,
    net_tls_connect: FuncId,
    net_tls_connect_timeout: FuncId,
    net_http_listen: FuncId,
    net_http_request_text: FuncId,
    net_http_request_text_timeout: FuncId,
    net_http_request_bytes: FuncId,
    net_http_request_bytes_timeout: FuncId,
    net_websocket_listen: FuncId,
    net_websocket_connect: FuncId,
    net_websocket_connect_timeout: FuncId,
    tcp_listener_accept: FuncId,
    tcp_listener_local_addr: FuncId,
    tcp_listener_close: FuncId,
    tcp_stream_read_all: FuncId,
    tcp_stream_read_line: FuncId,
    tcp_stream_read_bytes: FuncId,
    tcp_stream_read_exact: FuncId,
    tcp_stream_write_all: FuncId,
    tcp_stream_write_bytes: FuncId,
    tcp_stream_flush: FuncId,
    tcp_stream_local_addr: FuncId,
    tcp_stream_peer_addr: FuncId,
    tcp_stream_shutdown_read: FuncId,
    tcp_stream_shutdown_write: FuncId,
    tcp_stream_shutdown_both: FuncId,
    tcp_stream_close: FuncId,
    udp_socket_send_text: FuncId,
    udp_socket_send_bytes: FuncId,
    udp_socket_recv: FuncId,
    udp_socket_recv_from: FuncId,
    udp_socket_local_addr: FuncId,
    udp_socket_peer_addr: FuncId,
    udp_socket_close: FuncId,
    udp_datagram_address: FuncId,
    udp_datagram_bytes: FuncId,
    udp_datagram_text: FuncId,
    http_listener_accept: FuncId,
    http_listener_local_addr: FuncId,
    http_listener_close: FuncId,
    http_exchange_method: FuncId,
    http_exchange_path: FuncId,
    http_exchange_headers: FuncId,
    http_exchange_body_text: FuncId,
    http_exchange_body_bytes: FuncId,
    http_exchange_respond_text: FuncId,
    http_exchange_respond_bytes: FuncId,
    http_response_status: FuncId,
    http_response_reason: FuncId,
    http_response_headers: FuncId,
    http_response_text: FuncId,
    http_response_bytes: FuncId,
    websocket_listener_accept: FuncId,
    websocket_listener_local_addr: FuncId,
    websocket_send_text: FuncId,
    websocket_send_bytes: FuncId,
    websocket_recv_text: FuncId,
    websocket_recv_bytes: FuncId,
    websocket_close: FuncId,
    unix_listener_accept: FuncId,
    unix_listener_close: FuncId,
    unix_stream_read_line: FuncId,
    unix_stream_read_exact: FuncId,
    unix_stream_write_all: FuncId,
    unix_stream_close: FuncId,
    tls_listener_accept: FuncId,
    tls_listener_local_addr: FuncId,
    tls_listener_close: FuncId,
    tls_stream_read_line: FuncId,
    tls_stream_read_exact: FuncId,
    tls_stream_write_all: FuncId,
    tls_stream_close: FuncId,
    cancelled: FuncId,
    deadline_new: FuncId,
    deadline_ready: FuncId,
    deadline_drop: FuncId,
    select_wait: FuncId,
    sleep_value: FuncId,
    spawn_call: FuncId,
    string_data: HashMap<Vec<u8>, DataId>,
}

macro_rules! declare_runtime_functions {
    ($object:expr, $( $var:ident => ($name:literal, [$($param:expr),* $(,)?], $ret:expr) ),+ $(,)?) => {
        $(
            let $var = declare_runtime_function($object, $name, &[$($param),*], $ret)?;
        )+
    };
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

fn ordered_named_args<'a>(
    expected_names: &[&str],
    args: &'a [MirArg],
) -> std::result::Result<Vec<&'a MirArg>, String> {
    let mut values = vec![None; expected_names.len()];
    let mut next_positional = 0usize;
    for argument in args {
        if let Some(name) = argument.name.as_deref() {
            let Some(index) = expected_names
                .iter()
                .position(|candidate| *candidate == name)
            else {
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
    values
        .into_iter()
        .map(|value| {
            value.ok_or_else(|| "direct backend is missing a builtin argument".to_string())
        })
        .collect()
}

fn ordered_optional_named_args<'a>(
    expected_names: &[&str],
    args: &'a [MirArg],
) -> std::result::Result<Vec<Option<&'a MirArg>>, String> {
    let mut values = vec![None; expected_names.len()];
    let mut next_positional = 0usize;
    for argument in args {
        if let Some(name) = argument.name.as_deref() {
            let Some(index) = expected_names
                .iter()
                .position(|candidate| *candidate == name)
            else {
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

impl<'a> NativeCodegen<'a> {
    fn new(
        module: &'a MirModule,
        program_path: &str,
        program_source: &str,
    ) -> std::result::Result<Self, String> {
        validate_module(module)?;
        let mut classes = HashMap::new();
        for class in &module.classes {
            classes.insert(class.name.clone(), class.clone());
        }
        let trait_impls = module.trait_impls.clone();

        let mut flag_builder = settings::builder();
        try_or_string_error!(
            flag_builder.set("is_pic", "true"),
            "failed to configure native backend: {}"
        );
        let flags = settings::Flags::new(flag_builder);
        let isa_builder =
            try_or_string_error!(cranelift_native::builder(), "failed to detect host ISA: {}");
        let isa = try_or_string_error!(isa_builder.finish(flags), "failed to build host ISA: {}");
        let call_conv = isa.default_call_conv();
        let builder = try_or_string_error!(
            ObjectBuilder::new(isa, "aurora_direct".to_string(), default_libcall_names()),
            "failed to initialize object builder: {}"
        );
        let mut object = ObjectModule::new(builder);

        declare_runtime_functions!(
            &mut object,
            runtime_init => ("aurora_direct_runtime_init", [types::I64, types::I64, types::I64, types::I64], None),
            run_root => ("aurora_direct_run_root", [types::I64], Some(types::I32)),
            print_i64 => ("aurora_direct_print_i64", [types::I64], None),
            print_f64 => ("aurora_direct_print_f64", [types::F64], None),
            print_bool => ("aurora_direct_print_bool", [types::I64], None),
            print_value => ("aurora_direct_print_value", [types::I64], None),
            sqrt_f64 => ("aurora_direct_sqrt_f64", [types::F64], Some(types::F64)),
            fail_division_by_zero => ("aurora_direct_fail_division_by_zero", [types::I64, types::I64], None),
            fail_int32_overflow => ("aurora_direct_fail_int32_overflow", [types::I64, types::I64, types::I64], None),
            box_i64 => ("aurora_direct_box_i64", [types::I64], Some(types::I64)),
            box_uint_literal => ("aurora_direct_box_uint_literal", [types::I64, types::I64], Some(types::I64)),
            box_f64 => ("aurora_direct_box_f64", [types::F64], Some(types::I64)),
            box_bool => ("aurora_direct_box_bool", [types::I64], Some(types::I64)),
            box_unit => ("aurora_direct_box_unit", [], Some(types::I64)),
            string_literal => ("aurora_direct_string_literal", [types::I64, types::I64], Some(types::I64)),
            string_len => ("aurora_direct_string_len", [types::I64], Some(types::I64)),
            string_contains => ("aurora_direct_string_contains", [types::I64, types::I64], Some(types::I64)),
            string_starts_with => ("aurora_direct_string_starts_with", [types::I64, types::I64], Some(types::I64)),
            string_ends_with => ("aurora_direct_string_ends_with", [types::I64, types::I64], Some(types::I64)),
            string_split => ("aurora_direct_string_split", [types::I64, types::I64], Some(types::I64)),
            string_replace => ("aurora_direct_string_replace", [types::I64, types::I64, types::I64], Some(types::I64)),
            string_to_lower => ("aurora_direct_string_to_lower", [types::I64], Some(types::I64)),
            string_to_upper => ("aurora_direct_string_to_upper", [types::I64], Some(types::I64)),
            string_strip_prefix => ("aurora_direct_string_strip_prefix", [types::I64, types::I64], Some(types::I64)),
            string_strip_suffix => ("aurora_direct_string_strip_suffix", [types::I64, types::I64], Some(types::I64)),
            string_trim => ("aurora_direct_string_trim", [types::I64], Some(types::I64)),
            string_join => ("aurora_direct_string_join", [types::I64, types::I64], Some(types::I64)),
            stringify_value => ("aurora_direct_stringify_value", [types::I64], Some(types::I64)),
            abs_value => ("aurora_direct_abs", [types::I64], Some(types::I64)),
            min_value => ("aurora_direct_min", [types::I64, types::I64], Some(types::I64)),
            max_value => ("aurora_direct_max", [types::I64, types::I64], Some(types::I64)),
            sqrt_value => ("aurora_direct_sqrt", [types::I64], Some(types::I64)),
            parse_int32 => ("aurora_direct_parse_int32", [types::I64], Some(types::I64)),
            parse_int64 => ("aurora_direct_parse_int64", [types::I64], Some(types::I64)),
            parse_float64 => ("aurora_direct_parse_float64", [types::I64], Some(types::I64)),
            duration_literal => ("aurora_direct_duration_literal", [types::I64], Some(types::I64)),
            range_new => ("aurora_direct_range_new", [types::I64, types::I64], Some(types::I64)),
            range_current => ("aurora_direct_range_current", [types::I64], Some(types::I64)),
            range_end => ("aurora_direct_range_end", [types::I64], Some(types::I64)),
            range_advance => ("aurora_direct_range_advance", [types::I64], Some(types::I64)),
            vec_empty => ("aurora_direct_vec_empty", [], Some(types::I64)),
            vec_len => ("aurora_direct_vec_len", [types::I64], Some(types::I64)),
            vec_is_empty => ("aurora_direct_vec_is_empty", [types::I64], Some(types::I64)),
            vec_push_in_place => ("aurora_direct_vec_push_in_place", [types::I64, types::I64], Some(types::I64)),
            vec_pop_in_place => ("aurora_direct_vec_pop_in_place", [types::I64], Some(types::I64)),
            vec_get => ("aurora_direct_vec_get", [types::I64, types::I64], Some(types::I64)),
            vec_set_in_place => ("aurora_direct_vec_set_in_place", [types::I64, types::I64, types::I64], Some(types::I64)),
            vec_remove_in_place => ("aurora_direct_vec_remove_in_place", [types::I64, types::I64], Some(types::I64)),
            vec_swap_in_place => ("aurora_direct_vec_swap_in_place", [types::I64, types::I64, types::I64], Some(types::I64)),
            vec_contains => ("aurora_direct_vec_contains", [types::I64, types::I64], Some(types::I64)),
            vec_extend_in_place => ("aurora_direct_vec_extend_in_place", [types::I64, types::I64], Some(types::I64)),
            vec_insert_in_place => ("aurora_direct_vec_insert_in_place", [types::I64, types::I64, types::I64], Some(types::I64)),
            vec_clear_in_place => ("aurora_direct_vec_clear_in_place", [types::I64], Some(types::I64)),
            vec_reverse_in_place => ("aurora_direct_vec_reverse_in_place", [types::I64], Some(types::I64)),
            vec_index => ("aurora_direct_vec_index", [types::I64, types::I64, types::I64, types::I64], Some(types::I64)),
            vec_index_option => ("aurora_direct_vec_index_option", [types::I64, types::I64], Some(types::I64)),
            vec_set_index_in_place => ("aurora_direct_vec_set_index_in_place", [types::I64, types::I64, types::I64, types::I64, types::I64], Some(types::I64)),
            map_empty => ("aurora_direct_map_empty", [], Some(types::I64)),
            map_len => ("aurora_direct_map_len", [types::I64], Some(types::I64)),
            map_is_empty => ("aurora_direct_map_is_empty", [types::I64], Some(types::I64)),
            map_get => ("aurora_direct_map_get", [types::I64, types::I64], Some(types::I64)),
            map_set_in_place => ("aurora_direct_map_set_in_place", [types::I64, types::I64, types::I64], Some(types::I64)),
            map_remove_in_place => ("aurora_direct_map_remove_in_place", [types::I64, types::I64], Some(types::I64)),
            map_contains_key => ("aurora_direct_map_contains_key", [types::I64, types::I64], Some(types::I64)),
            map_keys => ("aurora_direct_map_keys", [types::I64], Some(types::I64)),
            map_values => ("aurora_direct_map_values", [types::I64], Some(types::I64)),
            map_items => ("aurora_direct_map_items", [types::I64], Some(types::I64)),
            map_entries => ("aurora_direct_map_entries", [types::I64], Some(types::I64)),
            map_clear_in_place => ("aurora_direct_map_clear_in_place", [types::I64], Some(types::I64)),
            map_extend_in_place => ("aurora_direct_map_extend_in_place", [types::I64, types::I64], Some(types::I64)),
            map_index => ("aurora_direct_map_index", [types::I64, types::I64, types::I64, types::I64], Some(types::I64)),
            map_set_index_in_place => ("aurora_direct_map_set_index_in_place", [types::I64, types::I64, types::I64, types::I64, types::I64], Some(types::I64)),
            set_empty => ("aurora_direct_set_empty", [], Some(types::I64)),
            set_len => ("aurora_direct_set_len", [types::I64], Some(types::I64)),
            set_is_empty => ("aurora_direct_set_is_empty", [types::I64], Some(types::I64)),
            set_contains => ("aurora_direct_set_contains", [types::I64, types::I64], Some(types::I64)),
            set_insert_in_place => ("aurora_direct_set_insert_in_place", [types::I64, types::I64], Some(types::I64)),
            set_remove_in_place => ("aurora_direct_set_remove_in_place", [types::I64, types::I64], Some(types::I64)),
            set_index_option => ("aurora_direct_set_index_option", [types::I64, types::I64], Some(types::I64)),
            retain_value => ("aurora_direct_retain_value", [types::I64], Some(types::I64)),
            release_value => ("aurora_direct_release_value", [types::I64], None),
            clone_value => ("aurora_direct_clone_value", [types::I64], Some(types::I64)),
            unbox_i64 => ("aurora_direct_unbox_i64", [types::I64], Some(types::I64)),
            unbox_f64 => ("aurora_direct_unbox_f64", [types::I64], Some(types::F64)),
            unbox_bool => ("aurora_direct_unbox_bool", [types::I64], Some(types::I64)),
            value_as_condition => ("aurora_direct_value_as_condition", [types::I64], Some(types::I64)),
            unary_value => ("aurora_direct_unary_value_at", [types::I64, types::I64, types::I64, types::I64], Some(types::I64)),
            binary_value => ("aurora_direct_binary_value_at", [types::I64, types::I64, types::I64, types::I64, types::I64], Some(types::I64)),
            cast_value => ("aurora_direct_cast_value_at", [types::I64, types::I64, types::I64, types::I64, types::I64], Some(types::I64)),
            value_type_matches => ("aurora_direct_value_type_matches", [types::I64, types::I64, types::I64], Some(types::I64)),
            enum_variant => ("aurora_direct_enum_variant", [types::I64, types::I64, types::I64, types::I64, types::I64], Some(types::I64)),
            variant_matches => ("aurora_direct_variant_matches", [types::I64, types::I64, types::I64, types::I64, types::I64], Some(types::I64)),
            variant_payload => ("aurora_direct_variant_payload", [types::I64, types::I64], Some(types::I64)),
            instance_empty => ("aurora_direct_instance_empty", [types::I64, types::I64], Some(types::I64)),
            instance_get_field => ("aurora_direct_instance_get_field", [types::I64, types::I64, types::I64], Some(types::I64)),
            instance_set_field => ("aurora_direct_instance_set_field", [types::I64, types::I64, types::I64, types::I64], Some(types::I64)),
            arg_buffer_new => ("aurora_direct_arg_buffer_new", [types::I64], Some(types::I64)),
            arg_buffer_store => ("aurora_direct_arg_buffer_store", [types::I64, types::I64, types::I64], None),
            i64_buffer_new => ("aurora_direct_i64_buffer_new", [types::I64], Some(types::I64)),
            i64_buffer_store => ("aurora_direct_i64_buffer_store", [types::I64, types::I64, types::I64], None),
            channel_new => ("aurora_direct_channel_new", [types::I64], Some(types::I64)),
            channel_can_send => ("aurora_direct_channel_can_send", [types::I64], Some(types::I64)),
            channel_send => ("aurora_direct_channel_send", [types::I64, types::I64], Some(types::I64)),
            channel_recv => ("aurora_direct_channel_recv", [types::I64], Some(types::I64)),
            channel_recv_timeout_value => ("aurora_direct_channel_recv_timeout_value", [types::I64, types::I64], Some(types::I64)),
            channel_try_recv => ("aurora_direct_channel_try_recv", [types::I64], Some(types::I64)),
            channel_close => ("aurora_direct_channel_close", [types::I64], Some(types::I64)),
            task_group_new => ("aurora_direct_task_group_new", [], Some(types::I64)),
            task_group_cancel => ("aurora_direct_task_group_cancel", [types::I64], Some(types::I64)),
            task_group_close => ("aurora_direct_task_group_close", [types::I64, types::I64], Some(types::I64)),
            task_join => ("aurora_direct_task_join", [types::I64], Some(types::I64)),
            io_write => ("aurora_direct_io_write", [types::I64], Some(types::I64)),
            io_flush => ("aurora_direct_io_flush", [], Some(types::I64)),
            io_read_line => ("aurora_direct_io_read_line", [], Some(types::I64)),
            fs_exists => ("aurora_direct_fs_exists", [types::I64], Some(types::I64)),
            fs_read_to_string => ("aurora_direct_fs_read_to_string", [types::I64], Some(types::I64)),
            fs_read_bytes => ("aurora_direct_fs_read_bytes", [types::I64], Some(types::I64)),
            fs_write_string => ("aurora_direct_fs_write_string", [types::I64, types::I64], Some(types::I64)),
            fs_write_bytes => ("aurora_direct_fs_write_bytes", [types::I64, types::I64], Some(types::I64)),
            fs_append_string => ("aurora_direct_fs_append_string", [types::I64, types::I64], Some(types::I64)),
            fs_append_bytes => ("aurora_direct_fs_append_bytes", [types::I64, types::I64], Some(types::I64)),
            fs_create_dir => ("aurora_direct_fs_create_dir", [types::I64], Some(types::I64)),
            fs_read_dir => ("aurora_direct_fs_read_dir", [types::I64], Some(types::I64)),
            fs_remove_file => ("aurora_direct_fs_remove_file", [types::I64], Some(types::I64)),
            fs_open => ("aurora_direct_fs_open", [types::I64], Some(types::I64)),
            fs_create => ("aurora_direct_fs_create", [types::I64], Some(types::I64)),
            fs_append => ("aurora_direct_fs_append", [types::I64], Some(types::I64)),
            file_read_all => ("aurora_direct_file_read_all", [types::I64], Some(types::I64)),
            file_read_bytes => ("aurora_direct_file_read_bytes", [types::I64], Some(types::I64)),
            file_write_all => ("aurora_direct_file_write_all", [types::I64, types::I64], Some(types::I64)),
            file_write_bytes => ("aurora_direct_file_write_bytes", [types::I64, types::I64], Some(types::I64)),
            file_flush => ("aurora_direct_file_flush", [types::I64], Some(types::I64)),
            file_close => ("aurora_direct_file_close", [types::I64], Some(types::I64)),
            net_connect => ("aurora_direct_net_connect", [types::I64], Some(types::I64)),
            net_connect_timeout => ("aurora_direct_net_connect_timeout", [types::I64, types::I64], Some(types::I64)),
            net_listen => ("aurora_direct_net_listen", [types::I64], Some(types::I64)),
            net_udp_bind => ("aurora_direct_net_udp_bind", [types::I64], Some(types::I64)),
            net_unix_listen => ("aurora_direct_net_unix_listen", [types::I64], Some(types::I64)),
            net_unix_connect => ("aurora_direct_net_unix_connect", [types::I64], Some(types::I64)),
            net_unix_connect_timeout => ("aurora_direct_net_unix_connect_timeout", [types::I64, types::I64], Some(types::I64)),
            net_tls_listen => ("aurora_direct_net_tls_listen", [types::I64, types::I64, types::I64], Some(types::I64)),
            net_tls_connect => ("aurora_direct_net_tls_connect", [types::I64, types::I64, types::I64], Some(types::I64)),
            net_tls_connect_timeout => ("aurora_direct_net_tls_connect_timeout", [types::I64, types::I64, types::I64, types::I64], Some(types::I64)),
            net_http_listen => ("aurora_direct_net_http_listen", [types::I64], Some(types::I64)),
            net_http_request_text => ("aurora_direct_net_http_request_text", [types::I64, types::I64, types::I64, types::I64], Some(types::I64)),
            net_http_request_text_timeout => ("aurora_direct_net_http_request_text_timeout", [types::I64, types::I64, types::I64, types::I64, types::I64], Some(types::I64)),
            net_http_request_bytes => ("aurora_direct_net_http_request_bytes", [types::I64, types::I64, types::I64, types::I64], Some(types::I64)),
            net_http_request_bytes_timeout => ("aurora_direct_net_http_request_bytes_timeout", [types::I64, types::I64, types::I64, types::I64, types::I64], Some(types::I64)),
            net_websocket_listen => ("aurora_direct_net_websocket_listen", [types::I64], Some(types::I64)),
            net_websocket_connect => ("aurora_direct_net_websocket_connect", [types::I64], Some(types::I64)),
            net_websocket_connect_timeout => ("aurora_direct_net_websocket_connect_timeout", [types::I64, types::I64], Some(types::I64)),
            tcp_listener_accept => ("aurora_direct_tcp_listener_accept", [types::I64, types::I64], Some(types::I64)),
            tcp_listener_local_addr => ("aurora_direct_tcp_listener_local_addr", [types::I64], Some(types::I64)),
            tcp_listener_close => ("aurora_direct_tcp_listener_close", [types::I64], Some(types::I64)),
            tcp_stream_read_all => ("aurora_direct_tcp_stream_read_all", [types::I64, types::I64], Some(types::I64)),
            tcp_stream_read_line => ("aurora_direct_tcp_stream_read_line", [types::I64, types::I64], Some(types::I64)),
            tcp_stream_read_bytes => ("aurora_direct_tcp_stream_read_bytes", [types::I64, types::I64, types::I64], Some(types::I64)),
            tcp_stream_read_exact => ("aurora_direct_tcp_stream_read_exact", [types::I64, types::I64, types::I64], Some(types::I64)),
            tcp_stream_write_all => ("aurora_direct_tcp_stream_write_all", [types::I64, types::I64, types::I64], Some(types::I64)),
            tcp_stream_write_bytes => ("aurora_direct_tcp_stream_write_bytes", [types::I64, types::I64, types::I64], Some(types::I64)),
            tcp_stream_flush => ("aurora_direct_tcp_stream_flush", [types::I64], Some(types::I64)),
            tcp_stream_local_addr => ("aurora_direct_tcp_stream_local_addr", [types::I64], Some(types::I64)),
            tcp_stream_peer_addr => ("aurora_direct_tcp_stream_peer_addr", [types::I64], Some(types::I64)),
            tcp_stream_shutdown_read => ("aurora_direct_tcp_stream_shutdown_read", [types::I64], Some(types::I64)),
            tcp_stream_shutdown_write => ("aurora_direct_tcp_stream_shutdown_write", [types::I64], Some(types::I64)),
            tcp_stream_shutdown_both => ("aurora_direct_tcp_stream_shutdown_both", [types::I64], Some(types::I64)),
            tcp_stream_close => ("aurora_direct_tcp_stream_close", [types::I64], Some(types::I64)),
            udp_socket_send_text => ("aurora_direct_udp_socket_send_text", [types::I64, types::I64, types::I64, types::I64], Some(types::I64)),
            udp_socket_send_bytes => ("aurora_direct_udp_socket_send_bytes", [types::I64, types::I64, types::I64, types::I64], Some(types::I64)),
            udp_socket_recv => ("aurora_direct_udp_socket_recv", [types::I64, types::I64, types::I64], Some(types::I64)),
            udp_socket_recv_from => ("aurora_direct_udp_socket_recv_from", [types::I64, types::I64, types::I64], Some(types::I64)),
            udp_socket_local_addr => ("aurora_direct_udp_socket_local_addr", [types::I64], Some(types::I64)),
            udp_socket_peer_addr => ("aurora_direct_udp_socket_peer_addr", [types::I64], Some(types::I64)),
            udp_socket_close => ("aurora_direct_udp_socket_close", [types::I64], Some(types::I64)),
            udp_datagram_address => ("aurora_direct_udp_datagram_address", [types::I64], Some(types::I64)),
            udp_datagram_bytes => ("aurora_direct_udp_datagram_bytes", [types::I64], Some(types::I64)),
            udp_datagram_text => ("aurora_direct_udp_datagram_text", [types::I64], Some(types::I64)),
            http_listener_accept => ("aurora_direct_http_listener_accept", [types::I64, types::I64], Some(types::I64)),
            http_listener_local_addr => ("aurora_direct_http_listener_local_addr", [types::I64], Some(types::I64)),
            http_listener_close => ("aurora_direct_http_listener_close", [types::I64], Some(types::I64)),
            http_exchange_method => ("aurora_direct_http_exchange_method", [types::I64], Some(types::I64)),
            http_exchange_path => ("aurora_direct_http_exchange_path", [types::I64], Some(types::I64)),
            http_exchange_headers => ("aurora_direct_http_exchange_headers", [types::I64], Some(types::I64)),
            http_exchange_body_text => ("aurora_direct_http_exchange_body_text", [types::I64], Some(types::I64)),
            http_exchange_body_bytes => ("aurora_direct_http_exchange_body_bytes", [types::I64], Some(types::I64)),
            http_exchange_respond_text => ("aurora_direct_http_exchange_respond_text", [types::I64, types::I64, types::I64, types::I64], Some(types::I64)),
            http_exchange_respond_bytes => ("aurora_direct_http_exchange_respond_bytes", [types::I64, types::I64, types::I64, types::I64], Some(types::I64)),
            http_response_status => ("aurora_direct_http_response_status", [types::I64], Some(types::I64)),
            http_response_reason => ("aurora_direct_http_response_reason", [types::I64], Some(types::I64)),
            http_response_headers => ("aurora_direct_http_response_headers", [types::I64], Some(types::I64)),
            http_response_text => ("aurora_direct_http_response_text", [types::I64], Some(types::I64)),
            http_response_bytes => ("aurora_direct_http_response_bytes", [types::I64], Some(types::I64)),
            websocket_listener_accept => ("aurora_direct_websocket_listener_accept", [types::I64, types::I64], Some(types::I64)),
            websocket_listener_local_addr => ("aurora_direct_websocket_listener_local_addr", [types::I64], Some(types::I64)),
            websocket_send_text => ("aurora_direct_websocket_send_text", [types::I64, types::I64, types::I64], Some(types::I64)),
            websocket_send_bytes => ("aurora_direct_websocket_send_bytes", [types::I64, types::I64, types::I64], Some(types::I64)),
            websocket_recv_text => ("aurora_direct_websocket_recv_text", [types::I64, types::I64], Some(types::I64)),
            websocket_recv_bytes => ("aurora_direct_websocket_recv_bytes", [types::I64, types::I64], Some(types::I64)),
            websocket_close => ("aurora_direct_websocket_close", [types::I64], Some(types::I64)),
            unix_listener_accept => ("aurora_direct_unix_listener_accept", [types::I64, types::I64], Some(types::I64)),
            unix_listener_close => ("aurora_direct_unix_listener_close", [types::I64], Some(types::I64)),
            unix_stream_read_line => ("aurora_direct_unix_stream_read_line", [types::I64, types::I64], Some(types::I64)),
            unix_stream_read_exact => ("aurora_direct_unix_stream_read_exact", [types::I64, types::I64, types::I64], Some(types::I64)),
            unix_stream_write_all => ("aurora_direct_unix_stream_write_all", [types::I64, types::I64, types::I64], Some(types::I64)),
            unix_stream_close => ("aurora_direct_unix_stream_close", [types::I64], Some(types::I64)),
            tls_listener_accept => ("aurora_direct_tls_listener_accept", [types::I64, types::I64], Some(types::I64)),
            tls_listener_local_addr => ("aurora_direct_tls_listener_local_addr", [types::I64], Some(types::I64)),
            tls_listener_close => ("aurora_direct_tls_listener_close", [types::I64], Some(types::I64)),
            tls_stream_read_line => ("aurora_direct_tls_stream_read_line", [types::I64, types::I64], Some(types::I64)),
            tls_stream_read_exact => ("aurora_direct_tls_stream_read_exact", [types::I64, types::I64, types::I64], Some(types::I64)),
            tls_stream_write_all => ("aurora_direct_tls_stream_write_all", [types::I64, types::I64, types::I64], Some(types::I64)),
            tls_stream_close => ("aurora_direct_tls_stream_close", [types::I64], Some(types::I64)),
            cancelled => ("aurora_direct_cancelled", [], Some(types::I64)),
            deadline_new => ("aurora_direct_deadline_new", [types::I64], Some(types::I64)),
            deadline_ready => ("aurora_direct_deadline_ready", [types::I64], Some(types::I64)),
            deadline_drop => ("aurora_direct_deadline_drop", [types::I64], None),
            select_wait => ("aurora_direct_select_wait", [types::I64, types::I64, types::I64, types::I64, types::I64, types::I64, types::I64], Some(types::I64)),
            sleep_value => ("aurora_direct_sleep_value", [types::I64], Some(types::I64)),
            spawn_call => ("aurora_direct_spawn_call", [types::I64, types::I64, types::I64, types::I64, types::I64], Some(types::I64)),
        );

        let mut functions = HashMap::new();
        let mut function_thunks = HashMap::new();
        let mut function_return_types = HashMap::new();
        let mut function_param_types = HashMap::new();
        let mut function_writeback_types = HashMap::new();
        for function in module.functions.iter().chain(module.top_level.iter()) {
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

        Ok(Self {
            module,
            program_path: program_path.to_string(),
            program_source: program_source.to_string(),
            object,
            functions,
            function_thunks,
            classes,
            trait_impls,
            function_return_types,
            function_param_types,
            function_writeback_types,
            call_conv,
            runtime_init,
            run_root,
            print_i64,
            print_f64,
            print_bool,
            print_value,
            sqrt_f64,
            fail_division_by_zero,
            fail_int32_overflow,
            box_i64,
            box_uint_literal,
            box_f64,
            box_bool,
            box_unit,
            string_literal,
            string_len,
            string_contains,
            string_starts_with,
            string_ends_with,
            string_split,
            string_replace,
            string_to_lower,
            string_to_upper,
            string_strip_prefix,
            string_strip_suffix,
            string_trim,
            string_join,
            stringify_value,
            abs_value,
            min_value,
            max_value,
            sqrt_value,
            parse_int32,
            parse_int64,
            parse_float64,
            duration_literal,
            range_new,
            range_current,
            range_end,
            range_advance,
            vec_empty,
            vec_len,
            vec_is_empty,
            vec_push_in_place,
            vec_pop_in_place,
            vec_get,
            vec_set_in_place,
            vec_remove_in_place,
            vec_swap_in_place,
            vec_contains,
            vec_extend_in_place,
            vec_insert_in_place,
            vec_clear_in_place,
            vec_reverse_in_place,
            vec_index,
            vec_index_option,
            vec_set_index_in_place,
            map_empty,
            map_len,
            map_is_empty,
            map_get,
            map_set_in_place,
            map_remove_in_place,
            map_contains_key,
            map_keys,
            map_values,
            map_items,
            map_entries,
            map_clear_in_place,
            map_extend_in_place,
            map_index,
            map_set_index_in_place,
            set_empty,
            set_len,
            set_is_empty,
            set_contains,
            set_insert_in_place,
            set_remove_in_place,
            set_index_option,
            retain_value,
            release_value,
            clone_value,
            unbox_i64,
            unbox_f64,
            unbox_bool,
            value_as_condition,
            unary_value,
            binary_value,
            cast_value,
            value_type_matches,
            enum_variant,
            variant_matches,
            variant_payload,
            instance_empty,
            instance_get_field,
            instance_set_field,
            arg_buffer_new,
            arg_buffer_store,
            i64_buffer_new,
            i64_buffer_store,
            channel_new,
            channel_can_send,
            channel_send,
            channel_recv,
            channel_recv_timeout_value,
            channel_try_recv,
            channel_close,
            task_group_new,
            task_group_cancel,
            task_group_close,
            task_join,
            io_write,
            io_flush,
            io_read_line,
            fs_exists,
            fs_read_to_string,
            fs_read_bytes,
            fs_write_string,
            fs_write_bytes,
            fs_append_string,
            fs_append_bytes,
            fs_create_dir,
            fs_read_dir,
            fs_remove_file,
            fs_open,
            fs_create,
            fs_append,
            file_read_all,
            file_read_bytes,
            file_write_all,
            file_write_bytes,
            file_flush,
            file_close,
            net_connect,
            net_connect_timeout,
            net_listen,
            net_udp_bind,
            net_unix_listen,
            net_unix_connect,
            net_unix_connect_timeout,
            net_tls_listen,
            net_tls_connect,
            net_tls_connect_timeout,
            net_http_listen,
            net_http_request_text,
            net_http_request_text_timeout,
            net_http_request_bytes,
            net_http_request_bytes_timeout,
            net_websocket_listen,
            net_websocket_connect,
            net_websocket_connect_timeout,
            tcp_listener_accept,
            tcp_listener_local_addr,
            tcp_listener_close,
            tcp_stream_read_all,
            tcp_stream_read_line,
            tcp_stream_read_bytes,
            tcp_stream_read_exact,
            tcp_stream_write_all,
            tcp_stream_write_bytes,
            tcp_stream_flush,
            tcp_stream_local_addr,
            tcp_stream_peer_addr,
            tcp_stream_shutdown_read,
            tcp_stream_shutdown_write,
            tcp_stream_shutdown_both,
            tcp_stream_close,
            udp_socket_send_text,
            udp_socket_send_bytes,
            udp_socket_recv,
            udp_socket_recv_from,
            udp_socket_local_addr,
            udp_socket_peer_addr,
            udp_socket_close,
            udp_datagram_address,
            udp_datagram_bytes,
            udp_datagram_text,
            http_listener_accept,
            http_listener_local_addr,
            http_listener_close,
            http_exchange_method,
            http_exchange_path,
            http_exchange_headers,
            http_exchange_body_text,
            http_exchange_body_bytes,
            http_exchange_respond_text,
            http_exchange_respond_bytes,
            http_response_status,
            http_response_reason,
            http_response_headers,
            http_response_text,
            http_response_bytes,
            websocket_listener_accept,
            websocket_listener_local_addr,
            websocket_send_text,
            websocket_send_bytes,
            websocket_recv_text,
            websocket_recv_bytes,
            websocket_close,
            unix_listener_accept,
            unix_listener_close,
            unix_stream_read_line,
            unix_stream_read_exact,
            unix_stream_write_all,
            unix_stream_close,
            tls_listener_accept,
            tls_listener_local_addr,
            tls_listener_close,
            tls_stream_read_line,
            tls_stream_read_exact,
            tls_stream_write_all,
            tls_stream_close,
            cancelled,
            deadline_new,
            deadline_ready,
            deadline_drop,
            select_wait,
            sleep_value,
            spawn_call,
            string_data: HashMap::new(),
        })
    }

    fn emit(mut self) -> std::result::Result<Vec<u8>, String> {
        let entry_name = if self.functions.contains_key("main") {
            Some("main")
        } else if self.functions.contains_key("__script") {
            Some("__script")
        } else {
            None
        };
        let spawn_targets = collect_spawn_targets(self.module);
        for function in self
            .module
            .functions
            .iter()
            .chain(self.module.top_level.iter())
        {
            self.define_function(function)?;
            if spawn_targets.contains(&function.name) || entry_name == Some(function.name.as_str())
            {
                self.define_function_thunk(function)?;
            }
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

        let mut blocks = HashMap::new();
        for block in &function.blocks {
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

        for block in &function.blocks {
            for instruction in &block.instructions {
                if let Instruction::Assign { target, value } = instruction {
                    if target.contains('.') {
                        continue;
                    }
                    if variables.contains_key(target) {
                        continue;
                    }
                    let ty = match infer_rvalue_type(
                        value,
                        &variable_types,
                        &self.function_return_types,
                        &self.classes,
                    ) {
                        Some(ty) => ty,
                        None => {
                            return Err(format!(
                                "direct backend could not infer direct type for temporary `{}` in `{}`",
                                target, function.name
                            ));
                        }
                    };
                    declare_root_variables(
                        &mut builder,
                        &mut variable_index,
                        &mut variables,
                        &mut variable_types,
                        target.clone(),
                        ty,
                        None,
                    );
                }
            }
            if let Terminator::Select { arms, .. } = &block.terminator {
                for arm in arms {
                    let Some(binding) = &arm.binding else {
                        continue;
                    };
                    if variables.contains_key(binding) {
                        continue;
                    }
                    let ty = match infer_select_binding_type(arm, &variable_types, &self.classes) {
                        Some(ty) => ty,
                        None => {
                            return Err(format!(
                                "direct backend could not infer direct type for select binding `{}` in `{}`",
                                binding, function.name
                            ));
                        }
                    };
                    declare_root_variables(
                        &mut builder,
                        &mut variable_index,
                        &mut variables,
                        &mut variable_types,
                        binding.clone(),
                        ty,
                        None,
                    );
                }
            }
            if let Terminator::ForRange { binding, .. } = &block.terminator {
                if !variables.contains_key(binding) {
                    declare_root_variables(
                        &mut builder,
                        &mut variable_index,
                        &mut variables,
                        &mut variable_types,
                        binding.clone(),
                        DirectType::Scalar(ScalarKind::Int32),
                        None,
                    );
                }
            }
        }

        let mut cleanup_places = Vec::<String>::new();
        for block in &function.blocks {
            for instruction in &block.instructions {
                let Instruction::PushCleanup { place } = instruction else {
                    continue;
                };
                if !cleanup_places.contains(place) {
                    cleanup_places.push(place.clone());
                }
            }
        }
        let mut cleanup_active_vars = HashMap::new();
        for place in &cleanup_places {
            let variable = Variable::from_u32(variable_index as u32);
            variable_index += 1;
            builder.declare_var(variable, types::I64);
            let zero = builder.ins().iconst(types::I64, 0);
            builder.def_var(variable, zero);
            cleanup_active_vars.insert(place.clone(), variable);
        }

        let mut writeback_locals = Vec::new();
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
        for (name, func_id) in &self.functions {
            let func_ref = self.object.declare_func_in_func(*func_id, builder.func);
            function_refs.insert(name.clone(), func_ref);
        }
        let mut function_thunk_refs = HashMap::new();
        for (name, func_id) in &self.function_thunks {
            let func_ref = self.object.declare_func_in_func(*func_id, builder.func);
            function_thunk_refs.insert(name.clone(), func_ref);
        }

        let print_i64 = self
            .object
            .declare_func_in_func(self.print_i64, builder.func);
        let print_f64 = self
            .object
            .declare_func_in_func(self.print_f64, builder.func);
        let print_bool = self
            .object
            .declare_func_in_func(self.print_bool, builder.func);
        let print_value = self
            .object
            .declare_func_in_func(self.print_value, builder.func);
        let sqrt_f64 = self
            .object
            .declare_func_in_func(self.sqrt_f64, builder.func);
        let fail_division_by_zero = self
            .object
            .declare_func_in_func(self.fail_division_by_zero, builder.func);
        let fail_int32_overflow = self
            .object
            .declare_func_in_func(self.fail_int32_overflow, builder.func);
        let box_i64 = self.object.declare_func_in_func(self.box_i64, builder.func);
        let box_uint_literal = self
            .object
            .declare_func_in_func(self.box_uint_literal, builder.func);
        let box_f64 = self.object.declare_func_in_func(self.box_f64, builder.func);
        let box_bool = self
            .object
            .declare_func_in_func(self.box_bool, builder.func);
        let box_unit = self
            .object
            .declare_func_in_func(self.box_unit, builder.func);
        let string_literal = self
            .object
            .declare_func_in_func(self.string_literal, builder.func);
        let string_len = self
            .object
            .declare_func_in_func(self.string_len, builder.func);
        let string_contains = self
            .object
            .declare_func_in_func(self.string_contains, builder.func);
        let string_starts_with = self
            .object
            .declare_func_in_func(self.string_starts_with, builder.func);
        let string_ends_with = self
            .object
            .declare_func_in_func(self.string_ends_with, builder.func);
        let string_split = self
            .object
            .declare_func_in_func(self.string_split, builder.func);
        let string_replace = self
            .object
            .declare_func_in_func(self.string_replace, builder.func);
        let string_to_lower = self
            .object
            .declare_func_in_func(self.string_to_lower, builder.func);
        let string_to_upper = self
            .object
            .declare_func_in_func(self.string_to_upper, builder.func);
        let string_strip_prefix = self
            .object
            .declare_func_in_func(self.string_strip_prefix, builder.func);
        let string_strip_suffix = self
            .object
            .declare_func_in_func(self.string_strip_suffix, builder.func);
        let string_trim = self
            .object
            .declare_func_in_func(self.string_trim, builder.func);
        let string_join = self
            .object
            .declare_func_in_func(self.string_join, builder.func);
        let stringify_value = self
            .object
            .declare_func_in_func(self.stringify_value, builder.func);
        let abs_value = self
            .object
            .declare_func_in_func(self.abs_value, builder.func);
        let min_value = self
            .object
            .declare_func_in_func(self.min_value, builder.func);
        let max_value = self
            .object
            .declare_func_in_func(self.max_value, builder.func);
        let sqrt_value = self
            .object
            .declare_func_in_func(self.sqrt_value, builder.func);
        let parse_int32 = self
            .object
            .declare_func_in_func(self.parse_int32, builder.func);
        let parse_int64 = self
            .object
            .declare_func_in_func(self.parse_int64, builder.func);
        let parse_float64 = self
            .object
            .declare_func_in_func(self.parse_float64, builder.func);
        let duration_literal = self
            .object
            .declare_func_in_func(self.duration_literal, builder.func);
        let range_new = self
            .object
            .declare_func_in_func(self.range_new, builder.func);
        let range_current = self
            .object
            .declare_func_in_func(self.range_current, builder.func);
        let range_end = self
            .object
            .declare_func_in_func(self.range_end, builder.func);
        let range_advance = self
            .object
            .declare_func_in_func(self.range_advance, builder.func);
        let vec_empty = self
            .object
            .declare_func_in_func(self.vec_empty, builder.func);
        let vec_len = self.object.declare_func_in_func(self.vec_len, builder.func);
        let vec_is_empty = self
            .object
            .declare_func_in_func(self.vec_is_empty, builder.func);
        let vec_push_in_place = self
            .object
            .declare_func_in_func(self.vec_push_in_place, builder.func);
        let vec_pop_in_place = self
            .object
            .declare_func_in_func(self.vec_pop_in_place, builder.func);
        let vec_get = self.object.declare_func_in_func(self.vec_get, builder.func);
        let vec_set_in_place = self
            .object
            .declare_func_in_func(self.vec_set_in_place, builder.func);
        let vec_remove_in_place = self
            .object
            .declare_func_in_func(self.vec_remove_in_place, builder.func);
        let vec_swap_in_place = self
            .object
            .declare_func_in_func(self.vec_swap_in_place, builder.func);
        let vec_contains = self
            .object
            .declare_func_in_func(self.vec_contains, builder.func);
        let vec_extend_in_place = self
            .object
            .declare_func_in_func(self.vec_extend_in_place, builder.func);
        let vec_insert_in_place = self
            .object
            .declare_func_in_func(self.vec_insert_in_place, builder.func);
        let vec_clear_in_place = self
            .object
            .declare_func_in_func(self.vec_clear_in_place, builder.func);
        let vec_reverse_in_place = self
            .object
            .declare_func_in_func(self.vec_reverse_in_place, builder.func);
        let vec_index = self
            .object
            .declare_func_in_func(self.vec_index, builder.func);
        let vec_index_option = self
            .object
            .declare_func_in_func(self.vec_index_option, builder.func);
        let vec_set_index_in_place = self
            .object
            .declare_func_in_func(self.vec_set_index_in_place, builder.func);
        let map_empty = self
            .object
            .declare_func_in_func(self.map_empty, builder.func);
        let map_len = self.object.declare_func_in_func(self.map_len, builder.func);
        let map_is_empty = self
            .object
            .declare_func_in_func(self.map_is_empty, builder.func);
        let map_get = self.object.declare_func_in_func(self.map_get, builder.func);
        let map_set_in_place = self
            .object
            .declare_func_in_func(self.map_set_in_place, builder.func);
        let map_remove_in_place = self
            .object
            .declare_func_in_func(self.map_remove_in_place, builder.func);
        let map_contains_key = self
            .object
            .declare_func_in_func(self.map_contains_key, builder.func);
        let map_keys = self
            .object
            .declare_func_in_func(self.map_keys, builder.func);
        let map_values = self
            .object
            .declare_func_in_func(self.map_values, builder.func);
        let map_items = self
            .object
            .declare_func_in_func(self.map_items, builder.func);
        let map_entries = self
            .object
            .declare_func_in_func(self.map_entries, builder.func);
        let map_clear_in_place = self
            .object
            .declare_func_in_func(self.map_clear_in_place, builder.func);
        let map_extend_in_place = self
            .object
            .declare_func_in_func(self.map_extend_in_place, builder.func);
        let map_index = self
            .object
            .declare_func_in_func(self.map_index, builder.func);
        let map_set_index_in_place = self
            .object
            .declare_func_in_func(self.map_set_index_in_place, builder.func);
        let set_empty = self
            .object
            .declare_func_in_func(self.set_empty, builder.func);
        let set_len = self.object.declare_func_in_func(self.set_len, builder.func);
        let set_is_empty = self
            .object
            .declare_func_in_func(self.set_is_empty, builder.func);
        let set_contains = self
            .object
            .declare_func_in_func(self.set_contains, builder.func);
        let set_insert_in_place = self
            .object
            .declare_func_in_func(self.set_insert_in_place, builder.func);
        let set_remove_in_place = self
            .object
            .declare_func_in_func(self.set_remove_in_place, builder.func);
        let set_index_option = self
            .object
            .declare_func_in_func(self.set_index_option, builder.func);
        let retain_value = self
            .object
            .declare_func_in_func(self.retain_value, builder.func);
        let release_value = self
            .object
            .declare_func_in_func(self.release_value, builder.func);
        let clone_value = self
            .object
            .declare_func_in_func(self.clone_value, builder.func);
        let unbox_i64 = self
            .object
            .declare_func_in_func(self.unbox_i64, builder.func);
        let unbox_f64 = self
            .object
            .declare_func_in_func(self.unbox_f64, builder.func);
        let unbox_bool = self
            .object
            .declare_func_in_func(self.unbox_bool, builder.func);
        let value_as_condition = self
            .object
            .declare_func_in_func(self.value_as_condition, builder.func);
        let unary_value = self
            .object
            .declare_func_in_func(self.unary_value, builder.func);
        let binary_value = self
            .object
            .declare_func_in_func(self.binary_value, builder.func);
        let cast_value = self
            .object
            .declare_func_in_func(self.cast_value, builder.func);
        let value_type_matches = self
            .object
            .declare_func_in_func(self.value_type_matches, builder.func);
        let enum_variant = self
            .object
            .declare_func_in_func(self.enum_variant, builder.func);
        let variant_matches = self
            .object
            .declare_func_in_func(self.variant_matches, builder.func);
        let variant_payload = self
            .object
            .declare_func_in_func(self.variant_payload, builder.func);
        let instance_empty = self
            .object
            .declare_func_in_func(self.instance_empty, builder.func);
        let instance_get_field = self
            .object
            .declare_func_in_func(self.instance_get_field, builder.func);
        let instance_set_field = self
            .object
            .declare_func_in_func(self.instance_set_field, builder.func);
        let arg_buffer_new = self
            .object
            .declare_func_in_func(self.arg_buffer_new, builder.func);
        let arg_buffer_store = self
            .object
            .declare_func_in_func(self.arg_buffer_store, builder.func);
        let i64_buffer_new = self
            .object
            .declare_func_in_func(self.i64_buffer_new, builder.func);
        let i64_buffer_store = self
            .object
            .declare_func_in_func(self.i64_buffer_store, builder.func);
        let channel_new = self
            .object
            .declare_func_in_func(self.channel_new, builder.func);
        let channel_can_send = self
            .object
            .declare_func_in_func(self.channel_can_send, builder.func);
        let channel_send = self
            .object
            .declare_func_in_func(self.channel_send, builder.func);
        let channel_recv = self
            .object
            .declare_func_in_func(self.channel_recv, builder.func);
        let channel_recv_timeout_value = self
            .object
            .declare_func_in_func(self.channel_recv_timeout_value, builder.func);
        let channel_try_recv = self
            .object
            .declare_func_in_func(self.channel_try_recv, builder.func);
        let channel_close = self
            .object
            .declare_func_in_func(self.channel_close, builder.func);
        let task_group_new = self
            .object
            .declare_func_in_func(self.task_group_new, builder.func);
        let task_group_cancel = self
            .object
            .declare_func_in_func(self.task_group_cancel, builder.func);
        let task_group_close = self
            .object
            .declare_func_in_func(self.task_group_close, builder.func);
        let task_join = self
            .object
            .declare_func_in_func(self.task_join, builder.func);
        let io_write = self
            .object
            .declare_func_in_func(self.io_write, builder.func);
        let io_flush = self
            .object
            .declare_func_in_func(self.io_flush, builder.func);
        let io_read_line = self
            .object
            .declare_func_in_func(self.io_read_line, builder.func);
        let fs_exists = self
            .object
            .declare_func_in_func(self.fs_exists, builder.func);
        let fs_read_to_string = self
            .object
            .declare_func_in_func(self.fs_read_to_string, builder.func);
        let fs_read_bytes = self
            .object
            .declare_func_in_func(self.fs_read_bytes, builder.func);
        let fs_write_string = self
            .object
            .declare_func_in_func(self.fs_write_string, builder.func);
        let fs_write_bytes = self
            .object
            .declare_func_in_func(self.fs_write_bytes, builder.func);
        let fs_append_string = self
            .object
            .declare_func_in_func(self.fs_append_string, builder.func);
        let fs_append_bytes = self
            .object
            .declare_func_in_func(self.fs_append_bytes, builder.func);
        let fs_create_dir = self
            .object
            .declare_func_in_func(self.fs_create_dir, builder.func);
        let fs_read_dir = self
            .object
            .declare_func_in_func(self.fs_read_dir, builder.func);
        let fs_remove_file = self
            .object
            .declare_func_in_func(self.fs_remove_file, builder.func);
        let fs_open = self.object.declare_func_in_func(self.fs_open, builder.func);
        let fs_create = self
            .object
            .declare_func_in_func(self.fs_create, builder.func);
        let fs_append = self
            .object
            .declare_func_in_func(self.fs_append, builder.func);
        let file_read_all = self
            .object
            .declare_func_in_func(self.file_read_all, builder.func);
        let file_read_bytes = self
            .object
            .declare_func_in_func(self.file_read_bytes, builder.func);
        let file_write_all = self
            .object
            .declare_func_in_func(self.file_write_all, builder.func);
        let file_write_bytes = self
            .object
            .declare_func_in_func(self.file_write_bytes, builder.func);
        let file_flush = self
            .object
            .declare_func_in_func(self.file_flush, builder.func);
        let file_close = self
            .object
            .declare_func_in_func(self.file_close, builder.func);
        let net_connect = self
            .object
            .declare_func_in_func(self.net_connect, builder.func);
        let net_connect_timeout = self
            .object
            .declare_func_in_func(self.net_connect_timeout, builder.func);
        let net_listen = self
            .object
            .declare_func_in_func(self.net_listen, builder.func);
        let net_udp_bind = self
            .object
            .declare_func_in_func(self.net_udp_bind, builder.func);
        let net_unix_listen = self
            .object
            .declare_func_in_func(self.net_unix_listen, builder.func);
        let net_unix_connect = self
            .object
            .declare_func_in_func(self.net_unix_connect, builder.func);
        let net_unix_connect_timeout = self
            .object
            .declare_func_in_func(self.net_unix_connect_timeout, builder.func);
        let net_tls_listen = self
            .object
            .declare_func_in_func(self.net_tls_listen, builder.func);
        let net_tls_connect = self
            .object
            .declare_func_in_func(self.net_tls_connect, builder.func);
        let net_tls_connect_timeout = self
            .object
            .declare_func_in_func(self.net_tls_connect_timeout, builder.func);
        let net_http_listen = self
            .object
            .declare_func_in_func(self.net_http_listen, builder.func);
        let net_http_request_text = self
            .object
            .declare_func_in_func(self.net_http_request_text, builder.func);
        let net_http_request_text_timeout = self
            .object
            .declare_func_in_func(self.net_http_request_text_timeout, builder.func);
        let net_http_request_bytes = self
            .object
            .declare_func_in_func(self.net_http_request_bytes, builder.func);
        let net_http_request_bytes_timeout = self
            .object
            .declare_func_in_func(self.net_http_request_bytes_timeout, builder.func);
        let net_websocket_listen = self
            .object
            .declare_func_in_func(self.net_websocket_listen, builder.func);
        let net_websocket_connect = self
            .object
            .declare_func_in_func(self.net_websocket_connect, builder.func);
        let net_websocket_connect_timeout = self
            .object
            .declare_func_in_func(self.net_websocket_connect_timeout, builder.func);
        let tcp_listener_accept = self
            .object
            .declare_func_in_func(self.tcp_listener_accept, builder.func);
        let tcp_listener_local_addr = self
            .object
            .declare_func_in_func(self.tcp_listener_local_addr, builder.func);
        let tcp_listener_close = self
            .object
            .declare_func_in_func(self.tcp_listener_close, builder.func);
        let tcp_stream_read_all = self
            .object
            .declare_func_in_func(self.tcp_stream_read_all, builder.func);
        let tcp_stream_read_line = self
            .object
            .declare_func_in_func(self.tcp_stream_read_line, builder.func);
        let tcp_stream_read_bytes = self
            .object
            .declare_func_in_func(self.tcp_stream_read_bytes, builder.func);
        let tcp_stream_read_exact = self
            .object
            .declare_func_in_func(self.tcp_stream_read_exact, builder.func);
        let tcp_stream_write_all = self
            .object
            .declare_func_in_func(self.tcp_stream_write_all, builder.func);
        let tcp_stream_write_bytes = self
            .object
            .declare_func_in_func(self.tcp_stream_write_bytes, builder.func);
        let tcp_stream_flush = self
            .object
            .declare_func_in_func(self.tcp_stream_flush, builder.func);
        let tcp_stream_local_addr = self
            .object
            .declare_func_in_func(self.tcp_stream_local_addr, builder.func);
        let tcp_stream_peer_addr = self
            .object
            .declare_func_in_func(self.tcp_stream_peer_addr, builder.func);
        let tcp_stream_shutdown_read = self
            .object
            .declare_func_in_func(self.tcp_stream_shutdown_read, builder.func);
        let tcp_stream_shutdown_write = self
            .object
            .declare_func_in_func(self.tcp_stream_shutdown_write, builder.func);
        let tcp_stream_shutdown_both = self
            .object
            .declare_func_in_func(self.tcp_stream_shutdown_both, builder.func);
        let tcp_stream_close = self
            .object
            .declare_func_in_func(self.tcp_stream_close, builder.func);
        let udp_socket_send_text = self
            .object
            .declare_func_in_func(self.udp_socket_send_text, builder.func);
        let udp_socket_send_bytes = self
            .object
            .declare_func_in_func(self.udp_socket_send_bytes, builder.func);
        let udp_socket_recv = self
            .object
            .declare_func_in_func(self.udp_socket_recv, builder.func);
        let udp_socket_recv_from = self
            .object
            .declare_func_in_func(self.udp_socket_recv_from, builder.func);
        let udp_socket_local_addr = self
            .object
            .declare_func_in_func(self.udp_socket_local_addr, builder.func);
        let udp_socket_peer_addr = self
            .object
            .declare_func_in_func(self.udp_socket_peer_addr, builder.func);
        let udp_socket_close = self
            .object
            .declare_func_in_func(self.udp_socket_close, builder.func);
        let udp_datagram_address = self
            .object
            .declare_func_in_func(self.udp_datagram_address, builder.func);
        let udp_datagram_bytes = self
            .object
            .declare_func_in_func(self.udp_datagram_bytes, builder.func);
        let udp_datagram_text = self
            .object
            .declare_func_in_func(self.udp_datagram_text, builder.func);
        let http_listener_accept = self
            .object
            .declare_func_in_func(self.http_listener_accept, builder.func);
        let http_listener_local_addr = self
            .object
            .declare_func_in_func(self.http_listener_local_addr, builder.func);
        let http_listener_close = self
            .object
            .declare_func_in_func(self.http_listener_close, builder.func);
        let http_exchange_method = self
            .object
            .declare_func_in_func(self.http_exchange_method, builder.func);
        let http_exchange_path = self
            .object
            .declare_func_in_func(self.http_exchange_path, builder.func);
        let http_exchange_headers = self
            .object
            .declare_func_in_func(self.http_exchange_headers, builder.func);
        let http_exchange_body_text = self
            .object
            .declare_func_in_func(self.http_exchange_body_text, builder.func);
        let http_exchange_body_bytes = self
            .object
            .declare_func_in_func(self.http_exchange_body_bytes, builder.func);
        let http_exchange_respond_text = self
            .object
            .declare_func_in_func(self.http_exchange_respond_text, builder.func);
        let http_exchange_respond_bytes = self
            .object
            .declare_func_in_func(self.http_exchange_respond_bytes, builder.func);
        let http_response_status = self
            .object
            .declare_func_in_func(self.http_response_status, builder.func);
        let http_response_reason = self
            .object
            .declare_func_in_func(self.http_response_reason, builder.func);
        let http_response_headers = self
            .object
            .declare_func_in_func(self.http_response_headers, builder.func);
        let http_response_text = self
            .object
            .declare_func_in_func(self.http_response_text, builder.func);
        let http_response_bytes = self
            .object
            .declare_func_in_func(self.http_response_bytes, builder.func);
        let websocket_listener_accept = self
            .object
            .declare_func_in_func(self.websocket_listener_accept, builder.func);
        let websocket_listener_local_addr = self
            .object
            .declare_func_in_func(self.websocket_listener_local_addr, builder.func);
        let websocket_send_text = self
            .object
            .declare_func_in_func(self.websocket_send_text, builder.func);
        let websocket_send_bytes = self
            .object
            .declare_func_in_func(self.websocket_send_bytes, builder.func);
        let websocket_recv_text = self
            .object
            .declare_func_in_func(self.websocket_recv_text, builder.func);
        let websocket_recv_bytes = self
            .object
            .declare_func_in_func(self.websocket_recv_bytes, builder.func);
        let websocket_close = self
            .object
            .declare_func_in_func(self.websocket_close, builder.func);
        let unix_listener_accept = self
            .object
            .declare_func_in_func(self.unix_listener_accept, builder.func);
        let unix_listener_close = self
            .object
            .declare_func_in_func(self.unix_listener_close, builder.func);
        let unix_stream_read_line = self
            .object
            .declare_func_in_func(self.unix_stream_read_line, builder.func);
        let unix_stream_read_exact = self
            .object
            .declare_func_in_func(self.unix_stream_read_exact, builder.func);
        let unix_stream_write_all = self
            .object
            .declare_func_in_func(self.unix_stream_write_all, builder.func);
        let unix_stream_close = self
            .object
            .declare_func_in_func(self.unix_stream_close, builder.func);
        let tls_listener_accept = self
            .object
            .declare_func_in_func(self.tls_listener_accept, builder.func);
        let tls_listener_local_addr = self
            .object
            .declare_func_in_func(self.tls_listener_local_addr, builder.func);
        let tls_listener_close = self
            .object
            .declare_func_in_func(self.tls_listener_close, builder.func);
        let tls_stream_read_line = self
            .object
            .declare_func_in_func(self.tls_stream_read_line, builder.func);
        let tls_stream_read_exact = self
            .object
            .declare_func_in_func(self.tls_stream_read_exact, builder.func);
        let tls_stream_write_all = self
            .object
            .declare_func_in_func(self.tls_stream_write_all, builder.func);
        let tls_stream_close = self
            .object
            .declare_func_in_func(self.tls_stream_close, builder.func);
        let cancelled = self
            .object
            .declare_func_in_func(self.cancelled, builder.func);
        let deadline_new = self
            .object
            .declare_func_in_func(self.deadline_new, builder.func);
        let deadline_ready = self
            .object
            .declare_func_in_func(self.deadline_ready, builder.func);
        let deadline_drop = self
            .object
            .declare_func_in_func(self.deadline_drop, builder.func);
        let select_wait = self
            .object
            .declare_func_in_func(self.select_wait, builder.func);
        let sleep_value = self
            .object
            .declare_func_in_func(self.sleep_value, builder.func);
        let spawn_call = self
            .object
            .declare_func_in_func(self.spawn_call, builder.func);

        let mut compiler = FunctionCompiler {
            builder,
            blocks,
            variables,
            variable_types,
            next_variable_index: variable_index,
            function_refs,
            function_thunk_refs,
            function_return_types: self.function_return_types.clone(),
            function_param_types: self.function_param_types.clone(),
            function_writeback_types: self.function_writeback_types.clone(),
            writeback_locals,
            classes: self.classes.clone(),
            trait_impls: self.trait_impls.clone(),
            owned_opaque_temporaries: HashSet::new(),
            object: &mut self.object,
            string_data: &mut self.string_data,
            cleanup_places,
            cleanup_active_vars,
            print_i64,
            print_f64,
            print_bool,
            print_value,
            sqrt_f64,
            fail_division_by_zero,
            fail_int32_overflow,
            box_i64,
            box_uint_literal,
            box_f64,
            box_bool,
            box_unit,
            string_literal,
            string_len,
            string_contains,
            string_starts_with,
            string_ends_with,
            string_split,
            string_replace,
            string_to_lower,
            string_to_upper,
            string_strip_prefix,
            string_strip_suffix,
            string_trim,
            string_join,
            stringify_value,
            abs_value,
            min_value,
            max_value,
            sqrt_value,
            parse_int32,
            parse_int64,
            parse_float64,
            duration_literal,
            range_new,
            range_current,
            range_end,
            range_advance,
            vec_empty,
            vec_len,
            vec_is_empty,
            vec_push_in_place,
            vec_pop_in_place,
            vec_get,
            vec_set_in_place,
            vec_remove_in_place,
            vec_swap_in_place,
            vec_contains,
            vec_extend_in_place,
            vec_insert_in_place,
            vec_clear_in_place,
            vec_reverse_in_place,
            vec_index,
            vec_index_option,
            vec_set_index_in_place,
            map_empty,
            map_len,
            map_is_empty,
            map_get,
            map_set_in_place,
            map_remove_in_place,
            map_contains_key,
            map_keys,
            map_values,
            map_items,
            map_entries,
            map_clear_in_place,
            map_extend_in_place,
            map_index,
            map_set_index_in_place,
            set_empty,
            set_len,
            set_is_empty,
            set_contains,
            set_insert_in_place,
            set_remove_in_place,
            set_index_option,
            retain_value,
            release_value,
            clone_value,
            unbox_i64,
            unbox_f64,
            unbox_bool,
            value_as_condition,
            unary_value,
            binary_value,
            cast_value,
            value_type_matches,
            enum_variant,
            variant_matches,
            variant_payload,
            instance_empty,
            instance_get_field,
            instance_set_field,
            arg_buffer_new,
            arg_buffer_store,
            i64_buffer_new,
            i64_buffer_store,
            channel_new,
            channel_can_send,
            channel_send,
            channel_recv,
            channel_recv_timeout_value,
            channel_try_recv,
            channel_close,
            task_group_new,
            task_group_cancel,
            task_group_close,
            task_join,
            io_write,
            io_flush,
            io_read_line,
            fs_exists,
            fs_read_to_string,
            fs_read_bytes,
            fs_write_string,
            fs_write_bytes,
            fs_append_string,
            fs_append_bytes,
            fs_create_dir,
            fs_read_dir,
            fs_remove_file,
            fs_open,
            fs_create,
            fs_append,
            file_read_all,
            file_read_bytes,
            file_write_all,
            file_write_bytes,
            file_flush,
            file_close,
            net_connect,
            net_connect_timeout,
            net_listen,
            net_udp_bind,
            net_unix_listen,
            net_unix_connect,
            net_unix_connect_timeout,
            net_tls_listen,
            net_tls_connect,
            net_tls_connect_timeout,
            net_http_listen,
            net_http_request_text,
            net_http_request_text_timeout,
            net_http_request_bytes,
            net_http_request_bytes_timeout,
            net_websocket_listen,
            net_websocket_connect,
            net_websocket_connect_timeout,
            tcp_listener_accept,
            tcp_listener_local_addr,
            tcp_listener_close,
            tcp_stream_read_all,
            tcp_stream_read_line,
            tcp_stream_read_bytes,
            tcp_stream_read_exact,
            tcp_stream_write_all,
            tcp_stream_write_bytes,
            tcp_stream_flush,
            tcp_stream_local_addr,
            tcp_stream_peer_addr,
            tcp_stream_shutdown_read,
            tcp_stream_shutdown_write,
            tcp_stream_shutdown_both,
            tcp_stream_close,
            udp_socket_send_text,
            udp_socket_send_bytes,
            udp_socket_recv,
            udp_socket_recv_from,
            udp_socket_local_addr,
            udp_socket_peer_addr,
            udp_socket_close,
            udp_datagram_address,
            udp_datagram_bytes,
            udp_datagram_text,
            http_listener_accept,
            http_listener_local_addr,
            http_listener_close,
            http_exchange_method,
            http_exchange_path,
            http_exchange_headers,
            http_exchange_body_text,
            http_exchange_body_bytes,
            http_exchange_respond_text,
            http_exchange_respond_bytes,
            http_response_status,
            http_response_reason,
            http_response_headers,
            http_response_text,
            http_response_bytes,
            websocket_listener_accept,
            websocket_listener_local_addr,
            websocket_send_text,
            websocket_send_bytes,
            websocket_recv_text,
            websocket_recv_bytes,
            websocket_close,
            unix_listener_accept,
            unix_listener_close,
            unix_stream_read_line,
            unix_stream_read_exact,
            unix_stream_write_all,
            unix_stream_close,
            tls_listener_accept,
            tls_listener_local_addr,
            tls_listener_close,
            tls_stream_read_line,
            tls_stream_read_exact,
            tls_stream_write_all,
            tls_stream_close,
            cancelled,
            deadline_new,
            deadline_ready,
            deadline_drop,
            select_wait,
            sleep_value,
            spawn_call,
        };

        let return_ty = ensure_direct_type(
            &function.return_type,
            &self.classes,
            &format!("return type of `{}`", function.name),
        )?;
        for block in &function.blocks {
            compiler.compile_block(block, &return_ty)?;
        }

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
                "direct backend does not yet support spawn thunks for methods like `{}`",
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
            .declare_func_in_func(self.unbox_i64, builder.func);
        let unbox_f64 = self
            .object
            .declare_func_in_func(self.unbox_f64, builder.func);
        let unbox_bool = self
            .object
            .declare_func_in_func(self.unbox_bool, builder.func);
        let release_value = self
            .object
            .declare_func_in_func(self.release_value, builder.func);

        let mut lowered_args = Vec::new();
        let param_types = self
            .function_param_types
            .get(&function.name)
            .cloned()
            .unwrap_or_default();
        for (index, param_ty) in param_types.iter().enumerate() {
            let raw = builder
                .ins()
                .load(types::I64, MemFlags::new(), args_ptr, (index as i32) * 8);
            match param_ty {
                DirectType::Opaque(_) => lowered_args.push(raw),
                DirectType::Scalar(ScalarKind::Int32) => {
                    let inst = builder.ins().call(unbox_i64, &[raw]);
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
                DirectType::PlainClass(_) => {
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
        let boxed = box_thunk_value(self, &mut builder, &results, &return_ty)?;
        builder.ins().return_(&[boxed]);
        builder.finalize();

        try_or_string_error!(
            self.object.define_function(thunk_id, &mut ctx),
            "failed to define direct function thunk `{}`: {}",
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
            .declare_func_in_func(self.runtime_init, builder.func);
        let run_root = self
            .object
            .declare_func_in_func(self.run_root, builder.func);
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

struct FunctionCompiler<'a> {
    builder: FunctionBuilder<'a>,
    blocks: HashMap<String, cranelift_codegen::ir::Block>,
    variables: HashMap<String, Vec<Variable>>,
    variable_types: HashMap<String, DirectType>,
    next_variable_index: usize,
    function_refs: HashMap<String, cranelift_codegen::ir::FuncRef>,
    function_thunk_refs: HashMap<String, cranelift_codegen::ir::FuncRef>,
    function_return_types: HashMap<String, DirectType>,
    function_param_types: HashMap<String, Vec<DirectType>>,
    function_writeback_types: HashMap<String, Vec<DirectType>>,
    writeback_locals: Vec<(String, DirectType)>,
    classes: HashMap<String, MirClass>,
    trait_impls: Vec<MirTraitImpl>,
    owned_opaque_temporaries: HashSet<Value>,
    object: &'a mut ObjectModule,
    string_data: &'a mut HashMap<Vec<u8>, DataId>,
    cleanup_places: Vec<String>,
    cleanup_active_vars: HashMap<String, Variable>,
    print_i64: cranelift_codegen::ir::FuncRef,
    print_f64: cranelift_codegen::ir::FuncRef,
    print_bool: cranelift_codegen::ir::FuncRef,
    print_value: cranelift_codegen::ir::FuncRef,
    sqrt_f64: cranelift_codegen::ir::FuncRef,
    fail_division_by_zero: cranelift_codegen::ir::FuncRef,
    fail_int32_overflow: cranelift_codegen::ir::FuncRef,
    box_i64: cranelift_codegen::ir::FuncRef,
    box_uint_literal: cranelift_codegen::ir::FuncRef,
    box_f64: cranelift_codegen::ir::FuncRef,
    box_bool: cranelift_codegen::ir::FuncRef,
    box_unit: cranelift_codegen::ir::FuncRef,
    string_literal: cranelift_codegen::ir::FuncRef,
    string_len: cranelift_codegen::ir::FuncRef,
    string_contains: cranelift_codegen::ir::FuncRef,
    string_starts_with: cranelift_codegen::ir::FuncRef,
    string_ends_with: cranelift_codegen::ir::FuncRef,
    string_split: cranelift_codegen::ir::FuncRef,
    string_replace: cranelift_codegen::ir::FuncRef,
    string_to_lower: cranelift_codegen::ir::FuncRef,
    string_to_upper: cranelift_codegen::ir::FuncRef,
    string_strip_prefix: cranelift_codegen::ir::FuncRef,
    string_strip_suffix: cranelift_codegen::ir::FuncRef,
    string_trim: cranelift_codegen::ir::FuncRef,
    string_join: cranelift_codegen::ir::FuncRef,
    stringify_value: cranelift_codegen::ir::FuncRef,
    abs_value: cranelift_codegen::ir::FuncRef,
    min_value: cranelift_codegen::ir::FuncRef,
    max_value: cranelift_codegen::ir::FuncRef,
    sqrt_value: cranelift_codegen::ir::FuncRef,
    parse_int32: cranelift_codegen::ir::FuncRef,
    parse_int64: cranelift_codegen::ir::FuncRef,
    parse_float64: cranelift_codegen::ir::FuncRef,
    duration_literal: cranelift_codegen::ir::FuncRef,
    range_new: cranelift_codegen::ir::FuncRef,
    range_current: cranelift_codegen::ir::FuncRef,
    range_end: cranelift_codegen::ir::FuncRef,
    range_advance: cranelift_codegen::ir::FuncRef,
    vec_empty: cranelift_codegen::ir::FuncRef,
    vec_len: cranelift_codegen::ir::FuncRef,
    vec_is_empty: cranelift_codegen::ir::FuncRef,
    vec_push_in_place: cranelift_codegen::ir::FuncRef,
    vec_pop_in_place: cranelift_codegen::ir::FuncRef,
    vec_get: cranelift_codegen::ir::FuncRef,
    vec_set_in_place: cranelift_codegen::ir::FuncRef,
    vec_remove_in_place: cranelift_codegen::ir::FuncRef,
    vec_swap_in_place: cranelift_codegen::ir::FuncRef,
    vec_contains: cranelift_codegen::ir::FuncRef,
    vec_extend_in_place: cranelift_codegen::ir::FuncRef,
    vec_insert_in_place: cranelift_codegen::ir::FuncRef,
    vec_clear_in_place: cranelift_codegen::ir::FuncRef,
    vec_reverse_in_place: cranelift_codegen::ir::FuncRef,
    vec_index: cranelift_codegen::ir::FuncRef,
    vec_index_option: cranelift_codegen::ir::FuncRef,
    vec_set_index_in_place: cranelift_codegen::ir::FuncRef,
    map_empty: cranelift_codegen::ir::FuncRef,
    map_len: cranelift_codegen::ir::FuncRef,
    map_is_empty: cranelift_codegen::ir::FuncRef,
    map_get: cranelift_codegen::ir::FuncRef,
    map_set_in_place: cranelift_codegen::ir::FuncRef,
    map_remove_in_place: cranelift_codegen::ir::FuncRef,
    map_contains_key: cranelift_codegen::ir::FuncRef,
    map_keys: cranelift_codegen::ir::FuncRef,
    map_values: cranelift_codegen::ir::FuncRef,
    map_items: cranelift_codegen::ir::FuncRef,
    map_entries: cranelift_codegen::ir::FuncRef,
    map_clear_in_place: cranelift_codegen::ir::FuncRef,
    map_extend_in_place: cranelift_codegen::ir::FuncRef,
    map_index: cranelift_codegen::ir::FuncRef,
    map_set_index_in_place: cranelift_codegen::ir::FuncRef,
    set_empty: cranelift_codegen::ir::FuncRef,
    set_len: cranelift_codegen::ir::FuncRef,
    set_is_empty: cranelift_codegen::ir::FuncRef,
    set_contains: cranelift_codegen::ir::FuncRef,
    set_insert_in_place: cranelift_codegen::ir::FuncRef,
    set_remove_in_place: cranelift_codegen::ir::FuncRef,
    set_index_option: cranelift_codegen::ir::FuncRef,
    retain_value: cranelift_codegen::ir::FuncRef,
    release_value: cranelift_codegen::ir::FuncRef,
    clone_value: cranelift_codegen::ir::FuncRef,
    unbox_i64: cranelift_codegen::ir::FuncRef,
    unbox_f64: cranelift_codegen::ir::FuncRef,
    unbox_bool: cranelift_codegen::ir::FuncRef,
    value_as_condition: cranelift_codegen::ir::FuncRef,
    unary_value: cranelift_codegen::ir::FuncRef,
    binary_value: cranelift_codegen::ir::FuncRef,
    cast_value: cranelift_codegen::ir::FuncRef,
    value_type_matches: cranelift_codegen::ir::FuncRef,
    enum_variant: cranelift_codegen::ir::FuncRef,
    variant_matches: cranelift_codegen::ir::FuncRef,
    variant_payload: cranelift_codegen::ir::FuncRef,
    instance_empty: cranelift_codegen::ir::FuncRef,
    instance_get_field: cranelift_codegen::ir::FuncRef,
    instance_set_field: cranelift_codegen::ir::FuncRef,
    arg_buffer_new: cranelift_codegen::ir::FuncRef,
    arg_buffer_store: cranelift_codegen::ir::FuncRef,
    i64_buffer_new: cranelift_codegen::ir::FuncRef,
    i64_buffer_store: cranelift_codegen::ir::FuncRef,
    channel_new: cranelift_codegen::ir::FuncRef,
    channel_can_send: cranelift_codegen::ir::FuncRef,
    channel_send: cranelift_codegen::ir::FuncRef,
    channel_recv: cranelift_codegen::ir::FuncRef,
    channel_recv_timeout_value: cranelift_codegen::ir::FuncRef,
    channel_try_recv: cranelift_codegen::ir::FuncRef,
    channel_close: cranelift_codegen::ir::FuncRef,
    task_group_new: cranelift_codegen::ir::FuncRef,
    task_group_cancel: cranelift_codegen::ir::FuncRef,
    task_group_close: cranelift_codegen::ir::FuncRef,
    task_join: cranelift_codegen::ir::FuncRef,
    io_write: cranelift_codegen::ir::FuncRef,
    io_flush: cranelift_codegen::ir::FuncRef,
    io_read_line: cranelift_codegen::ir::FuncRef,
    fs_exists: cranelift_codegen::ir::FuncRef,
    fs_read_to_string: cranelift_codegen::ir::FuncRef,
    fs_read_bytes: cranelift_codegen::ir::FuncRef,
    fs_write_string: cranelift_codegen::ir::FuncRef,
    fs_write_bytes: cranelift_codegen::ir::FuncRef,
    fs_append_string: cranelift_codegen::ir::FuncRef,
    fs_append_bytes: cranelift_codegen::ir::FuncRef,
    fs_create_dir: cranelift_codegen::ir::FuncRef,
    fs_read_dir: cranelift_codegen::ir::FuncRef,
    fs_remove_file: cranelift_codegen::ir::FuncRef,
    fs_open: cranelift_codegen::ir::FuncRef,
    fs_create: cranelift_codegen::ir::FuncRef,
    fs_append: cranelift_codegen::ir::FuncRef,
    file_read_all: cranelift_codegen::ir::FuncRef,
    file_read_bytes: cranelift_codegen::ir::FuncRef,
    file_write_all: cranelift_codegen::ir::FuncRef,
    file_write_bytes: cranelift_codegen::ir::FuncRef,
    file_flush: cranelift_codegen::ir::FuncRef,
    file_close: cranelift_codegen::ir::FuncRef,
    net_connect: cranelift_codegen::ir::FuncRef,
    net_connect_timeout: cranelift_codegen::ir::FuncRef,
    net_listen: cranelift_codegen::ir::FuncRef,
    net_udp_bind: cranelift_codegen::ir::FuncRef,
    net_unix_listen: cranelift_codegen::ir::FuncRef,
    net_unix_connect: cranelift_codegen::ir::FuncRef,
    net_unix_connect_timeout: cranelift_codegen::ir::FuncRef,
    net_tls_listen: cranelift_codegen::ir::FuncRef,
    net_tls_connect: cranelift_codegen::ir::FuncRef,
    net_tls_connect_timeout: cranelift_codegen::ir::FuncRef,
    net_http_listen: cranelift_codegen::ir::FuncRef,
    net_http_request_text: cranelift_codegen::ir::FuncRef,
    net_http_request_text_timeout: cranelift_codegen::ir::FuncRef,
    net_http_request_bytes: cranelift_codegen::ir::FuncRef,
    net_http_request_bytes_timeout: cranelift_codegen::ir::FuncRef,
    net_websocket_listen: cranelift_codegen::ir::FuncRef,
    net_websocket_connect: cranelift_codegen::ir::FuncRef,
    net_websocket_connect_timeout: cranelift_codegen::ir::FuncRef,
    tcp_listener_accept: cranelift_codegen::ir::FuncRef,
    tcp_listener_local_addr: cranelift_codegen::ir::FuncRef,
    tcp_listener_close: cranelift_codegen::ir::FuncRef,
    tcp_stream_read_all: cranelift_codegen::ir::FuncRef,
    tcp_stream_read_line: cranelift_codegen::ir::FuncRef,
    tcp_stream_read_bytes: cranelift_codegen::ir::FuncRef,
    tcp_stream_read_exact: cranelift_codegen::ir::FuncRef,
    tcp_stream_write_all: cranelift_codegen::ir::FuncRef,
    tcp_stream_write_bytes: cranelift_codegen::ir::FuncRef,
    tcp_stream_flush: cranelift_codegen::ir::FuncRef,
    tcp_stream_local_addr: cranelift_codegen::ir::FuncRef,
    tcp_stream_peer_addr: cranelift_codegen::ir::FuncRef,
    tcp_stream_shutdown_read: cranelift_codegen::ir::FuncRef,
    tcp_stream_shutdown_write: cranelift_codegen::ir::FuncRef,
    tcp_stream_shutdown_both: cranelift_codegen::ir::FuncRef,
    tcp_stream_close: cranelift_codegen::ir::FuncRef,
    udp_socket_send_text: cranelift_codegen::ir::FuncRef,
    udp_socket_send_bytes: cranelift_codegen::ir::FuncRef,
    udp_socket_recv: cranelift_codegen::ir::FuncRef,
    udp_socket_recv_from: cranelift_codegen::ir::FuncRef,
    udp_socket_local_addr: cranelift_codegen::ir::FuncRef,
    udp_socket_peer_addr: cranelift_codegen::ir::FuncRef,
    udp_socket_close: cranelift_codegen::ir::FuncRef,
    udp_datagram_address: cranelift_codegen::ir::FuncRef,
    udp_datagram_bytes: cranelift_codegen::ir::FuncRef,
    udp_datagram_text: cranelift_codegen::ir::FuncRef,
    http_listener_accept: cranelift_codegen::ir::FuncRef,
    http_listener_local_addr: cranelift_codegen::ir::FuncRef,
    http_listener_close: cranelift_codegen::ir::FuncRef,
    http_exchange_method: cranelift_codegen::ir::FuncRef,
    http_exchange_path: cranelift_codegen::ir::FuncRef,
    http_exchange_headers: cranelift_codegen::ir::FuncRef,
    http_exchange_body_text: cranelift_codegen::ir::FuncRef,
    http_exchange_body_bytes: cranelift_codegen::ir::FuncRef,
    http_exchange_respond_text: cranelift_codegen::ir::FuncRef,
    http_exchange_respond_bytes: cranelift_codegen::ir::FuncRef,
    http_response_status: cranelift_codegen::ir::FuncRef,
    http_response_reason: cranelift_codegen::ir::FuncRef,
    http_response_headers: cranelift_codegen::ir::FuncRef,
    http_response_text: cranelift_codegen::ir::FuncRef,
    http_response_bytes: cranelift_codegen::ir::FuncRef,
    websocket_listener_accept: cranelift_codegen::ir::FuncRef,
    websocket_listener_local_addr: cranelift_codegen::ir::FuncRef,
    websocket_send_text: cranelift_codegen::ir::FuncRef,
    websocket_send_bytes: cranelift_codegen::ir::FuncRef,
    websocket_recv_text: cranelift_codegen::ir::FuncRef,
    websocket_recv_bytes: cranelift_codegen::ir::FuncRef,
    websocket_close: cranelift_codegen::ir::FuncRef,
    unix_listener_accept: cranelift_codegen::ir::FuncRef,
    unix_listener_close: cranelift_codegen::ir::FuncRef,
    unix_stream_read_line: cranelift_codegen::ir::FuncRef,
    unix_stream_read_exact: cranelift_codegen::ir::FuncRef,
    unix_stream_write_all: cranelift_codegen::ir::FuncRef,
    unix_stream_close: cranelift_codegen::ir::FuncRef,
    tls_listener_accept: cranelift_codegen::ir::FuncRef,
    tls_listener_local_addr: cranelift_codegen::ir::FuncRef,
    tls_listener_close: cranelift_codegen::ir::FuncRef,
    tls_stream_read_line: cranelift_codegen::ir::FuncRef,
    tls_stream_read_exact: cranelift_codegen::ir::FuncRef,
    tls_stream_write_all: cranelift_codegen::ir::FuncRef,
    tls_stream_close: cranelift_codegen::ir::FuncRef,
    cancelled: cranelift_codegen::ir::FuncRef,
    deadline_new: cranelift_codegen::ir::FuncRef,
    deadline_ready: cranelift_codegen::ir::FuncRef,
    deadline_drop: cranelift_codegen::ir::FuncRef,
    select_wait: cranelift_codegen::ir::FuncRef,
    sleep_value: cranelift_codegen::ir::FuncRef,
    spawn_call: cranelift_codegen::ir::FuncRef,
}

impl<'a> FunctionCompiler<'a> {
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
        let inst = self.builder.ins().call(self.retain_value, &[value]);
        self.builder.inst_results(inst)[0]
    }

    fn release_opaque_handle(&mut self, value: Value) {
        let _ = self.builder.ins().call(self.release_value, &[value]);
    }

    fn release_all_temporary_owned(&mut self) {
        let owned = self
            .owned_opaque_temporaries
            .iter()
            .copied()
            .collect::<Vec<_>>();
        for value in owned {
            self.release_opaque_handle(value);
        }
        self.owned_opaque_temporaries.clear();
    }

    fn release_root_if_opaque(&mut self, name: &str) -> std::result::Result<(), String> {
        let ty = self
            .variable_types
            .get(name)
            .ok_or_else(|| format!("direct backend does not know local type for `{}`", name))?;
        if !matches!(ty, DirectType::Opaque(_)) {
            return Ok(());
        }
        let vars = self
            .variables
            .get(name)
            .ok_or_else(|| format!("direct backend does not know local `{}`", name))?;
        let current = self.builder.use_var(vars[0]);
        self.release_opaque_handle(current);
        Ok(())
    }

    fn release_all_opaque_roots(&mut self) -> std::result::Result<(), String> {
        let names = self.variable_types.keys().cloned().collect::<Vec<_>>();
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

    fn export_return_value(&mut self, value: ValueRef) -> Vec<Value> {
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

    fn drop_deadlines(&mut self, deadlines: &[Value]) {
        for deadline in deadlines {
            let _ = self.builder.ins().call(self.deadline_drop, &[*deadline]);
        }
    }

    fn compile_block(
        &mut self,
        block: &BasicBlock,
        return_ty: &DirectType,
    ) -> std::result::Result<(), String> {
        let block_id = self.blocks[&block.label];
        if self.builder.current_block() != Some(block_id) {
            self.builder.switch_to_block(block_id);
        }
        self.owned_opaque_temporaries.clear();

        for instruction in &block.instructions {
            self.compile_instruction(instruction)?;
        }
        self.compile_terminator(&block.terminator, return_ty)?;
        Ok(())
    }

    fn compile_instruction(
        &mut self,
        instruction: &Instruction,
    ) -> std::result::Result<(), String> {
        match instruction {
            Instruction::Assign { target, value } => {
                if let Rvalue::Try { value: try_value } = value {
                    let target_ty = self.type_of_place(target)?;
                    self.compile_try_assign(target, target_ty, try_value)?;
                } else {
                    let target_ty = self.type_of_place(target)?;
                    let compiled = self.compile_rvalue(value)?;
                    let coerced = self.coerce_value(compiled, &target_ty)?;
                    self.store_place(target, coerced)?;
                }
            }
            Instruction::Eval { value } => {
                let _ = self.load_operand(value)?;
            }
            Instruction::PushCleanup { place } => {
                self.set_cleanup_active(place, true)?;
            }
            Instruction::PopCleanup {
                place,
                cancel_before_cleanup,
            } => {
                self.emit_cleanup_for_place(place, *cancel_before_cleanup)?;
                self.set_cleanup_active(place, false)?;
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
                let value = self.load_operand(operand)?;
                let coerced = self.coerce_value(value, return_ty)?;
                self.emit_pending_cleanups(true)?;
                let return_values = self.build_return_values(coerced)?;
                self.release_all_temporary_owned();
                self.release_all_opaque_roots()?;
                self.builder.ins().return_(&return_values);
            }
            Terminator::Goto(label) => {
                self.release_all_temporary_owned();
                let block = self.blocks[label];
                self.builder.ins().jump(block, &[]);
            }
            Terminator::Branch {
                condition,
                then_label,
                else_label,
            } => {
                let condition = self.load_operand(condition)?;
                let condition = self.as_bool_value(condition)?;
                let then_block = self.blocks[then_label];
                let else_block = self.blocks[else_label];
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
                        self.builder.ins().jump(self.blocks[&arm.label], &[]);
                        return Ok(());
                    }
                    let next_block = self.builder.create_block();
                    let matched = self.variant_matches_value(
                        scrutinee.values[0],
                        arm.enum_name.as_deref().unwrap_or(scrutinee_enum_name),
                        arm.variant_name.as_deref().unwrap_or_default(),
                    )?;
                    let arm_block = self.blocks[&arm.label];
                    self.builder
                        .ins()
                        .brif(matched, arm_block, &[], next_block, &[]);
                    self.builder.switch_to_block(next_block);
                }
                self.release_all_temporary_owned();
                self.builder.ins().jump(self.blocks[otherwise], &[]);
            }
            Terminator::Select { arms, otherwise } => {
                self.compile_select(arms, otherwise)?;
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
            other => {
                return Err(format!(
                    "direct backend does not support MIR terminator `{:?}`",
                    other
                ))
            }
        }
        Ok(())
    }

    fn compile_rvalue(&mut self, rvalue: &Rvalue) -> std::result::Result<ValueRef, String> {
        match rvalue {
            Rvalue::Use(operand) => self.load_operand(operand),
            Rvalue::FormatString { parts } => self.compile_format_string(parts),
            Rvalue::Unary { op, value, span } => {
                let value = self.load_operand(value)?;
                self.compile_unary(*op, value, Some(*span))
            }
            Rvalue::Cast { value, ty, span } => {
                let value = self.load_operand(value)?;
                self.compile_cast(value, ty, Some(*span))
            }
            Rvalue::Binary {
                op,
                left,
                right,
                span,
            } => {
                let left = self.load_operand(left)?;
                let right = self.load_operand(right)?;
                self.compile_binary(*op, left, right, Some(*span))
            }
            Rvalue::Call { callee, args } => self.compile_call(callee, args),
            Rvalue::VecLiteral {
                elements,
                element_type,
            } => {
                let init = self.builder.ins().call(self.vec_empty, &[]);
                let vector = self.owned_opaque_result(
                    self.builder.inst_results(init).to_vec(),
                    Type::Named("Vec".to_string(), vec![element_type.clone()]),
                );
                for element in elements {
                    let value = self.load_operand(element)?;
                    let value = self.ensure_opaque(value)?;
                    let _ = self
                        .builder
                        .ins()
                        .call(self.vec_push_in_place, &[vector.values[0], value.values[0]]);
                }
                Ok(vector)
            }
            Rvalue::MapLiteral {
                entries,
                key_type,
                value_type,
            } => {
                let init = self.builder.ins().call(self.map_empty, &[]);
                let map = self.owned_opaque_result(
                    self.builder.inst_results(init).to_vec(),
                    Type::Named(
                        "Map".to_string(),
                        vec![key_type.clone(), value_type.clone()],
                    ),
                );
                for entry in entries {
                    let key = self.load_operand(&entry.key)?;
                    let key = self.ensure_opaque(key)?;
                    let value = self.load_operand(&entry.value)?;
                    let value = self.ensure_opaque(value)?;
                    let _ = self.builder.ins().call(
                        self.map_set_in_place,
                        &[map.values[0], key.values[0], value.values[0]],
                    );
                }
                Ok(map)
            }
            Rvalue::SetLiteral {
                elements,
                element_type,
            } => {
                let init = self.builder.ins().call(self.set_empty, &[]);
                let set = self.owned_opaque_result(
                    self.builder.inst_results(init).to_vec(),
                    Type::Named("Set".to_string(), vec![element_type.clone()]),
                );
                for element in elements {
                    let value = self.load_operand(element)?;
                    let value = self.ensure_opaque(value)?;
                    let _ = self
                        .builder
                        .ins()
                        .call(self.set_insert_in_place, &[set.values[0], value.values[0]]);
                }
                Ok(set)
            }
            Rvalue::Construct { class_name, fields } => self.compile_construct(class_name, fields),
            Rvalue::Member { object, field } => {
                let object = self.load_operand(object)?;
                self.extract_field(object, field)
            }
            Rvalue::EnumVariant {
                enum_name,
                variant_name,
                payloads,
            } => self.compile_enum_variant(enum_name, variant_name, payloads),
            Rvalue::VariantPayload { scrutinee, index } => {
                let scrutinee = self.load_operand(scrutinee)?;
                self.compile_variant_payload(scrutinee, *index)
            }
            Rvalue::Spawn {
                detached,
                task_group,
                function,
                args,
            } => self.compile_spawn(*detached, task_group.as_ref(), function, args),
            other => Err(format!(
                "direct backend does not support MIR rvalue `{:?}`",
                other
            )),
        }
    }

    fn compile_unary(
        &mut self,
        op: UnaryOp,
        value: ValueRef,
        span: Option<Span>,
    ) -> std::result::Result<ValueRef, String> {
        if matches!(value.ty, DirectType::Opaque(_)) {
            let opcode = match op {
                UnaryOp::Neg => 0,
                UnaryOp::Not => 1,
            };
            let opcode = self.builder.ins().iconst(types::I64, opcode);
            let (line, column) = self.span_values(span);
            let inst = self
                .builder
                .ins()
                .call(self.unary_value, &[opcode, value.values[0], line, column]);
            return Ok(self.owned_opaque_result(
                self.builder.inst_results(inst).to_vec(),
                Type::named("Unknown"),
            ));
        }
        match (op, value.ty.scalar_kind()) {
            (UnaryOp::Neg, Some(ScalarKind::Int32)) => Ok(ValueRef {
                values: vec![self.builder.ins().ineg(value.values[0])],
                ty: DirectType::Scalar(ScalarKind::Int32),
            }),
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

    fn compile_cast(
        &mut self,
        value: ValueRef,
        target: &Type,
        span: Option<Span>,
    ) -> std::result::Result<ValueRef, String> {
        let target_ty = ensure_direct_type(target, &self.classes, "cast target")?;
        if matches!(value.ty, DirectType::Opaque(_)) || matches!(target_ty, DirectType::Opaque(_)) {
            let boxed = self.ensure_opaque(value)?;
            let (target_ptr, target_len) = self.string_constant(target.to_string().as_bytes())?;
            let (line, column) = self.span_values(span);
            let inst = self.builder.ins().call(
                self.cast_value,
                &[boxed.values[0], target_ptr, target_len, line, column],
            );
            let boxed =
                self.owned_opaque_result(self.builder.inst_results(inst).to_vec(), target.clone());
            return self.coerce_value(boxed, &target_ty);
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
            (ScalarKind::Int32, ScalarKind::Int32) => source,
            (ScalarKind::Int32, kind) if kind.is_float() => self
                .builder
                .ins()
                .fcvt_from_sint(target_kind.signature_type(), source),
            (kind, ScalarKind::Int32) if kind.is_float() => {
                let converted = self.builder.ins().fcvt_to_sint_sat(types::I64, source);
                self.emit_int32_bounds_check(converted, span);
                converted
            }
            (lhs, rhs) if lhs.is_float() && rhs.is_float() => source,
            _ => {
                return Err(format!(
                    "direct backend only supports numeric casts, found `{}` to `{}`",
                    render_direct_type(&value.ty),
                    target
                ))
            }
        };

        Ok(ValueRef {
            values: vec![result],
            ty: DirectType::Scalar(target_kind),
        })
    }

    fn compile_binary(
        &mut self,
        op: BinaryOp,
        left: ValueRef,
        right: ValueRef,
        span: Option<Span>,
    ) -> std::result::Result<ValueRef, String> {
        if matches!(left.ty, DirectType::Opaque(_)) || matches!(right.ty, DirectType::Opaque(_)) {
            let left = self.ensure_opaque(left)?;
            let right = self.ensure_opaque(right)?;
            let binary_opcode = self.binary_opcode(op);
            let opcode = self.builder.ins().iconst(types::I64, binary_opcode);
            let (line, column) = self.span_values(span);
            let inst = self.builder.ins().call(
                self.binary_value,
                &[opcode, left.values[0], right.values[0], line, column],
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
            (Some(lhs), Some(rhs)) if lhs.is_float() && rhs.is_float() => {
                self.compile_float_binary(op, left.values[0], right.values[0], lhs, span)
            }
            (Some(ScalarKind::Bool), Some(ScalarKind::Bool)) => {
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
            BinaryOp::Div => {
                self.emit_int_division_guard(right, span);
                ValueRef {
                    values: vec![self.builder.ins().sdiv(left, right)],
                    ty,
                }
            }
            BinaryOp::Mod => {
                self.emit_int_division_guard(right, span);
                ValueRef {
                    values: vec![self.builder.ins().srem(left, right)],
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
            self.emit_int32_bounds_check(value.values[0], span);
        }
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
                self.emit_float_division_guard(right, span);
                Ok(ValueRef {
                    values: vec![self.builder.ins().fdiv(left, right)],
                    ty,
                })
            }
            BinaryOp::Mod => {
                let opcode_value = self.binary_opcode(BinaryOp::Mod);
                let left_box = self.builder.ins().call(self.box_f64, &[left]);
                let right_box = self.builder.ins().call(self.box_f64, &[right]);
                let left_boxed = self.builder.inst_results(left_box)[0];
                let right_boxed = self.builder.inst_results(right_box)[0];
                let opcode = self.builder.ins().iconst(types::I64, opcode_value);
                let (line, column) = self.span_values(span);
                let result = self.builder.ins().call(
                    self.binary_value,
                    &[opcode, left_boxed, right_boxed, line, column],
                );
                let result_boxed = self.builder.inst_results(result)[0];
                let unboxed = self.builder.ins().call(self.unbox_f64, &[result_boxed]);
                Ok(ValueRef {
                    values: self.builder.inst_results(unboxed).to_vec(),
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

    fn emit_int_division_guard(&mut self, divisor: Value, span: Option<Span>) {
        let zero = self.builder.ins().iconst(types::I64, 0);
        let is_zero = self.builder.ins().icmp(IntCC::Equal, divisor, zero);
        self.emit_division_failure_branch(is_zero, span);
    }

    fn emit_float_division_guard(&mut self, divisor: Value, span: Option<Span>) {
        let zero = self.builder.ins().f64const(Ieee64::with_float(0.0));
        let is_zero = self.builder.ins().fcmp(FloatCC::Equal, divisor, zero);
        self.emit_division_failure_branch(is_zero, span);
    }

    fn emit_division_failure_branch(&mut self, is_zero: Value, span: Option<Span>) {
        let fail_block = self.builder.create_block();
        let continue_block = self.builder.create_block();
        self.builder
            .ins()
            .brif(is_zero, fail_block, &[], continue_block, &[]);
        self.builder.switch_to_block(fail_block);
        let (line, column) = self.span_values(span);
        self.builder
            .ins()
            .call(self.fail_division_by_zero, &[line, column]);
        self.builder.ins().trap(TrapCode::INTEGER_DIVISION_BY_ZERO);
        self.builder.seal_block(fail_block);
        self.builder.switch_to_block(continue_block);
        self.builder.seal_block(continue_block);
    }

    fn compile_call(
        &mut self,
        callee: &CallTarget,
        args: &[MirArg],
    ) -> std::result::Result<ValueRef, String> {
        match callee {
            CallTarget::Name(name) if name == "print" => self.compile_print(args),
            CallTarget::Name(name) => self.compile_named_call(name, args),
            CallTarget::Member {
                object,
                field,
                receiver_place,
            } => self.compile_member_call(object, field, receiver_place.as_deref(), args),
        }
    }

    fn compile_print(&mut self, args: &[MirArg]) -> std::result::Result<ValueRef, String> {
        let Some(argument) = args.first() else {
            return Err("direct backend expected `print` to receive one argument".to_string());
        };
        let argument = self.load_operand(&argument.value)?;
        match argument.ty.scalar_kind() {
            Some(ScalarKind::Int32) => {
                self.builder
                    .ins()
                    .call(self.print_i64, &[argument.values[0]]);
            }
            Some(ScalarKind::Float32) | Some(ScalarKind::Float64) => {
                self.builder
                    .ins()
                    .call(self.print_f64, &[argument.values[0]]);
            }
            Some(ScalarKind::Bool) => {
                self.builder
                    .ins()
                    .call(self.print_bool, &[argument.values[0]]);
            }
            Some(ScalarKind::Unit) => {}
            None => {
                let argument = self.ensure_opaque(argument)?;
                self.builder
                    .ins()
                    .call(self.print_value, &[argument.values[0]]);
            }
        }
        Ok(unit_value(&mut self.builder))
    }

    fn compile_format_string(
        &mut self,
        parts: &[MirFormatPart],
    ) -> std::result::Result<ValueRef, String> {
        let mut current = self.string_value("")?;
        for part in parts {
            let next = match part {
                MirFormatPart::Literal(text) => self.string_value(text)?,
                MirFormatPart::Value(value) => {
                    let value = self.load_operand(value)?;
                    let value = self.ensure_opaque(value)?;
                    let call = self
                        .builder
                        .ins()
                        .call(self.stringify_value, &[value.values[0]]);
                    self.owned_opaque_result(
                        self.builder.inst_results(call).to_vec(),
                        Type::named("String"),
                    )
                }
            };
            current = self.compile_binary(BinaryOp::Add, current, next, None)?;
        }
        Ok(current)
    }

    fn string_value(&mut self, text: &str) -> std::result::Result<ValueRef, String> {
        let (ptr, len) = self.string_constant(text.as_bytes())?;
        let call = self.builder.ins().call(self.string_literal, &[ptr, len]);
        Ok(self.owned_opaque_result(
            self.builder.inst_results(call).to_vec(),
            Type::named("String"),
        ))
    }

    fn compile_named_call(
        &mut self,
        name: &str,
        args: &[MirArg],
    ) -> std::result::Result<ValueRef, String> {
        if name == "range" {
            return self.compile_range(args);
        }
        if name == "queue" {
            if args.len() > 1 {
                return Err(format!(
                    "direct backend expected `{}()` to take at most one capacity argument",
                    name
                ));
            }
            let capacity = match args {
                [] => self.builder.ins().iconst(types::I64, 0),
                [argument] => {
                    if argument.name.as_deref() != Some("capacity") && argument.name.is_some() {
                        return Err(
                            "direct backend expected `queue()` to receive only `capacity=`"
                                .to_string(),
                        );
                    }
                    let value = self.load_operand(&argument.value)?;
                    let value = self.ensure_opaque(value)?;
                    value.values[0]
                }
                _ => unreachable!(),
            };
            let inst = self.builder.ins().call(self.channel_new, &[capacity]);
            return Ok(self.owned_opaque_result(
                self.builder.inst_results(inst).to_vec(),
                Type::Named("Queue".to_string(), vec![Type::named("Unknown")]),
            ));
        }
        if name == "tasks" {
            if !args.is_empty() {
                return Err(format!(
                    "direct backend expected `{}()` to take no arguments",
                    name
                ));
            }
            let inst = self.builder.ins().call(self.task_group_new, &[]);
            return Ok(self.owned_opaque_result(
                self.builder.inst_results(inst).to_vec(),
                Type::named("TaskGroup"),
            ));
        }
        if name == "cancelled" {
            if !args.is_empty() {
                return Err(
                    "direct backend expected `cancelled()` to take no arguments".to_string()
                );
            }
            let inst = self.builder.ins().call(self.cancelled, &[]);
            return Ok(ValueRef {
                values: self.builder.inst_results(inst).to_vec(),
                ty: DirectType::Scalar(ScalarKind::Bool),
            });
        }
        if name == "sleep" {
            let [argument] = args else {
                return Err(
                    "direct backend expected `sleep()` to receive one duration argument"
                        .to_string(),
                );
            };
            let duration = self.load_operand(&argument.value)?;
            let duration = self.ensure_opaque(duration)?;
            let inst = self
                .builder
                .ins()
                .call(self.sleep_value, &[duration.values[0]]);
            return Ok(ValueRef {
                values: self.builder.inst_results(inst).to_vec(),
                ty: DirectType::Scalar(ScalarKind::Unit),
            });
        }
        if matches!(
            name,
            "io::write"
                | "io::flush"
                | "io::read_line"
                | "fs::exists"
                | "fs::read_to_string"
                | "fs::read_bytes"
                | "fs::write_string"
                | "fs::write_bytes"
                | "fs::append_string"
                | "fs::append_bytes"
                | "fs::create_dir"
                | "fs::read_dir"
                | "fs::remove_file"
                | "fs::open"
                | "fs::create"
                | "fs::append"
                | "net::connect"
                | "net::connect_timeout"
                | "net::listen"
                | "net::udp_bind"
                | "net::unix_listen"
                | "net::unix_connect"
                | "net::unix_connect_timeout"
                | "net::tls_listen"
                | "net::tls_connect"
                | "net::tls_connect_timeout"
                | "net::http_listen"
                | "net::http_request_text"
                | "net::http_request_text_timeout"
                | "net::http_request_bytes"
                | "net::http_request_bytes_timeout"
                | "net::websocket_listen"
                | "net::websocket_connect"
                | "net::websocket_connect_timeout"
        ) {
            return self.compile_builtin_io_named_call(name, args);
        }
        if name == "abs" {
            let [argument] = args else {
                return Err("direct backend expected `abs()` to receive one argument".to_string());
            };
            let loaded = self.load_operand(&argument.value)?;
            let return_ty = loaded.ty.clone();
            let value = self.ensure_opaque(loaded)?;
            let inst = self.builder.ins().call(self.abs_value, &[value.values[0]]);
            let result = self.owned_opaque_result(
                self.builder.inst_results(inst).to_vec(),
                Type::named("Unknown"),
            );
            return self.coerce_value(result, &return_ty);
        }
        if matches!(name, "parse_int32" | "parse_int64" | "parse_float64") {
            let [argument] = args else {
                return Err(format!(
                    "direct backend expected `{}`() to receive one string argument",
                    name
                ));
            };
            let loaded = self.load_operand(&argument.value)?;
            let value = self.ensure_opaque(loaded)?;
            let func = match name {
                "parse_int32" => self.parse_int32,
                "parse_int64" => self.parse_int64,
                "parse_float64" => self.parse_float64,
                _ => unreachable!(),
            };
            let inst = self.builder.ins().call(func, &[value.values[0]]);
            let return_ty = match name {
                "parse_int32" => Type::Named(
                    "Result".to_string(),
                    vec![Type::named("int32"), Type::named("String")],
                ),
                "parse_int64" => Type::Named(
                    "Result".to_string(),
                    vec![Type::named("int64"), Type::named("String")],
                ),
                "parse_float64" => Type::Named(
                    "Result".to_string(),
                    vec![Type::named("float64"), Type::named("String")],
                ),
                _ => unreachable!(),
            };
            return Ok(
                self.owned_opaque_result(self.builder.inst_results(inst).to_vec(), return_ty)
            );
        }
        if name == "min" || name == "max" {
            let [left_arg, right_arg] = args else {
                return Err(format!(
                    "direct backend expected `{}`() to receive two arguments",
                    name
                ));
            };
            let left = self.load_operand(&left_arg.value)?;
            let return_ty = left.ty.clone();
            let left = self.ensure_opaque(left)?;
            let right = self.load_operand(&right_arg.value)?;
            let right = self.ensure_opaque(right)?;
            let func = if name == "min" {
                self.min_value
            } else {
                self.max_value
            };
            let inst = self
                .builder
                .ins()
                .call(func, &[left.values[0], right.values[0]]);
            let result = self.owned_opaque_result(
                self.builder.inst_results(inst).to_vec(),
                Type::named("Unknown"),
            );
            return self.coerce_value(result, &return_ty);
        }
        if name == "sqrt" {
            let [argument] = args else {
                return Err("direct backend expected `sqrt()` to receive one argument".to_string());
            };
            let loaded = self.load_operand(&argument.value)?;
            let return_ty = loaded.ty.clone();
            let value = self.ensure_opaque(loaded)?;
            let inst = self.builder.ins().call(self.sqrt_value, &[value.values[0]]);
            let result = self.owned_opaque_result(
                self.builder.inst_results(inst).to_vec(),
                Type::named("Unknown"),
            );
            return self.coerce_value(result, &return_ty);
        }
        if matches!(name, "Vec" | "Set" | "Map") {
            if !args.is_empty() {
                return Err(format!(
                    "direct backend expected `{}`() to take no arguments",
                    name
                ));
            }
            let func = match name {
                "Vec" => self.vec_empty,
                "Set" => self.set_empty,
                "Map" => self.map_empty,
                _ => unreachable!(),
            };
            let inst = self.builder.ins().call(func, &[]);
            let ty = match name {
                "Vec" | "Set" => Type::Named(name.to_string(), vec![Type::named("Unknown")]),
                "Map" => Type::Named(
                    "Map".to_string(),
                    vec![Type::named("Unknown"), Type::named("Unknown")],
                ),
                _ => unreachable!(),
            };
            return Ok(self.owned_opaque_result(self.builder.inst_results(inst).to_vec(), ty));
        }
        let func_ref = *self
            .function_refs
            .get(name)
            .ok_or_else(|| format!("direct backend does not know function `{}`", name))?;
        let mut lowered_args = Vec::new();
        let expected = self
            .function_param_types
            .get(name)
            .cloned()
            .unwrap_or_default();
        let mut writeback_places = Vec::new();
        for (index, argument) in args.iter().enumerate() {
            let loaded = self.load_operand(&argument.value)?;
            let coerced = if let Some(expected_ty) = expected.get(index) {
                self.coerce_value(loaded, expected_ty)?
            } else {
                loaded
            };
            if let Some(place) = &argument.writeback_place {
                writeback_places.push(place.clone());
            }
            if matches!(coerced.ty, DirectType::Opaque(_)) {
                lowered_args.push(self.transfer_opaque_arg(&coerced));
            } else {
                lowered_args.extend(coerced.values);
            }
        }
        let inst = self.builder.ins().call(func_ref, &lowered_args);
        let results = self.builder.inst_results(inst).to_vec();
        let (result, writebacks) = self.split_call_results(name, results)?;
        self.apply_writeback_places(&writeback_places, writebacks)?;
        Ok(result)
    }

    fn compile_range(&mut self, args: &[MirArg]) -> std::result::Result<ValueRef, String> {
        let int_ty = DirectType::Scalar(ScalarKind::Int32);
        let (start_arg, stop_arg) = if args.iter().all(|arg| arg.name.is_none()) {
            match args {
                [stop] => (None, Some(stop)),
                [start, stop] => (Some(start), Some(stop)),
                _ => {
                    return Err(
                        "direct backend expected `range()` to receive one or two arguments"
                            .to_string(),
                    )
                }
            }
        } else {
            let mut start = None;
            let mut stop = None;
            let mut next_positional = 0usize;
            for arg in args {
                match arg.name.as_deref() {
                    Some("start") => start = Some(arg),
                    Some("stop") => stop = Some(arg),
                    Some(other) => {
                        return Err(format!(
                            "direct backend does not recognize `range()` argument `{}`",
                            other
                        ))
                    }
                    None => {
                        if next_positional == 0 {
                            start = Some(arg);
                        } else if next_positional == 1 {
                            stop = Some(arg);
                        } else {
                            return Err(
                                "direct backend expected `range()` to receive one or two arguments"
                                    .to_string(),
                            );
                        }
                        next_positional += 1;
                    }
                }
            }
            (start, stop)
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
        let stop_arg = stop_arg.ok_or_else(|| {
            "direct backend expected `range()` to receive a `stop` argument".to_string()
        })?;
        let stop = self.load_operand(&stop_arg.value)?;
        let stop = self.coerce_value(stop, &int_ty)?;
        let inst = self
            .builder
            .ins()
            .call(self.range_new, &[start.values[0], stop.values[0]]);
        Ok(self.owned_opaque_result(
            self.builder.inst_results(inst).to_vec(),
            Type::named("Range"),
        ))
    }

    fn lower_optional_opaque_arg(
        &mut self,
        argument: Option<&MirArg>,
    ) -> std::result::Result<Value, String> {
        if let Some(argument) = argument {
            let loaded = self.load_operand(&argument.value)?;
            let value = self.ensure_opaque(loaded)?;
            Ok(value.values[0])
        } else {
            Ok(self.builder.ins().iconst(types::I64, 0))
        }
    }

    fn compile_builtin_io_named_call(
        &mut self,
        name: &str,
        args: &[MirArg],
    ) -> std::result::Result<ValueRef, String> {
        let expected_names: &[&str] = match name {
            "io::write" => &["text"],
            "io::flush" | "io::read_line" => &[],
            "fs::exists" | "fs::read_to_string" | "fs::read_bytes" | "fs::create_dir"
            | "fs::read_dir" | "fs::remove_file" | "fs::open" | "fs::create" | "fs::append"
            | "net::unix_listen" | "net::unix_connect" => &["path"],
            "net::connect"
            | "net::listen"
            | "net::udp_bind"
            | "net::http_listen"
            | "net::websocket_listen" => &["address"],
            "net::websocket_connect" => &["url"],
            "fs::write_string" | "fs::append_string" => &["path", "text"],
            "fs::write_bytes" | "fs::append_bytes" => &["path", "bytes"],
            "net::connect_timeout" => &["address", "timeout"],
            "net::unix_connect_timeout" => &["path", "timeout"],
            "net::websocket_connect_timeout" => &["url", "timeout"],
            "net::tls_listen" => &["address", "cert_pem_path", "key_pem_path"],
            "net::tls_connect" => &["address", "server_name", "ca_pem_path"],
            "net::tls_connect_timeout" => &["address", "server_name", "ca_pem_path", "timeout"],
            "net::http_request_text" => &["method", "url", "body", "headers"],
            "net::http_request_text_timeout" => &["method", "url", "body", "headers", "timeout"],
            "net::http_request_bytes" => &["method", "url", "bytes", "headers"],
            "net::http_request_bytes_timeout" => &["method", "url", "bytes", "headers", "timeout"],
            _ => {
                return Err(format!(
                    "direct backend does not know builtin I/O call `{}`",
                    name
                ))
            }
        };
        let func = match name {
            "io::write" => self.io_write,
            "io::flush" => self.io_flush,
            "io::read_line" => self.io_read_line,
            "fs::exists" => self.fs_exists,
            "fs::read_to_string" => self.fs_read_to_string,
            "fs::read_bytes" => self.fs_read_bytes,
            "fs::write_string" => self.fs_write_string,
            "fs::write_bytes" => self.fs_write_bytes,
            "fs::append_string" => self.fs_append_string,
            "fs::append_bytes" => self.fs_append_bytes,
            "fs::create_dir" => self.fs_create_dir,
            "fs::read_dir" => self.fs_read_dir,
            "fs::remove_file" => self.fs_remove_file,
            "fs::open" => self.fs_open,
            "fs::create" => self.fs_create,
            "fs::append" => self.fs_append,
            "net::connect" => self.net_connect,
            "net::connect_timeout" => self.net_connect_timeout,
            "net::listen" => self.net_listen,
            "net::udp_bind" => self.net_udp_bind,
            "net::unix_listen" => self.net_unix_listen,
            "net::unix_connect" => self.net_unix_connect,
            "net::unix_connect_timeout" => self.net_unix_connect_timeout,
            "net::tls_listen" => self.net_tls_listen,
            "net::tls_connect" => self.net_tls_connect,
            "net::tls_connect_timeout" => self.net_tls_connect_timeout,
            "net::http_listen" => self.net_http_listen,
            "net::http_request_text" => self.net_http_request_text,
            "net::http_request_text_timeout" => self.net_http_request_text_timeout,
            "net::http_request_bytes" => self.net_http_request_bytes,
            "net::http_request_bytes_timeout" => self.net_http_request_bytes_timeout,
            "net::websocket_listen" => self.net_websocket_listen,
            "net::websocket_connect" => self.net_websocket_connect,
            "net::websocket_connect_timeout" => self.net_websocket_connect_timeout,
            _ => unreachable!(),
        };
        let bound = ordered_optional_named_args(expected_names, args)?;
        let mut lowered_args = Vec::new();
        for (index, argument) in bound.iter().enumerate() {
            let optional_timeout = matches!(
                name,
                "net::connect_timeout"
                    | "net::unix_connect_timeout"
                    | "net::tls_connect_timeout"
                    | "net::http_request_text_timeout"
                    | "net::http_request_bytes_timeout"
                    | "net::websocket_connect_timeout"
            ) && index == expected_names.len() - 1;
            if optional_timeout {
                lowered_args.push(self.lower_optional_opaque_arg(*argument)?);
                continue;
            }
            let argument = argument
                .ok_or_else(|| "direct backend is missing a builtin argument".to_string())?;
            let loaded = self.load_operand(&argument.value)?;
            let value = self.ensure_opaque(loaded)?;
            lowered_args.push(value.values[0]);
        }
        let inst = self.builder.ins().call(func, &lowered_args);
        let results = self.builder.inst_results(inst).to_vec();
        let io_error_ty = Type::Named("io.Error".to_string(), Vec::new());
        let bytes_ty = Type::Named("Vec".to_string(), vec![Type::named("uint8")]);
        match name {
            "fs::exists" => Ok(ValueRef {
                values: results,
                ty: DirectType::Scalar(ScalarKind::Bool),
            }),
            "io::write" | "io::flush" => Ok(self.owned_opaque_result(
                results,
                Type::Named(
                    "Result".to_string(),
                    vec![Type::Unit, Type::Named("io.Error".to_string(), Vec::new())],
                ),
            )),
            "io::read_line" => Ok(self.owned_opaque_result(
                results,
                Type::Named(
                    "Result".to_string(),
                    vec![
                        Type::Named("Option".to_string(), vec![Type::named("String")]),
                        Type::Named("io.Error".to_string(), Vec::new()),
                    ],
                ),
            )),
            "fs::read_to_string" => Ok(self.owned_opaque_result(
                results,
                Type::Named(
                    "Result".to_string(),
                    vec![
                        Type::named("String"),
                        Type::Named("io.Error".to_string(), Vec::new()),
                    ],
                ),
            )),
            "fs::read_bytes" => Ok(self.owned_opaque_result(
                results,
                Type::Named(
                    "Result".to_string(),
                    vec![bytes_ty.clone(), io_error_ty.clone()],
                ),
            )),
            "fs::write_string" | "fs::write_bytes" | "fs::append_string" | "fs::append_bytes"
            | "fs::create_dir" | "fs::remove_file" => Ok(self.owned_opaque_result(
                results,
                Type::Named("Result".to_string(), vec![Type::Unit, io_error_ty.clone()]),
            )),
            "fs::read_dir" => Ok(self.owned_opaque_result(
                results,
                Type::Named(
                    "Result".to_string(),
                    vec![
                        Type::Named("Vec".to_string(), vec![Type::named("String")]),
                        io_error_ty.clone(),
                    ],
                ),
            )),
            "fs::open" | "fs::create" | "fs::append" => Ok(self.owned_opaque_result(
                results,
                Type::Named(
                    "Result".to_string(),
                    vec![
                        Type::Named("fs.File".to_string(), Vec::new()),
                        io_error_ty.clone(),
                    ],
                ),
            )),
            "net::connect" | "net::connect_timeout" => Ok(self.owned_opaque_result(
                results,
                Type::Named(
                    "Result".to_string(),
                    vec![
                        Type::Named("net.TcpStream".to_string(), Vec::new()),
                        io_error_ty.clone(),
                    ],
                ),
            )),
            "net::listen" => Ok(self.owned_opaque_result(
                results,
                Type::Named(
                    "Result".to_string(),
                    vec![
                        Type::Named("net.TcpListener".to_string(), Vec::new()),
                        io_error_ty.clone(),
                    ],
                ),
            )),
            "net::udp_bind" => Ok(self.owned_opaque_result(
                results,
                Type::Named(
                    "Result".to_string(),
                    vec![
                        Type::Named("net.UdpSocket".to_string(), Vec::new()),
                        io_error_ty.clone(),
                    ],
                ),
            )),
            "net::unix_listen" => Ok(self.owned_opaque_result(
                results,
                Type::Named(
                    "Result".to_string(),
                    vec![
                        Type::Named("net.UnixListener".to_string(), Vec::new()),
                        io_error_ty.clone(),
                    ],
                ),
            )),
            "net::unix_connect" | "net::unix_connect_timeout" => Ok(self.owned_opaque_result(
                results,
                Type::Named(
                    "Result".to_string(),
                    vec![
                        Type::Named("net.UnixStream".to_string(), Vec::new()),
                        io_error_ty.clone(),
                    ],
                ),
            )),
            "net::tls_listen" => Ok(self.owned_opaque_result(
                results,
                Type::Named(
                    "Result".to_string(),
                    vec![
                        Type::Named("net.TlsListener".to_string(), Vec::new()),
                        io_error_ty.clone(),
                    ],
                ),
            )),
            "net::tls_connect" | "net::tls_connect_timeout" => Ok(self.owned_opaque_result(
                results,
                Type::Named(
                    "Result".to_string(),
                    vec![
                        Type::Named("net.TlsStream".to_string(), Vec::new()),
                        io_error_ty.clone(),
                    ],
                ),
            )),
            "net::http_listen" => Ok(self.owned_opaque_result(
                results,
                Type::Named(
                    "Result".to_string(),
                    vec![
                        Type::Named("net.HttpListener".to_string(), Vec::new()),
                        io_error_ty.clone(),
                    ],
                ),
            )),
            "net::http_request_text"
            | "net::http_request_text_timeout"
            | "net::http_request_bytes"
            | "net::http_request_bytes_timeout" => Ok(self.owned_opaque_result(
                results,
                Type::Named(
                    "Result".to_string(),
                    vec![
                        Type::Named("net.HttpResponse".to_string(), Vec::new()),
                        io_error_ty.clone(),
                    ],
                ),
            )),
            "net::websocket_listen" => Ok(self.owned_opaque_result(
                results,
                Type::Named(
                    "Result".to_string(),
                    vec![
                        Type::Named("net.WebSocketListener".to_string(), Vec::new()),
                        io_error_ty.clone(),
                    ],
                ),
            )),
            "net::websocket_connect" | "net::websocket_connect_timeout" => Ok(self
                .owned_opaque_result(
                    results,
                    Type::Named(
                        "Result".to_string(),
                        vec![
                            Type::Named("net.WebSocket".to_string(), Vec::new()),
                            io_error_ty.clone(),
                        ],
                    ),
                )),
            _ => unreachable!(),
        }
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
            .call(self.range_current, &[range.values[0]]);
        let current = self.builder.inst_results(current_inst)[0];
        let end_inst = self.builder.ins().call(self.range_end, &[range.values[0]]);
        let end = self.builder.inst_results(end_inst)[0];
        let has_next = self.builder.ins().icmp(IntCC::SignedLessThan, current, end);

        let next_block = self.builder.create_block();
        let body_block = self.blocks[body_label];
        let exit_block = self.blocks[exit_label];
        self.builder
            .ins()
            .brif(has_next, next_block, &[], exit_block, &[]);

        self.builder.switch_to_block(next_block);
        let binding_ty = self.type_of_place(binding)?;
        self.store_place(
            binding,
            ValueRef {
                values: vec![current],
                ty: DirectType::Scalar(ScalarKind::Int32),
            },
        )?;
        let advanced_inst = self
            .builder
            .ins()
            .call(self.range_advance, &[range.values[0]]);
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

    fn compile_member_call(
        &mut self,
        object: &Operand,
        field: &str,
        receiver_place: Option<&str>,
        args: &[MirArg],
    ) -> std::result::Result<ValueRef, String> {
        let object = self.load_operand(object)?;

        if matches!(object.ty.scalar_kind(), Some(kind) if kind.is_float()) && field == "sqrt" {
            if !args.is_empty() {
                return Err("direct backend expected `sqrt()` to take no arguments".to_string());
            }
            let inst = self.builder.ins().call(self.sqrt_f64, &[object.values[0]]);
            return Ok(ValueRef {
                values: self.builder.inst_results(inst).to_vec(),
                ty: object.ty,
            });
        }

        match object.ty.clone() {
            DirectType::PlainClass(class_ty) => self.compile_class_member_call(
                class_ty.class_name.as_str(),
                Some(Type::named(&class_ty.class_name)),
                object,
                field,
                receiver_place,
                args,
            ),
            DirectType::Opaque(ty) => {
                if let Type::Named(_name, _type_args) = &ty {
                    return self.compile_opaque_member_call(
                        &ty,
                        object,
                        field,
                        receiver_place,
                        args,
                    );
                }
                self.compile_opaque_member_call(&ty, object, field, receiver_place, args)
            }
            DirectType::Scalar(_) => {
                if field == "to_string" {
                    if !args.is_empty() {
                        return Err("direct backend expected `to_string()` to take no arguments"
                            .to_string());
                    }
                    let object = self.ensure_opaque(object)?;
                    let inst = self
                        .builder
                        .ins()
                        .call(self.stringify_value, &[object.values[0]]);
                    return Ok(self.owned_opaque_result(
                        self.builder.inst_results(inst).to_vec(),
                        Type::named("String"),
                    ));
                }
                let receiver_ty = direct_type_to_type(&object.ty);
                if self.find_trait_method(&receiver_ty, field).is_some() {
                    return self.compile_class_member_call(
                        &receiver_ty.to_string(),
                        Some(receiver_ty),
                        object,
                        field,
                        receiver_place,
                        args,
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
    ) -> std::result::Result<ValueRef, String> {
        let ty = ensure_direct_type(
            &Type::named(class_name),
            &self.classes,
            &format!("class `{}`", class_name),
        )?;
        match &ty {
            DirectType::PlainClass(class_ty) => {
                let mut by_name = HashMap::new();
                for field in fields {
                    by_name.insert(field.name.clone(), field.value.clone());
                }

                let mut values = Vec::new();
                for field in &class_ty.fields {
                    let operand = by_name.get(&field.name).ok_or_else(|| {
                        format!(
                            "direct backend construction for `{}` is missing field `{}`",
                            class_name, field.name
                        )
                    })?;
                    let value = self.load_operand(operand)?;
                    let coerced = self.coerce_value(value, &field.ty)?;
                    values.extend(coerced.values);
                }

                Ok(ValueRef {
                    values,
                    ty: ty.clone(),
                })
            }
            DirectType::Opaque(_) => self.compile_opaque_construct(class_name, fields),
            DirectType::Scalar(_) => Err(format!(
                "direct backend could not construct non-class type `{}`",
                class_name
            )),
        }
    }

    fn call_result_type(&self, name: &str) -> std::result::Result<DirectType, String> {
        self.function_return_types
            .get(name)
            .cloned()
            .ok_or_else(|| format!("direct backend does not know return type for `{}`", name))
    }

    fn type_of_place(&self, place: &str) -> std::result::Result<DirectType, String> {
        let mut segments = place.split('.');
        let root = segments
            .next()
            .ok_or_else(|| "direct backend encountered an empty place".to_string())?;
        let mut ty = self
            .variable_types
            .get(root)
            .cloned()
            .ok_or_else(|| format!("direct backend does not know local `{}`", root))?;
        for field in segments {
            ty = direct_field_type(&ty, field, &self.classes).ok_or_else(|| {
                format!(
                    "direct backend does not know field `{}` on `{}`",
                    field,
                    render_direct_type(&ty)
                )
            })?;
        }
        Ok(ty)
    }

    fn load_operand(&mut self, operand: &Operand) -> std::result::Result<ValueRef, String> {
        match operand {
            Operand::Place(place) => self.load_place(place),
            Operand::Int(value) => {
                if let Ok(narrowed) = i64::try_from(*value) {
                    return Ok(ValueRef {
                        values: vec![self.builder.ins().iconst(types::I64, narrowed)],
                        ty: DirectType::Scalar(ScalarKind::Int32),
                    });
                }
                let (ptr, len) = self.string_constant(value.to_string().as_bytes())?;
                let inst = self.builder.ins().call(self.box_uint_literal, &[ptr, len]);
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
                let inst = self.builder.ins().call(self.string_literal, &[ptr, len]);
                Ok(self.owned_opaque_result(
                    self.builder.inst_results(inst).to_vec(),
                    Type::named("String"),
                ))
            }
            Operand::Duration(value) => {
                let narrowed = i64::try_from(*value).map_err(|_| {
                    format!(
                        "direct backend only supports duration constants that fit in host i64, found `{}`",
                        value
                    )
                })?;
                let narrowed = self.builder.ins().iconst(types::I64, narrowed);
                let inst = self.builder.ins().call(self.duration_literal, &[narrowed]);
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

    fn load_place(&mut self, place: &str) -> std::result::Result<ValueRef, String> {
        let mut segments = place.split('.');
        let root = segments
            .next()
            .ok_or_else(|| "direct backend encountered an empty place".to_string())?;
        let mut value = self.load_root(root)?;
        for field in segments {
            value = self.extract_field(value, field)?;
        }
        Ok(value)
    }

    fn load_root(&mut self, name: &str) -> std::result::Result<ValueRef, String> {
        let vars = self
            .variables
            .get(name)
            .ok_or_else(|| format!("direct backend does not know local `{}`", name))?
            .clone();
        let ty = self
            .variable_types
            .get(name)
            .cloned()
            .ok_or_else(|| format!("direct backend does not know local type for `{}`", name))?;
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
                let (start, end, field_ty) = object.ty.field_slice(field).ok_or_else(|| {
                    format!(
                        "direct backend does not know field `{}` on `{}`",
                        field,
                        render_direct_type(&object.ty)
                    )
                })?;
                Ok(ValueRef {
                    values: object.values[start..end].to_vec(),
                    ty: field_ty,
                })
            }
            DirectType::Opaque(_) => {
                let (field_ptr, field_len) = self.string_constant(field.as_bytes())?;
                let inst = self.builder.ins().call(
                    self.instance_get_field,
                    &[object.values[0], field_ptr, field_len],
                );
                let loaded = ValueRef {
                    values: self.builder.inst_results(inst).to_vec(),
                    ty: DirectType::Opaque(Type::named("Unknown")),
                };
                self.mark_temporary_opaque_owned(&loaded);
                if let Some(field_ty) = direct_field_type(&object.ty, field, &self.classes) {
                    self.coerce_value(loaded, &field_ty)
                } else {
                    Ok(loaded)
                }
            }
            DirectType::Scalar(_) => Err(format!(
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
        if &value.ty == target {
            let value = self.normalize_scalar_value(value)?;
            if matches!(target.scalar_kind(), Some(ScalarKind::Int32)) {
                self.emit_int32_bounds_check(value.values[0], None);
            }
            return Ok(value);
        }

        if let DirectType::Opaque(target_ty) = target {
            if is_numeric_type_name(target_ty) {
                let boxed = self.ensure_opaque(value)?;
                let (target_ptr, target_len) =
                    self.string_constant(target_ty.to_string().as_bytes())?;
                let (line, column) = self.span_values(None);
                let inst = self.builder.ins().call(
                    self.cast_value,
                    &[boxed.values[0], target_ptr, target_len, line, column],
                );
                let casted = self.owned_opaque_result(
                    self.builder.inst_results(inst).to_vec(),
                    direct_type_to_type(target),
                );
                return Ok(casted);
            }
            return self.ensure_opaque(value);
        }

        if matches!(value.ty, DirectType::Opaque(_)) {
            let result = match target {
                DirectType::Scalar(ScalarKind::Int32) => {
                    let inst = self.builder.ins().call(self.unbox_i64, &[value.values[0]]);
                    ValueRef {
                        values: self.builder.inst_results(inst).to_vec(),
                        ty: target.clone(),
                    }
                }
                DirectType::Scalar(ScalarKind::Float32)
                | DirectType::Scalar(ScalarKind::Float64) => {
                    let inst = self.builder.ins().call(self.unbox_f64, &[value.values[0]]);
                    self.normalize_scalar_value(ValueRef {
                        values: self.builder.inst_results(inst).to_vec(),
                        ty: target.clone(),
                    })?
                }
                DirectType::Scalar(ScalarKind::Bool) => {
                    let inst = self.builder.ins().call(self.unbox_bool, &[value.values[0]]);
                    ValueRef {
                        values: self.builder.inst_results(inst).to_vec(),
                        ty: target.clone(),
                    }
                }
                DirectType::Scalar(ScalarKind::Unit) => unit_value(&mut self.builder),
                DirectType::PlainClass(class) => {
                    let mut values = Vec::new();
                    for field in &class.fields {
                        let (field_ptr, field_len) = self.string_constant(field.name.as_bytes())?;
                        let inst = self.builder.ins().call(
                            self.instance_get_field,
                            &[value.values[0], field_ptr, field_len],
                        );
                        let field_value = ValueRef {
                            values: self.builder.inst_results(inst).to_vec(),
                            ty: DirectType::Opaque(Type::named("Unknown")),
                        };
                        self.mark_temporary_opaque_owned(&field_value);
                        let coerced = self.coerce_value(field_value, &field.ty)?;
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
                self.emit_int32_bounds_check(result.values[0], None);
            }
            return Ok(result);
        }

        match (value.ty.scalar_kind(), target.scalar_kind()) {
            (Some(ScalarKind::Bool), Some(ScalarKind::Int32)) => Ok(ValueRef {
                values: value.values,
                ty: target.clone(),
            }),
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

    fn emit_int32_bounds_check(&mut self, value: Value, span: Option<Span>) {
        let min = self.builder.ins().iconst(types::I64, i32::MIN as i64);
        let max = self.builder.ins().iconst(types::I64, i32::MAX as i64);
        let below = self.builder.ins().icmp(IntCC::SignedLessThan, value, min);
        let above = self
            .builder
            .ins()
            .icmp(IntCC::SignedGreaterThan, value, max);
        let overflow = self.builder.ins().bor(below, above);
        let fail_block = self.builder.create_block();
        let continue_block = self.builder.create_block();
        self.builder
            .ins()
            .brif(overflow, fail_block, &[], continue_block, &[]);
        self.builder.switch_to_block(fail_block);
        let (line, column) = self.span_values(span);
        self.builder
            .ins()
            .call(self.fail_int32_overflow, &[value, line, column]);
        self.builder.ins().trap(TrapCode::INTEGER_OVERFLOW);
        self.builder.seal_block(fail_block);
        self.builder.switch_to_block(continue_block);
        self.builder.seal_block(continue_block);
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
                let inst = self
                    .builder
                    .ins()
                    .call(self.value_as_condition, &[value.values[0]]);
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

    fn store_place(&mut self, place: &str, value: ValueRef) -> std::result::Result<(), String> {
        let mut segments = place.split('.').collect::<Vec<_>>();
        let root = segments.remove(0);
        if segments.is_empty() {
            return self.store_root(root, value);
        }

        if matches!(self.variable_types.get(root), Some(DirectType::Opaque(_)))
            && segments.len() == 1
        {
            let current = self.load_root(root)?;
            let current = self.ensure_opaque(current)?;
            let updated_value = self.ensure_opaque(value)?;
            let (field_ptr, field_len) = self.string_constant(segments[0].as_bytes())?;
            let inst = self.builder.ins().call(
                self.instance_set_field,
                &[
                    current.values[0],
                    field_ptr,
                    field_len,
                    updated_value.values[0],
                ],
            );
            return self.store_root(
                root,
                ValueRef {
                    values: self.builder.inst_results(inst).to_vec(),
                    ty: self.variable_types.get(root).cloned().ok_or_else(|| {
                        format!("direct backend does not know local type for `{}`", root)
                    })?,
                },
            );
        }

        let root_value = self.load_root(root)?;
        let updated = self.replace_nested_field(root_value, &segments, value)?;
        self.store_root(root, updated)
    }

    fn replace_nested_field(
        &mut self,
        current: ValueRef,
        segments: &[&str],
        new_value: ValueRef,
    ) -> std::result::Result<ValueRef, String> {
        let (head, rest) = split_field_path_segments(segments)?;
        let (start, end, field_ty) = current.ty.field_slice(head).ok_or_else(|| {
            format!(
                "direct backend does not know field `{}` on `{}`",
                head,
                render_direct_type(&current.ty)
            )
        })?;

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
        let expected = self
            .variable_types
            .get(name)
            .cloned()
            .ok_or_else(|| format!("direct backend does not know local type for `{}`", name))?;
        let value = self.coerce_value(value, &expected)?;
        let vars = self
            .variables
            .get(name)
            .cloned()
            .ok_or_else(|| format!("direct backend does not know local `{}`", name))?;
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
        for (var, compiled) in vars.into_iter().zip(value.values.into_iter()) {
            self.builder.def_var(var, compiled);
        }
        Ok(())
    }

    fn ensure_opaque(&mut self, value: ValueRef) -> std::result::Result<ValueRef, String> {
        match value.ty {
            DirectType::Opaque(_) => Ok(value),
            DirectType::Scalar(ScalarKind::Int32) => {
                let inst = self.builder.ins().call(self.box_i64, &[value.values[0]]);
                let boxed = ValueRef {
                    values: self.builder.inst_results(inst).to_vec(),
                    ty: DirectType::Opaque(Type::named("int32")),
                };
                self.mark_temporary_opaque_owned(&boxed);
                Ok(boxed)
            }
            DirectType::Scalar(ScalarKind::Float32) | DirectType::Scalar(ScalarKind::Float64) => {
                let inst = self.builder.ins().call(self.box_f64, &[value.values[0]]);
                let boxed = ValueRef {
                    values: self.builder.inst_results(inst).to_vec(),
                    ty: DirectType::Opaque(Type::named("float64")),
                };
                self.mark_temporary_opaque_owned(&boxed);
                Ok(boxed)
            }
            DirectType::Scalar(ScalarKind::Bool) => {
                let inst = self.builder.ins().call(self.box_bool, &[value.values[0]]);
                let boxed = ValueRef {
                    values: self.builder.inst_results(inst).to_vec(),
                    ty: DirectType::Opaque(Type::named("bool")),
                };
                self.mark_temporary_opaque_owned(&boxed);
                Ok(boxed)
            }
            DirectType::Scalar(ScalarKind::Unit) => {
                let inst = self.builder.ins().call(self.box_unit, &[]);
                let boxed = ValueRef {
                    values: self.builder.inst_results(inst).to_vec(),
                    ty: DirectType::Opaque(Type::Unit),
                };
                self.mark_temporary_opaque_owned(&boxed);
                Ok(boxed)
            }
            DirectType::PlainClass(class) => {
                let (class_ptr, class_len) = self.string_constant(class.class_name.as_bytes())?;
                let init = self
                    .builder
                    .ins()
                    .call(self.instance_empty, &[class_ptr, class_len]);
                let mut current = self.builder.inst_results(init)[0];
                let mut start = 0usize;
                for field in &class.fields {
                    let end = start + field.ty.value_count();
                    let field_value = ValueRef {
                        values: value.values[start..end].to_vec(),
                        ty: field.ty.clone(),
                    };
                    let field_value = self.ensure_opaque(field_value)?;
                    let (field_ptr, field_len) = self.string_constant(field.name.as_bytes())?;
                    let inst = self.builder.ins().call(
                        self.instance_set_field,
                        &[current, field_ptr, field_len, field_value.values[0]],
                    );
                    current = self.builder.inst_results(inst)[0];
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

    fn binary_opcode(&self, op: BinaryOp) -> i64 {
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
        }
    }

    fn string_constant(&mut self, bytes: &[u8]) -> std::result::Result<(Value, Value), String> {
        declare_string_constant(self.object, self.string_data, &mut self.builder, bytes)
    }

    fn compile_enum_variant(
        &mut self,
        enum_name: &str,
        variant_name: &str,
        payloads: &[Operand],
    ) -> std::result::Result<ValueRef, String> {
        let (enum_ptr, enum_len) = self.string_constant(enum_name.as_bytes())?;
        let (variant_ptr, variant_len) = self.string_constant(variant_name.as_bytes())?;
        if payloads.len() > 1 {
            return Err(format!(
                "direct backend does not yet support enum variants with more than one payload (`{}.{}` has {})",
                enum_name,
                variant_name,
                payloads.len()
            ));
        }
        let payload = if let Some(payload) = payloads.first() {
            let loaded = self.load_operand(payload)?;
            self.ensure_opaque(loaded)?.values[0]
        } else {
            self.builder.ins().iconst(types::I64, 0)
        };
        let inst = self.builder.ins().call(
            self.enum_variant,
            &[enum_ptr, enum_len, variant_ptr, variant_len, payload],
        );
        Ok(self.owned_opaque_result(
            self.builder.inst_results(inst).to_vec(),
            Type::named(enum_name),
        ))
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
            self.variant_matches,
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
        let inst = self
            .builder
            .ins()
            .call(self.variant_payload, &[scrutinee.values[0], index]);
        let payload = ValueRef {
            values: self.builder.inst_results(inst).to_vec(),
            ty: DirectType::Opaque(Type::named("Unknown")),
        };
        self.mark_temporary_opaque_owned(&payload);
        Ok(payload)
    }

    fn compile_try_assign(
        &mut self,
        target: &str,
        target_ty: DirectType,
        try_value: &Operand,
    ) -> std::result::Result<(), String> {
        let loaded = self.load_operand(try_value)?;
        let value = self.ensure_opaque(loaded)?;
        let ok = self.variant_matches_value(value.values[0], "Result", "Ok")?;
        let ok_block = self.builder.create_block();
        let err_block = self.builder.create_block();
        let join_block = self.builder.create_block();
        self.builder.ins().brif(ok, ok_block, &[], err_block, &[]);

        self.builder.switch_to_block(ok_block);
        let payload = self.compile_variant_payload(value.clone(), 0)?;
        let coerced = self.coerce_value(payload, &target_ty)?;
        self.store_place(target, coerced)?;
        self.release_all_temporary_owned();
        self.builder.ins().jump(join_block, &[]);
        self.builder.seal_block(ok_block);

        self.builder.switch_to_block(err_block);
        self.emit_pending_cleanups(true)?;
        let return_values = self.build_return_values(value.clone())?;
        self.builder.ins().return_(&return_values);
        self.builder.seal_block(err_block);

        self.builder.switch_to_block(join_block);
        self.builder.seal_block(join_block);
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

    fn emit_pending_cleanups(
        &mut self,
        cancel_before_cleanup: bool,
    ) -> std::result::Result<(), String> {
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
            self.emit_cleanup_for_place(&place, cancel_before_cleanup)?;
            self.builder.def_var(variable, zero);
            self.builder.ins().jump(next_block, &[]);
            self.builder.seal_block(run_block);
            self.builder.switch_to_block(next_block);
            self.builder.seal_block(next_block);
        }
        Ok(())
    }

    fn build_return_values(
        &mut self,
        primary: ValueRef,
    ) -> std::result::Result<Vec<Value>, String> {
        let mut values = self.export_return_value(primary);
        for (name, ty) in self.writeback_locals.clone() {
            let current = self.load_root(&name)?;
            let coerced = self.coerce_value(current, &ty)?;
            values.extend(self.export_return_value(coerced));
        }
        Ok(values)
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
            if matches!(
                writebacks.last().map(|value| &value.ty),
                Some(DirectType::Opaque(_))
            ) {
                let value = writebacks.last().expect("just pushed writeback");
                self.mark_temporary_opaque_owned(value);
            }
            cursor += count;
        }
        let result = ValueRef {
            values: results[..result_count].to_vec(),
            ty: result_ty,
        };
        if matches!(result.ty, DirectType::Opaque(_)) {
            self.mark_temporary_opaque_owned(&result);
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
        for (place, value) in places.iter().zip(values.into_iter()) {
            self.store_place(place, value)?;
        }
        Ok(())
    }

    fn emit_cleanup_for_place(
        &mut self,
        place: &str,
        cancel_before_cleanup: bool,
    ) -> std::result::Result<(), String> {
        let receiver_ty = self.type_of_place(place)?;
        match &receiver_ty {
            DirectType::PlainClass(class_ty) => {
                let has_close = self
                    .classes
                    .get(&class_ty.class_name)
                    .and_then(|class| class.methods.iter().find(|method| method.name == "close"))
                    .is_some();
                if has_close {
                    let operand = Operand::Place(place.to_string());
                    let _ = self.compile_member_call(&operand, "close", Some(place), &[])?;
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
                    let _ = self
                        .builder
                        .ins()
                        .call(self.task_group_close, &[loaded.values[0], cancel_before]);
                    return Ok(());
                }
                if self
                    .compile_opaque_member_call(ty, loaded, "close", Some(place), &[])
                    .is_ok()
                {
                    return Ok(());
                }
            }
            DirectType::Scalar(_) => {}
        }
        Ok(())
    }

    fn compile_class_member_call(
        &mut self,
        class_name: &str,
        receiver_type_hint: Option<Type>,
        object: ValueRef,
        field: &str,
        receiver_place: Option<&str>,
        args: &[MirArg],
    ) -> std::result::Result<ValueRef, String> {
        let method = find_method(self.classes.get(class_name), field)
            .cloned()
            .or_else(|| {
                receiver_type_hint
                    .as_ref()
                    .and_then(|ty| self.find_trait_method(ty, field).cloned())
            })
            .or_else(|| {
                self.find_trait_method(&Type::named(class_name), field)
                    .cloned()
            })
            .or_else(|| {
                self.find_trait_method_for_class_name(class_name, field)
                    .cloned()
            })
            .ok_or_else(|| {
                format!(
                    "direct backend does not know method `{}.{}`",
                    class_name, field
                )
            })?;
        let method_function_name = method.function_name.clone();
        if method.receiver == Some(MirReceiverKind::BorrowMut) && receiver_place.is_none() {
            return Err(format!(
                "direct backend does not yet support temporary mutable receiver method `{}.{}`",
                class_name, field
            ));
        }
        let func_ref = *self
            .function_refs
            .get(&method_function_name)
            .ok_or_else(|| {
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
        let receiver_expected = expected
            .first()
            .cloned()
            .unwrap_or_else(|| object.ty.clone());
        let receiver = self.coerce_value(object.clone(), &receiver_expected)?;
        if matches!(receiver.ty, DirectType::Opaque(_)) {
            lowered_args.push(self.transfer_opaque_arg(&receiver));
        } else {
            lowered_args.extend(receiver.values);
        }
        if method.receiver == Some(MirReceiverKind::BorrowMut) {
            let Some(place) = receiver_place else {
                return Err(format!(
                    "direct backend does not yet support temporary mutable receiver method `{}.{}`",
                    class_name, field
                ));
            };
            writeback_places.push(place.to_string());
        }
        for (index, argument) in args.iter().enumerate() {
            let loaded = self.load_operand(&argument.value)?;
            let coerced = if let Some(expected_ty) = expected.get(index + 1) {
                self.coerce_value(loaded, expected_ty)?
            } else {
                loaded
            };
            if let Some(place) = &argument.writeback_place {
                writeback_places.push(place.clone());
            }
            if matches!(coerced.ty, DirectType::Opaque(_)) {
                lowered_args.push(self.transfer_opaque_arg(&coerced));
            } else {
                lowered_args.extend(coerced.values);
            }
        }
        let inst = self.builder.ins().call(func_ref, &lowered_args);
        let results = self.builder.inst_results(inst).to_vec();
        let (result, writebacks) = self.split_call_results(&method_function_name, results)?;
        self.apply_writeback_places(&writeback_places, writebacks)?;
        Ok(result)
    }

    fn compile_opaque_member_call(
        &mut self,
        object_ty: &Type,
        object: ValueRef,
        field: &str,
        receiver_place: Option<&str>,
        args: &[MirArg],
    ) -> std::result::Result<ValueRef, String> {
        if field == "to_string" {
            if !args.is_empty() {
                return Err(
                    "direct backend expected `to_string()` to take no arguments".to_string()
                );
            }
            let object = self.ensure_opaque(object)?;
            let inst = self
                .builder
                .ins()
                .call(self.stringify_value, &[object.values[0]]);
            return Ok(self.owned_opaque_result(
                self.builder.inst_results(inst).to_vec(),
                Type::named("String"),
            ));
        }
        if field == "clone" {
            if !args.is_empty() {
                return Err("direct backend expected `clone()` to take no arguments".to_string());
            }
            let object = self.ensure_opaque(object)?;
            let inst = self
                .builder
                .ins()
                .call(self.clone_value, &[object.values[0]]);
            return Ok(self
                .owned_opaque_result(self.builder.inst_results(inst).to_vec(), object_ty.clone()));
        }
        if let Type::Named(name, class_args) = object_ty {
            if name == "String" {
                let object = self.ensure_opaque(object)?;
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
                            .call(self.string_len, &[object.values[0]]);
                        let len = self.builder.inst_results(inst)[0];
                        self.emit_int32_bounds_check(len, None);
                        Ok(ValueRef {
                            values: vec![len],
                            ty: DirectType::Scalar(ScalarKind::Int32),
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
                            "contains" => self.string_contains,
                            "starts_with" => self.string_starts_with,
                            "ends_with" => self.string_ends_with,
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
                        let inst = self
                            .builder
                            .ins()
                            .call(self.string_split, &[object.values[0], value.values[0]]);
                        Ok(self.owned_opaque_result(
                            self.builder.inst_results(inst).to_vec(),
                            Type::Named("Vec".to_string(), vec![Type::named("String")]),
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
                            self.string_replace,
                            &[object.values[0], from.values[0], to.values[0]],
                        );
                        Ok(self.owned_opaque_result(
                            self.builder.inst_results(inst).to_vec(),
                            Type::named("String"),
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
                        let binary_opcode = self.binary_opcode(BinaryOp::Add);
                        let opcode = self.builder.ins().iconst(types::I64, binary_opcode);
                        let zero = self.builder.ins().iconst(types::I64, 0);
                        let inst = self.builder.ins().call(
                            self.binary_value,
                            &[opcode, object.values[0], value.values[0], zero, zero],
                        );
                        Ok(self.owned_opaque_result(
                            self.builder.inst_results(inst).to_vec(),
                            Type::named("String"),
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
                            .call(self.string_to_lower, &[object.values[0]]);
                        Ok(self.owned_opaque_result(
                            self.builder.inst_results(inst).to_vec(),
                            Type::named("String"),
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
                            .call(self.string_to_upper, &[object.values[0]]);
                        Ok(self.owned_opaque_result(
                            self.builder.inst_results(inst).to_vec(),
                            Type::named("String"),
                        ))
                    }
                    "join" => {
                        let [argument] = args else {
                            return Err(
                                "direct backend expected `join()` to receive one vector argument"
                                    .to_string(),
                            );
                        };
                        let loaded_value = self.load_operand(&argument.value)?;
                        let value = self.ensure_opaque(loaded_value)?;
                        let inst = self
                            .builder
                            .ins()
                            .call(self.string_join, &[object.values[0], value.values[0]]);
                        Ok(self.owned_opaque_result(
                            self.builder.inst_results(inst).to_vec(),
                            Type::named("String"),
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
                            "strip_prefix" => self.string_strip_prefix,
                            "strip_suffix" => self.string_strip_suffix,
                            _ => unreachable!(),
                        };
                        let inst = self
                            .builder
                            .ins()
                            .call(func, &[object.values[0], value.values[0]]);
                        Ok(self.owned_opaque_result(
                            self.builder.inst_results(inst).to_vec(),
                            Type::Named("Option".to_string(), vec![Type::named("String")]),
                        ))
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
                            .call(self.string_trim, &[object.values[0]]);
                        Ok(self.owned_opaque_result(
                            self.builder.inst_results(inst).to_vec(),
                            Type::named("String"),
                        ))
                    }
                    _ => Err(format!(
                        "direct backend does not know runtime member `{}.{}`",
                        name, field
                    )),
                };
            }
            if name == "Vec" {
                let object = self.ensure_opaque(object)?;
                let element_ty = class_args
                    .first()
                    .cloned()
                    .unwrap_or_else(|| Type::named("Unknown"));
                let element_direct_ty =
                    ensure_direct_type(&element_ty, &self.classes, "Vec element")?;
                return match field {
                    "len" => {
                        if !args.is_empty() {
                            return Err(
                                "direct backend expected `len()` to take no arguments".to_string()
                            );
                        }
                        let inst = self.builder.ins().call(self.vec_len, &[object.values[0]]);
                        let len = self.builder.inst_results(inst)[0];
                        self.emit_int32_bounds_check(len, None);
                        Ok(ValueRef {
                            values: vec![len],
                            ty: DirectType::Scalar(ScalarKind::Int32),
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
                            .call(self.vec_is_empty, &[object.values[0]]);
                        Ok(ValueRef {
                            values: self.builder.inst_results(inst).to_vec(),
                            ty: DirectType::Scalar(ScalarKind::Bool),
                        })
                    }
                    "push" => {
                        let [argument] = args else {
                            return Err("direct backend expected `push()` to receive one argument"
                                .to_string());
                        };
                        let loaded_value = self.load_operand(&argument.value)?;
                        let value = self.ensure_opaque(loaded_value)?;
                        let _ = self
                            .builder
                            .ins()
                            .call(self.vec_push_in_place, &[object.values[0], value.values[0]]);
                        if let Some(place) = receiver_place {
                            self.store_place(place, object.clone())?;
                        }
                        Ok(unit_value(&mut self.builder))
                    }
                    "pop" => {
                        if !args.is_empty() {
                            return Err(
                                "direct backend expected `pop()` to take no arguments".to_string()
                            );
                        }
                        let inst = self
                            .builder
                            .ins()
                            .call(self.vec_pop_in_place, &[object.values[0]]);
                        if let Some(place) = receiver_place {
                            self.store_place(place, object.clone())?;
                        }
                        Ok(self.owned_opaque_result(
                            self.builder.inst_results(inst).to_vec(),
                            Type::Named("Option".to_string(), vec![element_ty]),
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
                            .coerce_value(loaded_index, &DirectType::Scalar(ScalarKind::Int32))?;
                        let inst = self
                            .builder
                            .ins()
                            .call(self.vec_get, &[object.values[0], index.values[0]]);
                        Ok(self.owned_opaque_result(
                            self.builder.inst_results(inst).to_vec(),
                            Type::Named(
                                "Option".to_string(),
                                vec![class_args
                                    .first()
                                    .cloned()
                                    .unwrap_or_else(|| Type::named("Unknown"))],
                            ),
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
                            .coerce_value(loaded_index, &DirectType::Scalar(ScalarKind::Int32))?;
                        let inst = self
                            .builder
                            .ins()
                            .call(self.vec_index_option, &[object.values[0], index.values[0]]);
                        Ok(self.owned_opaque_result(
                            self.builder.inst_results(inst).to_vec(),
                            Type::Named(
                                "Option".to_string(),
                                vec![class_args
                                    .first()
                                    .cloned()
                                    .unwrap_or_else(|| Type::named("Unknown"))],
                            ),
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
                            .coerce_value(loaded_index, &DirectType::Scalar(ScalarKind::Int32))?;
                        let loaded_line = self.load_operand(&line_arg.value)?;
                        let line =
                            self.coerce_value(loaded_line, &DirectType::Scalar(ScalarKind::Int32))?;
                        let loaded_column = self.load_operand(&column_arg.value)?;
                        let column = self
                            .coerce_value(loaded_column, &DirectType::Scalar(ScalarKind::Int32))?;
                        let inst = self.builder.ins().call(
                            self.vec_index,
                            &[
                                object.values[0],
                                index.values[0],
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
                            .coerce_value(loaded_index, &DirectType::Scalar(ScalarKind::Int32))?;
                        let loaded_value = self.load_operand(&value_arg.value)?;
                        let value = self.ensure_opaque(loaded_value)?;
                        let inst = self.builder.ins().call(
                            self.vec_set_in_place,
                            &[object.values[0], index.values[0], value.values[0]],
                        );
                        if let Some(place) = receiver_place {
                            self.store_place(place, object.clone())?;
                        }
                        Ok(self.owned_opaque_result(
                            self.builder.inst_results(inst).to_vec(),
                            Type::Named(
                                "Option".to_string(),
                                vec![class_args
                                    .first()
                                    .cloned()
                                    .unwrap_or_else(|| Type::named("Unknown"))],
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
                            .coerce_value(loaded_index, &DirectType::Scalar(ScalarKind::Int32))?;
                        let loaded_value = self.load_operand(&value_arg.value)?;
                        let value = self.ensure_opaque(loaded_value)?;
                        let loaded_line = self.load_operand(&line_arg.value)?;
                        let line =
                            self.coerce_value(loaded_line, &DirectType::Scalar(ScalarKind::Int32))?;
                        let loaded_column = self.load_operand(&column_arg.value)?;
                        let column = self
                            .coerce_value(loaded_column, &DirectType::Scalar(ScalarKind::Int32))?;
                        let _ = self.builder.ins().call(
                            self.vec_set_index_in_place,
                            &[
                                object.values[0],
                                index.values[0],
                                value.values[0],
                                line.values[0],
                                column.values[0],
                            ],
                        );
                        if let Some(place) = receiver_place {
                            self.store_place(place, object.clone())?;
                        }
                        Ok(unit_value(&mut self.builder))
                    }
                    "remove" => {
                        let [argument] = args else {
                            return Err(
                                "direct backend expected `remove()` to receive one index argument"
                                    .to_string(),
                            );
                        };
                        let loaded_index = self.load_operand(&argument.value)?;
                        let index = self
                            .coerce_value(loaded_index, &DirectType::Scalar(ScalarKind::Int32))?;
                        let inst = self.builder.ins().call(
                            self.vec_remove_in_place,
                            &[object.values[0], index.values[0]],
                        );
                        if let Some(place) = receiver_place {
                            self.store_place(place, object.clone())?;
                        }
                        Ok(self.owned_opaque_result(
                            self.builder.inst_results(inst).to_vec(),
                            Type::Named(
                                "Option".to_string(),
                                vec![class_args
                                    .first()
                                    .cloned()
                                    .unwrap_or_else(|| Type::named("Unknown"))],
                            ),
                        ))
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
                            .coerce_value(loaded_first, &DirectType::Scalar(ScalarKind::Int32))?;
                        let loaded_second = self.load_operand(&second_arg.value)?;
                        let second = self
                            .coerce_value(loaded_second, &DirectType::Scalar(ScalarKind::Int32))?;
                        let inst = self.builder.ins().call(
                            self.vec_swap_in_place,
                            &[object.values[0], first.values[0], second.values[0]],
                        );
                        if let Some(place) = receiver_place {
                            self.store_place(place, object.clone())?;
                        }
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
                        let loaded_value = self.load_operand(&argument.value)?;
                        let value = self.ensure_opaque(loaded_value)?;
                        let inst = self
                            .builder
                            .ins()
                            .call(self.vec_contains, &[object.values[0], value.values[0]]);
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
                            .coerce_value(loaded_index, &DirectType::Scalar(ScalarKind::Int32))?;
                        let loaded_value = self.load_operand(&value_arg.value)?;
                        let value = self.ensure_opaque(loaded_value)?;
                        let inst = self.builder.ins().call(
                            self.vec_insert_in_place,
                            &[object.values[0], index.values[0], value.values[0]],
                        );
                        if let Some(place) = receiver_place {
                            self.store_place(place, object.clone())?;
                        }
                        Ok(ValueRef {
                            values: self.builder.inst_results(inst).to_vec(),
                            ty: DirectType::Scalar(ScalarKind::Bool),
                        })
                    }
                    "clear" => {
                        if !args.is_empty() {
                            return Err("direct backend expected `clear()` to take no arguments"
                                .to_string());
                        }
                        let _ = self
                            .builder
                            .ins()
                            .call(self.vec_clear_in_place, &[object.values[0]]);
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
                        let _ = self
                            .builder
                            .ins()
                            .call(self.vec_reverse_in_place, &[object.values[0]]);
                        if let Some(place) = receiver_place {
                            self.store_place(place, object.clone())?;
                        }
                        Ok(unit_value(&mut self.builder))
                    }
                    "extend" => {
                        let [argument] = args else {
                            return Err(
                                "direct backend expected `extend()` to receive one vector argument"
                                    .to_string(),
                            );
                        };
                        let loaded_value = self.load_operand(&argument.value)?;
                        let value = self.ensure_opaque(loaded_value)?;
                        let _ = self.builder.ins().call(
                            self.vec_extend_in_place,
                            &[object.values[0], value.values[0]],
                        );
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
            if name == "Map" {
                let object = self.ensure_opaque(object)?;
                let key_ty = class_args
                    .first()
                    .cloned()
                    .unwrap_or_else(|| Type::named("Unknown"));
                let value_ty = class_args
                    .get(1)
                    .cloned()
                    .unwrap_or_else(|| Type::named("Unknown"));
                let value_direct_ty = ensure_direct_type(&value_ty, &self.classes, "Map value")?;
                return match field {
                    "len" => {
                        if !args.is_empty() {
                            return Err(
                                "direct backend expected `len()` to take no arguments".to_string()
                            );
                        }
                        let inst = self.builder.ins().call(self.map_len, &[object.values[0]]);
                        let len = self.builder.inst_results(inst)[0];
                        self.emit_int32_bounds_check(len, None);
                        Ok(ValueRef {
                            values: vec![len],
                            ty: DirectType::Scalar(ScalarKind::Int32),
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
                            .call(self.map_is_empty, &[object.values[0]]);
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
                        let loaded_key = self.load_operand(&argument.value)?;
                        let key = self.ensure_opaque(loaded_key)?;
                        let inst = self
                            .builder
                            .ins()
                            .call(self.map_get, &[object.values[0], key.values[0]]);
                        Ok(self.owned_opaque_result(
                            self.builder.inst_results(inst).to_vec(),
                            Type::Named("Option".to_string(), vec![value_ty.clone()]),
                        ))
                    }
                    "__index" => {
                        let [key_arg, line_arg, column_arg] = args else {
                            return Err(
                                "direct backend expected internal map indexing to receive key, line, and column"
                                    .to_string(),
                            );
                        };
                        let loaded_key = self.load_operand(&key_arg.value)?;
                        let key = self.ensure_opaque(loaded_key)?;
                        let loaded_line = self.load_operand(&line_arg.value)?;
                        let line =
                            self.coerce_value(loaded_line, &DirectType::Scalar(ScalarKind::Int32))?;
                        let loaded_column = self.load_operand(&column_arg.value)?;
                        let column = self
                            .coerce_value(loaded_column, &DirectType::Scalar(ScalarKind::Int32))?;
                        let inst = self.builder.ins().call(
                            self.map_index,
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
                        let loaded_key = self.load_operand(&key_arg.value)?;
                        let key = self.ensure_opaque(loaded_key)?;
                        let loaded_value = self.load_operand(&value_arg.value)?;
                        let value = self.ensure_opaque(loaded_value)?;
                        let inst = self.builder.ins().call(
                            self.map_set_in_place,
                            &[object.values[0], key.values[0], value.values[0]],
                        );
                        if let Some(place) = receiver_place {
                            self.store_place(place, object.clone())?;
                        }
                        Ok(self.owned_opaque_result(
                            self.builder.inst_results(inst).to_vec(),
                            Type::Named("Option".to_string(), vec![value_ty.clone()]),
                        ))
                    }
                    "__set_index" => {
                        let [key_arg, value_arg, line_arg, column_arg] = args else {
                            return Err(
                                "direct backend expected internal map indexed assignment to receive key, value, line, and column"
                                    .to_string(),
                            );
                        };
                        let loaded_key = self.load_operand(&key_arg.value)?;
                        let key = self.ensure_opaque(loaded_key)?;
                        let loaded_value = self.load_operand(&value_arg.value)?;
                        let value = self.ensure_opaque(loaded_value)?;
                        let loaded_line = self.load_operand(&line_arg.value)?;
                        let line =
                            self.coerce_value(loaded_line, &DirectType::Scalar(ScalarKind::Int32))?;
                        let loaded_column = self.load_operand(&column_arg.value)?;
                        let column = self
                            .coerce_value(loaded_column, &DirectType::Scalar(ScalarKind::Int32))?;
                        let _ = self.builder.ins().call(
                            self.map_set_index_in_place,
                            &[
                                object.values[0],
                                key.values[0],
                                value.values[0],
                                line.values[0],
                                column.values[0],
                            ],
                        );
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
                        let loaded_key = self.load_operand(&argument.value)?;
                        let key = self.ensure_opaque(loaded_key)?;
                        let inst = self
                            .builder
                            .ins()
                            .call(self.map_remove_in_place, &[object.values[0], key.values[0]]);
                        if let Some(place) = receiver_place {
                            self.store_place(place, object.clone())?;
                        }
                        Ok(self.owned_opaque_result(
                            self.builder.inst_results(inst).to_vec(),
                            Type::Named("Option".to_string(), vec![value_ty.clone()]),
                        ))
                    }
                    "contains_key" => {
                        let [argument] = args else {
                            return Err(
                                "direct backend expected `contains_key()` to receive one key argument"
                                    .to_string(),
                            );
                        };
                        let loaded_key = self.load_operand(&argument.value)?;
                        let key = self.ensure_opaque(loaded_key)?;
                        let inst = self
                            .builder
                            .ins()
                            .call(self.map_contains_key, &[object.values[0], key.values[0]]);
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
                        let inst = self.builder.ins().call(self.map_keys, &[object.values[0]]);
                        Ok(self.owned_opaque_result(
                            self.builder.inst_results(inst).to_vec(),
                            Type::Named("Vec".to_string(), vec![key_ty.clone()]),
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
                            .call(self.map_values, &[object.values[0]]);
                        Ok(self.owned_opaque_result(
                            self.builder.inst_results(inst).to_vec(),
                            Type::Named("Vec".to_string(), vec![value_ty.clone()]),
                        ))
                    }
                    "items" | "entries" => {
                        if !args.is_empty() {
                            return Err(format!(
                                "direct backend expected `{}`() to take no arguments",
                                field
                            ));
                        }
                        let func = if field == "items" {
                            self.map_items
                        } else {
                            self.map_entries
                        };
                        let inst = self.builder.ins().call(func, &[object.values[0]]);
                        Ok(self.owned_opaque_result(
                            self.builder.inst_results(inst).to_vec(),
                            Type::Named(
                                "Vec".to_string(),
                                vec![Type::Named(
                                    "MapEntry".to_string(),
                                    vec![key_ty.clone(), value_ty.clone()],
                                )],
                            ),
                        ))
                    }
                    "clear" => {
                        if !args.is_empty() {
                            return Err("direct backend expected `clear()` to take no arguments"
                                .to_string());
                        }
                        let _ = self
                            .builder
                            .ins()
                            .call(self.map_clear_in_place, &[object.values[0]]);
                        if let Some(place) = receiver_place {
                            self.store_place(place, object.clone())?;
                        }
                        Ok(unit_value(&mut self.builder))
                    }
                    "extend" => {
                        let [argument] = args else {
                            return Err(
                                "direct backend expected `extend()` to receive one map argument"
                                    .to_string(),
                            );
                        };
                        let loaded_value = self.load_operand(&argument.value)?;
                        let value = self.ensure_opaque(loaded_value)?;
                        let _ = self.builder.ins().call(
                            self.map_extend_in_place,
                            &[object.values[0], value.values[0]],
                        );
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
            if name == "Set" {
                let object = self.ensure_opaque(object)?;
                let element_ty = class_args
                    .first()
                    .cloned()
                    .unwrap_or_else(|| Type::named("Unknown"));
                return match field {
                    "len" => {
                        if !args.is_empty() {
                            return Err(
                                "direct backend expected `len()` to take no arguments".to_string()
                            );
                        }
                        let inst = self.builder.ins().call(self.set_len, &[object.values[0]]);
                        let len = self.builder.inst_results(inst)[0];
                        self.emit_int32_bounds_check(len, None);
                        Ok(ValueRef {
                            values: vec![len],
                            ty: DirectType::Scalar(ScalarKind::Int32),
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
                            .call(self.set_is_empty, &[object.values[0]]);
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
                        let loaded_value = self.load_operand(&argument.value)?;
                        let value = self.ensure_opaque(loaded_value)?;
                        let inst = self
                            .builder
                            .ins()
                            .call(self.set_contains, &[object.values[0], value.values[0]]);
                        Ok(ValueRef {
                            values: self.builder.inst_results(inst).to_vec(),
                            ty: DirectType::Scalar(ScalarKind::Bool),
                        })
                    }
                    "insert" => {
                        let [argument] = args else {
                            return Err(
                                "direct backend expected `insert()` to receive one value argument"
                                    .to_string(),
                            );
                        };
                        let loaded_value = self.load_operand(&argument.value)?;
                        let value = self.ensure_opaque(loaded_value)?;
                        let inst = self.builder.ins().call(
                            self.set_insert_in_place,
                            &[object.values[0], value.values[0]],
                        );
                        if let Some(place) = receiver_place {
                            self.store_place(place, object.clone())?;
                        }
                        Ok(ValueRef {
                            values: self.builder.inst_results(inst).to_vec(),
                            ty: DirectType::Scalar(ScalarKind::Bool),
                        })
                    }
                    "remove" => {
                        let [argument] = args else {
                            return Err(
                                "direct backend expected `remove()` to receive one value argument"
                                    .to_string(),
                            );
                        };
                        let loaded_value = self.load_operand(&argument.value)?;
                        let value = self.ensure_opaque(loaded_value)?;
                        let inst = self.builder.ins().call(
                            self.set_remove_in_place,
                            &[object.values[0], value.values[0]],
                        );
                        if let Some(place) = receiver_place {
                            self.store_place(place, object.clone())?;
                        }
                        Ok(ValueRef {
                            values: self.builder.inst_results(inst).to_vec(),
                            ty: DirectType::Scalar(ScalarKind::Bool),
                        })
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
                            .coerce_value(loaded_index, &DirectType::Scalar(ScalarKind::Int32))?;
                        let inst = self
                            .builder
                            .ins()
                            .call(self.set_index_option, &[object.values[0], index.values[0]]);
                        Ok(self.owned_opaque_result(
                            self.builder.inst_results(inst).to_vec(),
                            Type::Named("Option".to_string(), vec![element_ty]),
                        ))
                    }
                    _ => Err(format!(
                        "direct backend does not know runtime member `{}.{}`",
                        name, field
                    )),
                };
            }
            if name == "fs.File" {
                let object = self.ensure_opaque(object)?;
                return match field {
                    "read_all" => {
                        if !args.is_empty() {
                            return Err(
                                "direct backend expected `read_all()` to take no arguments"
                                    .to_string(),
                            );
                        }
                        let inst = self
                            .builder
                            .ins()
                            .call(self.file_read_all, &[object.values[0]]);
                        Ok(self.owned_opaque_result(
                            self.builder.inst_results(inst).to_vec(),
                            Type::Named(
                                "Result".to_string(),
                                vec![
                                    Type::named("String"),
                                    Type::Named("io.Error".to_string(), Vec::new()),
                                ],
                            ),
                        ))
                    }
                    "read_bytes" => {
                        if !args.is_empty() {
                            return Err(
                                "direct backend expected `read_bytes()` to take no arguments"
                                    .to_string(),
                            );
                        }
                        let inst = self
                            .builder
                            .ins()
                            .call(self.file_read_bytes, &[object.values[0]]);
                        Ok(self.owned_opaque_result(
                            self.builder.inst_results(inst).to_vec(),
                            Type::Named(
                                "Result".to_string(),
                                vec![
                                    Type::Named("Vec".to_string(), vec![Type::named("uint8")]),
                                    Type::Named("io.Error".to_string(), Vec::new()),
                                ],
                            ),
                        ))
                    }
                    "write_all" => {
                        let [argument] = args else {
                            return Err(
                                "direct backend expected `write_all()` to receive one argument"
                                    .to_string(),
                            );
                        };
                        let loaded = self.load_operand(&argument.value)?;
                        let text = self.ensure_opaque(loaded)?;
                        let inst = self
                            .builder
                            .ins()
                            .call(self.file_write_all, &[object.values[0], text.values[0]]);
                        Ok(self.owned_opaque_result(
                            self.builder.inst_results(inst).to_vec(),
                            Type::Named(
                                "Result".to_string(),
                                vec![Type::Unit, Type::Named("io.Error".to_string(), Vec::new())],
                            ),
                        ))
                    }
                    "write_bytes" => {
                        let [argument] = args else {
                            return Err(
                                "direct backend expected `write_bytes()` to receive one argument"
                                    .to_string(),
                            );
                        };
                        let loaded = self.load_operand(&argument.value)?;
                        let bytes = self.ensure_opaque(loaded)?;
                        let inst = self
                            .builder
                            .ins()
                            .call(self.file_write_bytes, &[object.values[0], bytes.values[0]]);
                        Ok(self.owned_opaque_result(
                            self.builder.inst_results(inst).to_vec(),
                            Type::Named(
                                "Result".to_string(),
                                vec![Type::Unit, Type::Named("io.Error".to_string(), Vec::new())],
                            ),
                        ))
                    }
                    "flush" => {
                        if !args.is_empty() {
                            return Err("direct backend expected `flush()` to take no arguments"
                                .to_string());
                        }
                        let inst = self
                            .builder
                            .ins()
                            .call(self.file_flush, &[object.values[0]]);
                        Ok(self.owned_opaque_result(
                            self.builder.inst_results(inst).to_vec(),
                            Type::Named(
                                "Result".to_string(),
                                vec![Type::Unit, Type::Named("io.Error".to_string(), Vec::new())],
                            ),
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
                            .call(self.file_close, &[object.values[0]]);
                        Ok(ValueRef {
                            values: self.builder.inst_results(inst).to_vec(),
                            ty: DirectType::Scalar(ScalarKind::Unit),
                        })
                    }
                    _ => Err(format!(
                        "direct backend does not know runtime member `{}.{}`",
                        name, field
                    )),
                };
            }
            if name == "net.TcpListener" {
                let object = self.ensure_opaque(object)?;
                return match field {
                    "accept" => {
                        let bound = ordered_optional_named_args(&["timeout"], args)?;
                        let timeout = self.lower_optional_opaque_arg(bound[0])?;
                        let inst = self
                            .builder
                            .ins()
                            .call(self.tcp_listener_accept, &[object.values[0], timeout]);
                        Ok(self.owned_opaque_result(
                            self.builder.inst_results(inst).to_vec(),
                            Type::Named(
                                "Result".to_string(),
                                vec![
                                    Type::Named("net.TcpStream".to_string(), Vec::new()),
                                    Type::Named("io.Error".to_string(), Vec::new()),
                                ],
                            ),
                        ))
                    }
                    "local_addr" => {
                        if !args.is_empty() {
                            return Err(
                                "direct backend expected `local_addr()` to take no arguments"
                                    .to_string(),
                            );
                        }
                        let inst = self
                            .builder
                            .ins()
                            .call(self.tcp_listener_local_addr, &[object.values[0]]);
                        Ok(self.owned_opaque_result(
                            self.builder.inst_results(inst).to_vec(),
                            Type::Named(
                                "Result".to_string(),
                                vec![
                                    Type::named("String"),
                                    Type::Named("io.Error".to_string(), Vec::new()),
                                ],
                            ),
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
                            .call(self.tcp_listener_close, &[object.values[0]]);
                        Ok(ValueRef {
                            values: self.builder.inst_results(inst).to_vec(),
                            ty: DirectType::Scalar(ScalarKind::Unit),
                        })
                    }
                    _ => Err(format!(
                        "direct backend does not know runtime member `{}.{}`",
                        name, field
                    )),
                };
            }
            if name == "net.TcpStream" {
                let object = self.ensure_opaque(object)?;
                return match field {
                    "read_all" => {
                        let bound = ordered_optional_named_args(&["timeout"], args)?;
                        let timeout = self.lower_optional_opaque_arg(bound[0])?;
                        let inst = self
                            .builder
                            .ins()
                            .call(self.tcp_stream_read_all, &[object.values[0], timeout]);
                        Ok(self.owned_opaque_result(
                            self.builder.inst_results(inst).to_vec(),
                            Type::Named(
                                "Result".to_string(),
                                vec![
                                    Type::named("String"),
                                    Type::Named("io.Error".to_string(), Vec::new()),
                                ],
                            ),
                        ))
                    }
                    "read_line" => {
                        let bound = ordered_optional_named_args(&["timeout"], args)?;
                        let timeout = self.lower_optional_opaque_arg(bound[0])?;
                        let inst = self
                            .builder
                            .ins()
                            .call(self.tcp_stream_read_line, &[object.values[0], timeout]);
                        Ok(self.owned_opaque_result(
                            self.builder.inst_results(inst).to_vec(),
                            Type::Named(
                                "Result".to_string(),
                                vec![
                                    Type::Named("Option".to_string(), vec![Type::named("String")]),
                                    Type::Named("io.Error".to_string(), Vec::new()),
                                ],
                            ),
                        ))
                    }
                    "read_bytes" => {
                        let bound = ordered_optional_named_args(&["max_bytes", "timeout"], args)?;
                        let count = bound[0].ok_or_else(|| {
                            "direct backend expected `read_bytes()` to receive `max_bytes`"
                                .to_string()
                        })?;
                        let loaded_count = self.load_operand(&count.value)?;
                        let count = self
                            .coerce_value(loaded_count, &DirectType::Scalar(ScalarKind::Int32))?;
                        let count = self.ensure_opaque(count)?;
                        let timeout = self.lower_optional_opaque_arg(bound[1])?;
                        let inst = self.builder.ins().call(
                            self.tcp_stream_read_bytes,
                            &[object.values[0], count.values[0], timeout],
                        );
                        Ok(self.owned_opaque_result(
                            self.builder.inst_results(inst).to_vec(),
                            Type::Named(
                                "Result".to_string(),
                                vec![
                                    Type::Named(
                                        "Option".to_string(),
                                        vec![Type::Named(
                                            "Vec".to_string(),
                                            vec![Type::named("uint8")],
                                        )],
                                    ),
                                    Type::Named("io.Error".to_string(), Vec::new()),
                                ],
                            ),
                        ))
                    }
                    "read_exact" => {
                        let bound = ordered_optional_named_args(&["count", "timeout"], args)?;
                        let count = bound[0].ok_or_else(|| {
                            "direct backend expected `read_exact()` to receive `count`".to_string()
                        })?;
                        let loaded_count = self.load_operand(&count.value)?;
                        let count = self
                            .coerce_value(loaded_count, &DirectType::Scalar(ScalarKind::Int32))?;
                        let count = self.ensure_opaque(count)?;
                        let timeout = self.lower_optional_opaque_arg(bound[1])?;
                        let inst = self.builder.ins().call(
                            self.tcp_stream_read_exact,
                            &[object.values[0], count.values[0], timeout],
                        );
                        Ok(self.owned_opaque_result(
                            self.builder.inst_results(inst).to_vec(),
                            Type::Named(
                                "Result".to_string(),
                                vec![
                                    Type::Named("Vec".to_string(), vec![Type::named("uint8")]),
                                    Type::Named("io.Error".to_string(), Vec::new()),
                                ],
                            ),
                        ))
                    }
                    "write_all" => {
                        let bound = ordered_optional_named_args(&["text", "timeout"], args)?;
                        let argument = bound[0].ok_or_else(|| {
                            "direct backend expected `write_all()` to receive `text`".to_string()
                        })?;
                        let loaded = self.load_operand(&argument.value)?;
                        let text = self.ensure_opaque(loaded)?;
                        let timeout = self.lower_optional_opaque_arg(bound[1])?;
                        let inst = self.builder.ins().call(
                            self.tcp_stream_write_all,
                            &[object.values[0], text.values[0], timeout],
                        );
                        Ok(self.owned_opaque_result(
                            self.builder.inst_results(inst).to_vec(),
                            Type::Named(
                                "Result".to_string(),
                                vec![Type::Unit, Type::Named("io.Error".to_string(), Vec::new())],
                            ),
                        ))
                    }
                    "write_bytes" => {
                        let bound = ordered_optional_named_args(&["bytes", "timeout"], args)?;
                        let argument = bound[0].ok_or_else(|| {
                            "direct backend expected `write_bytes()` to receive `bytes`".to_string()
                        })?;
                        let loaded = self.load_operand(&argument.value)?;
                        let bytes = self.ensure_opaque(loaded)?;
                        let timeout = self.lower_optional_opaque_arg(bound[1])?;
                        let inst = self.builder.ins().call(
                            self.tcp_stream_write_bytes,
                            &[object.values[0], bytes.values[0], timeout],
                        );
                        Ok(self.owned_opaque_result(
                            self.builder.inst_results(inst).to_vec(),
                            Type::Named(
                                "Result".to_string(),
                                vec![Type::Unit, Type::Named("io.Error".to_string(), Vec::new())],
                            ),
                        ))
                    }
                    "flush" => {
                        if !args.is_empty() {
                            return Err("direct backend expected `flush()` to take no arguments"
                                .to_string());
                        }
                        let inst = self
                            .builder
                            .ins()
                            .call(self.tcp_stream_flush, &[object.values[0]]);
                        Ok(self.owned_opaque_result(
                            self.builder.inst_results(inst).to_vec(),
                            Type::Named(
                                "Result".to_string(),
                                vec![Type::Unit, Type::Named("io.Error".to_string(), Vec::new())],
                            ),
                        ))
                    }
                    "local_addr" => {
                        if !args.is_empty() {
                            return Err(
                                "direct backend expected `local_addr()` to take no arguments"
                                    .to_string(),
                            );
                        }
                        let inst = self
                            .builder
                            .ins()
                            .call(self.tcp_stream_local_addr, &[object.values[0]]);
                        Ok(self.owned_opaque_result(
                            self.builder.inst_results(inst).to_vec(),
                            Type::Named(
                                "Result".to_string(),
                                vec![
                                    Type::named("String"),
                                    Type::Named("io.Error".to_string(), Vec::new()),
                                ],
                            ),
                        ))
                    }
                    "peer_addr" => {
                        if !args.is_empty() {
                            return Err(
                                "direct backend expected `peer_addr()` to take no arguments"
                                    .to_string(),
                            );
                        }
                        let inst = self
                            .builder
                            .ins()
                            .call(self.tcp_stream_peer_addr, &[object.values[0]]);
                        Ok(self.owned_opaque_result(
                            self.builder.inst_results(inst).to_vec(),
                            Type::Named(
                                "Result".to_string(),
                                vec![
                                    Type::named("String"),
                                    Type::Named("io.Error".to_string(), Vec::new()),
                                ],
                            ),
                        ))
                    }
                    "shutdown_read" => {
                        if !args.is_empty() {
                            return Err(
                                "direct backend expected `shutdown_read()` to take no arguments"
                                    .to_string(),
                            );
                        }
                        let inst = self
                            .builder
                            .ins()
                            .call(self.tcp_stream_shutdown_read, &[object.values[0]]);
                        Ok(self.owned_opaque_result(
                            self.builder.inst_results(inst).to_vec(),
                            Type::Named(
                                "Result".to_string(),
                                vec![Type::Unit, Type::Named("io.Error".to_string(), Vec::new())],
                            ),
                        ))
                    }
                    "shutdown_write" => {
                        if !args.is_empty() {
                            return Err(
                                "direct backend expected `shutdown_write()` to take no arguments"
                                    .to_string(),
                            );
                        }
                        let inst = self
                            .builder
                            .ins()
                            .call(self.tcp_stream_shutdown_write, &[object.values[0]]);
                        Ok(self.owned_opaque_result(
                            self.builder.inst_results(inst).to_vec(),
                            Type::Named(
                                "Result".to_string(),
                                vec![Type::Unit, Type::Named("io.Error".to_string(), Vec::new())],
                            ),
                        ))
                    }
                    "shutdown_both" => {
                        if !args.is_empty() {
                            return Err(
                                "direct backend expected `shutdown_both()` to take no arguments"
                                    .to_string(),
                            );
                        }
                        let inst = self
                            .builder
                            .ins()
                            .call(self.tcp_stream_shutdown_both, &[object.values[0]]);
                        Ok(self.owned_opaque_result(
                            self.builder.inst_results(inst).to_vec(),
                            Type::Named(
                                "Result".to_string(),
                                vec![Type::Unit, Type::Named("io.Error".to_string(), Vec::new())],
                            ),
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
                            .call(self.tcp_stream_close, &[object.values[0]]);
                        Ok(ValueRef {
                            values: self.builder.inst_results(inst).to_vec(),
                            ty: DirectType::Scalar(ScalarKind::Unit),
                        })
                    }
                    _ => Err(format!(
                        "direct backend does not know runtime member `{}.{}`",
                        name, field
                    )),
                };
            }
            if name == "net.UdpSocket" {
                let object = self.ensure_opaque(object)?;
                return match field {
                    "send_text" => {
                        let bound =
                            ordered_optional_named_args(&["address", "text", "timeout"], args)?;
                        let address = bound[0].ok_or_else(|| {
                            "direct backend expected `send_text()` to receive `address`".to_string()
                        })?;
                        let text = bound[1].ok_or_else(|| {
                            "direct backend expected `send_text()` to receive `text`".to_string()
                        })?;
                        let loaded_address = self.load_operand(&address.value)?;
                        let address = self.ensure_opaque(loaded_address)?;
                        let loaded_text = self.load_operand(&text.value)?;
                        let text = self.ensure_opaque(loaded_text)?;
                        let timeout = self.lower_optional_opaque_arg(bound[2])?;
                        let inst = self.builder.ins().call(
                            self.udp_socket_send_text,
                            &[object.values[0], address.values[0], text.values[0], timeout],
                        );
                        Ok(self.owned_opaque_result(
                            self.builder.inst_results(inst).to_vec(),
                            Type::Named(
                                "Result".to_string(),
                                vec![Type::Unit, Type::Named("io.Error".to_string(), Vec::new())],
                            ),
                        ))
                    }
                    "send_bytes" => {
                        let bound =
                            ordered_optional_named_args(&["address", "bytes", "timeout"], args)?;
                        let address = bound[0].ok_or_else(|| {
                            "direct backend expected `send_bytes()` to receive `address`"
                                .to_string()
                        })?;
                        let bytes = bound[1].ok_or_else(|| {
                            "direct backend expected `send_bytes()` to receive `bytes`".to_string()
                        })?;
                        let loaded_address = self.load_operand(&address.value)?;
                        let address = self.ensure_opaque(loaded_address)?;
                        let loaded_bytes = self.load_operand(&bytes.value)?;
                        let bytes = self.ensure_opaque(loaded_bytes)?;
                        let timeout = self.lower_optional_opaque_arg(bound[2])?;
                        let inst = self.builder.ins().call(
                            self.udp_socket_send_bytes,
                            &[
                                object.values[0],
                                address.values[0],
                                bytes.values[0],
                                timeout,
                            ],
                        );
                        Ok(self.owned_opaque_result(
                            self.builder.inst_results(inst).to_vec(),
                            Type::Named(
                                "Result".to_string(),
                                vec![Type::Unit, Type::Named("io.Error".to_string(), Vec::new())],
                            ),
                        ))
                    }
                    "recv" => {
                        let bound = ordered_optional_named_args(&["max_bytes", "timeout"], args)?;
                        let count = bound[0].ok_or_else(|| {
                            "direct backend expected `recv()` to receive `max_bytes`".to_string()
                        })?;
                        let loaded_count = self.load_operand(&count.value)?;
                        let count = self
                            .coerce_value(loaded_count, &DirectType::Scalar(ScalarKind::Int32))?;
                        let count = self.ensure_opaque(count)?;
                        let timeout = self.lower_optional_opaque_arg(bound[1])?;
                        let inst = self.builder.ins().call(
                            self.udp_socket_recv,
                            &[object.values[0], count.values[0], timeout],
                        );
                        Ok(self.owned_opaque_result(
                            self.builder.inst_results(inst).to_vec(),
                            Type::Named(
                                "Result".to_string(),
                                vec![
                                    Type::Named(
                                        "Option".to_string(),
                                        vec![Type::Named(
                                            "Vec".to_string(),
                                            vec![Type::named("uint8")],
                                        )],
                                    ),
                                    Type::Named("io.Error".to_string(), Vec::new()),
                                ],
                            ),
                        ))
                    }
                    "recv_from" => {
                        let bound = ordered_optional_named_args(&["max_bytes", "timeout"], args)?;
                        let count = bound[0].ok_or_else(|| {
                            "direct backend expected `recv_from()` to receive `max_bytes`"
                                .to_string()
                        })?;
                        let loaded_count = self.load_operand(&count.value)?;
                        let count = self
                            .coerce_value(loaded_count, &DirectType::Scalar(ScalarKind::Int32))?;
                        let count = self.ensure_opaque(count)?;
                        let timeout = self.lower_optional_opaque_arg(bound[1])?;
                        let inst = self.builder.ins().call(
                            self.udp_socket_recv_from,
                            &[object.values[0], count.values[0], timeout],
                        );
                        Ok(self.owned_opaque_result(
                            self.builder.inst_results(inst).to_vec(),
                            Type::Named(
                                "Result".to_string(),
                                vec![
                                    Type::Named(
                                        "Option".to_string(),
                                        vec![Type::Named(
                                            "net.UdpDatagram".to_string(),
                                            Vec::new(),
                                        )],
                                    ),
                                    Type::Named("io.Error".to_string(), Vec::new()),
                                ],
                            ),
                        ))
                    }
                    "local_addr" => {
                        let inst = self
                            .builder
                            .ins()
                            .call(self.udp_socket_local_addr, &[object.values[0]]);
                        Ok(self.owned_opaque_result(
                            self.builder.inst_results(inst).to_vec(),
                            Type::Named(
                                "Result".to_string(),
                                vec![
                                    Type::named("String"),
                                    Type::Named("io.Error".to_string(), Vec::new()),
                                ],
                            ),
                        ))
                    }
                    "peer_addr" => {
                        let inst = self
                            .builder
                            .ins()
                            .call(self.udp_socket_peer_addr, &[object.values[0]]);
                        Ok(self.owned_opaque_result(
                            self.builder.inst_results(inst).to_vec(),
                            Type::Named(
                                "Result".to_string(),
                                vec![
                                    Type::named("String"),
                                    Type::Named("io.Error".to_string(), Vec::new()),
                                ],
                            ),
                        ))
                    }
                    "close" => {
                        let inst = self
                            .builder
                            .ins()
                            .call(self.udp_socket_close, &[object.values[0]]);
                        Ok(ValueRef {
                            values: self.builder.inst_results(inst).to_vec(),
                            ty: DirectType::Scalar(ScalarKind::Unit),
                        })
                    }
                    _ => Err(format!(
                        "direct backend does not know runtime member `{}.{}`",
                        name, field
                    )),
                };
            }
            if name == "net.UdpDatagram" {
                let object = self.ensure_opaque(object)?;
                return match field {
                    "address" => {
                        let results = self
                            .runtime_call_results(self.udp_datagram_address, &[object.values[0]]);
                        Ok(self.owned_opaque_result(results, Type::named("String")))
                    }
                    "bytes" => {
                        let results =
                            self.runtime_call_results(self.udp_datagram_bytes, &[object.values[0]]);
                        Ok(self.owned_opaque_result(
                            results,
                            Type::Named("Vec".to_string(), vec![Type::named("uint8")]),
                        ))
                    }
                    "text" => {
                        let results =
                            self.runtime_call_results(self.udp_datagram_text, &[object.values[0]]);
                        Ok(self.owned_opaque_result(
                            results,
                            Type::Named(
                                "Result".to_string(),
                                vec![
                                    Type::named("String"),
                                    Type::Named("io.Error".to_string(), Vec::new()),
                                ],
                            ),
                        ))
                    }
                    _ => Err(format!(
                        "direct backend does not know runtime member `{}.{}`",
                        name, field
                    )),
                };
            }
            if name == "net.HttpListener" {
                let object = self.ensure_opaque(object)?;
                return match field {
                    "accept" => {
                        let bound = ordered_optional_named_args(&["timeout"], args)?;
                        let timeout = self.lower_optional_opaque_arg(bound[0])?;
                        let inst = self
                            .builder
                            .ins()
                            .call(self.http_listener_accept, &[object.values[0], timeout]);
                        Ok(self.owned_opaque_result(
                            self.builder.inst_results(inst).to_vec(),
                            Type::Named(
                                "Result".to_string(),
                                vec![
                                    Type::Named("net.HttpExchange".to_string(), Vec::new()),
                                    Type::Named("io.Error".to_string(), Vec::new()),
                                ],
                            ),
                        ))
                    }
                    "local_addr" => {
                        let results = self.runtime_call_results(
                            self.http_listener_local_addr,
                            &[object.values[0]],
                        );
                        Ok(self.owned_opaque_result(
                            results,
                            Type::Named(
                                "Result".to_string(),
                                vec![
                                    Type::named("String"),
                                    Type::Named("io.Error".to_string(), Vec::new()),
                                ],
                            ),
                        ))
                    }
                    "close" => Ok(ValueRef {
                        values: self
                            .runtime_call_results(self.http_listener_close, &[object.values[0]]),
                        ty: DirectType::Scalar(ScalarKind::Unit),
                    }),
                    _ => Err(format!(
                        "direct backend does not know runtime member `{}.{}`",
                        name, field
                    )),
                };
            }
            if name == "net.HttpExchange" {
                let object = self.ensure_opaque(object)?;
                return match field {
                    "method" => {
                        let results = self
                            .runtime_call_results(self.http_exchange_method, &[object.values[0]]);
                        Ok(self.owned_opaque_result(results, Type::named("String")))
                    }
                    "path" => {
                        let results =
                            self.runtime_call_results(self.http_exchange_path, &[object.values[0]]);
                        Ok(self.owned_opaque_result(results, Type::named("String")))
                    }
                    "headers" => {
                        let results = self
                            .runtime_call_results(self.http_exchange_headers, &[object.values[0]]);
                        Ok(self.owned_opaque_result(
                            results,
                            Type::Named(
                                "Map".to_string(),
                                vec![Type::named("String"), Type::named("String")],
                            ),
                        ))
                    }
                    "body_text" => {
                        let results = self.runtime_call_results(
                            self.http_exchange_body_text,
                            &[object.values[0]],
                        );
                        Ok(self.owned_opaque_result(
                            results,
                            Type::Named(
                                "Result".to_string(),
                                vec![
                                    Type::named("String"),
                                    Type::Named("io.Error".to_string(), Vec::new()),
                                ],
                            ),
                        ))
                    }
                    "body_bytes" => {
                        let results = self.runtime_call_results(
                            self.http_exchange_body_bytes,
                            &[object.values[0]],
                        );
                        Ok(self.owned_opaque_result(
                            results,
                            Type::Named("Vec".to_string(), vec![Type::named("uint8")]),
                        ))
                    }
                    "respond_text" => {
                        let bound = ordered_named_args(&["status", "text", "headers"], args)?;
                        let loaded_status = self.load_operand(&bound[0].value)?;
                        let status = self
                            .coerce_value(loaded_status, &DirectType::Scalar(ScalarKind::Int32))?;
                        let status = self.ensure_opaque(status)?;
                        let loaded_text = self.load_operand(&bound[1].value)?;
                        let text = self.ensure_opaque(loaded_text)?;
                        let loaded_headers = self.load_operand(&bound[2].value)?;
                        let headers = self.ensure_opaque(loaded_headers)?;
                        let inst = self.builder.ins().call(
                            self.http_exchange_respond_text,
                            &[
                                object.values[0],
                                status.values[0],
                                text.values[0],
                                headers.values[0],
                            ],
                        );
                        Ok(self.owned_opaque_result(
                            self.builder.inst_results(inst).to_vec(),
                            Type::Named(
                                "Result".to_string(),
                                vec![Type::Unit, Type::Named("io.Error".to_string(), Vec::new())],
                            ),
                        ))
                    }
                    "respond_bytes" => {
                        let bound = ordered_named_args(&["status", "bytes", "headers"], args)?;
                        let loaded_status = self.load_operand(&bound[0].value)?;
                        let status = self
                            .coerce_value(loaded_status, &DirectType::Scalar(ScalarKind::Int32))?;
                        let status = self.ensure_opaque(status)?;
                        let loaded_bytes = self.load_operand(&bound[1].value)?;
                        let bytes = self.ensure_opaque(loaded_bytes)?;
                        let loaded_headers = self.load_operand(&bound[2].value)?;
                        let headers = self.ensure_opaque(loaded_headers)?;
                        let inst = self.builder.ins().call(
                            self.http_exchange_respond_bytes,
                            &[
                                object.values[0],
                                status.values[0],
                                bytes.values[0],
                                headers.values[0],
                            ],
                        );
                        Ok(self.owned_opaque_result(
                            self.builder.inst_results(inst).to_vec(),
                            Type::Named(
                                "Result".to_string(),
                                vec![Type::Unit, Type::Named("io.Error".to_string(), Vec::new())],
                            ),
                        ))
                    }
                    "close" => Ok(unit_value(&mut self.builder)),
                    _ => Err(format!(
                        "direct backend does not know runtime member `{}.{}`",
                        name, field
                    )),
                };
            }
            if name == "net.HttpResponse" {
                let object = self.ensure_opaque(object)?;
                return match field {
                    "status" => {
                        let inst = self
                            .builder
                            .ins()
                            .call(self.http_response_status, &[object.values[0]]);
                        let status = self.builder.inst_results(inst)[0];
                        self.emit_int32_bounds_check(status, None);
                        Ok(ValueRef {
                            values: vec![status],
                            ty: DirectType::Scalar(ScalarKind::Int32),
                        })
                    }
                    "reason" => {
                        let results = self
                            .runtime_call_results(self.http_response_reason, &[object.values[0]]);
                        Ok(self.owned_opaque_result(results, Type::named("String")))
                    }
                    "headers" => {
                        let results = self
                            .runtime_call_results(self.http_response_headers, &[object.values[0]]);
                        Ok(self.owned_opaque_result(
                            results,
                            Type::Named(
                                "Map".to_string(),
                                vec![Type::named("String"), Type::named("String")],
                            ),
                        ))
                    }
                    "text" => {
                        let results =
                            self.runtime_call_results(self.http_response_text, &[object.values[0]]);
                        Ok(self.owned_opaque_result(
                            results,
                            Type::Named(
                                "Result".to_string(),
                                vec![
                                    Type::named("String"),
                                    Type::Named("io.Error".to_string(), Vec::new()),
                                ],
                            ),
                        ))
                    }
                    "bytes" => {
                        let results = self
                            .runtime_call_results(self.http_response_bytes, &[object.values[0]]);
                        Ok(self.owned_opaque_result(
                            results,
                            Type::Named("Vec".to_string(), vec![Type::named("uint8")]),
                        ))
                    }
                    "close" => Ok(unit_value(&mut self.builder)),
                    _ => Err(format!(
                        "direct backend does not know runtime member `{}.{}`",
                        name, field
                    )),
                };
            }
            if name == "net.WebSocketListener" {
                let object = self.ensure_opaque(object)?;
                return match field {
                    "accept" => {
                        let bound = ordered_optional_named_args(&["timeout"], args)?;
                        let timeout = self.lower_optional_opaque_arg(bound[0])?;
                        let inst = self
                            .builder
                            .ins()
                            .call(self.websocket_listener_accept, &[object.values[0], timeout]);
                        Ok(self.owned_opaque_result(
                            self.builder.inst_results(inst).to_vec(),
                            Type::Named(
                                "Result".to_string(),
                                vec![
                                    Type::Named("net.WebSocket".to_string(), Vec::new()),
                                    Type::Named("io.Error".to_string(), Vec::new()),
                                ],
                            ),
                        ))
                    }
                    "local_addr" => {
                        let results = self.runtime_call_results(
                            self.websocket_listener_local_addr,
                            &[object.values[0]],
                        );
                        Ok(self.owned_opaque_result(
                            results,
                            Type::Named(
                                "Result".to_string(),
                                vec![
                                    Type::named("String"),
                                    Type::Named("io.Error".to_string(), Vec::new()),
                                ],
                            ),
                        ))
                    }
                    "close" => Ok(unit_value(&mut self.builder)),
                    _ => Err(format!(
                        "direct backend does not know runtime member `{}.{}`",
                        name, field
                    )),
                };
            }
            if name == "net.WebSocket" {
                let object = self.ensure_opaque(object)?;
                return match field {
                    "send_text" => {
                        let bound = ordered_optional_named_args(&["text", "timeout"], args)?;
                        let text = bound[0].ok_or_else(|| {
                            "direct backend expected `send_text()` to receive `text`".to_string()
                        })?;
                        let loaded_text = self.load_operand(&text.value)?;
                        let text = self.ensure_opaque(loaded_text)?;
                        let timeout = self.lower_optional_opaque_arg(bound[1])?;
                        let inst = self.builder.ins().call(
                            self.websocket_send_text,
                            &[object.values[0], text.values[0], timeout],
                        );
                        Ok(self.owned_opaque_result(
                            self.builder.inst_results(inst).to_vec(),
                            Type::Named(
                                "Result".to_string(),
                                vec![Type::Unit, Type::Named("io.Error".to_string(), Vec::new())],
                            ),
                        ))
                    }
                    "send_bytes" => {
                        let bound = ordered_optional_named_args(&["bytes", "timeout"], args)?;
                        let bytes = bound[0].ok_or_else(|| {
                            "direct backend expected `send_bytes()` to receive `bytes`".to_string()
                        })?;
                        let loaded_bytes = self.load_operand(&bytes.value)?;
                        let bytes = self.ensure_opaque(loaded_bytes)?;
                        let timeout = self.lower_optional_opaque_arg(bound[1])?;
                        let inst = self.builder.ins().call(
                            self.websocket_send_bytes,
                            &[object.values[0], bytes.values[0], timeout],
                        );
                        Ok(self.owned_opaque_result(
                            self.builder.inst_results(inst).to_vec(),
                            Type::Named(
                                "Result".to_string(),
                                vec![Type::Unit, Type::Named("io.Error".to_string(), Vec::new())],
                            ),
                        ))
                    }
                    "recv_text" => {
                        let bound = ordered_optional_named_args(&["timeout"], args)?;
                        let timeout = self.lower_optional_opaque_arg(bound[0])?;
                        let inst = self
                            .builder
                            .ins()
                            .call(self.websocket_recv_text, &[object.values[0], timeout]);
                        Ok(self.owned_opaque_result(
                            self.builder.inst_results(inst).to_vec(),
                            Type::Named(
                                "Result".to_string(),
                                vec![
                                    Type::Named("Option".to_string(), vec![Type::named("String")]),
                                    Type::Named("io.Error".to_string(), Vec::new()),
                                ],
                            ),
                        ))
                    }
                    "recv_bytes" => {
                        let bound = ordered_optional_named_args(&["timeout"], args)?;
                        let timeout = self.lower_optional_opaque_arg(bound[0])?;
                        let inst = self
                            .builder
                            .ins()
                            .call(self.websocket_recv_bytes, &[object.values[0], timeout]);
                        Ok(self.owned_opaque_result(
                            self.builder.inst_results(inst).to_vec(),
                            Type::Named(
                                "Result".to_string(),
                                vec![
                                    Type::Named(
                                        "Option".to_string(),
                                        vec![Type::Named(
                                            "Vec".to_string(),
                                            vec![Type::named("uint8")],
                                        )],
                                    ),
                                    Type::Named("io.Error".to_string(), Vec::new()),
                                ],
                            ),
                        ))
                    }
                    "close" => Ok(ValueRef {
                        values: self
                            .runtime_call_results(self.websocket_close, &[object.values[0]]),
                        ty: DirectType::Scalar(ScalarKind::Unit),
                    }),
                    _ => Err(format!(
                        "direct backend does not know runtime member `{}.{}`",
                        name, field
                    )),
                };
            }
            if name == "net.UnixListener" {
                let object = self.ensure_opaque(object)?;
                return match field {
                    "accept" => {
                        let bound = ordered_optional_named_args(&["timeout"], args)?;
                        let timeout = self.lower_optional_opaque_arg(bound[0])?;
                        let inst = self
                            .builder
                            .ins()
                            .call(self.unix_listener_accept, &[object.values[0], timeout]);
                        Ok(self.owned_opaque_result(
                            self.builder.inst_results(inst).to_vec(),
                            Type::Named(
                                "Result".to_string(),
                                vec![
                                    Type::Named("net.UnixStream".to_string(), Vec::new()),
                                    Type::Named("io.Error".to_string(), Vec::new()),
                                ],
                            ),
                        ))
                    }
                    "close" => Ok(ValueRef {
                        values: self
                            .runtime_call_results(self.unix_listener_close, &[object.values[0]]),
                        ty: DirectType::Scalar(ScalarKind::Unit),
                    }),
                    _ => Err(format!(
                        "direct backend does not know runtime member `{}.{}`",
                        name, field
                    )),
                };
            }
            if name == "net.UnixStream" {
                let object = self.ensure_opaque(object)?;
                return match field {
                    "read_line" => {
                        let bound = ordered_optional_named_args(&["timeout"], args)?;
                        let timeout = self.lower_optional_opaque_arg(bound[0])?;
                        let inst = self
                            .builder
                            .ins()
                            .call(self.unix_stream_read_line, &[object.values[0], timeout]);
                        Ok(self.owned_opaque_result(
                            self.builder.inst_results(inst).to_vec(),
                            Type::Named(
                                "Result".to_string(),
                                vec![
                                    Type::Named("Option".to_string(), vec![Type::named("String")]),
                                    Type::Named("io.Error".to_string(), Vec::new()),
                                ],
                            ),
                        ))
                    }
                    "read_exact" => {
                        let bound = ordered_optional_named_args(&["count", "timeout"], args)?;
                        let count = bound[0].ok_or_else(|| {
                            "direct backend expected `read_exact()` to receive `count`".to_string()
                        })?;
                        let loaded_count = self.load_operand(&count.value)?;
                        let count = self
                            .coerce_value(loaded_count, &DirectType::Scalar(ScalarKind::Int32))?;
                        let count = self.ensure_opaque(count)?;
                        let timeout = self.lower_optional_opaque_arg(bound[1])?;
                        let inst = self.builder.ins().call(
                            self.unix_stream_read_exact,
                            &[object.values[0], count.values[0], timeout],
                        );
                        Ok(self.owned_opaque_result(
                            self.builder.inst_results(inst).to_vec(),
                            Type::Named(
                                "Result".to_string(),
                                vec![
                                    Type::Named("Vec".to_string(), vec![Type::named("uint8")]),
                                    Type::Named("io.Error".to_string(), Vec::new()),
                                ],
                            ),
                        ))
                    }
                    "write_all" => {
                        let bound = ordered_optional_named_args(&["text", "timeout"], args)?;
                        let text = bound[0].ok_or_else(|| {
                            "direct backend expected `write_all()` to receive `text`".to_string()
                        })?;
                        let loaded_text = self.load_operand(&text.value)?;
                        let text = self.ensure_opaque(loaded_text)?;
                        let timeout = self.lower_optional_opaque_arg(bound[1])?;
                        let inst = self.builder.ins().call(
                            self.unix_stream_write_all,
                            &[object.values[0], text.values[0], timeout],
                        );
                        Ok(self.owned_opaque_result(
                            self.builder.inst_results(inst).to_vec(),
                            Type::Named(
                                "Result".to_string(),
                                vec![Type::Unit, Type::Named("io.Error".to_string(), Vec::new())],
                            ),
                        ))
                    }
                    "close" => Ok(ValueRef {
                        values: self
                            .runtime_call_results(self.unix_stream_close, &[object.values[0]]),
                        ty: DirectType::Scalar(ScalarKind::Unit),
                    }),
                    _ => Err(format!(
                        "direct backend does not know runtime member `{}.{}`",
                        name, field
                    )),
                };
            }
            if name == "net.TlsListener" {
                let object = self.ensure_opaque(object)?;
                return match field {
                    "accept" => {
                        let bound = ordered_optional_named_args(&["timeout"], args)?;
                        let timeout = self.lower_optional_opaque_arg(bound[0])?;
                        let inst = self
                            .builder
                            .ins()
                            .call(self.tls_listener_accept, &[object.values[0], timeout]);
                        Ok(self.owned_opaque_result(
                            self.builder.inst_results(inst).to_vec(),
                            Type::Named(
                                "Result".to_string(),
                                vec![
                                    Type::Named("net.TlsStream".to_string(), Vec::new()),
                                    Type::Named("io.Error".to_string(), Vec::new()),
                                ],
                            ),
                        ))
                    }
                    "local_addr" => {
                        let results = self.runtime_call_results(
                            self.tls_listener_local_addr,
                            &[object.values[0]],
                        );
                        Ok(self.owned_opaque_result(
                            results,
                            Type::Named(
                                "Result".to_string(),
                                vec![
                                    Type::named("String"),
                                    Type::Named("io.Error".to_string(), Vec::new()),
                                ],
                            ),
                        ))
                    }
                    "close" => Ok(ValueRef {
                        values: self
                            .runtime_call_results(self.tls_listener_close, &[object.values[0]]),
                        ty: DirectType::Scalar(ScalarKind::Unit),
                    }),
                    _ => Err(format!(
                        "direct backend does not know runtime member `{}.{}`",
                        name, field
                    )),
                };
            }
            if name == "net.TlsStream" {
                let object = self.ensure_opaque(object)?;
                return match field {
                    "read_line" => {
                        let bound = ordered_optional_named_args(&["timeout"], args)?;
                        let timeout = self.lower_optional_opaque_arg(bound[0])?;
                        let inst = self
                            .builder
                            .ins()
                            .call(self.tls_stream_read_line, &[object.values[0], timeout]);
                        Ok(self.owned_opaque_result(
                            self.builder.inst_results(inst).to_vec(),
                            Type::Named(
                                "Result".to_string(),
                                vec![
                                    Type::Named("Option".to_string(), vec![Type::named("String")]),
                                    Type::Named("io.Error".to_string(), Vec::new()),
                                ],
                            ),
                        ))
                    }
                    "read_exact" => {
                        let bound = ordered_optional_named_args(&["count", "timeout"], args)?;
                        let count = bound[0].ok_or_else(|| {
                            "direct backend expected `read_exact()` to receive `count`".to_string()
                        })?;
                        let loaded_count = self.load_operand(&count.value)?;
                        let count = self
                            .coerce_value(loaded_count, &DirectType::Scalar(ScalarKind::Int32))?;
                        let count = self.ensure_opaque(count)?;
                        let timeout = self.lower_optional_opaque_arg(bound[1])?;
                        let inst = self.builder.ins().call(
                            self.tls_stream_read_exact,
                            &[object.values[0], count.values[0], timeout],
                        );
                        Ok(self.owned_opaque_result(
                            self.builder.inst_results(inst).to_vec(),
                            Type::Named(
                                "Result".to_string(),
                                vec![
                                    Type::Named("Vec".to_string(), vec![Type::named("uint8")]),
                                    Type::Named("io.Error".to_string(), Vec::new()),
                                ],
                            ),
                        ))
                    }
                    "write_all" => {
                        let bound = ordered_optional_named_args(&["text", "timeout"], args)?;
                        let text = bound[0].ok_or_else(|| {
                            "direct backend expected `write_all()` to receive `text`".to_string()
                        })?;
                        let loaded_text = self.load_operand(&text.value)?;
                        let text = self.ensure_opaque(loaded_text)?;
                        let timeout = self.lower_optional_opaque_arg(bound[1])?;
                        let inst = self.builder.ins().call(
                            self.tls_stream_write_all,
                            &[object.values[0], text.values[0], timeout],
                        );
                        Ok(self.owned_opaque_result(
                            self.builder.inst_results(inst).to_vec(),
                            Type::Named(
                                "Result".to_string(),
                                vec![Type::Unit, Type::Named("io.Error".to_string(), Vec::new())],
                            ),
                        ))
                    }
                    "close" => Ok(ValueRef {
                        values: self
                            .runtime_call_results(self.tls_stream_close, &[object.values[0]]),
                        ty: DirectType::Scalar(ScalarKind::Unit),
                    }),
                    _ => Err(format!(
                        "direct backend does not know runtime member `{}.{}`",
                        name, field
                    )),
                };
            }
            if self.classes.contains_key(name) || self.find_trait_method(object_ty, field).is_some()
            {
                if let Ok(result) = self.compile_class_member_call(
                    name,
                    Some(object_ty.clone()),
                    object.clone(),
                    field,
                    receiver_place,
                    args,
                ) {
                    return Ok(result);
                }
            }
            if name == "Queue" {
                let object = self.ensure_opaque(object)?;
                return match field {
                    "put" => {
                        let [argument] = args else {
                            return Err(format!(
                                "direct backend expected `{}()` to receive one argument",
                                field
                            ));
                        };
                        let loaded = self.load_operand(&argument.value)?;
                        let value = self.ensure_opaque(loaded)?;
                        let inst = self
                            .builder
                            .ins()
                            .call(self.channel_send, &[object.values[0], value.values[0]]);
                        Ok(self.owned_opaque_result(
                            self.builder.inst_results(inst).to_vec(),
                            Type::Named(
                                "Result".to_string(),
                                vec![
                                    Type::Unit,
                                    Type::Named(
                                        "SendError".to_string(),
                                        vec![class_args
                                            .first()
                                            .cloned()
                                            .unwrap_or_else(|| Type::named("Unknown"))],
                                    ),
                                ],
                            ),
                        ))
                    }
                    "get" => {
                        let inst = match args {
                            [] => self
                                .builder
                                .ins()
                                .call(self.channel_recv, &[object.values[0]]),
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
                                    self.channel_recv_timeout_value,
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
                                "Option".to_string(),
                                vec![class_args
                                    .first()
                                    .cloned()
                                    .unwrap_or_else(|| Type::named("Unknown"))],
                            ),
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
                            .call(self.channel_close, &[object.values[0]]);
                        Ok(ValueRef {
                            values: self.builder.inst_results(inst).to_vec(),
                            ty: DirectType::Scalar(ScalarKind::Unit),
                        })
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
                        if !args.is_empty() {
                            return Err(format!(
                                "direct backend expected `{}` to take no arguments",
                                field
                            ));
                        }
                        let inst = self.builder.ins().call(self.task_join, &[object.values[0]]);
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
                        let inst = self
                            .builder
                            .ins()
                            .call(self.task_group_cancel, &[object.values[0]]);
                        Ok(ValueRef {
                            values: self.builder.inst_results(inst).to_vec(),
                            ty: DirectType::Scalar(ScalarKind::Unit),
                        })
                    }
                    "close" => {
                        if !args.is_empty() {
                            return Err("direct backend expected `close()` to take no arguments"
                                .to_string());
                        }
                        let cancel_before = self.builder.ins().iconst(types::I64, 0);
                        let inst = self
                            .builder
                            .ins()
                            .call(self.task_group_close, &[object.values[0], cancel_before]);
                        Ok(ValueRef {
                            values: self.builder.inst_results(inst).to_vec(),
                            ty: DirectType::Scalar(ScalarKind::Unit),
                        })
                    }
                    _ => Err(format!(
                        "direct backend does not know runtime member `{}.{}`",
                        name, field
                    )),
                };
            }
            let _ = class_args;
        }

        let candidates = self.dynamic_method_candidates(field);
        if candidates.is_empty() {
            return Err(format!(
                "direct backend does not know dynamic method `.{}` on `{}`",
                field, object_ty
            ));
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
            );
        }

        let result_ty = if candidates
            .iter()
            .map(|(_, method)| self.call_result_type(&method.function_name))
            .collect::<std::result::Result<Vec<_>, _>>()?
            .windows(2)
            .all(|window| window[0] == window[1])
        {
            self.call_result_type(&candidates[0].1.function_name)?
        } else {
            DirectType::Opaque(Type::named("Unknown"))
        };

        let join_block = self.builder.create_block();
        let mut current_fallthrough = None;
        let result_vars = self.declare_temporary_result_storage(&result_ty)?;
        for (candidate_ty, _method) in candidates.iter() {
            let Type::Named(candidate_name, _) = candidate_ty else {
                continue;
            };
            let matched = self.value_matches_runtime_type(object.values[0], candidate_ty)?;
            let then_block = self.builder.create_block();
            let else_block = self.builder.create_block();
            self.builder
                .ins()
                .brif(matched, then_block, &[], else_block, &[]);
            self.builder.switch_to_block(then_block);
            let call_result = self.compile_class_member_call(
                candidate_name,
                Some(candidate_ty.clone()),
                object.clone(),
                field,
                receiver_place,
                args,
            )?;
            self.store_result_vars(&result_vars, &call_result)?;
            self.release_all_temporary_owned();
            self.builder.ins().jump(join_block, &[]);
            self.builder.seal_block(then_block);
            self.builder.switch_to_block(else_block);
            current_fallthrough = Some(else_block);
        }
        if let Some(else_block) = current_fallthrough {
            self.builder.switch_to_block(else_block);
            self.builder.ins().trap(TrapCode::unwrap_user(1));
            self.builder.seal_block(else_block);
        }
        self.builder.switch_to_block(join_block);
        self.builder.seal_block(join_block);
        self.load_result_vars(&result_vars, result_ty)
    }

    fn compile_opaque_construct(
        &mut self,
        class_name: &str,
        fields: &[crate::mir::MirFieldInit],
    ) -> std::result::Result<ValueRef, String> {
        let (class_ptr, class_len) = self.string_constant(class_name.as_bytes())?;
        let init = self
            .builder
            .ins()
            .call(self.instance_empty, &[class_ptr, class_len]);
        let mut current = self.owned_opaque_result(
            self.builder.inst_results(init).to_vec(),
            Type::named(class_name),
        );
        for field in fields {
            let loaded = self.load_operand(&field.value)?;
            let loaded = self.ensure_opaque(loaded)?;
            let (field_ptr, field_len) = self.string_constant(field.name.as_bytes())?;
            let inst = self.builder.ins().call(
                self.instance_set_field,
                &[current.values[0], field_ptr, field_len, loaded.values[0]],
            );
            current = self.owned_opaque_result(
                self.builder.inst_results(inst).to_vec(),
                Type::named(class_name),
            );
        }
        Ok(current)
    }

    fn compile_spawn(
        &mut self,
        detached: bool,
        task_group: Option<&Operand>,
        function: &str,
        args: &[MirArg],
    ) -> std::result::Result<ValueRef, String> {
        let thunk_ref = *self.function_thunk_refs.get(function).ok_or_else(|| {
            format!(
                "direct backend does not know spawn thunk for `{}`",
                function
            )
        })?;
        let arg_count_value = self.builder.ins().iconst(types::I64, args.len() as i64);
        let buffer_call = self
            .builder
            .ins()
            .call(self.arg_buffer_new, &[arg_count_value]);
        let buffer = self.builder.inst_results(buffer_call)[0];
        for (index, arg) in args.iter().enumerate() {
            if arg.writeback_place.is_some() {
                return Err(
                    "direct backend does not yet support borrowed spawn arguments".to_string(),
                );
            }
            let value = self.load_operand(&arg.value)?;
            let value = self.ensure_opaque(value)?;
            let index_value = self.builder.ins().iconst(types::I64, index as i64);
            self.builder.ins().call(
                self.arg_buffer_store,
                &[buffer, index_value, value.values[0]],
            );
        }
        let thunk_ptr = self.builder.ins().func_addr(types::I64, thunk_ref);
        let detached_value = self
            .builder
            .ins()
            .iconst(types::I64, if detached { 1 } else { 0 });
        let task_group_value = if let Some(group) = task_group {
            let group = self.load_operand(group)?;
            let group = self.ensure_opaque(group)?;
            group.values[0]
        } else {
            self.builder.ins().iconst(types::I64, 0)
        };
        let call = self.builder.ins().call(
            self.spawn_call,
            &[
                thunk_ptr,
                buffer,
                arg_count_value,
                detached_value,
                task_group_value,
            ],
        );
        let ty = if detached {
            DirectType::Scalar(ScalarKind::Unit)
        } else {
            let return_ty = self
                .function_return_types
                .get(function)
                .cloned()
                .ok_or_else(|| {
                    format!(
                        "direct backend does not know return type for `{}`",
                        function
                    )
                })?;
            DirectType::Opaque(Type::Named(
                "Task".to_string(),
                vec![direct_type_to_type(&return_ty)],
            ))
        };
        match ty {
            DirectType::Opaque(ty) => {
                Ok(self.owned_opaque_result(self.builder.inst_results(call).to_vec(), ty))
            }
            _ => Ok(ValueRef {
                values: self.builder.inst_results(call).to_vec(),
                ty,
            }),
        }
    }

    fn compile_select(
        &mut self,
        arms: &[MirSelectArm],
        otherwise: &str,
    ) -> std::result::Result<(), String> {
        let loop_block = self.builder.create_block();
        let ignore_closed_recv = arms
            .iter()
            .any(|arm| matches!(arm.kind, MirSelectKind::After { .. }));
        let mut initial_deadlines = Vec::new();
        for arm in arms {
            if let MirSelectKind::After { duration } = &arm.kind {
                let duration = self.load_operand(duration)?;
                let duration = self.ensure_opaque(duration)?;
                let inst = self
                    .builder
                    .ins()
                    .call(self.deadline_new, &[duration.values[0]]);
                let deadline = self.builder.inst_results(inst)[0];
                self.builder.append_block_param(loop_block, types::I64);
                initial_deadlines.push(deadline);
            }
        }
        self.builder.ins().jump(loop_block, &initial_deadlines);
        self.builder.switch_to_block(loop_block);
        let deadline_params = self.builder.block_params(loop_block).to_vec();
        let mut deadline_index = 0usize;

        for arm in arms {
            match &arm.kind {
                MirSelectKind::Recv { channel } => {
                    let channel = self.load_operand(channel)?;
                    let channel = self.ensure_opaque(channel)?;
                    let inst = self
                        .builder
                        .ins()
                        .call(self.channel_try_recv, &[channel.values[0]]);
                    let result = self.builder.inst_results(inst)[0];
                    let zero = self.builder.ins().iconst(types::I64, 0);
                    let one = self.builder.ins().iconst(types::I64, 1);
                    let ready = if ignore_closed_recv {
                        self.builder
                            .ins()
                            .icmp(IntCC::UnsignedGreaterThan, result, one)
                    } else {
                        self.builder.ins().icmp(IntCC::NotEqual, result, zero)
                    };
                    let recv_block = self.builder.create_block();
                    let next_block = self.builder.create_block();
                    self.builder
                        .ins()
                        .brif(ready, recv_block, &[], next_block, &[]);
                    self.builder.switch_to_block(recv_block);
                    if let Some(binding) = &arm.binding {
                        let binding_ty = self.type_of_place(binding)?;
                        if ignore_closed_recv {
                            let received = ValueRef {
                                values: vec![result],
                                ty: binding_ty.clone(),
                            };
                            if matches!(binding_ty, DirectType::Opaque(_)) {
                                self.mark_temporary_opaque_owned(&received);
                            }
                            self.store_place(binding, received)?;
                        } else {
                            let closed_block = self.builder.create_block();
                            let value_block = self.builder.create_block();
                            let join_block = self.builder.create_block();
                            let is_closed = self.builder.ins().icmp(IntCC::Equal, result, one);
                            self.builder
                                .ins()
                                .brif(is_closed, closed_block, &[], value_block, &[]);

                            self.builder.switch_to_block(closed_block);
                            let none_value = self.compile_enum_variant("Option", "None", &[])?;
                            self.store_place(
                                binding,
                                ValueRef {
                                    values: none_value.values,
                                    ty: binding_ty.clone(),
                                },
                            )?;
                            self.builder.ins().jump(join_block, &[]);
                            self.builder.seal_block(closed_block);

                            self.builder.switch_to_block(value_block);
                            let received = ValueRef {
                                values: vec![result],
                                ty: binding_ty.clone(),
                            };
                            if matches!(binding_ty, DirectType::Opaque(_)) {
                                self.mark_temporary_opaque_owned(&received);
                            }
                            self.store_place(binding, received)?;
                            self.builder.ins().jump(join_block, &[]);
                            self.builder.seal_block(value_block);

                            self.builder.switch_to_block(join_block);
                            self.builder.seal_block(join_block);
                        }
                    }
                    self.drop_deadlines(&deadline_params);
                    self.builder.ins().jump(self.blocks[&arm.label], &[]);
                    self.builder.seal_block(recv_block);
                    self.builder.switch_to_block(next_block);
                }
                MirSelectKind::Send { channel, value } => {
                    let channel = self.load_operand(channel)?;
                    let channel = self.ensure_opaque(channel)?;
                    let ready = self
                        .builder
                        .ins()
                        .call(self.channel_can_send, &[channel.values[0]]);
                    let ready = self.builder.inst_results(ready)[0];
                    let zero = self.builder.ins().iconst(types::I64, 0);
                    let ready = self.builder.ins().icmp(IntCC::NotEqual, ready, zero);
                    let send_block = self.builder.create_block();
                    let next_block = self.builder.create_block();
                    self.builder
                        .ins()
                        .brif(ready, send_block, &[], next_block, &[]);
                    self.builder.switch_to_block(send_block);
                    let value = self.load_operand(value)?;
                    let value = self.ensure_opaque(value)?;
                    let inst = self
                        .builder
                        .ins()
                        .call(self.channel_send, &[channel.values[0], value.values[0]]);
                    if let Some(binding) = &arm.binding {
                        let binding_ty = self.type_of_place(binding)?;
                        let sent = ValueRef {
                            values: self.builder.inst_results(inst).to_vec(),
                            ty: binding_ty.clone(),
                        };
                        if matches!(binding_ty, DirectType::Opaque(_)) {
                            self.mark_temporary_opaque_owned(&sent);
                        }
                        self.store_place(binding, sent)?;
                    }
                    self.drop_deadlines(&deadline_params);
                    self.builder.ins().jump(self.blocks[&arm.label], &[]);
                    self.builder.seal_block(send_block);
                    self.builder.switch_to_block(next_block);
                }
                MirSelectKind::After { .. } => {
                    let deadline = deadline_params[deadline_index];
                    deadline_index += 1;
                    let inst = self.builder.ins().call(self.deadline_ready, &[deadline]);
                    let ready = self.builder.inst_results(inst)[0];
                    let zero = self.builder.ins().iconst(types::I64, 0);
                    let ready = self.builder.ins().icmp(IntCC::NotEqual, ready, zero);
                    let next_block = self.builder.create_block();
                    let ready_block = self.builder.create_block();
                    self.builder
                        .ins()
                        .brif(ready, ready_block, &[], next_block, &[]);
                    self.builder.switch_to_block(ready_block);
                    self.drop_deadlines(&deadline_params);
                    self.builder.ins().jump(self.blocks[&arm.label], &[]);
                    self.builder.seal_block(ready_block);
                    self.builder.switch_to_block(next_block);
                }
            }
        }

        let recv_arm_count = arms
            .iter()
            .filter(|arm| matches!(arm.kind, MirSelectKind::Recv { .. }))
            .count();
        let send_arm_count = arms
            .iter()
            .filter(|arm| matches!(arm.kind, MirSelectKind::Send { .. }))
            .count();
        let recv_buffer = if recv_arm_count == 0 {
            self.builder.ins().iconst(types::I64, 0)
        } else {
            let count_value = self.builder.ins().iconst(
                types::I64,
                i64::try_from(recv_arm_count)
                    .map_err(|_| "direct backend select recv arm count overflowed i64")?,
            );
            let call = self.builder.ins().call(self.i64_buffer_new, &[count_value]);
            let buffer = self.builder.inst_results(call)[0];
            let mut recv_index = 0usize;
            for arm in arms {
                if let MirSelectKind::Recv { channel } = &arm.kind {
                    let channel = self.load_operand(channel)?;
                    let channel = self.ensure_opaque(channel)?;
                    let index = self.builder.ins().iconst(
                        types::I64,
                        i64::try_from(recv_index)
                            .map_err(|_| "direct backend select recv index overflowed i64")?,
                    );
                    self.builder
                        .ins()
                        .call(self.i64_buffer_store, &[buffer, index, channel.values[0]]);
                    recv_index += 1;
                }
            }
            buffer
        };

        let send_buffer = if send_arm_count == 0 {
            self.builder.ins().iconst(types::I64, 0)
        } else {
            let count_value = self.builder.ins().iconst(
                types::I64,
                i64::try_from(send_arm_count)
                    .map_err(|_| "direct backend select send arm count overflowed i64")?,
            );
            let call = self.builder.ins().call(self.i64_buffer_new, &[count_value]);
            let buffer = self.builder.inst_results(call)[0];
            let mut send_index = 0usize;
            for arm in arms {
                if let MirSelectKind::Send { channel, .. } = &arm.kind {
                    let channel = self.load_operand(channel)?;
                    let channel = self.ensure_opaque(channel)?;
                    let index = self.builder.ins().iconst(
                        types::I64,
                        i64::try_from(send_index)
                            .map_err(|_| "direct backend select send index overflowed i64")?,
                    );
                    self.builder
                        .ins()
                        .call(self.i64_buffer_store, &[buffer, index, channel.values[0]]);
                    send_index += 1;
                }
            }
            buffer
        };

        let deadline_buffer = if deadline_params.is_empty() {
            self.builder.ins().iconst(types::I64, 0)
        } else {
            let count_value = self.builder.ins().iconst(
                types::I64,
                i64::try_from(deadline_params.len())
                    .map_err(|_| "direct backend select deadline count overflowed i64")?,
            );
            let call = self.builder.ins().call(self.i64_buffer_new, &[count_value]);
            let buffer = self.builder.inst_results(call)[0];
            for (index, deadline) in deadline_params.iter().enumerate() {
                let index = self.builder.ins().iconst(
                    types::I64,
                    i64::try_from(index)
                        .map_err(|_| "direct backend select deadline index overflowed i64")?,
                );
                self.builder
                    .ins()
                    .call(self.i64_buffer_store, &[buffer, index, *deadline]);
            }
            buffer
        };

        let recv_count_value = self.builder.ins().iconst(
            types::I64,
            i64::try_from(recv_arm_count)
                .map_err(|_| "direct backend select recv arm count overflowed i64")?,
        );
        let send_count_value = self.builder.ins().iconst(
            types::I64,
            i64::try_from(send_arm_count)
                .map_err(|_| "direct backend select send arm count overflowed i64")?,
        );
        let deadline_count_value = self.builder.ins().iconst(
            types::I64,
            i64::try_from(deadline_params.len())
                .map_err(|_| "direct backend select deadline count overflowed i64")?,
        );
        let ignore_closed = self
            .builder
            .ins()
            .iconst(types::I64, if ignore_closed_recv { 1 } else { 0 });
        let call = self.builder.ins().call(
            self.select_wait,
            &[
                recv_buffer,
                recv_count_value,
                send_buffer,
                send_count_value,
                ignore_closed,
                deadline_buffer,
                deadline_count_value,
            ],
        );
        let cancelled = self.builder.inst_results(call)[0];
        let zero = self.builder.ins().iconst(types::I64, 0);
        let cancelled = self.builder.ins().icmp(IntCC::NotEqual, cancelled, zero);
        let cancelled_block = self.builder.create_block();
        let continue_block = self.builder.create_block();
        self.builder
            .ins()
            .brif(cancelled, cancelled_block, &[], continue_block, &[]);
        self.builder.switch_to_block(cancelled_block);
        self.drop_deadlines(&deadline_params);
        self.builder.ins().jump(self.blocks[otherwise], &[]);
        self.builder.seal_block(cancelled_block);
        self.builder.switch_to_block(continue_block);
        self.builder.ins().jump(loop_block, &deadline_params);
        Ok(())
    }

    fn value_matches_type(
        &mut self,
        value: Value,
        type_name: &str,
    ) -> std::result::Result<Value, String> {
        let (ptr, len) = self.string_constant(type_name.as_bytes())?;
        let inst = self
            .builder
            .ins()
            .call(self.value_type_matches, &[value, ptr, len]);
        Ok(self.builder.inst_results(inst)[0])
    }

    fn value_matches_runtime_type(
        &mut self,
        value: Value,
        ty: &Type,
    ) -> std::result::Result<Value, String> {
        match ty {
            Type::TypeParam(_) => Ok(self.builder.ins().iconst(types::I64, 1)),
            Type::Unit => self.value_matches_type(value, "None"),
            Type::Module(path) => self.value_matches_type(value, &format!("module {}", path)),
            Type::Named(name, args) => {
                let mut matched = self.value_matches_type(value, name)?;
                if args.is_empty() {
                    return Ok(matched);
                }

                let Some(class) = self.classes.get(name).cloned() else {
                    return Ok(matched);
                };
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
                    let inst = self
                        .builder
                        .ins()
                        .call(self.instance_get_field, &[value, field_ptr, field_len]);
                    let field_value = self.builder.inst_results(inst)[0];
                    let field_matches = self.value_matches_runtime_type(field_value, &field_ty)?;
                    matched = self.builder.ins().band(matched, field_matches);
                }
                Ok(matched)
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

    fn dynamic_method_candidates(&self, field: &str) -> Vec<(Type, MirMethod)> {
        let mut candidates = Vec::new();
        for class in self.classes.values() {
            if let Some(method) = class.methods.iter().find(|method| method.name == field) {
                candidates.push((Type::named(&class.name), method.clone()));
            }
        }
        for trait_impl in &self.trait_impls {
            if let Some(method) = trait_impl
                .methods
                .iter()
                .find(|method| method.name == field)
            {
                candidates.push((trait_impl.for_type.clone(), method.clone()));
            }
        }
        candidates
    }

    fn find_trait_method(&self, ty: &Type, field: &str) -> Option<&MirMethod> {
        self.trait_impls.iter().find_map(|trait_impl| {
            let mut type_params = BTreeSet::new();
            collect_type_params_from_type(&trait_impl.for_type, &mut type_params);
            let mut substitutions = HashMap::new();
            if !crate::sema::type_pattern_matches(
                &trait_impl.for_type,
                ty,
                &type_params,
                &mut substitutions,
            ) {
                return None;
            }
            trait_impl
                .methods
                .iter()
                .find(|method| method.name == field)
        })
    }

    fn find_trait_method_for_class_name(
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

fn find_method<'a>(class: Option<&'a MirClass>, field: &str) -> Option<&'a MirMethod> {
    let class = class?;
    for method in &class.methods {
        if method.name == field {
            return Some(method);
        }
    }
    None
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

fn declare_string_constant(
    object: &mut ObjectModule,
    string_data: &mut HashMap<Vec<u8>, DataId>,
    builder: &mut FunctionBuilder<'_>,
    bytes: &[u8],
) -> std::result::Result<(Value, Value), String> {
    let id = if let Some(id) = string_data.get(bytes) {
        *id
    } else {
        let name = format!("aurora_data_{}", string_data.len());
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
        id
    };
    let global = object.declare_data_in_func(id, builder.func);
    let ptr = builder.ins().symbol_value(types::I64, global);
    let len = builder.ins().iconst(types::I64, bytes.len() as i64);
    Ok((ptr, len))
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

fn validate_module(module: &MirModule) -> std::result::Result<(), String> {
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
    for function in module.functions.iter().chain(module.top_level.iter()) {
        validate_function(function, &classes)?;
    }
    Ok(())
}

fn validate_function(
    function: &MirFunction,
    classes: &HashMap<String, MirClass>,
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
        for instruction in &block.instructions {
            match instruction {
                Instruction::Assign { value, .. } => validate_rvalue(value, classes)?,
                Instruction::Eval { value } => validate_operand(value)?,
                Instruction::PushCleanup { .. } | Instruction::PopCleanup { .. } => {}
            }
        }
        match &block.terminator {
            Terminator::Return(operand) => validate_operand(operand)?,
            Terminator::Goto(_) => {}
            Terminator::Branch { condition, .. } => validate_operand(condition)?,
            Terminator::ForRange { iterable, .. } => validate_operand(iterable)?,
            Terminator::Match { scrutinee, .. } => validate_operand(scrutinee)?,
            Terminator::Select { arms, .. } => {
                for arm in arms {
                    match &arm.kind {
                        MirSelectKind::Recv { channel } => validate_operand(channel)?,
                        MirSelectKind::Send { channel, value } => {
                            validate_operand(channel)?;
                            validate_operand(value)?;
                        }
                        MirSelectKind::After { duration } => validate_operand(duration)?,
                    }
                }
            }
            other => {
                return Err(format!(
                    "direct backend does not yet support MIR terminator `{:?}`",
                    other
                ))
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
        Rvalue::Use(operand) => validate_operand(operand),
        Rvalue::FormatString { parts } => {
            for part in parts {
                if let MirFormatPart::Value(value) = part {
                    validate_operand(value)?;
                }
            }
            Ok(())
        }
        Rvalue::Unary { value, .. } => validate_operand(value),
        Rvalue::Cast { value, ty, .. } => {
            validate_operand(value)?;
            ensure_direct_type(ty, classes, "cast target")?;
            Ok(())
        }
        Rvalue::Binary { left, right, .. } => {
            validate_operand(left)?;
            validate_operand(right)
        }
        Rvalue::Call { callee, args } => {
            match callee {
                CallTarget::Name(_) | CallTarget::Member { .. } => {}
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
        Rvalue::MapLiteral {
            entries,
            key_type,
            value_type,
        } => {
            ensure_direct_type(key_type, classes, "Map key")?;
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
            ensure_direct_type(element_type, classes, "Set element")?;
            for element in elements {
                validate_operand(element)?;
            }
            Ok(())
        }
        Rvalue::Construct { class_name, .. } => ensure_direct_type(
            &Type::named(class_name),
            classes,
            &format!("class `{}`", class_name),
        )
        .map(|_| ()),
        Rvalue::Member { object, .. } => validate_operand(object),
        Rvalue::EnumVariant { payloads, .. } => {
            for payload in payloads {
                validate_operand(payload)?;
            }
            Ok(())
        }
        Rvalue::VariantPayload { scrutinee, .. } => validate_operand(scrutinee),
        Rvalue::Try { value } => validate_operand(value),
        Rvalue::Spawn {
            task_group, args, ..
        } => {
            if let Some(group) = task_group {
                validate_operand(group)?;
            }
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
        | Operand::Int(_)
        | Operand::Bool(_)
        | Operand::Unit
        | Operand::Float(_)
        | Operand::String(_)
        | Operand::Duration(_) => Ok(()),
    }
}

fn ensure_direct_type(
    ty: &Type,
    classes: &HashMap<String, MirClass>,
    context: &str,
) -> std::result::Result<DirectType, String> {
    direct_type(ty, classes).ok_or_else(|| {
        format!(
            "direct backend does not yet support {} with type `{}`",
            context, ty
        )
    })
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
        Type::Unit => Some(DirectType::Scalar(ScalarKind::Unit)),
        Type::TypeParam(name) => Some(DirectType::Opaque(Type::TypeParam(name.clone()))),
        Type::Module(path) => Some(DirectType::Opaque(Type::Module(path.clone()))),
        Type::Named(name, args) if args.is_empty() && name == "int32" => {
            Some(DirectType::Scalar(ScalarKind::Int32))
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
                    if matches!(field_ty, DirectType::Opaque(_)) {
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

fn infer_rvalue_type(
    rvalue: &Rvalue,
    variable_types: &HashMap<String, DirectType>,
    function_return_types: &HashMap<String, DirectType>,
    classes: &HashMap<String, MirClass>,
) -> Option<DirectType> {
    match rvalue {
        Rvalue::Use(operand) => infer_operand_type(operand, variable_types, classes),
        Rvalue::FormatString { .. } => Some(DirectType::Opaque(Type::named("String"))),
        Rvalue::Unary { op, value, .. } => {
            match (op, infer_operand_type(value, variable_types, classes)?) {
                (UnaryOp::Neg, DirectType::Scalar(ScalarKind::Int32)) => {
                    Some(DirectType::Scalar(ScalarKind::Int32))
                }
                (UnaryOp::Neg, DirectType::Scalar(kind)) if kind.is_float() => {
                    Some(DirectType::Scalar(kind))
                }
                (UnaryOp::Not, _) => Some(DirectType::Scalar(ScalarKind::Bool)),
                _ => None,
            }
        }
        Rvalue::Cast { ty, .. } => direct_type(ty, classes),
        Rvalue::Binary { op, left, .. } => match op {
            BinaryOp::Eq
            | BinaryOp::NotEq
            | BinaryOp::Less
            | BinaryOp::LessEq
            | BinaryOp::Greater
            | BinaryOp::GreaterEq
            | BinaryOp::And
            | BinaryOp::Or => Some(DirectType::Scalar(ScalarKind::Bool)),
            BinaryOp::Add | BinaryOp::Sub | BinaryOp::Mul | BinaryOp::Div | BinaryOp::Mod => {
                infer_operand_type(left, variable_types, classes)
            }
        },
        Rvalue::Call { callee, .. } => match callee {
            CallTarget::Name(name) if name == "print" => Some(DirectType::Scalar(ScalarKind::Unit)),
            CallTarget::Name(name) if name == "range" => {
                Some(DirectType::Opaque(Type::named("Range")))
            }
            CallTarget::Name(name) if name == "queue" => Some(DirectType::Opaque(Type::Named(
                "Queue".to_string(),
                vec![Type::named("Unknown")],
            ))),
            CallTarget::Name(name) if name == "Vec" => Some(DirectType::Opaque(Type::Named(
                "Vec".to_string(),
                vec![Type::named("Unknown")],
            ))),
            CallTarget::Name(name) if name == "Set" => Some(DirectType::Opaque(Type::Named(
                "Set".to_string(),
                vec![Type::named("Unknown")],
            ))),
            CallTarget::Name(name) if name == "Map" => Some(DirectType::Opaque(Type::Named(
                "Map".to_string(),
                vec![Type::named("Unknown"), Type::named("Unknown")],
            ))),
            CallTarget::Name(name) if name == "tasks" => {
                Some(DirectType::Opaque(Type::named("TaskGroup")))
            }
            CallTarget::Name(name) if name == "cancelled" => {
                Some(DirectType::Scalar(ScalarKind::Bool))
            }
            CallTarget::Name(name) if name == "sleep" => Some(DirectType::Scalar(ScalarKind::Unit)),
            CallTarget::Name(name) if name == "io::write" || name == "io::flush" => {
                Some(DirectType::Opaque(Type::Named(
                    "Result".to_string(),
                    vec![Type::Unit, Type::Named("io.Error".to_string(), Vec::new())],
                )))
            }
            CallTarget::Name(name) if name == "io::read_line" => {
                Some(DirectType::Opaque(Type::Named(
                    "Result".to_string(),
                    vec![
                        Type::Named("Option".to_string(), vec![Type::named("String")]),
                        Type::Named("io.Error".to_string(), Vec::new()),
                    ],
                )))
            }
            CallTarget::Name(name) if name == "fs::exists" => {
                Some(DirectType::Scalar(ScalarKind::Bool))
            }
            CallTarget::Name(name) if name == "fs::read_to_string" => {
                Some(DirectType::Opaque(Type::Named(
                    "Result".to_string(),
                    vec![
                        Type::named("String"),
                        Type::Named("io.Error".to_string(), Vec::new()),
                    ],
                )))
            }
            CallTarget::Name(name) if name == "fs::read_bytes" => {
                Some(DirectType::Opaque(Type::Named(
                    "Result".to_string(),
                    vec![
                        Type::Named("Vec".to_string(), vec![Type::named("uint8")]),
                        Type::Named("io.Error".to_string(), Vec::new()),
                    ],
                )))
            }
            CallTarget::Name(name)
                if matches!(
                    name.as_str(),
                    "fs::write_string"
                        | "fs::write_bytes"
                        | "fs::append_string"
                        | "fs::append_bytes"
                        | "fs::create_dir"
                        | "fs::remove_file"
                ) =>
            {
                Some(DirectType::Opaque(Type::Named(
                    "Result".to_string(),
                    vec![Type::Unit, Type::Named("io.Error".to_string(), Vec::new())],
                )))
            }
            CallTarget::Name(name) if name == "fs::read_dir" => {
                Some(DirectType::Opaque(Type::Named(
                    "Result".to_string(),
                    vec![
                        Type::Named("Vec".to_string(), vec![Type::named("String")]),
                        Type::Named("io.Error".to_string(), Vec::new()),
                    ],
                )))
            }
            CallTarget::Name(name)
                if matches!(name.as_str(), "fs::open" | "fs::create" | "fs::append") =>
            {
                Some(DirectType::Opaque(Type::Named(
                    "Result".to_string(),
                    vec![
                        Type::Named("fs.File".to_string(), Vec::new()),
                        Type::Named("io.Error".to_string(), Vec::new()),
                    ],
                )))
            }
            CallTarget::Name(name)
                if matches!(name.as_str(), "net::connect" | "net::connect_timeout") =>
            {
                Some(DirectType::Opaque(Type::Named(
                    "Result".to_string(),
                    vec![
                        Type::Named("net.TcpStream".to_string(), Vec::new()),
                        Type::Named("io.Error".to_string(), Vec::new()),
                    ],
                )))
            }
            CallTarget::Name(name) if name == "net::listen" => {
                Some(DirectType::Opaque(Type::Named(
                    "Result".to_string(),
                    vec![
                        Type::Named("net.TcpListener".to_string(), Vec::new()),
                        Type::Named("io.Error".to_string(), Vec::new()),
                    ],
                )))
            }
            CallTarget::Name(name) if name == "net::udp_bind" => {
                Some(DirectType::Opaque(Type::Named(
                    "Result".to_string(),
                    vec![
                        Type::Named("net.UdpSocket".to_string(), Vec::new()),
                        Type::Named("io.Error".to_string(), Vec::new()),
                    ],
                )))
            }
            CallTarget::Name(name) if name == "net::unix_listen" => {
                Some(DirectType::Opaque(Type::Named(
                    "Result".to_string(),
                    vec![
                        Type::Named("net.UnixListener".to_string(), Vec::new()),
                        Type::Named("io.Error".to_string(), Vec::new()),
                    ],
                )))
            }
            CallTarget::Name(name)
                if matches!(
                    name.as_str(),
                    "net::unix_connect" | "net::unix_connect_timeout"
                ) =>
            {
                Some(DirectType::Opaque(Type::Named(
                    "Result".to_string(),
                    vec![
                        Type::Named("net.UnixStream".to_string(), Vec::new()),
                        Type::Named("io.Error".to_string(), Vec::new()),
                    ],
                )))
            }
            CallTarget::Name(name) if name == "net::tls_listen" => {
                Some(DirectType::Opaque(Type::Named(
                    "Result".to_string(),
                    vec![
                        Type::Named("net.TlsListener".to_string(), Vec::new()),
                        Type::Named("io.Error".to_string(), Vec::new()),
                    ],
                )))
            }
            CallTarget::Name(name)
                if matches!(
                    name.as_str(),
                    "net::tls_connect" | "net::tls_connect_timeout"
                ) =>
            {
                Some(DirectType::Opaque(Type::Named(
                    "Result".to_string(),
                    vec![
                        Type::Named("net.TlsStream".to_string(), Vec::new()),
                        Type::Named("io.Error".to_string(), Vec::new()),
                    ],
                )))
            }
            CallTarget::Name(name) if name == "net::http_listen" => {
                Some(DirectType::Opaque(Type::Named(
                    "Result".to_string(),
                    vec![
                        Type::Named("net.HttpListener".to_string(), Vec::new()),
                        Type::Named("io.Error".to_string(), Vec::new()),
                    ],
                )))
            }
            CallTarget::Name(name)
                if matches!(
                    name.as_str(),
                    "net::http_request_text"
                        | "net::http_request_text_timeout"
                        | "net::http_request_bytes"
                        | "net::http_request_bytes_timeout"
                ) =>
            {
                Some(DirectType::Opaque(Type::Named(
                    "Result".to_string(),
                    vec![
                        Type::Named("net.HttpResponse".to_string(), Vec::new()),
                        Type::Named("io.Error".to_string(), Vec::new()),
                    ],
                )))
            }
            CallTarget::Name(name) if name == "net::websocket_listen" => {
                Some(DirectType::Opaque(Type::Named(
                    "Result".to_string(),
                    vec![
                        Type::Named("net.WebSocketListener".to_string(), Vec::new()),
                        Type::Named("io.Error".to_string(), Vec::new()),
                    ],
                )))
            }
            CallTarget::Name(name)
                if matches!(
                    name.as_str(),
                    "net::websocket_connect" | "net::websocket_connect_timeout"
                ) =>
            {
                Some(DirectType::Opaque(Type::Named(
                    "Result".to_string(),
                    vec![
                        Type::Named("net.WebSocket".to_string(), Vec::new()),
                        Type::Named("io.Error".to_string(), Vec::new()),
                    ],
                )))
            }
            CallTarget::Name(name) if name == "parse_int32" => {
                Some(DirectType::Opaque(Type::Named(
                    "Result".to_string(),
                    vec![Type::named("int32"), Type::named("String")],
                )))
            }
            CallTarget::Name(name) if name == "parse_int64" => {
                Some(DirectType::Opaque(Type::Named(
                    "Result".to_string(),
                    vec![Type::named("int64"), Type::named("String")],
                )))
            }
            CallTarget::Name(name) if name == "parse_float64" => {
                Some(DirectType::Opaque(Type::Named(
                    "Result".to_string(),
                    vec![Type::named("float64"), Type::named("String")],
                )))
            }
            CallTarget::Name(name) => function_return_types.get(name).cloned(),
            CallTarget::Member { object, field, .. } => {
                let object_ty = infer_operand_type(object, variable_types, classes)?;
                if matches!(object_ty.scalar_kind(), Some(kind) if kind.is_float())
                    && field == "sqrt"
                {
                    return Some(object_ty);
                }
                if object_ty.scalar_kind().is_some() && field == "to_string" {
                    return Some(DirectType::Opaque(Type::named("String")));
                }
                match object_ty {
                    DirectType::PlainClass(class_ty) => {
                        let method = find_method(classes.get(&class_ty.class_name), field)?;
                        function_return_types.get(&method.function_name).cloned()
                    }
                    DirectType::Opaque(Type::Named(name, args)) => {
                        if let Some(method) = find_method(classes.get(&name), field) {
                            return function_return_types.get(&method.function_name).cloned();
                        }
                        builtin_opaque_member_return_type(&Type::Named(name, args), field, classes)
                            .or_else(|| Some(DirectType::Opaque(Type::named("Unknown"))))
                    }
                    DirectType::Opaque(ty) => {
                        builtin_opaque_member_return_type(&ty, field, classes)
                            .or_else(|| Some(DirectType::Opaque(Type::named("Unknown"))))
                    }
                    DirectType::Scalar(_) => None,
                }
            }
        },
        Rvalue::VecLiteral { element_type, .. } => Some(DirectType::Opaque(Type::Named(
            "Vec".to_string(),
            vec![element_type.clone()],
        ))),
        Rvalue::MapLiteral {
            key_type,
            value_type,
            ..
        } => Some(DirectType::Opaque(Type::Named(
            "Map".to_string(),
            vec![key_type.clone(), value_type.clone()],
        ))),
        Rvalue::SetLiteral { element_type, .. } => Some(DirectType::Opaque(Type::Named(
            "Set".to_string(),
            vec![element_type.clone()],
        ))),
        Rvalue::Construct { class_name, .. } => direct_type(&Type::named(class_name), classes),
        Rvalue::Member { object, field } => {
            match infer_operand_type(object, variable_types, classes)? {
                ty => direct_field_type(&ty, field, classes),
            }
        }
        Rvalue::EnumVariant { enum_name, .. } => Some(DirectType::Opaque(Type::named(enum_name))),
        Rvalue::VariantPayload { scrutinee, index } => {
            infer_variant_payload_type(scrutinee, *index, variable_types, classes)
                .or_else(|| Some(DirectType::Opaque(Type::named("Unknown"))))
        }
        Rvalue::Try { value } => infer_try_type(value, variable_types, classes)
            .or_else(|| Some(DirectType::Opaque(Type::named("Unknown")))),
        Rvalue::Spawn {
            detached, function, ..
        } => {
            if *detached {
                Some(DirectType::Scalar(ScalarKind::Unit))
            } else {
                function_return_types.get(function).map(|ty| {
                    DirectType::Opaque(Type::Named(
                        "Task".to_string(),
                        vec![direct_type_to_type(ty)],
                    ))
                })
            }
        }
    }
}

fn infer_select_binding_type(
    arm: &MirSelectArm,
    variable_types: &HashMap<String, DirectType>,
    classes: &HashMap<String, MirClass>,
) -> Option<DirectType> {
    match &arm.kind {
        MirSelectKind::Recv { channel } => {
            let channel_ty = infer_operand_type(channel, variable_types, classes)?;
            match channel_ty {
                DirectType::Opaque(Type::Named(name, args)) if name == "Queue" => {
                    Some(DirectType::Opaque(Type::Named(
                        "Option".to_string(),
                        vec![args
                            .first()
                            .cloned()
                            .unwrap_or_else(|| Type::named("Unknown"))],
                    )))
                }
                _ => Some(DirectType::Opaque(Type::Named(
                    "Option".to_string(),
                    vec![Type::named("Unknown")],
                ))),
            }
        }
        MirSelectKind::Send { channel, .. } => {
            let channel_ty = infer_operand_type(channel, variable_types, classes)?;
            match channel_ty {
                DirectType::Opaque(Type::Named(name, args)) if name == "Queue" => {
                    Some(DirectType::Opaque(Type::Named(
                        "Result".to_string(),
                        vec![
                            Type::Unit,
                            Type::Named(
                                "SendError".to_string(),
                                vec![args
                                    .first()
                                    .cloned()
                                    .unwrap_or_else(|| Type::named("Unknown"))],
                            ),
                        ],
                    )))
                }
                _ => Some(DirectType::Opaque(Type::Named(
                    "Result".to_string(),
                    vec![
                        Type::Unit,
                        Type::Named("SendError".to_string(), vec![Type::named("Unknown")]),
                    ],
                ))),
            }
        }
        MirSelectKind::After { duration } => {
            infer_operand_type(duration, variable_types, classes)?;
            Some(DirectType::Scalar(ScalarKind::Unit))
        }
    }
}

fn builtin_opaque_member_return_type(
    object_ty: &Type,
    field: &str,
    classes: &HashMap<String, MirClass>,
) -> Option<DirectType> {
    let Type::Named(name, args) = object_ty else {
        return None;
    };
    if args.is_empty()
        && field == "to_string"
        && matches!(
            name.as_str(),
            "bool"
                | "int8"
                | "int16"
                | "int32"
                | "int64"
                | "int128"
                | "intsize"
                | "uint8"
                | "uint16"
                | "uint32"
                | "uint64"
                | "uint128"
                | "uintsize"
                | "float32"
                | "float64"
        )
    {
        return Some(DirectType::Opaque(Type::named("String")));
    }
    match (name.as_str(), field) {
        ("String", "len") => direct_type(&Type::named("int32"), classes),
        ("String", "contains") | ("String", "starts_with") | ("String", "ends_with") => {
            Some(DirectType::Scalar(ScalarKind::Bool))
        }
        ("String", "split") => Some(DirectType::Opaque(Type::Named(
            "Vec".to_string(),
            vec![Type::named("String")],
        ))),
        ("String", "replace")
        | ("String", "add")
        | ("String", "to_lower")
        | ("String", "to_upper")
        | ("String", "trim")
        | ("String", "clone") => Some(DirectType::Opaque(Type::named("String"))),
        ("String", "join") => Some(DirectType::Opaque(Type::named("String"))),
        ("String", "strip_prefix") | ("String", "strip_suffix") => Some(DirectType::Opaque(
            Type::Named("Option".to_string(), vec![Type::named("String")]),
        )),
        ("Vec", "len") => direct_type(&Type::named("int32"), classes),
        ("Vec", "is_empty") => Some(DirectType::Scalar(ScalarKind::Bool)),
        ("Vec", "clone") => Some(DirectType::Opaque(Type::Named(
            "Vec".to_string(),
            args.clone(),
        ))),
        ("Vec", "push")
        | ("Vec", "extend")
        | ("Vec", "clear")
        | ("Vec", "reverse")
        | ("Vec", "__set_index") => Some(DirectType::Scalar(ScalarKind::Unit)),
        ("Vec", "swap") | ("Vec", "contains") | ("Vec", "insert") => {
            Some(DirectType::Scalar(ScalarKind::Bool))
        }
        ("Vec", "pop")
        | ("Vec", "get")
        | ("Vec", "set")
        | ("Vec", "remove")
        | ("Vec", "__index_option") => direct_type(
            &Type::Named(
                "Option".to_string(),
                vec![args
                    .first()
                    .cloned()
                    .unwrap_or_else(|| Type::named("Unknown"))],
            ),
            classes,
        ),
        ("Vec", "__index") => direct_type(args.first().unwrap_or(&Type::named("Unknown")), classes),
        ("Map", "len") => direct_type(&Type::named("int32"), classes),
        ("Map", "is_empty") | ("Map", "contains_key") => Some(DirectType::Scalar(ScalarKind::Bool)),
        ("Map", "clone") => Some(DirectType::Opaque(Type::Named(
            "Map".to_string(),
            args.clone(),
        ))),
        ("Map", "get") | ("Map", "set") | ("Map", "remove") => direct_type(
            &Type::Named(
                "Option".to_string(),
                vec![args
                    .get(1)
                    .cloned()
                    .unwrap_or_else(|| Type::named("Unknown"))],
            ),
            classes,
        ),
        ("Map", "keys") => direct_type(
            &Type::Named(
                "Vec".to_string(),
                vec![args
                    .first()
                    .cloned()
                    .unwrap_or_else(|| Type::named("Unknown"))],
            ),
            classes,
        ),
        ("Map", "values") => direct_type(
            &Type::Named(
                "Vec".to_string(),
                vec![args
                    .get(1)
                    .cloned()
                    .unwrap_or_else(|| Type::named("Unknown"))],
            ),
            classes,
        ),
        ("Map", "items") | ("Map", "entries") => direct_type(
            &Type::Named(
                "Vec".to_string(),
                vec![Type::Named(
                    "MapEntry".to_string(),
                    vec![
                        args.first()
                            .cloned()
                            .unwrap_or_else(|| Type::named("Unknown")),
                        args.get(1)
                            .cloned()
                            .unwrap_or_else(|| Type::named("Unknown")),
                    ],
                )],
            ),
            classes,
        ),
        ("Map", "clear") | ("Map", "extend") => Some(DirectType::Scalar(ScalarKind::Unit)),
        ("Map", "__index") => direct_type(args.get(1).unwrap_or(&Type::named("Unknown")), classes),
        ("Map", "__set_index") => Some(DirectType::Scalar(ScalarKind::Unit)),
        ("Set", "len") => direct_type(&Type::named("int32"), classes),
        ("Set", "is_empty") => Some(DirectType::Scalar(ScalarKind::Bool)),
        ("Set", "clone") => Some(DirectType::Opaque(Type::Named(
            "Set".to_string(),
            args.clone(),
        ))),
        ("Set", "contains") | ("Set", "insert") | ("Set", "remove") => {
            Some(DirectType::Scalar(ScalarKind::Bool))
        }
        ("Set", "__index_option") => direct_type(
            &Type::Named(
                "Option".to_string(),
                vec![args
                    .first()
                    .cloned()
                    .unwrap_or_else(|| Type::named("Unknown"))],
            ),
            classes,
        ),
        ("Queue", "put") => direct_type(
            &Type::Named(
                "Result".to_string(),
                vec![
                    Type::Unit,
                    Type::Named(
                        "SendError".to_string(),
                        vec![args
                            .first()
                            .cloned()
                            .unwrap_or_else(|| Type::named("Unknown"))],
                    ),
                ],
            ),
            classes,
        ),
        ("Queue", "get") => direct_type(
            &Type::Named(
                "Option".to_string(),
                vec![args
                    .first()
                    .cloned()
                    .unwrap_or_else(|| Type::named("Unknown"))],
            ),
            classes,
        ),
        ("Queue", "close") | ("TaskGroup", "cancel") | ("TaskGroup", "close") => {
            Some(DirectType::Scalar(ScalarKind::Unit))
        }
        ("Task", "result") => direct_type(args.first().unwrap_or(&Type::Unit), classes),
        ("fs.File", "read_all") => direct_type(
            &Type::Named(
                "Result".to_string(),
                vec![
                    Type::named("String"),
                    Type::Named("io.Error".to_string(), Vec::new()),
                ],
            ),
            classes,
        ),
        ("fs.File", "read_bytes") => direct_type(
            &Type::Named(
                "Result".to_string(),
                vec![
                    Type::Named("Vec".to_string(), vec![Type::named("uint8")]),
                    Type::Named("io.Error".to_string(), Vec::new()),
                ],
            ),
            classes,
        ),
        ("fs.File", "write_all") | ("fs.File", "write_bytes") | ("fs.File", "flush") => {
            direct_type(
                &Type::Named(
                    "Result".to_string(),
                    vec![Type::Unit, Type::Named("io.Error".to_string(), Vec::new())],
                ),
                classes,
            )
        }
        ("fs.File", "close") => Some(DirectType::Scalar(ScalarKind::Unit)),
        ("net.TcpListener", "accept") => direct_type(
            &Type::Named(
                "Result".to_string(),
                vec![
                    Type::Named("net.TcpStream".to_string(), Vec::new()),
                    Type::Named("io.Error".to_string(), Vec::new()),
                ],
            ),
            classes,
        ),
        ("net.TcpListener", "local_addr") => direct_type(
            &Type::Named(
                "Result".to_string(),
                vec![
                    Type::named("String"),
                    Type::Named("io.Error".to_string(), Vec::new()),
                ],
            ),
            classes,
        ),
        ("net.TcpListener", "close") => Some(DirectType::Scalar(ScalarKind::Unit)),
        ("net.TcpStream", "read_all")
        | ("net.TcpStream", "local_addr")
        | ("net.TcpStream", "peer_addr") => direct_type(
            &Type::Named(
                "Result".to_string(),
                vec![
                    Type::named("String"),
                    Type::Named("io.Error".to_string(), Vec::new()),
                ],
            ),
            classes,
        ),
        ("net.TcpStream", "read_line") => direct_type(
            &Type::Named(
                "Result".to_string(),
                vec![
                    Type::Named("Option".to_string(), vec![Type::named("String")]),
                    Type::Named("io.Error".to_string(), Vec::new()),
                ],
            ),
            classes,
        ),
        ("net.TcpStream", "read_bytes") => direct_type(
            &Type::Named(
                "Result".to_string(),
                vec![
                    Type::Named(
                        "Option".to_string(),
                        vec![Type::Named("Vec".to_string(), vec![Type::named("uint8")])],
                    ),
                    Type::Named("io.Error".to_string(), Vec::new()),
                ],
            ),
            classes,
        ),
        ("net.TcpStream", "read_exact") => direct_type(
            &Type::Named(
                "Result".to_string(),
                vec![
                    Type::Named("Vec".to_string(), vec![Type::named("uint8")]),
                    Type::Named("io.Error".to_string(), Vec::new()),
                ],
            ),
            classes,
        ),
        ("net.TcpStream", "write_all")
        | ("net.TcpStream", "write_bytes")
        | ("net.TcpStream", "flush")
        | ("net.TcpStream", "shutdown_read")
        | ("net.TcpStream", "shutdown_write")
        | ("net.TcpStream", "shutdown_both") => direct_type(
            &Type::Named(
                "Result".to_string(),
                vec![Type::Unit, Type::Named("io.Error".to_string(), Vec::new())],
            ),
            classes,
        ),
        ("net.TcpStream", "close") => Some(DirectType::Scalar(ScalarKind::Unit)),
        ("net.UdpSocket", "send_text") | ("net.UdpSocket", "send_bytes") => direct_type(
            &Type::Named(
                "Result".to_string(),
                vec![Type::Unit, Type::Named("io.Error".to_string(), Vec::new())],
            ),
            classes,
        ),
        ("net.UdpSocket", "recv") => direct_type(
            &Type::Named(
                "Result".to_string(),
                vec![
                    Type::Named(
                        "Option".to_string(),
                        vec![Type::Named("Vec".to_string(), vec![Type::named("uint8")])],
                    ),
                    Type::Named("io.Error".to_string(), Vec::new()),
                ],
            ),
            classes,
        ),
        ("net.UdpSocket", "recv_from") => direct_type(
            &Type::Named(
                "Result".to_string(),
                vec![
                    Type::Named(
                        "Option".to_string(),
                        vec![Type::Named("net.UdpDatagram".to_string(), Vec::new())],
                    ),
                    Type::Named("io.Error".to_string(), Vec::new()),
                ],
            ),
            classes,
        ),
        ("net.UdpSocket", "local_addr") | ("net.UdpSocket", "peer_addr") => direct_type(
            &Type::Named(
                "Result".to_string(),
                vec![
                    Type::named("String"),
                    Type::Named("io.Error".to_string(), Vec::new()),
                ],
            ),
            classes,
        ),
        ("net.UdpSocket", "close") => Some(DirectType::Scalar(ScalarKind::Unit)),
        ("net.UdpDatagram", "address") => direct_type(&Type::named("String"), classes),
        ("net.UdpDatagram", "bytes") => direct_type(
            &Type::Named("Vec".to_string(), vec![Type::named("uint8")]),
            classes,
        ),
        ("net.UdpDatagram", "text") => direct_type(
            &Type::Named(
                "Result".to_string(),
                vec![
                    Type::named("String"),
                    Type::Named("io.Error".to_string(), Vec::new()),
                ],
            ),
            classes,
        ),
        ("net.HttpListener", "accept") => direct_type(
            &Type::Named(
                "Result".to_string(),
                vec![
                    Type::Named("net.HttpExchange".to_string(), Vec::new()),
                    Type::Named("io.Error".to_string(), Vec::new()),
                ],
            ),
            classes,
        ),
        ("net.HttpListener", "local_addr") => direct_type(
            &Type::Named(
                "Result".to_string(),
                vec![
                    Type::named("String"),
                    Type::Named("io.Error".to_string(), Vec::new()),
                ],
            ),
            classes,
        ),
        ("net.HttpListener", "close") => Some(DirectType::Scalar(ScalarKind::Unit)),
        ("net.HttpExchange", "method") | ("net.HttpExchange", "path") => {
            direct_type(&Type::named("String"), classes)
        }
        ("net.HttpExchange", "headers") | ("net.HttpResponse", "headers") => direct_type(
            &Type::Named(
                "Map".to_string(),
                vec![Type::named("String"), Type::named("String")],
            ),
            classes,
        ),
        ("net.HttpExchange", "body_text") | ("net.HttpResponse", "text") => direct_type(
            &Type::Named(
                "Result".to_string(),
                vec![
                    Type::named("String"),
                    Type::Named("io.Error".to_string(), Vec::new()),
                ],
            ),
            classes,
        ),
        ("net.HttpExchange", "body_bytes") | ("net.HttpResponse", "bytes") => direct_type(
            &Type::Named("Vec".to_string(), vec![Type::named("uint8")]),
            classes,
        ),
        ("net.HttpExchange", "respond_text") | ("net.HttpExchange", "respond_bytes") => {
            direct_type(
                &Type::Named(
                    "Result".to_string(),
                    vec![Type::Unit, Type::Named("io.Error".to_string(), Vec::new())],
                ),
                classes,
            )
        }
        ("net.HttpExchange", "close") => Some(DirectType::Scalar(ScalarKind::Unit)),
        ("net.HttpResponse", "status") => direct_type(&Type::named("int32"), classes),
        ("net.HttpResponse", "reason") => direct_type(&Type::named("String"), classes),
        ("net.HttpResponse", "close") => Some(DirectType::Scalar(ScalarKind::Unit)),
        ("net.WebSocketListener", "accept") => direct_type(
            &Type::Named(
                "Result".to_string(),
                vec![
                    Type::Named("net.WebSocket".to_string(), Vec::new()),
                    Type::Named("io.Error".to_string(), Vec::new()),
                ],
            ),
            classes,
        ),
        ("net.WebSocketListener", "local_addr") => direct_type(
            &Type::Named(
                "Result".to_string(),
                vec![
                    Type::named("String"),
                    Type::Named("io.Error".to_string(), Vec::new()),
                ],
            ),
            classes,
        ),
        ("net.WebSocketListener", "close") => Some(DirectType::Scalar(ScalarKind::Unit)),
        ("net.WebSocket", "send_text") | ("net.WebSocket", "send_bytes") => direct_type(
            &Type::Named(
                "Result".to_string(),
                vec![Type::Unit, Type::Named("io.Error".to_string(), Vec::new())],
            ),
            classes,
        ),
        ("net.WebSocket", "recv_text") => direct_type(
            &Type::Named(
                "Result".to_string(),
                vec![
                    Type::Named("Option".to_string(), vec![Type::named("String")]),
                    Type::Named("io.Error".to_string(), Vec::new()),
                ],
            ),
            classes,
        ),
        ("net.WebSocket", "recv_bytes") => direct_type(
            &Type::Named(
                "Result".to_string(),
                vec![
                    Type::Named(
                        "Option".to_string(),
                        vec![Type::Named("Vec".to_string(), vec![Type::named("uint8")])],
                    ),
                    Type::Named("io.Error".to_string(), Vec::new()),
                ],
            ),
            classes,
        ),
        ("net.WebSocket", "close") => Some(DirectType::Scalar(ScalarKind::Unit)),
        ("net.UnixListener", "accept") => direct_type(
            &Type::Named(
                "Result".to_string(),
                vec![
                    Type::Named("net.UnixStream".to_string(), Vec::new()),
                    Type::Named("io.Error".to_string(), Vec::new()),
                ],
            ),
            classes,
        ),
        ("net.UnixListener", "close") => Some(DirectType::Scalar(ScalarKind::Unit)),
        ("net.UnixStream", "read_line") => direct_type(
            &Type::Named(
                "Result".to_string(),
                vec![
                    Type::Named("Option".to_string(), vec![Type::named("String")]),
                    Type::Named("io.Error".to_string(), Vec::new()),
                ],
            ),
            classes,
        ),
        ("net.UnixStream", "read_exact") => direct_type(
            &Type::Named(
                "Result".to_string(),
                vec![
                    Type::Named("Vec".to_string(), vec![Type::named("uint8")]),
                    Type::Named("io.Error".to_string(), Vec::new()),
                ],
            ),
            classes,
        ),
        ("net.UnixStream", "write_all") => direct_type(
            &Type::Named(
                "Result".to_string(),
                vec![Type::Unit, Type::Named("io.Error".to_string(), Vec::new())],
            ),
            classes,
        ),
        ("net.UnixStream", "close") => Some(DirectType::Scalar(ScalarKind::Unit)),
        ("net.TlsListener", "accept") => direct_type(
            &Type::Named(
                "Result".to_string(),
                vec![
                    Type::Named("net.TlsStream".to_string(), Vec::new()),
                    Type::Named("io.Error".to_string(), Vec::new()),
                ],
            ),
            classes,
        ),
        ("net.TlsListener", "local_addr") => direct_type(
            &Type::Named(
                "Result".to_string(),
                vec![
                    Type::named("String"),
                    Type::Named("io.Error".to_string(), Vec::new()),
                ],
            ),
            classes,
        ),
        ("net.TlsListener", "close") => Some(DirectType::Scalar(ScalarKind::Unit)),
        ("net.TlsStream", "read_line") => direct_type(
            &Type::Named(
                "Result".to_string(),
                vec![
                    Type::Named("Option".to_string(), vec![Type::named("String")]),
                    Type::Named("io.Error".to_string(), Vec::new()),
                ],
            ),
            classes,
        ),
        ("net.TlsStream", "read_exact") => direct_type(
            &Type::Named(
                "Result".to_string(),
                vec![
                    Type::Named("Vec".to_string(), vec![Type::named("uint8")]),
                    Type::Named("io.Error".to_string(), Vec::new()),
                ],
            ),
            classes,
        ),
        ("net.TlsStream", "write_all") => direct_type(
            &Type::Named(
                "Result".to_string(),
                vec![Type::Unit, Type::Named("io.Error".to_string(), Vec::new())],
            ),
            classes,
        ),
        ("net.TlsStream", "close") => Some(DirectType::Scalar(ScalarKind::Unit)),
        _ => None,
    }
}

fn infer_variant_payload_type(
    scrutinee: &Operand,
    index: usize,
    variable_types: &HashMap<String, DirectType>,
    classes: &HashMap<String, MirClass>,
) -> Option<DirectType> {
    let scrutinee_ty = infer_operand_type(scrutinee, variable_types, classes)?;
    let DirectType::Opaque(Type::Named(name, args)) = scrutinee_ty else {
        return None;
    };
    let payload_ty = match (name.as_str(), args.as_slice(), index) {
        ("Option", [inner], 0) => inner.clone(),
        ("Result", [ok, _], 0) => ok.clone(),
        ("SendError", [inner], 0) => inner.clone(),
        _ => return None,
    };
    direct_type(&payload_ty, classes)
}

fn infer_try_type(
    value: &Operand,
    variable_types: &HashMap<String, DirectType>,
    classes: &HashMap<String, MirClass>,
) -> Option<DirectType> {
    let value_ty = infer_operand_type(value, variable_types, classes)?;
    let DirectType::Opaque(Type::Named(name, args)) = value_ty else {
        return None;
    };
    match (name.as_str(), args.as_slice()) {
        ("Result", [ok, _]) => direct_type(ok, classes),
        _ => None,
    }
}

fn direct_field_type(
    ty: &DirectType,
    field: &str,
    classes: &HashMap<String, MirClass>,
) -> Option<DirectType> {
    if let Some((_, _, field_ty)) = ty.field_slice(field) {
        return Some(field_ty);
    }
    let DirectType::Opaque(Type::Named(class_name, args)) = ty else {
        return None;
    };
    if class_name == "MapEntry" {
        return match (field, args.as_slice()) {
            ("key", [key, _value]) => direct_type(key, classes),
            ("value", [_key, value]) => direct_type(value, classes),
            _ => None,
        };
    }
    if !args.is_empty() {
        return None;
    }
    let class = classes.get(class_name)?;
    let field_info = class
        .fields
        .iter()
        .find(|candidate| candidate.name == field)?;
    direct_type(&field_info.ty, classes)
}

fn infer_operand_type(
    operand: &Operand,
    variable_types: &HashMap<String, DirectType>,
    classes: &HashMap<String, MirClass>,
) -> Option<DirectType> {
    match operand {
        Operand::Place(place) => {
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
                Some(DirectType::Scalar(ScalarKind::Int32))
            } else {
                Some(DirectType::Opaque(Type::named("Unknown")))
            }
        }
        Operand::Float(_) => Some(DirectType::Scalar(ScalarKind::Float64)),
        Operand::Bool(_) => Some(DirectType::Scalar(ScalarKind::Bool)),
        Operand::Unit => Some(DirectType::Scalar(ScalarKind::Unit)),
        Operand::String(_) => Some(DirectType::Opaque(Type::named("String"))),
        Operand::Duration(_) => Some(DirectType::Opaque(Type::named("Duration"))),
    }
}

fn render_direct_type(ty: &DirectType) -> String {
    match ty {
        DirectType::Scalar(ScalarKind::Int32) => "int32".to_string(),
        DirectType::Scalar(ScalarKind::Float32) => "float32".to_string(),
        DirectType::Scalar(ScalarKind::Float64) => "float64".to_string(),
        DirectType::Scalar(ScalarKind::Bool) => "bool".to_string(),
        DirectType::Scalar(ScalarKind::Unit) => "None".to_string(),
        DirectType::PlainClass(class) => class.class_name.clone(),
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
        let name = format!("aurora_data_{}", codegen.string_data.len());
        let id = codegen
            .object
            .declare_data(&name, Linkage::Local, false, false)
            .map_err(|error| format!("failed to declare string data: {}", error))?;
        let mut data = DataDescription::new();
        data.define(bytes.to_vec().into_boxed_slice());
        codegen
            .object
            .define_data(id, &data)
            .map_err(|error| format!("failed to define string data: {}", error))?;
        codegen.string_data.insert(bytes.to_vec(), id);
        id
    };
    let global = codegen.object.declare_data_in_func(id, builder.func);
    let ptr = builder.ins().symbol_value(types::I64, global);
    let len = builder.ins().iconst(types::I64, bytes.len() as i64);
    Ok((ptr, len))
}

fn box_thunk_value(
    codegen: &mut NativeCodegen<'_>,
    builder: &mut FunctionBuilder<'_>,
    values: &[Value],
    ty: &DirectType,
) -> std::result::Result<Value, String> {
    match ty {
        DirectType::Opaque(_) => values.first().copied().ok_or_else(|| {
            format!(
                "spawn thunk expected an opaque value for `{}`",
                render_direct_type(ty)
            )
        }),
        DirectType::Scalar(ScalarKind::Int32) => {
            let box_i64 = codegen
                .object
                .declare_func_in_func(codegen.box_i64, builder.func);
            let inst = builder.ins().call(box_i64, &[values[0]]);
            Ok(builder.inst_results(inst)[0])
        }
        DirectType::Scalar(ScalarKind::Float32) | DirectType::Scalar(ScalarKind::Float64) => {
            let box_f64 = codegen
                .object
                .declare_func_in_func(codegen.box_f64, builder.func);
            let inst = builder.ins().call(box_f64, &[values[0]]);
            Ok(builder.inst_results(inst)[0])
        }
        DirectType::Scalar(ScalarKind::Bool) => {
            let box_bool = codegen
                .object
                .declare_func_in_func(codegen.box_bool, builder.func);
            let inst = builder.ins().call(box_bool, &[values[0]]);
            Ok(builder.inst_results(inst)[0])
        }
        DirectType::Scalar(ScalarKind::Unit) => {
            let box_unit = codegen
                .object
                .declare_func_in_func(codegen.box_unit, builder.func);
            let inst = builder.ins().call(box_unit, &[]);
            Ok(builder.inst_results(inst)[0])
        }
        DirectType::PlainClass(class) => {
            let instance_empty = codegen
                .object
                .declare_func_in_func(codegen.instance_empty, builder.func);
            let instance_set_field = codegen
                .object
                .declare_func_in_func(codegen.instance_set_field, builder.func);
            let (class_ptr, class_len) =
                thunk_string_constant(codegen, builder, class.class_name.as_bytes())?;
            let init = builder.ins().call(instance_empty, &[class_ptr, class_len]);
            let mut current = builder.inst_results(init)[0];
            let mut start = 0usize;
            for field in &class.fields {
                let end = start + field.ty.value_count();
                let field_value =
                    box_thunk_value(codegen, builder, &values[start..end], &field.ty)?;
                let (field_ptr, field_len) =
                    thunk_string_constant(codegen, builder, field.name.as_bytes())?;
                let inst = builder.ins().call(
                    instance_set_field,
                    &[current, field_ptr, field_len, field_value],
                );
                current = builder.inst_results(inst)[0];
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
        DirectType::Opaque(_) => Ok(vec![raw]),
        DirectType::Scalar(ScalarKind::Int32) => {
            let unbox_i64 = codegen
                .object
                .declare_func_in_func(codegen.unbox_i64, builder.func);
            let inst = builder.ins().call(unbox_i64, &[raw]);
            Ok(builder.inst_results(inst).to_vec())
        }
        DirectType::Scalar(ScalarKind::Float32) | DirectType::Scalar(ScalarKind::Float64) => {
            let unbox_f64 = codegen
                .object
                .declare_func_in_func(codegen.unbox_f64, builder.func);
            let inst = builder.ins().call(unbox_f64, &[raw]);
            Ok(builder.inst_results(inst).to_vec())
        }
        DirectType::Scalar(ScalarKind::Bool) => {
            let unbox_bool = codegen
                .object
                .declare_func_in_func(codegen.unbox_bool, builder.func);
            let inst = builder.ins().call(unbox_bool, &[raw]);
            Ok(builder.inst_results(inst).to_vec())
        }
        DirectType::Scalar(ScalarKind::Unit) => Ok(vec![builder.ins().iconst(types::I64, 0)]),
        DirectType::PlainClass(class) => {
            let instance_get_field = codegen
                .object
                .declare_func_in_func(codegen.instance_get_field, builder.func);
            let mut values = Vec::new();
            for field in &class.fields {
                let (field_ptr, field_len) =
                    thunk_string_constant(codegen, builder, field.name.as_bytes())?;
                let inst = builder
                    .ins()
                    .call(instance_get_field, &[raw, field_ptr, field_len]);
                let field_raw = builder.inst_results(inst)[0];
                values.extend(unbox_thunk_value(codegen, builder, field_raw, &field.ty)?);
            }
            Ok(values)
        }
    }
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

fn mangle_symbol(name: &str) -> String {
    let mut mangled = String::from("aurora_fn_");
    for ch in name.chars() {
        if ch.is_ascii_alphanumeric() {
            mangled.push(ch);
        } else {
            mangled.push('_');
        }
    }
    mangled
}

fn mangle_thunk_symbol(name: &str) -> String {
    let mut mangled = String::from("aurora_thunk_");
    for ch in name.chars() {
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
        DirectType::Scalar(ScalarKind::Float32) => Type::named("float32"),
        DirectType::Scalar(ScalarKind::Float64) => Type::named("float64"),
        DirectType::Scalar(ScalarKind::Bool) => Type::named("bool"),
        DirectType::Scalar(ScalarKind::Unit) => Type::Unit,
        DirectType::PlainClass(class) => Type::named(&class.class_name),
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
        Type::TypeParam(_) => true,
        Type::Named(name, _) if name == "Unknown" => true,
        Type::Named(_, args) => args.iter().any(runtime_type_is_wildcard),
        Type::Unit | Type::Module(_) => false,
    }
}

fn collect_spawn_targets(module: &MirModule) -> BTreeSet<String> {
    let mut targets = BTreeSet::new();
    for function in module.functions.iter().chain(module.top_level.iter()) {
        for block in &function.blocks {
            for instruction in &block.instructions {
                if let Instruction::Assign {
                    value: Rvalue::Spawn { function, .. },
                    ..
                } = instruction
                {
                    targets.insert(function.clone());
                }
            }
        }
    }
    targets
}

#[cfg(test)]
#[path = "native_codegen_tests.rs"]
mod tests;
