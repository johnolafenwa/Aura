#![cfg(coverage)]

use aurora_compiler::ast::ReceiverKind;
use aurora_compiler::native_runtime_coverage::*;
use aurora_compiler::sema::{FunctionParamContract, Type};
use aurora_compiler::Value;
use std::ffi::c_void;
use std::fs;
use std::io::{Read, Write};
use std::net::TcpListener;
use std::ptr;
use std::thread;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

extern "C-unwind" {
    fn aurora_direct_ffi_call(
        spec_ptr: *const u8,
        spec_len: i64,
        args_ptr: *const i64,
        arg_count: i64,
    ) -> *mut c_void;
}

fn append_direct_ffi_text(encoded: &mut Vec<u8>, text: &str) {
    encoded.extend_from_slice(&(text.len() as u32).to_le_bytes());
    encoded.extend_from_slice(text.as_bytes());
}

fn append_direct_ffi_type(encoded: &mut Vec<u8>, code: u8, nominal_name: &str) {
    encoded.push(code);
    append_direct_ffi_text(encoded, nominal_name);
}

fn direct_ffi_spec(symbol: &str, params: &[(u8, u8, &str)], result: (u8, &str)) -> Vec<u8> {
    let mut encoded = b"AUFI".to_vec();
    encoded.push(0);
    append_direct_ffi_text(&mut encoded, symbol);
    encoded.extend_from_slice(&(params.len() as u32).to_le_bytes());
    for (passing, ty, nominal_name) in params {
        encoded.push(*passing);
        append_direct_ffi_type(&mut encoded, *ty, nominal_name);
    }
    append_direct_ffi_type(&mut encoded, result.0, result.1);
    encoded
}

unsafe fn call_direct_ffi_adapter(spec: &[u8], arguments: &[*mut OpaqueValue]) -> *mut OpaqueValue {
    let arguments = arguments
        .iter()
        .map(|argument| *argument as i64)
        .collect::<Vec<_>>();
    unsafe {
        aurora_direct_ffi_call(
            spec.as_ptr(),
            spec.len() as i64,
            arguments.as_ptr(),
            arguments.len() as i64,
        )
        .cast()
    }
}

#[no_mangle]
unsafe extern "C" fn aurora_coverage_ffi_scalars(
    boolean: u8,
    i8_value: i8,
    i16_value: i16,
    i32_value: i32,
    i64_value: i64,
    u8_value: u8,
    u16_value: u16,
    u32_value: u32,
    u64_value: u64,
    f32_value: f32,
    f64_value: f64,
) -> f64 {
    if boolean == 1
        && i8_value == -8
        && i16_value == -1_600
        && i32_value == -32_000
        && i64_value == -64_000
        && u8_value == 8
        && u16_value == 1_600
        && u32_value == 32_000
        && u64_value == 64_000
        && f32_value == 3.0
        && f64_value == 7.0
    {
        42.5
    } else {
        -1.0
    }
}

#[no_mangle]
unsafe extern "C" fn aurora_coverage_ffi_bool_not(value: u8) -> u8 {
    u8::from(value == 0)
}

#[no_mangle]
unsafe extern "C" fn aurora_coverage_ffi_views(
    text: *const u8,
    text_len: usize,
    bytes: *const u8,
    bytes_len: usize,
    mutable_bytes: *mut u8,
    mutable_bytes_len: usize,
) -> u64 {
    let text = unsafe { std::slice::from_raw_parts(text, text_len) };
    let bytes = unsafe { std::slice::from_raw_parts(bytes, bytes_len) };
    let mutable_bytes = unsafe { std::slice::from_raw_parts_mut(mutable_bytes, mutable_bytes_len) };
    if text != b"ffi" || bytes != [1, 2, 3] {
        return 0;
    }
    for byte in mutable_bytes.iter_mut() {
        *byte = byte.wrapping_add(1);
    }
    mutable_bytes.iter().map(|byte| u64::from(*byte)).sum()
}

static AURORA_COVERAGE_HANDLE: u8 = 1;

#[no_mangle]
unsafe extern "C" fn aurora_coverage_ffi_new_handle() -> *mut c_void {
    std::ptr::from_ref(&AURORA_COVERAGE_HANDLE)
        .cast_mut()
        .cast()
}

#[no_mangle]
unsafe extern "C" fn aurora_coverage_ffi_consume_handle(handle: *mut c_void) {
    assert_eq!(
        handle,
        std::ptr::from_ref(&AURORA_COVERAGE_HANDLE)
            .cast_mut()
            .cast()
    );
}

unsafe fn release(value: *mut OpaqueValue) {
    if !value.is_null() {
        unsafe {
            aurora_direct_release_value(value);
        }
    }
}

unsafe fn int_value(value: i64) -> *mut OpaqueValue {
    aurora_direct_box_i64(value)
}

unsafe fn bool_value(value: bool) -> *mut OpaqueValue {
    aurora_direct_box_bool(i64::from(value))
}

unsafe fn float_value(value: i64, runtime_type: &str) -> *mut OpaqueValue {
    let integer = unsafe { int_value(value) };
    let float = aurora_direct_cast_value(integer, runtime_type.as_ptr(), runtime_type.len());
    unsafe {
        release(integer);
    }
    float
}

