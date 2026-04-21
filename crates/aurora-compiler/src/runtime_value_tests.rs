use super::{
    cast_numeric_value, create_dir_once, io_decode_utf8, io_error, lock_mutex, option_none,
    option_some, remove_file_checked, render_float, result_err, result_ok, run_blocking_io,
    run_lightweight_root_task, send_error_cancelled, send_error_closed,
    sleep_with_runtime_scheduler, spawn_lightweight_task, wait_for_runtime_scheduler,
    CancellationContext, ChannelValue, EnumVariantValue, FileValue, HttpListenerValue,
    HttpResponseValue, MapValue, ProcessRestartPolicy, ProcessStdioConfig, ProcessSupervisorValue,
    RangeValue, SetValue, TaskGroupValue, TaskValue, TcpListenerValue, TcpStreamValue,
    TryRecvResult, UdpSocketValue, Value, VecValue, WebSocketListenerValue,
};
use crate::diag::{Diagnostic, Span};
use crate::integer::IntegerValue;
use crate::sema::Type;
use rcgen::generate_simple_self_signed;
use std::fs;
use std::io::{self, Read, Write};
use std::path::PathBuf;
use std::thread;
use std::time::{Duration as StdDuration, Instant, SystemTime, UNIX_EPOCH};

#[cfg(unix)]
use super::{TlsListenerValue, TlsStreamValue, UnixListenerValue, UnixStreamValue};
#[cfg(unix)]
use std::os::fd::AsRawFd;

struct TempDir {
    path: PathBuf,
}

impl TempDir {
    fn new(prefix: &str) -> Self {
        let unique = format!(
            "{}-{}-{}",
            prefix,
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .expect("system time should be after unix epoch")
                .as_nanos()
        );
        let path = std::env::temp_dir().join(unique);
        fs::create_dir_all(&path).expect("failed to create temp dir");
        Self { path }
    }

    fn path(&self) -> &PathBuf {
        &self.path
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.path);
    }
}

#[cfg(unix)]
fn fd_is_nonblocking(fd: i32) -> bool {
    let flags = unsafe { libc::fcntl(fd, libc::F_GETFL) };
    assert!(flags >= 0, "fcntl(F_GETFL) should succeed");
    flags & libc::O_NONBLOCK != 0
}

#[test]
fn render_float_formats_current_surface() {
    assert_eq!(render_float(42.0), "42.0");
    assert_eq!(render_float(3.5), "3.5");
    assert_eq!(render_float(f64::INFINITY), "inf");

    let float32_value = (3.14f32) as f64;
    assert_eq!(render_float(float32_value), "3.14");
}

#[test]
fn option_and_result_helpers_render_expected_variants() {
    assert_eq!(
        option_some(Value::Int(IntegerValue::from_signed(7))).render(),
        "Option.Some(7)"
    );
    assert_eq!(option_none().render(), "Option.None");
    assert_eq!(result_ok(Value::Bool(true)).render(), "Result.Ok(true)");
    assert_eq!(
        result_err(Value::String("oops".to_string())).render(),
        "Result.Err(oops)"
    );
    assert_eq!(
        send_error_closed(Value::Int(IntegerValue::from_signed(3))).render(),
        "SendError.Closed(3)"
    );
    assert_eq!(
        send_error_cancelled(Value::Int(IntegerValue::from_signed(4))).render(),
        "SendError.Cancelled(4)"
    );
}

#[test]
fn cast_numeric_value_covers_success_and_failure_paths() {
    assert_eq!(
        cast_numeric_value(
            Value::Int(IntegerValue::from_signed(5)),
            &Type::named("float64"),
            None
        )
        .expect("int to float cast should succeed"),
        Value::Float(5.0)
    );

    assert_eq!(
        cast_numeric_value(Value::Float(3.0), &Type::named("int32"), None)
            .expect("float to int cast should succeed"),
        Value::Int(IntegerValue::from_signed(3))
    );

    let overflow = cast_numeric_value(
        Value::Float(500.0),
        &Type::named("int8"),
        Some(Span::new(4, 9)),
    )
    .expect_err("narrow integer overflow should fail");
    assert!(overflow.message.contains("does not fit in `int8`"));

    let non_numeric = cast_numeric_value(
        Value::String("Aurora".to_string()),
        &Type::named("int32"),
        Some(Span::new(2, 3)),
    )
    .expect_err("non-numeric casts should fail");
    assert!(non_numeric
        .message
        .contains("casts are only supported between numeric types"));

    let float64_precision = cast_numeric_value(
        Value::Int(IntegerValue::from_literal((1u128 << 53) + 1)),
        &Type::named("float64"),
        Some(Span::new(6, 7)),
    )
    .expect_err("precision-losing int64 to float64 casts should fail");
    assert!(float64_precision
        .message
        .contains("cannot be represented exactly as `float64`"));

    let float32_precision = cast_numeric_value(
        Value::Int(IntegerValue::from_literal((1u128 << 24) + 1)),
        &Type::named("float32"),
        Some(Span::new(8, 9)),
    )
    .expect_err("precision-losing int to float32 casts should fail");
    assert!(float32_precision
        .message
        .contains("cannot be represented exactly as `float32`"));
}

#[test]
fn channel_runtime_helpers_cover_send_receive_and_close_paths() {
    let channel = ChannelValue::new();
    assert_eq!(channel.try_recv(), TryRecvResult::Empty);

    channel
        .send(Value::Int(IntegerValue::from_signed(5)))
        .expect("send should succeed on open channel");
    assert_eq!(
        channel.try_recv(),
        TryRecvResult::Value(Value::Int(IntegerValue::from_signed(5)))
    );

    channel.close();
    assert_eq!(channel.try_recv(), TryRecvResult::Closed);
    assert_eq!(
        channel
            .send(Value::Bool(true))
            .expect_err("closed channel should reject sends"),
        Value::Bool(true)
    );
    assert!(channel.recv_with_cancellation(None, None).is_none());
}

#[test]
fn bounded_channel_waits_for_capacity_before_accepting_another_value() {
    let channel = ChannelValue::with_capacity(1);
    channel
        .send(Value::Int(IntegerValue::from_signed(1)))
        .expect("first bounded send should succeed");

    let delayed_recv = channel.clone();
    let worker = thread::spawn(move || {
        thread::sleep(StdDuration::from_millis(80));
        delayed_recv.try_recv()
    });

    let start = Instant::now();
    channel
        .send(Value::Int(IntegerValue::from_signed(2)))
        .expect("second send should succeed after capacity frees");
    let elapsed = start.elapsed();
    let received = worker
        .join()
        .expect("bounded channel worker should join successfully");

    assert_eq!(
        received,
        TryRecvResult::Value(Value::Int(IntegerValue::from_signed(1)))
    );
    assert!(
        elapsed >= StdDuration::from_millis(60),
        "bounded send should wait for free capacity; elapsed {:?}",
        elapsed
    );
    assert_eq!(
        channel.try_recv(),
        TryRecvResult::Value(Value::Int(IntegerValue::from_signed(2)))
    );
}

