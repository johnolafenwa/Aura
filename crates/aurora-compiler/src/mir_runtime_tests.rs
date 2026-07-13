use super::{
    bind_args, bind_builtin_args, bind_optional_builtin_args, build_range, bytes_vec_value,
    collect_runtime_type_substitutions, collect_type_params_from_type, eval_ordering,
    evaluate_named_args, option_none, option_some, render_runtime_error, result_err, result_ok,
    run_serialized_mir, send_error_closed, task_result_ready, write_stream, CancellationContext,
    Env, EvaluatedMirArg, MirRuntime, TaskGroupValue, TaskValue,
};
use crate::diag::{Diagnostic, Span};
use crate::integer::IntegerValue;
use crate::mir::{
    BasicBlock, CallTarget, Instruction, MirArg, MirClass, MirFunction, MirLocalType, MirMatchArm,
    MirMethod, MirModule, MirParam, MirTraitImpl, Operand, Rvalue, Terminator,
};
use crate::runtime_value::{
    ChannelValue, EnumVariantValue, FileValue, HttpListenerValue, HttpResponseValue, InstanceValue,
    MapValue, ProcessChildValue, ProcessCompletedValue, ProcessStdioConfig, ProcessSupervisorValue,
    RangeValue, TcpListenerValue, TcpStreamValue, TlsListenerValue, TlsStreamValue,
    UdpDatagramValue, UdpSocketValue, Value, VecValue, WebSocketListenerValue, WebSocketValue,
};
use crate::sema::Type;
use rcgen::generate_simple_self_signed;
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::io::{self, Write};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration as StdDuration;

#[cfg(unix)]
use crate::runtime_value::{UnixListenerValue, UnixStreamValue};

fn test_runtime() -> MirRuntime {
    MirRuntime::new(
        MirModule {
            functions: Vec::new(),
            classes: Vec::new(),
            trait_impls: Vec::new(),
            top_level: None,
        },
        Arc::new(Mutex::new(String::new())),
        CancellationContext::default(),
    )
}

fn mir_arg(name: Option<&str>, value: Operand) -> MirArg {
    MirArg {
        name: name.map(str::to_string),
        value,
        writeback_place: None,
    }
}

fn enum_payloads(value: Value, enum_name: &str, variant_name: &str) -> Vec<Value> {
    match value {
        Value::EnumVariant(variant) => {
            assert_eq!(variant.enum_name, enum_name);
            assert_eq!(variant.variant_name, variant_name);
            variant.payloads
        }
        other => panic!("expected {enum_name}.{variant_name}, found {other:?}"),
    }
}

fn result_ok_payload(value: Value) -> Value {
    let mut payloads = enum_payloads(value, "Result", "Ok");
    assert_eq!(payloads.len(), 1);
    payloads.remove(0)
}

fn assert_result_err(value: Value) {
    let payloads = enum_payloads(value, "Result", "Err");
    assert_eq!(payloads.len(), 1);
}

fn call_name(
    runtime: &mut MirRuntime,
    name: &str,
    args: &[MirArg],
    env: &mut Env,
) -> crate::diag::Result<Value> {
    runtime.evaluate_call(&crate::mir::CallTarget::Name(name.to_string()), args, env)
}

fn string_vec_value(items: &[&str]) -> Value {
    Value::Vec(VecValue {
        element_type: Type::named("String"),
        elements: items
            .iter()
            .map(|item| Value::String((*item).to_string()))
            .collect(),
    })
}

fn string_map_value(items: &[(&str, &str)]) -> Value {
    Value::Map(crate::runtime_value::MapValue {
        key_type: Type::named("String"),
        value_type: Type::named("String"),
        entries: items
            .iter()
            .map(|(key, value)| {
                (
                    Value::String((*key).to_string()),
                    Value::String((*value).to_string()),
                )
            })
            .collect(),
    })
}

fn run_native_entry(
    mir_ptr: *const u8,
    mir_len: usize,
    source_path_ptr: *const u8,
    source_path_len: usize,
    source_ptr: *const u8,
    source_len: usize,
) -> i32 {
    unsafe {
        crate::mir_runtime::aurora_native_run(
            mir_ptr,
            mir_len,
            source_path_ptr,
            source_path_len,
            source_ptr,
            source_len,
        )
    }
}

#[test]
fn env_place_helpers_cover_nested_reads_and_writes() {
    let mut env = Env::default();
    env.define_typed(
        "counter",
        Type::named("Counter"),
        Value::Instance(InstanceValue {
            class_name: "Counter".to_string(),
            fields: BTreeMap::from([(
                "value".to_string(),
                Value::Int(IntegerValue::from_signed(1)),
            )]),
        }),
    );

    assert_eq!(
        env.read_place("counter.value")
            .expect("nested place should read"),
        Value::Int(IntegerValue::from_signed(1))
    );
    env.write_place("counter.value", Value::Int(IntegerValue::from_signed(4)))
        .expect("nested place should write");
    assert_eq!(
        env.read_place("counter.value")
            .expect("updated nested place should read"),
        Value::Int(IntegerValue::from_signed(4))
    );
    assert_eq!(env.place_type("counter"), Some(&Type::named("Counter")));
    env.set_place_type("counter.value", Type::named("int32"));
    assert_eq!(env.place_type("counter.value"), Some(&Type::named("int32")));

    env.define_typed(
        "outer",
        Type::named("Outer"),
        Value::Instance(InstanceValue {
            class_name: "Outer".to_string(),
            fields: BTreeMap::from([(
                "inner".to_string(),
                Value::Instance(InstanceValue {
                    class_name: "Inner".to_string(),
                    fields: BTreeMap::from([(
                        "value".to_string(),
                        Value::Int(IntegerValue::from_signed(8)),
                    )]),
                }),
            )]),
        }),
    );
    assert_eq!(
        env.read_member("outer.inner", "value")
            .expect("nested member reads should work"),
        Value::Int(IntegerValue::from_signed(8))
    );
    let nested_non_instance = env
        .read_member("outer.inner.value", "missing")
        .expect_err("nested member reads should reject non-instance leaves");
    assert!(nested_non_instance
        .message
        .contains("cannot access field `missing` on non-instance MIR place"));

    let missing_nested_member = env
        .read_member("counter.missing", "value")
        .expect_err("nested member reads should reject missing child fields");
    assert!(missing_nested_member
        .message
        .contains("has no field `missing` in MIR place `counter.missing`"));

    let missing_member_root = env
        .read_member("missing", "value")
        .expect_err("member reads should reject missing roots");
    assert!(missing_member_root
        .message
        .contains("unknown MIR place `missing`"));

    let error = env
        .read_place("counter.missing")
        .expect_err("unknown field should fail");
    assert!(error.message.contains("has no field `missing`"));

    env.define_typed(
        "count",
        Type::named("int32"),
        Value::Int(IntegerValue::from_signed(9)),
    );
    let non_instance_read = env
        .read_place("count.value")
        .expect_err("non-instance nested reads should fail");
    assert!(non_instance_read
        .message
        .contains("cannot access field `value` on non-instance MIR place `count.value`"));
    let non_instance_member_root = env
        .read_member("count", "value")
        .expect_err("member reads should reject scalar roots");
    assert!(non_instance_member_root
        .message
        .contains("cannot access field `value` on non-instance MIR place `count`"));
    let non_instance_member_leaf = env
        .read_member("count.value", "leaf")
        .expect_err("member reads should reject scalar nested segments");
    assert!(non_instance_member_leaf
        .message
        .contains("cannot access field `value` on non-instance MIR place `count.value`"));
    let non_instance_write = env
        .write_place("count.value", Value::Int(IntegerValue::from_signed(2)))
        .expect_err("non-instance nested writes should fail");
    assert!(non_instance_write
        .message
        .contains("cannot assign nested MIR place `count.value` on non-instance value"));

    let missing_root = env
        .read_place("missing")
        .expect_err("unknown MIR places should fail");
    assert!(missing_root.message.contains("unknown MIR place `missing`"));

    let missing_root_write = env
        .write_place("missing.value", Value::Int(IntegerValue::from_signed(2)))
        .expect_err("nested writes should reject missing roots");
    assert!(missing_root_write
        .message
        .contains("unknown MIR place `missing.value`"));

    let missing_child_write = env
        .write_place(
            "counter.missing.value",
            Value::Int(IntegerValue::from_signed(2)),
        )
        .expect_err("nested writes should reject missing child fields");
    assert!(missing_child_write
        .message
        .contains("has no field `missing` in MIR place"));

    let trailing_dot = env
        .read_place("counter.")
        .expect_err("trailing dots should be invalid MIR places");
    assert!(trailing_dot
        .message
        .contains("invalid MIR place `counter.`"));
    let doubled_dot = env
        .read_place("counter..value")
        .expect_err("empty place segments should be invalid MIR places");
    assert!(doubled_dot
        .message
        .contains("invalid MIR place `counter..value`"));

    env.write_place("count", Value::Int(IntegerValue::from_signed(11)))
        .expect("root writes should succeed");
    assert_eq!(
        env.read_place("count").expect("root place should read"),
        Value::Int(IntegerValue::from_signed(11))
    );
}

#[test]
fn mir_runtime_helper_values_and_streams_cover_option_result_and_diagnostics() {
    assert_eq!(option_some(Value::Bool(true)).render(), "Option.Some(true)");
    assert_eq!(option_none().render(), "Option.None");
    assert_eq!(result_ok(Value::Bool(false)).render(), "Result.Ok(false)");
    assert_eq!(
        result_err(Value::String("oops".to_string())).render(),
        "Result.Err(oops)"
    );
    assert_eq!(
        send_error_closed(Value::Int(IntegerValue::from_signed(5))).render(),
        "SendError.Closed(5)"
    );

    let diagnostic = Diagnostic::at(Span::new(2, 3), "division by zero");
    let rendered = render_runtime_error("/tmp/test.au", "def main():\n    1 / 0\n", &diagnostic);
    assert!(rendered.contains("/tmp/test.au"));
    assert!(rendered.contains("division by zero"));

    let mut buffer = Vec::new();
    write_stream(&mut buffer, "aurora").expect("write_stream should flush successfully");
    assert_eq!(String::from_utf8(buffer).unwrap(), "aurora");

    assert_eq!(
        MirRuntime::infer_value_type(&Value::Duration(5)),
        Some(Type::named("Duration"))
    );
    assert_eq!(
        MirRuntime::infer_value_type(&Value::Range(RangeValue { start: 1, end: 3 })),
        Some(Type::named("Range"))
    );

    let string_type_error = super::expect_string_value(&Value::Bool(true), "path")
        .expect_err("string helper should reject booleans");
    assert!(string_type_error
        .message
        .contains("`path` expects `String`, found `true`"));

    let command_type_error = super::expect_command_vec(
        &Value::Vec(VecValue {
            element_type: Type::named("int32"),
            elements: vec![Value::Int(IntegerValue::from_signed(1))],
        }),
        "command",
    )
    .expect_err("command helper should reject non-string vectors");
    assert!(command_type_error
        .message
        .contains("`command` expects `Vec[String]`"));
    assert_eq!(
        super::expect_command_vec(
            &Value::Vec(VecValue {
                element_type: Type::named("String"),
                elements: vec![Value::String("echo".to_string())],
            }),
            "command",
        )
        .expect("string vectors should decode as commands"),
        vec!["echo".to_string()]
    );
    let malformed_command_error = super::expect_command_vec(
        &Value::Vec(VecValue {
            element_type: Type::named("String"),
            elements: vec![Value::Bool(true)],
        }),
        "command",
    )
    .expect_err("command helper should validate string vector elements");
    assert!(malformed_command_error
        .message
        .contains("`command` expects `String`"));

    assert_eq!(
        super::expect_bytes_value(
            &Value::Vec(VecValue {
                element_type: Type::named("Unknown"),
                elements: vec![
                    Value::Int(IntegerValue::from_signed(65)),
                    Value::Int(IntegerValue::from_signed(66)),
                ],
            }),
            "payload",
        )
        .expect("Unknown integer vectors should decode as bytes"),
        b"AB".to_vec()
    );
    let bytes_type_error = super::expect_bytes_value(&Value::String("bad".to_string()), "payload")
        .expect_err("byte helper should reject non-vector payloads");
    assert!(bytes_type_error
        .message
        .contains("`payload` expects `Vec[uint8]`"));
    let bytes_range_error = super::expect_bytes_value(
        &Value::Vec(VecValue {
            element_type: Type::named("uint8"),
            elements: vec![Value::Int(IntegerValue::from_signed(300))],
        }),
        "payload",
    )
    .expect_err("byte helper should reject out-of-range integers");
    assert!(bytes_range_error
        .message
        .contains("`payload` expects `Vec[uint8]`"));

    let bool_type_error = super::expect_bool_value(&Value::String("yes".to_string()), "flag")
        .expect_err("bool helper should reject strings");
    assert!(bool_type_error
        .message
        .contains("`flag` expects `bool`, found `yes`"));

    assert_eq!(
        super::expect_optional_string_value(&Value::Unit, "event")
            .expect("unit should decode as absent optional string"),
        None
    );
    assert_eq!(
        super::expect_optional_string_value(&option_none(), "event")
            .expect("Option.None should decode as absent optional string"),
        None
    );
    assert_eq!(
        super::expect_optional_string_value(
            &option_some(Value::String("ready".to_string())),
            "event"
        )
        .expect("Option.Some(String) should decode"),
        Some("ready".to_string())
    );
    let malformed_option_error = super::expect_optional_string_value(
        &Value::EnumVariant(EnumVariantValue {
            enum_name: "Option".to_string(),
            variant_name: "Some".to_string(),
            payloads: Vec::new(),
        }),
        "event",
    )
    .expect_err("malformed Option.Some should be rejected");
    assert!(malformed_option_error
        .message
        .contains("malformed option payload"));
    let optional_string_type_error =
        super::expect_optional_string_value(&Value::Bool(false), "event")
            .expect_err("optional string helper should reject booleans");
    assert!(optional_string_type_error
        .message
        .contains("`event` expects `Option[String]`"));

    assert_eq!(
        super::expect_i32_value(&Value::Int(IntegerValue::from_signed(7)), "count")
            .expect("small integers should decode as i32"),
        7
    );
    let i32_range_error = super::expect_i32_value(
        &Value::Int(IntegerValue::from_signed(i128::from(i32::MAX) + 1)),
        "count",
    )
    .expect_err("oversized integers should be rejected as i32");
    assert!(i32_range_error.message.contains("`count` expects `int32`"));
    let i32_type_error = super::expect_i32_value(&Value::String("7".to_string()), "count")
        .expect_err("i32 helper should reject strings");
    assert!(i32_type_error
        .message
        .contains("`count` expects `int32`, found `7`"));

    assert_eq!(
        super::expect_process_optional_timeout(&Value::Unit, "timeout")
            .expect("unit process timeout should decode as absent"),
        None
    );
    assert_eq!(
        super::expect_process_optional_timeout(&Value::Duration(-1), "timeout")
            .expect("negative process timeout should decode as absent"),
        None
    );
    assert_eq!(
        super::expect_process_optional_timeout(&Value::Duration(10), "timeout")
            .expect("positive process timeout should decode"),
        Some(StdDuration::from_millis(10))
    );
    let process_timeout_range_error = super::expect_process_optional_timeout(
        &Value::Duration(i128::from(u64::MAX) + 1),
        "timeout",
    )
    .expect_err("process timeout helper should reject oversized durations");
    assert!(process_timeout_range_error
        .message
        .contains("duration must be non-negative"));
    let process_timeout_type_error =
        super::expect_process_optional_timeout(&Value::String("soon".to_string()), "timeout")
            .expect_err("process timeout helper should reject strings");
    assert!(process_timeout_type_error
        .message
        .contains("`timeout` expects `Duration`"));

    assert_eq!(
        super::expect_duration_value(&Value::Duration(4), "timeout")
            .expect("positive duration should decode"),
        StdDuration::from_millis(4)
    );
    let negative_duration_error = super::expect_duration_value(&Value::Duration(-4), "timeout")
        .expect_err("negative durations should be rejected");
    assert!(negative_duration_error
        .message
        .contains("duration must be non-negative"));
    let duration_type_error =
        super::expect_duration_value(&Value::String("soon".to_string()), "timeout")
            .expect_err("duration helper should reject strings");
    assert!(duration_type_error
        .message
        .contains("`timeout` expects `Duration`"));

    assert_eq!(
        super::expect_supervisor_max_restarts(&Value::Int(IntegerValue::from_signed(-1)), "max")
            .expect("-1 should decode as unbounded max_restarts"),
        None
    );
    assert_eq!(
        super::expect_supervisor_max_restarts(&Value::Int(IntegerValue::from_signed(2)), "max")
            .expect("positive max_restarts should decode"),
        Some(2)
    );
    let restart_error =
        super::expect_supervisor_max_restarts(&Value::Int(IntegerValue::from_signed(-2)), "max")
            .expect_err("max_restarts below -1 should be rejected");
    assert!(restart_error.message.contains("to be -1 or greater"));

    assert_eq!(
        super::expect_optional_timeout(None, "timeout")
            .expect("missing optional timeout should decode as absent"),
        None
    );
    assert_eq!(
        super::expect_optional_timeout(Some(&Value::Unit), "timeout")
            .expect("unit optional timeout should decode as absent"),
        None
    );
    assert_eq!(
        super::expect_optional_timeout(Some(&Value::Duration(8)), "timeout")
            .expect("duration optional timeout should decode"),
        Some(StdDuration::from_millis(8))
    );
    let optional_timeout_negative =
        super::expect_optional_timeout(Some(&Value::Duration(-8)), "timeout")
            .expect_err("negative optional timeout should be rejected");
    assert!(optional_timeout_negative
        .message
        .contains("duration must be non-negative"));
    let optional_timeout_type_error =
        super::expect_optional_timeout(Some(&Value::String("soon".to_string())), "timeout")
            .expect_err("optional timeout should reject strings");
    assert!(optional_timeout_type_error
        .message
        .contains("`timeout` expects `Duration`"));

    assert_eq!(
        super::expect_headers_map(
            &Value::Map(MapValue {
                key_type: Type::named("String"),
                value_type: Type::named("String"),
                entries: vec![(
                    Value::String("Accept".to_string()),
                    Value::String("*/*".to_string())
                )],
            }),
            "headers",
        )
        .expect("string header maps should decode"),
        vec![("Accept".to_string(), "*/*".to_string())]
    );
    assert_eq!(
        super::headers_map_value(vec![("X-Test".to_string(), "1".to_string())]).render(),
        "{X-Test: 1}"
    );
    let malformed_headers_error = super::expect_headers_map(
        &Value::Map(MapValue {
            key_type: Type::named("String"),
            value_type: Type::named("String"),
            entries: vec![(Value::Bool(true), Value::String("bad".to_string()))],
        }),
        "headers",
    )
    .expect_err("headers helper should validate map entries");
    assert!(malformed_headers_error
        .message
        .contains("`headers` expects `String`"));
    let headers_type_error = super::expect_headers_map(&Value::Bool(true), "headers")
        .expect_err("headers helper should reject non-maps");
    assert!(headers_type_error
        .message
        .contains("`headers` expects `Map[String, String]`"));
}

#[test]
fn mir_runtime_process_capture_helpers_cover_success_and_malformed_results() {
    fn assert_process_error_variant(value: Value, variant_name: &str) {
        let Value::EnumVariant(variant) = value else {
            panic!("expected process error enum variant");
        };
        assert_eq!(variant.variant_name, variant_name);
    }

    assert_process_error_variant(
        super::process_error_from_io(io::Error::new(io::ErrorKind::TimedOut, "slow")),
        "TimedOut",
    );
    assert_process_error_variant(
        super::process_error_from_io(io::Error::new(io::ErrorKind::Interrupted, "cancelled")),
        "Cancelled",
    );
    assert_process_error_variant(
        super::process_error_from_io(io::Error::new(io::ErrorKind::Other, "io failed")),
        "Io",
    );

    let runtime = test_runtime();
    assert_eq!(
        runtime
            .await_process_capture_task(None, "stdout")
            .expect("missing capture task should produce empty bytes"),
        Vec::<u8>::new()
    );

    let bytes_task = TaskValue::from_handle(thread::spawn(|| {
        Ok(Value::Vec(VecValue {
            element_type: Type::named("uint8"),
            elements: vec![
                Value::Int(IntegerValue::from_signed(65)),
                Value::Int(IntegerValue::from_literal(66)),
            ],
        }))
    }));
    assert_eq!(
        runtime
            .await_process_capture_task(Some(bytes_task), "stdout")
            .expect("byte capture task should decode bytes"),
        b"AB".to_vec()
    );

    let non_byte_integer = TaskValue::from_handle(thread::spawn(|| {
        Ok(Value::Vec(VecValue {
            element_type: Type::named("uint8"),
            elements: vec![Value::Int(IntegerValue::from_signed(300))],
        }))
    }));
    let error = runtime
        .await_process_capture_task(Some(non_byte_integer), "stdout")
        .expect_err("non-byte integers should fail capture decoding");
    assert!(error
        .message
        .contains("process stdout capture returned a non-byte integer"));

    let wrong_payload = TaskValue::from_handle(thread::spawn(|| {
        Ok(Value::Vec(VecValue {
            element_type: Type::named("uint8"),
            elements: vec![Value::String("bad".to_string())],
        }))
    }));
    let error = runtime
        .await_process_capture_task(Some(wrong_payload), "stderr")
        .expect_err("non-integer byte payloads should fail capture decoding");
    assert!(error
        .message
        .contains("process stderr capture returned `bad` inside `Vec[uint8]"));

    let wrong_result_type = TaskValue::from_handle(thread::spawn(|| {
        Ok(Value::Vec(VecValue {
            element_type: Type::named("String"),
            elements: vec![Value::String("bad".to_string())],
        }))
    }));
    let error = runtime
        .await_process_capture_task(Some(wrong_result_type), "stderr")
        .expect_err("wrong capture result types should fail");
    assert!(error
        .message
        .contains("process stderr capture returned `[bad]` instead of `Vec[uint8]"));

    let capture_error =
        TaskValue::from_handle(thread::spawn(|| Err(Diagnostic::new("pipe failed"))));
    let error = runtime
        .await_process_capture_task(Some(capture_error), "stdout")
        .expect_err("capture task diagnostics should propagate");
    assert_eq!(error.message, "pipe failed");

    let group = TaskGroupValue::new(&CancellationContext::default());
    let cancelled_runtime = MirRuntime::new(
        MirModule {
            functions: Vec::new(),
            classes: Vec::new(),
            trait_impls: Vec::new(),
            top_level: None,
        },
        Arc::new(Mutex::new(String::new())),
        group.child_cancellation(),
    );
    let slow_capture = TaskValue::from_handle(thread::spawn(|| {
        thread::sleep(StdDuration::from_millis(50));
        Ok(Value::Vec(VecValue {
            element_type: Type::named("uint8"),
            elements: Vec::new(),
        }))
    }));
    group.cancel();
    let error = cancelled_runtime
        .await_process_capture_task(Some(slow_capture), "stderr")
        .expect_err("cancelled capture waits should fail");
    assert!(error
        .message
        .contains("process stderr capture was cancelled unexpectedly"));
}

#[test]
fn mir_runtime_infers_resource_value_types_for_runtime_backed_surfaces() {
    let timestamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("system time should be after epoch")
        .as_nanos();
    let file_path = std::env::temp_dir().join(format!(
        "aurora-mir-runtime-resource-inference-{timestamp}.txt"
    ));
    std::fs::write(&file_path, "resource").expect("test file should be written");
    let file = FileValue::open(file_path.to_str().expect("temp path should be utf-8"))
        .expect("file should open");
    assert_eq!(
        MirRuntime::infer_value_type(&Value::File(file.clone())),
        Some(Type::named("fs.File"))
    );
    file.close();
    let _ = std::fs::remove_file(&file_path);

    let tcp_listener = TcpListenerValue::bind("127.0.0.1:0").expect("tcp listener should bind");
    let tcp_address = tcp_listener
        .local_addr()
        .expect("tcp listener address should be available");
    let tcp_server = {
        let listener = tcp_listener.clone();
        thread::spawn(move || {
            listener
                .accept(
                    Some(StdDuration::from_secs(5)),
                    Some(&CancellationContext::default()),
                )
                .expect("tcp accept should succeed")
        })
    };
    let tcp_client = TcpStreamValue::connect(
        &tcp_address,
        Some(StdDuration::from_secs(2)),
        Some(&CancellationContext::default()),
    )
    .expect("tcp client should connect");
    let tcp_server_stream = tcp_server.join().expect("tcp server should join");
    assert_eq!(
        MirRuntime::infer_value_type(&Value::TcpListener(tcp_listener.clone())),
        Some(Type::named("net.TcpListener"))
    );
    assert_eq!(
        MirRuntime::infer_value_type(&Value::TcpStream(tcp_client.clone())),
        Some(Type::named("net.TcpStream"))
    );
    tcp_client.close();
    tcp_server_stream.close();
    tcp_listener.close();

    let udp_socket = UdpSocketValue::bind("127.0.0.1:0").expect("udp socket should bind");
    assert_eq!(
        MirRuntime::infer_value_type(&Value::UdpSocket(udp_socket.clone())),
        Some(Type::named("net.UdpSocket"))
    );
    assert_eq!(
        MirRuntime::infer_value_type(&Value::UdpDatagram(UdpDatagramValue {
            address: "127.0.0.1:7".to_string(),
            data: vec![1, 2, 3],
        })),
        Some(Type::named("net.UdpDatagram"))
    );
    udp_socket.close();

    let http_listener = HttpListenerValue::bind("127.0.0.1:0").expect("http listener should bind");
    let http_address = http_listener
        .local_addr()
        .expect("http listener address should be available");
    assert_eq!(
        MirRuntime::infer_value_type(&Value::HttpListener(http_listener.clone())),
        Some(Type::named("net.HttpListener"))
    );
    let http_server = {
        let listener = http_listener.clone();
        thread::spawn(move || {
            let exchange = listener
                .accept(
                    Some(StdDuration::from_secs(5)),
                    Some(&CancellationContext::default()),
                )
                .expect("http accept should succeed");
            assert_eq!(
                MirRuntime::infer_value_type(&Value::HttpExchange(exchange.clone())),
                Some(Type::named("net.HttpExchange"))
            );
            exchange
                .respond_text(200, "ok", Vec::new())
                .expect("http response should write");
        })
    };
    let http_response = HttpResponseValue::request_text(
        "GET",
        &format!("http://{http_address}/types"),
        "",
        Vec::new(),
        Some(StdDuration::from_secs(2)),
        Some(&CancellationContext::default()),
    )
    .expect("http request should succeed");
    assert_eq!(
        MirRuntime::infer_value_type(&Value::HttpResponse(http_response)),
        Some(Type::named("net.HttpResponse"))
    );
    http_server.join().expect("http server should join");

    let supervisor = ProcessSupervisorValue::new();
    assert_eq!(
        MirRuntime::infer_value_type(&Value::ProcessSupervisor(supervisor.clone())),
        Some(Type::named("process.Supervisor"))
    );
    supervisor.close();

    let child = ProcessChildValue::spawn(
        vec![
            "/bin/sh".to_string(),
            "-c".to_string(),
            "printf pipe".to_string(),
        ],
        None,
        Vec::new(),
        ProcessStdioConfig::Null,
        ProcessStdioConfig::Pipe,
        ProcessStdioConfig::Null,
        false,
    )
    .expect("process child should spawn");
    let stdout = child.stdout().expect("child stdout should be piped");
    assert_eq!(
        MirRuntime::infer_value_type(&Value::ProcessChild(child.clone())),
        Some(Type::named("process.Child"))
    );
    assert_eq!(
        MirRuntime::infer_value_type(&Value::ProcessPipe(stdout.clone())),
        Some(Type::named("process.Pipe"))
    );
    child.wait(
        Some(StdDuration::from_secs(2)),
        Some(&CancellationContext::default()),
    );
    stdout.close();

    let completed = ProcessCompletedValue::new(
        Value::EnumVariant(EnumVariantValue {
            enum_name: "process.ExitStatus".to_string(),
            variant_name: "Exited".to_string(),
            payloads: vec![Value::Int(IntegerValue::from_signed(0))],
        }),
        b"out".to_vec(),
        b"err".to_vec(),
    );
    assert_eq!(
        MirRuntime::infer_value_type(&Value::ProcessCompleted(completed)),
        Some(Type::named("process.Completed"))
    );

    #[cfg(unix)]
    {
        let socket_path = std::path::PathBuf::from(format!(
            "/tmp/aura-mir-{}-{}.sock",
            std::process::id(),
            timestamp % 1_000_000
        ));
        let _ = std::fs::remove_file(&socket_path);
        let unix_listener =
            UnixListenerValue::bind(socket_path.to_str().expect("utf-8 socket path"))
                .expect("unix listener should bind");
        let unix_server = {
            let listener = unix_listener.clone();
            thread::spawn(move || {
                listener
                    .accept(
                        Some(StdDuration::from_secs(2)),
                        Some(&CancellationContext::default()),
                    )
                    .expect("unix accept should succeed")
            })
        };
        let unix_client = UnixStreamValue::connect(
            socket_path.to_str().expect("utf-8 socket path"),
            Some(StdDuration::from_secs(2)),
            Some(&CancellationContext::default()),
        )
        .expect("unix client should connect");
        let unix_server_stream = unix_server.join().expect("unix server should join");
        assert_eq!(
            MirRuntime::infer_value_type(&Value::UnixListener(unix_listener.clone())),
            Some(Type::named("net.UnixListener"))
        );
        assert_eq!(
            MirRuntime::infer_value_type(&Value::UnixStream(unix_client.clone())),
            Some(Type::named("net.UnixStream"))
        );
        unix_client.close();
        unix_server_stream.close();
        unix_listener.close();
        let _ = std::fs::remove_file(&socket_path);
    }

    let certificate =
        generate_simple_self_signed(vec!["localhost".to_string()]).expect("cert generation");
    let cert_path = std::env::temp_dir().join(format!(
        "aurora-mir-runtime-resource-inference-{timestamp}-cert.pem"
    ));
    let key_path = std::env::temp_dir().join(format!(
        "aurora-mir-runtime-resource-inference-{timestamp}-key.pem"
    ));
    std::fs::write(&cert_path, certificate.cert.pem()).expect("cert should be written");
    std::fs::write(&key_path, certificate.key_pair.serialize_pem()).expect("key should be written");
    let tls_listener = TlsListenerValue::bind(
        "127.0.0.1:0",
        cert_path.to_str().expect("cert path should be utf-8"),
        key_path.to_str().expect("key path should be utf-8"),
    )
    .expect("tls listener should bind");
    assert_eq!(
        MirRuntime::infer_value_type(&Value::TlsListener(tls_listener.clone())),
        Some(Type::named("net.TlsListener"))
    );
    let mut tls_runtime = test_runtime();
    let tls_env = Env::default();
    let tls_address = result_ok_payload(
        tls_runtime
            .evaluate_tls_listener_method(tls_listener.clone(), "local_addr", &[], &tls_env)
            .expect("tls listener local_addr should succeed"),
    );
    let Value::String(tls_address) = tls_address else {
        panic!("tls listener local_addr should return a string");
    };
    assert!(tls_runtime
        .evaluate_tls_listener_method(tls_listener.clone(), "unsupported", &[], &tls_env)
        .expect_err("unsupported tls listener methods should fail")
        .message
        .contains("unsupported MIR tls listener method"));
    let tls_server = {
        let listener = tls_listener.clone();
        thread::spawn(move || {
            let mut server_runtime = test_runtime();
            let server_env = Env::default();
            let stream = result_ok_payload(
                server_runtime
                    .evaluate_tls_listener_method(
                        listener,
                        "accept",
                        &[mir_arg(Some("timeout"), Operand::Duration(2_000))],
                        &server_env,
                    )
                    .expect("tls accept should succeed"),
            );
            let Value::TlsStream(stream) = stream else {
                panic!("tls accept should return a tls stream");
            };
            assert_eq!(
                MirRuntime::infer_value_type(&Value::TlsStream(stream.clone())),
                Some(Type::named("net.TlsStream"))
            );
            let line = result_ok_payload(
                server_runtime
                    .evaluate_tls_stream_method(
                        stream.clone(),
                        "read_line",
                        &[mir_arg(Some("timeout"), Operand::Duration(2_000))],
                        &server_env,
                    )
                    .expect("tls server read_line should succeed"),
            );
            assert_eq!(
                enum_payloads(line, "Option", "Some"),
                vec![Value::String("secure".to_string())]
            );
            assert_eq!(
                result_ok_payload(
                    server_runtime
                        .evaluate_tls_stream_method(
                            stream.clone(),
                            "write_all",
                            &[
                                mir_arg(Some("text"), Operand::String("ok".to_string())),
                                mir_arg(Some("timeout"), Operand::Duration(2_000)),
                            ],
                            &server_env,
                        )
                        .expect("tls server write_all should succeed")
                ),
                Value::Unit
            );
            assert_eq!(
                server_runtime
                    .evaluate_tls_stream_method(stream, "close", &[], &server_env)
                    .expect("tls server close should succeed"),
                Value::Unit
            );
        })
    };
    let tls_client = TlsStreamValue::connect(
        &tls_address,
        "localhost",
        Some(cert_path.to_str().expect("cert path should be utf-8")),
        Some(StdDuration::from_secs(2)),
        Some(&CancellationContext::default()),
    )
    .expect("tls client should connect");
    assert_eq!(
        MirRuntime::infer_value_type(&Value::TlsStream(tls_client.clone())),
        Some(Type::named("net.TlsStream"))
    );
    assert_eq!(
        result_ok_payload(
            tls_runtime
                .evaluate_tls_stream_method(
                    tls_client.clone(),
                    "write_all",
                    &[
                        mir_arg(Some("text"), Operand::String("secure\n".to_string())),
                        mir_arg(Some("timeout"), Operand::Duration(2_000)),
                    ],
                    &tls_env,
                )
                .expect("tls client write_all should succeed")
        ),
        Value::Unit
    );
    assert_eq!(
        result_ok_payload(
            tls_runtime
                .evaluate_tls_stream_method(
                    tls_client.clone(),
                    "read_exact",
                    &[
                        mir_arg(Some("count"), Operand::Int(2)),
                        mir_arg(Some("timeout"), Operand::Duration(2_000)),
                    ],
                    &tls_env,
                )
                .expect("tls client read_exact should succeed")
        ),
        bytes_vec_value(b"ok".to_vec())
    );
    assert!(tls_runtime
        .evaluate_tls_stream_method(tls_client.clone(), "unsupported", &[], &tls_env)
        .expect_err("unsupported tls stream methods should fail")
        .message
        .contains("unsupported MIR tls stream method"));
    assert_eq!(
        tls_runtime
            .evaluate_tls_stream_method(tls_client, "close", &[], &tls_env)
            .expect("tls client close should succeed"),
        Value::Unit
    );
    tls_server.join().expect("tls server should join");
    tls_listener.close();
    let _ = std::fs::remove_file(&cert_path);
    let _ = std::fs::remove_file(&key_path);
}

#[test]
fn mir_runtime_resource_member_helpers_cover_io_process_and_network_paths() {
    let mut runtime = test_runtime();
    let mut env = Env::default();
    env.define_typed(
        "bytes",
        Type::Named("Vec".to_string(), vec![Type::named("uint8")]),
        bytes_vec_value(b"-bytes".to_vec()),
    );

    let timestamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("system time should be after epoch")
        .as_nanos();
    let read_path = std::env::temp_dir().join(format!(
        "aurora-mir-runtime-read-{timestamp}-{}.txt",
        std::process::id()
    ));
    let write_path = std::env::temp_dir().join(format!(
        "aurora-mir-runtime-write-{timestamp}-{}.txt",
        std::process::id()
    ));
    std::fs::write(&read_path, "hello").expect("read fixture should be written");

    let read_file = FileValue::open(read_path.to_str().expect("temp path should be utf-8"))
        .expect("read fixture should open");
    let file_text = runtime
        .evaluate_file_method(read_file.clone(), "read_all", &[], &env)
        .expect("file read_all should succeed");
    assert_eq!(
        result_ok_payload(file_text),
        Value::String("hello".to_string())
    );
    read_file.close();

    let read_file = FileValue::open(read_path.to_str().expect("temp path should be utf-8"))
        .expect("read fixture should reopen");
    let file_bytes = runtime
        .evaluate_file_method(read_file.clone(), "read_bytes", &[], &env)
        .expect("file read_bytes should succeed");
    assert_eq!(
        result_ok_payload(file_bytes),
        bytes_vec_value(b"hello".to_vec())
    );
    read_file.close();

    let write_file = FileValue::create(write_path.to_str().expect("temp path should be utf-8"))
        .expect("write fixture should open");
    assert_eq!(
        result_ok_payload(
            runtime
                .evaluate_file_method(
                    write_file.clone(),
                    "write_all",
                    &[mir_arg(
                        Some("text"),
                        Operand::String("written".to_string())
                    )],
                    &env,
                )
                .expect("file write_all should succeed")
        ),
        Value::Unit
    );
    assert_eq!(
        result_ok_payload(
            runtime
                .evaluate_file_method(
                    write_file.clone(),
                    "write_bytes",
                    &[mir_arg(Some("bytes"), Operand::Place("bytes".to_string()))],
                    &env,
                )
                .expect("file write_bytes should succeed")
        ),
        Value::Unit
    );
    assert_eq!(
        result_ok_payload(
            runtime
                .evaluate_file_method(write_file.clone(), "flush", &[], &env)
                .expect("file flush should succeed")
        ),
        Value::Unit
    );
    let bad_file_write = runtime
        .evaluate_file_method(
            write_file.clone(),
            "write_all",
            &[mir_arg(Some("text"), Operand::Bool(true))],
            &env,
        )
        .expect_err("file write_all should reject non-string text");
    assert!(bad_file_write.message.contains("expects `String`"));
    assert_eq!(
        runtime
            .evaluate_file_method(write_file.clone(), "close", &[], &env)
            .expect("file close should succeed"),
        Value::Unit
    );
    let missing_file_method = runtime
        .evaluate_file_method(write_file, "missing", &[], &env)
        .expect_err("unknown file method should fail");
    assert!(missing_file_method
        .message
        .contains("unsupported MIR file method"));
    assert_eq!(
        std::fs::read_to_string(&write_path).expect("write fixture should be readable"),
        "written-bytes"
    );
    let _ = std::fs::remove_file(&read_path);
    let _ = std::fs::remove_file(&write_path);

    let completed = ProcessCompletedValue::new(
        Value::EnumVariant(EnumVariantValue {
            enum_name: "ExitStatus".to_string(),
            variant_name: "Exited".to_string(),
            payloads: vec![Value::Int(IntegerValue::from_signed(0))],
        }),
        b"out".to_vec(),
        b"err".to_vec(),
    );
    enum_payloads(
        runtime
            .evaluate_process_completed_method(completed.clone(), "status", &[], &env)
            .expect("completed status should succeed"),
        "ExitStatus",
        "Exited",
    );
    assert_eq!(
        runtime
            .evaluate_process_completed_method(completed.clone(), "success", &[], &env)
            .expect("completed success should succeed"),
        Value::Bool(true)
    );
    assert_eq!(
        runtime
            .evaluate_process_completed_method(completed.clone(), "stdout", &[], &env)
            .expect("completed stdout should succeed"),
        Value::String("out".to_string())
    );
    assert_eq!(
        runtime
            .evaluate_process_completed_method(completed.clone(), "stderr_bytes", &[], &env)
            .expect("completed stderr bytes should succeed"),
        bytes_vec_value(b"err".to_vec())
    );
    assert_eq!(
        result_ok_payload(
            runtime
                .evaluate_process_completed_method(completed.clone(), "check", &[], &env)
                .expect("completed check should succeed")
        ),
        Value::Unit
    );
    let bad_completed_method = runtime
        .evaluate_process_completed_method(completed, "missing", &[], &env)
        .expect_err("unknown completed method should fail");
    assert!(bad_completed_method
        .message
        .contains("unsupported MIR process completed method"));

    let child = ProcessChildValue::spawn(
        vec![
            "/bin/sh".to_string(),
            "-c".to_string(),
            "printf out".to_string(),
        ],
        None,
        Vec::new(),
        ProcessStdioConfig::Null,
        ProcessStdioConfig::Pipe,
        ProcessStdioConfig::Pipe,
        false,
    )
    .expect("process child should spawn");
    enum_payloads(
        runtime
            .evaluate_process_child_method(child.clone(), "stdin", &[], &env)
            .expect("child stdin method should succeed"),
        "Option",
        "None",
    );
    enum_payloads(
        runtime
            .evaluate_process_child_method(child.clone(), "stdout", &[], &env)
            .expect("child stdout method should succeed"),
        "Option",
        "Some",
    );
    enum_payloads(
        runtime
            .evaluate_process_child_method(child.clone(), "stderr", &[], &env)
            .expect("child stderr method should succeed"),
        "Option",
        "Some",
    );
    let stdout_pipe = child.stdout().expect("child stdout should be piped");
    assert_eq!(
        result_ok_payload(
            runtime
                .evaluate_process_pipe_method(stdout_pipe, "read_all", &[], &env)
                .expect("pipe read_all should succeed")
        ),
        Value::String("out".to_string())
    );
    enum_payloads(
        runtime
            .evaluate_process_child_method(child.clone(), "wait", &[], &env)
            .expect("child wait should succeed"),
        "Wait",
        "Exited",
    );

    let ok_child = ProcessChildValue::spawn(
        vec![
            "/bin/sh".to_string(),
            "-c".to_string(),
            "exit 0".to_string(),
        ],
        None,
        Vec::new(),
        ProcessStdioConfig::Null,
        ProcessStdioConfig::Null,
        ProcessStdioConfig::Null,
        false,
    )
    .expect("process child should spawn");
    enum_payloads(
        result_ok_payload(
            runtime
                .evaluate_process_child_method(ok_child, "wait_ok", &[], &env)
                .expect("wait_ok should succeed"),
        ),
        "ExitStatus",
        "Exited",
    );

    let cat = ProcessChildValue::spawn(
        vec!["/bin/cat".to_string()],
        None,
        Vec::new(),
        ProcessStdioConfig::Pipe,
        ProcessStdioConfig::Pipe,
        ProcessStdioConfig::Null,
        false,
    )
    .expect("cat should spawn");
    let stdin_pipe = cat.stdin().expect("cat stdin should be piped");
    assert_eq!(
        result_ok_payload(
            runtime
                .evaluate_process_pipe_method(
                    stdin_pipe.clone(),
                    "write_all",
                    &[mir_arg(Some("text"), Operand::String("cat".to_string()))],
                    &env,
                )
                .expect("pipe write_all should succeed")
        ),
        Value::Unit
    );
    assert_eq!(
        result_ok_payload(
            runtime
                .evaluate_process_pipe_method(stdin_pipe.clone(), "flush", &[], &env)
                .expect("pipe flush should succeed")
        ),
        Value::Unit
    );
    assert_eq!(
        runtime
            .evaluate_process_pipe_method(stdin_pipe, "close", &[], &env)
            .expect("pipe close should succeed"),
        Value::Unit
    );
    let cat_stdout = cat.stdout().expect("cat stdout should be piped");
    assert_eq!(
        result_ok_payload(
            runtime
                .evaluate_process_pipe_method(cat_stdout, "read_all", &[], &env)
                .expect("cat stdout should be readable")
        ),
        Value::String("cat".to_string())
    );
    enum_payloads(
        runtime
            .evaluate_process_child_method(cat, "wait", &[], &env)
            .expect("cat wait should succeed"),
        "Wait",
        "Exited",
    );

    let cat_bytes = ProcessChildValue::spawn(
        vec!["/bin/cat".to_string()],
        None,
        Vec::new(),
        ProcessStdioConfig::Pipe,
        ProcessStdioConfig::Pipe,
        ProcessStdioConfig::Null,
        false,
    )
    .expect("byte cat should spawn");
    let bytes_stdin = cat_bytes.stdin().expect("byte cat stdin should be piped");
    assert_eq!(
        result_ok_payload(
            runtime
                .evaluate_process_pipe_method(
                    bytes_stdin.clone(),
                    "write_bytes",
                    &[
                        mir_arg(Some("bytes"), Operand::Place("bytes".to_string())),
                        mir_arg(Some("timeout"), Operand::Duration(1_000)),
                    ],
                    &env,
                )
                .expect("pipe write_bytes should succeed")
        ),
        Value::Unit
    );
    assert_eq!(
        runtime
            .evaluate_process_pipe_method(bytes_stdin, "close", &[], &env)
            .expect("byte cat stdin close should succeed"),
        Value::Unit
    );
    let bytes_stdout = cat_bytes.stdout().expect("byte cat stdout should be piped");
    let byte_read = result_ok_payload(
        runtime
            .evaluate_process_pipe_method(
                bytes_stdout,
                "read_bytes",
                &[
                    mir_arg(Some("max_bytes"), Operand::Int(6)),
                    mir_arg(Some("timeout"), Operand::Duration(1_000)),
                ],
                &env,
            )
            .expect("pipe read_bytes should succeed"),
    );
    let byte_payload = enum_payloads(byte_read, "Option", "Some");
    assert_eq!(byte_payload, vec![bytes_vec_value(b"-bytes".to_vec())]);
    enum_payloads(
        runtime
            .evaluate_process_child_method(cat_bytes, "wait", &[], &env)
            .expect("byte cat wait should succeed"),
        "Wait",
        "Exited",
    );

    let supervisor = ProcessSupervisorValue::new();
    assert_eq!(
        runtime
            .evaluate_process_supervisor_method(supervisor.clone(), "is_empty", &[], &env)
            .expect("supervisor is_empty should succeed"),
        Value::Bool(true)
    );
    enum_payloads(
        runtime
            .evaluate_process_supervisor_method(supervisor.clone(), "wait", &[], &env)
            .expect("empty supervisor wait should time out immediately"),
        "SupervisorWait",
        "TimedOut",
    );
    assert_eq!(
        result_ok_payload(
            runtime
                .evaluate_process_supervisor_method(supervisor.clone(), "stop", &[], &env)
                .expect("empty supervisor stop should succeed")
        ),
        Value::Unit
    );
    assert_eq!(
        runtime
            .evaluate_process_supervisor_method(supervisor.clone(), "close", &[], &env)
            .expect("supervisor close should succeed"),
        Value::Unit
    );
    let supervisor_error = runtime
        .evaluate_process_supervisor_method(supervisor, "missing", &[], &env)
        .expect_err("unknown supervisor method should fail");
    assert!(supervisor_error
        .message
        .contains("unsupported MIR process supervisor method"));

    let tcp_listener = TcpListenerValue::bind("127.0.0.1:0").expect("tcp listener should bind");
    match result_ok_payload(
        runtime
            .evaluate_tcp_listener_method(tcp_listener.clone(), "local_addr", &[], &env)
            .expect("tcp listener local_addr should succeed"),
    ) {
        Value::String(address) => assert!(address.starts_with("127.0.0.1:")),
        other => panic!("expected tcp local address string, found {other:?}"),
    }
    enum_payloads(
        runtime
            .evaluate_tcp_listener_method(
                tcp_listener.clone(),
                "accept",
                &[mir_arg(Some("timeout"), Operand::Duration(1))],
                &env,
            )
            .expect("tcp listener accept should return a Result"),
        "Result",
        "Err",
    );
    assert_eq!(
        runtime
            .evaluate_tcp_listener_method(tcp_listener.clone(), "close", &[], &env)
            .expect("tcp listener close should succeed"),
        Value::Unit
    );
    let tcp_listener_error = runtime
        .evaluate_tcp_listener_method(tcp_listener, "missing", &[], &env)
        .expect_err("unknown tcp listener method should fail");
    assert!(tcp_listener_error
        .message
        .contains("unsupported MIR tcp listener method"));

    let udp_receiver = UdpSocketValue::bind("127.0.0.1:0").expect("udp receiver should bind");
    let udp_address = udp_receiver
        .local_addr()
        .expect("udp receiver address should be available");
    let udp_sender = UdpSocketValue::bind("127.0.0.1:0").expect("udp sender should bind");
    assert_eq!(
        result_ok_payload(
            runtime
                .evaluate_udp_socket_method(
                    udp_sender.clone(),
                    "send_text",
                    &[
                        mir_arg(Some("address"), Operand::String(udp_address.clone())),
                        mir_arg(Some("text"), Operand::String("ping".to_string())),
                        mir_arg(Some("timeout"), Operand::Duration(1_000),),
                    ],
                    &env,
                )
                .expect("udp send_text should succeed")
        ),
        Value::Unit
    );
    assert_eq!(
        result_ok_payload(
            runtime
                .evaluate_udp_socket_method(
                    udp_sender.clone(),
                    "send_bytes",
                    &[
                        mir_arg(Some("address"), Operand::String(udp_address.clone())),
                        mir_arg(Some("bytes"), Operand::Place("bytes".to_string())),
                        mir_arg(Some("timeout"), Operand::Duration(1_000)),
                    ],
                    &env,
                )
                .expect("udp send_bytes should succeed")
        ),
        Value::Unit
    );
    let udp_recv = result_ok_payload(
        runtime
            .evaluate_udp_socket_method(
                udp_receiver.clone(),
                "recv",
                &[
                    mir_arg(Some("max_bytes"), Operand::Int(16)),
                    mir_arg(Some("timeout"), Operand::Duration(1_000)),
                ],
                &env,
            )
            .expect("udp recv should succeed"),
    );
    enum_payloads(udp_recv, "Option", "Some");
    let udp_recv_from = result_ok_payload(
        runtime
            .evaluate_udp_socket_method(
                udp_receiver.clone(),
                "recv_from",
                &[
                    mir_arg(Some("max_bytes"), Operand::Int(16)),
                    mir_arg(Some("timeout"), Operand::Duration(1_000)),
                ],
                &env,
            )
            .expect("udp recv_from should succeed"),
    );
    enum_payloads(udp_recv_from, "Option", "Some");
    match result_ok_payload(
        runtime
            .evaluate_udp_socket_method(udp_receiver.clone(), "local_addr", &[], &env)
            .expect("udp local_addr should succeed"),
    ) {
        Value::String(address) => assert!(address.starts_with("127.0.0.1:")),
        other => panic!("expected udp local address string, found {other:?}"),
    }
    enum_payloads(
        runtime
            .evaluate_udp_socket_method(udp_sender.clone(), "peer_addr", &[], &env)
            .expect("udp peer_addr should return a Result"),
        "Result",
        "Err",
    );
    assert_eq!(
        runtime
            .evaluate_udp_socket_method(udp_sender.clone(), "close", &[], &env)
            .expect("udp close should succeed"),
        Value::Unit
    );
    let udp_error = runtime
        .evaluate_udp_socket_method(udp_sender, "missing", &[], &env)
        .expect_err("unknown udp socket method should fail");
    assert!(udp_error
        .message
        .contains("unsupported MIR udp socket method"));
    udp_receiver.close();

    let datagram = UdpDatagramValue {
        address: "127.0.0.1:9".to_string(),
        data: b"text".to_vec(),
    };
    assert_eq!(
        runtime
            .evaluate_udp_datagram_method(datagram.clone(), "address", &[], &env)
            .expect("udp datagram address should succeed"),
        Value::String("127.0.0.1:9".to_string())
    );
    assert_eq!(
        runtime
            .evaluate_udp_datagram_method(datagram.clone(), "bytes", &[], &env)
            .expect("udp datagram bytes should succeed"),
        bytes_vec_value(b"text".to_vec())
    );
    assert_eq!(
        result_ok_payload(
            runtime
                .evaluate_udp_datagram_method(datagram.clone(), "text", &[], &env)
                .expect("udp datagram text should succeed")
        ),
        Value::String("text".to_string())
    );
    let datagram_error = runtime
        .evaluate_udp_datagram_method(datagram, "missing", &[], &env)
        .expect_err("unknown datagram method should fail");
    assert!(datagram_error
        .message
        .contains("unsupported MIR udp datagram method"));

    let http_listener = HttpListenerValue::bind("127.0.0.1:0").expect("http listener should bind");
    match result_ok_payload(
        runtime
            .evaluate_http_listener_method(http_listener.clone(), "local_addr", &[], &env)
            .expect("http listener local_addr should succeed"),
    ) {
        Value::String(address) => assert!(address.starts_with("127.0.0.1:")),
        other => panic!("expected http local address string, found {other:?}"),
    }
    enum_payloads(
        runtime
            .evaluate_http_listener_method(
                http_listener.clone(),
                "accept",
                &[mir_arg(Some("timeout"), Operand::Duration(1))],
                &env,
            )
            .expect("http listener accept should return a Result"),
        "Result",
        "Err",
    );
    assert_eq!(
        runtime
            .evaluate_http_listener_method(http_listener.clone(), "close", &[], &env)
            .expect("http listener close should succeed"),
        Value::Unit
    );
    let http_listener_error = runtime
        .evaluate_http_listener_method(http_listener, "missing", &[], &env)
        .expect_err("unknown http listener method should fail");
    assert!(http_listener_error
        .message
        .contains("unsupported MIR http listener method"));
}

#[test]
fn mir_runtime_network_member_helpers_cover_closed_and_validation_edges() {
    let mut runtime = test_runtime();
    let mut env = Env::default();
    env.define_typed(
        "bytes",
        Type::Named("Vec".to_string(), vec![Type::named("uint8")]),
        bytes_vec_value(b"payload".to_vec()),
    );
    env.define_typed(
        "negative",
        Type::named("int32"),
        Value::Int(IntegerValue::from_signed(-1)),
    );

    let tcp_listener = TcpListenerValue::bind("127.0.0.1:0").expect("tcp listener should bind");
    let tcp_address = tcp_listener
        .local_addr()
        .expect("tcp listener address should be available");
    let tcp_server = {
        let listener = tcp_listener.clone();
        thread::spawn(move || {
            listener
                .accept(
                    Some(StdDuration::from_secs(5)),
                    Some(&CancellationContext::default()),
                )
                .expect("tcp server should accept")
        })
    };
    let tcp_client = TcpStreamValue::connect(
        &tcp_address,
        Some(StdDuration::from_secs(5)),
        Some(&CancellationContext::default()),
    )
    .expect("tcp client should connect");
    let tcp_server_stream = tcp_server.join().expect("tcp server should join");
    tcp_client.close();
    tcp_server_stream.close();
    for method in [
        "read_all",
        "read_line",
        "flush",
        "local_addr",
        "peer_addr",
        "shutdown_read",
        "shutdown_write",
        "shutdown_both",
    ] {
        assert_result_err(
            runtime
                .evaluate_tcp_stream_method(tcp_client.clone(), method, &[], &env)
                .expect("closed tcp stream methods should return Result.Err"),
        );
    }
    for (method, args) in [
        (
            "read_bytes",
            vec![
                mir_arg(Some("max_bytes"), Operand::Int(4)),
                mir_arg(Some("timeout"), Operand::Unit),
            ],
        ),
        (
            "read_exact",
            vec![
                mir_arg(Some("count"), Operand::Int(4)),
                mir_arg(Some("timeout"), Operand::Unit),
            ],
        ),
        (
            "write_bytes",
            vec![
                mir_arg(Some("bytes"), Operand::Place("bytes".to_string())),
                mir_arg(Some("timeout"), Operand::Unit),
            ],
        ),
    ] {
        assert_result_err(
            runtime
                .evaluate_tcp_stream_method(tcp_client.clone(), method, &args, &env)
                .expect("closed tcp stream method should return Result.Err"),
        );
    }
    let negative_tcp_read = runtime
        .evaluate_tcp_stream_method(
            tcp_client.clone(),
            "read_bytes",
            &[
                mir_arg(Some("max_bytes"), Operand::Place("negative".to_string())),
                mir_arg(Some("timeout"), Operand::Unit),
            ],
            &env,
        )
        .expect_err("negative tcp read size should fail before IO");
    assert!(negative_tcp_read
        .message
        .contains("requires a non-negative max_bytes"));
    let bad_tcp_write = runtime
        .evaluate_tcp_stream_method(
            tcp_client.clone(),
            "write_all",
            &[
                mir_arg(Some("text"), Operand::Bool(true)),
                mir_arg(Some("timeout"), Operand::Unit),
            ],
            &env,
        )
        .expect_err("tcp write_all should reject non-string input");
    assert!(bad_tcp_write.message.contains("expects `String`"));

    tcp_listener.close();
    assert_result_err(
        runtime
            .evaluate_tcp_listener_method(tcp_listener.clone(), "local_addr", &[], &env)
            .expect("closed tcp listener local_addr should return Result.Err"),
    );
    assert_result_err(
        runtime
            .evaluate_tcp_listener_method(
                tcp_listener,
                "accept",
                &[mir_arg(Some("timeout"), Operand::Unit)],
                &env,
            )
            .expect("closed tcp listener accept should return Result.Err"),
    );

    let udp_socket = UdpSocketValue::bind("127.0.0.1:0").expect("udp socket should bind");
    let negative_udp_recv = runtime
        .evaluate_udp_socket_method(
            udp_socket.clone(),
            "recv",
            &[
                mir_arg(Some("max_bytes"), Operand::Place("negative".to_string())),
                mir_arg(Some("timeout"), Operand::Unit),
            ],
            &env,
        )
        .expect_err("negative udp recv size should fail before IO");
    assert!(negative_udp_recv
        .message
        .contains("requires a non-negative max_bytes"));
    let negative_udp_recv_from = runtime
        .evaluate_udp_socket_method(
            udp_socket.clone(),
            "recv_from",
            &[
                mir_arg(Some("max_bytes"), Operand::Place("negative".to_string())),
                mir_arg(Some("timeout"), Operand::Unit),
            ],
            &env,
        )
        .expect_err("negative udp recv_from size should fail before IO");
    assert!(negative_udp_recv_from
        .message
        .contains("requires a non-negative max_bytes"));
    udp_socket.close();
    assert_result_err(
        runtime
            .evaluate_udp_socket_method(udp_socket.clone(), "local_addr", &[], &env)
            .expect("closed udp local_addr should return Result.Err"),
    );
    assert_result_err(
        runtime
            .evaluate_udp_socket_method(udp_socket.clone(), "peer_addr", &[], &env)
            .expect("closed udp peer_addr should return Result.Err"),
    );
    assert_result_err(
        runtime
            .evaluate_udp_socket_method(
                udp_socket,
                "send_bytes",
                &[
                    mir_arg(Some("address"), Operand::String("127.0.0.1:9".to_string())),
                    mir_arg(Some("bytes"), Operand::Place("bytes".to_string())),
                    mir_arg(Some("timeout"), Operand::Unit),
                ],
                &env,
            )
            .expect("closed udp send_bytes should return Result.Err"),
    );

    let invalid_datagram = UdpDatagramValue {
        address: "127.0.0.1:9".to_string(),
        data: vec![0xff, 0xfe],
    };
    assert_result_err(
        runtime
            .evaluate_udp_datagram_method(invalid_datagram, "text", &[], &env)
            .expect("invalid utf-8 datagram text should return Result.Err"),
    );

    let http_listener = HttpListenerValue::bind("127.0.0.1:0").expect("http listener should bind");
    http_listener.close();
    assert_result_err(
        runtime
            .evaluate_http_listener_method(http_listener.clone(), "local_addr", &[], &env)
            .expect("closed http listener local_addr should return Result.Err"),
    );
    assert_result_err(
        runtime
            .evaluate_http_listener_method(
                http_listener,
                "accept",
                &[mir_arg(Some("timeout"), Operand::Unit)],
                &env,
            )
            .expect("closed http listener accept should return Result.Err"),
    );

    #[cfg(unix)]
    {
        let timestamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("system time should be after epoch")
            .as_nanos();
        let socket_path =
            std::path::PathBuf::from(format!("/tmp/aum{}-{timestamp}.sock", std::process::id()));
        let _ = std::fs::remove_file(&socket_path);
        let socket_text = socket_path.to_string_lossy().into_owned();
        let unix_listener =
            UnixListenerValue::bind(&socket_text).expect("unix listener should bind");
        let unix_server = {
            let listener = unix_listener.clone();
            thread::spawn(move || {
                listener
                    .accept(
                        Some(StdDuration::from_secs(5)),
                        Some(&CancellationContext::default()),
                    )
                    .expect("unix server should accept")
            })
        };
        let unix_client = UnixStreamValue::connect(
            &socket_text,
            Some(StdDuration::from_secs(5)),
            Some(&CancellationContext::default()),
        )
        .expect("unix client should connect");
        let unix_server_stream = unix_server.join().expect("unix server should join");
        unix_client.close();
        unix_server_stream.close();
        assert_result_err(
            runtime
                .evaluate_unix_stream_method(
                    unix_client.clone(),
                    "read_line",
                    &[mir_arg(Some("timeout"), Operand::Unit)],
                    &env,
                )
                .expect("closed unix read_line should return Result.Err"),
        );
        assert_result_err(
            runtime
                .evaluate_unix_stream_method(
                    unix_client.clone(),
                    "read_exact",
                    &[
                        mir_arg(Some("count"), Operand::Int(4)),
                        mir_arg(Some("timeout"), Operand::Unit),
                    ],
                    &env,
                )
                .expect("closed unix read_exact should return Result.Err"),
        );
        assert_result_err(
            runtime
                .evaluate_unix_stream_method(
                    unix_client.clone(),
                    "write_all",
                    &[
                        mir_arg(Some("text"), Operand::String("closed".to_string())),
                        mir_arg(Some("timeout"), Operand::Unit),
                    ],
                    &env,
                )
                .expect("closed unix write_all should return Result.Err"),
        );
        let negative_unix_read = runtime
            .evaluate_unix_stream_method(
                unix_client,
                "read_exact",
                &[
                    mir_arg(Some("count"), Operand::Place("negative".to_string())),
                    mir_arg(Some("timeout"), Operand::Unit),
                ],
                &env,
            )
            .expect_err("negative unix read_exact size should fail before IO");
        assert!(negative_unix_read
            .message
            .contains("requires a non-negative count"));
        unix_listener.close();
        assert_result_err(
            runtime
                .evaluate_unix_listener_method(
                    unix_listener,
                    "accept",
                    &[mir_arg(Some("timeout"), Operand::Unit)],
                    &env,
                )
                .expect("closed unix listener accept should return Result.Err"),
        );
        let _ = std::fs::remove_file(&socket_path);
    }
}

#[test]
fn mir_runtime_stream_and_http_member_helpers_cover_resource_branches() {
    let mut runtime = test_runtime();
    let mut env = Env::default();
    env.define_typed(
        "bytes",
        Type::Named("Vec".to_string(), vec![Type::named("uint8")]),
        bytes_vec_value(b"client-bytes".to_vec()),
    );

    let tcp_listener = TcpListenerValue::bind("127.0.0.1:0").expect("tcp listener should bind");
    let tcp_address = tcp_listener
        .local_addr()
        .expect("tcp listener address should be available");
    let tcp_server = {
        let listener = tcp_listener.clone();
        thread::spawn(move || {
            let stream = listener
                .accept(
                    Some(StdDuration::from_secs(5)),
                    Some(&CancellationContext::default()),
                )
                .expect("tcp server should accept");
            let request = stream
                .read_line(
                    Some(StdDuration::from_secs(5)),
                    Some(&CancellationContext::default()),
                )
                .expect("tcp server should read a line");
            assert_eq!(request.as_deref(), Some("ping"));
            stream
                .write_all(
                    "pong\nextra",
                    Some(StdDuration::from_secs(5)),
                    Some(&CancellationContext::default()),
                )
                .expect("tcp server should write response");
            stream.flush().expect("tcp server flush should succeed");
            stream.close();
        })
    };
    let tcp_client = TcpStreamValue::connect(
        &tcp_address,
        Some(StdDuration::from_secs(5)),
        Some(&CancellationContext::default()),
    )
    .expect("tcp client should connect");
    assert_eq!(
        result_ok_payload(
            runtime
                .evaluate_tcp_stream_method(
                    tcp_client.clone(),
                    "write_all",
                    &[
                        mir_arg(Some("text"), Operand::String("ping\n".to_string())),
                        mir_arg(Some("timeout"), Operand::Duration(5_000)),
                    ],
                    &env,
                )
                .expect("tcp write_all should succeed")
        ),
        Value::Unit
    );
    assert_eq!(
        result_ok_payload(
            runtime
                .evaluate_tcp_stream_method(tcp_client.clone(), "flush", &[], &env)
                .expect("tcp flush should succeed")
        ),
        Value::Unit
    );
    assert_eq!(
        result_ok_payload(
            runtime
                .evaluate_tcp_stream_method(tcp_client.clone(), "shutdown_write", &[], &env)
                .expect("tcp shutdown_write should succeed")
        ),
        Value::Unit
    );
    match result_ok_payload(
        runtime
            .evaluate_tcp_stream_method(tcp_client.clone(), "local_addr", &[], &env)
            .expect("tcp local_addr should succeed"),
    ) {
        Value::String(address) => assert!(address.starts_with("127.0.0.1:")),
        other => panic!("expected tcp local address string, found {other:?}"),
    }
    match result_ok_payload(
        runtime
            .evaluate_tcp_stream_method(tcp_client.clone(), "peer_addr", &[], &env)
            .expect("tcp peer_addr should succeed"),
    ) {
        Value::String(address) => assert_eq!(address, tcp_address),
        other => panic!("expected tcp peer address string, found {other:?}"),
    }
    let tcp_line = result_ok_payload(
        runtime
            .evaluate_tcp_stream_method(
                tcp_client.clone(),
                "read_line",
                &[mir_arg(Some("timeout"), Operand::Duration(5_000))],
                &env,
            )
            .expect("tcp read_line should succeed"),
    );
    let line_payloads = enum_payloads(tcp_line, "Option", "Some");
    assert_eq!(line_payloads, vec![Value::String("pong".to_string())]);
    assert_eq!(
        result_ok_payload(
            runtime
                .evaluate_tcp_stream_method(
                    tcp_client.clone(),
                    "read_exact",
                    &[
                        mir_arg(Some("count"), Operand::Int(5)),
                        mir_arg(Some("timeout"), Operand::Duration(5_000)),
                    ],
                    &env,
                )
                .expect("tcp read_exact should succeed")
        ),
        bytes_vec_value(b"extra".to_vec())
    );
    let no_more_tcp = result_ok_payload(
        runtime
            .evaluate_tcp_stream_method(
                tcp_client.clone(),
                "read_bytes",
                &[
                    mir_arg(Some("max_bytes"), Operand::Int(4)),
                    mir_arg(Some("timeout"), Operand::Duration(50)),
                ],
                &env,
            )
            .expect("tcp read_bytes should return a Result"),
    );
    enum_payloads(no_more_tcp, "Option", "None");
    assert_eq!(
        runtime
            .evaluate_tcp_stream_method(tcp_client.clone(), "close", &[], &env)
            .expect("tcp close should succeed"),
        Value::Unit
    );
    let tcp_error = runtime
        .evaluate_tcp_stream_method(tcp_client, "missing", &[], &env)
        .expect_err("unknown tcp stream method should fail");
    assert!(tcp_error
        .message
        .contains("unsupported MIR tcp stream method"));
    tcp_server.join().expect("tcp server should join");
    tcp_listener.close();

    #[cfg(unix)]
    {
        let timestamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("system time should be after epoch")
            .as_nanos();
        let socket_path = std::env::temp_dir().join(format!(
            "aurora-mir-runtime-unix-{}-{timestamp}.sock",
            std::process::id()
        ));
        let _ = std::fs::remove_file(&socket_path);
        let socket_text = socket_path.to_string_lossy().into_owned();
        let unix_listener =
            UnixListenerValue::bind(&socket_text).expect("unix listener should bind");
        let unix_server = {
            let listener = unix_listener.clone();
            thread::spawn(move || {
                let stream = listener
                    .accept(
                        Some(StdDuration::from_secs(5)),
                        Some(&CancellationContext::default()),
                    )
                    .expect("unix server should accept");
                stream
                    .write_all(
                        "exact",
                        Some(StdDuration::from_secs(5)),
                        Some(&CancellationContext::default()),
                    )
                    .expect("unix server should write");
                stream.close();
            })
        };
        let unix_client = UnixStreamValue::connect(
            &socket_text,
            Some(StdDuration::from_secs(5)),
            Some(&CancellationContext::default()),
        )
        .expect("unix client should connect");
        assert_eq!(
            result_ok_payload(
                runtime
                    .evaluate_unix_stream_method(
                        unix_client.clone(),
                        "read_exact",
                        &[
                            mir_arg(Some("count"), Operand::Int(5)),
                            mir_arg(Some("timeout"), Operand::Duration(5_000)),
                        ],
                        &env,
                    )
                    .expect("unix read_exact should succeed")
            ),
            bytes_vec_value(b"exact".to_vec())
        );
        assert_eq!(
            runtime
                .evaluate_unix_stream_method(unix_client.clone(), "close", &[], &env)
                .expect("unix stream close should succeed"),
            Value::Unit
        );
        unix_server.join().expect("unix server should join");
        unix_listener.close();
        let _ = std::fs::remove_file(&socket_path);
    }

    let websocket_listener =
        WebSocketListenerValue::bind("127.0.0.1:0").expect("websocket listener should bind");
    assert_eq!(
        MirRuntime::infer_value_type(&Value::WebSocketListener(websocket_listener.clone())),
        Some(Type::named("net.WebSocketListener"))
    );
    let websocket_address = websocket_listener
        .local_addr()
        .expect("websocket listener address should be available");
    let websocket_server = {
        let listener = websocket_listener.clone();
        thread::spawn(move || {
            let socket = listener
                .accept(Some(StdDuration::from_secs(5)))
                .expect("websocket server should accept");
            let mut server_runtime = test_runtime();
            let mut server_env = Env::default();
            server_env.define_typed(
                "bytes",
                Type::Named("Vec".to_string(), vec![Type::named("uint8")]),
                bytes_vec_value(b"server-bytes".to_vec()),
            );
            let client_text = result_ok_payload(
                server_runtime
                    .evaluate_websocket_method(
                        socket.clone(),
                        "recv_text",
                        &[mir_arg(Some("timeout"), Operand::Duration(5_000))],
                        &server_env,
                    )
                    .expect("websocket recv_text should succeed"),
            );
            assert_eq!(
                enum_payloads(client_text, "Option", "Some"),
                vec![Value::String("hello websocket".to_string())]
            );
            assert_eq!(
                result_ok_payload(
                    server_runtime
                        .evaluate_websocket_method(
                            socket.clone(),
                            "send_bytes",
                            &[
                                mir_arg(Some("bytes"), Operand::Place("bytes".to_string())),
                                mir_arg(Some("timeout"), Operand::Duration(5_000)),
                            ],
                            &server_env,
                        )
                        .expect("websocket send_bytes should succeed")
                ),
                Value::Unit
            );
            let client_bytes = result_ok_payload(
                server_runtime
                    .evaluate_websocket_method(
                        socket.clone(),
                        "recv_bytes",
                        &[mir_arg(Some("timeout"), Operand::Duration(5_000))],
                        &server_env,
                    )
                    .expect("websocket recv_bytes should succeed"),
            );
            assert_eq!(
                enum_payloads(client_bytes, "Option", "Some"),
                vec![bytes_vec_value(b"client-bytes".to_vec())]
            );
            assert_eq!(
                result_ok_payload(
                    server_runtime
                        .evaluate_websocket_method(
                            socket.clone(),
                            "send_text",
                            &[
                                mir_arg(Some("text"), Operand::String("server-done".to_string())),
                                mir_arg(Some("timeout"), Operand::Duration(5_000)),
                            ],
                            &server_env,
                        )
                        .expect("websocket send_text should succeed")
                ),
                Value::Unit
            );
            assert_eq!(
                server_runtime
                    .evaluate_websocket_method(socket, "close", &[], &server_env)
                    .expect("websocket close should succeed"),
                Value::Unit
            );
        })
    };
    let websocket_client = WebSocketValue::connect(
        &format!("ws://{websocket_address}"),
        Some(StdDuration::from_secs(5)),
    )
    .expect("websocket client should connect");
    assert_eq!(
        MirRuntime::infer_value_type(&Value::WebSocket(websocket_client.clone())),
        Some(Type::named("net.WebSocket"))
    );
    assert_eq!(
        result_ok_payload(
            runtime
                .evaluate_websocket_method(
                    websocket_client.clone(),
                    "send_text",
                    &[
                        mir_arg(Some("text"), Operand::String("hello websocket".to_string())),
                        mir_arg(Some("timeout"), Operand::Duration(5_000)),
                    ],
                    &env,
                )
                .expect("websocket client send_text should succeed")
        ),
        Value::Unit
    );
    let server_bytes = result_ok_payload(
        runtime
            .evaluate_websocket_method(
                websocket_client.clone(),
                "recv_bytes",
                &[mir_arg(Some("timeout"), Operand::Duration(5_000))],
                &env,
            )
            .expect("websocket client recv_bytes should succeed"),
    );
    assert_eq!(
        enum_payloads(server_bytes, "Option", "Some"),
        vec![bytes_vec_value(b"server-bytes".to_vec())]
    );
    assert_eq!(
        result_ok_payload(
            runtime
                .evaluate_websocket_method(
                    websocket_client.clone(),
                    "send_bytes",
                    &[
                        mir_arg(Some("bytes"), Operand::Place("bytes".to_string())),
                        mir_arg(Some("timeout"), Operand::Duration(5_000)),
                    ],
                    &env,
                )
                .expect("websocket client send_bytes should succeed")
        ),
        Value::Unit
    );
    let done = result_ok_payload(
        runtime
            .evaluate_websocket_method(
                websocket_client.clone(),
                "recv_text",
                &[mir_arg(Some("timeout"), Operand::Duration(5_000))],
                &env,
            )
            .expect("websocket client recv_text should succeed"),
    );
    assert_eq!(
        enum_payloads(done, "Option", "Some"),
        vec![Value::String("server-done".to_string())]
    );
    assert_eq!(
        runtime
            .evaluate_websocket_method(websocket_client, "close", &[], &env)
            .expect("websocket client close should succeed"),
        Value::Unit
    );
    websocket_server
        .join()
        .expect("websocket server should join");

    let http_listener = HttpListenerValue::bind("127.0.0.1:0").expect("http listener should bind");
    let http_address = http_listener
        .local_addr()
        .expect("http listener address should be available");
    let http_server = {
        let listener = http_listener.clone();
        thread::spawn(move || {
            let mut server_runtime = test_runtime();
            let mut server_env = Env::default();
            server_env.define_typed(
                "headers",
                Type::Named(
                    "Map".to_string(),
                    vec![Type::named("String"), Type::named("String")],
                ),
                Value::Map(crate::runtime_value::MapValue {
                    key_type: Type::named("String"),
                    value_type: Type::named("String"),
                    entries: vec![(
                        Value::String("Content-Type".to_string()),
                        Value::String("text/plain".to_string()),
                    )],
                }),
            );
            server_env.define_typed(
                "bytes",
                Type::Named("Vec".to_string(), vec![Type::named("uint8")]),
                bytes_vec_value(b"bytes-reply".to_vec()),
            );
            let exchange = listener
                .accept(
                    Some(StdDuration::from_secs(5)),
                    Some(&CancellationContext::default()),
                )
                .expect("http server should accept");
            assert_eq!(
                server_runtime
                    .evaluate_http_exchange_method(exchange.clone(), "method", &[], &server_env)
                    .expect("http method should succeed"),
                Value::String("POST".to_string())
            );
            assert_eq!(
                server_runtime
                    .evaluate_http_exchange_method(exchange.clone(), "path", &[], &server_env)
                    .expect("http path should succeed"),
                Value::String("/demo".to_string())
            );
            match server_runtime
                .evaluate_http_exchange_method(exchange.clone(), "headers", &[], &server_env)
                .expect("http headers should succeed")
            {
                Value::Map(headers) => assert!(!headers.entries.is_empty()),
                other => panic!("expected header map, found {other:?}"),
            }
            assert_eq!(
                result_ok_payload(
                    server_runtime
                        .evaluate_http_exchange_method(
                            exchange.clone(),
                            "body_text",
                            &[],
                            &server_env,
                        )
                        .expect("http body_text should succeed")
                ),
                Value::String("body".to_string())
            );
            assert_eq!(
                server_runtime
                    .evaluate_http_exchange_method(exchange.clone(), "body_bytes", &[], &server_env)
                    .expect("http body_bytes should succeed"),
                bytes_vec_value(b"body".to_vec())
            );
            assert_eq!(
                result_ok_payload(
                    server_runtime
                        .evaluate_http_exchange_method(
                            exchange.clone(),
                            "respond_text",
                            &[
                                mir_arg(Some("status"), Operand::Int(200)),
                                mir_arg(Some("text"), Operand::String("reply".to_string())),
                                mir_arg(Some("headers"), Operand::Place("headers".to_string())),
                            ],
                            &server_env,
                        )
                        .expect("http respond_text should succeed")
                ),
                Value::Unit
            );
            let exchange_error = server_runtime
                .evaluate_http_exchange_method(exchange, "missing", &[], &server_env)
                .expect_err("unknown exchange method should fail");
            assert!(exchange_error
                .message
                .contains("unsupported MIR http exchange method"));

            let exchange = listener
                .accept(
                    Some(StdDuration::from_secs(5)),
                    Some(&CancellationContext::default()),
                )
                .expect("http server should accept a bytes request");
            assert_eq!(
                server_runtime
                    .evaluate_http_exchange_method(exchange.clone(), "body_bytes", &[], &server_env)
                    .expect("http body_bytes should succeed"),
                bytes_vec_value(b"bytes".to_vec())
            );
            assert_eq!(
                result_ok_payload(
                    server_runtime
                        .evaluate_http_exchange_method(
                            exchange,
                            "respond_bytes",
                            &[
                                mir_arg(Some("status"), Operand::Int(200)),
                                mir_arg(Some("bytes"), Operand::Place("bytes".to_string())),
                                mir_arg(Some("headers"), Operand::Place("headers".to_string())),
                            ],
                            &server_env,
                        )
                        .expect("respond_bytes should succeed")
                ),
                Value::Unit
            );
        })
    };
    let response = HttpResponseValue::request_text(
        "POST",
        &format!("http://{http_address}/demo"),
        "body",
        vec![("Content-Type".to_string(), "text/plain".to_string())],
        Some(StdDuration::from_secs(5)),
        Some(&CancellationContext::default()),
    )
    .expect("http request should succeed");
    assert_eq!(
        runtime
            .evaluate_http_response_method(response.clone(), "status", &[], &env)
            .expect("http response status should succeed"),
        Value::Int(IntegerValue::from_signed(200))
    );
    assert_eq!(
        runtime
            .evaluate_http_response_method(response.clone(), "reason", &[], &env)
            .expect("http response reason should succeed"),
        Value::String("OK".to_string())
    );
    match runtime
        .evaluate_http_response_method(response.clone(), "headers", &[], &env)
        .expect("http response headers should succeed")
    {
        Value::Map(headers) => assert!(!headers.entries.is_empty()),
        other => panic!("expected response header map, found {other:?}"),
    }
    assert_eq!(
        result_ok_payload(
            runtime
                .evaluate_http_response_method(response.clone(), "text", &[], &env)
                .expect("http response text should succeed")
        ),
        Value::String("reply".to_string())
    );
    assert_eq!(
        runtime
            .evaluate_http_response_method(response.clone(), "bytes", &[], &env)
            .expect("http response bytes should succeed"),
        bytes_vec_value(b"reply".to_vec())
    );
    let bytes_response = HttpResponseValue::request_bytes(
        "POST",
        &format!("http://{http_address}/demo-bytes"),
        b"bytes",
        vec![(
            "Content-Type".to_string(),
            "application/octet-stream".to_string(),
        )],
        Some(StdDuration::from_secs(5)),
        Some(&CancellationContext::default()),
    )
    .expect("http bytes request should succeed");
    assert_eq!(
        runtime
            .evaluate_http_response_method(bytes_response, "bytes", &[], &env)
            .expect("http bytes response should succeed"),
        bytes_vec_value(b"bytes-reply".to_vec())
    );
    let response_error = runtime
        .evaluate_http_response_method(response, "missing", &[], &env)
        .expect_err("unknown response method should fail");
    assert!(response_error
        .message
        .contains("unsupported MIR http response method"));
    http_server.join().expect("http server should join");
    http_listener.close();
}

struct FlushFailWriter;

impl Write for FlushFailWriter {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        Ok(buf.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Err(io::Error::new(io::ErrorKind::BrokenPipe, "broken pipe"))
    }
}

struct WriteFailWriter {
    kind: io::ErrorKind,
}

impl Write for WriteFailWriter {
    fn write(&mut self, _buf: &[u8]) -> io::Result<usize> {
        Err(io::Error::new(self.kind, "write failed"))
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

#[test]
fn mir_runtime_stream_and_entrypoint_helpers_cover_success_and_error_paths() {
    let flush_error = write_stream(&mut FlushFailWriter, "aurora")
        .expect_err("flush failures should be surfaced");
    assert_eq!(flush_error.kind(), io::ErrorKind::BrokenPipe);

    let source = "def main() -> int32:\n    return 3\n";
    let mir = crate::lower_source_to_mir(source).expect("source should lower to MIR");
    let mir_json = serde_json::to_vec(&mir).expect("MIR should serialize");
    let source_path = b"/tmp/runtime_entry.au";
    let code = run_native_entry(
        mir_json.as_ptr(),
        mir_json.len(),
        source_path.as_ptr(),
        source_path.len(),
        source.as_ptr(),
        source.len(),
    );
    assert_eq!(code, 3);

    let invalid_json = b"not-json";
    let invalid_code = run_native_entry(
        invalid_json.as_ptr(),
        invalid_json.len(),
        source_path.as_ptr(),
        source_path.len(),
        source.as_ptr(),
        source.len(),
    );
    assert_eq!(invalid_code, 1);

    let tiny = [b'x'];
    let oversized_code = run_native_entry(
        tiny.as_ptr(),
        (1 << 30) + 1,
        source_path.as_ptr(),
        source_path.len(),
        source.as_ptr(),
        source.len(),
    );
    assert_eq!(oversized_code, 1);

    let stdout_source = "def main() -> int32:\n    print(\"before\")\n    return 5\n";
    let stdout_mir =
        crate::lower_source_to_mir(stdout_source).expect("stdout source should lower to MIR");
    let stdout_mir_json = serde_json::to_vec(&stdout_mir).expect("stdout MIR should serialize");
    let mut captured_stdout = Vec::new();
    let mut captured_stderr = Vec::new();
    let stdout_code = super::run_serialized_mir_entrypoint_with_streams(
        &stdout_mir_json,
        "/tmp/stdout.au",
        stdout_source,
        &mut captured_stdout,
        &mut captured_stderr,
    );
    assert_eq!(stdout_code, 5);
    assert_eq!(String::from_utf8(captured_stdout).unwrap(), "before\n");
    assert!(captured_stderr.is_empty());

    let unit_source = "def main():\n    print(\"unit\")\n";
    let unit_mir =
        crate::lower_source_to_mir(unit_source).expect("unit source should lower to MIR");
    let unit_mir_json = serde_json::to_vec(&unit_mir).expect("unit MIR should serialize");
    let mut unit_stdout = Vec::new();
    let mut unit_stderr = Vec::new();
    let unit_code = super::run_serialized_mir_entrypoint_with_streams(
        &unit_mir_json,
        "/tmp/unit.au",
        unit_source,
        &mut unit_stdout,
        &mut unit_stderr,
    );
    assert_eq!(unit_code, 0);
    assert_eq!(String::from_utf8(unit_stdout).unwrap(), "unit\n");
    assert!(unit_stderr.is_empty());

    let mut broken_stdout = WriteFailWriter {
        kind: io::ErrorKind::BrokenPipe,
    };
    let mut ignored_stderr = Vec::new();
    let broken_stdout_code = super::run_serialized_mir_entrypoint_with_streams(
        &stdout_mir_json,
        "/tmp/stdout.au",
        stdout_source,
        &mut broken_stdout,
        &mut ignored_stderr,
    );
    assert_eq!(broken_stdout_code, 0);
    assert!(ignored_stderr.is_empty());

    let mut failing_stdout = WriteFailWriter {
        kind: io::ErrorKind::PermissionDenied,
    };
    let mut write_error_stderr = Vec::new();
    let write_error_code = super::run_serialized_mir_entrypoint_with_streams(
        &stdout_mir_json,
        "/tmp/stdout.au",
        stdout_source,
        &mut failing_stdout,
        &mut write_error_stderr,
    );
    assert_eq!(write_error_code, 1);
    assert!(String::from_utf8(write_error_stderr)
        .unwrap()
        .contains("failed to write to stdout"));

    let error_source = "def main() -> int32:\n    print(\"before\")\n    return 1 / 0\n";
    let error_mir =
        crate::lower_source_to_mir(error_source).expect("error source should lower to MIR");
    let error_mir_json = serde_json::to_vec(&error_mir).expect("error MIR should serialize");

    let mut broken_error_stdout = WriteFailWriter {
        kind: io::ErrorKind::BrokenPipe,
    };
    let mut ignored_error_stderr = Vec::new();
    let broken_error_code = super::run_serialized_mir_entrypoint_with_streams(
        &error_mir_json,
        "/tmp/error.au",
        error_source,
        &mut broken_error_stdout,
        &mut ignored_error_stderr,
    );
    assert_eq!(broken_error_code, 0);
    assert!(ignored_error_stderr.is_empty());

    let mut failing_error_stdout = WriteFailWriter {
        kind: io::ErrorKind::PermissionDenied,
    };
    let mut error_write_stderr = Vec::new();
    let error_write_code = super::run_serialized_mir_entrypoint_with_streams(
        &error_mir_json,
        "/tmp/error.au",
        error_source,
        &mut failing_error_stdout,
        &mut error_write_stderr,
    );
    assert_eq!(error_write_code, 1);
    assert!(String::from_utf8(error_write_stderr)
        .unwrap()
        .contains("failed to write to stdout"));

    let mut partial_error_stdout = Vec::new();
    let mut rendered_partial_error_stderr = Vec::new();
    let partial_error_code = super::run_serialized_mir_entrypoint_with_streams(
        &error_mir_json,
        "/tmp/error.au",
        error_source,
        &mut partial_error_stdout,
        &mut rendered_partial_error_stderr,
    );
    assert_eq!(partial_error_code, 1);
    assert_eq!(String::from_utf8(partial_error_stdout).unwrap(), "before\n");
    assert!(String::from_utf8(rendered_partial_error_stderr)
        .unwrap()
        .contains("division by zero"));

    let mut rendered_error_stdout = Vec::new();
    let mut rendered_error_stderr = Vec::new();
    let rendered_error_code = super::run_serialized_mir_entrypoint_with_streams(
        invalid_json,
        "/tmp/error.au",
        error_source,
        &mut rendered_error_stdout,
        &mut rendered_error_stderr,
    );
    assert_eq!(rendered_error_code, 1);
    assert!(rendered_error_stdout.is_empty());
    assert!(String::from_utf8(rendered_error_stderr)
        .unwrap()
        .contains("failed to deserialize embedded MIR"));
}

#[test]
fn mir_runtime_public_run_wrappers_cover_serialized_success_and_error_paths() {
    let source = "def main() -> int32:\n    return 7\n";
    let mir = crate::lower_source_to_mir(source).expect("source should lower to MIR");

    let output = crate::mir_runtime::run(&mir).expect("run wrapper should succeed");
    assert_eq!(output.value, Value::Int(IntegerValue::from_signed(7)));
    assert_eq!(output.stdout, "");

    let serialized = serde_json::to_vec(&mir).expect("MIR should serialize");
    let from_json = run_serialized_mir(&serialized, "/tmp/demo.au", source)
        .expect("serialized MIR should execute");
    assert_eq!(from_json.value, Value::Int(IntegerValue::from_signed(7)));

    let error = run_serialized_mir(b"{", "/tmp/demo.au", source)
        .expect_err("invalid serialized MIR should fail");
    assert!(error.message.contains("failed to deserialize embedded MIR"));
}

#[test]
fn mir_runtime_argument_binding_helpers_cover_named_and_positional_cases() {
    let mut env = Env::default();
    env.define_typed(
        "count",
        Type::named("int32"),
        Value::Int(IntegerValue::from_signed(7)),
    );
    let evaluated = evaluate_named_args(
        &[
            MirArg {
                name: Some("value".to_string()),
                value: Operand::Place("count".to_string()),
                writeback_place: None,
            },
            MirArg {
                name: None,
                value: Operand::Bool(true),
                writeback_place: Some("flag".to_string()),
            },
        ],
        &env,
    )
    .expect("args should evaluate");
    assert_eq!(evaluated[0].value, Value::Int(IntegerValue::from_signed(7)));
    assert_eq!(evaluated[1].writeback_place.as_deref(), Some("flag"));

    let bound = bind_builtin_args(
        &["left", "right"],
        vec![
            EvaluatedMirArg {
                name: Some("right".to_string()),
                value: Value::Int(IntegerValue::from_signed(2)),
                writeback_place: None,
            },
            EvaluatedMirArg {
                name: None,
                value: Value::Int(IntegerValue::from_signed(1)),
                writeback_place: None,
            },
        ],
    )
    .expect("args should bind");
    assert_eq!(bound[0].value, Value::Int(IntegerValue::from_signed(1)));
    assert_eq!(bound[1].name.as_deref(), Some("right"));

    let named_first_then_positional = bind_builtin_args(
        &["value", "timeout"],
        vec![
            EvaluatedMirArg {
                name: Some("value".to_string()),
                value: Value::Int(IntegerValue::from_signed(9)),
                writeback_place: None,
            },
            EvaluatedMirArg {
                name: None,
                value: Value::Duration(25),
                writeback_place: None,
            },
        ],
    )
    .expect("positional MIR args should skip named slots");
    assert_eq!(
        named_first_then_positional[0].value,
        Value::Int(IntegerValue::from_signed(9))
    );
    assert_eq!(named_first_then_positional[1].value, Value::Duration(25));

    let params = vec![
        MirParam {
            name: "left".to_string(),
            passing: crate::mir::MirReceiverKind::Value,
            ty: Type::named("int32"),
        },
        MirParam {
            name: "right".to_string(),
            passing: crate::mir::MirReceiverKind::Value,
            ty: Type::named("int32"),
        },
    ];
    let rebound = bind_args(&params, bound.clone()).expect("mir params should bind");
    assert_eq!(rebound.len(), 2);

    let missing = bind_builtin_args(
        &["value"],
        vec![EvaluatedMirArg {
            name: Some("other".to_string()),
            value: Value::Bool(true),
            writeback_place: None,
        }],
    )
    .err()
    .expect("unknown MIR argument should fail");
    assert!(missing.message.contains("unknown MIR argument"));

    let duplicate = bind_builtin_args(
        &["value"],
        vec![
            EvaluatedMirArg {
                name: Some("value".to_string()),
                value: Value::Int(IntegerValue::from_signed(1)),
                writeback_place: None,
            },
            EvaluatedMirArg {
                name: Some("value".to_string()),
                value: Value::Int(IntegerValue::from_signed(2)),
                writeback_place: None,
            },
        ],
    )
    .expect("duplicate named MIR arguments should keep the last value");
    assert_eq!(duplicate[0].value, Value::Int(IntegerValue::from_signed(2)));

    let too_many = bind_builtin_args(
        &["value"],
        vec![
            EvaluatedMirArg {
                name: None,
                value: Value::Bool(true),
                writeback_place: None,
            },
            EvaluatedMirArg {
                name: None,
                value: Value::Bool(false),
                writeback_place: None,
            },
        ],
    )
    .err()
    .expect("extra positional MIR arguments should fail");
    assert!(too_many.message.contains("too many MIR arguments"));

    let optional_named_then_positional = bind_optional_builtin_args(
        &["left", "right"],
        vec![
            EvaluatedMirArg {
                name: Some("left".to_string()),
                value: Value::Int(IntegerValue::from_signed(1)),
                writeback_place: None,
            },
            EvaluatedMirArg {
                name: None,
                value: Value::Int(IntegerValue::from_signed(2)),
                writeback_place: None,
            },
        ],
    )
    .expect("optional MIR args should skip pre-filled named slots");
    assert_eq!(
        optional_named_then_positional[0]
            .as_ref()
            .expect("left should be bound")
            .value,
        Value::Int(IntegerValue::from_signed(1))
    );
    assert_eq!(
        optional_named_then_positional[1]
            .as_ref()
            .expect("right should be bound")
            .value,
        Value::Int(IntegerValue::from_signed(2))
    );

    let optional_unknown = bind_optional_builtin_args(
        &["value"],
        vec![EvaluatedMirArg {
            name: Some("other".to_string()),
            value: Value::Int(IntegerValue::from_signed(1)),
            writeback_place: None,
        }],
    )
    .err()
    .expect("unknown optional MIR arguments should fail");
    assert!(optional_unknown.message.contains("unknown MIR argument"));

    let optional_too_many = match bind_optional_builtin_args(
        &["value"],
        vec![
            EvaluatedMirArg {
                name: Some("value".to_string()),
                value: Value::Int(IntegerValue::from_signed(1)),
                writeback_place: None,
            },
            EvaluatedMirArg {
                name: None,
                value: Value::Int(IntegerValue::from_signed(2)),
                writeback_place: None,
            },
        ],
    ) {
        Ok(_) => panic!("extra optional MIR args should fail"),
        Err(error) => error,
    };
    assert!(optional_too_many.message.contains("too many MIR arguments"));

    let missing_required = bind_builtin_args(
        &["left", "right"],
        vec![EvaluatedMirArg {
            name: None,
            value: Value::Int(IntegerValue::from_signed(1)),
            writeback_place: None,
        }],
    )
    .err()
    .expect("missing MIR arguments should fail");
    assert!(missing_required.message.contains("missing MIR argument"));

    let eval_error = evaluate_named_args(
        &[MirArg {
            name: Some("value".to_string()),
            value: Operand::Place("missing".to_string()),
            writeback_place: None,
        }],
        &env,
    )
    .err()
    .expect("reading a missing MIR place should fail");
    assert!(eval_error.message.contains("unknown MIR place `missing`"));

    let unit_value = evaluate_named_args(
        &[MirArg {
            name: Some("unit".to_string()),
            value: Operand::Unit,
            writeback_place: None,
        }],
        &env,
    )
    .expect("unit operands should evaluate");
    assert_eq!(unit_value[0].value, Value::Unit);
}

#[test]
fn mir_runtime_deadline_helper_rejects_overflowing_instants() {
    let error = super::runtime_deadline_after_timeout(Some(StdDuration::MAX))
        .expect_err("overflowing instant deadlines should be rejected");
    assert!(error
        .message
        .contains("overflows the MIR runtime deadline range"));
}

#[test]
fn mir_runtime_complexity_guard_rejects_excessive_instruction_counts() {
    super::validate_embedded_runtime_length("MIR payload", super::MAX_EMBEDDED_RUNTIME_BYTES)
        .expect("embedded runtime payloads at the limit should pass");
    let length_error = super::validate_embedded_runtime_length(
        "MIR payload",
        super::MAX_EMBEDDED_RUNTIME_BYTES + 1,
    )
    .expect_err("embedded runtime payloads above the limit should fail");
    assert!(length_error.contains("exceeds the supported runtime limit"));

    let block = |label: &str, terminator: Terminator| BasicBlock {
        label: label.to_string(),
        instructions: Vec::new(),
        terminator,
    };
    let module_with_blocks = |blocks: Vec<BasicBlock>| MirModule {
        functions: vec![MirFunction {
            name: "main".to_string(),
            module_name: "<test>".to_string(),
            span: Span::new(1, 1),
            receiver: None,
            params: Vec::new(),
            local_types: Vec::new(),
            return_type: Type::named("int32"),
            entry: "entry".to_string(),
            blocks,
        }],
        classes: Vec::new(),
        trait_impls: Vec::new(),
        top_level: None,
    };

    let module = MirModule {
        functions: vec![MirFunction {
            name: "main".to_string(),
            module_name: "<test>".to_string(),
            span: Span::new(1, 1),
            receiver: None,
            params: Vec::new(),
            local_types: Vec::new(),
            return_type: Type::named("int32"),
            entry: "entry".to_string(),
            blocks: vec![BasicBlock {
                label: "entry".to_string(),
                instructions: vec![
                    Instruction::Eval {
                        value: Operand::Unit
                    };
                    super::MAX_RUNTIME_INSTRUCTIONS + 1
                ],
                terminator: Terminator::Return(Operand::Int(0)),
            }],
        }],
        classes: Vec::new(),
        trait_impls: Vec::new(),
        top_level: None,
    };

    let error = super::validate_runtime_module_complexity(&module)
        .expect_err("oversized MIR modules should be rejected");
    assert!(error.message.contains("instruction limit"));

    let block_error = super::validate_runtime_module_complexity_with_limits(
        &module_with_blocks(vec![
            block("entry", Terminator::Goto("overflow".to_string())),
            block("overflow", Terminator::Return(Operand::Int(0))),
        ]),
        super::RuntimeModuleLimits {
            max_blocks: 1,
            max_instructions: 10,
            max_terminator_arms: 10,
        },
    )
    .expect_err("block-heavy MIR modules should be rejected");
    assert!(block_error.message.contains("block limit"));

    let arm_limit_module = module_with_blocks(vec![block(
        "entry",
        Terminator::Match {
            scrutinee: Operand::Bool(true),
            arms: vec![
                MirMatchArm {
                    enum_name: None,
                    variant_name: None,
                    wildcard: true,
                    label: "done".to_string(),
                },
                MirMatchArm {
                    enum_name: None,
                    variant_name: None,
                    wildcard: true,
                    label: "done".to_string(),
                },
            ],
            otherwise: "done".to_string(),
        },
    )]);
    let arm_error = super::validate_runtime_module_complexity_with_limits(
        &arm_limit_module,
        super::RuntimeModuleLimits {
            max_blocks: 10,
            max_instructions: 10,
            max_terminator_arms: 1,
        },
    )
    .expect_err("branch-heavy MIR modules should be rejected");
    assert!(arm_error.message.contains("branching-arm limit"));

    let match_module = module_with_blocks(vec![block(
        "entry",
        Terminator::Match {
            scrutinee: Operand::Bool(true),
            arms: vec![MirMatchArm {
                enum_name: None,
                variant_name: None,
                wildcard: true,
                label: "done".to_string(),
            }],
            otherwise: "done".to_string(),
        },
    )]);
    super::validate_runtime_module_complexity(&match_module)
        .expect("small match terminator modules should be accepted");
}

#[test]
fn mir_runtime_task_detection_helpers_cover_task_and_process_shapes() {
    let make_function = |name: &str, instructions: Vec<Instruction>| MirFunction {
        name: name.to_string(),
        module_name: "<test>".to_string(),
        span: Span::new(1, 1),
        receiver: None,
        params: Vec::new(),
        local_types: Vec::new(),
        return_type: Type::Unit,
        entry: "entry".to_string(),
        blocks: vec![BasicBlock {
            label: "entry".to_string(),
            instructions,
            terminator: Terminator::Return(Operand::Unit),
        }],
    };

    let ordinary = make_function(
        "ordinary",
        vec![
            Instruction::Eval {
                value: Operand::Unit,
            },
            Instruction::PushCleanup {
                place: "resource".to_string(),
            },
            Instruction::PopCleanup {
                place: "resource".to_string(),
                cancel_before_cleanup: false,
            },
            Instruction::Assign {
                target: "ignored".to_string(),
                value: Rvalue::Call {
                    callee: CallTarget::Member {
                        object: Operand::Place("service".to_string()),
                        field: "run".to_string(),
                        receiver_place: Some("service".to_string()),
                    },
                    args: Vec::new(),
                },
            },
            Instruction::Assign {
                target: "also_ignored".to_string(),
                value: Rvalue::Call {
                    callee: CallTarget::Name("print".to_string()),
                    args: Vec::new(),
                },
            },
        ],
    );
    assert!(!super::function_uses_lightweight_tasks(&ordinary));

    let started_task = make_function(
        "started_task",
        vec![Instruction::Assign {
            target: "task".to_string(),
            value: Rvalue::StartTask {
                returns_handle: true,
                task_group: Operand::Unit,
                function: "worker".to_string(),
                args: Vec::new(),
            },
        }],
    );
    assert!(super::function_uses_lightweight_tasks(&started_task));

    let process_run = make_function(
        "process_run",
        vec![Instruction::Assign {
            target: "completed".to_string(),
            value: Rvalue::Call {
                callee: CallTarget::Name("process::run".to_string()),
                args: Vec::new(),
            },
        }],
    );
    assert!(super::function_uses_lightweight_tasks(&process_run));

    let without_tasks = MirModule {
        functions: vec![ordinary.clone()],
        classes: Vec::new(),
        trait_impls: Vec::new(),
        top_level: None,
    };
    assert!(!super::module_uses_lightweight_tasks(&without_tasks));

    let with_top_level_process_run = MirModule {
        functions: vec![ordinary],
        classes: Vec::new(),
        trait_impls: Vec::new(),
        top_level: Some(process_run),
    };
    assert!(super::module_uses_lightweight_tasks(
        &with_top_level_process_run
    ));
}

#[test]
fn mir_runtime_writeback_and_spawn_helpers_cover_borrow_mut_edges() {
    let mut runtime = test_runtime();
    let mut env = Env::default();
    env.define_typed(
        "target",
        Type::named("int32"),
        Value::Int(IntegerValue::from_signed(1)),
    );

    let params = vec![
        MirParam {
            name: "source".to_string(),
            passing: crate::mir::MirReceiverKind::Borrow,
            ty: Type::named("int32"),
        },
        MirParam {
            name: "target".to_string(),
            passing: crate::mir::MirReceiverKind::BorrowMut,
            ty: Type::named("int32"),
        },
    ];
    let evaluated_args = vec![
        EvaluatedMirArg {
            name: Some("source".to_string()),
            value: Value::Int(IntegerValue::from_signed(1)),
            writeback_place: None,
        },
        EvaluatedMirArg {
            name: Some("target".to_string()),
            value: Value::Int(IntegerValue::from_signed(1)),
            writeback_place: Some("target".to_string()),
        },
    ];
    runtime
        .apply_borrowed_param_writebacks(
            &params,
            &evaluated_args,
            &[
                (0, Value::Int(IntegerValue::from_signed(4))),
                (1, Value::Int(IntegerValue::from_signed(7))),
                (9, Value::Int(IntegerValue::from_signed(11))),
            ],
            &mut env,
        )
        .expect("borrow-mut writebacks should update explicit writeback places");
    assert_eq!(
        env.read_place("target"),
        Ok(Value::Int(IntegerValue::from_signed(7)))
    );

    let missing_writeback = runtime
        .apply_borrowed_param_writebacks(
            &params,
            &[
                EvaluatedMirArg {
                    name: Some("source".to_string()),
                    value: Value::Int(IntegerValue::from_signed(1)),
                    writeback_place: None,
                },
                EvaluatedMirArg {
                    name: Some("target".to_string()),
                    value: Value::Int(IntegerValue::from_signed(1)),
                    writeback_place: None,
                },
            ],
            &[(1, Value::Int(IntegerValue::from_signed(9)))],
            &mut env,
        )
        .expect_err("borrow-mut writebacks require an explicit writeback place");
    assert!(missing_writeback
        .message
        .contains("requires a writeback place"));

    let by_value = MirFunction {
        name: "work".to_string(),
        module_name: "<test>".to_string(),
        span: Span::new(1, 1),
        receiver: None,
        params: vec![MirParam {
            name: "value".to_string(),
            passing: crate::mir::MirReceiverKind::Value,
            ty: Type::named("int32"),
        }],
        local_types: Vec::new(),
        return_type: Type::named("int32"),
        entry: "entry".to_string(),
        blocks: Vec::new(),
    };
    runtime
        .require_task_startable_function(&by_value)
        .expect("by-value MIR functions should be task-startable");
    let task_start_error = runtime
        .require_task_startable_function(&MirFunction {
            params: vec![MirParam {
                name: "value".to_string(),
                passing: crate::mir::MirReceiverKind::Borrow,
                ty: Type::named("int32"),
            }],
            ..by_value
        })
        .expect_err("borrowed params should not be task-startable in MIR");
    assert!(task_start_error
        .message
        .contains("does not yet support borrowed parameter `value`"));
}

#[test]
fn mir_runtime_builtin_call_surface_covers_named_and_error_paths() {
    let mut runtime = test_runtime();
    let mut env = Env::default();
    env.define_typed("delay", Type::named("Duration"), Value::Duration(0));
    env.define_typed(
        "negative_delay",
        Type::named("Duration"),
        Value::Duration(-1),
    );
    env.define_typed(
        "neg",
        Type::named("int32"),
        Value::Int(IntegerValue::from_signed(-4)),
    );
    env.define_typed(
        "uint",
        Type::named("uint64"),
        Value::Int(IntegerValue::from_literal(7)),
    );
    env.define_typed("ratio", Type::named("float64"), Value::Float(-2.5));
    env.define_typed(
        "text",
        Type::named("String"),
        Value::String("12".to_string()),
    );
    env.define_typed(
        "word",
        Type::named("String"),
        Value::String("Aurora".to_string()),
    );
    env.define_typed(
        "float_text",
        Type::named("String"),
        Value::String("1.5e2".to_string()),
    );
    env.define_typed(
        "infinite_text",
        Type::named("String"),
        Value::String("inf".to_string()),
    );

    assert_eq!(
        runtime
            .evaluate_call(
                &crate::mir::CallTarget::Name("range".to_string()),
                &[MirArg {
                    name: Some("stop".to_string()),
                    value: Operand::Int(3),
                    writeback_place: None,
                }],
                &mut env,
            )
            .expect("named range call should succeed"),
        Value::Range(RangeValue { start: 0, end: 3 })
    );
    assert!(matches!(
        runtime
            .evaluate_call(
                &crate::mir::CallTarget::Name("Vec".to_string()),
                &[],
                &mut env
            )
            .expect("Vec() should succeed"),
        Value::Vec(_)
    ));
    assert!(matches!(
        runtime
            .evaluate_call(
                &crate::mir::CallTarget::Name("Set".to_string()),
                &[],
                &mut env
            )
            .expect("Set() should succeed"),
        Value::Set(_)
    ));
    assert!(matches!(
        runtime
            .evaluate_call(
                &crate::mir::CallTarget::Name("Map".to_string()),
                &[],
                &mut env
            )
            .expect("Map() should succeed"),
        Value::Map(_)
    ));
    assert!(matches!(
        runtime
            .evaluate_call(
                &crate::mir::CallTarget::Name("TaskGroup".to_string()),
                &[],
                &mut env
            )
            .expect("TaskGroup() should succeed"),
        Value::TaskGroup(_)
    ));
    assert!(matches!(
        runtime
            .evaluate_call(
                &crate::mir::CallTarget::Name("Queue".to_string()),
                &[MirArg {
                    name: Some("capacity".to_string()),
                    value: Operand::Int(2),
                    writeback_place: None,
                }],
                &mut env,
            )
            .expect("Queue(capacity=...) should create a bounded queue"),
        Value::Channel(_)
    ));
    assert_eq!(
        runtime
            .evaluate_call(
                &crate::mir::CallTarget::Name("cancelled".to_string()),
                &[],
                &mut env,
            )
            .expect("cancelled() should succeed"),
        Value::Bool(false)
    );
    assert_eq!(
        runtime
            .evaluate_call(
                &crate::mir::CallTarget::Name("sleep".to_string()),
                &[MirArg {
                    name: Some("duration".to_string()),
                    value: Operand::Place("delay".to_string()),
                    writeback_place: None,
                }],
                &mut env,
            )
            .expect("sleep() should accept zero duration"),
        Value::Unit
    );
    let sleep_error = runtime
        .evaluate_call(
            &crate::mir::CallTarget::Name("sleep".to_string()),
            &[MirArg {
                name: Some("duration".to_string()),
                value: Operand::Place("negative_delay".to_string()),
                writeback_place: None,
            }],
            &mut env,
        )
        .expect_err("negative sleep durations should fail");
    assert!(sleep_error
        .message
        .contains("does not fit in the MIR runtime timer range"));

    assert_eq!(
        runtime
            .evaluate_call(
                &crate::mir::CallTarget::Name("abs".to_string()),
                &[mir_arg(None, Operand::Place("neg".to_string()))],
                &mut env,
            )
            .expect("abs(int) should succeed"),
        Value::Int(IntegerValue::from_signed(4))
    );
    assert_eq!(
        runtime
            .evaluate_call(
                &crate::mir::CallTarget::Name("abs".to_string()),
                &[mir_arg(None, Operand::Place("uint".to_string()))],
                &mut env,
            )
            .expect("abs(uint) should succeed"),
        Value::Int(IntegerValue::from_literal(7))
    );
    assert_eq!(
        runtime
            .evaluate_call(
                &crate::mir::CallTarget::Name("abs".to_string()),
                &[mir_arg(None, Operand::Place("ratio".to_string()))],
                &mut env,
            )
            .expect("abs(float) should succeed"),
        Value::Float(2.5)
    );
    assert_eq!(
        runtime
            .evaluate_call(
                &crate::mir::CallTarget::Name("min".to_string()),
                &[
                    mir_arg(None, Operand::Int(8)),
                    mir_arg(None, Operand::Int(3))
                ],
                &mut env,
            )
            .expect("min(int, int) should succeed"),
        Value::Int(IntegerValue::from_signed(3))
    );
    assert_eq!(
        runtime
            .evaluate_call(
                &crate::mir::CallTarget::Name("min".to_string()),
                &[
                    mir_arg(None, Operand::Int(3)),
                    mir_arg(None, Operand::Int(8))
                ],
                &mut env,
            )
            .expect("min(int, int) should keep the left value when smaller"),
        Value::Int(IntegerValue::from_signed(3))
    );
    assert_eq!(
        runtime
            .evaluate_call(
                &crate::mir::CallTarget::Name("max".to_string()),
                &[
                    mir_arg(None, Operand::Float(1.5)),
                    mir_arg(None, Operand::Float(2.5)),
                ],
                &mut env,
            )
            .expect("max(float, float) should succeed"),
        Value::Float(2.5)
    );
    assert_eq!(
        runtime
            .evaluate_call(
                &crate::mir::CallTarget::Name("max".to_string()),
                &[
                    mir_arg(None, Operand::Float(3.5)),
                    mir_arg(None, Operand::Float(2.5)),
                ],
                &mut env,
            )
            .expect("max(float, float) should keep the left value when larger"),
        Value::Float(3.5)
    );
    let min_error = runtime
        .evaluate_call(
            &crate::mir::CallTarget::Name("min".to_string()),
            &[
                mir_arg(None, Operand::Int(1)),
                mir_arg(None, Operand::Bool(true)),
            ],
            &mut env,
        )
        .expect_err("min() should reject mismatched types");
    assert!(min_error
        .message
        .contains("expects matching numeric arguments"));
    let sqrt_error = runtime
        .evaluate_call(
            &crate::mir::CallTarget::Name("sqrt".to_string()),
            &[mir_arg(None, Operand::Int(9))],
            &mut env,
        )
        .expect_err("sqrt() should reject integer operands");
    assert!(sqrt_error
        .message
        .contains("expects `float32` or `float64`"));

    assert_eq!(
        runtime
            .evaluate_call(
                &crate::mir::CallTarget::Name("parse_int32".to_string()),
                &[MirArg {
                    name: Some("text".to_string()),
                    value: Operand::Place("text".to_string()),
                    writeback_place: None,
                }],
                &mut env,
            )
            .expect("parse_int32() should succeed"),
        result_ok(Value::Int(IntegerValue::from_signed(12)))
    );
    assert_eq!(
        runtime
            .evaluate_call(
                &crate::mir::CallTarget::Name("parse_int64".to_string()),
                &[MirArg {
                    name: Some("text".to_string()),
                    value: Operand::Place("text".to_string()),
                    writeback_place: None,
                }],
                &mut env,
            )
            .expect("parse_int64() should succeed"),
        result_ok(Value::Int(IntegerValue::from_signed(12)))
    );
    assert_eq!(
        runtime
            .evaluate_call(
                &crate::mir::CallTarget::Name("parse_float64".to_string()),
                &[MirArg {
                    name: Some("text".to_string()),
                    value: Operand::Place("float_text".to_string()),
                    writeback_place: None,
                }],
                &mut env,
            )
            .expect("parse_float64() should parse finite floats"),
        result_ok(Value::Float(150.0))
    );
    assert_eq!(
        runtime
            .evaluate_call(
                &crate::mir::CallTarget::Name("parse_float64".to_string()),
                &[MirArg {
                    name: Some("text".to_string()),
                    value: Operand::Place("word".to_string()),
                    writeback_place: None,
                }],
                &mut env,
            )
            .expect("parse_float64() should return Result.Err for bad strings"),
        result_err(Value::String("invalid float literal".to_string()))
    );
    assert_eq!(
        runtime
            .evaluate_call(
                &crate::mir::CallTarget::Name("parse_float64".to_string()),
                &[MirArg {
                    name: Some("text".to_string()),
                    value: Operand::Place("infinite_text".to_string()),
                    writeback_place: None,
                }],
                &mut env,
            )
            .expect("parse_float64() should reject non-finite floats as Result.Err"),
        result_err(Value::String("float must be finite".to_string()))
    );
    let queue_error = runtime
        .evaluate_call(
            &crate::mir::CallTarget::Name("Queue".to_string()),
            &[
                mir_arg(None, Operand::Int(1)),
                mir_arg(None, Operand::Int(2)),
            ],
            &mut env,
        )
        .expect_err("Queue() should reject extra arguments");
    assert!(queue_error
        .message
        .contains("expects at most one optional `capacity` argument"));
}

#[test]
fn mir_runtime_process_child_methods_cover_timeout_cancel_and_error_edges() {
    let mut runtime = test_runtime();
    let env = Env::default();

    let sleeper = ProcessChildValue::spawn(
        vec![
            "/bin/sh".to_string(),
            "-c".to_string(),
            "sleep 5".to_string(),
        ],
        None,
        Vec::new(),
        ProcessStdioConfig::Null,
        ProcessStdioConfig::Null,
        ProcessStdioConfig::Null,
        false,
    )
    .expect("sleeper should spawn");
    enum_payloads(
        runtime
            .evaluate_process_child_method(
                sleeper.clone(),
                "wait",
                &[mir_arg(Some("timeout"), Operand::Duration(0))],
                &env,
            )
            .expect("wait should surface timeout"),
        "Wait",
        "TimedOut",
    );
    enum_payloads(
        result_ok_payload(
            runtime
                .evaluate_process_child_method(
                    sleeper.clone(),
                    "wait_or_none",
                    &[mir_arg(Some("timeout"), Operand::Duration(0))],
                    &env,
                )
                .expect("wait_or_none should surface timeout as None"),
        ),
        "Option",
        "None",
    );
    assert_eq!(
        result_ok_payload(
            runtime
                .evaluate_process_child_method(sleeper.clone(), "kill", &[], &env)
                .expect("kill should succeed")
        ),
        Value::Unit
    );
    enum_payloads(
        runtime
            .evaluate_process_child_method(
                sleeper.clone(),
                "wait",
                &[mir_arg(Some("timeout"), Operand::Duration(1_000))],
                &env,
            )
            .expect("killed child should become waitable"),
        "Wait",
        "Exited",
    );
    let unknown_method = runtime
        .evaluate_process_child_method(sleeper, "missing", &[], &env)
        .expect_err("unknown process child methods should fail");
    assert!(unknown_method
        .message
        .contains("unsupported MIR process child method"));

    let failing = ProcessChildValue::spawn(
        vec![
            "/bin/sh".to_string(),
            "-c".to_string(),
            "exit 7".to_string(),
        ],
        None,
        Vec::new(),
        ProcessStdioConfig::Null,
        ProcessStdioConfig::Null,
        ProcessStdioConfig::Null,
        false,
    )
    .expect("failing child should spawn");
    enum_payloads(
        runtime
            .evaluate_process_child_method(
                failing,
                "wait_ok",
                &[mir_arg(Some("timeout"), Operand::Duration(1_000))],
                &env,
            )
            .expect("wait_ok should return a Result"),
        "Result",
        "Err",
    );

    let terminable = ProcessChildValue::spawn(
        vec![
            "/bin/sh".to_string(),
            "-c".to_string(),
            "sleep 5".to_string(),
        ],
        None,
        Vec::new(),
        ProcessStdioConfig::Null,
        ProcessStdioConfig::Null,
        ProcessStdioConfig::Null,
        false,
    )
    .expect("terminable child should spawn");
    assert_eq!(
        result_ok_payload(
            runtime
                .evaluate_process_child_method(terminable.clone(), "terminate", &[], &env)
                .expect("terminate should succeed")
        ),
        Value::Unit
    );
    let _ = runtime.evaluate_process_child_method(
        terminable.clone(),
        "wait",
        &[mir_arg(Some("timeout"), Operand::Duration(1_000))],
        &env,
    );
    let _ = runtime.evaluate_process_child_method(terminable, "close", &[], &env);

    let cancellation = CancellationContext::default();
    let group = TaskGroupValue::new(&cancellation);
    let cancelled_context = group.child_cancellation();
    group.cancel();
    let mut cancelled_runtime = MirRuntime::new(
        MirModule {
            functions: Vec::new(),
            classes: Vec::new(),
            trait_impls: Vec::new(),
            top_level: None,
        },
        Arc::new(Mutex::new(String::new())),
        cancelled_context,
    );
    let cancelled_child = ProcessChildValue::spawn(
        vec![
            "/bin/sh".to_string(),
            "-c".to_string(),
            "sleep 5".to_string(),
        ],
        None,
        Vec::new(),
        ProcessStdioConfig::Null,
        ProcessStdioConfig::Null,
        ProcessStdioConfig::Null,
        false,
    )
    .expect("cancelled-runtime child should spawn");
    enum_payloads(
        cancelled_runtime
            .evaluate_process_child_method(
                cancelled_child.clone(),
                "wait",
                &[mir_arg(Some("timeout"), Operand::Duration(1_000))],
                &env,
            )
            .expect("wait should observe cancellation"),
        "Wait",
        "Cancelled",
    );
    let _ = cancelled_runtime.evaluate_process_child_method(cancelled_child, "close", &[], &env);
}

#[test]
fn mir_runtime_process_resource_members_cover_completed_errors_and_pipe_edges() {
    let mut runtime = test_runtime();
    let mut env = Env::default();
    env.define_typed(
        "bytes",
        Type::Named("Vec".to_string(), vec![Type::named("uint8")]),
        bytes_vec_value(b"payload".to_vec()),
    );
    env.define_typed(
        "negative",
        Type::named("int32"),
        Value::Int(IntegerValue::from_signed(-1)),
    );

    let failed_completed = ProcessCompletedValue::new(
        Value::EnumVariant(EnumVariantValue {
            enum_name: "ExitStatus".to_string(),
            variant_name: "Exited".to_string(),
            payloads: vec![Value::Int(IntegerValue::from_signed(7))],
        }),
        vec![0xff],
        vec![0xfe],
    );
    assert_eq!(
        runtime
            .evaluate_process_completed_method(failed_completed.clone(), "success", &[], &env)
            .expect("completed success should evaluate"),
        Value::Bool(false)
    );
    assert!(runtime
        .evaluate_process_completed_method(failed_completed.clone(), "stdout", &[], &env)
        .expect_err("invalid stdout utf-8 should be rejected")
        .message
        .contains("invalid utf-8"));
    assert!(runtime
        .evaluate_process_completed_method(failed_completed.clone(), "stderr", &[], &env)
        .expect_err("invalid stderr utf-8 should be rejected")
        .message
        .contains("invalid utf-8"));
    assert_result_err(
        runtime
            .evaluate_process_completed_method(failed_completed, "check", &[], &env)
            .expect("failed process check should return Result.Err"),
    );

    let eof_child = ProcessChildValue::spawn(
        vec!["/bin/sh".to_string(), "-c".to_string(), String::new()],
        None,
        Vec::new(),
        ProcessStdioConfig::Null,
        ProcessStdioConfig::Pipe,
        ProcessStdioConfig::Null,
        false,
    )
    .expect("eof process should spawn");
    let eof_stdout = eof_child.stdout().expect("eof stdout should be piped");
    enum_payloads(
        result_ok_payload(
            runtime
                .evaluate_process_pipe_method(
                    eof_stdout.clone(),
                    "read_line",
                    &[mir_arg(Some("timeout"), Operand::Duration(1_000))],
                    &env,
                )
                .expect("eof read_line should succeed"),
        ),
        "Option",
        "None",
    );
    enum_payloads(
        result_ok_payload(
            runtime
                .evaluate_process_pipe_method(
                    eof_stdout,
                    "read_bytes",
                    &[
                        mir_arg(Some("max_bytes"), Operand::Int(8)),
                        mir_arg(Some("timeout"), Operand::Duration(1_000)),
                    ],
                    &env,
                )
                .expect("eof read_bytes should succeed"),
        ),
        "Option",
        "None",
    );
    let _ = runtime.evaluate_process_child_method(eof_child, "wait", &[], &env);

    let reader_child = ProcessChildValue::spawn(
        vec![
            "/bin/sh".to_string(),
            "-c".to_string(),
            "sleep 0.05".to_string(),
        ],
        None,
        Vec::new(),
        ProcessStdioConfig::Null,
        ProcessStdioConfig::Pipe,
        ProcessStdioConfig::Null,
        false,
    )
    .expect("reader process should spawn");
    let closed_reader = reader_child
        .stdout()
        .expect("reader stdout should be piped");
    assert_eq!(
        runtime
            .evaluate_process_pipe_method(closed_reader.clone(), "close", &[], &env)
            .expect("reader pipe close should succeed"),
        Value::Unit
    );
    assert_result_err(
        runtime
            .evaluate_process_pipe_method(closed_reader.clone(), "read_all", &[], &env)
            .expect("closed read_all should return Result.Err"),
    );
    assert_result_err(
        runtime
            .evaluate_process_pipe_method(
                closed_reader.clone(),
                "read_line",
                &[mir_arg(Some("timeout"), Operand::Duration(1_000))],
                &env,
            )
            .expect("closed read_line should return Result.Err"),
    );
    assert_result_err(
        runtime
            .evaluate_process_pipe_method(
                closed_reader.clone(),
                "read_bytes",
                &[
                    mir_arg(Some("max_bytes"), Operand::Int(8)),
                    mir_arg(Some("timeout"), Operand::Duration(1_000)),
                ],
                &env,
            )
            .expect("closed read_bytes should return Result.Err"),
    );
    assert!(runtime
        .evaluate_process_pipe_method(
            closed_reader,
            "read_bytes",
            &[
                mir_arg(Some("max_bytes"), Operand::Place("negative".to_string())),
                mir_arg(Some("timeout"), Operand::Duration(1_000)),
            ],
            &env,
        )
        .expect_err("negative process pipe read sizes should fail")
        .message
        .contains("non-negative"));
    let _ = runtime.evaluate_process_child_method(reader_child, "wait", &[], &env);

    let writer_child = ProcessChildValue::spawn(
        vec!["/bin/cat".to_string()],
        None,
        Vec::new(),
        ProcessStdioConfig::Pipe,
        ProcessStdioConfig::Null,
        ProcessStdioConfig::Null,
        false,
    )
    .expect("writer process should spawn");
    let closed_writer = writer_child.stdin().expect("writer stdin should be piped");
    assert_eq!(
        runtime
            .evaluate_process_pipe_method(closed_writer.clone(), "close", &[], &env)
            .expect("writer pipe close should succeed"),
        Value::Unit
    );
    assert_result_err(
        runtime
            .evaluate_process_pipe_method(
                closed_writer.clone(),
                "write_all",
                &[
                    mir_arg(Some("text"), Operand::String("closed".to_string())),
                    mir_arg(Some("timeout"), Operand::Duration(1_000)),
                ],
                &env,
            )
            .expect("closed write_all should return Result.Err"),
    );
    assert_result_err(
        runtime
            .evaluate_process_pipe_method(
                closed_writer.clone(),
                "write_bytes",
                &[
                    mir_arg(Some("bytes"), Operand::Place("bytes".to_string())),
                    mir_arg(Some("timeout"), Operand::Duration(1_000)),
                ],
                &env,
            )
            .expect("closed write_bytes should return Result.Err"),
    );
    assert_result_err(
        runtime
            .evaluate_process_pipe_method(closed_writer.clone(), "flush", &[], &env)
            .expect("closed flush should return Result.Err"),
    );
    assert!(runtime
        .evaluate_process_pipe_method(closed_writer, "missing", &[], &env)
        .expect_err("unknown process pipe methods should fail")
        .message
        .contains("unsupported MIR process pipe method"));
    let _ = runtime.evaluate_process_child_method(writer_child, "wait", &[], &env);
}

#[test]
fn mir_runtime_process_supervisor_methods_cover_start_wait_and_cancel_edges() {
    let mut runtime = test_runtime();
    let mut env = Env::default();
    env.define_typed(
        "exit_command",
        Type::Named("Vec".to_string(), vec![Type::named("String")]),
        string_vec_value(&["/bin/sh", "-c", "exit 0"]),
    );
    env.define_typed(
        "sleep_command",
        Type::Named("Vec".to_string(), vec![Type::named("String")]),
        string_vec_value(&["/bin/sh", "-c", "sleep 5"]),
    );
    let stdio_variant = |variant_name: &str| {
        Value::EnumVariant(EnumVariantValue {
            enum_name: "process.Stdio".to_string(),
            variant_name: variant_name.to_string(),
            payloads: Vec::new(),
        })
    };
    env.define_typed(
        "supervisor_cwd",
        Type::named("Option[String]"),
        option_some(Value::String(
            std::env::temp_dir().to_string_lossy().into_owned(),
        )),
    );
    env.define_typed(
        "supervisor_env",
        Type::Named(
            "Map".to_string(),
            vec![Type::named("String"), Type::named("String")],
        ),
        string_map_value(&[("AURORA_SUPERVISOR_TEST", "1")]),
    );
    env.define_typed(
        "stdio_null",
        Type::named("process.Stdio"),
        stdio_variant("Null"),
    );
    env.define_typed(
        "stdio_pipe",
        Type::named("process.Stdio"),
        stdio_variant("Pipe"),
    );
    env.define_typed(
        "stdio_inherit",
        Type::named("process.Stdio"),
        stdio_variant("Inherit"),
    );
    env.define_typed(
        "restart_never",
        Type::named("process.RestartPolicy"),
        Value::EnumVariant(EnumVariantValue {
            enum_name: "process.RestartPolicy".to_string(),
            variant_name: "Never".to_string(),
            payloads: Vec::new(),
        }),
    );

    let supervisor = ProcessSupervisorValue::new();
    let missing_command = runtime
        .evaluate_process_supervisor_method(
            supervisor.clone(),
            "start",
            &[mir_arg(
                Some("name"),
                Operand::String("missing-command".to_string()),
            )],
            &env,
        )
        .expect_err("supervisor start should require a command");
    assert!(missing_command
        .message
        .contains("missing MIR argument `command`"));
    assert_eq!(
        result_ok_payload(
            runtime
                .evaluate_process_supervisor_method(
                    supervisor.clone(),
                    "start",
                    &[
                        mir_arg(Some("name"), Operand::String("oneshot".to_string())),
                        mir_arg(Some("command"), Operand::Place("exit_command".to_string())),
                    ],
                    &env,
                )
                .expect("supervisor start should succeed with defaulted optional args")
        ),
        Value::Unit
    );
    enum_payloads(
        runtime
            .evaluate_process_supervisor_method(
                supervisor.clone(),
                "wait",
                &[mir_arg(Some("timeout"), Operand::Duration(5_000))],
                &env,
            )
            .expect("supervisor wait should surface an event"),
        "SupervisorWait",
        "Event",
    );
    enum_payloads(
        result_ok_payload(
            runtime
                .evaluate_process_supervisor_method(
                    supervisor.clone(),
                    "wait_or_none",
                    &[mir_arg(Some("timeout"), Operand::Duration(0))],
                    &env,
                )
                .expect("empty supervisor wait_or_none should return Result.Ok"),
        ),
        "Option",
        "None",
    );
    assert_eq!(
        result_ok_payload(
            runtime
                .evaluate_process_supervisor_method(
                    supervisor.clone(),
                    "start",
                    &[
                        mir_arg(Some("name"), Operand::String("configured".to_string())),
                        mir_arg(Some("command"), Operand::Place("exit_command".to_string())),
                        mir_arg(Some("cwd"), Operand::Place("supervisor_cwd".to_string())),
                        mir_arg(Some("env"), Operand::Place("supervisor_env".to_string())),
                        mir_arg(Some("stdin"), Operand::Place("stdio_null".to_string())),
                        mir_arg(Some("stdout"), Operand::Place("stdio_pipe".to_string())),
                        mir_arg(Some("stderr"), Operand::Place("stdio_inherit".to_string())),
                        mir_arg(Some("restart"), Operand::Place("restart_never".to_string())),
                        mir_arg(Some("backoff"), Operand::Duration(1)),
                        mir_arg(Some("max_restarts"), Operand::Int(1)),
                        mir_arg(Some("group"), Operand::Bool(false)),
                    ],
                    &env,
                )
                .expect("supervisor start should accept all optional args")
        ),
        Value::Unit
    );
    enum_payloads(
        result_ok_payload(
            runtime
                .evaluate_process_supervisor_method(
                    supervisor.clone(),
                    "wait_or_none",
                    &[mir_arg(Some("timeout"), Operand::Duration(5_000))],
                    &env,
                )
                .expect("supervisor wait_or_none should surface ready events"),
        ),
        "Option",
        "Some",
    );

    assert_eq!(
        result_ok_payload(
            runtime
                .evaluate_process_supervisor_method(
                    supervisor.clone(),
                    "start",
                    &[
                        mir_arg(Some("name"), Operand::String("dupe".to_string())),
                        mir_arg(Some("command"), Operand::Place("sleep_command".to_string())),
                    ],
                    &env,
                )
                .expect("supervisor start should accept a long-running child")
        ),
        Value::Unit
    );
    enum_payloads(
        runtime
            .evaluate_process_supervisor_method(
                supervisor.clone(),
                "start",
                &[
                    mir_arg(Some("name"), Operand::String("dupe".to_string())),
                    mir_arg(Some("command"), Operand::Place("sleep_command".to_string())),
                ],
                &env,
            )
            .expect("duplicate supervisor starts should return Result.Err"),
        "Result",
        "Err",
    );
    assert_eq!(
        result_ok_payload(
            runtime
                .evaluate_process_supervisor_method(supervisor.clone(), "stop", &[], &env)
                .expect("supervisor stop should clean up running children")
        ),
        Value::Unit
    );
    assert_eq!(
        runtime
            .evaluate_process_supervisor_method(supervisor, "close", &[], &env)
            .expect("supervisor close should succeed"),
        Value::Unit
    );

    let cancellation = CancellationContext::default();
    let group = TaskGroupValue::new(&cancellation);
    let cancelled_context = group.child_cancellation();
    group.cancel();
    let mut cancelled_runtime = MirRuntime::new(
        MirModule {
            functions: Vec::new(),
            classes: Vec::new(),
            trait_impls: Vec::new(),
            top_level: None,
        },
        Arc::new(Mutex::new(String::new())),
        cancelled_context,
    );
    let cancelled_supervisor = ProcessSupervisorValue::new();
    assert_eq!(
        result_ok_payload(
            cancelled_runtime
                .evaluate_process_supervisor_method(
                    cancelled_supervisor.clone(),
                    "start",
                    &[
                        mir_arg(Some("name"), Operand::String("cancelled".to_string())),
                        mir_arg(Some("command"), Operand::Place("sleep_command".to_string())),
                    ],
                    &env,
                )
                .expect("cancelled-runtime supervisor start should still register children")
        ),
        Value::Unit
    );
    enum_payloads(
        cancelled_runtime
            .evaluate_process_supervisor_method(
                cancelled_supervisor.clone(),
                "wait",
                &[mir_arg(Some("timeout"), Operand::Duration(5_000))],
                &env,
            )
            .expect("supervisor wait should observe cancellation"),
        "SupervisorWait",
        "Cancelled",
    );
    enum_payloads(
        cancelled_runtime
            .evaluate_process_supervisor_method(
                cancelled_supervisor.clone(),
                "wait_or_none",
                &[mir_arg(Some("timeout"), Operand::Duration(5_000))],
                &env,
            )
            .expect("cancelled supervisor wait_or_none should return Result.Err"),
        "Result",
        "Err",
    );
    let _ = cancelled_runtime.evaluate_process_supervisor_method(
        cancelled_supervisor.clone(),
        "stop",
        &[],
        &env,
    );
    let _ = cancelled_runtime.evaluate_process_supervisor_method(
        cancelled_supervisor,
        "close",
        &[],
        &env,
    );
}

#[test]
fn mir_runtime_builtin_io_calls_cover_process_filesystem_and_network_paths() {
    let mut runtime = test_runtime();
    let mut env = Env::default();

    let stdio_null = call_name(&mut runtime, "process::null", &[], &mut env)
        .expect("process.null() should succeed");
    let stdio_pipe = call_name(&mut runtime, "process::pipe", &[], &mut env)
        .expect("process.pipe() should succeed");
    let stdio_inherit = call_name(&mut runtime, "process::inherit", &[], &mut env)
        .expect("process.inherit() should succeed");
    assert!(matches!(
        call_name(&mut runtime, "process::supervisor", &[], &mut env)
            .expect("process.supervisor() should succeed"),
        Value::ProcessSupervisor(_)
    ));

    env.define_typed(
        "empty_cmd",
        Type::Named("Vec".to_string(), vec![Type::named("String")]),
        string_vec_value(&[]),
    );
    env.define_typed(
        "child_cmd",
        Type::Named("Vec".to_string(), vec![Type::named("String")]),
        string_vec_value(&["/bin/sh", "-c", "printf child"]),
    );
    env.define_typed("cwd", Type::named("Option[String]"), option_none());
    env.define_typed(
        "env_map",
        Type::Named(
            "Map".to_string(),
            vec![Type::named("String"), Type::named("String")],
        ),
        string_map_value(&[]),
    );
    env.define_typed(
        "stdio_null",
        Type::named("process.Stdio"),
        stdio_null.clone(),
    );
    env.define_typed(
        "stdio_pipe",
        Type::named("process.Stdio"),
        stdio_pipe.clone(),
    );
    env.define_typed("stdio_inherit", Type::named("process.Stdio"), stdio_inherit);

    let start_no_command = call_name(
        &mut runtime,
        "process::start",
        &[
            mir_arg(Some("command"), Operand::Place("empty_cmd".to_string())),
            mir_arg(Some("cwd"), Operand::Place("cwd".to_string())),
            mir_arg(Some("env"), Operand::Place("env_map".to_string())),
            mir_arg(Some("stdin"), Operand::Place("stdio_null".to_string())),
            mir_arg(Some("stdout"), Operand::Place("stdio_null".to_string())),
            mir_arg(Some("stderr"), Operand::Place("stdio_null".to_string())),
            mir_arg(Some("group"), Operand::Bool(false)),
        ],
        &mut env,
    )
    .expect("process.start should return a Result for empty commands");
    let start_error = enum_payloads(start_no_command, "Result", "Err").remove(0);
    enum_payloads(start_error, "Error", "NoCommand");

    let child = result_ok_payload(
        call_name(
            &mut runtime,
            "process::start",
            &[
                mir_arg(Some("command"), Operand::Place("child_cmd".to_string())),
                mir_arg(Some("cwd"), Operand::Place("cwd".to_string())),
                mir_arg(Some("env"), Operand::Place("env_map".to_string())),
                mir_arg(Some("stdin"), Operand::Place("stdio_null".to_string())),
                mir_arg(Some("stdout"), Operand::Place("stdio_pipe".to_string())),
                mir_arg(Some("stderr"), Operand::Place("stdio_null".to_string())),
                mir_arg(Some("group"), Operand::Bool(false)),
            ],
            &mut env,
        )
        .expect("process.start should spawn a child"),
    );
    match child {
        Value::ProcessChild(child) => child.close(),
        other => panic!("expected process child, found {other:?}"),
    }

    let timestamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("system time should be after epoch")
        .as_nanos();
    let temp_root = std::env::temp_dir().join(format!(
        "aurora-mir-builtin-{}-{timestamp}",
        std::process::id()
    ));
    std::fs::create_dir_all(&temp_root).expect("temp root should be created");
    let read_path = temp_root.join("read.txt");
    let write_path = temp_root.join("write.txt");
    let bytes_path = temp_root.join("bytes.bin");
    let dir_path = temp_root.join("items");
    std::fs::write(&read_path, "hello").expect("read fixture should be written");

    env.define_typed(
        "read_path",
        Type::named("String"),
        Value::String(read_path.to_string_lossy().into_owned()),
    );
    env.define_typed(
        "write_path",
        Type::named("String"),
        Value::String(write_path.to_string_lossy().into_owned()),
    );
    env.define_typed(
        "bytes_path",
        Type::named("String"),
        Value::String(bytes_path.to_string_lossy().into_owned()),
    );
    env.define_typed(
        "dir_path",
        Type::named("String"),
        Value::String(dir_path.to_string_lossy().into_owned()),
    );
    env.define_typed(
        "text",
        Type::named("String"),
        Value::String("hello".to_string()),
    );
    env.define_typed(
        "suffix",
        Type::named("String"),
        Value::String("-again".to_string()),
    );
    env.define_typed(
        "bytes",
        Type::Named("Vec".to_string(), vec![Type::named("uint8")]),
        bytes_vec_value(b"ab".to_vec()),
    );

    assert_eq!(
        call_name(
            &mut runtime,
            "fs::exists",
            &[mir_arg(
                Some("path"),
                Operand::Place("read_path".to_string())
            )],
            &mut env,
        )
        .expect("fs.exists should succeed"),
        Value::Bool(true)
    );
    assert_eq!(
        result_ok_payload(
            call_name(
                &mut runtime,
                "fs::read_to_string",
                &[mir_arg(
                    Some("path"),
                    Operand::Place("read_path".to_string())
                )],
                &mut env,
            )
            .expect("fs.read_to_string should succeed")
        ),
        Value::String("hello".to_string())
    );
    assert_eq!(
        result_ok_payload(
            call_name(
                &mut runtime,
                "fs::read_bytes",
                &[mir_arg(
                    Some("path"),
                    Operand::Place("read_path".to_string())
                )],
                &mut env,
            )
            .expect("fs.read_bytes should succeed")
        ),
        bytes_vec_value(b"hello".to_vec())
    );
    assert_eq!(
        result_ok_payload(
            call_name(
                &mut runtime,
                "fs::write_string",
                &[
                    mir_arg(Some("path"), Operand::Place("write_path".to_string())),
                    mir_arg(Some("text"), Operand::Place("text".to_string())),
                ],
                &mut env,
            )
            .expect("fs.write_string should succeed")
        ),
        Value::Unit
    );
    let write_text_error = call_name(
        &mut runtime,
        "fs::write_string",
        &[
            mir_arg(Some("path"), Operand::Place("write_path".to_string())),
            mir_arg(Some("text"), Operand::Int(7)),
        ],
        &mut env,
    )
    .expect_err("fs.write_string should reject non-string text");
    assert!(write_text_error
        .message
        .contains("expects `String` for `text`"));
    assert_eq!(
        result_ok_payload(
            call_name(
                &mut runtime,
                "fs::append_string",
                &[
                    mir_arg(Some("path"), Operand::Place("write_path".to_string())),
                    mir_arg(Some("text"), Operand::Place("suffix".to_string())),
                ],
                &mut env,
            )
            .expect("fs.append_string should succeed")
        ),
        Value::Unit
    );
    assert_eq!(
        std::fs::read_to_string(&write_path).expect("write fixture should be readable"),
        "hello-again"
    );
    assert_eq!(
        result_ok_payload(
            call_name(
                &mut runtime,
                "fs::write_bytes",
                &[
                    mir_arg(Some("path"), Operand::Place("bytes_path".to_string())),
                    mir_arg(Some("bytes"), Operand::Place("bytes".to_string())),
                ],
                &mut env,
            )
            .expect("fs.write_bytes should succeed")
        ),
        Value::Unit
    );
    assert_eq!(
        result_ok_payload(
            call_name(
                &mut runtime,
                "fs::append_bytes",
                &[
                    mir_arg(Some("path"), Operand::Place("bytes_path".to_string())),
                    mir_arg(Some("bytes"), Operand::Place("bytes".to_string())),
                ],
                &mut env,
            )
            .expect("fs.append_bytes should succeed")
        ),
        Value::Unit
    );
    assert_eq!(
        result_ok_payload(
            call_name(
                &mut runtime,
                "fs::read_bytes",
                &[mir_arg(
                    Some("path"),
                    Operand::Place("bytes_path".to_string())
                )],
                &mut env,
            )
            .expect("fs.read_bytes should read appended bytes")
        ),
        bytes_vec_value(b"abab".to_vec())
    );
    assert_eq!(
        result_ok_payload(
            call_name(
                &mut runtime,
                "fs::create_dir",
                &[mir_arg(
                    Some("path"),
                    Operand::Place("dir_path".to_string())
                )],
                &mut env,
            )
            .expect("fs.create_dir should succeed")
        ),
        Value::Unit
    );
    assert!(matches!(
        result_ok_payload(
            call_name(
                &mut runtime,
                "fs::read_dir",
                &[mir_arg(
                    Some("path"),
                    Operand::Place("dir_path".to_string())
                )],
                &mut env,
            )
            .expect("fs.read_dir should succeed")
        ),
        Value::Vec(_)
    ));
    assert_eq!(
        result_ok_payload(
            call_name(
                &mut runtime,
                "fs::remove_file",
                &[mir_arg(
                    Some("path"),
                    Operand::Place("read_path".to_string())
                )],
                &mut env,
            )
            .expect("fs.remove_file should succeed")
        ),
        Value::Unit
    );
    for builtin in ["fs::open", "fs::create", "fs::append"] {
        match result_ok_payload(
            call_name(
                &mut runtime,
                builtin,
                &[mir_arg(
                    Some("path"),
                    Operand::Place("write_path".to_string()),
                )],
                &mut env,
            )
            .expect("file constructor should return a Result"),
        ) {
            Value::File(file) => file.close(),
            other => panic!("expected file from {builtin}, found {other:?}"),
        }
    }
    let open_type_error = call_name(
        &mut runtime,
        "fs::open",
        &[mir_arg(Some("path"), Operand::Bool(false))],
        &mut env,
    )
    .expect_err("fs.open should reject non-string paths");
    assert!(open_type_error.message.contains("expects `String`"));

    let listener = result_ok_payload(
        call_name(
            &mut runtime,
            "net::listen",
            &[mir_arg(
                Some("address"),
                Operand::String("127.0.0.1:0".to_string()),
            )],
            &mut env,
        )
        .expect("net.listen should return a Result"),
    );
    let tcp_listener = match listener {
        Value::TcpListener(listener) => listener,
        other => panic!("expected tcp listener, found {other:?}"),
    };
    let tcp_address = tcp_listener
        .local_addr()
        .expect("tcp listener address should be available");
    env.define_typed(
        "tcp_address",
        Type::named("String"),
        Value::String(tcp_address),
    );
    let tcp_server = {
        let listener = tcp_listener.clone();
        thread::spawn(move || {
            for _ in 0..2 {
                let stream = listener
                    .accept(
                        Some(StdDuration::from_secs(2)),
                        Some(&CancellationContext::default()),
                    )
                    .expect("tcp server should accept");
                stream.close();
            }
        })
    };
    for builtin in ["net::connect", "net::connect_timeout"] {
        let args = if builtin == "net::connect" {
            vec![mir_arg(
                Some("address"),
                Operand::Place("tcp_address".to_string()),
            )]
        } else {
            vec![
                mir_arg(Some("address"), Operand::Place("tcp_address".to_string())),
                mir_arg(Some("timeout"), Operand::Duration(1_000)),
            ]
        };
        match result_ok_payload(
            call_name(&mut runtime, builtin, &args, &mut env)
                .expect("tcp connect builtin should return a Result"),
        ) {
            Value::TcpStream(stream) => stream.close(),
            other => panic!("expected tcp stream from {builtin}, found {other:?}"),
        }
    }
    tcp_server.join().expect("tcp server should join");
    tcp_listener.close();
    let connect_type_error = call_name(
        &mut runtime,
        "net::connect",
        &[mir_arg(Some("address"), Operand::Bool(true))],
        &mut env,
    )
    .expect_err("net.connect should reject non-string addresses");
    assert!(connect_type_error.message.contains("expects `String`"));

    match result_ok_payload(
        call_name(
            &mut runtime,
            "net::udp_bind",
            &[mir_arg(
                Some("address"),
                Operand::String("127.0.0.1:0".to_string()),
            )],
            &mut env,
        )
        .expect("net.udp_bind should return a Result"),
    ) {
        Value::UdpSocket(socket) => socket.close(),
        other => panic!("expected udp socket, found {other:?}"),
    }

    #[cfg(unix)]
    {
        let socket_path = std::path::PathBuf::from(format!(
            "/tmp/aumir-{}-{}.sock",
            std::process::id(),
            timestamp % 1_000_000
        ));
        let _ = std::fs::remove_file(&socket_path);
        let socket_text = socket_path.to_string_lossy().into_owned();
        let unix_listener = result_ok_payload(
            call_name(
                &mut runtime,
                "net::unix_listen",
                &[mir_arg(Some("path"), Operand::String(socket_text.clone()))],
                &mut env,
            )
            .expect("net.unix_listen should return a Result"),
        );
        let unix_listener = match unix_listener {
            Value::UnixListener(listener) => listener,
            other => panic!("expected unix listener, found {other:?}"),
        };
        let unix_server = {
            let listener = unix_listener.clone();
            thread::spawn(move || {
                for _ in 0..2 {
                    let stream = listener
                        .accept(
                            Some(StdDuration::from_secs(2)),
                            Some(&CancellationContext::default()),
                        )
                        .expect("unix server should accept");
                    stream.close();
                }
            })
        };
        for builtin in ["net::unix_connect", "net::unix_connect_timeout"] {
            let args = if builtin == "net::unix_connect" {
                vec![mir_arg(Some("path"), Operand::String(socket_text.clone()))]
            } else {
                vec![
                    mir_arg(Some("path"), Operand::String(socket_text.clone())),
                    mir_arg(Some("timeout"), Operand::Duration(1_000)),
                ]
            };
            match result_ok_payload(
                call_name(&mut runtime, builtin, &args, &mut env)
                    .expect("unix connect builtin should return a Result"),
            ) {
                Value::UnixStream(stream) => stream.close(),
                other => panic!("expected unix stream from {builtin}, found {other:?}"),
            }
        }
        unix_server.join().expect("unix server should join");
        unix_listener.close();
        let _ = std::fs::remove_file(&socket_path);
    }

    let http_listener = result_ok_payload(
        call_name(
            &mut runtime,
            "net::http_listen",
            &[mir_arg(
                Some("address"),
                Operand::String("127.0.0.1:0".to_string()),
            )],
            &mut env,
        )
        .expect("net.http_listen should return a Result"),
    );
    let http_listener = match http_listener {
        Value::HttpListener(listener) => listener,
        other => panic!("expected http listener, found {other:?}"),
    };
    let http_address = http_listener
        .local_addr()
        .expect("http listener address should be available");
    let http_server = {
        let listener = http_listener.clone();
        thread::spawn(move || {
            for response_text in ["text-ok", "text-timeout-ok", "bytes-ok", "bytes-timeout-ok"] {
                let exchange = listener
                    .accept(
                        Some(StdDuration::from_secs(2)),
                        Some(&CancellationContext::default()),
                    )
                    .expect("http server should accept");
                exchange
                    .respond_text(200, response_text, Vec::new())
                    .expect("http response should write");
            }
        })
    };
    env.define_typed(
        "http_url",
        Type::named("String"),
        Value::String(format!("http://{http_address}/builtin")),
    );
    env.define_typed(
        "headers",
        Type::Named(
            "Map".to_string(),
            vec![Type::named("String"), Type::named("String")],
        ),
        string_map_value(&[("Content-Type", "text/plain")]),
    );
    assert!(matches!(
        result_ok_payload(
            call_name(
                &mut runtime,
                "net::http_request_text",
                &[
                    mir_arg(Some("method"), Operand::String("POST".to_string())),
                    mir_arg(Some("url"), Operand::Place("http_url".to_string())),
                    mir_arg(Some("body"), Operand::String("body".to_string())),
                    mir_arg(Some("headers"), Operand::Place("headers".to_string())),
                ],
                &mut env,
            )
            .expect("http text request should return a Result")
        ),
        Value::HttpResponse(_)
    ));
    assert!(matches!(
        result_ok_payload(
            call_name(
                &mut runtime,
                "net::http_request_text_timeout",
                &[
                    mir_arg(Some("method"), Operand::String("POST".to_string())),
                    mir_arg(Some("url"), Operand::Place("http_url".to_string())),
                    mir_arg(Some("body"), Operand::String("body".to_string())),
                    mir_arg(Some("headers"), Operand::Place("headers".to_string())),
                    mir_arg(Some("timeout"), Operand::Duration(1_000)),
                ],
                &mut env,
            )
            .expect("http text timeout request should return a Result")
        ),
        Value::HttpResponse(_)
    ));
    assert!(matches!(
        result_ok_payload(
            call_name(
                &mut runtime,
                "net::http_request_bytes",
                &[
                    mir_arg(Some("method"), Operand::String("POST".to_string())),
                    mir_arg(Some("url"), Operand::Place("http_url".to_string())),
                    mir_arg(Some("bytes"), Operand::Place("bytes".to_string())),
                    mir_arg(Some("headers"), Operand::Place("headers".to_string())),
                ],
                &mut env,
            )
            .expect("http bytes request should return a Result")
        ),
        Value::HttpResponse(_)
    ));
    assert!(matches!(
        result_ok_payload(
            call_name(
                &mut runtime,
                "net::http_request_bytes_timeout",
                &[
                    mir_arg(Some("method"), Operand::String("POST".to_string())),
                    mir_arg(Some("url"), Operand::Place("http_url".to_string())),
                    mir_arg(Some("bytes"), Operand::Place("bytes".to_string())),
                    mir_arg(Some("headers"), Operand::Place("headers".to_string())),
                    mir_arg(Some("timeout"), Operand::Duration(1_000)),
                ],
                &mut env,
            )
            .expect("http bytes request should return a Result")
        ),
        Value::HttpResponse(_)
    ));
    http_server.join().expect("http server should join");
    http_listener.close();

    for builtin in [
        "net::tls_listen",
        "net::tls_connect",
        "net::tls_connect_timeout",
    ] {
        let args = if builtin == "net::tls_listen" {
            vec![
                mir_arg(Some("address"), Operand::String("127.0.0.1:0".to_string())),
                mir_arg(
                    Some("cert_pem_path"),
                    Operand::String("missing-cert.pem".to_string()),
                ),
                mir_arg(
                    Some("key_pem_path"),
                    Operand::String("missing-key.pem".to_string()),
                ),
            ]
        } else if builtin == "net::tls_connect" {
            vec![
                mir_arg(Some("address"), Operand::String("127.0.0.1:9".to_string())),
                mir_arg(
                    Some("server_name"),
                    Operand::String("localhost".to_string()),
                ),
                mir_arg(
                    Some("ca_pem_path"),
                    Operand::String("missing-ca.pem".to_string()),
                ),
            ]
        } else {
            vec![
                mir_arg(Some("address"), Operand::String("127.0.0.1:9".to_string())),
                mir_arg(
                    Some("server_name"),
                    Operand::String("localhost".to_string()),
                ),
                mir_arg(
                    Some("ca_pem_path"),
                    Operand::String("missing-ca.pem".to_string()),
                ),
                mir_arg(Some("timeout"), Operand::Duration(1)),
            ]
        };
        enum_payloads(
            call_name(&mut runtime, builtin, &args, &mut env)
                .expect("tls builtin should return a Result"),
            "Result",
            "Err",
        );
    }

    assert!(matches!(
        result_ok_payload(
            call_name(
                &mut runtime,
                "net::websocket_listen",
                &[mir_arg(
                    Some("address"),
                    Operand::String("127.0.0.1:0".to_string()),
                )],
                &mut env,
            )
            .expect("websocket listen should return a Result")
        ),
        Value::WebSocketListener(_)
    ));
    for builtin in ["net::websocket_connect", "net::websocket_connect_timeout"] {
        let args = if builtin == "net::websocket_connect" {
            vec![mir_arg(
                Some("url"),
                Operand::String("not a websocket url".to_string()),
            )]
        } else {
            vec![
                mir_arg(
                    Some("url"),
                    Operand::String("not a websocket url".to_string()),
                ),
                mir_arg(Some("timeout"), Operand::Duration(1)),
            ]
        };
        enum_payloads(
            call_name(&mut runtime, builtin, &args, &mut env)
                .expect("websocket connect builtin should return a Result"),
            "Result",
            "Err",
        );
    }

    let unknown = runtime
        .evaluate_builtin_io_call("unknown::call", Vec::new())
        .expect_err("unknown builtin I/O calls should report diagnostics");
    assert!(unknown.message.contains("unsupported builtin I/O call"));
    let _ = std::fs::remove_dir_all(&temp_root);
}

#[test]
fn mir_runtime_builtin_io_error_results_cover_filesystem_and_network_edges() {
    fn expect_result_err(runtime: &mut MirRuntime, env: &mut Env, name: &str, args: Vec<MirArg>) {
        enum_payloads(
            call_name(runtime, name, &args, env)
                .unwrap_or_else(|error| panic!("{name} should return Result.Err: {error:?}")),
            "Result",
            "Err",
        );
    }

    let mut runtime = test_runtime();
    let mut env = Env::default();
    let timestamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("system time should be after epoch")
        .as_nanos();
    let temp_root = std::env::temp_dir().join(format!(
        "aurora-mir-runtime-io-errors-{timestamp}-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&temp_root);
    std::fs::create_dir_all(&temp_root).expect("temp root should be created");
    let occupied_file = temp_root.join("occupied.txt");
    std::fs::write(&occupied_file, "occupied").expect("occupied file should be written");
    let missing_file = temp_root.join("missing.txt");

    let dir_path = temp_root.to_string_lossy().into_owned();
    let occupied_path = occupied_file.to_string_lossy().into_owned();
    let missing_path = missing_file.to_string_lossy().into_owned();
    env.define_typed(
        "bytes",
        Type::Named("Vec".to_string(), vec![Type::named("uint8")]),
        bytes_vec_value(b"bytes".to_vec()),
    );

    let write_path_error = call_name(
        &mut runtime,
        "fs::write_string",
        &[
            mir_arg(Some("path"), Operand::Bool(false)),
            mir_arg(Some("text"), Operand::String("text".to_string())),
        ],
        &mut env,
    )
    .expect_err("fs.write_string should reject non-string paths");
    assert!(write_path_error
        .message
        .contains("expects `String` for `path`"));

    expect_result_err(
        &mut runtime,
        &mut env,
        "fs::write_string",
        vec![
            mir_arg(Some("path"), Operand::String(dir_path.clone())),
            mir_arg(Some("text"), Operand::String("text".to_string())),
        ],
    );
    expect_result_err(
        &mut runtime,
        &mut env,
        "fs::write_bytes",
        vec![
            mir_arg(Some("path"), Operand::String(dir_path.clone())),
            mir_arg(Some("bytes"), Operand::Place("bytes".to_string())),
        ],
    );
    expect_result_err(
        &mut runtime,
        &mut env,
        "fs::create_dir",
        vec![mir_arg(
            Some("path"),
            Operand::String(occupied_path.clone()),
        )],
    );
    expect_result_err(
        &mut runtime,
        &mut env,
        "fs::read_dir",
        vec![mir_arg(
            Some("path"),
            Operand::String(occupied_path.clone()),
        )],
    );
    expect_result_err(
        &mut runtime,
        &mut env,
        "fs::open",
        vec![mir_arg(Some("path"), Operand::String(missing_path.clone()))],
    );

    for builtin in [
        "net::connect",
        "net::connect_timeout",
        "net::listen",
        "net::udp_bind",
    ] {
        let mut args = vec![mir_arg(
            Some("address"),
            Operand::String("not a socket address".to_string()),
        )];
        if builtin == "net::connect_timeout" {
            args.push(mir_arg(Some("timeout"), Operand::Duration(1)));
        }
        expect_result_err(&mut runtime, &mut env, builtin, args);
    }

    let listen_type_error = call_name(
        &mut runtime,
        "net::listen",
        &[mir_arg(Some("address"), Operand::Bool(false))],
        &mut env,
    )
    .expect_err("net.listen should reject non-string addresses");
    assert!(listen_type_error.message.contains("expects `String`"));

    #[cfg(unix)]
    {
        expect_result_err(
            &mut runtime,
            &mut env,
            "net::unix_listen",
            vec![mir_arg(
                Some("path"),
                Operand::String(occupied_path.clone()),
            )],
        );
        expect_result_err(
            &mut runtime,
            &mut env,
            "net::unix_connect",
            vec![mir_arg(Some("path"), Operand::String(missing_path.clone()))],
        );
        expect_result_err(
            &mut runtime,
            &mut env,
            "net::unix_connect_timeout",
            vec![
                mir_arg(Some("path"), Operand::String(missing_path.clone())),
                mir_arg(Some("timeout"), Operand::Duration(1)),
            ],
        );
    }

    let _ = std::fs::remove_dir_all(&temp_root);
}

#[test]
fn mir_runtime_process_run_builtin_captures_stdio_under_scheduler() {
    let output = crate::runtime_value::run_lightweight_root_task(|| {
        let mut runtime = test_runtime();
        let mut env = Env::default();
        env.define_typed(
            "run_cmd",
            Type::Named("Vec".to_string(), vec![Type::named("String")]),
            string_vec_value(&["/bin/sh", "-c", "printf out; printf err >&2"]),
        );
        env.define_typed("cwd", Type::named("Option[String]"), option_none());
        env.define_typed(
            "env_map",
            Type::Named(
                "Map".to_string(),
                vec![Type::named("String"), Type::named("String")],
            ),
            string_map_value(&[]),
        );
        let stdio_null = call_name(&mut runtime, "process::null", &[], &mut env)?;
        let stdout_pipe = call_name(&mut runtime, "process::pipe", &[], &mut env)?;
        let stderr_pipe = call_name(&mut runtime, "process::pipe", &[], &mut env)?;
        env.define_typed("stdio_null", Type::named("process.Stdio"), stdio_null);
        env.define_typed("stdout_pipe", Type::named("process.Stdio"), stdout_pipe);
        env.define_typed("stderr_pipe", Type::named("process.Stdio"), stderr_pipe);
        call_name(
            &mut runtime,
            "process::run",
            &[
                mir_arg(Some("command"), Operand::Place("run_cmd".to_string())),
                mir_arg(Some("cwd"), Operand::Place("cwd".to_string())),
                mir_arg(Some("env"), Operand::Place("env_map".to_string())),
                mir_arg(Some("stdin"), Operand::Place("stdio_null".to_string())),
                mir_arg(Some("stdout"), Operand::Place("stdout_pipe".to_string())),
                mir_arg(Some("stderr"), Operand::Place("stderr_pipe".to_string())),
                mir_arg(Some("timeout"), Operand::Duration(2_000)),
                mir_arg(Some("group"), Operand::Bool(false)),
            ],
            &mut env,
        )
    })
    .expect("process.run should execute inside the lightweight scheduler");

    let completed = match result_ok_payload(output) {
        Value::ProcessCompleted(completed) => completed,
        other => panic!("expected process completed value, found {other:?}"),
    };
    assert_eq!(completed.stdout_bytes(), b"out".to_vec());
    assert_eq!(completed.stderr_bytes(), b"err".to_vec());
}

#[test]
fn mir_runtime_process_builtins_cover_spawn_timeout_and_cancelled_edges() {
    fn expect_process_error_variant(value: Value, variant_name: &str) {
        let error = enum_payloads(value, "Result", "Err").remove(0);
        enum_payloads(error, "Error", variant_name);
    }

    fn install_process_env(runtime: &mut MirRuntime, env: &mut Env, command: &[&str]) {
        env.define_typed(
            "command",
            Type::Named("Vec".to_string(), vec![Type::named("String")]),
            string_vec_value(command),
        );
        env.define_typed("cwd", Type::named("Option[String]"), option_none());
        env.define_typed(
            "env_map",
            Type::Named(
                "Map".to_string(),
                vec![Type::named("String"), Type::named("String")],
            ),
            string_map_value(&[]),
        );
        let stdio_null = call_name(runtime, "process::null", &[], env)
            .expect("process.null should construct stdio");
        env.define_typed("stdio_null", Type::named("process.Stdio"), stdio_null);
    }

    fn process_args(timeout: Option<i128>) -> Vec<MirArg> {
        let mut args = vec![
            mir_arg(Some("command"), Operand::Place("command".to_string())),
            mir_arg(Some("cwd"), Operand::Place("cwd".to_string())),
            mir_arg(Some("env"), Operand::Place("env_map".to_string())),
            mir_arg(Some("stdin"), Operand::Place("stdio_null".to_string())),
            mir_arg(Some("stdout"), Operand::Place("stdio_null".to_string())),
            mir_arg(Some("stderr"), Operand::Place("stdio_null".to_string())),
        ];
        if let Some(timeout) = timeout {
            args.push(mir_arg(Some("timeout"), Operand::Duration(timeout)));
        }
        args.push(mir_arg(Some("group"), Operand::Bool(false)));
        args
    }

    let mut runtime = test_runtime();
    let mut env = Env::default();
    install_process_env(
        &mut runtime,
        &mut env,
        &["/__definitely_missing_aurora_process_builtin__"],
    );
    expect_process_error_variant(
        call_name(
            &mut runtime,
            "process::start",
            &process_args(None),
            &mut env,
        )
        .expect("process.start spawn failures should return Result.Err"),
        "Spawn",
    );
    expect_process_error_variant(
        call_name(
            &mut runtime,
            "process::run",
            &process_args(Some(1_000)),
            &mut env,
        )
        .expect("process.run spawn failures should return Result.Err"),
        "Spawn",
    );

    let mut timeout_runtime = test_runtime();
    let mut timeout_env = Env::default();
    install_process_env(
        &mut timeout_runtime,
        &mut timeout_env,
        &["/bin/sh", "-c", "sleep 1"],
    );
    expect_process_error_variant(
        call_name(
            &mut timeout_runtime,
            "process::run",
            &process_args(Some(0)),
            &mut timeout_env,
        )
        .expect("process.run timeouts should return Result.Err"),
        "TimedOut",
    );

    let cancellation = CancellationContext::default();
    let group = TaskGroupValue::new(&cancellation);
    let cancelled_context = group.child_cancellation();
    group.cancel();
    let mut cancelled_runtime = MirRuntime::new(
        MirModule {
            functions: Vec::new(),
            classes: Vec::new(),
            trait_impls: Vec::new(),
            top_level: None,
        },
        Arc::new(Mutex::new(String::new())),
        cancelled_context,
    );
    let mut cancelled_env = Env::default();
    install_process_env(
        &mut cancelled_runtime,
        &mut cancelled_env,
        &["/bin/sh", "-c", "sleep 1"],
    );
    expect_process_error_variant(
        call_name(
            &mut cancelled_runtime,
            "process::run",
            &process_args(Some(1_000)),
            &mut cancelled_env,
        )
        .expect("process.run cancellations should return Result.Err"),
        "Cancelled",
    );
}

#[test]
fn mir_runtime_member_call_dispatch_covers_builtin_runtime_and_trait_receivers() {
    let mut runtime = test_runtime();
    runtime.functions.insert(
        "widget_render".to_string(),
        MirFunction {
            name: "widget_render".to_string(),
            module_name: "<test>".to_string(),
            span: Span::new(1, 1),
            receiver: Some(crate::mir::MirReceiverKind::Borrow),
            params: Vec::new(),
            local_types: Vec::new(),
            return_type: Type::named("String"),
            entry: "entry".to_string(),
            blocks: vec![BasicBlock {
                label: "entry".to_string(),
                instructions: Vec::new(),
                terminator: Terminator::Return(Operand::String("widget".to_string())),
            }],
        },
    );
    runtime.functions.insert(
        "status_label".to_string(),
        MirFunction {
            name: "status_label".to_string(),
            module_name: "<test>".to_string(),
            span: Span::new(1, 1),
            receiver: Some(crate::mir::MirReceiverKind::Borrow),
            params: Vec::new(),
            local_types: Vec::new(),
            return_type: Type::named("String"),
            entry: "entry".to_string(),
            blocks: vec![BasicBlock {
                label: "entry".to_string(),
                instructions: Vec::new(),
                terminator: Terminator::Return(Operand::String("done".to_string())),
            }],
        },
    );
    runtime.classes.insert(
        "Widget".to_string(),
        MirClass {
            name: "Widget".to_string(),
            type_params: Vec::new(),
            fields: Vec::new(),
            methods: vec![MirMethod {
                name: "render".to_string(),
                function_name: "widget_render".to_string(),
                receiver: Some(crate::mir::MirReceiverKind::Borrow),
            }],
        },
    );
    runtime.trait_impls.push(MirTraitImpl {
        trait_name: "Label".to_string(),
        trait_args: Vec::new(),
        for_type: Type::named("Status"),
        methods: vec![MirMethod {
            name: "label".to_string(),
            function_name: "status_label".to_string(),
            receiver: Some(crate::mir::MirReceiverKind::Borrow),
        }],
    });

    let mut env = Env::default();
    env.define_typed(
        "number",
        Type::named("int32"),
        Value::Int(IntegerValue::from_signed(7)),
    );
    env.define_typed("ratio", Type::named("float64"), Value::Float(4.0));
    env.define_typed("flag", Type::named("bool"), Value::Bool(true));
    env.define_typed(
        "text",
        Type::named("String"),
        Value::String("Aurora".to_string()),
    );
    env.define_typed(
        "values",
        Type::Named("Vec".to_string(), vec![Type::named("int32")]),
        Value::Vec(crate::runtime_value::VecValue {
            element_type: Type::named("int32"),
            elements: vec![
                Value::Int(IntegerValue::from_signed(1)),
                Value::Int(IntegerValue::from_signed(2)),
            ],
        }),
    );
    env.define_typed(
        "counts",
        Type::Named(
            "Map".to_string(),
            vec![Type::named("String"), Type::named("int32")],
        ),
        Value::Map(crate::runtime_value::MapValue {
            key_type: Type::named("String"),
            value_type: Type::named("int32"),
            entries: vec![(
                Value::String("count".to_string()),
                Value::Int(IntegerValue::from_signed(1)),
            )],
        }),
    );
    env.define_typed(
        "seen",
        Type::Named("Set".to_string(), vec![Type::named("String")]),
        Value::Set(crate::runtime_value::SetValue {
            element_type: Type::named("String"),
            elements: vec![Value::String("ready".to_string())],
        }),
    );
    env.define_typed(
        "jobs",
        Type::Named("Queue".to_string(), vec![Type::named("int32")]),
        Value::Channel(ChannelValue::new()),
    );
    env.define_typed(
        "task",
        Type::Named("Task".to_string(), vec![Type::named("bool")]),
        Value::Task(TaskValue::from_handle(std::thread::spawn(|| {
            Ok(Value::Bool(true))
        }))),
    );
    env.define_typed(
        "group",
        Type::named("TaskGroup"),
        Value::TaskGroup(TaskGroupValue::new(&CancellationContext::default())),
    );
    env.define_typed(
        "widget",
        Type::named("Widget"),
        Value::Instance(InstanceValue {
            class_name: "Widget".to_string(),
            fields: BTreeMap::new(),
        }),
    );
    env.define_typed(
        "status",
        Type::named("Status"),
        Value::EnumVariant(EnumVariantValue {
            enum_name: "Status".to_string(),
            variant_name: "Done".to_string(),
            payloads: Vec::new(),
        }),
    );
    env.define_typed("unit", Type::Unit, Value::Unit);

    assert_eq!(
        runtime
            .evaluate_call(
                &crate::mir::CallTarget::Member {
                    object: Operand::Place("ratio".to_string()),
                    field: "sqrt".to_string(),
                    receiver_place: Some("ratio".to_string()),
                },
                &[],
                &mut env,
            )
            .expect("float.sqrt() should succeed"),
        Value::Float(2.0)
    );
    assert_eq!(
        runtime
            .evaluate_call(
                &crate::mir::CallTarget::Member {
                    object: Operand::Place("number".to_string()),
                    field: "to_string".to_string(),
                    receiver_place: Some("number".to_string()),
                },
                &[],
                &mut env,
            )
            .expect("int.to_string() should succeed"),
        Value::String("7".to_string())
    );
    assert_eq!(
        runtime
            .evaluate_call(
                &crate::mir::CallTarget::Member {
                    object: Operand::Place("flag".to_string()),
                    field: "to_string".to_string(),
                    receiver_place: Some("flag".to_string()),
                },
                &[],
                &mut env,
            )
            .expect("bool.to_string() should succeed"),
        Value::String("true".to_string())
    );
    assert_eq!(
        runtime
            .evaluate_call(
                &crate::mir::CallTarget::Member {
                    object: Operand::Place("text".to_string()),
                    field: "contains".to_string(),
                    receiver_place: Some("text".to_string()),
                },
                &[mir_arg(None, Operand::String("ror".to_string()))],
                &mut env,
            )
            .expect("string member calls should dispatch"),
        Value::Bool(true)
    );
    assert_eq!(
        runtime
            .evaluate_call(
                &crate::mir::CallTarget::Member {
                    object: Operand::Place("values".to_string()),
                    field: "insert".to_string(),
                    receiver_place: Some("values".to_string()),
                },
                &[
                    mir_arg(None, Operand::Int(1)),
                    mir_arg(None, Operand::Int(9)),
                ],
                &mut env,
            )
            .expect("vec member calls should dispatch"),
        Value::Bool(true)
    );
    assert_eq!(
        runtime
            .evaluate_call(
                &crate::mir::CallTarget::Member {
                    object: Operand::Place("counts".to_string()),
                    field: "clear".to_string(),
                    receiver_place: Some("counts".to_string()),
                },
                &[],
                &mut env,
            )
            .expect("map member calls should dispatch"),
        Value::Unit
    );
    assert_eq!(
        runtime
            .evaluate_call(
                &crate::mir::CallTarget::Member {
                    object: Operand::Place("seen".to_string()),
                    field: "insert".to_string(),
                    receiver_place: Some("seen".to_string()),
                },
                &[mir_arg(None, Operand::String("go".to_string()))],
                &mut env,
            )
            .expect("set member calls should dispatch"),
        Value::Bool(true)
    );
    assert_eq!(
        runtime
            .evaluate_call(
                &crate::mir::CallTarget::Member {
                    object: Operand::Place("jobs".to_string()),
                    field: "close".to_string(),
                    receiver_place: Some("jobs".to_string()),
                },
                &[],
                &mut env,
            )
            .expect("channel member calls should dispatch"),
        Value::Unit
    );
    assert_eq!(
        runtime
            .evaluate_call(
                &crate::mir::CallTarget::Member {
                    object: Operand::Place("task".to_string()),
                    field: "result".to_string(),
                    receiver_place: Some("task".to_string()),
                },
                &[],
                &mut env,
            )
            .expect("task member calls should dispatch"),
        task_result_ready(Value::Bool(true))
    );
    assert_eq!(
        runtime
            .evaluate_call(
                &crate::mir::CallTarget::Member {
                    object: Operand::Place("group".to_string()),
                    field: "cancel".to_string(),
                    receiver_place: Some("group".to_string()),
                },
                &[],
                &mut env,
            )
            .expect("task-group member calls should dispatch"),
        Value::Unit
    );
    assert_eq!(
        runtime
            .evaluate_call(
                &crate::mir::CallTarget::Member {
                    object: Operand::Place("widget".to_string()),
                    field: "render".to_string(),
                    receiver_place: Some("widget".to_string()),
                },
                &[],
                &mut env,
            )
            .expect("class member calls should dispatch"),
        Value::String("widget".to_string())
    );
    assert_eq!(
        runtime
            .evaluate_call(
                &crate::mir::CallTarget::Member {
                    object: Operand::Place("status".to_string()),
                    field: "label".to_string(),
                    receiver_place: Some("status".to_string()),
                },
                &[],
                &mut env,
            )
            .expect("runtime type fallback trait dispatch should succeed"),
        Value::String("done".to_string())
    );

    let unsupported = runtime
        .evaluate_call(
            &crate::mir::CallTarget::Member {
                object: Operand::Place("unit".to_string()),
                field: "missing".to_string(),
                receiver_place: Some("unit".to_string()),
            },
            &[],
            &mut env,
        )
        .expect_err("unsupported members should fail cleanly");
    assert!(unsupported
        .message
        .contains("unsupported MIR member call `missing`"));
}

#[test]
fn mir_runtime_builtin_error_surface_covers_additional_builtin_branches() {
    let mut runtime = test_runtime();
    let mut env = Env::default();
    env.define_typed(
        "huge_unsigned",
        Type::named("uint128"),
        Value::Int(IntegerValue::from_literal((i128::MAX as u128) + 1)),
    );
    env.define_typed(
        "min_signed",
        Type::named("int128"),
        Value::Int(IntegerValue::from_signed(i128::MIN)),
    );
    env.define_typed(
        "word",
        Type::named("String"),
        Value::String("aurora".to_string()),
    );
    env.define_typed("flag", Type::named("bool"), Value::Bool(true));
    env.define_typed(
        "negative_duration",
        Type::named("Duration"),
        Value::Duration(-1),
    );

    let sleep_range = runtime
        .evaluate_call(
            &crate::mir::CallTarget::Name("sleep".to_string()),
            &[mir_arg(
                None,
                Operand::Place("negative_duration".to_string()),
            )],
            &mut env,
        )
        .expect_err("sleep() should reject negative durations");
    assert!(sleep_range
        .message
        .contains("does not fit in the MIR runtime timer range"));

    let sleep_unsigned_range = runtime
        .evaluate_call(
            &crate::mir::CallTarget::Name("sleep".to_string()),
            &[mir_arg(None, Operand::Place("huge_unsigned".to_string()))],
            &mut env,
        )
        .expect_err("sleep() should reject unsigned values outside signed timer range");
    assert!(sleep_unsigned_range
        .message
        .contains("duration must fit in signed timer range"));

    assert_eq!(
        runtime
            .evaluate_call(
                &crate::mir::CallTarget::Name("sleep".to_string()),
                &[mir_arg(None, Operand::Int(0))],
                &mut env,
            )
            .expect("sleep() should accept integer millisecond durations"),
        Value::Unit
    );

    let sleep_type = runtime
        .evaluate_call(
            &crate::mir::CallTarget::Name("sleep".to_string()),
            &[mir_arg(None, Operand::Place("flag".to_string()))],
            &mut env,
        )
        .expect_err("sleep() should reject non-duration values");
    assert!(sleep_type
        .message
        .contains("expects a duration value in MIR runtime"));

    let abs_overflow = runtime
        .evaluate_call(
            &crate::mir::CallTarget::Name("abs".to_string()),
            &[mir_arg(None, Operand::Place("min_signed".to_string()))],
            &mut env,
        )
        .expect_err("abs() should reject signed overflow");
    assert!(abs_overflow
        .message
        .contains("overflowed the signed integer range"));

    let abs_type = runtime
        .evaluate_call(
            &crate::mir::CallTarget::Name("abs".to_string()),
            &[mir_arg(None, Operand::Place("flag".to_string()))],
            &mut env,
        )
        .expect_err("abs() should reject non-numeric values");
    assert!(abs_type
        .message
        .contains("expects an integer or float value"));

    let parse_int64_type = runtime
        .evaluate_call(
            &crate::mir::CallTarget::Name("parse_int64".to_string()),
            &[mir_arg(None, Operand::Place("flag".to_string()))],
            &mut env,
        )
        .expect_err("parse_int64() should reject non-strings");
    assert!(parse_int64_type
        .message
        .contains("expects `String`, found `true`"));

    let parse_int32_type = runtime
        .evaluate_call(
            &crate::mir::CallTarget::Name("parse_int32".to_string()),
            &[mir_arg(None, Operand::Place("flag".to_string()))],
            &mut env,
        )
        .expect_err("parse_int32() should reject non-strings");
    assert!(parse_int32_type
        .message
        .contains("expects `String`, found `true`"));

    let parse_float64_type = runtime
        .evaluate_call(
            &crate::mir::CallTarget::Name("parse_float64".to_string()),
            &[mir_arg(None, Operand::Place("flag".to_string()))],
            &mut env,
        )
        .expect_err("parse_float64() should reject non-strings");
    assert!(parse_float64_type
        .message
        .contains("expects `String`, found `true`"));

    let io_write_type = runtime
        .evaluate_call(
            &crate::mir::CallTarget::Name("io::write".to_string()),
            &[mir_arg(None, Operand::Place("flag".to_string()))],
            &mut env,
        )
        .expect_err("io.write() should reject non-string text");
    assert!(io_write_type
        .message
        .contains("`io.write(...)` expects `String`, found `true`"));

    let fs_exists_type = runtime
        .evaluate_call(
            &crate::mir::CallTarget::Name("fs::exists".to_string()),
            &[mir_arg(None, Operand::Place("flag".to_string()))],
            &mut env,
        )
        .expect_err("fs.exists() should reject non-string paths");
    assert!(fs_exists_type
        .message
        .contains("`fs.exists(...)` expects `String`, found `true`"));

    let unknown = runtime
        .evaluate_call(
            &crate::mir::CallTarget::Name("missing".to_string()),
            &[],
            &mut env,
        )
        .expect_err("unknown MIR functions should fail");
    assert!(unknown.message.contains("unknown MIR function `missing`"));

    let queue_name = runtime
        .evaluate_call(
            &crate::mir::CallTarget::Name("Queue".to_string()),
            &[MirArg {
                name: Some("size".to_string()),
                value: Operand::Int(1),
                writeback_place: None,
            }],
            &mut env,
        )
        .expect_err("Queue() should reject unknown named arguments");
    assert!(queue_name
        .message
        .contains("expects an optional `capacity=` argument"));

    let queue_capacity = runtime
        .evaluate_call(
            &crate::mir::CallTarget::Name("Queue".to_string()),
            &[MirArg {
                name: Some("capacity".to_string()),
                value: Operand::Int(0),
                writeback_place: None,
            }],
            &mut env,
        )
        .expect_err("Queue() should reject non-positive capacities");
    assert!(queue_capacity
        .message
        .contains("expects a positive `int32`"));

    assert_eq!(
        runtime
            .evaluate_call(
                &crate::mir::CallTarget::Name("parse_int32".to_string()),
                &[mir_arg(None, Operand::Place("word".to_string()))],
                &mut env,
            )
            .expect("parse_int32() should still return Result.Err for invalid strings"),
        result_err(Value::String("invalid digit found in string".to_string()))
    );
}

#[test]
fn mir_runtime_member_error_surface_covers_remaining_dispatch_branches() {
    let mut runtime = test_runtime();
    runtime.classes.insert(
        "Empty".to_string(),
        MirClass {
            name: "Empty".to_string(),
            type_params: Vec::new(),
            fields: Vec::new(),
            methods: Vec::new(),
        },
    );
    runtime.classes.insert(
        "Broken".to_string(),
        MirClass {
            name: "Broken".to_string(),
            type_params: Vec::new(),
            fields: Vec::new(),
            methods: vec![MirMethod {
                name: "render".to_string(),
                function_name: "missing_impl".to_string(),
                receiver: Some(crate::mir::MirReceiverKind::Borrow),
            }],
        },
    );
    runtime.trait_impls.push(MirTraitImpl {
        trait_name: "Render".to_string(),
        trait_args: Vec::new(),
        for_type: Type::named("Status"),
        methods: vec![MirMethod {
            name: "render".to_string(),
            function_name: "missing_trait_impl".to_string(),
            receiver: Some(crate::mir::MirReceiverKind::Borrow),
        }],
    });

    let mut env = Env::default();
    env.define_typed("ratio", Type::named("float64"), Value::Float(4.0));
    env.define_typed(
        "number",
        Type::named("int32"),
        Value::Int(IntegerValue::from_signed(7)),
    );
    env.define_typed("flag", Type::named("bool"), Value::Bool(true));
    env.define_typed(
        "values",
        Type::Named("Vec".to_string(), vec![Type::named("int32")]),
        Value::Vec(crate::runtime_value::VecValue {
            element_type: Type::named("int32"),
            elements: vec![Value::Int(IntegerValue::from_signed(1))],
        }),
    );
    env.define_typed(
        "ghost",
        Type::named("Ghost"),
        Value::Instance(InstanceValue {
            class_name: "Ghost".to_string(),
            fields: BTreeMap::new(),
        }),
    );
    env.define_typed(
        "empty",
        Type::named("Empty"),
        Value::Instance(InstanceValue {
            class_name: "Empty".to_string(),
            fields: BTreeMap::new(),
        }),
    );
    env.define_typed(
        "broken",
        Type::named("Broken"),
        Value::Instance(InstanceValue {
            class_name: "Broken".to_string(),
            fields: BTreeMap::new(),
        }),
    );
    env.define_typed(
        "status",
        Type::named("Status"),
        Value::EnumVariant(EnumVariantValue {
            enum_name: "Status".to_string(),
            variant_name: "Ready".to_string(),
            payloads: Vec::new(),
        }),
    );

    let sqrt_args = runtime
        .evaluate_call(
            &crate::mir::CallTarget::Member {
                object: Operand::Place("ratio".to_string()),
                field: "sqrt".to_string(),
                receiver_place: Some("ratio".to_string()),
            },
            &[mir_arg(None, Operand::Int(1))],
            &mut env,
        )
        .expect_err("sqrt() should reject extra arguments");
    assert!(sqrt_args.message.contains("`sqrt` does not take arguments"));

    let int_to_string_args = runtime
        .evaluate_call(
            &crate::mir::CallTarget::Member {
                object: Operand::Place("number".to_string()),
                field: "to_string".to_string(),
                receiver_place: Some("number".to_string()),
            },
            &[mir_arg(None, Operand::Int(1))],
            &mut env,
        )
        .expect_err("int.to_string() should reject extra arguments");
    assert!(int_to_string_args
        .message
        .contains("`to_string` does not take arguments"));

    let bool_to_string_args = runtime
        .evaluate_call(
            &crate::mir::CallTarget::Member {
                object: Operand::Place("flag".to_string()),
                field: "to_string".to_string(),
                receiver_place: Some("flag".to_string()),
            },
            &[mir_arg(None, Operand::Int(1))],
            &mut env,
        )
        .expect_err("bool.to_string() should reject extra arguments");
    assert!(bool_to_string_args
        .message
        .contains("`to_string` does not take arguments"));

    let len_args = runtime
        .evaluate_call(
            &crate::mir::CallTarget::Member {
                object: Operand::Place("values".to_string()),
                field: "len".to_string(),
                receiver_place: Some("values".to_string()),
            },
            &[mir_arg(None, Operand::Int(1))],
            &mut env,
        )
        .expect_err("len() should reject extra arguments");
    assert!(len_args.message.contains("`len` does not take arguments"));

    let push_no_place = runtime
        .evaluate_call(
            &crate::mir::CallTarget::Member {
                object: Operand::Place("values".to_string()),
                field: "push".to_string(),
                receiver_place: None,
            },
            &[mir_arg(None, Operand::Int(9))],
            &mut env,
        )
        .expect_err("push() should require a mutable receiver place");
    assert!(push_no_place
        .message
        .contains("requires a mutable vector place"));

    let internal_index_args = runtime
        .evaluate_call(
            &crate::mir::CallTarget::Member {
                object: Operand::Place("values".to_string()),
                field: "__index".to_string(),
                receiver_place: Some("values".to_string()),
            },
            &[mir_arg(None, Operand::Int(0))],
            &mut env,
        )
        .expect_err("internal __index should enforce operand count");
    assert!(internal_index_args
        .message
        .contains("requires index, line, and column operands"));

    let internal_set_args = runtime
        .evaluate_call(
            &crate::mir::CallTarget::Member {
                object: Operand::Place("values".to_string()),
                field: "__set_index".to_string(),
                receiver_place: Some("values".to_string()),
            },
            &[
                mir_arg(None, Operand::Int(0)),
                mir_arg(None, Operand::Int(1)),
                mir_arg(None, Operand::Int(2)),
            ],
            &mut env,
        )
        .expect_err("internal __set_index should enforce operand count");
    assert!(internal_set_args
        .message
        .contains("requires index, value, line, and column operands"));

    let unsupported_vector_method = runtime
        .evaluate_call(
            &crate::mir::CallTarget::Member {
                object: Operand::Place("values".to_string()),
                field: "mystery".to_string(),
                receiver_place: Some("values".to_string()),
            },
            &[],
            &mut env,
        )
        .expect_err("unknown vector methods should fail");
    assert!(unsupported_vector_method
        .message
        .contains("unsupported vector method `mystery`"));

    let unknown_class = runtime
        .evaluate_call(
            &crate::mir::CallTarget::Member {
                object: Operand::Place("ghost".to_string()),
                field: "render".to_string(),
                receiver_place: Some("ghost".to_string()),
            },
            &[],
            &mut env,
        )
        .expect_err("unknown classes should fail");
    assert!(unknown_class.message.contains("unknown MIR class `Ghost`"));

    let missing_class_method = runtime
        .evaluate_call(
            &crate::mir::CallTarget::Member {
                object: Operand::Place("empty".to_string()),
                field: "render".to_string(),
                receiver_place: Some("empty".to_string()),
            },
            &[],
            &mut env,
        )
        .expect_err("missing class methods should fail");
    assert!(missing_class_method
        .message
        .contains("class `Empty` has no MIR method `render`"));

    let missing_method_body = runtime
        .evaluate_call(
            &crate::mir::CallTarget::Member {
                object: Operand::Place("broken".to_string()),
                field: "render".to_string(),
                receiver_place: Some("broken".to_string()),
            },
            &[],
            &mut env,
        )
        .expect_err("missing method bodies should fail");
    assert!(missing_method_body
        .message
        .contains("unknown MIR method body `missing_impl`"));

    let missing_trait_method_body = runtime
        .evaluate_call(
            &crate::mir::CallTarget::Member {
                object: Operand::Place("status".to_string()),
                field: "render".to_string(),
                receiver_place: Some("status".to_string()),
            },
            &[],
            &mut env,
        )
        .expect_err("missing trait method bodies should fail");
    assert!(missing_trait_method_body
        .message
        .contains("unknown MIR method body `missing_trait_impl`"));
}

#[test]
fn mir_runtime_mutating_member_calls_write_back_receivers_and_params() {
    let mut runtime = test_runtime();
    runtime.functions.insert(
        "counter_replace".to_string(),
        MirFunction {
            name: "counter_replace".to_string(),
            module_name: "<test>".to_string(),
            span: Span::new(1, 1),
            receiver: Some(crate::mir::MirReceiverKind::BorrowMut),
            params: vec![MirParam {
                name: "amount".to_string(),
                passing: crate::mir::MirReceiverKind::BorrowMut,
                ty: Type::named("int32"),
            }],
            local_types: Vec::new(),
            return_type: Type::Unit,
            entry: "entry".to_string(),
            blocks: vec![BasicBlock {
                label: "entry".to_string(),
                instructions: vec![
                    Instruction::Assign {
                        target: "self.value".to_string(),
                        value: Rvalue::Use(Operand::Int(42)),
                    },
                    Instruction::Assign {
                        target: "amount".to_string(),
                        value: Rvalue::Use(Operand::Int(17)),
                    },
                ],
                terminator: Terminator::Return(Operand::Unit),
            }],
        },
    );
    runtime.functions.insert(
        "counter_borrow_only".to_string(),
        MirFunction {
            name: "counter_borrow_only".to_string(),
            module_name: "<test>".to_string(),
            span: Span::new(1, 1),
            receiver: Some(crate::mir::MirReceiverKind::Borrow),
            params: Vec::new(),
            local_types: Vec::new(),
            return_type: Type::Unit,
            entry: "entry".to_string(),
            blocks: vec![BasicBlock {
                label: "entry".to_string(),
                instructions: Vec::new(),
                terminator: Terminator::Return(Operand::Unit),
            }],
        },
    );
    runtime.functions.insert(
        "status_mark".to_string(),
        MirFunction {
            name: "status_mark".to_string(),
            module_name: "<test>".to_string(),
            span: Span::new(1, 1),
            receiver: Some(crate::mir::MirReceiverKind::BorrowMut),
            params: vec![MirParam {
                name: "flag".to_string(),
                passing: crate::mir::MirReceiverKind::BorrowMut,
                ty: Type::named("bool"),
            }],
            local_types: Vec::new(),
            return_type: Type::Unit,
            entry: "entry".to_string(),
            blocks: vec![BasicBlock {
                label: "entry".to_string(),
                instructions: vec![Instruction::Assign {
                    target: "flag".to_string(),
                    value: Rvalue::Use(Operand::Bool(false)),
                }],
                terminator: Terminator::Return(Operand::Unit),
            }],
        },
    );
    runtime.functions.insert(
        "status_borrow_only".to_string(),
        MirFunction {
            name: "status_borrow_only".to_string(),
            module_name: "<test>".to_string(),
            span: Span::new(1, 1),
            receiver: Some(crate::mir::MirReceiverKind::Borrow),
            params: Vec::new(),
            local_types: Vec::new(),
            return_type: Type::Unit,
            entry: "entry".to_string(),
            blocks: vec![BasicBlock {
                label: "entry".to_string(),
                instructions: Vec::new(),
                terminator: Terminator::Return(Operand::Unit),
            }],
        },
    );
    runtime.classes.insert(
        "Counter".to_string(),
        MirClass {
            name: "Counter".to_string(),
            type_params: Vec::new(),
            fields: vec![crate::mir::MirClassField {
                name: "value".to_string(),
                ty: Type::named("int32"),
            }],
            methods: vec![
                MirMethod {
                    name: "replace".to_string(),
                    function_name: "counter_replace".to_string(),
                    receiver: Some(crate::mir::MirReceiverKind::BorrowMut),
                },
                MirMethod {
                    name: "broken".to_string(),
                    function_name: "counter_borrow_only".to_string(),
                    receiver: Some(crate::mir::MirReceiverKind::BorrowMut),
                },
            ],
        },
    );
    runtime.trait_impls.push(MirTraitImpl {
        trait_name: "Mark".to_string(),
        trait_args: Vec::new(),
        for_type: Type::named("Status"),
        methods: vec![
            MirMethod {
                name: "mark".to_string(),
                function_name: "status_mark".to_string(),
                receiver: Some(crate::mir::MirReceiverKind::BorrowMut),
            },
            MirMethod {
                name: "broken".to_string(),
                function_name: "status_borrow_only".to_string(),
                receiver: Some(crate::mir::MirReceiverKind::BorrowMut),
            },
        ],
    });

    let mut env = Env::default();
    env.define_typed(
        "counter",
        Type::named("Counter"),
        Value::Instance(InstanceValue {
            class_name: "Counter".to_string(),
            fields: BTreeMap::from([(
                "value".to_string(),
                Value::Int(IntegerValue::from_signed(1)),
            )]),
        }),
    );
    env.define_typed(
        "amount",
        Type::named("int32"),
        Value::Int(IntegerValue::from_signed(3)),
    );
    env.define_typed(
        "status",
        Type::named("Status"),
        Value::EnumVariant(EnumVariantValue {
            enum_name: "Status".to_string(),
            variant_name: "Ready".to_string(),
            payloads: Vec::new(),
        }),
    );
    env.define_typed("flag", Type::named("bool"), Value::Bool(true));

    assert_eq!(
        runtime
            .evaluate_call(
                &crate::mir::CallTarget::Member {
                    object: Operand::Place("counter".to_string()),
                    field: "replace".to_string(),
                    receiver_place: Some("counter".to_string()),
                },
                &[MirArg {
                    name: None,
                    value: Operand::Place("amount".to_string()),
                    writeback_place: Some("amount".to_string()),
                }],
                &mut env,
            )
            .expect("mutable class member calls should dispatch"),
        Value::Unit
    );
    assert_eq!(
        env.read_place("counter.value"),
        Ok(Value::Int(IntegerValue::from_signed(42)))
    );
    assert_eq!(
        env.read_place("amount"),
        Ok(Value::Int(IntegerValue::from_signed(17)))
    );

    let missing_class_update = runtime
        .evaluate_call(
            &crate::mir::CallTarget::Member {
                object: Operand::Place("counter".to_string()),
                field: "broken".to_string(),
                receiver_place: Some("counter".to_string()),
            },
            &[],
            &mut env,
        )
        .expect_err("mutable class metadata must be matched by function metadata");
    assert!(missing_class_update
        .message
        .contains("mutable MIR method `broken` did not return an updated receiver"));

    assert_eq!(
        runtime
            .evaluate_call(
                &crate::mir::CallTarget::Member {
                    object: Operand::Place("status".to_string()),
                    field: "mark".to_string(),
                    receiver_place: Some("status".to_string()),
                },
                &[MirArg {
                    name: None,
                    value: Operand::Place("flag".to_string()),
                    writeback_place: Some("flag".to_string()),
                }],
                &mut env,
            )
            .expect("mutable trait member calls should dispatch"),
        Value::Unit
    );
    assert_eq!(env.read_place("flag"), Ok(Value::Bool(false)));

    let missing_trait_update = runtime
        .evaluate_call(
            &crate::mir::CallTarget::Member {
                object: Operand::Place("status".to_string()),
                field: "broken".to_string(),
                receiver_place: Some("status".to_string()),
            },
            &[],
            &mut env,
        )
        .expect_err("mutable trait metadata must be matched by function metadata");
    assert!(missing_trait_update
        .message
        .contains("mutable MIR method `broken` did not return an updated receiver"));
}

#[test]
fn mir_runtime_range_and_type_substitution_helpers_cover_remaining_paths() {
    let range = build_range(vec![
        EvaluatedMirArg {
            name: None,
            value: Value::Int(IntegerValue::from_signed(2)),
            writeback_place: None,
        },
        EvaluatedMirArg {
            name: None,
            value: Value::Int(IntegerValue::from_signed(5)),
            writeback_place: None,
        },
    ])
    .expect("range should build");
    assert_eq!(range, Value::Range(RangeValue { start: 2, end: 5 }));
    let named_range = build_range(vec![EvaluatedMirArg {
        name: Some("stop".to_string()),
        value: Value::Int(IntegerValue::from_signed(3)),
        writeback_place: None,
    }])
    .expect("named stop should build range from zero");
    assert_eq!(named_range, Value::Range(RangeValue { start: 0, end: 3 }));
    let range_error = build_range(vec![EvaluatedMirArg {
        name: Some("unknown".to_string()),
        value: Value::Int(IntegerValue::from_signed(1)),
        writeback_place: None,
    }])
    .expect_err("unknown range argument should fail");
    assert!(range_error.message.contains("unknown MIR `range` argument"));
    let named_start_stop_range = build_range(vec![
        EvaluatedMirArg {
            name: Some("start".to_string()),
            value: Value::Int(IntegerValue::from_signed(4)),
            writeback_place: None,
        },
        EvaluatedMirArg {
            name: Some("stop".to_string()),
            value: Value::Int(IntegerValue::from_signed(9)),
            writeback_place: None,
        },
    ])
    .expect("named start and stop should build range");
    assert_eq!(
        named_start_stop_range,
        Value::Range(RangeValue { start: 4, end: 9 })
    );
    let non_int_range_error = build_range(vec![EvaluatedMirArg {
        name: None,
        value: Value::String("5".to_string()),
        writeback_place: None,
    }])
    .expect_err("range should reject non-integer arguments");
    assert!(non_int_range_error
        .message
        .contains("requires integer arguments"));
    let too_many_range_args = build_range(vec![
        EvaluatedMirArg {
            name: None,
            value: Value::Int(IntegerValue::from_signed(1)),
            writeback_place: None,
        },
        EvaluatedMirArg {
            name: None,
            value: Value::Int(IntegerValue::from_signed(2)),
            writeback_place: None,
        },
        EvaluatedMirArg {
            name: None,
            value: Value::Int(IntegerValue::from_signed(3)),
            writeback_place: None,
        },
    ])
    .expect_err("range should reject more than two positional arguments");
    assert!(too_many_range_args
        .message
        .contains("takes at most two arguments"));
    let missing_stop_range = build_range(vec![EvaluatedMirArg {
        name: Some("start".to_string()),
        value: Value::Int(IntegerValue::from_signed(1)),
        writeback_place: None,
    }])
    .expect_err("range should require a stop endpoint");
    assert!(missing_stop_range.message.contains("requires `stop`"));

    let mut substitutions = HashMap::new();
    collect_runtime_type_substitutions(
        &Type::Named(
            "Map".to_string(),
            vec![
                Type::TypeParam("K".to_string()),
                Type::TypeParam("V".to_string()),
            ],
        ),
        &Type::Named(
            "Map".to_string(),
            vec![Type::named("String"), Type::named("int32")],
        ),
        &mut substitutions,
    );
    assert_eq!(substitutions.get("K"), Some(&Type::named("String")));
    assert_eq!(substitutions.get("V"), Some(&Type::named("int32")));
    collect_runtime_type_substitutions(
        &Type::Named(
            "Vec".to_string(),
            vec![Type::TypeParam("Ignored".to_string())],
        ),
        &Type::named("String"),
        &mut substitutions,
    );
    collect_runtime_type_substitutions(
        &Type::Named(
            "Vec".to_string(),
            vec![Type::TypeParam("Ignored".to_string())],
        ),
        &Type::Named("Set".to_string(), vec![Type::named("String")]),
        &mut substitutions,
    );
    collect_runtime_type_substitutions(&Type::Unit, &Type::Unit, &mut substitutions);
    assert!(!substitutions.contains_key("Ignored"));

    let mut collected = BTreeSet::new();
    collect_type_params_from_type(
        &Type::Named(
            "Result".to_string(),
            vec![
                Type::TypeParam("T".to_string()),
                Type::TypeParam("E".to_string()),
            ],
        ),
        &mut collected,
    );
    assert_eq!(
        collected,
        BTreeSet::from(["E".to_string(), "T".to_string()])
    );
    collect_type_params_from_type(&Type::TypeParam("Direct".to_string()), &mut collected);
    collect_type_params_from_type(&Type::Unit, &mut collected);
    assert!(collected.contains("Direct"));

    assert_eq!(
        eval_ordering(
            crate::ast::BinaryOp::Less,
            Value::Int(IntegerValue::from_signed(1)),
            Value::Int(IntegerValue::from_signed(2)),
        )
        .expect("int ordering should work"),
        Value::Bool(true)
    );
    assert_eq!(
        eval_ordering(
            crate::ast::BinaryOp::LessEq,
            Value::Float(1.0),
            Value::Float(1.0),
        )
        .expect("float <= ordering should work"),
        Value::Bool(true)
    );
    assert_eq!(
        eval_ordering(
            crate::ast::BinaryOp::Greater,
            Value::Float(2.0),
            Value::Float(1.0),
        )
        .expect("float > ordering should work"),
        Value::Bool(true)
    );
    assert_eq!(
        eval_ordering(
            crate::ast::BinaryOp::GreaterEq,
            Value::Float(2.0),
            Value::Float(2.0),
        )
        .expect("float >= ordering should work"),
        Value::Bool(true)
    );
    let ordering_error = eval_ordering(
        crate::ast::BinaryOp::Less,
        Value::Bool(true),
        Value::Bool(false),
    )
    .expect_err("non-numeric ordering should fail");
    assert!(ordering_error.message.contains("matching numeric operands"));
}

#[test]
fn serialized_mir_helper_reports_invalid_payloads() {
    let error = run_serialized_mir(
        b"{not-json}",
        "/tmp/test.au",
        "def main() -> int32:\n    return 0\n",
    )
    .expect_err("invalid embedded MIR should fail");
    assert!(error.message.contains("failed to deserialize embedded MIR"));
}

#[test]
fn mir_runtime_try_error_conversion_helpers_cover_context_and_from_paths() {
    let mut runtime = test_runtime();
    let no_context = runtime
        .convert_try_error_via_from(Value::String("boom".to_string()), &Type::named("String"))
        .expect_err("try error conversion should require a Result return context");
    assert!(no_context
        .message
        .contains("only allowed inside a function returning `Result`"));

    runtime.return_type_stack.push(Type::named("int32"));
    let non_result_context = runtime
        .convert_try_error_via_from(Value::String("boom".to_string()), &Type::named("String"))
        .expect_err("try error conversion should reject non-Result return types");
    assert!(non_result_context
        .message
        .contains("only allowed inside a function returning `Result`"));
    runtime.return_type_stack.pop();

    runtime.return_type_stack.push(Type::Named(
        "Result".to_string(),
        vec![Type::named("int32"), Type::named("bool")],
    ));
    let mismatch = runtime
        .convert_try_error_via_from(Value::String("boom".to_string()), &Type::named("String"))
        .expect_err("try error conversion should reject unrelated error types");
    assert!(mismatch
        .message
        .contains("does not match enclosing `Result`"));
    runtime.return_type_stack.pop();

    let lookup_runtime = MirRuntime::new(
        MirModule {
            functions: Vec::new(),
            classes: Vec::new(),
            trait_impls: vec![
                MirTraitImpl {
                    trait_name: "Display".to_string(),
                    trait_args: vec![Type::named("int32")],
                    for_type: Type::named("String"),
                    methods: Vec::new(),
                },
                MirTraitImpl {
                    trait_name: "From".to_string(),
                    trait_args: Vec::new(),
                    for_type: Type::named("String"),
                    methods: Vec::new(),
                },
                MirTraitImpl {
                    trait_name: "From".to_string(),
                    trait_args: vec![Type::named("int32")],
                    for_type: Type::named("bool"),
                    methods: Vec::new(),
                },
                MirTraitImpl {
                    trait_name: "From".to_string(),
                    trait_args: vec![Type::named("bool")],
                    for_type: Type::named("String"),
                    methods: Vec::new(),
                },
                MirTraitImpl {
                    trait_name: "From".to_string(),
                    trait_args: vec![Type::named("int32")],
                    for_type: Type::named("String"),
                    methods: vec![MirMethod {
                        name: "from".to_string(),
                        function_name: "missing_from_body".to_string(),
                        receiver: None,
                    }],
                },
            ],
            top_level: None,
        },
        Arc::new(Mutex::new(String::new())),
        CancellationContext::default(),
    );
    assert!(lookup_runtime
        .find_from_trait_impl_method(&Type::named("int32"), &Type::named("String"))
        .is_none());

    let from_function = MirFunction {
        name: "from_int_error".to_string(),
        module_name: "<test>".to_string(),
        span: Span::new(1, 1),
        receiver: None,
        params: vec![MirParam {
            name: "value".to_string(),
            passing: crate::mir::MirReceiverKind::Value,
            ty: Type::named("int32"),
        }],
        local_types: Vec::new(),
        return_type: Type::named("String"),
        entry: "entry".to_string(),
        blocks: vec![BasicBlock {
            label: "entry".to_string(),
            instructions: Vec::new(),
            terminator: Terminator::Return(Operand::String("converted".to_string())),
        }],
    };
    let mut converting_runtime = MirRuntime::new(
        MirModule {
            functions: vec![from_function],
            classes: Vec::new(),
            trait_impls: vec![MirTraitImpl {
                trait_name: "From".to_string(),
                trait_args: vec![Type::named("int32")],
                for_type: Type::named("String"),
                methods: vec![MirMethod {
                    name: "from".to_string(),
                    function_name: "from_int_error".to_string(),
                    receiver: None,
                }],
            }],
            top_level: None,
        },
        Arc::new(Mutex::new(String::new())),
        CancellationContext::default(),
    );
    converting_runtime.return_type_stack.push(Type::Named(
        "Result".to_string(),
        vec![Type::named("int32"), Type::named("String")],
    ));
    assert_eq!(
        converting_runtime
            .convert_try_error_via_from(
                Value::Int(IntegerValue::from_signed(7)),
                &Type::named("int32")
            )
            .expect("From-based try error conversion should run the impl method"),
        Value::String("converted".to_string())
    );
}

#[test]
fn trait_impl_lookup_and_top_level_run_helpers_cover_runtime_paths() {
    let render_method = MirMethod {
        name: "render".to_string(),
        function_name: "render_impl".to_string(),
        receiver: Some(crate::mir::MirReceiverKind::Borrow),
    };
    let runtime = MirRuntime::new(
        MirModule {
            functions: Vec::new(),
            classes: Vec::new(),
            trait_impls: vec![
                MirTraitImpl {
                    trait_name: "Render".to_string(),
                    trait_args: Vec::new(),
                    for_type: Type::Named(
                        "Box".to_string(),
                        vec![Type::TypeParam("T".to_string())],
                    ),
                    methods: vec![render_method.clone()],
                },
                MirTraitImpl {
                    trait_name: "Render".to_string(),
                    trait_args: Vec::new(),
                    for_type: Type::named("Widget"),
                    methods: vec![render_method.clone()],
                },
                MirTraitImpl {
                    trait_name: "Preview".to_string(),
                    trait_args: Vec::new(),
                    for_type: Type::named("Widget"),
                    methods: vec![render_method.clone()],
                },
                MirTraitImpl {
                    trait_name: "Display".to_string(),
                    trait_args: Vec::new(),
                    for_type: Type::named("Widget"),
                    methods: vec![MirMethod {
                        name: "display".to_string(),
                        function_name: "display_impl".to_string(),
                        receiver: Some(crate::mir::MirReceiverKind::Borrow),
                    }],
                },
            ],
            top_level: None,
        },
        Arc::new(Mutex::new(String::new())),
        CancellationContext::default(),
    );
    assert_eq!(
        runtime
            .find_trait_impl_method(
                &Type::Named("Box".to_string(), vec![Type::named("int32")]),
                "render",
            )
            .map(|method| method.function_name.as_str()),
        Some("render_impl")
    );
    assert_eq!(
        runtime
            .find_trait_impl_method_for_class_name("Widget", "display")
            .map(|method| method.function_name.as_str()),
        Some("display_impl")
    );
    assert!(
        runtime
            .find_trait_impl_method_for_class_name("Widget", "render")
            .is_none(),
        "ambiguous class-name trait lookups should return None",
    );
    assert!(runtime
        .find_trait_impl_method(&Type::named("Missing"), "render")
        .is_none());
    assert!(runtime
        .find_trait_impl_method_for_class_name("Missing", "render")
        .is_none());

    assert_eq!(
        MirRuntime::infer_value_type(&Value::Vec(crate::runtime_value::VecValue {
            element_type: Type::named("int32"),
            elements: vec![Value::Int(IntegerValue::from_signed(1))],
        })),
        Some(Type::Named("Vec".to_string(), vec![Type::named("int32")]))
    );
    assert_eq!(
        MirRuntime::infer_value_type(&Value::Set(crate::runtime_value::SetValue {
            element_type: Type::named("String"),
            elements: vec![Value::String("ready".to_string())],
        })),
        Some(Type::Named("Set".to_string(), vec![Type::named("String")]))
    );
    assert_eq!(
        MirRuntime::infer_value_type(&Value::Map(crate::runtime_value::MapValue {
            key_type: Type::named("String"),
            value_type: Type::named("int32"),
            entries: vec![(
                Value::String("count".to_string()),
                Value::Int(IntegerValue::from_signed(1)),
            )],
        })),
        Some(Type::Named(
            "Map".to_string(),
            vec![Type::named("String"), Type::named("int32")]
        ))
    );
    assert_eq!(
        MirRuntime::infer_value_type(&option_none()),
        Some(Type::Named("Option".to_string(), vec![Type::Unit]))
    );
    assert_eq!(
        MirRuntime::infer_value_type(&result_err(Value::String("oops".to_string()))),
        Some(Type::Named(
            "Result".to_string(),
            vec![Type::Unit, Type::named("String")]
        ))
    );
    assert_eq!(
        MirRuntime::infer_value_type(&send_error_closed(Value::Bool(true))),
        Some(Type::Named(
            "SendError".to_string(),
            vec![Type::named("bool")]
        ))
    );
    assert_eq!(
        MirRuntime::infer_value_type(&Value::EnumVariant(EnumVariantValue {
            enum_name: "SendError".to_string(),
            variant_name: "Cancelled".to_string(),
            payloads: vec![Value::String("payload".to_string())],
        })),
        Some(Type::Named(
            "SendError".to_string(),
            vec![Type::named("String")]
        ))
    );
    assert_eq!(
        MirRuntime::infer_value_type(&Value::EnumVariant(EnumVariantValue {
            enum_name: "Wait".to_string(),
            variant_name: "TimedOut".to_string(),
            payloads: Vec::new(),
        })),
        Some(Type::Named("process.Wait".to_string(), Vec::new()))
    );
    assert_eq!(
        MirRuntime::infer_value_type(&Value::EnumVariant(EnumVariantValue {
            enum_name: "Error".to_string(),
            variant_name: "TimedOut".to_string(),
            payloads: Vec::new(),
        })),
        Some(Type::Named("process.Error".to_string(), Vec::new()))
    );
    assert_eq!(
        MirRuntime::infer_value_type(&Value::EnumVariant(EnumVariantValue {
            enum_name: "Stdio".to_string(),
            variant_name: "Pipe".to_string(),
            payloads: Vec::new(),
        })),
        Some(Type::Named("process.Stdio".to_string(), Vec::new()))
    );
    assert_eq!(
        MirRuntime::infer_value_type(&Value::Channel(ChannelValue::new())),
        None
    );
    assert_eq!(
        MirRuntime::infer_value_type(&Value::Task(TaskValue::from_handle(std::thread::spawn(
            || Ok(Value::Unit)
        )))),
        None
    );
    assert_eq!(
        MirRuntime::infer_value_type(&Value::TaskGroup(TaskGroupValue::new(
            &CancellationContext::default()
        ))),
        None
    );

    let top_level_module =
        crate::lower_source_to_mir("print(1)\n").expect("top-level script should lower");
    let stdout = Arc::new(Mutex::new(String::new()));
    let mut top_level_runtime = MirRuntime::new(
        top_level_module,
        stdout.clone(),
        CancellationContext::default(),
    );
    assert_eq!(
        top_level_runtime
            .run_main()
            .expect("top-level script should execute"),
        Value::Int(IntegerValue::zero())
    );
    assert_eq!(stdout.lock().unwrap().as_str(), "1\n");

    let mut missing_entrypoint_runtime = MirRuntime::new(
        MirModule {
            functions: Vec::new(),
            classes: vec![MirClass {
                name: "Box".to_string(),
                type_params: vec!["T".to_string()],
                fields: vec![crate::mir::MirClassField {
                    name: "value".to_string(),
                    ty: Type::TypeParam("T".to_string()),
                }],
                methods: Vec::new(),
            }],
            trait_impls: Vec::new(),
            top_level: None,
        },
        Arc::new(Mutex::new(String::new())),
        CancellationContext::default(),
    );
    assert_eq!(
        missing_entrypoint_runtime.infer_instance_type(&InstanceValue {
            class_name: "Box".to_string(),
            fields: BTreeMap::from([(
                "value".to_string(),
                Value::Int(IntegerValue::from_signed(9)),
            )]),
        }),
        Some(Type::Named("Box".to_string(), vec![Type::named("int32")]))
    );
    let entrypoint_error = missing_entrypoint_runtime
        .run_main()
        .expect_err("missing entrypoints should fail");
    assert!(entrypoint_error
        .message
        .contains("no `main` function or top-level script statements were found"));
}

#[test]
fn mir_runtime_collection_string_and_task_helpers_cover_remaining_paths() {
    let mut runtime = test_runtime();
    let mut env = Env::default();
    env.define_typed(
        "values",
        Type::Named("Vec".to_string(), vec![Type::named("int32")]),
        Value::Vec(crate::runtime_value::VecValue {
            element_type: Type::named("int32"),
            elements: vec![
                Value::Int(IntegerValue::from_signed(1)),
                Value::Int(IntegerValue::from_signed(2)),
            ],
        }),
    );
    env.define_typed(
        "other",
        Type::Named("Vec".to_string(), vec![Type::named("int32")]),
        Value::Vec(crate::runtime_value::VecValue {
            element_type: Type::named("int32"),
            elements: vec![Value::Int(IntegerValue::from_signed(3))],
        }),
    );
    env.define_typed(
        "texts",
        Type::Named("Vec".to_string(), vec![Type::named("String")]),
        Value::Vec(crate::runtime_value::VecValue {
            element_type: Type::named("String"),
            elements: vec![
                Value::String("one".to_string()),
                Value::String("two".to_string()),
            ],
        }),
    );
    env.define_typed(
        "mapping",
        Type::Named(
            "Map".to_string(),
            vec![Type::named("String"), Type::named("int32")],
        ),
        Value::Map(crate::runtime_value::MapValue {
            key_type: Type::named("String"),
            value_type: Type::named("int32"),
            entries: vec![(
                Value::String("count".to_string()),
                Value::Int(IntegerValue::from_signed(1)),
            )],
        }),
    );
    env.define_typed(
        "mapping_other",
        Type::Named(
            "Map".to_string(),
            vec![Type::named("String"), Type::named("int32")],
        ),
        Value::Map(crate::runtime_value::MapValue {
            key_type: Type::named("String"),
            value_type: Type::named("int32"),
            entries: vec![(
                Value::String("next".to_string()),
                Value::Int(IntegerValue::from_signed(9)),
            )],
        }),
    );
    env.define_typed(
        "flags",
        Type::Named("Set".to_string(), vec![Type::named("String")]),
        Value::Set(crate::runtime_value::SetValue {
            element_type: Type::named("String"),
            elements: vec![Value::String("ready".to_string())],
        }),
    );
    env.define_typed(
        "jobs",
        Type::Named("Queue".to_string(), vec![Type::named("int32")]),
        Value::Channel(ChannelValue::new()),
    );
    let vec_from_env = |env: &Env, place: &str| match env.read_place(place).unwrap() {
        Value::Vec(vector) => vector,
        other => panic!("expected vec at `{place}`, found {other:?}"),
    };
    let map_from_env = |env: &Env, place: &str| match env.read_place(place).unwrap() {
        Value::Map(map) => map,
        other => panic!("expected map at `{place}`, found {other:?}"),
    };
    let channel_from_env = |env: &Env, place: &str| match env.read_place(place).unwrap() {
        Value::Channel(channel) => channel,
        other => panic!("expected channel at `{place}`, found {other:?}"),
    };

    let vec_len = runtime
        .evaluate_vec_method(
            vec_from_env(&env, "values"),
            "len",
            Some("values"),
            &[],
            &mut env,
        )
        .expect("vec len should succeed");
    assert_eq!(vec_len, Value::Int(IntegerValue::from_signed(2)));

    let vec_empty = runtime
        .evaluate_vec_method(
            vec_from_env(&env, "values"),
            "is_empty",
            Some("values"),
            &[],
            &mut env,
        )
        .expect("vec is_empty should succeed");
    assert_eq!(vec_empty, Value::Bool(false));

    let vec_clone = runtime
        .evaluate_vec_method(
            vec_from_env(&env, "values"),
            "clone",
            Some("values"),
            &[],
            &mut env,
        )
        .expect("vec clone should succeed");
    match vec_clone {
        Value::Vec(vector) => assert_eq!(vector.elements.len(), 2),
        other => panic!("expected vec clone, found {other:?}"),
    }

    let vec_get = runtime
        .evaluate_vec_method(
            vec_from_env(&env, "values"),
            "get",
            Some("values"),
            &[mir_arg(Some("index"), Operand::Int(0))],
            &mut env,
        )
        .expect("vec get should succeed");
    assert_eq!(
        vec_get,
        option_some(Value::Int(IntegerValue::from_signed(1)))
    );

    let vec_contains = runtime
        .evaluate_vec_method(
            vec_from_env(&env, "values"),
            "contains",
            Some("values"),
            &[mir_arg(Some("value"), Operand::Int(2))],
            &mut env,
        )
        .expect("vec contains should succeed");
    assert_eq!(vec_contains, Value::Bool(true));

    let vec_is_empty_args = runtime
        .evaluate_vec_method(
            vec_from_env(&env, "values"),
            "is_empty",
            Some("values"),
            &[mir_arg(None, Operand::Int(1))],
            &mut env,
        )
        .expect_err("vec is_empty should reject arguments");
    assert!(vec_is_empty_args
        .message
        .contains("`is_empty` does not take arguments"));
    let vec_clone_args = runtime
        .evaluate_vec_method(
            vec_from_env(&env, "values"),
            "clone",
            Some("values"),
            &[mir_arg(None, Operand::Int(1))],
            &mut env,
        )
        .expect_err("vec clone should reject arguments");
    assert!(vec_clone_args
        .message
        .contains("`clone` does not take arguments"));
    let vec_pop_args = runtime
        .evaluate_vec_method(
            vec_from_env(&env, "values"),
            "pop",
            Some("values"),
            &[mir_arg(None, Operand::Int(1))],
            &mut env,
        )
        .expect_err("vec pop should reject arguments");
    assert!(vec_pop_args
        .message
        .contains("`pop` does not take arguments"));
    let vec_pop_no_place = runtime
        .evaluate_vec_method(vec_from_env(&env, "values"), "pop", None, &[], &mut env)
        .expect_err("vec pop should require a receiver place");
    assert!(vec_pop_no_place
        .message
        .contains("requires a mutable vector place"));
    let vec_set_index_no_place = runtime
        .evaluate_vec_method(
            vec_from_env(&env, "values"),
            "__set_index",
            None,
            &[
                mir_arg(None, Operand::Int(0)),
                mir_arg(None, Operand::Int(7)),
                mir_arg(None, Operand::Int(1)),
                mir_arg(None, Operand::Int(1)),
            ],
            &mut env,
        )
        .expect_err("internal indexed vector assignment should require a receiver place");
    assert!(vec_set_index_no_place
        .message
        .contains("requires a mutable vector place"));
    let vec_swap_no_place = runtime
        .evaluate_vec_method(
            vec_from_env(&env, "values"),
            "swap",
            None,
            &[
                mir_arg(Some("first"), Operand::Int(0)),
                mir_arg(Some("second"), Operand::Int(1)),
            ],
            &mut env,
        )
        .expect_err("vec swap should require a receiver place");
    assert!(vec_swap_no_place
        .message
        .contains("requires a mutable vector place"));
    let vec_clear_args = runtime
        .evaluate_vec_method(
            vec_from_env(&env, "values"),
            "clear",
            Some("values"),
            &[mir_arg(None, Operand::Int(1))],
            &mut env,
        )
        .expect_err("vec clear should reject arguments");
    assert!(vec_clear_args
        .message
        .contains("`clear` does not take arguments"));
    let vec_reverse_args = runtime
        .evaluate_vec_method(
            vec_from_env(&env, "values"),
            "reverse",
            Some("values"),
            &[mir_arg(None, Operand::Int(1))],
            &mut env,
        )
        .expect_err("vec reverse should reject arguments");
    assert!(vec_reverse_args
        .message
        .contains("`reverse` does not take arguments"));
    let vec_extend_no_place = runtime
        .evaluate_vec_method(
            vec_from_env(&env, "values"),
            "extend",
            None,
            &[mir_arg(Some("other"), Operand::Place("other".to_string()))],
            &mut env,
        )
        .expect_err("vec extend should require a receiver place");
    assert!(vec_extend_no_place
        .message
        .contains("requires a mutable vector place"));

    let map_len = runtime
        .evaluate_map_method(
            map_from_env(&env, "mapping"),
            "len",
            Some("mapping"),
            &[],
            &mut env,
        )
        .expect("map len should succeed");
    assert_eq!(map_len, Value::Int(IntegerValue::from_signed(1)));

    let map_empty = runtime
        .evaluate_map_method(
            map_from_env(&env, "mapping"),
            "is_empty",
            Some("mapping"),
            &[],
            &mut env,
        )
        .expect("map is_empty should succeed");
    assert_eq!(map_empty, Value::Bool(false));

    let map_clone = runtime
        .evaluate_map_method(
            map_from_env(&env, "mapping"),
            "clone",
            Some("mapping"),
            &[],
            &mut env,
        )
        .expect("map clone should succeed");
    match map_clone {
        Value::Map(map) => assert_eq!(map.entries.len(), 1),
        other => panic!("expected map clone, found {other:?}"),
    }

    let map_get = runtime
        .evaluate_map_method(
            map_from_env(&env, "mapping"),
            "get",
            Some("mapping"),
            &[mir_arg(Some("key"), Operand::String("count".to_string()))],
            &mut env,
        )
        .expect("map get should succeed");
    assert_eq!(
        map_get,
        option_some(Value::Int(IntegerValue::from_signed(1)))
    );
    let map_get_missing = runtime
        .evaluate_map_method(
            map_from_env(&env, "mapping"),
            "get",
            Some("mapping"),
            &[mir_arg(Some("key"), Operand::String("missing".to_string()))],
            &mut env,
        )
        .expect("missing map get should return Option.None");
    assert_eq!(map_get_missing, option_none());

    let map_values = runtime
        .evaluate_map_method(
            map_from_env(&env, "mapping"),
            "values",
            Some("mapping"),
            &[],
            &mut env,
        )
        .expect("map values should succeed");
    match map_values {
        Value::Vec(values) => assert_eq!(values.elements.len(), 1),
        other => panic!("expected vec of values, found {other:?}"),
    }

    let map_keys = runtime
        .evaluate_map_method(
            map_from_env(&env, "mapping"),
            "keys",
            Some("mapping"),
            &[],
            &mut env,
        )
        .expect("map keys should succeed");
    match map_keys {
        Value::Vec(keys) => assert_eq!(keys.elements.len(), 1),
        other => panic!("expected vec of keys, found {other:?}"),
    }

    let map_contains = runtime
        .evaluate_map_method(
            map_from_env(&env, "mapping"),
            "contains_key",
            Some("mapping"),
            &[mir_arg(Some("key"), Operand::String("count".to_string()))],
            &mut env,
        )
        .expect("map contains_key should succeed");
    assert_eq!(map_contains, Value::Bool(true));
    let map_missing_contains = runtime
        .evaluate_map_method(
            map_from_env(&env, "mapping"),
            "contains_key",
            Some("mapping"),
            &[mir_arg(Some("key"), Operand::String("missing".to_string()))],
            &mut env,
        )
        .expect("map contains_key should return false for missing keys");
    assert_eq!(map_missing_contains, Value::Bool(false));
    let map_index = runtime
        .evaluate_map_method(
            map_from_env(&env, "mapping"),
            "__index",
            Some("mapping"),
            &[
                mir_arg(None, Operand::String("count".to_string())),
                mir_arg(None, Operand::Int(2)),
                mir_arg(None, Operand::Int(3)),
            ],
            &mut env,
        )
        .expect("internal map indexing should succeed for existing keys");
    assert_eq!(map_index, Value::Int(IntegerValue::from_signed(1)));

    let map_len_args = runtime
        .evaluate_map_method(
            map_from_env(&env, "mapping"),
            "len",
            Some("mapping"),
            &[mir_arg(None, Operand::Int(1))],
            &mut env,
        )
        .expect_err("map len should reject arguments");
    assert!(map_len_args
        .message
        .contains("`len` does not take arguments"));
    let map_empty_args = runtime
        .evaluate_map_method(
            map_from_env(&env, "mapping"),
            "is_empty",
            Some("mapping"),
            &[mir_arg(None, Operand::Int(1))],
            &mut env,
        )
        .expect_err("map is_empty should reject arguments");
    assert!(map_empty_args
        .message
        .contains("`is_empty` does not take arguments"));
    let map_clone_args = runtime
        .evaluate_map_method(
            map_from_env(&env, "mapping"),
            "clone",
            Some("mapping"),
            &[mir_arg(None, Operand::Int(1))],
            &mut env,
        )
        .expect_err("map clone should reject arguments");
    assert!(map_clone_args
        .message
        .contains("`clone` does not take arguments"));
    let set_len = runtime
        .evaluate_set_method(
            match env.read_place("flags").unwrap() {
                Value::Set(set) => set,
                other => panic!("expected set, found {other:?}"),
            },
            "len",
            Some("flags"),
            &[],
            &mut env,
        )
        .expect("set len should succeed");
    assert_eq!(set_len, Value::Int(IntegerValue::from_signed(1)));

    let set_empty = runtime
        .evaluate_set_method(
            match env.read_place("flags").unwrap() {
                Value::Set(set) => set,
                other => panic!("expected set, found {other:?}"),
            },
            "is_empty",
            Some("flags"),
            &[],
            &mut env,
        )
        .expect("set is_empty should succeed");
    assert_eq!(set_empty, Value::Bool(false));

    let set_clone = runtime
        .evaluate_set_method(
            match env.read_place("flags").unwrap() {
                Value::Set(set) => set,
                other => panic!("expected set, found {other:?}"),
            },
            "clone",
            Some("flags"),
            &[],
            &mut env,
        )
        .expect("set clone should succeed");
    match set_clone {
        Value::Set(set) => assert_eq!(set.elements.len(), 1),
        other => panic!("expected set clone, found {other:?}"),
    }

    let set_contains = runtime
        .evaluate_set_method(
            match env.read_place("flags").unwrap() {
                Value::Set(set) => set,
                other => panic!("expected set, found {other:?}"),
            },
            "contains",
            Some("flags"),
            &[mir_arg(Some("value"), Operand::String("ready".to_string()))],
            &mut env,
        )
        .expect("set contains should succeed");
    assert_eq!(set_contains, Value::Bool(true));

    let set_index = runtime
        .evaluate_set_method(
            match env.read_place("flags").unwrap() {
                Value::Set(set) => set,
                other => panic!("expected set, found {other:?}"),
            },
            "__index_option",
            Some("flags"),
            &[mir_arg(Some("index"), Operand::Int(0))],
            &mut env,
        )
        .expect("set __index_option should succeed");
    assert_eq!(set_index, option_some(Value::String("ready".to_string())));

    let string_len = runtime
        .evaluate_string_method("Aurora".to_string(), "len", &[], &mut env)
        .expect("string len should succeed");
    assert_eq!(string_len, Value::Int(IntegerValue::from_signed(6)));

    let starts_with = runtime
        .evaluate_string_method(
            "Aurora".to_string(),
            "starts_with",
            &[mir_arg(Some("text"), Operand::String("Aur".to_string()))],
            &mut env,
        )
        .expect("string starts_with should succeed");
    assert_eq!(starts_with, Value::Bool(true));

    let ends_with = runtime
        .evaluate_string_method(
            "Aurora".to_string(),
            "ends_with",
            &[mir_arg(Some("text"), Operand::String("ora".to_string()))],
            &mut env,
        )
        .expect("string ends_with should succeed");
    assert_eq!(ends_with, Value::Bool(true));

    let split = runtime
        .evaluate_string_method(
            "au-ro-ra".to_string(),
            "split",
            &[mir_arg(Some("text"), Operand::String("-".to_string()))],
            &mut env,
        )
        .expect("string split should succeed");
    match split {
        Value::Vec(parts) => assert_eq!(parts.elements.len(), 3),
        other => panic!("expected split vec, found {other:?}"),
    }

    let replace = runtime
        .evaluate_string_method(
            "Aurora".to_string(),
            "replace",
            &[
                mir_arg(Some("from"), Operand::String("Aur".to_string())),
                mir_arg(Some("to"), Operand::String("Our".to_string())),
            ],
            &mut env,
        )
        .expect("string replace should succeed");
    assert_eq!(replace, Value::String("Ourora".to_string()));

    let lower = runtime
        .evaluate_string_method("AuRoRa".to_string(), "to_lower", &[], &mut env)
        .expect("string to_lower should succeed");
    assert_eq!(lower, Value::String("aurora".to_string()));

    let upper = runtime
        .evaluate_string_method("AuRoRa".to_string(), "to_upper", &[], &mut env)
        .expect("string to_upper should succeed");
    assert_eq!(upper, Value::String("AURORA".to_string()));

    let suffix = runtime
        .evaluate_string_method(
            "prefix-value".to_string(),
            "strip_suffix",
            &[mir_arg(Some("text"), Operand::String("-value".to_string()))],
            &mut env,
        )
        .expect("string strip_suffix should succeed");
    match suffix {
        Value::EnumVariant(variant) => assert_eq!(variant.variant_name, "Some"),
        other => panic!("expected option result, found {other:?}"),
    }

    let trim = runtime
        .evaluate_string_method("  Aurora  ".to_string(), "trim", &[], &mut env)
        .expect("string trim should succeed");
    assert_eq!(trim, Value::String("Aurora".to_string()));

    let string_clone = runtime
        .evaluate_string_method("Aurora".to_string(), "clone", &[], &mut env)
        .expect("string clone should succeed");
    assert_eq!(string_clone, Value::String("Aurora".to_string()));

    let send = runtime
        .evaluate_channel_method(
            channel_from_env(&env, "jobs"),
            "put",
            &[mir_arg(Some("value"), Operand::Int(5))],
            &env,
        )
        .expect("queue put should succeed");
    match send {
        Value::EnumVariant(variant) => assert_eq!(variant.variant_name, "Ok"),
        other => panic!("expected Result from send, found {other:?}"),
    }

    let recv = runtime
        .evaluate_channel_method(channel_from_env(&env, "jobs"), "get", &[], &env)
        .expect("queue get should succeed");
    assert_eq!(
        recv,
        Value::EnumVariant(EnumVariantValue {
            enum_name: "QueueReceive".to_string(),
            variant_name: "Item".to_string(),
            payloads: vec![Value::Int(IntegerValue::from_signed(5))],
        })
    );

    let close = runtime
        .evaluate_channel_method(channel_from_env(&env, "jobs"), "close", &[], &env)
        .expect("channel close should succeed");
    assert_eq!(close, Value::Unit);

    let assert_send_error = |value: Value, variant_name: &str, expected_payload: i128| {
        let mut result_payloads = enum_payloads(value, "Result", "Err");
        assert_eq!(result_payloads.len(), 1);
        let mut send_payloads = enum_payloads(result_payloads.remove(0), "SendError", variant_name);
        assert_eq!(send_payloads.len(), 1);
        assert_eq!(
            send_payloads.remove(0),
            Value::Int(IntegerValue::from_signed(expected_payload))
        );
    };

    let closed_channel = ChannelValue::new();
    closed_channel.close();
    assert_send_error(
        runtime
            .evaluate_channel_method(
                closed_channel.clone(),
                "put",
                &[mir_arg(Some("value"), Operand::Int(6))],
                &env,
            )
            .expect("closed queue put should return a send error"),
        "Closed",
        6,
    );
    assert_send_error(
        runtime
            .evaluate_channel_method(
                closed_channel,
                "try_put",
                &[mir_arg(Some("value"), Operand::Int(7))],
                &env,
            )
            .expect("closed queue try_put should return a send error"),
        "Closed",
        7,
    );

    let full_channel = ChannelValue::with_capacity(0);
    assert_send_error(
        runtime
            .evaluate_channel_method(
                full_channel.clone(),
                "try_put",
                &[mir_arg(Some("value"), Operand::Int(8))],
                &env,
            )
            .expect("full queue try_put should return a send error"),
        "Full",
        8,
    );
    assert_send_error(
        runtime
            .evaluate_channel_method(
                full_channel,
                "put",
                &[
                    mir_arg(Some("value"), Operand::Int(9)),
                    mir_arg(Some("timeout"), Operand::Duration(0)),
                ],
                &env,
            )
            .expect("timed out queue put should return a send error"),
        "TimedOut",
        9,
    );

    let cancellation_group = TaskGroupValue::new(&CancellationContext::default());
    cancellation_group.cancel();
    let mut cancelled_runtime = MirRuntime::new(
        MirModule {
            functions: Vec::new(),
            classes: Vec::new(),
            trait_impls: Vec::new(),
            top_level: None,
        },
        Arc::new(Mutex::new(String::new())),
        cancellation_group.child_cancellation(),
    );
    assert_send_error(
        cancelled_runtime
            .evaluate_channel_method(
                ChannelValue::with_capacity(0),
                "put",
                &[mir_arg(Some("value"), Operand::Int(10))],
                &env,
            )
            .expect("cancelled queue put should return a send error"),
        "Cancelled",
        10,
    );

    let assert_queue_receive_variant =
        |value: Value, variant_name: &str| enum_payloads(value, "QueueReceive", variant_name);

    let empty_get_or_none = runtime
        .evaluate_channel_method(ChannelValue::new(), "get_or_none", &[], &env)
        .expect("empty get_or_none should return Option.None immediately");
    assert_eq!(empty_get_or_none, option_none());

    let queued_for_get_or_none = ChannelValue::new();
    queued_for_get_or_none
        .try_send_result(Value::Int(IntegerValue::from_signed(12)))
        .expect("queue should accept a value");
    assert_eq!(
        runtime
            .evaluate_channel_method(queued_for_get_or_none, "get_or_none", &[], &env,)
            .expect("queued get_or_none should return Option.Some"),
        option_some(Value::Int(IntegerValue::from_signed(12)))
    );

    let closed_get_or_none = ChannelValue::new();
    closed_get_or_none.close();
    assert_eq!(
        runtime
            .evaluate_channel_method(closed_get_or_none, "get_or_none", &[], &env)
            .expect("closed get_or_none should return Option.None"),
        option_none()
    );
    assert_eq!(
        cancelled_runtime
            .evaluate_channel_method(ChannelValue::new(), "get_or_none", &[], &env)
            .expect("cancelled get_or_none should return Option.None"),
        option_none()
    );

    let queued_for_get_or = ChannelValue::new();
    queued_for_get_or
        .try_send_result(Value::Int(IntegerValue::from_signed(14)))
        .expect("queue should accept a value");
    assert_eq!(
        runtime
            .evaluate_channel_method(
                queued_for_get_or,
                "get_or",
                &[mir_arg(Some("default"), Operand::Int(99))],
                &env,
            )
            .expect("queued get_or should return the queued value"),
        Value::Int(IntegerValue::from_signed(14))
    );
    assert_eq!(
        runtime
            .evaluate_channel_method(
                ChannelValue::new(),
                "get_or",
                &[mir_arg(Some("default"), Operand::Int(99))],
                &env,
            )
            .expect("empty get_or should return the fallback immediately"),
        Value::Int(IntegerValue::from_signed(99))
    );
    let closed_get_or = ChannelValue::new();
    closed_get_or.close();
    assert_eq!(
        runtime
            .evaluate_channel_method(
                closed_get_or,
                "get_or",
                &[mir_arg(Some("default"), Operand::Int(100))],
                &env,
            )
            .expect("closed get_or should return the fallback"),
        Value::Int(IntegerValue::from_signed(100))
    );
    assert_eq!(
        cancelled_runtime
            .evaluate_channel_method(
                ChannelValue::new(),
                "get_or",
                &[mir_arg(Some("default"), Operand::Int(101))],
                &env,
            )
            .expect("cancelled get_or should return the fallback"),
        Value::Int(IntegerValue::from_signed(101))
    );

    let iteration_arg_error = runtime
        .evaluate_channel_method(ChannelValue::new(), "__get_in_task_group", &[], &env)
        .expect_err("internal task-group get helper should enforce arity");
    assert!(iteration_arg_error
        .message
        .contains("expects one task-group"));

    let iteration_type_error = runtime
        .evaluate_channel_method(
            ChannelValue::new(),
            "__get_in_task_group",
            &[mir_arg(None, Operand::Int(1))],
            &env,
        )
        .expect_err("internal task-group get helper should require a task group");
    assert!(iteration_type_error
        .message
        .contains("expected `TaskGroup`"));

    let mut iteration_env = Env::default();
    iteration_env.define_typed(
        "iter_group",
        Type::named("TaskGroup"),
        Value::TaskGroup(TaskGroupValue::new(&CancellationContext::default())),
    );
    let closed_iteration_channel = ChannelValue::new();
    closed_iteration_channel.close();
    assert!(assert_queue_receive_variant(
        runtime
            .evaluate_channel_method(
                closed_iteration_channel,
                "__get_in_task_group",
                &[mir_arg(None, Operand::Place("iter_group".to_string()))],
                &iteration_env,
            )
            .expect("closed task-group iteration helper should return Closed"),
        "Closed",
    )
    .is_empty());

    let registered_arg_error = runtime
        .evaluate_channel_method(
            ChannelValue::new(),
            "__get_with_registered_producers",
            &[mir_arg(None, Operand::Int(1))],
            &env,
        )
        .expect_err("registered-producer helper should reject arguments");
    assert!(registered_arg_error
        .message
        .contains("expects no arguments"));
    let closed_registered_channel = ChannelValue::new();
    closed_registered_channel.close();
    assert!(assert_queue_receive_variant(
        runtime
            .evaluate_channel_method(
                closed_registered_channel,
                "__get_with_registered_producers",
                &[],
                &env,
            )
            .expect("closed registered-producer helper should return Closed"),
        "Closed",
    )
    .is_empty());

    let insert = runtime
        .evaluate_vec_method(
            match env.read_place("values").unwrap() {
                Value::Vec(vector) => vector,
                other => panic!("expected vec, found {other:?}"),
            },
            "insert",
            Some("values"),
            &[
                mir_arg(Some("index"), Operand::Int(1)),
                mir_arg(Some("value"), Operand::Int(99)),
            ],
            &mut env,
        )
        .expect("vec insert should succeed");
    assert_eq!(insert, Value::Bool(true));

    let reverse = runtime
        .evaluate_vec_method(
            match env.read_place("values").unwrap() {
                Value::Vec(vector) => vector,
                other => panic!("expected vec, found {other:?}"),
            },
            "reverse",
            Some("values"),
            &[],
            &mut env,
        )
        .expect("vec reverse should succeed");
    assert_eq!(reverse, Value::Unit);

    let extend = runtime
        .evaluate_vec_method(
            match env.read_place("values").unwrap() {
                Value::Vec(vector) => vector,
                other => panic!("expected vec, found {other:?}"),
            },
            "extend",
            Some("values"),
            &[mir_arg(Some("other"), Operand::Place("other".to_string()))],
            &mut env,
        )
        .expect("vec extend should succeed");
    assert_eq!(extend, Value::Unit);

    let clear = runtime
        .evaluate_vec_method(
            match env.read_place("values").unwrap() {
                Value::Vec(vector) => vector,
                other => panic!("expected vec, found {other:?}"),
            },
            "clear",
            Some("values"),
            &[],
            &mut env,
        )
        .expect("vec clear should succeed");
    assert_eq!(clear, Value::Unit);

    let vec_error = runtime
        .evaluate_vec_method(
            match env.read_place("texts").unwrap() {
                Value::Vec(vector) => vector,
                other => panic!("expected vec, found {other:?}"),
            },
            "mystery",
            Some("texts"),
            &[],
            &mut env,
        )
        .expect_err("unsupported vec method should fail");
    assert!(vec_error.message.contains("unsupported vector method"));

    let map_items = runtime
        .evaluate_map_method(
            match env.read_place("mapping").unwrap() {
                Value::Map(map) => map,
                other => panic!("expected map, found {other:?}"),
            },
            "items",
            Some("mapping"),
            &[],
            &mut env,
        )
        .expect("map items should succeed");
    match map_items {
        Value::Vec(entries) => assert_eq!(entries.elements.len(), 1),
        other => panic!("expected vec, found {other:?}"),
    }
    let map_entries = runtime
        .evaluate_map_method(
            match env.read_place("mapping").unwrap() {
                Value::Map(map) => map,
                other => panic!("expected map, found {other:?}"),
            },
            "entries",
            Some("mapping"),
            &[],
            &mut env,
        )
        .expect("map entries alias should succeed");
    match map_entries {
        Value::Vec(entries) => assert_eq!(entries.elements.len(), 1),
        other => panic!("expected vec, found {other:?}"),
    }

    let map_extend = runtime
        .evaluate_map_method(
            match env.read_place("mapping").unwrap() {
                Value::Map(map) => map,
                other => panic!("expected map, found {other:?}"),
            },
            "extend",
            Some("mapping"),
            &[mir_arg(
                Some("other"),
                Operand::Place("mapping_other".to_string()),
            )],
            &mut env,
        )
        .expect("map extend should succeed");
    assert_eq!(map_extend, Value::Unit);

    let map_set_existing = runtime
        .evaluate_map_method(
            match env.read_place("mapping").unwrap() {
                Value::Map(map) => map,
                other => panic!("expected map, found {other:?}"),
            },
            "set",
            Some("mapping"),
            &[
                mir_arg(Some("key"), Operand::String("count".to_string())),
                mir_arg(Some("value"), Operand::Int(4)),
            ],
            &mut env,
        )
        .expect("map set should replace existing keys");
    assert_eq!(
        map_set_existing,
        option_some(Value::Int(IntegerValue::from_signed(1)))
    );

    runtime
        .evaluate_map_method(
            map_from_env(&env, "mapping"),
            "__set_index",
            Some("mapping"),
            &[
                mir_arg(None, Operand::String("count".to_string())),
                mir_arg(None, Operand::Int(5)),
                mir_arg(None, Operand::Int(1)),
                mir_arg(None, Operand::Int(1)),
            ],
            &mut env,
        )
        .expect("internal map indexed assignment should update existing keys");
    let map_set_index_no_place = runtime
        .evaluate_map_method(
            map_from_env(&env, "mapping"),
            "__set_index",
            None,
            &[
                mir_arg(None, Operand::String("count".to_string())),
                mir_arg(None, Operand::Int(6)),
                mir_arg(None, Operand::Int(1)),
                mir_arg(None, Operand::Int(1)),
            ],
            &mut env,
        )
        .expect_err("internal map indexed assignment should require a receiver place");
    assert!(map_set_index_no_place
        .message
        .contains("requires a mutable map place"));
    env.define_typed(
        "mapping_update",
        Type::Named(
            "Map".to_string(),
            vec![Type::named("String"), Type::named("int32")],
        ),
        Value::Map(crate::runtime_value::MapValue {
            key_type: Type::named("String"),
            value_type: Type::named("int32"),
            entries: vec![(
                Value::String("count".to_string()),
                Value::Int(IntegerValue::from_signed(9)),
            )],
        }),
    );
    runtime
        .evaluate_map_method(
            map_from_env(&env, "mapping"),
            "extend",
            Some("mapping"),
            &[mir_arg(
                Some("other"),
                Operand::Place("mapping_update".to_string()),
            )],
            &mut env,
        )
        .expect("map extend should update existing keys");
    let map_extend_no_place = runtime
        .evaluate_map_method(
            map_from_env(&env, "mapping"),
            "extend",
            None,
            &[mir_arg(
                Some("other"),
                Operand::Place("mapping_other".to_string()),
            )],
            &mut env,
        )
        .expect_err("map extend should require a receiver place");
    assert!(map_extend_no_place
        .message
        .contains("requires a mutable map place"));
    let unsupported_map_method = runtime
        .evaluate_map_method(
            map_from_env(&env, "mapping"),
            "mystery",
            Some("mapping"),
            &[],
            &mut env,
        )
        .expect_err("unknown map methods should fail");
    assert!(unsupported_map_method
        .message
        .contains("unsupported map method `mystery`"));

    let map_set_new = runtime
        .evaluate_map_method(
            match env.read_place("mapping").unwrap() {
                Value::Map(map) => map,
                other => panic!("expected map, found {other:?}"),
            },
            "set",
            Some("mapping"),
            &[
                mir_arg(Some("key"), Operand::String("fresh".to_string())),
                mir_arg(Some("value"), Operand::Int(5)),
            ],
            &mut env,
        )
        .expect("map set should insert missing keys");
    assert_eq!(map_set_new, option_none());

    let map_remove_missing = runtime
        .evaluate_map_method(
            match env.read_place("mapping").unwrap() {
                Value::Map(map) => map,
                other => panic!("expected map, found {other:?}"),
            },
            "remove",
            Some("mapping"),
            &[mir_arg(Some("key"), Operand::String("missing".to_string()))],
            &mut env,
        )
        .expect("map remove should return Option.None for missing keys");
    assert_eq!(map_remove_missing, option_none());

    let map_remove_existing = runtime
        .evaluate_map_method(
            match env.read_place("mapping").unwrap() {
                Value::Map(map) => map,
                other => panic!("expected map, found {other:?}"),
            },
            "remove",
            Some("mapping"),
            &[mir_arg(Some("key"), Operand::String("fresh".to_string()))],
            &mut env,
        )
        .expect("map remove should return the removed value");
    assert_eq!(
        map_remove_existing,
        option_some(Value::Int(IntegerValue::from_signed(5)))
    );

    let map_set_index = runtime
        .evaluate_map_method(
            match env.read_place("mapping").unwrap() {
                Value::Map(map) => map,
                other => panic!("expected map, found {other:?}"),
            },
            "__set_index",
            Some("mapping"),
            &[
                mir_arg(None, Operand::String("indexed".to_string())),
                mir_arg(None, Operand::Int(7)),
                mir_arg(None, Operand::Int(2)),
                mir_arg(None, Operand::Int(3)),
            ],
            &mut env,
        )
        .expect("internal map indexed assignment should insert or update keys");
    assert_eq!(map_set_index, Value::Unit);

    let map_clear = runtime
        .evaluate_map_method(
            match env.read_place("mapping").unwrap() {
                Value::Map(map) => map,
                other => panic!("expected map, found {other:?}"),
            },
            "clear",
            Some("mapping"),
            &[],
            &mut env,
        )
        .expect("map clear should succeed");
    assert_eq!(map_clear, Value::Unit);

    let map_error = runtime
        .evaluate_map_method(
            match env.read_place("mapping_other").unwrap() {
                Value::Map(map) => map,
                other => panic!("expected map, found {other:?}"),
            },
            "extend",
            Some("mapping_other"),
            &[mir_arg(Some("other"), Operand::Int(7))],
            &mut env,
        )
        .expect_err("map extend should reject non-map values");
    assert!(map_error
        .message
        .contains("requires another `Map[K, V]` value"));

    let set_insert = runtime
        .evaluate_set_method(
            match env.read_place("flags").unwrap() {
                Value::Set(set) => set,
                other => panic!("expected set, found {other:?}"),
            },
            "insert",
            Some("flags"),
            &[mir_arg(Some("value"), Operand::String("go".to_string()))],
            &mut env,
        )
        .expect("set insert should succeed");
    assert_eq!(set_insert, Value::Bool(true));

    let set_remove = runtime
        .evaluate_set_method(
            match env.read_place("flags").unwrap() {
                Value::Set(set) => set,
                other => panic!("expected set, found {other:?}"),
            },
            "remove",
            Some("flags"),
            &[mir_arg(Some("value"), Operand::String("ready".to_string()))],
            &mut env,
        )
        .expect("set remove should succeed");
    assert_eq!(set_remove, Value::Bool(true));

    let set_insert_existing = runtime
        .evaluate_set_method(
            match env.read_place("flags").unwrap() {
                Value::Set(set) => set,
                other => panic!("expected set, found {other:?}"),
            },
            "insert",
            Some("flags"),
            &[mir_arg(Some("value"), Operand::String("go".to_string()))],
            &mut env,
        )
        .expect("set insert should return false for duplicate values");
    assert_eq!(set_insert_existing, Value::Bool(false));

    let set_remove_missing = runtime
        .evaluate_set_method(
            match env.read_place("flags").unwrap() {
                Value::Set(set) => set,
                other => panic!("expected set, found {other:?}"),
            },
            "remove",
            Some("flags"),
            &[mir_arg(
                Some("value"),
                Operand::String("missing".to_string()),
            )],
            &mut env,
        )
        .expect("set remove should return false for missing values");
    assert_eq!(set_remove_missing, Value::Bool(false));

    let set_error = runtime
        .evaluate_set_method(
            match env.read_place("flags").unwrap() {
                Value::Set(set) => set,
                other => panic!("expected set, found {other:?}"),
            },
            "unknown",
            Some("flags"),
            &[],
            &mut env,
        )
        .expect_err("unsupported set method should fail");
    assert!(set_error.message.contains("unsupported set method"));

    let contains = runtime
        .evaluate_string_method(
            "aurora".to_string(),
            "contains",
            &[mir_arg(Some("text"), Operand::String("ror".to_string()))],
            &mut env,
        )
        .expect("string contains should succeed");
    assert_eq!(contains, Value::Bool(true));

    let join = runtime
        .evaluate_string_method(
            ", ".to_string(),
            "join",
            &[mir_arg(Some("parts"), Operand::Place("texts".to_string()))],
            &mut env,
        )
        .expect("string join should succeed");
    assert_eq!(join, Value::String("one, two".to_string()));

    let strip = runtime
        .evaluate_string_method(
            "prefix-value".to_string(),
            "strip_prefix",
            &[mir_arg(
                Some("text"),
                Operand::String("prefix-".to_string()),
            )],
            &mut env,
        )
        .expect("string strip_prefix should succeed");
    match strip {
        Value::EnumVariant(variant) => assert_eq!(variant.variant_name, "Some"),
        other => panic!("expected option result, found {other:?}"),
    }

    let string_error = runtime
        .evaluate_string_method(
            "aurora".to_string(),
            "contains",
            &[mir_arg(Some("text"), Operand::Bool(true))],
            &mut env,
        )
        .expect_err("string contains should reject non-string args");
    assert!(string_error.message.contains("requires a `String`"));

    let join_error = runtime
        .evaluate_string_method(
            ", ".to_string(),
            "join",
            &[mir_arg(Some("parts"), Operand::Int(1))],
            &mut env,
        )
        .expect_err("string join should reject non-vectors");
    assert!(join_error.message.contains("requires `Vec[String]`"));

    env.define_typed(
        "non_string_parts",
        Type::Named("Vec".to_string(), vec![Type::named("int32")]),
        Value::Vec(crate::runtime_value::VecValue {
            element_type: Type::named("int32"),
            elements: vec![Value::Int(IntegerValue::from_signed(1))],
        }),
    );

    for (field, args, expected) in [
        (
            "len",
            vec![mir_arg(None, Operand::Int(1))],
            "`len` does not take arguments",
        ),
        (
            "starts_with",
            vec![mir_arg(Some("text"), Operand::Bool(true))],
            "`starts_with` requires a `String` argument",
        ),
        (
            "ends_with",
            vec![mir_arg(Some("text"), Operand::Bool(true))],
            "`ends_with` requires a `String` argument",
        ),
        (
            "split",
            vec![mir_arg(Some("text"), Operand::Bool(true))],
            "`split` requires a `String` argument",
        ),
        (
            "replace",
            vec![
                mir_arg(Some("from"), Operand::Bool(true)),
                mir_arg(Some("to"), Operand::String("x".to_string())),
            ],
            "`replace` requires `String` for `from`",
        ),
        (
            "replace",
            vec![
                mir_arg(Some("from"), Operand::String("a".to_string())),
                mir_arg(Some("to"), Operand::Bool(true)),
            ],
            "`replace` requires `String` for `to`",
        ),
        (
            "to_lower",
            vec![mir_arg(None, Operand::Int(1))],
            "`to_lower` does not take arguments",
        ),
        (
            "to_upper",
            vec![mir_arg(None, Operand::Int(1))],
            "`to_upper` does not take arguments",
        ),
        (
            "join",
            vec![mir_arg(
                Some("parts"),
                Operand::Place("non_string_parts".to_string()),
            )],
            "`join` requires `Vec[String]`",
        ),
        (
            "add",
            vec![mir_arg(Some("other"), Operand::Bool(true))],
            "`add` requires a `String` argument",
        ),
        (
            "strip_prefix",
            vec![mir_arg(Some("text"), Operand::Bool(true))],
            "`strip_prefix` requires a `String` argument",
        ),
        (
            "strip_suffix",
            vec![mir_arg(Some("text"), Operand::Bool(true))],
            "`strip_suffix` requires a `String` argument",
        ),
        (
            "trim",
            vec![mir_arg(None, Operand::Int(1))],
            "`trim` does not take arguments",
        ),
        (
            "clone",
            vec![mir_arg(None, Operand::Int(1))],
            "`clone` does not take arguments",
        ),
        ("missing", Vec::new(), "unsupported string method `missing`"),
    ] {
        let error = runtime
            .evaluate_string_method("aurora".to_string(), field, &args, &mut env)
            .expect_err("string helper edge should fail");
        assert!(
            error.message.contains(expected),
            "expected `{expected}` in diagnostic, got `{}`",
            error.message
        );
    }

    let task_clone = runtime
        .evaluate_task_method(
            TaskValue::from_handle(std::thread::spawn(|| Ok(Value::Unit))),
            "clone",
            &[],
            &env,
        )
        .expect_err("task clone should be unsupported");
    assert!(task_clone
        .message
        .contains("unsupported task method `clone`"));

    let task_join = runtime
        .evaluate_task_method(
            TaskValue::from_handle(std::thread::spawn(|| Ok(Value::Bool(true)))),
            "result",
            &[],
            &env,
        )
        .expect("task result should succeed");
    assert_eq!(task_join, task_result_ready(Value::Bool(true)));

    let task_error = runtime
        .evaluate_task_method(
            TaskValue::from_handle(std::thread::spawn(|| Ok(Value::Unit))),
            "cancel",
            &[],
            &env,
        )
        .expect_err("unsupported task method should fail");
    assert!(task_error.message.contains("unsupported task method"));

    let group = TaskGroupValue::new(&CancellationContext::default());
    let cancel = runtime
        .evaluate_task_group_method(group.clone(), "cancel", &[], &env)
        .expect("task-group cancel should succeed");
    assert_eq!(cancel, Value::Unit);

    let no_target = runtime
        .evaluate_task_group_method(group.clone(), "start", &[], &env)
        .expect_err("task-group start should reject empty args");
    assert!(no_target.message.contains("expects a target function"));

    let bad_target = runtime
        .evaluate_task_group_method(
            group,
            "start",
            &[mir_arg(Some("target"), Operand::Int(3))],
            &env,
        )
        .expect_err("task-group start should stay in MIR lowering");
    assert!(bad_target
        .message
        .contains("should lower to MIR `Spawn` directly"));
}

#[test]
fn mir_runtime_index_helpers_cover_error_paths() {
    let mut runtime = test_runtime();
    let mut env = Env::default();
    env.define_typed("flag", Type::named("bool"), Value::Bool(true));
    env.define_typed(
        "text",
        Type::named("String"),
        Value::String("aurora".to_string()),
    );

    let negative = runtime
        .mir_index_from_value(Value::Int(IntegerValue::from_signed(-1)))
        .expect_err("negative indices should fail");
    assert!(negative.message.contains("cannot be negative"));

    let non_integer = runtime
        .mir_index_from_value(Value::Bool(true))
        .expect_err("non-integer indices should fail");
    assert!(non_integer
        .message
        .contains("vector indices must be integers"));

    let vec_missing_place = runtime
        .evaluate_vec_method(
            crate::runtime_value::VecValue {
                element_type: Type::named("int32"),
                elements: vec![Value::Int(IntegerValue::from_signed(1))],
            },
            "clear",
            None,
            &[],
            &mut env,
        )
        .expect_err("mutable vec methods should require a receiver place");
    assert!(vec_missing_place
        .message
        .contains("requires a mutable vector place"));

    for (field, args, expected) in [
        (
            "set",
            vec![
                mir_arg(Some("index"), Operand::Int(0)),
                mir_arg(Some("value"), Operand::Int(1)),
            ],
            "`set` requires a mutable vector place",
        ),
        (
            "remove",
            vec![mir_arg(Some("index"), Operand::Int(0))],
            "`remove` requires a mutable vector place",
        ),
        (
            "swap",
            vec![
                mir_arg(Some("first"), Operand::Int(0)),
                mir_arg(Some("second"), Operand::Int(1)),
            ],
            "vector swap indices `0` and `1` are out of bounds for length `1`",
        ),
        (
            "insert",
            vec![
                mir_arg(Some("index"), Operand::Int(0)),
                mir_arg(Some("value"), Operand::Int(1)),
            ],
            "`insert` requires a mutable vector place",
        ),
        (
            "reverse",
            Vec::new(),
            "`reverse` requires a mutable vector place",
        ),
        (
            "extend",
            vec![mir_arg(Some("other"), Operand::Bool(true))],
            "`extend` requires another `Vec[T]` value",
        ),
    ] {
        let error = runtime
            .evaluate_vec_method(
                crate::runtime_value::VecValue {
                    element_type: Type::named("int32"),
                    elements: vec![Value::Int(IntegerValue::from_signed(1))],
                },
                field,
                None,
                &args,
                &mut env,
            )
            .expect_err("vector helper edge should fail");
        assert!(
            error.message.contains(expected),
            "expected `{expected}` in diagnostic, got `{}`",
            error.message
        );
    }

    let internal_index_oob = runtime
        .evaluate_vec_method(
            crate::runtime_value::VecValue {
                element_type: Type::named("int32"),
                elements: vec![Value::Int(IntegerValue::from_signed(1))],
            },
            "__index",
            Some("missing"),
            &[
                mir_arg(None, Operand::Int(5)),
                mir_arg(None, Operand::Int(9)),
                mir_arg(None, Operand::Int(2)),
            ],
            &mut env,
        )
        .expect_err("internal vector indexing should report out-of-bounds spans");
    assert!(internal_index_oob.message.contains("out of bounds"));
    assert_eq!(internal_index_oob.span, Some(crate::diag::Span::new(9, 2)));

    let internal_set_oob = runtime
        .evaluate_vec_method(
            crate::runtime_value::VecValue {
                element_type: Type::named("int32"),
                elements: vec![Value::Int(IntegerValue::from_signed(1))],
            },
            "__set_index",
            Some("missing"),
            &[
                mir_arg(None, Operand::Int(5)),
                mir_arg(None, Operand::Int(7)),
                mir_arg(None, Operand::Int(4)),
                mir_arg(None, Operand::Int(6)),
            ],
            &mut env,
        )
        .expect_err("internal indexed assignment should report out-of-bounds spans");
    assert!(internal_set_oob.message.contains("out of bounds"));
    assert_eq!(internal_set_oob.span, Some(crate::diag::Span::new(4, 6)));

    let map_value = || crate::runtime_value::MapValue {
        key_type: Type::named("String"),
        value_type: Type::named("int32"),
        entries: vec![(
            Value::String("count".to_string()),
            Value::Int(IntegerValue::from_signed(1)),
        )],
    };
    for (field, args, receiver_place, expected) in [
        (
            "__index",
            Vec::new(),
            Some("mapping"),
            "internal map indexing requires key, line, and column operands",
        ),
        (
            "__set_index",
            Vec::new(),
            Some("mapping"),
            "internal map indexed assignment requires key, value, line, and column operands",
        ),
        (
            "set",
            vec![
                mir_arg(Some("key"), Operand::String("count".to_string())),
                mir_arg(Some("value"), Operand::Int(2)),
            ],
            None,
            "`set` requires a mutable map place",
        ),
        (
            "remove",
            vec![mir_arg(Some("key"), Operand::String("count".to_string()))],
            None,
            "`remove` requires a mutable map place",
        ),
        (
            "keys",
            vec![mir_arg(None, Operand::Int(1))],
            Some("mapping"),
            "`keys` does not take arguments",
        ),
        (
            "values",
            vec![mir_arg(None, Operand::Int(1))],
            Some("mapping"),
            "`values` does not take arguments",
        ),
        (
            "items",
            vec![mir_arg(None, Operand::Int(1))],
            Some("mapping"),
            "`items` does not take arguments",
        ),
        (
            "entries",
            vec![mir_arg(None, Operand::Int(1))],
            Some("mapping"),
            "`entries` does not take arguments",
        ),
        (
            "clear",
            vec![mir_arg(None, Operand::Int(1))],
            Some("mapping"),
            "`clear` does not take arguments",
        ),
    ] {
        let error = runtime
            .evaluate_map_method(map_value(), field, receiver_place, &args, &mut env)
            .expect_err("map helper edge should fail");
        assert!(
            error.message.contains(expected),
            "expected `{expected}` in diagnostic, got `{}`",
            error.message
        );
    }

    let map_missing_place = runtime
        .evaluate_map_method(
            crate::runtime_value::MapValue {
                key_type: Type::named("String"),
                value_type: Type::named("int32"),
                entries: vec![],
            },
            "clear",
            None,
            &[],
            &mut env,
        )
        .expect_err("mutable map methods should require a receiver place");
    assert!(map_missing_place
        .message
        .contains("requires a mutable map place"));

    let set_missing_place = runtime
        .evaluate_set_method(
            crate::runtime_value::SetValue {
                element_type: Type::named("String"),
                elements: vec![],
            },
            "insert",
            None,
            &[mir_arg(Some("value"), Operand::String("go".to_string()))],
            &mut env,
        )
        .expect_err("mutable set methods should require a receiver place");
    assert!(set_missing_place
        .message
        .contains("requires a mutable set place"));

    let set_value = || crate::runtime_value::SetValue {
        element_type: Type::named("String"),
        elements: vec![Value::String("ready".to_string())],
    };
    for (field, args, receiver_place, expected) in [
        (
            "len",
            vec![mir_arg(None, Operand::Int(1))],
            Some("flags"),
            "`len` does not take arguments",
        ),
        (
            "is_empty",
            vec![mir_arg(None, Operand::Int(1))],
            Some("flags"),
            "`is_empty` does not take arguments",
        ),
        (
            "clone",
            vec![mir_arg(None, Operand::Int(1))],
            Some("flags"),
            "`clone` does not take arguments",
        ),
        (
            "remove",
            vec![mir_arg(Some("value"), Operand::String("ready".to_string()))],
            None,
            "`remove` requires a mutable set place",
        ),
        (
            "missing",
            Vec::new(),
            Some("flags"),
            "unsupported set method",
        ),
    ] {
        let error = runtime
            .evaluate_set_method(set_value(), field, receiver_place, &args, &mut env)
            .expect_err("set helper edge should fail");
        assert!(
            error.message.contains(expected),
            "expected `{expected}` in diagnostic, got `{}`",
            error.message
        );
    }
}

#[test]
fn mir_runtime_operator_and_task_helpers_cover_additional_branches() {
    let mut runtime = test_runtime();
    let span = Some(Span::new(4, 5));

    assert_eq!(
        runtime
            .eval_binary(
                crate::ast::BinaryOp::And,
                Value::Bool(true),
                Value::Bool(false),
                None,
            )
            .expect("bool and should evaluate"),
        Value::Bool(false)
    );
    assert_eq!(
        runtime
            .eval_binary(
                crate::ast::BinaryOp::Or,
                Value::Bool(false),
                Value::Bool(true),
                None,
            )
            .expect("bool or should evaluate"),
        Value::Bool(true)
    );
    let bad_and = runtime
        .eval_binary(
            crate::ast::BinaryOp::And,
            Value::Bool(true),
            Value::Int(IntegerValue::from_signed(1)),
            None,
        )
        .expect_err("non-bool logical operands should fail");
    assert!(bad_and.message.contains("must both have type `bool`"));
    let bad_or = runtime
        .eval_binary(
            crate::ast::BinaryOp::Or,
            Value::Bool(false),
            Value::Int(IntegerValue::from_signed(1)),
            None,
        )
        .expect_err("non-bool logical operands should fail");
    assert!(bad_or.message.contains("must both have type `bool`"));

    assert_eq!(
        runtime
            .eval_binary(
                crate::ast::BinaryOp::Add,
                Value::String("au".to_string()),
                Value::String("rora".to_string()),
                None,
            )
            .expect("string addition should concatenate"),
        Value::String("aurora".to_string())
    );
    let overflow = runtime
        .eval_binary(
            crate::ast::BinaryOp::Add,
            Value::Int(IntegerValue::from_literal(u128::MAX)),
            Value::Int(IntegerValue::from_signed(1)),
            span,
        )
        .expect_err("integer overflow should fail");
    assert!(overflow.message.contains("integer overflow"));
    let overflow_without_span = runtime
        .eval_binary(
            crate::ast::BinaryOp::Add,
            Value::Int(IntegerValue::from_literal(u128::MAX)),
            Value::Int(IntegerValue::from_signed(1)),
            None,
        )
        .expect_err("integer overflow without a source span should fail");
    assert!(overflow_without_span.message.contains("integer overflow"));
    let bad_add = runtime
        .eval_binary(
            crate::ast::BinaryOp::Add,
            Value::Bool(true),
            Value::String("x".to_string()),
            None,
        )
        .expect_err("unsupported add operands should fail");
    assert!(bad_add.message.contains("matching supported operand types"));

    assert_eq!(
        runtime
            .eval_binary(
                crate::ast::BinaryOp::Sub,
                Value::Int(IntegerValue::from_signed(9)),
                Value::Int(IntegerValue::from_signed(4)),
                None,
            )
            .expect("integer subtraction should evaluate"),
        Value::Int(IntegerValue::from_signed(5))
    );
    assert_eq!(
        runtime
            .eval_binary(
                crate::ast::BinaryOp::Sub,
                Value::Float(7.5),
                Value::Float(2.0),
                None,
            )
            .expect("float subtraction should evaluate"),
        Value::Float(5.5)
    );
    let sub_overflow = runtime
        .eval_binary(
            crate::ast::BinaryOp::Sub,
            Value::Int(IntegerValue::Signed(i128::MIN)),
            Value::Int(IntegerValue::from_signed(1)),
            span,
        )
        .expect_err("integer subtraction overflow should fail");
    assert!(sub_overflow.message.contains("integer overflow"));
    let sub_overflow_without_span = runtime
        .eval_binary(
            crate::ast::BinaryOp::Sub,
            Value::Int(IntegerValue::Signed(i128::MIN)),
            Value::Int(IntegerValue::from_signed(1)),
            None,
        )
        .expect_err("integer subtraction overflow without a source span should fail");
    assert!(sub_overflow_without_span
        .message
        .contains("integer overflow"));
    let bad_sub = runtime
        .eval_binary(
            crate::ast::BinaryOp::Sub,
            Value::String("x".to_string()),
            Value::String("y".to_string()),
            None,
        )
        .expect_err("string subtraction should fail");
    assert!(bad_sub.message.contains("matching numeric operands"));

    assert_eq!(
        runtime
            .eval_binary(
                crate::ast::BinaryOp::Mul,
                Value::Int(IntegerValue::from_signed(6)),
                Value::Int(IntegerValue::from_signed(7)),
                None,
            )
            .expect("integer multiplication should evaluate"),
        Value::Int(IntegerValue::from_signed(42))
    );
    assert_eq!(
        runtime
            .eval_binary(
                crate::ast::BinaryOp::Mul,
                Value::Float(2.5),
                Value::Float(4.0),
                None,
            )
            .expect("float multiplication should evaluate"),
        Value::Float(10.0)
    );
    let mul_overflow = runtime
        .eval_binary(
            crate::ast::BinaryOp::Mul,
            Value::Int(IntegerValue::from_literal(u128::MAX)),
            Value::Int(IntegerValue::from_signed(2)),
            span,
        )
        .expect_err("integer multiplication overflow should fail");
    assert!(mul_overflow.message.contains("integer overflow"));
    let mul_overflow_without_span = runtime
        .eval_binary(
            crate::ast::BinaryOp::Mul,
            Value::Int(IntegerValue::from_literal(u128::MAX)),
            Value::Int(IntegerValue::from_signed(2)),
            None,
        )
        .expect_err("integer multiplication overflow without a source span should fail");
    assert!(mul_overflow_without_span
        .message
        .contains("integer overflow"));
    let bad_mul = runtime
        .eval_binary(
            crate::ast::BinaryOp::Mul,
            Value::Bool(true),
            Value::Bool(false),
            None,
        )
        .expect_err("bool multiplication should fail");
    assert!(bad_mul.message.contains("matching numeric operands"));

    let div_zero = runtime
        .eval_binary(
            crate::ast::BinaryOp::Div,
            Value::Int(IntegerValue::from_signed(4)),
            Value::Int(IntegerValue::from_signed(0)),
            span,
        )
        .expect_err("division by zero should fail");
    assert!(div_zero.message.contains("division by zero"));
    let div_zero_without_span = runtime
        .eval_binary(
            crate::ast::BinaryOp::Div,
            Value::Int(IntegerValue::from_signed(4)),
            Value::Int(IntegerValue::from_signed(0)),
            None,
        )
        .expect_err("division by zero without a source span should fail");
    assert!(div_zero_without_span.message.contains("division by zero"));
    assert_eq!(
        runtime
            .eval_binary(
                crate::ast::BinaryOp::Div,
                Value::Int(IntegerValue::from_signed(9)),
                Value::Int(IntegerValue::from_signed(3)),
                None,
            )
            .expect("integer division should evaluate"),
        Value::Int(IntegerValue::from_signed(3))
    );
    let bad_div = runtime
        .eval_binary(
            crate::ast::BinaryOp::Div,
            Value::String("x".to_string()),
            Value::String("y".to_string()),
            None,
        )
        .expect_err("string division should fail");
    assert!(bad_div.message.contains("matching numeric operands"));
    let float_div_zero = runtime
        .eval_binary(
            crate::ast::BinaryOp::Div,
            Value::Float(7.5),
            Value::Float(0.0),
            span,
        )
        .expect_err("float division by zero should fail");
    assert!(float_div_zero.message.contains("division by zero"));
    let float_div_zero_without_span = runtime
        .eval_binary(
            crate::ast::BinaryOp::Div,
            Value::Float(7.5),
            Value::Float(0.0),
            None,
        )
        .expect_err("float division by zero without a source span should fail");
    assert!(float_div_zero_without_span
        .message
        .contains("division by zero"));
    assert_eq!(
        runtime
            .eval_binary(
                crate::ast::BinaryOp::Div,
                Value::Float(7.5),
                Value::Float(2.5),
                None,
            )
            .expect("float division should evaluate"),
        Value::Float(3.0)
    );

    let mod_zero_without_span = runtime
        .eval_binary(
            crate::ast::BinaryOp::Mod,
            Value::Int(IntegerValue::from_signed(7)),
            Value::Int(IntegerValue::from_signed(0)),
            None,
        )
        .expect_err("integer remainder by zero without a source span should fail");
    assert!(mod_zero_without_span.message.contains("division by zero"));
    assert_eq!(
        runtime
            .eval_binary(
                crate::ast::BinaryOp::Mod,
                Value::Int(IntegerValue::from_signed(7)),
                Value::Int(IntegerValue::from_signed(3)),
                None,
            )
            .expect("integer remainder should evaluate"),
        Value::Int(IntegerValue::from_signed(1))
    );
    assert_eq!(
        runtime
            .eval_binary(
                crate::ast::BinaryOp::Mod,
                Value::Float(7.5),
                Value::Float(2.0),
                None,
            )
            .expect("float remainder should evaluate"),
        Value::Float(1.5)
    );
    let bad_mod = runtime
        .eval_binary(
            crate::ast::BinaryOp::Mod,
            Value::Bool(true),
            Value::Bool(false),
            None,
        )
        .expect_err("bool remainder should fail");
    assert!(bad_mod.message.contains("matching numeric operands"));

    let float_mod_zero = runtime
        .eval_binary(
            crate::ast::BinaryOp::Mod,
            Value::Float(7.5),
            Value::Float(0.0),
            span,
        )
        .expect_err("float remainder by zero should fail");
    assert!(float_mod_zero.message.contains("division by zero"));
    let float_mod_zero_without_span = runtime
        .eval_binary(
            crate::ast::BinaryOp::Mod,
            Value::Float(7.5),
            Value::Float(0.0),
            None,
        )
        .expect_err("float remainder by zero without a source span should fail");
    assert!(float_mod_zero_without_span
        .message
        .contains("division by zero"));

    let task = TaskValue::from_handle(std::thread::spawn(|| Ok(Value::Bool(true))));
    let env = Env::default();
    let clone_error = runtime
        .evaluate_task_method(task.clone(), "clone", &[], &env)
        .expect_err("task clone should be unsupported");
    assert!(clone_error
        .message
        .contains("unsupported task method `clone`"));
    let join_args = runtime
        .evaluate_task_method(
            task.clone(),
            "result",
            &[mir_arg(None, Operand::Int(1))],
            &env,
        )
        .expect_err("result should reject arguments");
    assert!(join_args
        .message
        .contains("`result(timeout=...)` expects `Duration`, found `1`"));
    let bad_task_member = runtime
        .evaluate_task_method(task, "missing", &[], &env)
        .expect_err("unknown task members should fail");
    assert!(bad_task_member
        .message
        .contains("unsupported task method `missing`"));

    let cancellation = CancellationContext::default();
    let group = TaskGroupValue::new(&cancellation);
    let env = Env::default();
    assert_eq!(
        runtime
            .evaluate_task_group_method(group.clone(), "cancel", &[], &env)
            .expect("group cancel should succeed"),
        Value::Unit
    );
    let spawn_error = runtime
        .evaluate_task_group_method(group.clone(), "start", &[], &env)
        .expect_err("group start should reject empty arg lists");
    assert!(spawn_error.message.contains("expects a target function"));
    let bad_group_member = runtime
        .evaluate_task_group_method(group, "missing", &[], &env)
        .expect_err("unknown task-group members should fail");
    assert!(bad_group_member
        .message
        .contains("unsupported task-group method `missing`"));
}

#[test]
fn mir_runtime_task_result_or_helpers_cover_nonblocking_shortcuts() {
    let mut runtime = test_runtime();
    let env = Env::default();

    let ready_task = TaskValue::from_handle(std::thread::spawn(|| Ok(Value::Bool(true))));
    match ready_task.wait_result_with_cancellation_observed(Some(StdDuration::from_secs(1)), None) {
        crate::runtime_value::TaskWaitStatus::Ready(Ok(Value::Bool(true))) => {}
        other => panic!("expected ready bool task, got {other:?}"),
    }

    let maybe_ready = runtime
        .evaluate_task_method(ready_task.clone(), "result_or_none", &[], &env)
        .expect("completed result_or_none should use cached task result");
    assert_eq!(
        enum_payloads(maybe_ready, "Option", "Some"),
        vec![Value::Bool(true)]
    );
    assert_eq!(
        runtime
            .evaluate_task_method(
                ready_task,
                "result_or",
                &[mir_arg(None, Operand::Bool(false))],
                &env,
            )
            .expect("completed result_or should use cached task result"),
        Value::Bool(true)
    );

    let root_result = crate::runtime_value::run_lightweight_root_task(|| {
        let cancelled_task =
            crate::runtime_value::spawn_lightweight_task(|| -> crate::diag::Result<Value> {
                crate::runtime_value::cancel_current_lightweight_task_boundary()
            })?;
        match cancelled_task
            .wait_result_with_cancellation_observed(Some(StdDuration::from_secs(1)), None)
        {
            crate::runtime_value::TaskWaitStatus::Cancelled => {}
            other => panic!("expected cancelled lightweight task, got {other:?}"),
        }

        let mut runtime = test_runtime();
        let env = Env::default();
        assert_eq!(
            runtime.evaluate_task_method(cancelled_task.clone(), "result_or_none", &[], &env)?,
            option_none()
        );
        assert_eq!(
            runtime.evaluate_task_method(
                cancelled_task,
                "result_or",
                &[mir_arg(None, Operand::String("fallback".to_string()))],
                &env,
            )?,
            Value::String("fallback".to_string())
        );
        Ok(Value::Unit)
    })
    .expect("cancelled lightweight task shortcuts should evaluate");
    assert_eq!(root_result, Value::Unit);

    let cancellation = CancellationContext::default();
    let group = TaskGroupValue::new(&cancellation);
    let cancelled_runtime_context = group.child_cancellation();
    group.cancel();
    let mut cancelled_runtime = MirRuntime::new(
        MirModule {
            functions: Vec::new(),
            classes: Vec::new(),
            trait_impls: Vec::new(),
            top_level: None,
        },
        Arc::new(Mutex::new(String::new())),
        cancelled_runtime_context,
    );
    let blocker = ChannelValue::new();
    let unblocker = blocker.clone();
    let pending_task = TaskValue::from_handle(std::thread::spawn(move || {
        let _ = unblocker.recv_with_cancellation(None, None);
        Ok(Value::Unit)
    }));
    assert_eq!(
        cancelled_runtime
            .evaluate_task_method(pending_task.clone(), "result_or_none", &[], &env)
            .expect("cancelled runtimes should return Option.None"),
        option_none()
    );
    assert_eq!(
        cancelled_runtime
            .evaluate_task_method(
                pending_task.clone(),
                "result_or",
                &[mir_arg(None, Operand::String("fallback".to_string()))],
                &env,
            )
            .expect("cancelled runtimes should return the fallback"),
        Value::String("fallback".to_string())
    );
    blocker.close();
    let _ =
        pending_task.wait_result_with_cancellation_observed(Some(StdDuration::from_secs(1)), None);
}

#[test]
fn mir_runtime_wait_helpers_cover_task_lists_ready_error_timeout_and_cancel_paths() {
    let mut runtime = test_runtime();
    let ready_task = TaskValue::from_handle(std::thread::spawn(|| Ok(Value::Bool(true))));
    let task_list = Value::Vec(VecValue {
        element_type: Type::Named("Task".to_string(), vec![Type::named("bool")]),
        elements: vec![Value::Task(ready_task.clone())],
    });
    assert_eq!(
        runtime
            .expect_task_list(&task_list, "wait_any(...)")
            .expect("task vectors should decode")
            .len(),
        1
    );
    let non_vec = runtime
        .expect_task_list(&Value::Bool(true), "wait_any(...)")
        .expect_err("non-vector task lists should fail");
    assert!(non_vec.message.contains("expects `Vec[Task[T]]`"));
    let non_task_list = Value::Vec(VecValue {
        element_type: Type::named("int32"),
        elements: vec![Value::Int(IntegerValue::from_signed(1))],
    });
    let non_task = runtime
        .expect_task_list(&non_task_list, "wait_any(...)")
        .expect_err("task vectors with non-task elements should fail");
    assert!(non_task.message.contains("expects `Vec[Task[T]]`"));

    assert_eq!(
        enum_payloads(
            runtime
                .join_task(ready_task.clone(), Some(StdDuration::from_secs(1)))
                .expect("ready task should join"),
            "TaskResult",
            "Ready",
        ),
        vec![Value::Bool(true)]
    );
    assert_eq!(
        enum_payloads(
            runtime
                .wait_any(vec![ready_task.clone()], Some(StdDuration::from_secs(1)))
                .expect("ready wait_any should succeed"),
            "WaitAny",
            "Ready",
        ),
        vec![Value::Int(IntegerValue::from_signed(0)), Value::Bool(true)]
    );
    let wait_all_ready = enum_payloads(
        runtime
            .wait_all(vec![ready_task], Some(StdDuration::from_secs(1)))
            .expect("ready wait_all should succeed"),
        "WaitAll",
        "Ready",
    );
    match wait_all_ready.as_slice() {
        [Value::Vec(values)] => assert_eq!(values.elements, vec![Value::Bool(true)]),
        other => panic!("expected WaitAll.Ready vector payload, found {other:?}"),
    }

    let error_task =
        TaskValue::from_handle(std::thread::spawn(|| Err(Diagnostic::new("task failed"))));
    assert_eq!(
        enum_payloads(
            runtime
                .join_task(error_task.clone(), Some(StdDuration::from_secs(1)))
                .expect("error task should join as TaskResult.Error"),
            "TaskResult",
            "Error",
        ),
        vec![Value::String("task failed".to_string())]
    );
    assert_eq!(
        enum_payloads(
            runtime
                .wait_any(vec![error_task.clone()], Some(StdDuration::from_secs(1)))
                .expect("error wait_any should return WaitAny.Error"),
            "WaitAny",
            "Error",
        ),
        vec![
            Value::Int(IntegerValue::from_signed(0)),
            Value::String("task failed".to_string())
        ]
    );
    let wait_all_error_task =
        TaskValue::from_handle(std::thread::spawn(|| Err(Diagnostic::new("all failed"))));
    assert_eq!(
        enum_payloads(
            runtime
                .wait_all(vec![wait_all_error_task], Some(StdDuration::from_secs(1)),)
                .expect("error wait_all should return WaitAll.Error"),
            "WaitAll",
            "Error",
        ),
        vec![
            Value::Int(IntegerValue::from_signed(0)),
            Value::String("all failed".to_string())
        ]
    );

    let timeout_blocker = ChannelValue::new();
    let timeout_unblocker = timeout_blocker.clone();
    let pending_task = TaskValue::from_handle(std::thread::spawn(move || {
        let _ = timeout_unblocker.recv_with_cancellation(None, None);
        Ok(Value::Unit)
    }));
    enum_payloads(
        runtime
            .join_task(pending_task.clone(), Some(StdDuration::ZERO))
            .expect("timed out join_task should return a TaskResult"),
        "TaskResult",
        "TimedOut",
    );
    enum_payloads(
        runtime
            .wait_any(vec![pending_task.clone()], Some(StdDuration::ZERO))
            .expect("timed out wait_any should return a WaitAny value"),
        "WaitAny",
        "TimedOut",
    );
    enum_payloads(
        runtime
            .wait_all(vec![pending_task.clone()], Some(StdDuration::ZERO))
            .expect("timed out wait_all should return a WaitAll value"),
        "WaitAll",
        "TimedOut",
    );
    timeout_blocker.close();
    let _ =
        pending_task.wait_result_with_cancellation_observed(Some(StdDuration::from_secs(1)), None);

    enum_payloads(
        runtime
            .wait_any(Vec::new(), None)
            .expect("empty wait_any should time out immediately"),
        "WaitAny",
        "TimedOut",
    );

    let parent = CancellationContext::default();
    let group = TaskGroupValue::new(&parent);
    let cancelled_context = group.child_cancellation();
    group.cancel();
    let mut cancelled_runtime = MirRuntime::new(
        MirModule {
            functions: Vec::new(),
            classes: Vec::new(),
            trait_impls: Vec::new(),
            top_level: None,
        },
        Arc::new(Mutex::new(String::new())),
        cancelled_context,
    );
    enum_payloads(
        cancelled_runtime
            .wait_any(Vec::new(), None)
            .expect("empty wait_any should observe cancellation"),
        "WaitAny",
        "Cancelled",
    );

    let cancel_blocker = ChannelValue::new();
    let cancel_unblocker = cancel_blocker.clone();
    let cancelled_task = TaskValue::from_handle(std::thread::spawn(move || {
        let _ = cancel_unblocker.recv_with_cancellation(None, None);
        Ok(Value::Unit)
    }));
    enum_payloads(
        cancelled_runtime
            .join_task(cancelled_task.clone(), None)
            .expect("cancelled join_task should return a TaskResult"),
        "TaskResult",
        "Cancelled",
    );
    enum_payloads(
        cancelled_runtime
            .wait_any(vec![cancelled_task.clone()], None)
            .expect("cancelled wait_any should return a WaitAny value"),
        "WaitAny",
        "Cancelled",
    );
    enum_payloads(
        cancelled_runtime
            .wait_all(vec![cancelled_task.clone()], None)
            .expect("cancelled wait_all should return a WaitAll value"),
        "WaitAll",
        "Cancelled",
    );
    cancel_blocker.close();
    let _ = cancelled_task
        .wait_result_with_cancellation_observed(Some(StdDuration::from_secs(1)), None);
}

#[test]
fn mir_runtime_print_tolerates_poisoned_stdout_lock() {
    let stdout = Arc::new(Mutex::new(String::new()));
    let poisoned = stdout.clone();
    let _ = std::thread::spawn(move || {
        let _guard = poisoned.lock().expect("poison setup lock should succeed");
        panic!("poison stdout lock");
    })
    .join();

    let mut runtime = MirRuntime::new(
        MirModule {
            functions: Vec::new(),
            classes: Vec::new(),
            trait_impls: Vec::new(),
            top_level: None,
        },
        stdout.clone(),
        CancellationContext::default(),
    );
    let mut env = Env::default();
    let value = Value::Int(IntegerValue::from_signed(3));
    env.define_typed("value", Type::named("int32"), value.clone());

    let printed = runtime
        .evaluate_call(
            &crate::mir::CallTarget::Name("print".to_string()),
            &[mir_arg(Some("value"), Operand::Place("value".to_string()))],
            &mut env,
        )
        .expect("poisoned stdout should not panic");
    assert_eq!(printed, Value::Unit);
    assert_eq!(
        stdout
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .as_str(),
        "3\n"
    );
}

#[test]
fn mir_runtime_io_write_streams_to_stdout_sink() {
    let stdout = Arc::new(Mutex::new(String::new()));
    let streamed = Arc::new(Mutex::new(String::new()));
    let sink_output = streamed.clone();
    let sink = Arc::new(move |chunk: &str| {
        sink_output
            .lock()
            .expect("sink output should lock")
            .push_str(chunk);
    });
    let mut runtime = MirRuntime::new_with_stdout_sink(
        MirModule {
            functions: Vec::new(),
            classes: Vec::new(),
            trait_impls: Vec::new(),
            top_level: None,
        },
        stdout.clone(),
        Some(sink),
        CancellationContext::default(),
    );
    let mut env = Env::default();
    env.define_typed(
        "text",
        Type::named("String"),
        Value::String("hello".to_string()),
    );

    runtime
        .evaluate_call(
            &CallTarget::Name("io::write".to_string()),
            &[mir_arg(Some("text"), Operand::Place("text".to_string()))],
            &mut env,
        )
        .expect("io.write should succeed");

    assert_eq!(*stdout.lock().expect("stdout should lock"), "hello");
    assert_eq!(*streamed.lock().expect("sink output should lock"), "hello");
}

#[test]
fn mir_runtime_range_rejects_unsigned_endpoints_outside_signed_index_space() {
    let error = build_range(vec![EvaluatedMirArg {
        name: Some("stop".to_string()),
        value: Value::Int(IntegerValue::from_literal((i128::MAX as u128) + 1)),
        writeback_place: None,
    }])
    .expect_err("oversized unsigned range endpoints should fail");
    assert!(error
        .message
        .contains("must fit in signed index space in MIR runtime"));
    let start_error = build_range(vec![
        EvaluatedMirArg {
            name: Some("start".to_string()),
            value: Value::Int(IntegerValue::from_literal((i128::MAX as u128) + 1)),
            writeback_place: None,
        },
        EvaluatedMirArg {
            name: Some("stop".to_string()),
            value: Value::Int(IntegerValue::from_signed(1)),
            writeback_place: None,
        },
    ])
    .expect_err("oversized unsigned range starts should fail");
    assert!(start_error
        .message
        .contains("start must fit in signed index space"));
}

#[test]
fn mir_runtime_terminator_and_cleanup_helpers_cover_branch_and_error_paths() {
    let mut runtime = test_runtime();
    let mut env = Env::default();
    let mut loop_state = HashMap::new();
    let mut cleanup_stack = Vec::new();

    env.define_typed("cond", Type::named("bool"), Value::Bool(true));
    match runtime
        .execute_terminator(
            "entry",
            &Terminator::Branch {
                condition: Operand::Place("cond".to_string()),
                then_label: "then".to_string(),
                else_label: "else".to_string(),
            },
            &mut env,
            &mut loop_state,
            &mut cleanup_stack,
        )
        .expect("bool branch should succeed")
    {
        super::BlockOutcome::Goto(label) => assert_eq!(label, "then"),
        super::BlockOutcome::Return(_) => panic!("expected goto"),
    }

    env.define_typed(
        "not_bool",
        Type::named("int32"),
        Value::Int(IntegerValue::from_signed(1)),
    );
    let branch_error = match runtime.execute_terminator(
        "entry",
        &Terminator::Branch {
            condition: Operand::Place("not_bool".to_string()),
            then_label: "then".to_string(),
            else_label: "else".to_string(),
        },
        &mut env,
        &mut loop_state,
        &mut cleanup_stack,
    ) {
        Ok(_) => panic!("non-bool branches should fail"),
        Err(error) => error,
    };
    assert!(branch_error.message.contains("must evaluate to `bool`"));

    env.define_typed(
        "iter",
        Type::named("Range"),
        Value::Range(RangeValue { start: 0, end: 2 }),
    );
    env.define_typed(
        "item",
        Type::named("int32"),
        Value::Int(IntegerValue::zero()),
    );
    match runtime
        .execute_terminator(
            "loop",
            &Terminator::ForRange {
                binding: "item".to_string(),
                iterable: Operand::Place("iter".to_string()),
                body_label: "body".to_string(),
                exit_label: "exit".to_string(),
            },
            &mut env,
            &mut loop_state,
            &mut cleanup_stack,
        )
        .expect("range loop should start")
    {
        super::BlockOutcome::Goto(label) => assert_eq!(label, "body"),
        super::BlockOutcome::Return(_) => panic!("expected goto"),
    }
    assert_eq!(
        env.read_place("item")
            .expect("loop binding should be written"),
        Value::Int(IntegerValue::zero())
    );
    let _ = runtime.execute_terminator(
        "loop",
        &Terminator::ForRange {
            binding: "item".to_string(),
            iterable: Operand::Place("iter".to_string()),
            body_label: "body".to_string(),
            exit_label: "exit".to_string(),
        },
        &mut env,
        &mut loop_state,
        &mut cleanup_stack,
    );
    match runtime
        .execute_terminator(
            "loop",
            &Terminator::ForRange {
                binding: "item".to_string(),
                iterable: Operand::Place("iter".to_string()),
                body_label: "body".to_string(),
                exit_label: "exit".to_string(),
            },
            &mut env,
            &mut loop_state,
            &mut cleanup_stack,
        )
        .expect("range loop should exit")
    {
        super::BlockOutcome::Goto(label) => assert_eq!(label, "exit"),
        super::BlockOutcome::Return(_) => panic!("expected goto"),
    }

    env.define_typed(
        "bad_iter",
        Type::named("int32"),
        Value::Int(IntegerValue::zero()),
    );
    let range_error = match runtime.execute_terminator(
        "bad-loop",
        &Terminator::ForRange {
            binding: "item".to_string(),
            iterable: Operand::Place("bad_iter".to_string()),
            body_label: "body".to_string(),
            exit_label: "exit".to_string(),
        },
        &mut env,
        &mut loop_state,
        &mut cleanup_stack,
    ) {
        Ok(_) => panic!("non-range iterables should fail"),
        Err(error) => error,
    };
    assert!(range_error.message.contains("requires a `Range`"));

    env.define_typed(
        "status",
        Type::named("Status"),
        Value::EnumVariant(crate::runtime_value::EnumVariantValue {
            enum_name: "Status".to_string(),
            variant_name: "Ready".to_string(),
            payloads: Vec::new(),
        }),
    );
    match runtime
        .execute_terminator(
            "match",
            &Terminator::Match {
                scrutinee: Operand::Place("status".to_string()),
                arms: vec![
                    MirMatchArm {
                        enum_name: None,
                        variant_name: Some("Ready".to_string()),
                        label: "ready".to_string(),
                        wildcard: false,
                    },
                    MirMatchArm {
                        enum_name: None,
                        variant_name: None,
                        label: "wild".to_string(),
                        wildcard: true,
                    },
                ],
                otherwise: "other".to_string(),
            },
            &mut env,
            &mut loop_state,
            &mut cleanup_stack,
        )
        .expect("match should select a branch")
    {
        super::BlockOutcome::Goto(label) => assert_eq!(label, "ready"),
        super::BlockOutcome::Return(_) => panic!("expected goto"),
    }

    let match_error = match runtime.execute_terminator(
        "match",
        &Terminator::Match {
            scrutinee: Operand::Place("bad_iter".to_string()),
            arms: Vec::new(),
            otherwise: "other".to_string(),
        },
        &mut env,
        &mut loop_state,
        &mut cleanup_stack,
    ) {
        Ok(_) => panic!("non-enum matches should fail"),
        Err(error) => error,
    };
    assert!(match_error.message.contains("expected an enum value"));

    let unreachable = match runtime.execute_terminator(
        "dead",
        &Terminator::Unreachable,
        &mut env,
        &mut loop_state,
        &mut cleanup_stack,
    ) {
        Ok(_) => panic!("unreachable terminators should fail"),
        Err(error) => error,
    };
    assert!(unreachable
        .message
        .contains("reached unreachable MIR block"));

    let underflow = runtime
        .pop_cleanup("resource", &mut Vec::new(), &mut env, false)
        .expect_err("missing cleanup entries should fail");
    assert!(underflow.message.contains("cleanup stack underflow"));

    let mut mismatched_stack = vec!["other".to_string()];
    let mismatch = runtime
        .pop_cleanup("resource", &mut mismatched_stack, &mut env, false)
        .expect_err("mismatched cleanup entries should fail");
    assert!(mismatch.message.contains("cleanup stack mismatch"));
}

#[test]
fn mir_runtime_entrypoint_call_and_type_helpers_cover_remaining_edges() {
    assert_eq!(
        run_native_entry(
            std::ptr::null(),
            0,
            std::ptr::null(),
            0,
            std::ptr::null(),
            0,
        ),
        1
    );

    let source = "def main() -> int32:\n    return 0\n";
    let mir = crate::lower_source_to_mir(source).expect("source should lower");
    let mir_json = serde_json::to_vec(&mir).expect("mir should serialize");
    let bad_utf8 = [0xffu8];
    assert_eq!(
        run_native_entry(
            mir_json.as_ptr(),
            mir_json.len(),
            bad_utf8.as_ptr(),
            bad_utf8.len(),
            source.as_ptr(),
            source.len(),
        ),
        1
    );
    assert_eq!(
        run_native_entry(
            mir_json.as_ptr(),
            mir_json.len(),
            b"/tmp/test.au".as_ptr(),
            b"/tmp/test.au".len(),
            bad_utf8.as_ptr(),
            bad_utf8.len(),
        ),
        1
    );
    let tiny = [b'x'];
    assert_eq!(
        run_native_entry(
            tiny.as_ptr(),
            (1 << 30) + 1,
            b"/tmp/test.au".as_ptr(),
            b"/tmp/test.au".len(),
            source.as_ptr(),
            source.len(),
        ),
        1
    );

    let mut runtime = MirRuntime::new(
        MirModule {
            functions: Vec::new(),
            classes: vec![MirClass {
                name: "Pair".to_string(),
                type_params: vec!["T".to_string(), "U".to_string()],
                fields: vec![crate::mir::MirClassField {
                    name: "left".to_string(),
                    ty: Type::TypeParam("T".to_string()),
                }],
                methods: Vec::new(),
            }],
            trait_impls: Vec::new(),
            top_level: None,
        },
        Arc::new(Mutex::new(String::new())),
        CancellationContext::default(),
    );

    assert_eq!(
        runtime.infer_instance_type(&InstanceValue {
            class_name: "Pair".to_string(),
            fields: BTreeMap::from([(
                "left".to_string(),
                Value::Int(IntegerValue::from_signed(9)),
            )]),
        }),
        Some(Type::Named(
            "Pair".to_string(),
            vec![Type::named("int32"), Type::named("Unknown")],
        )),
    );
    assert_eq!(
        runtime.infer_instance_type(&InstanceValue {
            class_name: "Pair".to_string(),
            fields: BTreeMap::new(),
        }),
        None
    );
    assert_eq!(
        runtime.infer_runtime_value_type(&option_none()),
        Some(Type::Named(
            "Option".to_string(),
            vec![Type::named("Unknown")]
        )),
    );
    assert_eq!(
        runtime.infer_runtime_value_type(&result_ok(Value::Int(IntegerValue::from_signed(4)))),
        Some(Type::Named(
            "Result".to_string(),
            vec![Type::named("int32"), Type::named("Unknown")],
        )),
    );

    let mut env = Env::default();
    env.define_typed(
        "pair",
        Type::Named(
            "Pair".to_string(),
            vec![Type::named("int32"), Type::named("bool")],
        ),
        Value::Instance(InstanceValue {
            class_name: "Pair".to_string(),
            fields: BTreeMap::from([(
                "left".to_string(),
                Value::Int(IntegerValue::from_signed(1)),
            )]),
        }),
    );
    env.define_typed(
        "number",
        Type::named("int32"),
        Value::Int(IntegerValue::from_signed(2)),
    );
    assert_eq!(
        runtime.resolve_place_type("pair", &env),
        Some(Type::Named(
            "Pair".to_string(),
            vec![Type::named("int32"), Type::named("bool")]
        ))
    );
    assert_eq!(runtime.resolve_place_type("pair.left", &env), None);
    assert_eq!(runtime.resolve_place_type("number.value", &env), None);
    runtime
        .validate_value_fits_type(&Value::Bool(true), &Type::named("int32"), None)
        .expect("non-integer values are ignored by integer-width validation");
    let overflow = runtime
        .validate_value_fits_type(
            &Value::Int(IntegerValue::from_signed(999)),
            &Type::named("int8"),
            None,
        )
        .expect_err("overflowing integers should fail validation");
    assert!(overflow.message.contains("does not fit in `int8`"));
    assert_eq!(
        runtime
            .coerce_value_to_type(
                Value::Int(IntegerValue::from_signed(7)),
                &Type::named("float64"),
                None
            )
            .expect("int-to-float coercion should work"),
        Value::Float(7.0)
    );
    assert_eq!(
        runtime
            .coerce_value_to_type(Value::Float(7.0), &Type::named("int32"), None)
            .expect("float-to-int coercion should work"),
        Value::Int(IntegerValue::from_signed(7))
    );

    let missing_receiver = MirFunction {
        name: "touch".to_string(),
        module_name: "<test>".to_string(),
        span: Span::new(1, 1),
        receiver: Some(crate::mir::MirReceiverKind::Borrow),
        params: Vec::new(),
        local_types: Vec::new(),
        return_type: Type::Unit,
        entry: "entry".to_string(),
        blocks: vec![BasicBlock {
            label: "entry".to_string(),
            instructions: Vec::new(),
            terminator: Terminator::Return(Operand::Unit),
        }],
    };
    let receiver_error = match runtime.call_function(&missing_receiver, None, Vec::new()) {
        Ok(_) => panic!("receiver methods should require an explicit receiver"),
        Err(error) => error,
    };
    assert!(receiver_error.message.contains("missing its receiver"));

    let borrow_mut = MirFunction {
        name: "mutate".to_string(),
        module_name: "<test>".to_string(),
        span: Span::new(1, 1),
        receiver: Some(crate::mir::MirReceiverKind::BorrowMut),
        params: vec![MirParam {
            name: "value".to_string(),
            passing: crate::mir::MirReceiverKind::BorrowMut,
            ty: Type::named("int32"),
        }],
        local_types: vec![MirLocalType {
            name: "temp".to_string(),
            ty: Type::named("int32"),
        }],
        return_type: Type::Unit,
        entry: "entry".to_string(),
        blocks: vec![BasicBlock {
            label: "entry".to_string(),
            instructions: vec![
                Instruction::Assign {
                    target: "temp".to_string(),
                    value: Rvalue::Use(Operand::Int(4)),
                },
                Instruction::Eval {
                    value: Operand::Place("temp".to_string()),
                },
            ],
            terminator: Terminator::Return(Operand::Unit),
        }],
    };
    let group = TaskGroupValue::new(&CancellationContext::default());
    env.define_typed(
        "group",
        Type::named("TaskGroup"),
        Value::TaskGroup(group.clone()),
    );
    let outcome = runtime
        .call_function(
            &borrow_mut,
            Some(Value::Instance(InstanceValue {
                class_name: "Pair".to_string(),
                fields: BTreeMap::from([(
                    "left".to_string(),
                    Value::Int(IntegerValue::from_signed(8)),
                )]),
            })),
            vec![EvaluatedMirArg {
                name: None,
                value: Value::Int(IntegerValue::from_signed(11)),
                writeback_place: Some("value".to_string()),
            }],
        )
        .expect("borrow-mut functions should return updated writebacks");
    assert_eq!(
        outcome.updated_receiver,
        Some(Value::Instance(InstanceValue {
            class_name: "Pair".to_string(),
            fields: BTreeMap::from([(
                "left".to_string(),
                Value::Int(IntegerValue::from_signed(8)),
            )]),
        }))
    );
    assert_eq!(
        outcome.updated_params,
        vec![(0, Value::Int(IntegerValue::from_signed(11)))]
    );
    let mut cleanup_stack = Vec::new();
    runtime
        .execute_instruction(
            &Instruction::PushCleanup {
                place: "group".to_string(),
            },
            &mut env,
            &mut cleanup_stack,
        )
        .expect("push cleanup should succeed");
    assert_eq!(cleanup_stack, vec!["group".to_string()]);
    runtime
        .execute_instruction(
            &Instruction::PopCleanup {
                place: "group".to_string(),
                cancel_before_cleanup: true,
            },
            &mut env,
            &mut cleanup_stack,
        )
        .expect("pop cleanup should run the resource cleanup path");
    assert!(cleanup_stack.is_empty());

    let bad_entry = MirFunction {
        name: "broken".to_string(),
        module_name: "<test>".to_string(),
        span: Span::new(1, 1),
        receiver: None,
        params: Vec::new(),
        local_types: Vec::new(),
        return_type: Type::Unit,
        entry: "missing".to_string(),
        blocks: vec![BasicBlock {
            label: "entry".to_string(),
            instructions: Vec::new(),
            terminator: Terminator::Return(Operand::Unit),
        }],
    };
    let execute_error = runtime
        .execute_function(&bad_entry, &mut Env::default())
        .expect_err("missing block labels should fail execution");
    assert!(execute_error
        .message
        .contains("unknown MIR block `missing`"));

    let cleanup_function = MirFunction {
        name: "cleanup".to_string(),
        module_name: "<test>".to_string(),
        span: Span::new(1, 1),
        receiver: None,
        params: Vec::new(),
        local_types: Vec::new(),
        return_type: Type::Unit,
        entry: "current".to_string(),
        blocks: vec![
            BasicBlock {
                label: "inner".to_string(),
                instructions: Vec::new(),
                terminator: Terminator::ForRange {
                    binding: "i".to_string(),
                    iterable: Operand::Place("iter".to_string()),
                    body_label: "body".to_string(),
                    exit_label: "after".to_string(),
                },
            },
            BasicBlock {
                label: "current".to_string(),
                instructions: Vec::new(),
                terminator: Terminator::Goto("after".to_string()),
            },
        ],
    };
    let mut loop_state =
        HashMap::from([("inner".to_string(), 1i128), ("current".to_string(), 2i128)]);
    MirRuntime::clear_exited_for_range_states(
        &cleanup_function,
        "current",
        "after",
        &mut loop_state,
    );
    assert!(!loop_state.contains_key("inner"));
    assert!(loop_state.contains_key("current"));
}

#[test]
fn mir_runtime_entrypoint_and_env_helpers_cover_write_stream_and_place_edges() {
    let mut sink = Vec::new();
    write_stream(&mut sink, "aurora").expect("write_stream should write into Vec sinks");
    assert_eq!(
        String::from_utf8(sink).expect("sink should be UTF-8"),
        "aurora"
    );

    let source = "def main() -> int32:\n    print(1)\n    return 7\n";
    let module = crate::lower_source_to_mir(source).expect("source should lower to MIR");
    let mir_json = serde_json::to_vec(&module).expect("MIR should serialize");
    assert_eq!(
        super::run_serialized_mir_entrypoint(&mir_json, "/tmp/entry.au", source),
        7
    );
    assert_eq!(
        super::run_serialized_mir_entrypoint(b"{not json", "/tmp/entry.au", source),
        1
    );

    let env = Env::default();
    let empty_place = env
        .read_place("")
        .expect_err("empty places should be rejected");
    assert!(empty_place.message.contains("empty MIR place"));
    let missing_place = env
        .read_place("missing")
        .expect_err("missing places should be rejected");
    assert!(missing_place.message.contains("unknown MIR place"));

    let mut env = Env::default();
    env.define_typed("flag", Type::named("bool"), Value::Bool(true));
    let non_instance = env
        .read_place("flag.value")
        .expect_err("scalar field access should fail");
    assert!(non_instance
        .message
        .contains("cannot access field `value` on non-instance MIR place `flag.value`"));

    env.define_typed(
        "pair",
        Type::named("Pair"),
        Value::Instance(InstanceValue {
            class_name: "Pair".to_string(),
            fields: BTreeMap::new(),
        }),
    );
    let missing_field = env
        .read_place("pair.value")
        .expect_err("missing fields should fail");
    assert!(missing_field
        .message
        .contains("class `Pair` has no field `value` in MIR place `pair.value`"));

    let empty_write = env
        .write_place("", Value::Unit)
        .expect_err("empty roots should be rejected");
    assert!(empty_write.message.contains("empty MIR place"));
}

#[test]
fn mir_runtime_cleanup_and_rvalue_helpers_cover_remaining_error_paths() {
    let close_fn = MirFunction {
        name: "close_managed".to_string(),
        module_name: "<test>".to_string(),
        span: Span::new(1, 1),
        receiver: Some(crate::mir::MirReceiverKind::BorrowMut),
        params: Vec::new(),
        local_types: Vec::new(),
        return_type: Type::Unit,
        entry: "entry".to_string(),
        blocks: vec![BasicBlock {
            label: "entry".to_string(),
            instructions: Vec::new(),
            terminator: Terminator::Return(Operand::Unit),
        }],
    };
    let close_borrow_fn = MirFunction {
        name: "close_borrow_managed".to_string(),
        module_name: "<test>".to_string(),
        span: Span::new(1, 1),
        receiver: Some(crate::mir::MirReceiverKind::Borrow),
        params: Vec::new(),
        local_types: Vec::new(),
        return_type: Type::Unit,
        entry: "entry".to_string(),
        blocks: vec![BasicBlock {
            label: "entry".to_string(),
            instructions: Vec::new(),
            terminator: Terminator::Return(Operand::Unit),
        }],
    };
    let managed_class = MirClass {
        name: "Managed".to_string(),
        type_params: Vec::new(),
        fields: vec![crate::mir::MirClassField {
            name: "value".to_string(),
            ty: Type::named("int32"),
        }],
        methods: vec![MirMethod {
            name: "close".to_string(),
            function_name: "close_managed".to_string(),
            receiver: Some(crate::mir::MirReceiverKind::BorrowMut),
        }],
    };
    let borrow_managed_class = MirClass {
        name: "BorrowManaged".to_string(),
        type_params: Vec::new(),
        fields: Vec::new(),
        methods: vec![MirMethod {
            name: "close".to_string(),
            function_name: "close_borrow_managed".to_string(),
            receiver: Some(crate::mir::MirReceiverKind::Borrow),
        }],
    };
    let worker_class = MirClass {
        name: "Worker".to_string(),
        type_params: Vec::new(),
        fields: Vec::new(),
        methods: Vec::new(),
    };
    let broken_class = MirClass {
        name: "Broken".to_string(),
        type_params: Vec::new(),
        fields: Vec::new(),
        methods: vec![MirMethod {
            name: "close".to_string(),
            function_name: "missing_body".to_string(),
            receiver: Some(crate::mir::MirReceiverKind::BorrowMut),
        }],
    };
    let mut runtime = MirRuntime::new(
        MirModule {
            functions: vec![close_fn, close_borrow_fn],
            classes: vec![
                managed_class,
                borrow_managed_class,
                worker_class,
                broken_class,
            ],
            trait_impls: Vec::new(),
            top_level: None,
        },
        Arc::new(Mutex::new(String::new())),
        CancellationContext::default(),
    );
    let mut env = Env::default();
    env.define_typed(
        "count",
        Type::named("int32"),
        Value::Int(IntegerValue::from_signed(1)),
    );
    let non_resource = runtime
        .run_cleanup_place("count", &mut env, false)
        .expect_err("non-resource cleanup targets should fail");
    assert!(non_resource.message.contains("is not a managed resource"));

    env.define_typed(
        "completed",
        Type::named("process.Completed"),
        Value::ProcessCompleted(ProcessCompletedValue::new(
            Value::EnumVariant(EnumVariantValue {
                enum_name: "process.ExitStatus".to_string(),
                variant_name: "Exited".to_string(),
                payloads: vec![Value::Int(IntegerValue::from_signed(0))],
            }),
            Vec::new(),
            Vec::new(),
        )),
    );
    runtime
        .run_cleanup_place("completed", &mut env, false)
        .expect("completed process values should be harmless cleanup resources");

    let pipe_child = ProcessChildValue::spawn(
        vec![
            "/bin/sh".to_string(),
            "-c".to_string(),
            "printf pipe-cleanup".to_string(),
        ],
        None,
        Vec::new(),
        ProcessStdioConfig::Null,
        ProcessStdioConfig::Pipe,
        ProcessStdioConfig::Null,
        false,
    )
    .expect("process child should spawn with piped stdout");
    let stdout_pipe = pipe_child.stdout().expect("child stdout should be piped");
    pipe_child.wait(
        Some(StdDuration::from_secs(2)),
        Some(&CancellationContext::default()),
    );
    env.define_typed(
        "pipe",
        Type::named("process.Pipe"),
        Value::ProcessPipe(stdout_pipe.clone()),
    );
    runtime
        .run_cleanup_place("pipe", &mut env, false)
        .expect("process pipe cleanup should close the pipe");

    env.define_typed(
        "ghost",
        Type::named("Ghost"),
        Value::Instance(InstanceValue {
            class_name: "Ghost".to_string(),
            fields: BTreeMap::new(),
        }),
    );
    let unknown_class = runtime
        .run_cleanup_place("ghost", &mut env, false)
        .expect_err("unknown MIR classes should fail cleanup");
    assert!(unknown_class.message.contains("unknown MIR class `Ghost`"));

    env.define_typed(
        "worker",
        Type::named("Worker"),
        Value::Instance(InstanceValue {
            class_name: "Worker".to_string(),
            fields: BTreeMap::new(),
        }),
    );
    let missing_close = runtime
        .run_cleanup_place("worker", &mut env, false)
        .expect_err("classes without close should fail");
    assert!(missing_close
        .message
        .contains("cannot be used with MIR `with` because it has no `close` method"));

    env.define_typed(
        "broken",
        Type::named("Broken"),
        Value::Instance(InstanceValue {
            class_name: "Broken".to_string(),
            fields: BTreeMap::new(),
        }),
    );
    let missing_body = runtime
        .run_cleanup_place("broken", &mut env, false)
        .expect_err("missing method bodies should fail");
    assert!(missing_body
        .message
        .contains("unknown MIR method body `missing_body`"));

    env.define_typed(
        "managed",
        Type::named("Managed"),
        Value::Instance(InstanceValue {
            class_name: "Managed".to_string(),
            fields: BTreeMap::from([(
                "value".to_string(),
                Value::Int(IntegerValue::from_signed(3)),
            )]),
        }),
    );
    runtime
        .run_cleanup_place("managed", &mut env, false)
        .expect("managed resources with close methods should clean up");

    env.define_typed(
        "borrow_managed",
        Type::named("BorrowManaged"),
        Value::Instance(InstanceValue {
            class_name: "BorrowManaged".to_string(),
            fields: BTreeMap::new(),
        }),
    );
    runtime
        .run_cleanup_place("borrow_managed", &mut env, false)
        .expect("borrowed close receivers should clean up without receiver writeback");

    match runtime
        .evaluate_rvalue(
            &Rvalue::Unary {
                op: crate::ast::UnaryOp::Not,
                value: Operand::Bool(true),
                span: Span::new(1, 1),
            },
            &mut env,
        )
        .expect("MIR unary not should evaluate booleans")
    {
        super::RvalueOutcome::Value(value) => assert_eq!(value, Value::Bool(false)),
        _ => panic!("expected unary value outcome"),
    }
    match runtime
        .evaluate_rvalue(
            &Rvalue::Unary {
                op: crate::ast::UnaryOp::Neg,
                value: Operand::Int(4),
                span: Span::new(1, 1),
            },
            &mut env,
        )
        .expect("MIR unary neg should evaluate integers")
    {
        super::RvalueOutcome::Value(value) => {
            assert_eq!(value, Value::Int(IntegerValue::from_signed(-4)))
        }
        _ => panic!("expected unary value outcome"),
    }
    match runtime
        .evaluate_rvalue(
            &Rvalue::Unary {
                op: crate::ast::UnaryOp::Neg,
                value: Operand::Float(-1.5),
                span: Span::new(1, 1),
            },
            &mut env,
        )
        .expect("MIR unary neg should evaluate floats")
    {
        super::RvalueOutcome::Value(value) => assert_eq!(value, Value::Float(1.5)),
        _ => panic!("expected unary value outcome"),
    }
    let not_type = match runtime.evaluate_rvalue(
        &Rvalue::Unary {
            op: crate::ast::UnaryOp::Not,
            value: Operand::Int(1),
            span: Span::new(1, 1),
        },
        &mut env,
    ) {
        Ok(_) => panic!("MIR unary not should reject non-booleans"),
        Err(error) => error,
    };
    assert!(not_type.message.contains("`not` expects `bool`"));
    let neg_type = match runtime.evaluate_rvalue(
        &Rvalue::Unary {
            op: crate::ast::UnaryOp::Neg,
            value: Operand::String("nope".to_string()),
            span: Span::new(1, 1),
        },
        &mut env,
    ) {
        Ok(_) => panic!("MIR unary neg should reject non-numeric values"),
        Err(error) => error,
    };
    assert!(neg_type
        .message
        .contains("unary `-` expects a numeric value"));

    let try_non_result = match runtime.evaluate_rvalue(
        &Rvalue::Try {
            value: Operand::Int(1),
        },
        &mut env,
    ) {
        Ok(_) => panic!("MIR try should require Result values"),
        Err(error) => error,
    };
    assert!(try_non_result
        .message
        .contains("MIR `try` requires a `Result` value"));

    env.define_typed(
        "option_value",
        Type::Named("Option".to_string(), vec![Type::named("int32")]),
        Value::EnumVariant(EnumVariantValue {
            enum_name: "Option".to_string(),
            variant_name: "Some".to_string(),
            payloads: vec![Value::Int(IntegerValue::from_signed(1))],
        }),
    );
    let try_wrong_enum = match runtime.evaluate_rvalue(
        &Rvalue::Try {
            value: Operand::Place("option_value".to_string()),
        },
        &mut env,
    ) {
        Ok(_) => panic!("MIR try should require Result enum values"),
        Err(error) => error,
    };
    assert!(try_wrong_enum
        .message
        .contains("MIR `try` requires a `Result` value"));

    env.define_typed(
        "ok_result",
        Type::Named(
            "Result".to_string(),
            vec![Type::named("int32"), Type::named("String")],
        ),
        result_ok(Value::Int(IntegerValue::from_signed(8))),
    );
    match runtime
        .evaluate_rvalue(
            &Rvalue::Try {
                value: Operand::Place("ok_result".to_string()),
            },
            &mut env,
        )
        .expect("MIR try should unwrap Result.Ok payloads")
    {
        super::RvalueOutcome::Value(value) => {
            assert_eq!(value, Value::Int(IntegerValue::from_signed(8)))
        }
        _ => panic!("expected try value outcome"),
    }

    env.define_typed(
        "err_result",
        Type::Named(
            "Result".to_string(),
            vec![Type::named("int32"), Type::named("String")],
        ),
        result_err(Value::String("boom".to_string())),
    );
    runtime.return_type_stack.push(Type::Named(
        "Result".to_string(),
        vec![Type::named("int32"), Type::named("String")],
    ));
    match runtime
        .evaluate_rvalue(
            &Rvalue::Try {
                value: Operand::Place("err_result".to_string()),
            },
            &mut env,
        )
        .expect("MIR try should return Result.Err payloads")
    {
        super::RvalueOutcome::Return(Value::EnumVariant(variant)) => {
            assert_eq!(variant.enum_name, "Result");
            assert_eq!(variant.variant_name, "Err");
            assert_eq!(variant.payloads, vec![Value::String("boom".to_string())]);
        }
        _ => panic!("expected try return outcome"),
    }
    runtime.return_type_stack.pop();

    env.define_typed(
        "broken_result",
        Type::Named(
            "Result".to_string(),
            vec![Type::named("int32"), Type::named("String")],
        ),
        Value::EnumVariant(EnumVariantValue {
            enum_name: "Result".to_string(),
            variant_name: "Ok".to_string(),
            payloads: Vec::new(),
        }),
    );
    let invalid_payload = match runtime.evaluate_rvalue(
        &Rvalue::Try {
            value: Operand::Place("broken_result".to_string()),
        },
        &mut env,
    ) {
        Ok(_) => panic!("invalid Result payloads should fail"),
        Err(error) => error,
    };
    assert!(invalid_payload
        .message
        .contains("encountered an invalid `Result` payload"));

    let non_enum_payload = match runtime.evaluate_rvalue(
        &Rvalue::VariantPayload {
            scrutinee: Operand::Int(1),
            variant_name: "Some".to_string(),
            index: 0,
        },
        &mut env,
    ) {
        Ok(_) => panic!("variant payload extraction should require enum values"),
        Err(error) => error,
    };
    assert!(non_enum_payload.message.contains("expected an enum value"));

    env.define_typed(
        "payload_status",
        Type::named("Status"),
        Value::EnumVariant(EnumVariantValue {
            enum_name: "Status".to_string(),
            variant_name: "Ready".to_string(),
            payloads: vec![Value::String("ok".to_string())],
        }),
    );
    match runtime
        .evaluate_rvalue(
            &Rvalue::VariantPayload {
                scrutinee: Operand::Place("payload_status".to_string()),
                variant_name: "Ready".to_string(),
                index: 0,
            },
            &mut env,
        )
        .expect("variant payload extraction should return existing payloads")
    {
        super::RvalueOutcome::Value(value) => assert_eq!(value, Value::String("ok".to_string())),
        _ => panic!("expected variant payload value outcome"),
    }

    env.define_typed(
        "status",
        Type::named("Status"),
        Value::EnumVariant(EnumVariantValue {
            enum_name: "Status".to_string(),
            variant_name: "Ready".to_string(),
            payloads: Vec::new(),
        }),
    );
    let no_payload = match runtime.evaluate_rvalue(
        &Rvalue::VariantPayload {
            scrutinee: Operand::Place("status".to_string()),
            variant_name: "Ready".to_string(),
            index: 0,
        },
        &mut env,
    ) {
        Ok(_) => panic!("unit variants should reject payload extraction"),
        Err(error) => error,
    };
    assert!(no_payload.message.contains("does not carry a payload"));

    let member_on_int = match runtime.evaluate_rvalue(
        &Rvalue::Member {
            object: Operand::Int(1),
            field: "value".to_string(),
        },
        &mut env,
    ) {
        Ok(_) => panic!("member access on scalars should fail"),
        Err(error) => error,
    };
    assert!(member_on_int
        .message
        .contains("cannot access field `value` on non-instance value"));

    env.define_typed(
        "empty_instance",
        Type::named("Managed"),
        Value::Instance(InstanceValue {
            class_name: "Managed".to_string(),
            fields: BTreeMap::new(),
        }),
    );
    let missing_field = match runtime.evaluate_rvalue(
        &Rvalue::Member {
            object: Operand::Place("empty_instance".to_string()),
            field: "missing".to_string(),
        },
        &mut env,
    ) {
        Ok(_) => panic!("missing fields should fail member access"),
        Err(error) => error,
    };
    assert!(missing_field.message.contains("has no field `missing`"));
}

#[test]
fn mir_runtime_env_and_entry_helpers_cover_additional_branch_paths() {
    let mut env = Env::default();
    env.define_typed(
        "root",
        Type::named("Box"),
        Value::Instance(InstanceValue {
            class_name: "Box".to_string(),
            fields: BTreeMap::from([(
                "value".to_string(),
                Value::Int(IntegerValue::from_signed(4)),
            )]),
        }),
    );
    assert_eq!(
        env.read_place("root.value")
            .expect("nested place should read"),
        Value::Int(IntegerValue::from_signed(4))
    );
    let missing_place = env
        .write_place("missing.value", Value::Bool(true))
        .expect_err("missing MIR roots should fail");
    assert!(missing_place
        .message
        .contains("unknown MIR place `missing.value`"));
    let empty_write = env
        .write_place("", Value::Bool(true))
        .expect_err("empty MIR roots should be rejected");
    assert!(empty_write.message.contains("empty MIR place"));

    let runtime = test_runtime();
    assert_eq!(
        runtime
            .resolve_place_type("root", &env)
            .expect("root type should resolve"),
        Type::named("Box")
    );
    assert!(runtime.resolve_place_type("root.value", &env).is_none());
    assert!(runtime.resolve_place_type("missing", &env).is_none());

    let typed_runtime = MirRuntime::new(
        MirModule {
            functions: Vec::new(),
            classes: vec![MirClass {
                name: "Box".to_string(),
                type_params: Vec::new(),
                fields: vec![crate::mir::MirClassField {
                    name: "value".to_string(),
                    ty: Type::named("int32"),
                }],
                methods: Vec::new(),
            }],
            trait_impls: Vec::new(),
            top_level: None,
        },
        Arc::new(Mutex::new(String::new())),
        CancellationContext::default(),
    );
    assert_eq!(
        typed_runtime.resolve_place_type("root.value", &env),
        Some(Type::named("int32"))
    );

    let mut no_top_level = MirRuntime::new(
        MirModule {
            functions: vec![MirFunction {
                name: "main".to_string(),
                module_name: "<test>".to_string(),
                span: Span::new(1, 1),
                receiver: None,
                params: Vec::new(),
                local_types: Vec::new(),
                return_type: Type::named("int32"),
                entry: "entry".to_string(),
                blocks: vec![BasicBlock {
                    label: "entry".to_string(),
                    instructions: Vec::new(),
                    terminator: Terminator::Return(Operand::Int(9)),
                }],
            }],
            classes: vec![MirClass {
                name: "Box".to_string(),
                type_params: vec!["T".to_string()],
                fields: vec![crate::mir::MirClassField {
                    name: "value".to_string(),
                    ty: Type::TypeParam("T".to_string()),
                }],
                methods: Vec::new(),
            }],
            trait_impls: Vec::new(),
            top_level: None,
        },
        Arc::new(Mutex::new(String::new())),
        CancellationContext::default(),
    );
    assert_eq!(
        no_top_level.run_main().expect("main should execute"),
        Value::Int(IntegerValue::from_signed(9))
    );
    assert_eq!(
        MirRuntime::infer_value_type(&Value::ModuleNamespace(
            crate::runtime_value::ModuleNamespaceValue {
                path: "pkg.tools".to_string(),
            },
        )),
        None
    );

    let mut needs_receiver = MirRuntime::new(
        MirModule {
            functions: vec![MirFunction {
                name: "update".to_string(),
                module_name: "<test>".to_string(),
                span: Span::new(1, 1),
                receiver: Some(crate::mir::MirReceiverKind::BorrowMut),
                params: Vec::new(),
                local_types: Vec::new(),
                return_type: Type::Unit,
                entry: "entry".to_string(),
                blocks: vec![BasicBlock {
                    label: "entry".to_string(),
                    instructions: Vec::new(),
                    terminator: Terminator::Return(Operand::Unit),
                }],
            }],
            classes: Vec::new(),
            trait_impls: Vec::new(),
            top_level: None,
        },
        Arc::new(Mutex::new(String::new())),
        CancellationContext::default(),
    );
    let update = needs_receiver
        .functions
        .get("update")
        .cloned()
        .expect("update function should exist");
    let receiver_error = match needs_receiver.call_function(&update, None, Vec::new()) {
        Ok(_) => panic!("missing MIR receivers should fail"),
        Err(error) => error,
    };
    assert!(receiver_error.message.contains("missing its receiver"));

    let panic_code = run_native_entry(
        std::ptr::null(),
        0,
        std::ptr::null(),
        0,
        std::ptr::null(),
        0,
    );
    assert_eq!(panic_code, 1);

    let missing_main_runtime = MirRuntime::new(
        MirModule {
            functions: Vec::new(),
            classes: Vec::new(),
            trait_impls: Vec::new(),
            top_level: Some(MirFunction {
                name: "<top-level>".to_string(),
                module_name: "<test>".to_string(),
                span: Span::new(1, 1),
                receiver: None,
                params: Vec::new(),
                local_types: Vec::new(),
                return_type: Type::named("int32"),
                entry: "entry".to_string(),
                blocks: vec![BasicBlock {
                    label: "entry".to_string(),
                    instructions: Vec::new(),
                    terminator: Terminator::Return(Operand::Int(0)),
                }],
            }),
        },
        Arc::new(Mutex::new(String::new())),
        CancellationContext::default(),
    );
    assert_eq!(
        missing_main_runtime.resolve_place_type("missing", &Env::default()),
        None
    );
}