unsafe fn duration_value(value: i64) -> *mut OpaqueValue {
    let nanoseconds = (value as i128) * 1_000_000;
    aurora_direct_duration_literal(nanoseconds as i64, (nanoseconds >> 64) as i64)
}

unsafe fn string_value(value: &str) -> *mut OpaqueValue {
    aurora_direct_string_literal(value.as_ptr(), value.len())
}

unsafe fn enum_unit(enum_name: &str, variant_name: &str) -> *mut OpaqueValue {
    aurora_direct_enum_variant(
        enum_name.as_ptr(),
        enum_name.len(),
        variant_name.as_ptr(),
        variant_name.len(),
        ptr::null_mut(),
        0,
    )
}

unsafe fn expect_i64(value: *mut OpaqueValue) -> i64 {
    let unboxed = aurora_direct_unbox_i64(value);
    unsafe {
        release(value);
    }
    unboxed
}

unsafe fn expect_bool(value: *mut OpaqueValue) -> bool {
    let unboxed = aurora_direct_unbox_bool(value) != 0;
    unsafe {
        release(value);
    }
    unboxed
}

unsafe fn cloned_value(value: *mut OpaqueValue) -> Value {
    unsafe { aurora_direct_coverage_clone_value(value) }
}

unsafe fn expect_string(value: *mut OpaqueValue) -> String {
    let text = match unsafe { cloned_value(value) } {
        Value::String(text) => text,
        other => panic!("expected string, found {other:?}"),
    };
    unsafe {
        release(value);
    }
    text
}

unsafe fn expect_variant_payload(
    value: *mut OpaqueValue,
    enum_name: &str,
    variant_name: &str,
) -> *mut OpaqueValue {
    match unsafe { cloned_value(value) } {
        Value::EnumVariant(variant)
            if variant.enum_name == enum_name && variant.variant_name == variant_name => {}
        other => panic!("expected {enum_name}.{variant_name}, found {other:?}"),
    }
    let payload = aurora_direct_variant_payload(value, 0);
    unsafe {
        release(value);
    }
    payload
}

unsafe fn expect_result_ok_payload(value: *mut OpaqueValue) -> *mut OpaqueValue {
    unsafe { expect_variant_payload(value, "Result", "Ok") }
}

unsafe fn expect_option_some_payload(value: *mut OpaqueValue) -> *mut OpaqueValue {
    unsafe { expect_variant_payload(value, "Option", "Some") }
}

unsafe fn expect_result_ok_string(value: *mut OpaqueValue) -> String {
    let payload = unsafe { expect_result_ok_payload(value) };
    unsafe { expect_string(payload) }
}

unsafe fn expect_result_ok_unit(value: *mut OpaqueValue) {
    let payload = unsafe { expect_result_ok_payload(value) };
    match unsafe { cloned_value(payload) } {
        Value::Unit => {}
        other => panic!("expected unit payload, found {other:?}"),
    }
    unsafe {
        release(payload);
    }
}

unsafe fn string_vec(values: &[&str]) -> *mut OpaqueValue {
    let vec = aurora_direct_vec_empty();
    for value in values {
        let item = unsafe { string_value(value) };
        release(aurora_direct_vec_push_in_place(vec, item));
    }
    vec
}

unsafe fn byte_vec(values: &[u8]) -> *mut OpaqueValue {
    let vec = aurora_direct_vec_empty();
    for value in values {
        let item = unsafe { int_value(i64::from(*value)) };
        let runtime_type = "uint8";
        aurora_direct_tag_value_type(item, runtime_type.as_ptr(), runtime_type.len());
        release(aurora_direct_vec_push_in_place(vec, item));
    }
    vec
}

fn unique_temp_path(name: &str) -> String {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system clock should be after unix epoch")
        .as_nanos();
    std::env::temp_dir()
        .join(format!(
            "aurora-native-runtime-ffi-{name}-{}-{nanos}",
            std::process::id()
        ))
        .to_string_lossy()
        .to_string()
}