#[test]
fn task_and_cancellation_helpers_cover_current_runtime_contract() {
    let task = TaskValue::from_handle(thread::spawn(|| {
        Ok(Value::Int(IntegerValue::from_signed(9)))
    }));
    assert_eq!(
        match task.join_result() {
            super::TaskExecutionResult::Ready(result) => result.expect("first join should succeed"),
            super::TaskExecutionResult::Cancelled => panic!("task should not be cancelled"),
        },
        Value::Int(IntegerValue::from_signed(9))
    );
    assert_eq!(
        match task.join_result() {
            super::TaskExecutionResult::Ready(result) => {
                result.expect("cached join should also succeed")
            }
            super::TaskExecutionResult::Cancelled => panic!("task should not be cancelled"),
        },
        Value::Int(IntegerValue::from_signed(9))
    );

    let cancellation = CancellationContext::default();
    assert!(!cancellation.is_cancelled());
    let group = TaskGroupValue::new(&cancellation);
    let registered = TaskValue::from_handle(thread::spawn(|| Ok(Value::Unit)));
    group.register_task(registered.clone());
    assert_eq!(group.drain_tasks(), vec![registered]);
    group.cancel();
    assert!(group.child_cancellation().is_cancelled());
}

#[test]
fn channel_and_task_helpers_tolerate_poisoned_locks() {
    let channel = ChannelValue::new();
    let poisoned_channel = channel.clone();
    let _ = thread::spawn(move || {
        let _guard = poisoned_channel
            .inner
            .state
            .lock()
            .expect("poison setup lock");
        panic!("poison channel lock");
    })
    .join();
    channel
        .send(Value::Int(IntegerValue::from_signed(11)))
        .expect("poisoned channel lock should recover");
    assert_eq!(
        channel.try_recv(),
        TryRecvResult::Value(Value::Int(IntegerValue::from_signed(11)))
    );
    channel.close();
    assert_eq!(channel.recv_with_cancellation(None, None), None);

    let cancellation = CancellationContext::default();
    let group = TaskGroupValue::new(&cancellation);
    let poisoned_group = group.clone();
    let _ = thread::spawn(move || {
        let _guard = poisoned_group
            .inner
            .tasks
            .lock()
            .expect("poison setup lock");
        panic!("poison task-group lock");
    })
    .join();
    let registered = TaskValue::from_handle(thread::spawn(|| Ok(Value::Unit)));
    group.register_task(registered.clone());
    assert_eq!(group.drain_tasks(), vec![registered]);

    let task = TaskValue::from_handle(thread::spawn(|| {
        Ok(Value::Int(IntegerValue::from_signed(17)))
    }));
    let poisoned_task = task.clone();
    let _ = thread::spawn(move || {
        let _guard = poisoned_task
            .inner
            .handle
            .lock()
            .expect("poison setup lock");
        panic!("poison task lock");
    })
    .join();
    assert_eq!(
        match task.join_result() {
            super::TaskExecutionResult::Ready(result) => {
                result.expect("poisoned task handle lock should recover")
            }
            super::TaskExecutionResult::Cancelled => panic!("task should not be cancelled"),
        },
        Value::Int(IntegerValue::from_signed(17))
    );
}

#[test]
fn runtime_scheduler_wakes_sleep_on_cancellation() {
    let parent = CancellationContext::default();
    let group = TaskGroupValue::new(&parent);
    let cancellation = group.child_cancellation();
    let start = Instant::now();
    let worker = thread::spawn(move || {
        sleep_with_runtime_scheduler(StdDuration::from_millis(250), Some(&cancellation))
    });

    thread::sleep(StdDuration::from_millis(20));
    group.cancel();
    assert_eq!(
        worker.join().expect("scheduler sleep worker should join"),
        super::RuntimeSchedulerWakeReason::Cancelled
    );
    assert!(
        start.elapsed() < StdDuration::from_millis(100),
        "scheduler sleep should wake promptly when cancelled; elapsed {:?}",
        start.elapsed()
    );
}

#[test]
fn runtime_scheduler_wakes_select_wait_on_cancellation() {
    let parent = CancellationContext::default();
    let group = TaskGroupValue::new(&parent);
    let cancellation = group.child_cancellation();
    let channel = ChannelValue::new();
    let start = Instant::now();
    let worker = thread::spawn(move || {
        let deadline = Instant::now() + StdDuration::from_millis(250);
        let _ = wait_for_runtime_scheduler(
            vec![channel],
            true,
            Vec::new(),
            Vec::new(),
            Some(deadline),
            Some(&cancellation),
        );
    });

    thread::sleep(StdDuration::from_millis(20));
    group.cancel();
    worker.join().expect("scheduler wait worker should join");
    assert!(
        start.elapsed() < StdDuration::from_millis(100),
        "scheduler wait should wake promptly when cancelled; elapsed {:?}",
        start.elapsed()
    );
}

#[test]
fn value_equality_and_render_cover_collection_shapes() {
    let vec_value = Value::Vec(VecValue {
        element_type: Type::named("int32"),
        elements: vec![
            Value::Int(IntegerValue::from_signed(1)),
            Value::Int(IntegerValue::from_signed(2)),
        ],
    });
    assert_eq!(vec_value.render(), "[1, 2]");

    let set_a = Value::Set(SetValue {
        element_type: Type::named("int32"),
        elements: vec![
            Value::Int(IntegerValue::from_signed(1)),
            Value::Int(IntegerValue::from_signed(2)),
        ],
    });
    let set_b = Value::Set(SetValue {
        element_type: Type::named("int32"),
        elements: vec![
            Value::Int(IntegerValue::from_signed(2)),
            Value::Int(IntegerValue::from_signed(1)),
        ],
    });
    assert_eq!(set_a, set_b);

    let map_a = Value::Map(MapValue {
        key_type: Type::named("String"),
        value_type: Type::named("int32"),
        entries: vec![
            (
                Value::String("a".to_string()),
                Value::Int(IntegerValue::from_signed(1)),
            ),
            (
                Value::String("b".to_string()),
                Value::Int(IntegerValue::from_signed(2)),
            ),
        ],
    });
    let map_b = Value::Map(MapValue {
        key_type: Type::named("String"),
        value_type: Type::named("int32"),
        entries: vec![
            (
                Value::String("b".to_string()),
                Value::Int(IntegerValue::from_signed(2)),
            ),
            (
                Value::String("a".to_string()),
                Value::Int(IntegerValue::from_signed(1)),
            ),
        ],
    });
    assert_eq!(map_a, map_b);

    assert_eq!(
        Value::EnumVariant(EnumVariantValue {
            enum_name: "Status".to_string(),
            variant_name: "Done".to_string(),
            payloads: vec![Value::Int(IntegerValue::from_signed(2))],
        })
        .render(),
        "Status.Done(2)"
    );
    assert_eq!(
        Value::Range(RangeValue { start: 1, end: 4 }).render(),
        "range(1, 4)"
    );
}

#[test]
fn file_and_encoding_helpers_cover_binary_roundtrip_surface() {
    let temp = TempDir::new("aurora-runtime-bytes");
    let path = temp.path().join("data.bin");
    let encoded = b"aurora".to_vec();

    let file = FileValue::create(path.to_str().expect("temp path should be valid UTF-8"))
        .expect("file create should succeed");
    file.write_bytes(&encoded)
        .expect("write_bytes should succeed");
    file.flush().expect("flush should succeed");
    file.close();

    let reopened = FileValue::open(path.to_str().expect("temp path should be valid UTF-8"))
        .expect("file open should succeed");
    let read_back = reopened.read_bytes().expect("read_bytes should succeed");
    assert_eq!(read_back, encoded);
    assert_eq!(
        io_decode_utf8(&read_back).expect("decode_utf8 should succeed"),
        "aurora"
    );
}

