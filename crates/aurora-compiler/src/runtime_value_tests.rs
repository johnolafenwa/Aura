use super::{
    cast_numeric_value, io_decode_utf8, lock_mutex, option_none, option_some, render_float,
    result_err, result_ok, send_error_cancelled, send_error_closed, sleep_with_runtime_scheduler,
    wait_for_select_progress, CancellationContext, ChannelValue, EnumVariantValue, FileValue,
    HttpListenerValue, HttpResponseValue, MapValue, RangeValue, SetValue, TaskGroupValue,
    TaskValue, TcpListenerValue, TcpStreamValue, TryRecvResult, UdpSocketValue, Value, VecValue,
    WebSocketListenerValue,
};
use crate::diag::Span;
use crate::integer::IntegerValue;
use crate::sema::Type;
use rcgen::generate_simple_self_signed;
use std::fs;
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
        task.join_result().expect("first join should succeed"),
        Value::Int(IntegerValue::from_signed(9))
    );
    assert_eq!(
        task.join_result().expect("cached join should also succeed"),
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
        task.join_result()
            .expect("poisoned task handle lock should recover"),
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
        sleep_with_runtime_scheduler(StdDuration::from_millis(250), Some(&cancellation));
    });

    thread::sleep(StdDuration::from_millis(20));
    group.cancel();
    worker.join().expect("scheduler sleep worker should join");
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
        wait_for_select_progress(
            &[channel],
            true,
            &[],
            &[Instant::now() + StdDuration::from_millis(250)],
            Some(&cancellation),
        );
    });

    thread::sleep(StdDuration::from_millis(20));
    group.cancel();
    worker.join().expect("scheduler select worker should join");
    assert!(
        start.elapsed() < StdDuration::from_millis(100),
        "scheduler select wait should wake promptly when cancelled; elapsed {:?}",
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