#[test]
fn direct_ffi_adapter_executes_every_v0_boundary_kind_through_the_library_copy() {
    // Keep every test-owned C symbol in the executable's process-global symbol
    // table. The adapter still resolves by name, exactly as generated Aurora
    // programs do.
    std::hint::black_box(aurora_coverage_ffi_scalars as *const ());
    std::hint::black_box(aurora_coverage_ffi_bool_not as *const ());
    std::hint::black_box(aurora_coverage_ffi_views as *const ());
    std::hint::black_box(aurora_coverage_ffi_new_handle as *const ());
    std::hint::black_box(aurora_coverage_ffi_consume_handle as *const ());

    unsafe {
        let scalar_spec = direct_ffi_spec(
            "aurora_coverage_ffi_scalars",
            &[
                (0, 1, ""),
                (0, 2, ""),
                (0, 3, ""),
                (0, 4, ""),
                (0, 5, ""),
                (0, 6, ""),
                (0, 7, ""),
                (0, 8, ""),
                (0, 9, ""),
                (0, 10, ""),
                (0, 11, ""),
            ],
            (11, ""),
        );
        let scalar_arguments = [
            bool_value(true),
            int_value(-8),
            int_value(-1_600),
            int_value(-32_000),
            int_value(-64_000),
            int_value(8),
            int_value(1_600),
            int_value(32_000),
            int_value(64_000),
            float_value(3, "float32"),
            float_value(7, "float64"),
        ];
        let scalar_result = call_direct_ffi_adapter(&scalar_spec, &scalar_arguments);
        assert_eq!(cloned_value(scalar_result), Value::Float(42.5));
        for argument in scalar_arguments {
            release(argument);
        }
        release(scalar_result);

        let bool_spec = direct_ffi_spec("aurora_coverage_ffi_bool_not", &[(0, 1, "")], (1, ""));
        let truth = bool_value(true);
        let inverted = call_direct_ffi_adapter(&bool_spec, &[truth]);
        assert_eq!(cloned_value(inverted), Value::Bool(false));
        release(truth);
        release(inverted);

        let views_spec = direct_ffi_spec(
            "aurora_coverage_ffi_views",
            &[(0, 12, ""), (0, 13, ""), (1, 14, "")],
            (9, ""),
        );
        let text = string_value("ffi");
        let bytes = byte_vec(&[1, 2, 3]);
        let mutable_bytes = byte_vec(&[4, 5]);
        let sum = call_direct_ffi_adapter(&views_spec, &[text, bytes, mutable_bytes]);
        assert_eq!(expect_i64(sum), 11);
        assert_eq!(cloned_value(mutable_bytes).render(), "[5, 6]");
        release(text);
        release(bytes);
        release(mutable_bytes);

        let new_handle_spec = direct_ffi_spec(
            "aurora_coverage_ffi_new_handle",
            &[],
            (15, "CoverageHandle"),
        );
        let handle = call_direct_ffi_adapter(&new_handle_spec, &[]);
        assert_eq!(cloned_value(handle).render(), "<opaque CoverageHandle>");

        let consume_handle_spec = direct_ffi_spec(
            "aurora_coverage_ffi_consume_handle",
            &[(2, 15, "CoverageHandle")],
            (0, ""),
        );
        let consumed = call_direct_ffi_adapter(&consume_handle_spec, &[handle]);
        assert_eq!(cloned_value(consumed), Value::Unit);
        release(handle);
        release(consumed);
    }
}

#[test]
fn direct_callable_ffi_symbols_preserve_the_public_runtime_contract() {
    let signature = Type::Function {
        params: vec![
            FunctionParamContract {
                name: "path".to_string(),
                ty: Type::named("String"),
                passing: ReceiverKind::Borrow,
                has_default: false,
                default_erased: false,
            },
            FunctionParamContract {
                name: "buffer".to_string(),
                ty: Type::Named("Vec".to_string(), vec![Type::named("uint8")]),
                passing: ReceiverKind::BorrowMut,
                has_default: true,
                default_erased: false,
            },
        ],
        return_type: Box::new(Type::named("int64")),
    };
    let encoded_signature =
        serde_json::to_vec(&signature).expect("function signature should serialize");
    let name = b"read_into";
    let path = b"/workspace/io.au";

    let function = aurora_direct_function_value(
        0x1234,
        0x5678,
        name.as_ptr(),
        name.len(),
        encoded_signature.as_ptr(),
        encoded_signature.len(),
        path.as_ptr(),
        path.len(),
        9,
        4,
    );

    assert_eq!(aurora_direct_function_thunk(function), 0x1234);
    assert_eq!(aurora_direct_function_default_binder(function), 0x5678);
    match unsafe { cloned_value(function) } {
        Value::Function(value) => {
            assert_eq!(value.name, "read_into");
            assert_eq!(value.signature, signature);
            assert_eq!(value.source_path.as_deref(), Some("/workspace/io.au"));
            assert_eq!(value.entry_span.line, 9);
            assert_eq!(value.entry_span.column, 4);
        }
        other => panic!("expected function value, found {other:?}"),
    }
    unsafe {
        release(function);
    }
}

#[test]
fn direct_random_ffi_symbols_preserve_the_public_runtime_contract() {
    unsafe {
        let integers = aurora_direct_rng_new(42);
        assert_eq!(aurora_direct_rng_next_int(integers, 0, 10), 2);
        assert_eq!(aurora_direct_rng_next_int(integers, -5, 6), 2);
        assert_eq!(
            aurora_direct_rng_next_int(integers, i64::MIN, i64::MAX),
            3_321_214_725_393_783_201
        );
        release(integers);

        let floats = aurora_direct_rng_new(42);
        assert_eq!(
            aurora_direct_rng_next_float(floats),
            0.083_862_971_059_882_16
        );
        release(floats);

        let shuffle_rng = aurora_direct_rng_new(42);
        let values = string_vec(&["a", "b", "c", "d", "e", "f"]);
        aurora_direct_rng_shuffle(shuffle_rng, values);
        match cloned_value(values) {
            Value::Vec(vector) => assert_eq!(
                vector
                    .elements
                    .into_iter()
                    .map(|value| value.render())
                    .collect::<Vec<_>>(),
                ["d", "f", "e", "b", "c", "a"]
            ),
            other => panic!("expected shuffled vector, found {other:?}"),
        }
        release(values);
        release(shuffle_rng);

        assert_eq!(aurora_direct_random_secure_int(5, 6), 5);
        let bytes = aurora_direct_random_secure_bytes(0);
        match cloned_value(bytes) {
            Value::Vec(vector) => {
                assert_eq!(vector.element_type.to_string(), "uint8");
                assert!(vector.elements.is_empty());
            }
            other => panic!("expected secure byte vector, found {other:?}"),
        }
        release(bytes);
    }
}