#[test]
fn filesystem_helpers_surface_directory_conflicts_precisely() {
    let temp = TempDir::new("aurora-fs-errors");
    let dir = temp.path().join("data");
    fs::create_dir(&dir).expect("directory should be created");

    let already_exists = create_dir_once(&dir).expect_err("existing directory should fail");
    assert_eq!(already_exists.kind(), std::io::ErrorKind::AlreadyExists);

    let is_directory = remove_file_checked(&dir).expect_err("directory removal should fail");
    assert_eq!(is_directory.kind(), std::io::ErrorKind::IsADirectory);

    let rendered = io_error(is_directory);
    let Value::EnumVariant(variant) = rendered else {
        panic!("io_error should return an enum variant");
    };
    assert_eq!(variant.variant_name, "IsDirectory");
}

#[test]
fn io_error_maps_closed_and_cancelled_resource_conditions() {
    let Value::EnumVariant(closed) = io_error(super::closed_resource_error()) else {
        panic!("closed resource errors should render as io.Error variants");
    };
    assert_eq!(closed.variant_name, "Closed");

    let Value::EnumVariant(cancelled) = io_error(super::cancelled_resource_error()) else {
        panic!("cancelled resource errors should render as io.Error variants");
    };
    assert_eq!(cancelled.variant_name, "Cancelled");
}

#[cfg(unix)]
#[test]
fn unix_listener_bind_rejects_existing_regular_files() {
    let temp = TempDir::new("aurora-runtime-unix-bind");
    let path = temp.path().join("existing.txt");
    fs::write(&path, "important-user-data").expect("write regular file");

    let error = UnixListenerValue::bind(path.to_str().expect("path should be valid UTF-8"))
        .expect_err("binding over a regular file should fail");
    assert!(
        matches!(
            error.kind(),
            std::io::ErrorKind::AlreadyExists
                | std::io::ErrorKind::InvalidInput
                | std::io::ErrorKind::PermissionDenied
        ),
        "unexpected unix bind error kind: {:?}",
        error.kind()
    );
    assert!(
        path.is_file(),
        "failed unix bind should leave the original regular file intact"
    );
}

#[cfg(unix)]
#[test]
fn unix_listener_bind_rejects_existing_live_socket_paths() {
    let path = PathBuf::from(format!("/tmp/aurora-live-{}.sock", std::process::id()));
    let _ = fs::remove_file(&path);
    let listener = UnixListenerValue::bind(path.to_str().expect("valid unix socket path"))
        .expect("first unix listener bind should succeed");

    let error = UnixListenerValue::bind(path.to_str().expect("valid unix socket path"))
        .expect_err("binding over a live unix socket should fail");
    assert_eq!(error.kind(), std::io::ErrorKind::AlreadyExists);

    listener.close();
    let _ = fs::remove_file(&path);
}

#[test]
fn supervisor_rejects_zero_backoff_when_restart_is_enabled() {
    let supervisor = ProcessSupervisorValue::new();
    let error = supervisor
        .start(
            "flaky".to_string(),
            vec!["/usr/bin/false".to_string()],
            None,
            Vec::new(),
            ProcessStdioConfig::Null,
            ProcessStdioConfig::Null,
            ProcessStdioConfig::Null,
            ProcessRestartPolicy::Always,
            StdDuration::ZERO,
            Some(1),
            true,
        )
        .expect_err("zero-backoff restart loops should be rejected");
    let Value::EnumVariant(variant) = error else {
        panic!("process supervisor start should return a process.Error variant");
    };
    assert_eq!(variant.enum_name, "Error");
    assert_eq!(variant.variant_name, "Io");
    assert_eq!(variant.payloads.len(), 1);
    assert_eq!(variant.payloads[0].render(), "io.Error.InvalidInput");
}

#[test]
fn tcp_udp_http_and_websocket_helpers_cover_timeout_and_protocol_surface() {
    let short_timeout = StdDuration::from_secs(5);
    let cancellation = CancellationContext::default();
    let listener = TcpListenerValue::bind("127.0.0.1:0").expect("tcp bind should succeed");
    let address = listener
        .local_addr()
        .expect("listener local addr should succeed");
    let server = listener.clone();
    let server_thread = thread::spawn(move || {
        let stream = server
            .accept(Some(short_timeout), Some(&CancellationContext::default()))
            .expect("tcp accept should succeed");
        let line = stream
            .read_line(Some(short_timeout), Some(&CancellationContext::default()))
            .expect("tcp read_line should succeed");
        assert_eq!(line.as_deref(), Some("ping"));
        stream
            .write_bytes(
                b"pong",
                Some(short_timeout),
                Some(&CancellationContext::default()),
            )
            .expect("tcp write_bytes should succeed");
        stream.close();
    });

    let client = TcpStreamValue::connect(&address, Some(short_timeout), Some(&cancellation))
        .expect("tcp connect should succeed");
    client
        .write_all("ping\n", Some(short_timeout), Some(&cancellation))
        .expect("tcp write_all should succeed");
    let bytes = client
        .read_exact(4, Some(short_timeout), Some(&cancellation))
        .expect("tcp read_exact should succeed");
    assert_eq!(bytes, b"pong");
    server_thread.join().expect("tcp server thread should join");

    let udp_server = UdpSocketValue::bind("127.0.0.1:0").expect("udp bind should succeed");
    let udp_address = udp_server
        .local_addr()
        .expect("udp local addr should succeed");
    let udp_thread = {
        let server = udp_server.clone();
        thread::spawn(move || {
            let datagram = server
                .recv_from(
                    64,
                    Some(short_timeout),
                    Some(&CancellationContext::default()),
                )
                .expect("udp recv_from should succeed")
                .expect("udp recv_from should return a datagram");
            assert_eq!(
                datagram.text().expect("udp datagram text should decode"),
                "ping"
            );
            server
                .send_to_bytes(
                    &datagram.address(),
                    b"pong",
                    Some(short_timeout),
                    Some(&CancellationContext::default()),
                )
                .expect("udp send_to_bytes should succeed");
        })
    };
    let udp_client = UdpSocketValue::bind("127.0.0.1:0").expect("udp client bind should succeed");
    udp_client
        .send_to_text(
            &udp_address,
            "ping",
            Some(short_timeout),
            Some(&cancellation),
        )
        .expect("udp send_to_text should succeed");
    let reply = udp_client
        .recv(64, Some(short_timeout), Some(&cancellation))
        .expect("udp recv should succeed")
        .expect("udp recv should return data");
    assert_eq!(reply, b"pong");
    udp_thread.join().expect("udp thread should join");

    let http_listener =
        HttpListenerValue::bind("127.0.0.1:0").expect("http listener bind should succeed");
    let http_address = http_listener
        .local_addr()
        .expect("http listener local addr should succeed");
    let http_thread = {
        let listener = http_listener.clone();
        thread::spawn(move || {
            let exchange = listener
                .accept(Some(short_timeout), Some(&CancellationContext::default()))
                .expect("http accept should succeed");
            assert_eq!(exchange.method(), "POST");
            assert_eq!(exchange.path(), "/echo");
            assert_eq!(
                exchange.body_text().expect("http body text should decode"),
                "aurora"
            );
            exchange
                .respond_text(
                    200,
                    "ok",
                    vec![("content-type".to_string(), "text/plain".to_string())],
                )
                .expect("http respond should succeed");
        })
    };
    let response = HttpResponseValue::request_text(
        "POST",
        &format!("http://{}/echo", http_address),
        "aurora",
        vec![("x-test".to_string(), "1".to_string())],
        Some(short_timeout),
        Some(&cancellation),
    )
    .expect("http request should succeed");
    assert_eq!(response.status(), 200);
    assert_eq!(
        response.text().expect("http response text should decode"),
        "ok"
    );
    http_thread.join().expect("http thread should join");

    let ws_listener =
        WebSocketListenerValue::bind("127.0.0.1:0").expect("websocket bind should succeed");
    let ws_address = ws_listener
        .local_addr()
        .expect("websocket listener local addr should succeed");
    let ws_thread = {
        let listener = ws_listener.clone();
        thread::spawn(move || {
            let socket = listener
                .accept(Some(short_timeout))
                .expect("websocket accept should succeed");
            let text = socket
                .recv_text(Some(short_timeout))
                .expect("websocket recv_text should succeed")
                .expect("websocket text message should be present");
            assert_eq!(text, "hello");
            socket
                .send_bytes(b"pong", Some(short_timeout))
                .expect("websocket send_bytes should succeed");
            socket.close().expect("websocket close should succeed");
        })
    };
    let ws_client =
        super::WebSocketValue::connect(&format!("ws://{}", ws_address), Some(short_timeout))
            .expect("websocket connect should succeed");
    ws_client
        .send_text("hello", Some(short_timeout))
        .expect("websocket send_text should succeed");
    let ws_reply = ws_client
        .recv_bytes(Some(short_timeout))
        .expect("websocket recv_bytes should succeed")
        .expect("websocket bytes should be present");
    assert_eq!(ws_reply, b"pong");
    ws_thread.join().expect("websocket thread should join");
}

#[test]
fn tcp_and_http_helpers_handle_large_payloads() {
    let timeout = StdDuration::from_secs(5);
    let cancellation = CancellationContext::default();
    let payload = vec![b'x'; 350_000];

    let listener = TcpListenerValue::bind("127.0.0.1:0").expect("tcp bind should succeed");
    let address = listener
        .local_addr()
        .expect("listener local addr should succeed");
    let server = listener.clone();
    let expected_len = payload.len();
    let server_thread = thread::spawn(move || {
        let stream = server
            .accept(Some(timeout), Some(&CancellationContext::default()))
            .expect("tcp accept should succeed");
        let bytes = stream
            .read_exact(
                expected_len,
                Some(timeout),
                Some(&CancellationContext::default()),
            )
            .expect("tcp read_exact should succeed");
        assert_eq!(bytes.len(), expected_len);
        stream.close();
    });

    let client = TcpStreamValue::connect(&address, Some(timeout), Some(&cancellation))
        .expect("tcp connect should succeed");
    client
        .write_bytes(&payload, Some(timeout), Some(&cancellation))
        .expect("tcp write_bytes should succeed for large payloads");
    client.close();
    server_thread
        .join()
        .expect("tcp large-payload server should join");

    let body = "x".repeat(100_000);
    let http_listener =
        HttpListenerValue::bind("127.0.0.1:0").expect("http listener bind should succeed");
    let http_address = http_listener
        .local_addr()
        .expect("http listener local addr should succeed");
    let expected_body = body.clone();
    let http_thread = {
        let listener = http_listener.clone();
        thread::spawn(move || {
            let exchange = listener
                .accept(Some(timeout), Some(&CancellationContext::default()))
                .expect("http accept should succeed");
            exchange
                .respond_text(200, &expected_body, Vec::new())
                .expect("http respond should succeed for large payloads");
        })
    };
    let response = HttpResponseValue::request_text(
        "GET",
        &format!("http://{http_address}/large"),
        "",
        Vec::new(),
        Some(timeout),
        Some(&cancellation),
    )
    .expect("http request should succeed for large payloads");
    assert_eq!(
        response.text().expect("http body should decode"),
        body,
        "large HTTP bodies should round-trip"
    );
    http_thread.join().expect("http thread should join");
}

#[test]
fn lightweight_scheduler_handles_large_http_binary_round_trip() {
    let timeout = StdDuration::from_secs(5);
    let body = vec![0x7au8; 50_000];
    let expected = body.clone();
    let result = run_lightweight_root_task(move || {
        let listener = HttpListenerValue::bind("127.0.0.1:0")
            .map_err(|error| Diagnostic::new(error.to_string()))?;
        let address = listener
            .local_addr()
            .map_err(|error| Diagnostic::new(error.to_string()))?;
        let server_body = expected.clone();
        let server = spawn_lightweight_task(move || {
            let exchange = listener
                .accept(Some(timeout), None)
                .map_err(|error| Diagnostic::new(error.to_string()))?;
            exchange
                .respond_bytes(200, &server_body, Vec::new())
                .map_err(|error| Diagnostic::new(error.to_string()))?;
            Ok(Value::Unit)
        })?;

        let response = HttpResponseValue::request_bytes(
            "GET",
            &format!("http://{address}/large"),
            &[0],
            Vec::new(),
            Some(timeout),
            None,
        )
        .map_err(|error| Diagnostic::new(error.to_string()))?;
        assert_eq!(response.bytes(), body);
        match server.join_result() {
            super::TaskExecutionResult::Ready(result) => {
                result?;
            }
            super::TaskExecutionResult::Cancelled => {
                return Err(Diagnostic::new("http server task was cancelled"));
            }
        }
        Ok(Value::Unit)
    });
    assert!(
        result.is_ok(),
        "lightweight HTTP round-trip should succeed: {result:?}"
    );
}

#[test]
fn lightweight_scheduler_handles_http_after_blocking_io_server_step() {
    let timeout = StdDuration::from_secs(2);
    let result = run_lightweight_root_task(move || {
        let listener = HttpListenerValue::bind("127.0.0.1:0")
            .map_err(|error| Diagnostic::new(error.to_string()))?;
        let address = listener
            .local_addr()
            .map_err(|error| Diagnostic::new(error.to_string()))?;
        let server = spawn_lightweight_task(move || {
            let exchange = listener
                .accept(Some(timeout), None)
                .map_err(|error| Diagnostic::new(error.to_string()))?;
            let body = run_blocking_io(
                move || {
                    thread::sleep(StdDuration::from_millis(20));
                    Ok::<_, std::io::Error>("x".repeat(50_000))
                },
                None,
            )
            .map_err(|error| Diagnostic::new(error.to_string()))?;
            exchange
                .respond_text(200, &body, Vec::new())
                .map_err(|error| Diagnostic::new(error.to_string()))?;
            Ok(Value::Unit)
        })?;

        let response = HttpResponseValue::request_text(
            "GET",
            &format!("http://{address}/large"),
            "x",
            Vec::new(),
            Some(timeout),
            None,
        )
        .map_err(|error| Diagnostic::new(error.to_string()))?;
        assert_eq!(
            response
                .text()
                .map_err(|error| Diagnostic::new(error.to_string()))?,
            "x".repeat(50_000)
        );
        match server.join_result() {
            super::TaskExecutionResult::Ready(result) => {
                result?;
            }
            super::TaskExecutionResult::Cancelled => {
                return Err(Diagnostic::new("http server task was cancelled"));
            }
        }
        Ok(Value::Unit)
    });
    assert!(
        result.is_ok(),
        "mixed HTTP/blocking-I/O scheduler path should succeed: {result:?}"
    );
}