#[test]
fn direct_runtime_exported_ffi_symbols_execute_through_the_library_copy() {
    unsafe {
        let negated = aurora_direct_unary_value(0, int_value(7));
        assert_eq!(expect_i64(negated), -7);

        let inverted = aurora_direct_unary_value_at(1, bool_value(false), 1, 1);
        assert!(expect_bool(inverted));

        let sum = aurora_direct_binary_value(0, int_value(20), int_value(22));
        assert_eq!(expect_i64(sum), 42);

        let floor = aurora_direct_binary_value(13, int_value(-7), int_value(3));
        assert_eq!(expect_i64(floor), -3);

        let ordered = aurora_direct_binary_value_at(7, int_value(2), int_value(3), 1, 1);
        assert!(expect_bool(ordered));

        let floor_at = aurora_direct_binary_value_at(13, int_value(7), int_value(-3), 1, 1);
        assert_eq!(expect_i64(floor_at), -3);

        let duration = aurora_direct_duration_from_i64(1_500, 1_000_000);
        assert_eq!(cloned_value(duration), Value::Duration(1_500_000_000));
        assert_eq!(
            aurora_direct_duration_to_float(duration, 1_000_000),
            1_500.0
        );
        assert_eq!(
            aurora_direct_duration_to_float(duration, 1_000_000_000),
            1.5
        );
        release(duration);

        let zero_duration = duration_value(0);
        let slept = aurora_direct_sleep_value(zero_duration);
        assert_eq!(cloned_value(slept), Value::Unit);
        release(slept);
        release(zero_duration);

        let zero_duration = duration_value(0);
        aurora_direct_sleep_value_void(zero_duration);
        release(zero_duration);

        aurora_direct_yield_now();

        let first_monotonic_ms = aurora_direct_monotonic_time_ms();
        let second_monotonic_ms = aurora_direct_monotonic_time_ms();
        assert!(
            second_monotonic_ms >= first_monotonic_ms,
            "the exported direct monotonic clock must not move backwards"
        );

        let cast_target = "float64";
        let cast = aurora_direct_cast_value(int_value(5), cast_target.as_ptr(), cast_target.len());
        release(cast);

        let wide_signed = aurora_direct_box_i64(i64::MIN);
        assert_eq!(aurora_direct_unbox_int64(wide_signed), i64::MIN);
        release(wide_signed);

        let wide_unsigned = aurora_direct_box_u64(u64::MAX);
        assert_eq!(aurora_direct_unbox_u64(wide_unsigned), u64::MAX);
        assert_eq!(
            aurora_direct_integer_to_float(wide_unsigned),
            u64::MAX as f64
        );
        release(wide_unsigned);

        assert_eq!(
            aurora_direct_cast_integer_to_integer(u64::MAX, 1, 2, 1, 1),
            u64::MAX
        );
        assert_eq!(
            aurora_direct_cast_integer_to_float(1_u64 << 63, 1, 1, 1, 1),
            (1_u64 << 63) as f64
        );
        assert_eq!(
            aurora_direct_cast_float_to_integer(4_294_967_296.75, 2, 1, 1),
            4_294_967_296
        );

        let truthy = int_value(9);
        assert_eq!(aurora_direct_value_as_condition(truthy), 1);
        release(truthy);

        let unicode = string_value("é🎉e\u{301}");
        assert_eq!(aurora_direct_string_len(unicode), 4);
        assert_eq!(aurora_direct_string_byte_len(unicode), 9);
        assert_eq!(
            expect_string(aurora_direct_string_slice(unicode, 1, 1, -1, 1, 1, 1)),
            "🎉e"
        );
        release(unicode);

        let write_arg = string_value("");
        let write_result = aurora_direct_io_write(write_arg);
        release(write_arg);
        release(write_result);
        release(aurora_direct_io_flush());

        let fs_dir = unique_temp_path("dir");
        let fs_dir_value = string_value(&fs_dir);
        release(aurora_direct_fs_create_dir(fs_dir_value));
        release(fs_dir_value);
        let fs_file = format!("{fs_dir}/out.txt");
        let append_path = string_value(&fs_file);
        let append_text = string_value("hello");
        release(aurora_direct_fs_append_string(append_path, append_text));
        release(append_path);
        release(append_text);
        let read_dir_path = string_value(&fs_dir);
        release(aurora_direct_fs_read_dir(read_dir_path));
        release(read_dir_path);
        let create_file = format!("{fs_dir}/created.txt");
        let create_path = string_value(&create_file);
        release(aurora_direct_fs_create(create_path));
        release(create_path);
        let file_path = format!("{fs_dir}/ffi-file.txt");
        let file_path_value = string_value(&file_path);
        let created = aurora_direct_fs_create(file_path_value);
        release(file_path_value);
        let file = aurora_direct_variant_payload(created, 0);
        release(created);
        let file_text = string_value("file text");
        release(aurora_direct_file_write_all(file, file_text));
        release(file_text);
        release(aurora_direct_file_flush(file));
        release(aurora_direct_file_close(file));
        release(file);
        let file_path_value = string_value(&file_path);
        let opened = aurora_direct_fs_open(file_path_value);
        release(file_path_value);
        let file = aurora_direct_variant_payload(opened, 0);
        release(opened);
        release(aurora_direct_file_read_all(file));
        release(aurora_direct_file_close(file));
        release(file);
        fs::remove_dir_all(&fs_dir).expect("temporary direct runtime ffi dir should be removable");

        let map = aurora_direct_map_empty();
        let map_key = string_value("answer");
        let map_value = int_value(42);
        release(aurora_direct_map_set_index_in_place(
            map, map_key, map_value, 1, 1,
        ));
        let lookup_key = string_value("answer");
        assert_eq!(
            expect_i64(aurora_direct_map_index(map, lookup_key, 1, 1)),
            42
        );
        release(lookup_key);
        release(map);

        let vec = aurora_direct_vec_empty();
        assert_eq!(aurora_direct_vec_is_empty(vec), 1);
        release(aurora_direct_vec_push_in_place(vec, int_value(1)));
        release(aurora_direct_vec_push_in_place(vec, int_value(3)));
        assert_eq!(aurora_direct_vec_len(vec), 2);
        assert_eq!(aurora_direct_vec_contains(vec, int_value(1)), 1);
        release(aurora_direct_vec_get(vec, 0));
        assert_eq!(expect_i64(aurora_direct_vec_index(vec, 1, 1, 1)), 3);
        release(aurora_direct_vec_index_option(vec, 8));
        release(aurora_direct_vec_set_in_place(vec, 1, int_value(4)));
        assert_eq!(expect_i64(aurora_direct_vec_index(vec, 1, 1, 1)), 4);
        release(aurora_direct_vec_set_index_in_place(
            vec,
            0,
            int_value(0),
            1,
            1,
        ));
        assert_eq!(aurora_direct_vec_insert_in_place(vec, 1, int_value(2)), 1);
        assert_eq!(aurora_direct_vec_swap_in_place(vec, 0, 2), 1);
        let vec_slice = aurora_direct_vec_slice(vec, 0, 0, -1, 1, 1, 1);
        assert_eq!(cloned_value(vec_slice).render(), "[4, 2]");
        release(vec_slice);
        release(aurora_direct_vec_reverse_in_place(vec));
        release(aurora_direct_vec_remove_in_place(vec, 1));
        release(aurora_direct_vec_pop_in_place(vec));
        let other_vec = aurora_direct_vec_empty();
        release(aurora_direct_vec_push_in_place(other_vec, int_value(9)));
        release(aurora_direct_vec_extend_in_place(vec, other_vec));
        release(aurora_direct_vec_clear_in_place(vec));
        assert_eq!(aurora_direct_vec_is_empty(vec), 1);
        release(vec);

        let map = aurora_direct_map_empty();
        assert_eq!(aurora_direct_map_is_empty(map), 1);
        release(aurora_direct_map_set_in_place(
            map,
            string_value("left"),
            int_value(1),
        ));
        release(aurora_direct_map_set_in_place(
            map,
            string_value("right"),
            int_value(2),
        ));
        assert_eq!(aurora_direct_map_len(map), 2);
        assert_eq!(aurora_direct_map_contains_key(map, string_value("left")), 1);
        release(aurora_direct_map_get(map, string_value("left")));
        assert_eq!(
            expect_i64(aurora_direct_map_index(map, string_value("right"), 1, 1)),
            2
        );
        release(aurora_direct_map_keys(map));
        release(aurora_direct_map_values(map));
        release(aurora_direct_map_items(map));
        release(aurora_direct_map_entries(map));
        release(aurora_direct_map_remove_in_place(
            map,
            string_value("missing"),
        ));
        let other_map = aurora_direct_map_empty();
        release(aurora_direct_map_set_in_place(
            other_map,
            string_value("extra"),
            int_value(5),
        ));
        release(aurora_direct_map_extend_in_place(map, other_map));
        assert_eq!(aurora_direct_map_len(map), 3);
        release(aurora_direct_map_clear_in_place(map));
        assert_eq!(aurora_direct_map_is_empty(map), 1);
        release(map);

        let sys_args_name = b"sys::args";
        let sys_args = aurora_direct_host_builtin(
            sys_args_name.as_ptr(),
            sys_args_name.len(),
            aurora_direct_arg_buffer_new(0),
            0,
        );
        let expected_args = std::env::args().skip(1).collect::<Vec<_>>();
        let actual_args = match cloned_value(sys_args) {
            Value::Vec(values) => values
                .elements
                .into_iter()
                .map(|value| match value {
                    Value::String(value) => value,
                    other => panic!("sys.args returned non-string element: {other:?}"),
                })
                .collect::<Vec<_>>(),
            other => panic!("sys.args returned non-vector value: {other:?}"),
        };
        assert_eq!(actual_args, expected_args);
        release(sys_args);

        let set = aurora_direct_set_empty();
        assert_eq!(aurora_direct_set_is_empty(set), 1);
        assert_eq!(aurora_direct_set_insert_in_place(set, int_value(1)), 1);
        assert_eq!(aurora_direct_set_insert_in_place(set, int_value(1)), 0);
        assert_eq!(aurora_direct_set_len(set), 1);
        assert_eq!(aurora_direct_set_contains(set, int_value(1)), 1);
        release(aurora_direct_set_index_option(set, 0));
        assert_eq!(aurora_direct_set_remove_in_place(set, int_value(1)), 1);
        assert_eq!(aurora_direct_set_is_empty(set), 1);
        release(set);

        let command = aurora_direct_vec_empty();
        release(aurora_direct_vec_push_in_place(
            command,
            string_value("/bin/sh"),
        ));
        release(aurora_direct_vec_push_in_place(command, string_value("-c")));
        release(aurora_direct_vec_push_in_place(
            command,
            string_value("printf out; printf err >&2"),
        ));
        let cwd = enum_unit("Option", "None");
        let env = aurora_direct_map_empty();
        let stdin = aurora_direct_process_null();
        let stdout = aurora_direct_process_null();
        let stderr = aurora_direct_process_null();
        let timeout: *mut OpaqueValue = ptr::null_mut();
        let group = bool_value(false);
        let completed_result =
            aurora_direct_process_run(command, cwd, env, stdin, stdout, stderr, timeout, group);
        release(command);
        release(cwd);
        release(env);
        release(stdin);
        release(stdout);
        release(stderr);
        release(group);
        let completed = aurora_direct_variant_payload(completed_result, 0);
        release(completed_result);
        assert_eq!(aurora_direct_process_completed_success(completed), 1);
        release(aurora_direct_process_completed_status(completed));
        release(aurora_direct_process_completed_stdout(completed));
        release(aurora_direct_process_completed_stderr(completed));
        release(aurora_direct_process_completed_stdout_bytes(completed));
        release(aurora_direct_process_completed_stderr_bytes(completed));
        release(aurora_direct_process_completed_check(completed));
        release(completed);

        let field_name = b"value";
        let field_names = [field_name.as_ptr()];
        let field_lens = [field_name.len()];
        let field_value = int_value(11);
        let field_values = [field_value];
        let instance = aurora_direct_instance_new(
            b"Counter".as_ptr(),
            "Counter".len(),
            field_names.as_ptr(),
            field_lens.as_ptr(),
            field_values.as_ptr(),
            field_values.len(),
        );
        assert_eq!(
            expect_i64(aurora_direct_instance_get_field(
                instance,
                field_name.as_ptr(),
                field_name.len(),
            )),
            11
        );
        release(field_value);
        release(instance);

        let capacity = int_value(1);
        let channel = aurora_direct_channel_new(capacity);
        release(capacity);
        let duration = duration_value(0);
        let sent = aurora_direct_channel_send_timeout_value(channel, int_value(7), duration);
        release(sent);
        release(duration);
        release(aurora_direct_channel_recv(channel));
        release(aurora_direct_channel_try_send(channel, int_value(8)));
        release(aurora_direct_channel_recv(channel));
        release(aurora_direct_close_value(channel, 0));
        release(channel);

        let task_list = aurora_direct_vec_empty();
        release(aurora_direct_wait_all(task_list));
        release(task_list);
        let task_list = aurora_direct_vec_empty();
        release(aurora_direct_wait_any(task_list));
        release(task_list);
        let task_list = aurora_direct_vec_empty();
        let timeout = duration_value(0);
        release(aurora_direct_wait_all_timeout_value(task_list, timeout));
        release(timeout);
        release(task_list);
        let task_list = aurora_direct_vec_empty();
        let timeout = duration_value(0);
        release(aurora_direct_wait_any_timeout_value(task_list, timeout));
        release(timeout);
        release(task_list);

        let source_buffer = aurora_direct_arg_buffer_new(1);
        aurora_direct_arg_buffer_store_owned(source_buffer, 0, duration_value(0) as i64);
        let sources = aurora_direct_tuple_new(source_buffer, 1);
        let deadline_index =
            expect_variant_payload(aurora_direct_select(sources), "SelectOutcome", "Deadline");
        assert_eq!(expect_i64(deadline_index), 0);

        aurora_direct_sleep_ms(0);
    }
}