#[test]
fn lightweight_tasks_observe_blocking_io_completion_before_parent_timeout() {
    let timeout = StdDuration::from_millis(250);
    let start = Instant::now();
    let result = run_lightweight_root_task(move || {
        let task = spawn_lightweight_task(move || {
            let value = run_blocking_io(
                move || {
                    thread::sleep(StdDuration::from_millis(20));
                    Ok::<_, std::io::Error>(41i32)
                },
                None,
            )
            .map_err(|error| Diagnostic::new(error.to_string()))?;
            Ok(Value::Int(IntegerValue::from_signed(i128::from(value))))
        })?;

        match task.wait_result_with_cancellation(Some(timeout), None) {
            super::TaskWaitStatus::Ready(result) => {
                assert_eq!(
                    result?,
                    Value::Int(IntegerValue::from_signed(41)),
                    "blocking I/O completion should resume the waiting task promptly"
                );
                Ok(Value::Unit)
            }
            super::TaskWaitStatus::TimedOut => {
                Err(Diagnostic::new("blocking-I/O child task timed out"))
            }
            super::TaskWaitStatus::Cancelled => {
                Err(Diagnostic::new("blocking-I/O child task was cancelled"))
            }
        }
    });
    assert!(
        result.is_ok(),
        "blocking-I/O child task should finish before the wait timeout: {result:?}"
    );
    assert!(
        start.elapsed() < StdDuration::from_millis(150),
        "blocking-I/O wake should be prompt; elapsed {:?}",
        start.elapsed()
    );
}

#[test]
fn read_all_surfaces_size_limits_for_unbounded_resources() {
    let temp = TempDir::new("aurora-read-all-limit");
    let file_path = temp.path().join("large.txt");
    fs::File::create(&file_path)
        .expect("large test file should be created")
        .set_len((super::MAX_READ_ALL_BYTES + 1) as u64)
        .expect("large test file should be extended");

    let file = FileValue::open(file_path.to_str().expect("utf-8 path")).expect("file should open");
    let error = file.read_all().expect_err("oversized read_all should fail");
    assert_eq!(error.kind(), std::io::ErrorKind::InvalidData);

    let listener = TcpListenerValue::bind("127.0.0.1:0").expect("tcp bind should succeed");
    let address = listener
        .local_addr()
        .expect("listener local addr should succeed");
    let server = listener.clone();
    let server_thread = thread::spawn(move || {
        const CHUNK_SIZE: usize = 64 * 1024;
        let chunk = vec![b'x'; CHUNK_SIZE];
        let mut bytes_remaining = super::MAX_READ_ALL_BYTES + 1;
        let stream = server
            .accept(
                Some(StdDuration::from_secs(5)),
                Some(&CancellationContext::default()),
            )
            .expect("accept should succeed");
        while bytes_remaining > 0 {
            let chunk_len = chunk.len().min(bytes_remaining);
            if stream
                .write_bytes(
                    &chunk[..chunk_len],
                    Some(StdDuration::from_secs(5)),
                    Some(&CancellationContext::default()),
                )
                .is_err()
            {
                break;
            }
            bytes_remaining -= chunk_len;
        }
        stream.close();
    });

    let client = TcpStreamValue::connect(
        &address,
        Some(StdDuration::from_secs(5)),
        Some(&CancellationContext::default()),
    )
    .expect("client should connect");
    let error = client
        .read_all(
            Some(StdDuration::from_secs(5)),
            Some(&CancellationContext::default()),
        )
        .expect_err("oversized tcp read_all should fail");
    assert_eq!(error.kind(), std::io::ErrorKind::InvalidData);
    server_thread.join().expect("server thread should join");
}

#[test]
fn http_request_rejects_control_characters_in_headers() {
    let error = HttpResponseValue::request_text(
        "GET",
        "http://127.0.0.1:1/test",
        "",
        vec![("X-Test".to_string(), "safe\r\nX-Evil: injected".to_string())],
        Some(StdDuration::from_secs(1)),
        Some(&CancellationContext::default()),
    )
    .expect_err("request headers with CRLF should be rejected");
    assert_eq!(error.kind(), std::io::ErrorKind::InvalidInput);
}

#[test]
fn http_request_rejects_invalid_header_names_and_non_ascii_values() {
    let bad_name = HttpResponseValue::request_text(
        "GET",
        "http://127.0.0.1:1/test",
        "",
        vec![("Bad(Name)".to_string(), "value".to_string())],
        Some(StdDuration::from_secs(1)),
        Some(&CancellationContext::default()),
    )
    .expect_err("request headers with invalid token characters should be rejected");
    assert_eq!(bad_name.kind(), std::io::ErrorKind::InvalidInput);

    let bad_value = HttpResponseValue::request_text(
        "GET",
        "http://127.0.0.1:1/test",
        "",
        vec![("X-Test".to_string(), "caf\u{00e9}".to_string())],
        Some(StdDuration::from_secs(1)),
        Some(&CancellationContext::default()),
    )
    .expect_err("request headers with non-ASCII values should be rejected");
    assert_eq!(bad_value.kind(), std::io::ErrorKind::InvalidInput);
}

#[test]
fn http_listener_replies_with_413_for_oversized_requests_and_continues_accepting() {
    let listener =
        HttpListenerValue::bind("127.0.0.1:0").expect("http listener bind should succeed");
    let address = listener
        .local_addr()
        .expect("http listener local addr should succeed");
    let server = listener.clone();
    let oversized_len = super::MAX_HTTP_MESSAGE_BYTES + 1;
    let server_thread = thread::spawn(move || {
        let exchange = server
            .accept(
                Some(StdDuration::from_secs(2)),
                Some(&CancellationContext::default()),
            )
            .expect("server should skip the oversized request and accept the next client");
        assert_eq!(exchange.method(), "GET");
        assert_eq!(exchange.path(), "/ok");
        exchange
            .respond_text(200, "ok", Vec::new())
            .expect("server should reply to the valid request");
    });

    let mut client =
        std::net::TcpStream::connect(&address).expect("http client should connect to listener");
    client
        .write_all(
            format!(
                "POST /upload HTTP/1.1\r\nHost: {address}\r\nContent-Length: {oversized_len}\r\n\r\n"
            )
            .as_bytes(),
        )
        .expect("http request head should write");
    client
        .shutdown(std::net::Shutdown::Write)
        .expect("client shutdown should succeed");

    let mut response = String::new();
    client
        .read_to_string(&mut response)
        .expect("client should receive an HTTP response");
    assert!(
        response.starts_with("HTTP/1.1 413 Payload Too Large\r\n"),
        "expected a 413 response, got: {response:?}"
    );

    let response = HttpResponseValue::request_text(
        "GET",
        &format!("http://{}/ok", address),
        "",
        Vec::new(),
        Some(StdDuration::from_secs(2)),
        Some(&CancellationContext::default()),
    )
    .expect("listener should continue accepting after a 413");
    assert_eq!(response.status(), 200);
    assert_eq!(
        response.text().expect("response body should decode"),
        "ok".to_string()
    );

    server_thread
        .join()
        .expect("oversized http server thread should join");
}

#[test]
fn http_listener_replies_with_431_for_too_many_headers_and_continues_accepting() {
    let listener =
        HttpListenerValue::bind("127.0.0.1:0").expect("http listener bind should succeed");
    let address = listener
        .local_addr()
        .expect("http listener local addr should succeed");
    let server = listener.clone();
    let server_thread = thread::spawn(move || {
        let exchange = server
            .accept(
                Some(StdDuration::from_secs(2)),
                Some(&CancellationContext::default()),
            )
            .expect("server should skip the invalid request and accept the next client");
        assert_eq!(exchange.method(), "GET");
        assert_eq!(exchange.path(), "/ok");
        exchange
            .respond_text(200, "ok", Vec::new())
            .expect("server should reply to the valid request");
    });

    let mut client =
        std::net::TcpStream::connect(&address).expect("http client should connect to listener");
    let mut request = format!("GET /headers HTTP/1.1\r\nHost: {address}\r\n");
    for index in 0..=super::MAX_HTTP_HEADERS {
        request.push_str(&format!("X-Test-{index}: value\r\n"));
    }
    request.push_str("\r\n");
    client
        .write_all(request.as_bytes())
        .expect("request with too many headers should write");
    client
        .shutdown(std::net::Shutdown::Write)
        .expect("client shutdown should succeed");

    let mut response = String::new();
    client
        .read_to_string(&mut response)
        .expect("client should receive an HTTP response");
    assert!(
        response.starts_with("HTTP/1.1 431 Request Header Fields Too Large\r\n"),
        "expected a 431 response, got: {response:?}"
    );

    let response = HttpResponseValue::request_text(
        "GET",
        &format!("http://{}/ok", address),
        "",
        Vec::new(),
        Some(StdDuration::from_secs(2)),
        Some(&CancellationContext::default()),
    )
    .expect("listener should continue accepting after a 431");
    assert_eq!(response.status(), 200);
    assert_eq!(
        response.text().expect("response body should decode"),
        "ok".to_string()
    );

    server_thread
        .join()
        .expect("too-many-headers server thread should join");
}