#[test]
fn direct_runtime_resource_ffi_symbols_execute_through_the_library_copy() {
    unsafe {
        let command = string_vec(&["/bin/sh", "-c", "cat; printf err >&2"]);
        let cwd = enum_unit("Option", "None");
        let env = aurora_direct_map_empty();
        let stdin = aurora_direct_process_pipe();
        let stdout = aurora_direct_process_pipe();
        let stderr = aurora_direct_process_pipe();
        let group = bool_value(false);
        let child_result =
            aurora_direct_process_start(command, cwd, env, stdin, stdout, stderr, group);
        release(command);
        release(cwd);
        release(env);
        release(stdin);
        release(stdout);
        release(stderr);
        release(group);
        let child = expect_result_ok_payload(child_result);
        let child_stdin = expect_option_some_payload(aurora_direct_process_child_stdin(child));
        let child_stdout = expect_option_some_payload(aurora_direct_process_child_stdout(child));
        let child_stderr = expect_option_some_payload(aurora_direct_process_child_stderr(child));
        expect_result_ok_unit(aurora_direct_process_pipe_write_all(
            child_stdin,
            string_value("left"),
            duration_value(5_000),
        ));
        expect_result_ok_unit(aurora_direct_process_pipe_write_bytes(
            child_stdin,
            byte_vec(b"-right\n"),
            duration_value(5_000),
        ));
        expect_result_ok_unit(aurora_direct_process_pipe_flush(child_stdin));
        release(aurora_direct_process_pipe_close(child_stdin));
        assert_eq!(
            expect_result_ok_string(aurora_direct_process_pipe_read_all(child_stdout)),
            "left-right\n"
        );
        assert_eq!(
            expect_result_ok_string(aurora_direct_process_pipe_read_all(child_stderr)),
            "err"
        );
        release(aurora_direct_process_child_wait_or_none(
            child,
            duration_value(5_000),
        ));
        release(aurora_direct_process_child_wait(child, ptr::null_mut()));
        release(aurora_direct_process_child_wait_ok(child, ptr::null_mut()));
        release(aurora_direct_process_pipe_close(child_stdout));
        release(aurora_direct_process_pipe_close(child_stderr));
        release(aurora_direct_process_child_close(child));
        release(child);
        release(child_stdin);
        release(child_stdout);
        release(child_stderr);

        let listener =
            expect_result_ok_payload(aurora_direct_net_listen(string_value("127.0.0.1:0")));
        let tcp_address = expect_result_ok_string(aurora_direct_tcp_listener_local_addr(listener));
        let listener_handle = listener as usize;
        let tcp_server = thread::spawn(move || {
            let listener = listener_handle as *mut OpaqueValue;
            let accepted = expect_result_ok_payload(aurora_direct_tcp_listener_accept(
                listener,
                duration_value(5_000),
            ));
            assert_eq!(
                expect_result_ok_string(aurora_direct_tcp_stream_read_all(
                    accepted,
                    duration_value(5_000),
                )),
                "ping\n"
            );
            expect_result_ok_unit(aurora_direct_tcp_stream_write_all(
                accepted,
                string_value("pong-rest"),
                duration_value(5_000),
            ));
            expect_result_ok_unit(aurora_direct_tcp_stream_flush(accepted));
            expect_result_ok_unit(aurora_direct_tcp_stream_shutdown_write(accepted));
            release(aurora_direct_tcp_stream_close(accepted));
            release(accepted);
        });
        let tcp_client =
            expect_result_ok_payload(aurora_direct_net_connect(string_value(&tcp_address)));
        expect_result_ok_unit(aurora_direct_tcp_stream_write_bytes(
            tcp_client,
            byte_vec(b"ping\n"),
            duration_value(5_000),
        ));
        expect_result_ok_unit(aurora_direct_tcp_stream_shutdown_write(tcp_client));
        release(aurora_direct_tcp_stream_local_addr(tcp_client));
        release(aurora_direct_tcp_stream_peer_addr(tcp_client));
        release(aurora_direct_tcp_stream_read_exact(
            tcp_client,
            int_value(4),
            duration_value(5_000),
        ));
        assert_eq!(
            expect_result_ok_string(aurora_direct_tcp_stream_read_all(
                tcp_client,
                duration_value(5_000),
            )),
            "-rest"
        );
        release(aurora_direct_tcp_stream_shutdown_read(tcp_client));
        release(aurora_direct_tcp_stream_close(tcp_client));
        release(tcp_client);
        tcp_server
            .join()
            .expect("direct TCP FFI server should finish");
        release(aurora_direct_tcp_listener_close(listener));
        release(listener);

        let udp_sender =
            expect_result_ok_payload(aurora_direct_net_udp_bind(string_value("127.0.0.1:0")));
        let udp_receiver =
            expect_result_ok_payload(aurora_direct_net_udp_bind(string_value("127.0.0.1:0")));
        let udp_receiver_address =
            expect_result_ok_string(aurora_direct_udp_socket_local_addr(udp_receiver));
        expect_result_ok_unit(aurora_direct_udp_socket_send_bytes(
            udp_sender,
            string_value(&udp_receiver_address),
            byte_vec(b"hello"),
            duration_value(5_000),
        ));
        let datagram = expect_option_some_payload(expect_result_ok_payload(
            aurora_direct_udp_socket_recv_from(udp_receiver, int_value(64), duration_value(5_000)),
        ));
        let reply_address = expect_string(aurora_direct_udp_datagram_address(datagram));
        release(aurora_direct_udp_datagram_bytes(datagram));
        assert_eq!(
            expect_result_ok_string(aurora_direct_udp_datagram_text(datagram)),
            "hello"
        );
        expect_result_ok_unit(aurora_direct_udp_socket_send_bytes(
            udp_receiver,
            string_value(&reply_address),
            byte_vec(b"ok"),
            duration_value(5_000),
        ));
        release(expect_option_some_payload(expect_result_ok_payload(
            aurora_direct_udp_socket_recv(udp_sender, int_value(64), duration_value(5_000)),
        )));
        release(aurora_direct_udp_socket_close(udp_sender));
        release(aurora_direct_udp_socket_close(udp_receiver));
        release(udp_sender);
        release(udp_receiver);
        release(datagram);

        #[cfg(unix)]
        {
            let unix_path = format!(
                "/tmp/a-nrf-{}-{}.sock",
                std::process::id(),
                SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .expect("system clock should be after unix epoch")
                    .as_nanos()
                    % 1_000_000
            );
            let _ = fs::remove_file(&unix_path);
            let unix_listener =
                expect_result_ok_payload(aurora_direct_net_unix_listen(string_value(&unix_path)));
            let unix_listener_handle = unix_listener as usize;
            let unix_server = thread::spawn(move || {
                let unix_listener = unix_listener_handle as *mut OpaqueValue;
                let accepted = expect_result_ok_payload(aurora_direct_unix_listener_accept(
                    unix_listener,
                    duration_value(5_000),
                ));
                release(aurora_direct_unix_stream_read_exact(
                    accepted,
                    int_value(4),
                    duration_value(5_000),
                ));
                expect_result_ok_unit(aurora_direct_unix_stream_write_all(
                    accepted,
                    string_value("ok"),
                    duration_value(5_000),
                ));
                release(aurora_direct_unix_stream_close(accepted));
                release(accepted);
            });
            let unix_client =
                expect_result_ok_payload(aurora_direct_net_unix_connect(string_value(&unix_path)));
            expect_result_ok_unit(aurora_direct_unix_stream_write_all(
                unix_client,
                string_value("unix"),
                duration_value(5_000),
            ));
            release(aurora_direct_unix_stream_read_exact(
                unix_client,
                int_value(2),
                duration_value(5_000),
            ));
            release(aurora_direct_unix_stream_close(unix_client));
            release(unix_client);
            unix_server
                .join()
                .expect("direct Unix-socket FFI server should finish");
            release(aurora_direct_unix_listener_close(unix_listener));
            release(unix_listener);
            let _ = fs::remove_file(&unix_path);
        }

        let http_listener =
            TcpListener::bind("127.0.0.1:0").expect("HTTP fixture should bind locally");
        let http_address = http_listener
            .local_addr()
            .expect("HTTP fixture should expose a local address");
        let http_server = thread::spawn(move || {
            let (mut stream, _) = http_listener
                .accept()
                .expect("HTTP fixture should accept one request");
            let mut request = [0_u8; 512];
            let _ = stream
                .read(&mut request)
                .expect("HTTP fixture should read request");
            stream
                .write_all(
                    b"HTTP/1.1 201 Created\r\nTransfer-Encoding: chunked\r\nX-Test: yes\r\n\r\n",
                )
                .expect("HTTP fixture should write response headers");
            stream
                .flush()
                .expect("HTTP fixture should flush response headers");
            thread::sleep(Duration::from_millis(5));
            stream
                .write_all(b"1;kind=text\r\no\r\n1\r\nk\r\n0\r\nX-Done: yes\r\n\r\n")
                .expect("HTTP fixture should write chunked response body");
        });
        let http_response = expect_result_ok_payload(aurora_direct_net_http_request_bytes_timeout(
            string_value("POST"),
            string_value(&format!("http://{http_address}/ffi")),
            byte_vec(b"body"),
            aurora_direct_map_empty(),
            duration_value(5_000),
        ));
        assert_eq!(aurora_direct_http_response_status(http_response), 201);
        assert_eq!(
            expect_string(aurora_direct_http_response_reason(http_response)),
            "Created"
        );
        release(aurora_direct_http_response_headers(http_response));
        assert_eq!(
            expect_result_ok_string(aurora_direct_http_response_text(http_response)),
            "ok"
        );
        release(aurora_direct_http_response_bytes(http_response));
        release(http_response);
        http_server
            .join()
            .expect("HTTP fixture server should finish");

        let ws_listener = expect_result_ok_payload(aurora_direct_net_websocket_listen(
            string_value("127.0.0.1:0"),
        ));
        let ws_address =
            expect_result_ok_string(aurora_direct_websocket_listener_local_addr(ws_listener));
        let ws_listener_handle = ws_listener as usize;
        let ws_server = thread::spawn(move || {
            let ws_listener = ws_listener_handle as *mut OpaqueValue;
            let socket = expect_result_ok_payload(aurora_direct_websocket_listener_accept(
                ws_listener,
                duration_value(5_000),
            ));
            release(expect_option_some_payload(expect_result_ok_payload(
                aurora_direct_websocket_recv_text(socket, duration_value(5_000)),
            )));
            expect_result_ok_unit(aurora_direct_websocket_send_bytes(
                socket,
                byte_vec(b"ok"),
                duration_value(5_000),
            ));
            release(aurora_direct_websocket_close(socket));
            release(socket);
        });
        let ws_client = expect_result_ok_payload(aurora_direct_net_websocket_connect(
            string_value(&format!("ws://{ws_address}")),
        ));
        expect_result_ok_unit(aurora_direct_websocket_send_text(
            ws_client,
            string_value("hello"),
            duration_value(5_000),
        ));
        release(expect_option_some_payload(expect_result_ok_payload(
            aurora_direct_websocket_recv_bytes(ws_client, duration_value(5_000)),
        )));
        release(aurora_direct_websocket_close(ws_client));
        release(ws_client);
        ws_server
            .join()
            .expect("WebSocket fixture server should finish");
        release(ws_listener);
    }
}