#[cfg(unix)]
#[test]
fn http_resources_use_nonblocking_descriptors_internally() {
    let short_timeout = StdDuration::from_secs(5);
    let listener =
        HttpListenerValue::bind("127.0.0.1:0").expect("http listener bind should succeed");
    let listener_fd = lock_mutex(&listener.inner.listener)
        .as_ref()
        .expect("http listener should still be open")
        .as_raw_fd();
    assert!(
        fd_is_nonblocking(listener_fd),
        "http listeners should stay in nonblocking mode internally"
    );

    let address = listener
        .local_addr()
        .expect("http listener local addr should succeed");
    let server_thread = {
        let server = listener.clone();
        thread::spawn(move || {
            let exchange = server
                .accept(Some(short_timeout), Some(&CancellationContext::default()))
                .expect("http accept should succeed");
            {
                let stream_guard = lock_mutex(&exchange.inner.stream);
                let stream = stream_guard
                    .as_ref()
                    .expect("http exchange stream should still be open");
                let stream_fd = lock_mutex(&stream.inner.stream)
                    .as_ref()
                    .expect("http exchange stream should still be open")
                    .as_raw_fd();
                assert!(
                    fd_is_nonblocking(stream_fd),
                    "http exchange streams should stay in nonblocking mode internally"
                );
            }
            exchange
                .respond_text(200, "ok", Vec::new())
                .expect("http respond should succeed");
        })
    };

    let response = HttpResponseValue::request_text(
        "GET",
        &format!("http://{}/nonblocking", address),
        "",
        Vec::new(),
        Some(short_timeout),
        Some(&CancellationContext::default()),
    )
    .expect("http request should succeed");
    assert_eq!(response.status(), 200);
    server_thread
        .join()
        .expect("http nonblocking server thread should join");
}

#[cfg(unix)]
#[test]
fn network_resources_use_nonblocking_descriptors_internally() {
    let short_timeout = StdDuration::from_secs(5);
    let cancellation = CancellationContext::default();

    let tcp_listener = TcpListenerValue::bind("127.0.0.1:0").expect("tcp bind should succeed");
    let tcp_listener_fd = lock_mutex(&tcp_listener.inner.listener)
        .as_ref()
        .expect("listener should still be open")
        .as_raw_fd();
    assert!(
        fd_is_nonblocking(tcp_listener_fd),
        "tcp listeners should stay in nonblocking mode internally"
    );
    let tcp_address = tcp_listener
        .local_addr()
        .expect("listener local addr should succeed");
    let tcp_server = tcp_listener.clone();
    let tcp_thread = thread::spawn(move || {
        let accepted = tcp_server
            .accept(Some(short_timeout), Some(&CancellationContext::default()))
            .expect("tcp accept should succeed");
        let accepted_fd = lock_mutex(&accepted.inner.stream)
            .as_ref()
            .expect("accepted tcp stream should still be open")
            .as_raw_fd();
        assert!(
            fd_is_nonblocking(accepted_fd),
            "accepted tcp streams should stay in nonblocking mode internally"
        );
        accepted.close();
    });
    let tcp_client =
        TcpStreamValue::connect(&tcp_address, Some(short_timeout), Some(&cancellation))
            .expect("tcp connect should succeed");
    let tcp_client_fd = lock_mutex(&tcp_client.inner.stream)
        .as_ref()
        .expect("tcp client stream should still be open")
        .as_raw_fd();
    assert!(
        fd_is_nonblocking(tcp_client_fd),
        "tcp client streams should stay in nonblocking mode internally"
    );
    tcp_client.close();
    tcp_thread.join().expect("tcp server thread should join");

    let udp_socket = UdpSocketValue::bind("127.0.0.1:0").expect("udp bind should succeed");
    let udp_fd = lock_mutex(&udp_socket.inner.socket)
        .as_ref()
        .expect("udp socket should still be open")
        .as_raw_fd();
    assert!(
        fd_is_nonblocking(udp_fd),
        "udp sockets should stay in nonblocking mode internally"
    );
}

#[cfg(unix)]
#[test]
fn socket_timeouts_honor_the_requested_budget() {
    let listener = TcpListenerValue::bind("127.0.0.1:0").expect("tcp bind should succeed");
    let started = Instant::now();
    let error = listener
        .accept(
            Some(StdDuration::from_millis(200)),
            Some(&CancellationContext::default()),
        )
        .expect_err("accept without a peer should time out");
    let elapsed = started.elapsed();
    assert_eq!(error.kind(), std::io::ErrorKind::TimedOut);
    assert!(
        elapsed >= StdDuration::from_millis(120),
        "timeouts should honor the caller's budget instead of returning after the first poll slice; elapsed: {elapsed:?}"
    );
}

#[cfg(unix)]
#[test]
fn unix_and_tls_helpers_cover_local_socket_and_tls_surface() {
    let temp = TempDir::new("aurora-runtime-unix-tls");
    let socket_path = PathBuf::from(format!("/tmp/aurora-{}.sock", std::process::id()));
    let listener = UnixListenerValue::bind(
        socket_path
            .to_str()
            .expect("unix socket path should be valid UTF-8"),
    )
    .expect("unix listener bind should succeed");
    let server = listener.clone();
    let server_thread = thread::spawn(move || {
        let stream = server
            .accept(
                Some(StdDuration::from_secs(1)),
                Some(&CancellationContext::default()),
            )
            .expect("unix accept should succeed");
        let line = stream
            .read_line(
                Some(StdDuration::from_secs(1)),
                Some(&CancellationContext::default()),
            )
            .expect("unix read_line should succeed");
        assert_eq!(line.as_deref(), Some("ping"));
        stream
            .write_all(
                "pong",
                Some(StdDuration::from_secs(1)),
                Some(&CancellationContext::default()),
            )
            .expect("unix write_all should succeed");
        stream.close();
    });
    let client = UnixStreamValue::connect(
        socket_path
            .to_str()
            .expect("unix socket path should be valid UTF-8"),
        Some(StdDuration::from_secs(1)),
        Some(&CancellationContext::default()),
    )
    .expect("unix connect should succeed");
    client
        .write_all(
            "ping\n",
            Some(StdDuration::from_secs(1)),
            Some(&CancellationContext::default()),
        )
        .expect("unix write should succeed");
    let reply = client
        .read_exact(
            4,
            Some(StdDuration::from_secs(1)),
            Some(&CancellationContext::default()),
        )
        .expect("unix read_exact should succeed");
    assert_eq!(reply, b"pong");
    server_thread
        .join()
        .expect("unix server thread should join");

    let certificate =
        generate_simple_self_signed(vec!["localhost".to_string()]).expect("cert generation");
    let cert_pem = certificate.cert.pem();
    let key_pem = certificate.key_pair.serialize_pem();
    let cert_path = temp.path().join("cert.pem");
    let key_path = temp.path().join("key.pem");
    fs::write(&cert_path, cert_pem.as_bytes()).expect("write cert pem");
    fs::write(&key_path, key_pem.as_bytes()).expect("write key pem");

    let tls_listener = TlsListenerValue::bind(
        "127.0.0.1:0",
        cert_path.to_str().expect("cert path should be valid UTF-8"),
        key_path.to_str().expect("key path should be valid UTF-8"),
    )
    .expect("tls listener bind should succeed");
    let tls_address = tls_listener
        .local_addr()
        .expect("tls listener local addr should succeed");
    let tls_server = tls_listener.clone();
    let tls_thread = thread::spawn(move || {
        let stream = tls_server
            .accept(
                Some(StdDuration::from_secs(2)),
                Some(&CancellationContext::default()),
            )
            .expect("tls accept should succeed");
        let line = stream
            .read_line(
                Some(StdDuration::from_secs(2)),
                Some(&CancellationContext::default()),
            )
            .expect("tls read_line should succeed");
        assert_eq!(line.as_deref(), Some("secure"));
        stream
            .write_all(
                "ok",
                Some(StdDuration::from_secs(2)),
                Some(&CancellationContext::default()),
            )
            .expect("tls write_all should succeed");
        stream.close();
    });

    let tls_client = TlsStreamValue::connect(
        &tls_address,
        "localhost",
        Some(cert_path.to_str().expect("cert path should be valid UTF-8")),
        Some(StdDuration::from_secs(2)),
        Some(&CancellationContext::default()),
    )
    .expect("tls connect should succeed");
    tls_client
        .write_all(
            "secure\n",
            Some(StdDuration::from_secs(2)),
            Some(&CancellationContext::default()),
        )
        .expect("tls write_all should succeed");
    let tls_reply = tls_client
        .read_exact(
            2,
            Some(StdDuration::from_secs(2)),
            Some(&CancellationContext::default()),
        )
        .expect("tls read_exact should succeed");
    assert_eq!(tls_reply, b"ok");
    tls_thread.join().expect("tls server thread should join");
}

#[cfg(unix)]
#[test]
fn tls_listener_accept_requires_a_completed_handshake() {
    let temp = TempDir::new("aurora-runtime-tls-timeout");
    let certificate =
        generate_simple_self_signed(vec!["localhost".to_string()]).expect("cert generation");
    let cert_path = temp.path().join("cert.pem");
    let key_path = temp.path().join("key.pem");
    fs::write(&cert_path, certificate.cert.pem()).expect("write cert");
    fs::write(&key_path, certificate.key_pair.serialize_pem()).expect("write key");

    let listener = TlsListenerValue::bind(
        "127.0.0.1:0",
        cert_path.to_str().expect("valid cert path"),
        key_path.to_str().expect("valid key path"),
    )
    .expect("tls listener bind should succeed");
    let address = listener
        .local_addr()
        .expect("tls listener addr should succeed");

    let silent_client = thread::spawn(move || {
        let _client =
            std::net::TcpStream::connect(address).expect("plain tcp client should connect");
        thread::sleep(StdDuration::from_millis(300));
    });

    let error = listener
        .accept(
            Some(StdDuration::from_millis(200)),
            Some(&CancellationContext::default()),
        )
        .expect_err("tls accept should fail when the peer never handshakes");
    assert_eq!(error.kind(), std::io::ErrorKind::TimedOut);

    silent_client
        .join()
        .expect("silent tls client thread should join");
    listener.close();
}

#[cfg(unix)]
#[test]
fn tls_listener_accept_skips_timed_out_handshakes_and_accepts_the_next_peer() {
    let temp = TempDir::new("aurora-runtime-tls-slowloris");
    let certificate =
        generate_simple_self_signed(vec!["localhost".to_string()]).expect("cert generation");
    let cert_path = temp.path().join("cert.pem");
    let key_path = temp.path().join("key.pem");
    fs::write(&cert_path, certificate.cert.pem()).expect("write cert");
    fs::write(&key_path, certificate.key_pair.serialize_pem()).expect("write key");

    let listener = TlsListenerValue::bind(
        "127.0.0.1:0",
        cert_path.to_str().expect("valid cert path"),
        key_path.to_str().expect("valid key path"),
    )
    .expect("tls listener bind should succeed");
    let address = listener
        .local_addr()
        .expect("tls listener addr should succeed");

    let server = listener.clone();
    let server_thread = thread::spawn(move || {
        let stream = server
            .accept(
                Some(StdDuration::from_secs(11)),
                Some(&CancellationContext::default()),
            )
            .expect("tls listener should skip the stalled client");
        let line = stream
            .read_line(
                Some(StdDuration::from_secs(2)),
                Some(&CancellationContext::default()),
            )
            .expect("tls read_line should succeed");
        assert_eq!(line.as_deref(), Some("ready"));
        stream
            .write_all(
                "ok",
                Some(StdDuration::from_secs(2)),
                Some(&CancellationContext::default()),
            )
            .expect("tls write_all should succeed");
    });

    let stalled_address = address.clone();
    let stalled_client = thread::spawn(move || {
        let _client =
            std::net::TcpStream::connect(stalled_address).expect("plain tcp client should connect");
        thread::sleep(StdDuration::from_secs(11));
    });

    thread::sleep(StdDuration::from_millis(100));
    let tls_client = TlsStreamValue::connect(
        &address,
        "localhost",
        Some(cert_path.to_str().expect("cert path should be valid UTF-8")),
        Some(StdDuration::from_secs(12)),
        Some(&CancellationContext::default()),
    )
    .expect("tls connect should succeed after the stalled peer is discarded");
    tls_client
        .write_all(
            "ready\n",
            Some(StdDuration::from_secs(2)),
            Some(&CancellationContext::default()),
        )
        .expect("tls write_all should succeed");
    let reply = tls_client
        .read_exact(
            2,
            Some(StdDuration::from_secs(2)),
            Some(&CancellationContext::default()),
        )
        .expect("tls read_exact should succeed");
    assert_eq!(reply, b"ok");

    stalled_client
        .join()
        .expect("stalled tls client thread should join");
    server_thread
        .join()
        .expect("tls slowloris server thread should join");
}

#[test]
fn tls_handshake_deadline_caps_requested_timeout_to_default_budget() {
    let deadline = super::tls_handshake_deadline(Some(
        Instant::now()
            .checked_add(StdDuration::from_secs(60))
            .expect("future deadline should exist"),
    ))
    .expect("handshake deadline should exist");
    let remaining = deadline.saturating_duration_since(Instant::now());
    assert!(
        remaining <= super::DEFAULT_TLS_HANDSHAKE_TIMEOUT + StdDuration::from_millis(250),
        "handshake deadline should cap user timeouts to the default budget; remaining {remaining:?}"
    );
}

#[test]
fn websocket_error_mapping_preserves_io_error_kinds() {
    let error = super::websocket_error_to_io(tungstenite::Error::Io(io::Error::new(
        io::ErrorKind::BrokenPipe,
        "broken pipe",
    )));
    assert_eq!(error.kind(), io::ErrorKind::BrokenPipe);

    let other = super::websocket_error_to_io(tungstenite::Error::ConnectionClosed);
    assert_eq!(other.kind(), io::ErrorKind::Other);
}

#[cfg(unix)]
#[test]
fn unix_tls_and_websocket_resources_use_nonblocking_descriptors_internally() {
    let temp = TempDir::new("aurora-runtime-evented-network");

    let socket_path = PathBuf::from(format!("/tmp/aurora-evented-{}.sock", std::process::id()));
    let unix_listener = UnixListenerValue::bind(
        socket_path
            .to_str()
            .expect("unix socket path should be valid UTF-8"),
    )
    .expect("unix listener bind should succeed");
    let unix_listener_fd = lock_mutex(&unix_listener.inner.listener)
        .as_ref()
        .expect("unix listener should still be open")
        .as_raw_fd();
    assert!(
        fd_is_nonblocking(unix_listener_fd),
        "unix listeners should stay in nonblocking mode internally"
    );
    let unix_server = unix_listener.clone();
    let unix_thread = thread::spawn(move || {
        let accepted = unix_server
            .accept(
                Some(StdDuration::from_secs(2)),
                Some(&CancellationContext::default()),
            )
            .expect("unix accept should succeed");
        let accepted_fd = lock_mutex(&accepted.inner.stream)
            .as_ref()
            .expect("unix accepted stream should still be open")
            .as_raw_fd();
        assert!(
            fd_is_nonblocking(accepted_fd),
            "accepted unix streams should stay in nonblocking mode internally"
        );
        accepted.close();
    });
    let unix_client = UnixStreamValue::connect(
        socket_path
            .to_str()
            .expect("unix socket path should be valid UTF-8"),
        Some(StdDuration::from_secs(2)),
        Some(&CancellationContext::default()),
    )
    .expect("unix connect should succeed");
    let unix_client_fd = lock_mutex(&unix_client.inner.stream)
        .as_ref()
        .expect("unix client stream should still be open")
        .as_raw_fd();
    assert!(
        fd_is_nonblocking(unix_client_fd),
        "unix client streams should stay in nonblocking mode internally"
    );
    unix_client.close();
    unix_thread.join().expect("unix server thread should join");

    let certificate =
        generate_simple_self_signed(vec!["localhost".to_string()]).expect("cert generation");
    let cert_path = temp.path().join("cert.pem");
    let key_path = temp.path().join("key.pem");
    fs::write(&cert_path, certificate.cert.pem().as_bytes()).expect("write cert pem");
    fs::write(&key_path, certificate.key_pair.serialize_pem().as_bytes()).expect("write key pem");

    let tls_listener = TlsListenerValue::bind(
        "127.0.0.1:0",
        cert_path.to_str().expect("cert path should be valid UTF-8"),
        key_path.to_str().expect("key path should be valid UTF-8"),
    )
    .expect("tls listener bind should succeed");
    let tls_listener_fd = lock_mutex(&tls_listener.inner.listener)
        .as_ref()
        .expect("tls listener should still be open")
        .as_raw_fd();
    assert!(
        fd_is_nonblocking(tls_listener_fd),
        "tls listeners should stay in nonblocking mode internally"
    );
    let tls_address = tls_listener
        .local_addr()
        .expect("tls listener local addr should succeed");
    let tls_server = tls_listener.clone();
    let tls_thread = thread::spawn(move || {
        let accepted = tls_server
            .accept(
                Some(StdDuration::from_secs(2)),
                Some(&CancellationContext::default()),
            )
            .expect("tls accept should succeed");
        let accepted_fd = match lock_mutex(&accepted.inner.stream)
            .as_ref()
            .expect("tls accepted stream should still be open")
        {
            super::TlsStreamKind::Client(stream) => stream.sock.as_raw_fd(),
            super::TlsStreamKind::Server(stream) => stream.sock.as_raw_fd(),
        };
        assert!(
            fd_is_nonblocking(accepted_fd),
            "accepted tls streams should stay in nonblocking mode internally"
        );
        assert_eq!(
            accepted
                .read_exact(
                    1,
                    Some(StdDuration::from_secs(2)),
                    Some(&CancellationContext::default()),
                )
                .expect("tls handshake read should succeed"),
            b"x"
        );
        accepted.close();
    });
    let tls_client = TlsStreamValue::connect(
        &tls_address,
        "localhost",
        Some(cert_path.to_str().expect("cert path should be valid UTF-8")),
        Some(StdDuration::from_secs(2)),
        Some(&CancellationContext::default()),
    )
    .expect("tls connect should succeed");
    let tls_client_fd = match lock_mutex(&tls_client.inner.stream)
        .as_ref()
        .expect("tls client stream should still be open")
    {
        super::TlsStreamKind::Client(stream) => stream.sock.as_raw_fd(),
        super::TlsStreamKind::Server(stream) => stream.sock.as_raw_fd(),
    };
    assert!(
        fd_is_nonblocking(tls_client_fd),
        "tls client streams should stay in nonblocking mode internally"
    );
    tls_client
        .write_all(
            "x",
            Some(StdDuration::from_secs(2)),
            Some(&CancellationContext::default()),
        )
        .expect("tls handshake write should succeed");
    tls_client.close();
    tls_thread.join().expect("tls server thread should join");

    let ws_listener = WebSocketListenerValue::bind("127.0.0.1:0")
        .expect("websocket listener bind should succeed");
    let ws_listener_fd = lock_mutex(&ws_listener.inner.listener)
        .as_ref()
        .expect("websocket listener should still be open")
        .as_raw_fd();
    assert!(
        fd_is_nonblocking(ws_listener_fd),
        "websocket listeners should stay in nonblocking mode internally"
    );
    let ws_address = ws_listener
        .local_addr()
        .expect("websocket listener local addr should succeed");
    let ws_server = ws_listener.clone();
    let ws_thread = thread::spawn(move || {
        let socket = ws_server
            .accept(Some(StdDuration::from_secs(2)))
            .expect("websocket accept should succeed");
        let accepted_fd = match lock_mutex(&socket.inner.socket)
            .as_ref()
            .expect("accepted websocket should still be open")
        {
            super::WebSocketStateKind::Plain(socket) => socket.get_ref().as_raw_fd(),
            super::WebSocketStateKind::MaybeTls(socket) => match socket.get_ref() {
                tungstenite::stream::MaybeTlsStream::Plain(stream) => stream.as_raw_fd(),
                tungstenite::stream::MaybeTlsStream::Rustls(stream) => stream.get_ref().as_raw_fd(),
                _ => unreachable!("unexpected websocket transport"),
            },
        };
        assert!(
            fd_is_nonblocking(accepted_fd),
            "accepted websocket streams should stay in nonblocking mode internally"
        );
        socket.close().expect("websocket close should succeed");
    });
    let ws_client = super::WebSocketValue::connect(
        &format!("ws://{}", ws_address),
        Some(StdDuration::from_secs(2)),
    )
    .expect("websocket connect should succeed");
    let ws_client_fd = match lock_mutex(&ws_client.inner.socket)
        .as_ref()
        .expect("websocket client should still be open")
    {
        super::WebSocketStateKind::Plain(socket) => socket.get_ref().as_raw_fd(),
        super::WebSocketStateKind::MaybeTls(socket) => match socket.get_ref() {
            tungstenite::stream::MaybeTlsStream::Plain(stream) => stream.as_raw_fd(),
            tungstenite::stream::MaybeTlsStream::Rustls(stream) => stream.get_ref().as_raw_fd(),
            _ => unreachable!("unexpected websocket transport"),
        },
    };
    assert!(
        fd_is_nonblocking(ws_client_fd),
        "websocket clients should stay in nonblocking mode internally"
    );
    ws_client
        .close()
        .expect("websocket client close should succeed");
    ws_thread
        .join()
        .expect("websocket server thread should join");
}
