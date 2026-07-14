use super::{
    cancel_current_lightweight_task_boundary, cast_numeric_value, create_dir_once,
    decode_process_restart_policy, decode_process_stdio, finalize_task_execution,
    float_floor_divmod, io_decode_utf8, io_error, lock_mutex, non_unix_tls_listener_wait_timeout,
    option_none, option_some, process_error_cancelled, process_error_no_command,
    process_error_other, process_error_spawn, process_error_timed_out,
    process_supervisor_event_failed, process_supervisor_wait_cancelled,
    process_supervisor_wait_event, process_supervisor_wait_timed_out, process_wait_cancelled,
    process_wait_failed, process_wait_timed_out, queue_receive_cancelled, queue_receive_closed,
    queue_receive_item, queue_receive_timed_out, recv_for_task_group_iteration,
    remove_file_checked, render_float, render_float32, result_err, result_ok, run_blocking_io,
    run_lightweight_root_task, send_error_cancelled, send_error_closed, send_error_full,
    send_error_timed_out, sleep_with_runtime_scheduler, spawn_lightweight_task,
    spawn_lightweight_task_with_cancellation,
    spawn_lightweight_task_with_cancellation_and_forced_exit_cleanup,
    task_group_cleanup_should_cancel, task_result_cancelled, task_result_error, task_result_ready,
    task_result_timed_out, validate_read_line_capacity, validate_requested_read_size,
    wait_all_cancelled, wait_all_error, wait_all_ready, wait_all_timed_out, wait_any_cancelled,
    wait_any_error, wait_any_ready, wait_any_timed_out, wait_condvar, wait_for_runtime_scheduler,
    wait_timeout_condvar, CancellationContext, ChannelValue, EnumVariantValue, FileValue,
    HttpListenerValue, HttpResponseValue, LightweightTaskFailureSignal, MapValue,
    ModuleNamespaceValue, ProcessChildValue, ProcessChildWaitStatus, ProcessCompletedValue,
    ProcessRestartPolicy, ProcessStdioConfig, ProcessSupervisorValue, ProcessSupervisorWaitStatus,
    RangeValue, RecvValueResult, SetValue, TaskCancelledSignal, TaskExecutionResult,
    TaskGroupValue, TaskValue, TaskWaitStatus, TcpListenerValue, TcpStreamValue, TryRecvResult,
    UdpDatagramValue, UdpSocketValue, Value, VecValue, WebSocketListenerValue, MAX_READ_ALL_BYTES,
};
use crate::diag::{Diagnostic, Span};
use crate::integer::IntegerValue;
use crate::sema::Type;
use rcgen::generate_simple_self_signed;
use std::collections::BTreeMap;
use std::fs;
use std::io::{self, Read, Write};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::thread;
use std::time::{Duration as StdDuration, Instant, SystemTime, UNIX_EPOCH};

#[cfg(unix)]
use super::{
    read_all_with_fd_deadline, read_exact_with_fd_deadline, read_line_with_fd_deadline,
    read_some_with_fd_deadline, write_all_with_fd_deadline, TlsListenerValue, TlsStreamValue,
    UnixListenerValue, UnixStreamValue,
};
#[cfg(unix)]
use std::os::fd::AsRawFd;

struct TempDir {
    path: PathBuf,
}

fn wait_task_ready(task: &TaskValue) -> Result<Value, Diagnostic> {
    match task.wait_result_with_cancellation_observed(None, None) {
        TaskWaitStatus::Ready(result) => result,
        TaskWaitStatus::Cancelled => Err(Diagnostic::new("task was cancelled")),
        TaskWaitStatus::TimedOut => Err(Diagnostic::new("task wait timed out")),
    }
}

fn assert_cast_source_type(value: Value, expected_source: &str) {
    let error = cast_numeric_value(value, &Type::named("int32"), None)
        .expect_err("non-numeric runtime values should not cast to integers");
    assert!(
        error
            .message
            .contains(&format!("found `{expected_source}` and `int32`")),
        "unexpected diagnostic for {expected_source}: {}",
        error.message
    );
}

fn assert_value_equals_clone(value: Value) {
    assert_eq!(value, value.clone());
}

#[test]
fn bounded_read_helpers_reject_zero_and_oversized_requests_without_allocation() {
    let error = validate_requested_read_size("read_bytes(...)", 0)
        .expect_err("zero-byte bounded reads should be rejected before reading");
    assert_eq!(error.kind(), io::ErrorKind::InvalidInput);

    let error = validate_requested_read_size("read_exact(...)", MAX_READ_ALL_BYTES + 1)
        .expect_err("oversized read_exact requests should fail before allocation");
    assert_eq!(error.kind(), io::ErrorKind::InvalidInput);

    let error = validate_read_line_capacity(MAX_READ_ALL_BYTES)
        .expect_err("line reads should enforce the shared read limit");
    assert_eq!(error.kind(), io::ErrorKind::InvalidData);

    let error = super::validate_udp_datagram_limit(super::MAX_UDP_DATAGRAM_BYTES + 1)
        .expect_err("oversized UDP reads should fail before allocation");
    assert_eq!(error.kind(), io::ErrorKind::InvalidInput);

    let error = super::validate_udp_datagram_limit(0)
        .expect_err("zero-byte UDP reads should be rejected before receiving a datagram");
    assert_eq!(error.kind(), io::ErrorKind::InvalidInput);
}

#[cfg(unix)]
#[test]
fn fd_reads_check_deadline_and_size_before_ready_reads() {
    let expired = Instant::now() - StdDuration::from_millis(1);
    let mut line_reader = io::Cursor::new(b"ready\n".to_vec());
    let error = read_line_with_fd_deadline(&mut line_reader, 0, libc::POLLIN, Some(expired), None)
        .expect_err("expired read_line deadline should fail before consuming ready bytes");
    assert_eq!(error.kind(), io::ErrorKind::TimedOut);

    let mut exact_reader = io::empty();
    let error = read_exact_with_fd_deadline(
        &mut exact_reader,
        0,
        MAX_READ_ALL_BYTES + 1,
        libc::POLLIN,
        None,
        None,
    )
    .expect_err("oversized read_exact should fail before allocating");
    assert_eq!(error.kind(), io::ErrorKind::InvalidInput);

    let mut some_reader = io::empty();
    let error = read_some_with_fd_deadline(
        &mut some_reader,
        0,
        MAX_READ_ALL_BYTES + 1,
        libc::POLLIN,
        None,
        None,
    )
    .expect_err("oversized read_bytes should fail before allocating");
    assert_eq!(error.kind(), io::ErrorKind::InvalidInput);

    let mut zero_exact_reader = io::Cursor::new(b"ready".to_vec());
    let error =
        read_exact_with_fd_deadline(&mut zero_exact_reader, -1, 0, libc::POLLIN, None, None)
            .expect_err("zero-byte exact reads should be rejected");
    assert_eq!(error.kind(), io::ErrorKind::InvalidInput);
    assert_eq!(zero_exact_reader.position(), 0);

    let mut zero_some_reader = io::Cursor::new(b"ready".to_vec());
    let error = read_some_with_fd_deadline(&mut zero_some_reader, -1, 0, libc::POLLIN, None, None)
        .expect_err("zero-byte bounded reads should be rejected");
    assert_eq!(error.kind(), io::ErrorKind::InvalidInput);
    assert_eq!(zero_some_reader.position(), 0);

    let mut empty_line_reader = io::Cursor::new(Vec::<u8>::new());
    assert_eq!(
        read_line_with_fd_deadline(&mut empty_line_reader, -1, libc::POLLIN, None, None)
            .expect("empty streams should report EOF without polling"),
        None
    );

    let mut trimmed_line_reader = io::Cursor::new(b"ready\r\n".to_vec());
    assert_eq!(
        read_line_with_fd_deadline(&mut trimmed_line_reader, -1, libc::POLLIN, None, None)
            .expect("ready line should decode before polling"),
        Some("ready".to_string())
    );

    let mut short_exact_reader = io::Cursor::new(b"x".to_vec());
    let error =
        read_exact_with_fd_deadline(&mut short_exact_reader, -1, 2, libc::POLLIN, None, None)
            .expect_err("short streams should report unexpected EOF");
    assert_eq!(error.kind(), io::ErrorKind::UnexpectedEof);

    let mut ready_some_reader = io::Cursor::new(b"abc".to_vec());
    assert_eq!(
        read_some_with_fd_deadline(&mut ready_some_reader, -1, 8, libc::POLLIN, None, None)
            .expect("ready bytes should be returned without polling"),
        Some(b"abc".to_vec())
    );

    struct FailingReader;

    impl Read for FailingReader {
        fn read(&mut self, _buf: &mut [u8]) -> io::Result<usize> {
            Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "test read failure",
            ))
        }
    }

    let mut failing_line_reader = FailingReader;
    let error = read_line_with_fd_deadline(&mut failing_line_reader, -1, libc::POLLIN, None, None)
        .expect_err("non-retryable read_line failures should be returned");
    assert_eq!(error.kind(), io::ErrorKind::InvalidInput);

    let mut failing_exact_reader = FailingReader;
    let error =
        read_exact_with_fd_deadline(&mut failing_exact_reader, -1, 1, libc::POLLIN, None, None)
            .expect_err("non-retryable read_exact failures should be returned");
    assert_eq!(error.kind(), io::ErrorKind::InvalidInput);

    let mut failing_some_reader = FailingReader;
    let error =
        read_some_with_fd_deadline(&mut failing_some_reader, -1, 1, libc::POLLIN, None, None)
            .expect_err("non-retryable read_bytes failures should be returned");
    assert_eq!(error.kind(), io::ErrorKind::InvalidInput);

    let mut failing_all_reader = FailingReader;
    let error = read_all_with_fd_deadline(&mut failing_all_reader, -1, libc::POLLIN, None, None)
        .expect_err("non-retryable read_all failures should be returned");
    assert_eq!(error.kind(), io::ErrorKind::InvalidInput);

    struct ZeroWriter;

    impl Write for ZeroWriter {
        fn write(&mut self, _buf: &[u8]) -> io::Result<usize> {
            Ok(0)
        }

        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }

    let mut zero_writer = ZeroWriter;
    let error = write_all_with_fd_deadline(&mut zero_writer, -1, b"x", libc::POLLOUT, None, None)
        .expect_err("zero-byte writes should report WriteZero");
    assert_eq!(error.kind(), io::ErrorKind::WriteZero);

    struct FailingWriter;

    impl Write for FailingWriter {
        fn write(&mut self, _buf: &[u8]) -> io::Result<usize> {
            Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "test write failure",
            ))
        }

        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }

    let mut failing_writer = FailingWriter;
    let error =
        write_all_with_fd_deadline(&mut failing_writer, -1, b"x", libc::POLLOUT, None, None)
            .expect_err("non-retryable write failures should be returned");
    assert_eq!(error.kind(), io::ErrorKind::InvalidInput);
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
static UNIX_SOCKET_COUNTER: AtomicUsize = AtomicUsize::new(0);

#[cfg(unix)]
fn unique_unix_socket_path(prefix: &str) -> PathBuf {
    let nonce = UNIX_SOCKET_COUNTER.fetch_add(1, Ordering::SeqCst);
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system time should be after unix epoch")
        .as_nanos();
    let path = PathBuf::from(format!(
        "/tmp/{prefix}-{}-{nonce}-{nanos}.sock",
        std::process::id()
    ));
    let _ = fs::remove_file(&path);
    path
}

#[cfg(unix)]
fn fd_is_nonblocking(fd: i32) -> bool {
    let flags = unsafe { libc::fcntl(fd, libc::F_GETFL) };
    assert!(flags >= 0, "fcntl(F_GETFL) should succeed");
    flags & libc::O_NONBLOCK != 0
}

#[test]
fn runtime_io_wait_helpers_cover_deadlines_cancellation_and_poll_edges() {
    assert!(super::deadline_from_timeout(None).is_none());
    assert!(super::deadline_from_timeout(Some(StdDuration::from_millis(1))).is_some());
    assert_eq!(super::duration_to_poll_timeout(StdDuration::ZERO), 0);
    assert_eq!(
        super::duration_to_poll_timeout(StdDuration::from_millis(i32::MAX as u64 + 1)),
        i32::MAX
    );
    assert!(super::tls_handshake_deadline(None).is_some());
    let requested_tls_deadline = Instant::now() + StdDuration::from_millis(1);
    assert_eq!(
        super::tls_handshake_deadline(Some(requested_tls_deadline)),
        Some(requested_tls_deadline)
    );

    let cancel_flag = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(true));
    let cancelled = CancellationContext {
        flags: vec![cancel_flag],
    };
    let error = super::check_deadline_and_cancellation(None, Some(&cancelled))
        .expect_err("cancelled context should abort before deadline checks");
    assert_eq!(error.kind(), io::ErrorKind::Interrupted);
    let error = super::next_wait_slice(None, Some(&cancelled))
        .expect_err("cancelled context should abort wait-slice calculation");
    assert_eq!(error.kind(), io::ErrorKind::Interrupted);

    let active_cancellation = CancellationContext::default();
    let slice = super::next_wait_slice(None, Some(&active_cancellation))
        .expect("active cancellation handle should produce a polling slice")
        .expect("cancellable waits should not block forever");
    assert!(slice <= StdDuration::from_millis(50));
    assert!(super::next_wait_slice(None, None)
        .expect("non-cancellable wait should be valid")
        .is_none());
    assert!(
        super::next_wait_slice(Some(Instant::now() + StdDuration::from_millis(5)), None)
            .expect("future deadlines should produce a finite wait")
            .is_some()
    );
    let expired = Instant::now() - StdDuration::from_millis(1);
    let error = super::next_wait_slice(Some(expired), None)
        .expect_err("expired deadlines should fail immediately");
    assert_eq!(error.kind(), io::ErrorKind::TimedOut);
    let error = non_unix_tls_listener_wait_timeout(true, Some(expired), None)
        .expect_err("empty TLS listener waits should honor expired deadlines");
    assert_eq!(error.kind(), io::ErrorKind::TimedOut);
    #[cfg(unix)]
    {
        let (_writer, reader) =
            std::os::unix::net::UnixStream::pair().expect("unix stream pair should be available");
        super::wait_for_tls_listener_progress(
            reader.as_raw_fd(),
            false,
            Some(Instant::now() + StdDuration::from_millis(10)),
            None,
        )
        .expect("non-empty TLS handshake queues should ignore short listener poll timeouts");
    }

    assert!(super::is_retryable_network_error(&io::Error::new(
        io::ErrorKind::WouldBlock,
        "retry",
    )));
    assert!(super::is_retryable_network_error(&io::Error::new(
        io::ErrorKind::TimedOut,
        "retry",
    )));
    assert!(super::is_retryable_network_error(&io::Error::new(
        io::ErrorKind::Interrupted,
        "retry",
    )));
    assert!(!super::is_retryable_network_error(&io::Error::new(
        io::ErrorKind::InvalidInput,
        "stop",
    )));

    let recv_ready = ChannelValue::new();
    recv_ready
        .send(Value::Unit)
        .expect("queued channel values should make receives ready");
    assert_eq!(
        wait_for_runtime_scheduler(vec![recv_ready], false, Vec::new(), Vec::new(), None, None),
        super::RuntimeSchedulerWakeReason::Ready
    );
    assert_eq!(
        wait_for_runtime_scheduler(
            Vec::new(),
            false,
            vec![ChannelValue::with_capacity(1)],
            Vec::new(),
            None,
            None,
        ),
        super::RuntimeSchedulerWakeReason::Ready
    );
    let completed_task = TaskValue::from_handle(thread::spawn(|| Ok(Value::Unit)));
    assert_eq!(
        wait_task_ready(&completed_task).expect("test task should complete"),
        Value::Unit
    );
    assert_eq!(
        wait_for_runtime_scheduler(
            Vec::new(),
            false,
            Vec::new(),
            vec![completed_task],
            None,
            None
        ),
        super::RuntimeSchedulerWakeReason::Ready
    );

    let scheduler = super::runtime_scheduler().clone();
    let first_deadline = Instant::now() + StdDuration::from_millis(10);
    let second_deadline = Instant::now() + StdDuration::from_millis(50);
    let first = scheduler.register(
        Vec::new(),
        false,
        Vec::new(),
        Vec::new(),
        Some(first_deadline),
        None,
    );
    let second = scheduler.register(
        Vec::new(),
        false,
        Vec::new(),
        Vec::new(),
        Some(second_deadline),
        None,
    );
    assert_eq!(first.wait(), super::RuntimeSchedulerWakeReason::TimedOut);
    drop(second);
    scheduler.notify();
}

#[cfg(unix)]
#[test]
fn lightweight_scheduler_external_event_paths_cover_ready_queue_and_fd_polling() {
    let mut scheduler = super::LightweightTaskScheduler::new();
    scheduler
        .ready
        .push_back((7, super::RuntimeSchedulerWakeReason::Ready));
    scheduler.wait_for_external_events();
    assert_eq!(
        scheduler.ready.pop_front(),
        Some((7, super::RuntimeSchedulerWakeReason::Ready))
    );

    let (_idle_writer, idle_reader) =
        std::os::unix::net::UnixStream::pair().expect("idle stream pair should be available");
    scheduler.waiting.insert(
        8,
        super::TaskWaitRegistration {
            recv_channels: Vec::new(),
            ignore_closed_recv_channels: false,
            send_channels: Vec::new(),
            task_waits: Vec::new(),
            deadline: None,
            cancellation: None,
            fd_wait: Some(super::FdWaitRegistration {
                fd: idle_reader.as_raw_fd(),
                events: libc::POLLIN,
            }),
        },
    );
    scheduler.waiting.insert(
        9,
        super::TaskWaitRegistration {
            recv_channels: Vec::new(),
            ignore_closed_recv_channels: false,
            send_channels: Vec::new(),
            task_waits: Vec::new(),
            deadline: None,
            cancellation: None,
            fd_wait: None,
        },
    );
    scheduler.wait_for_external_events();
    assert!(scheduler.ready.is_empty());
    scheduler.waiting.clear();

    let (mut writer, reader) =
        std::os::unix::net::UnixStream::pair().expect("ready stream pair should be available");
    writer
        .write_all(b"x")
        .expect("ready stream should accept a byte");
    scheduler.waiting.insert(
        10,
        super::TaskWaitRegistration {
            recv_channels: Vec::new(),
            ignore_closed_recv_channels: false,
            send_channels: Vec::new(),
            task_waits: Vec::new(),
            deadline: Some(Instant::now() + StdDuration::from_millis(100)),
            cancellation: None,
            fd_wait: Some(super::FdWaitRegistration {
                fd: reader.as_raw_fd(),
                events: libc::POLLIN,
            }),
        },
    );
    scheduler.wait_for_external_events();
    assert_eq!(
        scheduler.ready.pop_front(),
        Some((10, super::RuntimeSchedulerWakeReason::Ready))
    );
}

#[test]
fn lightweight_scheduler_completion_helpers_cover_waiters_and_unbounded_waits() {
    let mut scheduler = super::LightweightTaskScheduler::new();
    scheduler.resume_task(999, super::RuntimeSchedulerWakeReason::Ready);

    let blocker = ChannelValue::new();
    let release = blocker.clone();
    let task = TaskValue::from_handle(thread::spawn(move || {
        let _ = blocker.recv_with_cancellation(None, None);
        Ok(Value::Unit)
    }));
    {
        let mut handle = lock_mutex(&task.inner.handle);
        match &mut *handle {
            super::TaskHandle::Running { waiters } => waiters.push(42),
            super::TaskHandle::Completed(_) => panic!("test task should still be running"),
        }
    }
    scheduler.waiting.insert(
        42,
        super::TaskWaitRegistration {
            recv_channels: Vec::new(),
            ignore_closed_recv_channels: false,
            send_channels: Vec::new(),
            task_waits: Vec::new(),
            deadline: None,
            cancellation: None,
            fd_wait: None,
        },
    );
    scheduler.complete_task(99, &task.inner, TaskExecutionResult::Ready(Ok(Value::Unit)));
    assert!(!scheduler.waiting.contains_key(&42));
    assert_eq!(
        scheduler.ready.pop_front(),
        Some((42, super::RuntimeSchedulerWakeReason::Ready))
    );
    scheduler.complete_task(99, &task.inner, TaskExecutionResult::Cancelled);
    release.close();

    let waiting_task = scheduler
        .spawn_task(None, || {
            let _ = super::yield_current_lightweight_wait(super::TaskWaitRegistration {
                recv_channels: Vec::new(),
                ignore_closed_recv_channels: false,
                send_channels: Vec::new(),
                task_waits: Vec::new(),
                deadline: None,
                cancellation: None,
                fd_wait: None,
            });
            Ok(Value::Unit)
        })
        .expect("lightweight task should spawn");
    scheduler.resume_task(1, super::RuntimeSchedulerWakeReason::Ready);
    assert!(scheduler.task_wait_is_unbounded(&waiting_task));
    scheduler.resume_task(1, super::RuntimeSchedulerWakeReason::Ready);
    assert!(!scheduler.task_wait_is_unbounded(&waiting_task));
    match waiting_task.completed_result() {
        Some(TaskExecutionResult::Ready(Ok(Value::Unit))) => {}
        other => panic!("expected completed unit task, got {other:?}"),
    }

    let timed_wait_task = scheduler
        .spawn_task(None, || {
            let _ = super::yield_current_lightweight_wait(super::TaskWaitRegistration {
                recv_channels: Vec::new(),
                ignore_closed_recv_channels: false,
                send_channels: Vec::new(),
                task_waits: Vec::new(),
                deadline: Some(Instant::now() + StdDuration::from_secs(1)),
                cancellation: None,
                fd_wait: None,
            });
            Ok(Value::Unit)
        })
        .expect("lightweight task should spawn");
    scheduler.resume_task(2, super::RuntimeSchedulerWakeReason::Ready);
    assert!(!scheduler.task_wait_is_unbounded(&timed_wait_task));
    scheduler.resume_task(2, super::RuntimeSchedulerWakeReason::Ready);
    match timed_wait_task.completed_result() {
        Some(TaskExecutionResult::Ready(Ok(Value::Unit))) => {}
        other => panic!("expected completed timed-wait task, got {other:?}"),
    }

    let exit_without_result = scheduler
        .spawn_task(None, || {
            let _ = super::yield_current_lightweight_task(super::TaskYield::Exit);
            Ok(Value::Unit)
        })
        .expect("lightweight task should spawn");
    scheduler.resume_task(3, super::RuntimeSchedulerWakeReason::Ready);
    match exit_without_result.completed_result() {
        Some(TaskExecutionResult::Ready(Err(error))) => assert!(error
            .message
            .contains("lightweight task exited without a result")),
        other => panic!("expected defensive missing-result error, got {other:?}"),
    }

    let mut manual_scheduler = super::LightweightTaskScheduler::new();
    let context = super::LightweightTaskContext {
        scheduler: &mut manual_scheduler as *mut _,
        task_id: 99,
        yielder: std::cell::Cell::new(std::ptr::null()),
        cancellation: None,
    };
    let _guard = super::enter_lightweight_task_context(&context);
    assert_eq!(
        super::yield_current_lightweight_task(super::TaskYield::YieldNow),
        None
    );
}

#[cfg(debug_assertions)]
#[test]
#[should_panic(
    expected = "structured concurrency invariant violated: direct task remained suspended at scheduler teardown"
)]
fn lightweight_scheduler_rejects_abandoned_direct_tasks_at_teardown() {
    let _ = run_lightweight_root_task(|| {
        unsafe {
            spawn_lightweight_task_with_cancellation_and_forced_exit_cleanup(
                CancellationContext::default(),
                || Ok(Value::Unit),
                || {},
            )?;
        }
        Ok(Value::Unit)
    });
}

#[cfg(unix)]
#[test]
fn unix_fd_nonblocking_helper_toggles_socket_flags_and_reports_bad_fds() {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("listener should bind");
    super::set_fd_nonblocking(listener.as_raw_fd(), true)
        .expect("socket should become nonblocking");
    assert!(fd_is_nonblocking(listener.as_raw_fd()));
    super::set_fd_nonblocking(listener.as_raw_fd(), false)
        .expect("socket should return to blocking mode");
    assert!(!fd_is_nonblocking(listener.as_raw_fd()));

    let (mut writer, reader) =
        std::os::unix::net::UnixStream::pair().expect("unix stream pair should be available");
    writer
        .write_all(b"x")
        .expect("ready peer byte should be written");
    super::wait_for_fd_event(reader.as_raw_fd(), libc::POLLIN, None, None)
        .expect("ready fd should not need a deadline to wake");

    let cancelled_poll = run_lightweight_root_task(|| {
        let group = TaskGroupValue::new(&CancellationContext::default());
        let cancellation = group.child_cancellation();
        group.cancel();
        let (_writer, reader) =
            std::os::unix::net::UnixStream::pair().expect("unix stream pair should be available");
        let error =
            super::wait_for_fd_event(reader.as_raw_fd(), libc::POLLIN, None, Some(&cancellation))
                .expect_err("cancelled lightweight fd waits should return promptly");
        assert_eq!(error.kind(), io::ErrorKind::Interrupted);
        Ok(Value::Bool(true))
    });
    assert_eq!(
        cancelled_poll.expect("cancelled fd poll root should complete"),
        Value::Bool(true)
    );

    super::set_fd_nonblocking(-1, true).expect_err("bad fds should report fcntl errors");
}

#[test]
fn render_float_formats_current_surface() {
    assert_eq!(render_float(42.0), "42.0");
    assert_eq!(render_float(3.5), "3.5");
    assert_eq!(render_float(f64::INFINITY), "inf");
    assert_eq!(render_float(9_007_199_254_740_992.0), "9007199254740992.0");
    assert_eq!(render_float(1e300), "1e300");
    assert_eq!(render_float(1e-300), "1e-300");
    assert_eq!(render_float(-0.0), "-0.0");
    assert_eq!(render_float(0.1 + 0.2), "0.30000000000000004");

    assert_eq!(render_float32(3.14), "3.14");
    assert_eq!(render_float32(-0.0), "-0.0");
}

#[test]
fn float_floor_divmod_matches_python_sign_precision_and_zero_rules() {
    for (left, right, quotient, remainder) in [
        (7.5, 2.0, 3.0, 1.5),
        (-7.5, 2.0, -4.0, 0.5),
        (7.5, -2.0, -4.0, -0.5),
        (-7.5, -2.0, 3.0, -1.5),
        (1.0, 0.1, 9.0, 0.099_999_999_999_999_95),
        (
            5e-300,
            1.300_000_000_000_000_1e-300,
            3.0,
            1.099_999_999_999_999_5e-300,
        ),
        (1e308, 3.0, 3.333_333_333_333_333e307, 2.0),
    ] {
        let (actual_quotient, actual_remainder) = float_floor_divmod(left, right);
        assert_eq!(actual_quotient, quotient);
        assert_eq!(actual_remainder, remainder);
    }

    let (negative_zero_quotient, negative_zero_remainder) = float_floor_divmod(0.0, -3.0);
    assert_eq!(negative_zero_quotient.to_bits(), (-0.0_f64).to_bits());
    assert_eq!(negative_zero_remainder.to_bits(), (-0.0_f64).to_bits());
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
fn async_and_process_result_helpers_render_expected_variants() {
    fn assert_variant(
        value: Value,
        enum_name: &str,
        variant_name: &str,
        expected_payloads: Vec<Value>,
    ) {
        let Value::EnumVariant(variant) = value else {
            panic!("expected {enum_name}.{variant_name} to render as an enum variant");
        };
        assert_eq!(variant.enum_name, enum_name);
        assert_eq!(variant.variant_name, variant_name);
        assert_eq!(variant.payloads, expected_payloads);
    }

    let payload = Value::Int(IntegerValue::from_signed(5));
    assert_variant(
        send_error_timed_out(payload.clone()),
        "SendError",
        "TimedOut",
        vec![payload.clone()],
    );
    assert_variant(
        send_error_full(payload.clone()),
        "SendError",
        "Full",
        vec![payload.clone()],
    );

    assert_variant(
        queue_receive_item(payload.clone()),
        "QueueReceive",
        "Item",
        vec![payload.clone()],
    );
    assert_variant(queue_receive_closed(), "QueueReceive", "Closed", Vec::new());
    assert_variant(
        queue_receive_timed_out(),
        "QueueReceive",
        "TimedOut",
        Vec::new(),
    );
    assert_variant(
        queue_receive_cancelled(),
        "QueueReceive",
        "Cancelled",
        Vec::new(),
    );

    let ready = Value::String("done".to_string());
    assert_variant(
        task_result_ready(ready.clone()),
        "TaskResult",
        "Ready",
        vec![ready.clone()],
    );
    assert_variant(
        task_result_error("boom".to_string()),
        "TaskResult",
        "Error",
        vec![Value::String("boom".to_string())],
    );
    assert_variant(
        task_result_timed_out(),
        "TaskResult",
        "TimedOut",
        Vec::new(),
    );
    assert_variant(
        task_result_cancelled(),
        "TaskResult",
        "Cancelled",
        Vec::new(),
    );

    assert_variant(
        wait_any_ready(2, ready.clone()),
        "WaitAny",
        "Ready",
        vec![Value::Int(IntegerValue::from_signed(2)), ready.clone()],
    );
    assert_variant(
        wait_any_error(3, "failed".to_string()),
        "WaitAny",
        "Error",
        vec![
            Value::Int(IntegerValue::from_signed(3)),
            Value::String("failed".to_string()),
        ],
    );
    assert_variant(wait_any_timed_out(), "WaitAny", "TimedOut", Vec::new());
    assert_variant(wait_any_cancelled(), "WaitAny", "Cancelled", Vec::new());

    let all_ready = vec![payload.clone(), ready.clone()];
    assert_variant(
        wait_all_ready(all_ready.clone()),
        "WaitAll",
        "Ready",
        vec![Value::Vec(VecValue {
            element_type: Type::named("Unknown"),
            elements: all_ready,
        })],
    );
    assert_variant(
        wait_all_error(4, "bad".to_string()),
        "WaitAll",
        "Error",
        vec![
            Value::Int(IntegerValue::from_signed(4)),
            Value::String("bad".to_string()),
        ],
    );
    assert_variant(wait_all_timed_out(), "WaitAll", "TimedOut", Vec::new());
    assert_variant(wait_all_cancelled(), "WaitAll", "Cancelled", Vec::new());

    assert_variant(process_wait_timed_out(), "Wait", "TimedOut", Vec::new());
    assert_variant(process_wait_cancelled(), "Wait", "Cancelled", Vec::new());

    let spawn_error = process_error_spawn("missing executable".to_string());
    assert_variant(
        spawn_error.clone(),
        "Error",
        "Spawn",
        vec![Value::String("missing executable".to_string())],
    );
    assert_variant(
        process_wait_failed(spawn_error.clone()),
        "Wait",
        "Failed",
        vec![spawn_error],
    );

    let event_error = process_error_other("crash".to_string());
    let supervisor_event = process_supervisor_event_failed(
        "worker".to_string(),
        event_error.clone(),
        IntegerValue::from_signed(2),
    );
    assert_variant(
        supervisor_event.clone(),
        "SupervisorEvent",
        "Failed",
        vec![
            Value::String("worker".to_string()),
            event_error,
            Value::Int(IntegerValue::from_signed(2)),
        ],
    );
    assert_variant(
        process_supervisor_wait_event(supervisor_event.clone()),
        "SupervisorWait",
        "Event",
        vec![supervisor_event],
    );
    assert_variant(
        process_supervisor_wait_timed_out(),
        "SupervisorWait",
        "TimedOut",
        Vec::new(),
    );
    assert_variant(
        process_supervisor_wait_cancelled(),
        "SupervisorWait",
        "Cancelled",
        Vec::new(),
    );

    assert_variant(process_error_no_command(), "Error", "NoCommand", Vec::new());
    assert_variant(process_error_timed_out(), "Error", "TimedOut", Vec::new());
    assert_variant(process_error_cancelled(), "Error", "Cancelled", Vec::new());
}

#[test]
fn process_config_decoders_report_unknown_and_wrong_variants() {
    let stdio_variant = |variant_name: &str| {
        Value::EnumVariant(EnumVariantValue {
            enum_name: "process.Stdio".to_string(),
            variant_name: variant_name.to_string(),
            payloads: Vec::new(),
        })
    };
    assert!(matches!(
        decode_process_stdio(&stdio_variant("Inherit"), "stdin").expect("Inherit should decode"),
        ProcessStdioConfig::Inherit
    ));
    assert!(matches!(
        decode_process_stdio(&stdio_variant("Null"), "stdin").expect("Null should decode"),
        ProcessStdioConfig::Null
    ));
    assert!(matches!(
        decode_process_stdio(&stdio_variant("Pipe"), "stdout").expect("Pipe should decode"),
        ProcessStdioConfig::Pipe
    ));
    let unknown_stdio = match decode_process_stdio(&stdio_variant("Bogus"), "stdin") {
        Ok(_) => panic!("unknown stdio variants should fail"),
        Err(error) => error,
    };
    assert!(unknown_stdio
        .message
        .contains("unknown `process.Stdio` variant"));
    let wrong_stdio = match decode_process_stdio(&Value::Bool(true), "stdin") {
        Ok(_) => panic!("wrong stdio values should fail"),
        Err(error) => error,
    };
    assert!(wrong_stdio.message.contains("expects `process.Stdio`"));

    let restart_variant = |variant_name: &str| {
        Value::EnumVariant(EnumVariantValue {
            enum_name: "process.RestartPolicy".to_string(),
            variant_name: variant_name.to_string(),
            payloads: Vec::new(),
        })
    };
    assert_eq!(
        decode_process_restart_policy(&restart_variant("Never"), "restart")
            .expect("Never should decode"),
        ProcessRestartPolicy::Never
    );
    assert_eq!(
        decode_process_restart_policy(&restart_variant("OnFailure"), "restart")
            .expect("OnFailure should decode"),
        ProcessRestartPolicy::OnFailure
    );
    assert_eq!(
        decode_process_restart_policy(&restart_variant("Always"), "restart")
            .expect("Always should decode"),
        ProcessRestartPolicy::Always
    );
    assert!(
        decode_process_restart_policy(&restart_variant("Bogus"), "restart")
            .expect_err("unknown restart variants should fail")
            .message
            .contains("unknown `process.RestartPolicy` variant")
    );
    assert!(
        decode_process_restart_policy(&Value::String("Always".to_string()), "restart")
            .expect_err("wrong restart values should fail")
            .message
            .contains("expects `process.RestartPolicy`")
    );
}

#[test]
fn cast_numeric_value_covers_success_and_failure_paths() {
    assert_eq!(
        cast_numeric_value(
            Value::Int(IntegerValue::from_signed(5)),
            &Type::named("int8"),
            None
        )
        .expect("integer-to-integer cast should preserve in-range values"),
        Value::Int(IntegerValue::from_signed(5))
    );

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

    let integer_to_non_numeric = cast_numeric_value(
        Value::Int(IntegerValue::from_signed(5)),
        &Type::named("String"),
        None,
    )
    .expect_err("integer casts to nonnumeric targets should fail");
    assert!(
        integer_to_non_numeric
            .message
            .contains("found `integer` and `String`"),
        "unexpected integer cast diagnostic: {}",
        integer_to_non_numeric.message
    );

    let float_to_non_numeric = cast_numeric_value(Value::Float(1.5), &Type::named("String"), None)
        .expect_err("float casts to nonnumeric targets should fail");
    assert!(float_to_non_numeric
        .message
        .contains("found `float64` and `String`"));

    let non_finite = cast_numeric_value(Value::Float(f64::INFINITY), &Type::named("int32"), None)
        .expect_err("non-finite float casts to integers should fail");
    assert!(non_finite
        .message
        .contains("cannot cast non-finite float to `int32`"));

    let unsigned_negative = cast_numeric_value(Value::Float(-1.0), &Type::named("uint8"), None)
        .expect_err("negative float casts to unsigned integers should fail");
    assert!(unsigned_negative
        .message
        .contains("does not fit in `uint8`"));

    assert_eq!(
        cast_numeric_value(Value::Float(42.0), &Type::named("uint8"), None)
            .expect("float to unsigned integer cast should succeed"),
        Value::Int(IntegerValue::from_literal(42))
    );

    let unsigned_rounding_overflow =
        cast_numeric_value(Value::Float(u64::MAX as f64), &Type::named("uint64"), None)
            .expect_err("rounded float casts outside uint64 range should fail");
    assert!(unsigned_rounding_overflow
        .message
        .contains("does not fit in `uint64`"));

    assert_eq!(
        cast_numeric_value(Value::Float(3.25), &Type::named("float32"), None)
            .expect("float64 to float32 cast should succeed"),
        Value::Float((3.25f32) as f64)
    );
    assert_eq!(
        cast_numeric_value(Value::Float(3.25), &Type::named("float64"), None)
            .expect("float64 to float64 cast should succeed"),
        Value::Float(3.25)
    );

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
fn cast_numeric_value_reports_source_types_for_runtime_values() {
    fn assert_source_type(value: Value, expected_source: &str) {
        let error = cast_numeric_value(value, &Type::named("int32"), None)
            .expect_err("non-numeric runtime values should not cast to integers");
        assert!(
            error
                .message
                .contains(&format!("found `{expected_source}` and `int32`")),
            "unexpected diagnostic for {expected_source}: {}",
            error.message
        );
    }

    assert_source_type(Value::Bool(true), "bool");
    assert_source_type(Value::String("Aurora".to_string()), "String");
    assert_source_type(
        Value::Vec(VecValue {
            element_type: Type::named("int32"),
            elements: vec![],
        }),
        "Vec",
    );
    assert_source_type(
        Value::Set(SetValue {
            element_type: Type::named("String"),
            elements: vec![],
        }),
        "Set",
    );
    assert_source_type(
        Value::Map(MapValue {
            key_type: Type::named("String"),
            value_type: Type::named("int32"),
            entries: vec![],
        }),
        "Map",
    );
    assert_source_type(Value::Duration(5), "Duration");
    assert_source_type(Value::Range(RangeValue { start: 1, end: 3 }), "Range");
    assert_source_type(
        Value::ModuleNamespace(ModuleNamespaceValue {
            path: "pkg.tools".to_string(),
        }),
        "module pkg.tools",
    );
    assert_source_type(Value::Unit, "None");
    assert_source_type(
        Value::Instance(super::InstanceValue {
            class_name: "Widget".to_string(),
            fields: Default::default(),
        }),
        "Widget",
    );
    assert_source_type(
        Value::EnumVariant(EnumVariantValue {
            enum_name: "Status".to_string(),
            variant_name: "Ready".to_string(),
            payloads: vec![],
        }),
        "Status",
    );

    let channel = ChannelValue::new();
    assert_eq!(format!("{channel:?}"), "ChannelValue(..)");
    assert_eq!(channel, channel.clone());
    assert_value_equals_clone(Value::Channel(channel.clone()));
    assert_eq!(Value::Channel(channel.clone()).render(), "<queue>");
    assert_source_type(Value::Channel(channel.clone()), "Queue");

    let task = TaskValue::from_handle(thread::spawn(|| Ok(Value::Unit)));
    assert_eq!(format!("{task:?}"), "TaskValue(..)");
    assert_eq!(task, task.clone());
    assert_value_equals_clone(Value::Task(task.clone()));
    assert_eq!(Value::Task(task.clone()).render(), "<task>");
    assert_source_type(Value::Task(task.clone()), "Task");
    wait_task_ready(&task).expect("test task should complete");

    let cancellation = CancellationContext::default();
    let task_group = TaskGroupValue::new(&cancellation);
    assert_eq!(format!("{task_group:?}"), "TaskGroupValue(..)");
    assert_eq!(task_group, task_group.clone());
    assert_value_equals_clone(Value::TaskGroup(task_group.clone()));
    assert_eq!(Value::TaskGroup(task_group.clone()).render(), "<tasks>");
    assert_source_type(Value::TaskGroup(task_group.clone()), "TaskGroup");

    let temp = TempDir::new("aurora-runtime-value-cast");
    let file_path = temp.path().join("data.txt");
    let file = FileValue::create(file_path.to_str().expect("utf8 temp path"))
        .expect("temp file should be created");
    assert_eq!(format!("{file:?}"), "FileValue(..)");
    assert_eq!(file, file.clone());
    assert_value_equals_clone(Value::File(file.clone()));
    assert_eq!(Value::File(file.clone()).render(), "<file>");
    assert_source_type(Value::File(file.clone()), "fs.File");
    file.close();

    let tcp_listener =
        TcpListenerValue::bind("127.0.0.1:0").expect("tcp listener should bind locally");
    assert_eq!(format!("{tcp_listener:?}"), "TcpListenerValue(..)");
    assert_eq!(tcp_listener, tcp_listener.clone());
    let tcp_address = tcp_listener
        .local_addr()
        .expect("tcp listener should expose a local address");
    let accept_listener = tcp_listener.clone();
    let accept_thread = thread::spawn(move || {
        accept_listener
            .accept(Some(StdDuration::from_secs(1)), None)
            .expect("tcp listener should accept local client")
    });
    let tcp_stream = TcpStreamValue::connect(&tcp_address, Some(StdDuration::from_secs(1)), None)
        .expect("tcp stream should connect locally");
    let accepted_stream = accept_thread
        .join()
        .expect("tcp accept worker should join successfully");
    assert_eq!(format!("{tcp_stream:?}"), "TcpStreamValue(..)");
    assert_eq!(tcp_stream, tcp_stream.clone());
    assert_value_equals_clone(Value::TcpListener(tcp_listener.clone()));
    assert_value_equals_clone(Value::TcpStream(tcp_stream.clone()));
    assert_eq!(
        Value::TcpListener(tcp_listener.clone()).render(),
        "<tcp-listener>"
    );
    assert_eq!(
        Value::TcpStream(tcp_stream.clone()).render(),
        "<tcp-stream>"
    );
    assert_source_type(Value::TcpListener(tcp_listener.clone()), "net.TcpListener");
    assert_source_type(Value::TcpStream(tcp_stream.clone()), "net.TcpStream");
    tcp_stream.close();
    accepted_stream.close();
    tcp_listener.close();

    let udp_socket = UdpSocketValue::bind("127.0.0.1:0").expect("udp socket should bind locally");
    assert_eq!(format!("{udp_socket:?}"), "UdpSocketValue(..)");
    assert_eq!(udp_socket, udp_socket.clone());
    assert_value_equals_clone(Value::UdpSocket(udp_socket.clone()));
    assert_eq!(
        Value::UdpSocket(udp_socket.clone()).render(),
        "<udp-socket>"
    );
    assert_source_type(Value::UdpSocket(udp_socket.clone()), "net.UdpSocket");
    udp_socket.close();
    assert_value_equals_clone(Value::UdpDatagram(UdpDatagramValue {
        address: "127.0.0.1:9".to_string(),
        data: vec![1, 2, 3],
    }));
    assert_eq!(
        Value::UdpDatagram(UdpDatagramValue {
            address: "127.0.0.1:9".to_string(),
            data: vec![1, 2, 3],
        })
        .render(),
        "<udp-datagram 127.0.0.1:9 3 bytes>"
    );
    assert_source_type(
        Value::UdpDatagram(UdpDatagramValue {
            address: "127.0.0.1:9".to_string(),
            data: vec![1, 2, 3],
        }),
        "net.UdpDatagram",
    );
    assert_eq!(
        Value::HttpResponse(HttpResponseValue {
            status: 200,
            reason: "OK".to_string(),
            headers: vec![],
            body: vec![1, 2],
        })
        .render(),
        "<http-response 200 2 bytes>"
    );
    assert_value_equals_clone(Value::HttpResponse(HttpResponseValue {
        status: 200,
        reason: "OK".to_string(),
        headers: vec![],
        body: vec![1, 2],
    }));
    assert_source_type(
        Value::HttpResponse(HttpResponseValue {
            status: 200,
            reason: "OK".to_string(),
            headers: vec![],
            body: vec![],
        }),
        "net.HttpResponse",
    );

    let http_listener =
        HttpListenerValue::bind("127.0.0.1:0").expect("http listener should bind locally");
    assert_eq!(format!("{http_listener:?}"), "HttpListenerValue(..)");
    assert_eq!(http_listener, http_listener.clone());
    assert_value_equals_clone(Value::HttpListener(http_listener.clone()));
    assert_eq!(
        Value::HttpListener(http_listener.clone()).render(),
        "<http-listener>"
    );
    assert_source_type(
        Value::HttpListener(http_listener.clone()),
        "net.HttpListener",
    );
    http_listener.close();

    let websocket_listener = WebSocketListenerValue::bind("127.0.0.1:0")
        .expect("websocket listener should bind locally");
    assert_eq!(
        format!("{websocket_listener:?}"),
        "WebSocketListenerValue(..)"
    );
    assert_eq!(websocket_listener, websocket_listener.clone());
    assert_value_equals_clone(Value::WebSocketListener(websocket_listener.clone()));
    assert_eq!(
        Value::WebSocketListener(websocket_listener.clone()).render(),
        "<websocket-listener>"
    );
    assert_source_type(
        Value::WebSocketListener(websocket_listener.clone()),
        "net.WebSocketListener",
    );

    let completed = ProcessCompletedValue::new(
        Value::EnumVariant(EnumVariantValue {
            enum_name: "process.ExitStatus".to_string(),
            variant_name: "Exited".to_string(),
            payloads: vec![Value::Int(IntegerValue::from_signed(0))],
        }),
        vec![],
        vec![],
    );
    assert_eq!(format!("{completed:?}"), "ProcessCompletedValue(..)");
    assert_eq!(completed, completed.clone());
    assert_value_equals_clone(Value::ProcessCompleted(completed.clone()));
    assert_eq!(
        Value::ProcessCompleted(completed.clone()).render(),
        "<process-completed process.ExitStatus.Exited(0)>"
    );
    let failed_completed = ProcessCompletedValue::new(
        Value::EnumVariant(EnumVariantValue {
            enum_name: "process.ExitStatus".to_string(),
            variant_name: "Exited".to_string(),
            payloads: vec![Value::Int(IntegerValue::from_signed(2))],
        }),
        vec![],
        vec![],
    );
    assert!(failed_completed
        .check()
        .expect_err("non-zero completed processes should fail check()")
        .render()
        .contains("process exited with process.ExitStatus.Exited(2)"));
    assert_source_type(Value::ProcessCompleted(completed), "process.Completed");

    let child = ProcessChildValue::spawn(
        vec![
            std::env::current_exe()
                .expect("current test binary should be available")
                .to_string_lossy()
                .into_owned(),
            "--help".to_string(),
        ],
        None,
        Vec::new(),
        ProcessStdioConfig::Null,
        ProcessStdioConfig::Pipe,
        ProcessStdioConfig::Null,
        false,
    )
    .expect("process child should spawn");
    let stdout_pipe = child.stdout().expect("stdout pipe should be captured");
    assert_eq!(format!("{child:?}"), "ProcessChildValue(..)");
    assert_eq!(child, child.clone());
    assert_value_equals_clone(Value::ProcessChild(child.clone()));
    assert_eq!(
        Value::ProcessChild(child.clone()).render(),
        "<process-child>"
    );
    assert_source_type(Value::ProcessChild(child.clone()), "process.Child");
    assert_eq!(format!("{stdout_pipe:?}"), "ProcessPipeValue(..)");
    assert_eq!(stdout_pipe, stdout_pipe.clone());
    assert_value_equals_clone(Value::ProcessPipe(stdout_pipe.clone()));
    assert_eq!(
        Value::ProcessPipe(stdout_pipe.clone()).render(),
        "<process-pipe>"
    );
    assert_source_type(Value::ProcessPipe(stdout_pipe.clone()), "process.Pipe");
    let _ = child.wait(Some(StdDuration::from_secs(1)), None);
    child.close();

    let failed_child = ProcessChildValue::spawn(
        vec![
            "/bin/sh".to_string(),
            "-c".to_string(),
            "exit 3".to_string(),
        ],
        None,
        Vec::new(),
        ProcessStdioConfig::Null,
        ProcessStdioConfig::Null,
        ProcessStdioConfig::Null,
        false,
    )
    .expect("failing child should spawn");
    let error = failed_child
        .wait_ok(Some(StdDuration::from_secs(2)), None)
        .expect_err("wait_ok should reject non-zero exits");
    assert!(error.render().contains("ExitStatus.Exited(3)"));

    let timed_out_child = ProcessChildValue::spawn(
        vec![
            "/bin/sh".to_string(),
            "-c".to_string(),
            "sleep 1".to_string(),
        ],
        None,
        Vec::new(),
        ProcessStdioConfig::Null,
        ProcessStdioConfig::Null,
        ProcessStdioConfig::Null,
        false,
    )
    .expect("sleep child should spawn");
    let error = timed_out_child
        .wait_ok(Some(StdDuration::ZERO), None)
        .expect_err("wait_ok should surface timeouts");
    assert_eq!(error.render(), "Error.TimedOut");
    timed_out_child.close();

    let cancelled_child = ProcessChildValue::spawn(
        vec![
            "/bin/sh".to_string(),
            "-c".to_string(),
            "sleep 1".to_string(),
        ],
        None,
        Vec::new(),
        ProcessStdioConfig::Null,
        ProcessStdioConfig::Null,
        ProcessStdioConfig::Null,
        false,
    )
    .expect("cancellable child should spawn");
    let cancel_group = TaskGroupValue::new(&CancellationContext::default());
    let cancellation = cancel_group.child_cancellation();
    cancel_group.cancel();
    let error = cancelled_child
        .wait_ok(Some(StdDuration::from_secs(2)), Some(&cancellation))
        .expect_err("wait_ok should surface cancellation");
    assert_eq!(error.render(), "Error.Cancelled");
    cancelled_child.close();

    #[cfg(unix)]
    {
        let grouped_child = ProcessChildValue::spawn(
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
            true,
        )
        .expect("grouped sleep child should spawn");
        grouped_child.close();
    }

    let supervisor = ProcessSupervisorValue::new();
    assert_eq!(format!("{supervisor:?}"), "ProcessSupervisorValue(..)");
    assert_eq!(supervisor, supervisor.clone());
    assert_value_equals_clone(Value::ProcessSupervisor(supervisor.clone()));
    assert_eq!(
        Value::ProcessSupervisor(supervisor.clone()).render(),
        "<process-supervisor>"
    );
    assert_source_type(Value::ProcessSupervisor(supervisor), "process.Supervisor");

    #[cfg(unix)]
    {
        let unix_socket_path = unique_unix_socket_path("a-rv");
        let unix_path = unix_socket_path
            .to_str()
            .expect("unix socket path should be utf8");
        let unix_listener =
            UnixListenerValue::bind(unix_path).expect("unix listener should bind locally");
        assert_eq!(format!("{unix_listener:?}"), "UnixListenerValue(..)");
        assert_eq!(unix_listener, unix_listener.clone());
        assert_value_equals_clone(Value::UnixListener(unix_listener.clone()));
        assert_eq!(
            Value::UnixListener(unix_listener.clone()).render(),
            "<unix-listener>"
        );
        assert_source_type(
            Value::UnixListener(unix_listener.clone()),
            "net.UnixListener",
        );
        let unix_server = unix_listener.clone();
        let unix_accept = thread::spawn(move || {
            unix_server
                .accept(Some(StdDuration::from_secs(1)), None)
                .expect("unix listener should accept local client")
        });
        let unix_client = UnixStreamValue::connect(
            unix_path,
            Some(StdDuration::from_secs(1)),
            Some(&CancellationContext::default()),
        )
        .expect("unix stream should connect locally");
        let unix_stream = unix_accept
            .join()
            .expect("unix accept worker should join successfully");
        assert_eq!(format!("{unix_client:?}"), "UnixStreamValue(..)");
        assert_eq!(unix_client, unix_client.clone());
        assert_value_equals_clone(Value::UnixStream(unix_client.clone()));
        assert_eq!(
            Value::UnixStream(unix_client.clone()).render(),
            "<unix-stream>"
        );
        assert_source_type(Value::UnixStream(unix_client.clone()), "net.UnixStream");
        unix_client.close();
        unix_stream.close();
        unix_listener.close();
        let _ = fs::remove_file(unix_socket_path);
    }
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

    let bounded = ChannelValue::with_capacity(1);
    bounded
        .send(Value::Unit)
        .expect("bounded channel should accept one queued value");
    assert_eq!(
        bounded
            .try_send_result(Value::Bool(false))
            .expect_err("full bounded channel should reject try_send_result"),
        super::SendValueError::Full(Box::new(Value::Bool(false)))
    );
    assert_eq!(
        bounded
            .send_with_deadline(Value::Bool(true), None, None, true)
            .expect_err("fail-fast bounded sends should report full"),
        super::SendValueError::Full(Box::new(Value::Bool(true)))
    );
    assert_eq!(
        bounded
            .send_with_timeout(Value::Bool(true), Some(StdDuration::ZERO), None)
            .expect_err("timed bounded sends should report timeout when capacity stays full"),
        super::SendValueError::TimedOut(Box::new(Value::Bool(true)))
    );
    let cancel_flag = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(true));
    let cancelled = CancellationContext {
        flags: vec![cancel_flag],
    };
    assert_eq!(
        bounded
            .send_with_cancellation(Value::Bool(true), Some(&cancelled))
            .expect_err("cancelled bounded sends should report cancellation"),
        super::SendValueError::Cancelled(Box::new(Value::Bool(true)))
    );

    let producer_channel = ChannelValue::new();
    let producer = TaskValue::from_handle(thread::spawn(|| Ok(Value::Unit)));
    producer_channel.register_producer_task(&producer);
    assert_eq!(
        wait_task_ready(&producer).expect("producer task should complete"),
        Value::Unit
    );
    drop(producer);
    for _ in 0..100 {
        if producer_channel.registered_producer_tasks().is_empty() {
            break;
        }
        thread::sleep(StdDuration::from_millis(1));
    }
    assert!(producer_channel.registered_producer_tasks().is_empty());
    assert!(producer_channel.all_registered_producer_tasks_completed());

    assert_eq!(
        channel
            .try_send_result(Value::Bool(false))
            .expect_err("closed channel should reject try_send_result"),
        super::SendValueError::Closed(Box::new(Value::Bool(false)))
    );
    for error in [
        super::SendValueError::Closed(Box::new(Value::Int(IntegerValue::from_signed(1)))),
        super::SendValueError::Cancelled(Box::new(Value::Int(IntegerValue::from_signed(2)))),
        super::SendValueError::TimedOut(Box::new(Value::Int(IntegerValue::from_signed(3)))),
        super::SendValueError::Full(Box::new(Value::Int(IntegerValue::from_signed(4)))),
    ] {
        let value = *error.clone().into_value();
        let expected = match error {
            super::SendValueError::Closed(value)
            | super::SendValueError::Cancelled(value)
            | super::SendValueError::TimedOut(value)
            | super::SendValueError::Full(value) => *value,
        };
        assert_eq!(value, expected);
    }
}

#[test]
fn host_control_plane_builtins_cover_success_and_error_boundaries() {
    fn call(name: &str, args: Vec<Value>) -> Value {
        super::evaluate_host_builtin(name, args)
            .unwrap_or_else(|error| panic!("{name} should succeed: {error}"))
    }
    fn string_map(entries: &[(&str, &str)]) -> Value {
        Value::Map(MapValue {
            key_type: Type::named("String"),
            value_type: Type::named("String"),
            entries: entries
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

    let previous_program_args = std::env::var_os("AURORA_PROGRAM_ARGS_JSON");
    std::env::set_var("AURORA_PROGRAM_ARGS_JSON", "[\"spoofed\"]");
    let Value::Vec(actual_args) = call("sys::args", vec![]) else {
        panic!("sys.args should return a Vec");
    };
    assert_eq!(
        actual_args.elements,
        super::host_process_args()
            .into_iter()
            .map(Value::String)
            .collect::<Vec<_>>(),
        "the retired environment transport must not override real host argv"
    );
    let explicit_args = vec!["alpha".to_string(), "beta".to_string()];
    let Value::Vec(explicit) =
        super::evaluate_host_builtin_with_program_args("sys::args", vec![], &explicit_args)
            .expect("an explicit MIR argv context should be accepted")
    else {
        panic!("sys.args should return a Vec");
    };
    assert_eq!(
        explicit.elements,
        vec![
            Value::String("alpha".to_string()),
            Value::String("beta".to_string())
        ]
    );
    match previous_program_args {
        Some(value) => std::env::set_var("AURORA_PROGRAM_ARGS_JSON", value),
        None => std::env::remove_var("AURORA_PROGRAM_ARGS_JSON"),
    }
    assert_eq!(
        call(
            "sys::env",
            vec![Value::String(
                "AURORA_TEST_ENV_THAT_DOES_NOT_EXIST_52D3".to_string()
            )]
        )
        .render(),
        "Option.None"
    );
    assert!(call("sys::current_dir", vec![])
        .render()
        .starts_with("Result.Ok("));
    assert!(matches!(call("sys::unix_time_ms", vec![]), Value::Int(_)));
    assert!(matches!(
        call("sys::monotonic_time_ms", vec![]),
        Value::Int(_)
    ));
    assert!(super::host_millis_value(u128::MAX, "test clock").is_err());

    assert_eq!(
        call(
            "path::join",
            vec![Value::String("a".into()), Value::String("b".into())]
        ),
        Value::String(
            std::path::Path::new("a")
                .join("b")
                .to_string_lossy()
                .to_string()
        )
    );
    assert_eq!(
        call("path::parent", vec![Value::String("a/b".into())]).render(),
        "Option.Some(a)"
    );
    assert_eq!(
        call("path::file_name", vec![Value::String("a/b.au".into())]).render(),
        "Option.Some(b.au)"
    );
    assert_eq!(
        call("path::extension", vec![Value::String("a/b.au".into())]).render(),
        "Option.Some(au)"
    );
    assert_eq!(
        call(
            "path::extension",
            vec![Value::String("no-extension".into())]
        )
        .render(),
        "Option.None"
    );
    assert_eq!(
        call("path::is_absolute", vec![Value::String("relative".into())]),
        Value::Bool(false)
    );

    let labels = string_map(&[("name", "aurora")]);
    assert_eq!(
        call("json::is_valid", vec![Value::String("[]".into())]),
        Value::Bool(true)
    );
    assert_eq!(
        call("json::is_valid", vec![Value::String("{".into())]),
        Value::Bool(false)
    );
    assert_eq!(
        call("json::stringify_map", vec![labels.clone()]).render(),
        "Result.Ok({\"name\":\"aurora\"})"
    );
    assert!(call(
        "json::parse_string_map",
        vec![Value::String("{\"name\":\"aurora\"}".into())]
    )
    .render()
    .starts_with("Result.Ok("));
    assert!(
        call("json::parse_string_map", vec![Value::String("[]".into())])
            .render()
            .starts_with("Result.Err(")
    );
    assert_eq!(
        call(
            "toml::is_valid",
            vec![Value::String("name = \"aurora\"".into())]
        ),
        Value::Bool(true)
    );
    assert_eq!(
        call("toml::is_valid", vec![Value::String("name =".into())]),
        Value::Bool(false)
    );
    assert!(call("toml::stringify_map", vec![labels.clone()])
        .render()
        .starts_with("Result.Ok("));
    assert!(call(
        "toml::parse_string_map",
        vec![Value::String("name = \"aurora\"".into())]
    )
    .render()
    .starts_with("Result.Ok("));
    assert!(call(
        "toml::parse_string_map",
        vec![Value::String("name = [1]".into())]
    )
    .render()
    .starts_with("Result.Err("));

    call("metrics::reset", vec![]);
    call(
        "metrics::increment",
        vec![
            Value::String("jobs".into()),
            Value::Int(IntegerValue::from_signed(2)),
        ],
    );
    assert_eq!(
        call("metrics::get", vec![Value::String("jobs".into())]),
        Value::Int(IntegerValue::from_signed(2))
    );
    assert_eq!(
        call("metrics::get", vec![Value::String("missing".into())]),
        Value::Int(IntegerValue::zero())
    );
    for level in ["debug", "info", "warn", "error"] {
        assert_eq!(
            call(
                &format!("log::{level}"),
                vec![Value::String("ready".into()), labels.clone()]
            ),
            Value::Unit
        );
    }
    assert_eq!(
        call(
            "trace::event",
            vec![Value::String("boot".into()), labels.clone()]
        ),
        Value::Unit
    );

    assert!(super::evaluate_host_builtin("sys::args", vec![Value::Unit]).is_err());
    assert!(super::evaluate_host_builtin("sys::env", vec![Value::Bool(true)]).is_err());
    assert!(super::evaluate_host_builtin("json::stringify_map", vec![Value::Unit]).is_err());
    assert!(super::evaluate_host_builtin(
        "metrics::increment",
        vec![Value::String("x".into()), Value::Unit]
    )
    .is_err());
    assert!(super::evaluate_host_builtin(
        "metrics::increment",
        vec![
            Value::String("x".into()),
            Value::Int(IntegerValue::from_literal(u128::MAX)),
        ]
    )
    .is_err());
    call("metrics::reset", vec![]);
    call(
        "metrics::increment",
        vec![
            Value::String("overflow".into()),
            Value::Int(IntegerValue::from_signed(i128::from(i64::MAX))),
        ],
    );
    assert!(super::evaluate_host_builtin(
        "metrics::increment",
        vec![
            Value::String("overflow".into()),
            Value::Int(IntegerValue::from_signed(1)),
        ]
    )
    .is_err());
    assert_eq!(
        call("metrics::get", vec![Value::String("overflow".into())]),
        Value::Int(IntegerValue::from_signed(i128::from(i64::MAX)))
    );
    assert!(super::evaluate_host_builtin(
        "metrics::increment",
        vec![
            Value::String("outside-int64".into()),
            Value::Int(IntegerValue::from_signed(i128::from(i64::MAX) + 1)),
        ]
    )
    .is_err());
    call(
        "metrics::increment",
        vec![
            Value::String("underflow".into()),
            Value::Int(IntegerValue::from_signed(i128::from(i64::MIN))),
        ],
    );
    assert!(super::evaluate_host_builtin(
        "metrics::increment",
        vec![
            Value::String("underflow".into()),
            Value::Int(IntegerValue::from_signed(-1)),
        ]
    )
    .is_err());
    assert_eq!(
        call("metrics::get", vec![Value::String("underflow".into())]),
        Value::Int(IntegerValue::from_signed(i128::from(i64::MIN)))
    );
    assert!(super::evaluate_host_builtin("missing::call", vec![]).is_err());
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
        wait_task_ready(&task).expect("first wait should succeed"),
        Value::Int(IntegerValue::from_signed(9))
    );
    assert_eq!(
        wait_task_ready(&task).expect("cached wait should also succeed"),
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

    let inactive_spawn = spawn_lightweight_task(|| Ok(Value::Unit))
        .expect_err("spawning outside a lightweight scheduler should fail");
    assert!(inactive_spawn
        .message
        .contains("requires an active task scheduler"));
    let inactive_cancellable_spawn =
        spawn_lightweight_task_with_cancellation(CancellationContext::default(), || {
            Ok(Value::Unit)
        })
        .expect_err("cancellable spawning outside a lightweight scheduler should fail");
    assert!(inactive_cancellable_spawn
        .message
        .contains("requires an active task scheduler"));
}

#[test]
fn task_group_wake_flags_cover_already_completed_and_duplicate_registrations() {
    let completed = TaskValue::from_handle(thread::spawn(|| Ok(Value::Unit)));
    assert_eq!(
        wait_task_ready(&completed).expect("completed task should be ready"),
        Value::Unit
    );

    let completion_flag = Arc::new(AtomicBool::new(false));
    completed.register_group_completion_wake_flag(completion_flag.clone());
    assert!(completion_flag.load(Ordering::SeqCst));
    completion_flag.store(false, Ordering::SeqCst);
    completed.register_group_completion_wake_flag(completion_flag.clone());
    assert!(completion_flag.load(Ordering::SeqCst));

    let failed = TaskValue::from_handle(thread::spawn(|| Err(Diagnostic::new("boom"))));
    for _ in 0..100 {
        if failed.completed_result().is_some() {
            break;
        }
        thread::sleep(StdDuration::from_millis(1));
    }
    assert!(failed.unobserved_error().is_some());

    let failure_flag = Arc::new(AtomicBool::new(false));
    failed.register_group_failure_wake_flag(failure_flag.clone());
    assert!(failure_flag.load(Ordering::SeqCst));
    failure_flag.store(false, Ordering::SeqCst);
    failed.register_group_failure_wake_flag(failure_flag.clone());
    assert!(failure_flag.load(Ordering::SeqCst));

    let running_group = TaskGroupValue::new(&CancellationContext::default());
    let blocker = ChannelValue::new();
    let release = blocker.clone();
    let running = TaskValue::from_handle(thread::spawn(move || {
        let _ = blocker.recv_with_cancellation(None, None);
        Ok(Value::Unit)
    }));
    running_group.register_task(running.clone());
    running_group
        .inner
        .completion_wake_flag
        .store(true, Ordering::SeqCst);
    running_group.clear_completion_wake_if_tasks_still_running();
    assert!(!running_group
        .inner
        .completion_wake_flag
        .load(Ordering::SeqCst));

    release.close();
    assert_eq!(
        wait_task_ready(&running).expect("released task should complete"),
        Value::Unit
    );
}

#[test]
fn task_execution_finalization_maps_failures_to_task_results() {
    let previous_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(|_| {}));

    let direct_failure = finalize_task_execution(|| -> std::result::Result<Value, Diagnostic> {
        std::panic::panic_any(LightweightTaskFailureSignal(Diagnostic::new(
            "direct failure",
        )));
    });
    let owned_panic = finalize_task_execution(|| -> std::result::Result<Value, Diagnostic> {
        std::panic::panic_any("owned panic".to_string());
    });
    let static_panic = finalize_task_execution(|| -> std::result::Result<Value, Diagnostic> {
        std::panic::panic_any("static panic");
    });
    let opaque_panic = finalize_task_execution(|| -> std::result::Result<Value, Diagnostic> {
        std::panic::panic_any(17usize);
    });

    std::panic::set_hook(previous_hook);

    match direct_failure {
        TaskExecutionResult::Ready(Err(error)) => assert_eq!(error.message, "direct failure"),
        other => panic!("expected direct failure diagnostic, got {other:?}"),
    }
    match owned_panic {
        TaskExecutionResult::Ready(Err(error)) => assert!(error.message.contains("owned panic")),
        other => panic!("expected owned panic diagnostic, got {other:?}"),
    }
    match static_panic {
        TaskExecutionResult::Ready(Err(error)) => assert!(error.message.contains("static panic")),
        other => panic!("expected static panic diagnostic, got {other:?}"),
    }
    match opaque_panic {
        TaskExecutionResult::Ready(Err(error)) => assert!(error.message.contains("non-string")),
        other => panic!("expected opaque panic diagnostic, got {other:?}"),
    }

    let cancelled_root = run_lightweight_root_task(|| -> std::result::Result<Value, Diagnostic> {
        std::panic::panic_any(TaskCancelledSignal);
    })
    .expect_err("cancelled root task should surface as a diagnostic");
    assert!(cancelled_root
        .message
        .contains("root Aurora task was cancelled"));
}

#[test]
fn lightweight_task_cancel_boundary_marks_child_cancelled() {
    let result = run_lightweight_root_task(|| {
        let task = spawn_lightweight_task(|| {
            cancel_current_lightweight_task_boundary();
        })?;
        match task.wait_result_with_cancellation_observed(Some(StdDuration::from_secs(1)), None) {
            TaskWaitStatus::Cancelled => Ok(Value::Bool(true)),
            other => panic!("expected cancelled child task, got {other:?}"),
        }
    });

    assert_eq!(
        result.expect("root task should complete"),
        Value::Bool(true)
    );
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
        wait_task_ready(&task).expect("poisoned task handle lock should recover"),
        Value::Int(IntegerValue::from_signed(17))
    );
}

#[test]
fn condvar_helpers_tolerate_poisoned_wait_guards() {
    let pair = Arc::new((Mutex::new(false), Condvar::new()));
    let poisoned_pair = pair.clone();
    let _ = thread::spawn(move || {
        let (lock, _) = &*poisoned_pair;
        let _guard = lock.lock().expect("poison setup lock");
        panic!("poison condvar wait lock");
    })
    .join();

    let (lock, condvar) = &*pair;
    let mut guard = match lock.lock() {
        Ok(guard) => guard,
        Err(poisoned) => poisoned.into_inner(),
    };
    let notify_pair = pair.clone();
    let notifier = thread::spawn(move || {
        let (lock, condvar) = &*notify_pair;
        let mut guard = match lock.lock() {
            Ok(guard) => guard,
            Err(poisoned) => poisoned.into_inner(),
        };
        *guard = true;
        condvar.notify_all();
    });
    while !*guard {
        guard = wait_condvar(condvar, guard);
    }
    notifier.join().expect("notifier should finish");

    let timeout_pair = Arc::new((Mutex::new(false), Condvar::new()));
    let poisoned_timeout_pair = timeout_pair.clone();
    let _ = thread::spawn(move || {
        let (lock, _) = &*poisoned_timeout_pair;
        let _guard = lock.lock().expect("poison setup lock");
        panic!("poison condvar timeout lock");
    })
    .join();

    let (lock, condvar) = &*timeout_pair;
    let guard = match lock.lock() {
        Ok(guard) => guard,
        Err(poisoned) => poisoned.into_inner(),
    };
    let timeout_notify_pair = timeout_pair.clone();
    let timeout_notifier = thread::spawn(move || {
        let (lock, condvar) = &*timeout_notify_pair;
        let mut guard = match lock.lock() {
            Ok(guard) => guard,
            Err(poisoned) => poisoned.into_inner(),
        };
        *guard = true;
        condvar.notify_all();
    });
    let (guard, timed_out) = wait_timeout_condvar(condvar, guard, StdDuration::from_secs(1));
    assert!(!timed_out);
    assert!(*guard);
    timeout_notifier
        .join()
        .expect("timeout notifier should finish");
}

#[test]
fn non_unix_tls_listener_wait_timeout_blocks_when_no_handshakes_are_pending() {
    assert_eq!(
        non_unix_tls_listener_wait_timeout(true, None, None)
            .expect("idle non-Unix TLS wait should not fail"),
        None
    );
}

#[test]
fn non_unix_tls_listener_wait_timeout_uses_full_deadline_when_queue_is_empty() {
    let deadline = Instant::now() + StdDuration::from_millis(200);
    let wait = non_unix_tls_listener_wait_timeout(true, Some(deadline), None)
        .expect("deadline-based non-Unix TLS wait should not fail")
        .expect("deadline-based wait should produce a timeout");
    assert!(
        wait > StdDuration::from_millis(100),
        "idle wait should use the remaining deadline instead of a fixed slice, got {:?}",
        wait
    );
}

#[test]
fn non_unix_tls_listener_wait_timeout_keeps_short_slices_when_handshakes_are_pending() {
    let deadline = Instant::now() + StdDuration::from_millis(200);
    let wait = non_unix_tls_listener_wait_timeout(false, Some(deadline), None)
        .expect("pending-handshake non-Unix TLS wait should not fail")
        .expect("pending-handshake wait should produce a slice");
    assert!(
        wait <= StdDuration::from_millis(50),
        "pending handshakes should still advance on short slices, got {:?}",
        wait
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
fn queue_iteration_wait_wakes_for_unobserved_task_group_failure() {
    let group = TaskGroupValue::new(&CancellationContext::default());
    let task = TaskValue::from_handle(thread::spawn(|| Err(Diagnostic::new("boom"))));
    group.register_task(task);
    thread::sleep(StdDuration::from_millis(20));

    assert_eq!(
        recv_for_task_group_iteration(
            &ChannelValue::new(),
            &CancellationContext::default(),
            &group
        ),
        RecvValueResult::Cancelled
    );
}

#[test]
fn task_group_cleanup_probe_detects_unbounded_waits_after_fresh_spawns() {
    let root_result = run_lightweight_root_task(|| {
        let group = TaskGroupValue::new(&CancellationContext::default());
        let channel = ChannelValue::new();
        let child_cancellation = group.child_cancellation();
        let child_channel = channel.clone();
        let waiting =
            spawn_lightweight_task_with_cancellation(child_cancellation.clone(), move || {
                let _ =
                    child_channel.recv_result_with_cancellation(None, Some(&child_cancellation));
                Ok(Value::Unit)
            })?;
        group.register_task(waiting);

        let tasks = group.drain_tasks();
        Ok(Value::Bool(task_group_cleanup_should_cancel(
            &tasks,
            &CancellationContext::default(),
        )))
    });

    assert_eq!(root_result.unwrap(), Value::Bool(true));
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
    assert_ne!(
        set_a,
        Value::Set(SetValue {
            element_type: Type::named("int32"),
            elements: vec![Value::Int(IntegerValue::from_signed(1))],
        })
    );
    assert_ne!(
        set_a,
        Value::Set(SetValue {
            element_type: Type::named("int32"),
            elements: vec![
                Value::Int(IntegerValue::from_signed(1)),
                Value::Int(IntegerValue::from_signed(3)),
            ],
        })
    );
    assert_eq!(set_a.render(), "Set{1, 2}");

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
    assert_ne!(
        map_a,
        Value::Map(MapValue {
            key_type: Type::named("String"),
            value_type: Type::named("int32"),
            entries: vec![(
                Value::String("a".to_string()),
                Value::Int(IntegerValue::from_signed(1)),
            )],
        })
    );
    assert_ne!(
        map_a,
        Value::Map(MapValue {
            key_type: Type::named("String"),
            value_type: Type::named("int32"),
            entries: vec![
                (
                    Value::String("a".to_string()),
                    Value::Int(IntegerValue::from_signed(1)),
                ),
                (
                    Value::String("b".to_string()),
                    Value::Int(IntegerValue::from_signed(3)),
                ),
            ],
        })
    );
    assert_eq!(map_a.render(), "{a: 1, b: 2}");
    assert_eq!(
        Value::ModuleNamespace(ModuleNamespaceValue {
            path: "pkg.tools".to_string(),
        })
        .render(),
        "<module pkg.tools>"
    );
    assert_value_equals_clone(Value::ModuleNamespace(ModuleNamespaceValue {
        path: "pkg.tools".to_string(),
    }));
    assert_value_equals_clone(Value::Duration(5));
    assert_value_equals_clone(Value::Range(RangeValue { start: 1, end: 4 }));
    assert_eq!(Value::Unit.render(), "");
    assert_value_equals_clone(Value::Unit);
    assert_ne!(Value::Unit, Value::Bool(false));

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
fn nested_queue_producer_registration_walks_collections_instances_and_variants() {
    let queue_in_vec = ChannelValue::new();
    let queue_in_set = ChannelValue::new();
    let queue_in_map_key = ChannelValue::new();
    let queue_in_map_value = ChannelValue::new();
    let queue_in_instance = ChannelValue::new();
    let queue_in_variant = ChannelValue::new();
    let task = TaskValue::from_handle(thread::spawn(|| Ok(Value::Unit)));

    let nested_values = [
        Value::Vec(VecValue {
            element_type: Type::named("Queue"),
            elements: vec![Value::Channel(queue_in_vec.clone())],
        }),
        Value::Set(SetValue {
            element_type: Type::named("Queue"),
            elements: vec![Value::Channel(queue_in_set.clone())],
        }),
        Value::Map(MapValue {
            key_type: Type::named("Queue"),
            value_type: Type::named("Queue"),
            entries: vec![(
                Value::Channel(queue_in_map_key.clone()),
                Value::Channel(queue_in_map_value.clone()),
            )],
        }),
        Value::Instance(super::InstanceValue {
            class_name: "Envelope".to_string(),
            fields: BTreeMap::from([(
                "queue".to_string(),
                Value::Channel(queue_in_instance.clone()),
            )]),
        }),
        Value::EnumVariant(EnumVariantValue {
            enum_name: "Envelope".to_string(),
            variant_name: "Some".to_string(),
            payloads: vec![Value::Channel(queue_in_variant.clone())],
        }),
    ];

    super::register_task_as_queue_producer_for_values(nested_values.iter(), &task);
    queue_in_vec.register_producer_task(&task);

    for queue in [
        queue_in_vec,
        queue_in_set,
        queue_in_map_key,
        queue_in_map_value,
        queue_in_instance,
        queue_in_variant,
    ] {
        assert_eq!(queue.registered_producer_tasks(), vec![task.clone()]);
    }

    wait_task_ready(&task).expect("registered producer task should complete");
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
    let invalid = io_decode_utf8(&[0xff]).expect_err("invalid UTF-8 should be rejected");
    assert_eq!(invalid.kind(), io::ErrorKind::InvalidData);

    let appender = FileValue::append(path.to_str().expect("temp path should be valid UTF-8"))
        .expect("file append should succeed");
    appender
        .write_all("-tail")
        .expect("append write_all should succeed");
    appender.flush().expect("append flush should succeed");
    appender.close();

    let appended = FileValue::open(path.to_str().expect("temp path should be valid UTF-8"))
        .expect("appended file should reopen");
    assert_eq!(
        appended.read_all().expect("read_all should decode text"),
        "aurora-tail"
    );
    appended.close();

    let closed = FileValue::create(
        temp.path()
            .join("closed.txt")
            .to_str()
            .expect("temp path should be valid UTF-8"),
    )
    .expect("closed file should be created");
    closed.close();
    assert_eq!(
        closed
            .read_all()
            .expect_err("closed file read_all should fail")
            .kind(),
        io::ErrorKind::BrokenPipe
    );
    assert_eq!(
        closed
            .read_bytes()
            .expect_err("closed file read_bytes should fail")
            .kind(),
        io::ErrorKind::BrokenPipe
    );
    assert_eq!(
        closed
            .write_all("closed")
            .expect_err("closed file write_all should fail")
            .kind(),
        io::ErrorKind::BrokenPipe
    );
    assert_eq!(
        closed
            .write_bytes(b"closed")
            .expect_err("closed file write_bytes should fail")
            .kind(),
        io::ErrorKind::BrokenPipe
    );
    assert_eq!(
        closed
            .flush()
            .expect_err("closed file flush should fail")
            .kind(),
        io::ErrorKind::BrokenPipe
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

#[test]
fn io_error_maps_standard_error_kinds_to_stable_variants() {
    let cases = [
        (io::ErrorKind::NotFound, "NotFound"),
        (io::ErrorKind::PermissionDenied, "PermissionDenied"),
        (io::ErrorKind::AlreadyExists, "AlreadyExists"),
        (io::ErrorKind::IsADirectory, "IsDirectory"),
        (io::ErrorKind::ConnectionRefused, "ConnectionRefused"),
        (io::ErrorKind::ConnectionReset, "ConnectionReset"),
        (io::ErrorKind::ConnectionAborted, "ConnectionAborted"),
        (io::ErrorKind::NotConnected, "NotConnected"),
        (io::ErrorKind::AddrInUse, "AddrInUse"),
        (io::ErrorKind::AddrNotAvailable, "AddrNotAvailable"),
        (io::ErrorKind::BrokenPipe, "BrokenPipe"),
        (io::ErrorKind::TimedOut, "TimedOut"),
        (io::ErrorKind::WouldBlock, "WouldBlock"),
        (io::ErrorKind::UnexpectedEof, "UnexpectedEof"),
        (io::ErrorKind::InvalidInput, "InvalidInput"),
        (io::ErrorKind::InvalidData, "InvalidData"),
    ];
    for (kind, expected) in cases {
        let Value::EnumVariant(variant) = io_error(io::Error::new(kind, "plain error")) else {
            panic!("io_error should render {expected} as an enum variant");
        };
        assert_eq!(variant.enum_name, "io.Error");
        assert_eq!(variant.variant_name, expected);
        assert!(variant.payloads.is_empty());
    }

    let Value::EnumVariant(other) = io_error(io::Error::other("other diagnostic payload")) else {
        panic!("other io errors should render as io.Error.Other");
    };
    assert_eq!(other.variant_name, "Other");
    assert!(matches!(
        other.payloads.as_slice(),
        [Value::String(message)] if message == "other diagnostic payload"
    ));
}

#[test]
fn process_child_helpers_cover_empty_command_and_cancellation_edges() {
    fn assert_variant(value: Value, enum_name: &str, variant_name: &str) {
        let Value::EnumVariant(variant) = value else {
            panic!("expected {enum_name}.{variant_name} to render as an enum variant");
        };
        assert_eq!(variant.enum_name, enum_name);
        assert_eq!(variant.variant_name, variant_name);
        assert!(variant.payloads.is_empty());
    }

    let empty_command = ProcessChildValue::spawn(
        Vec::new(),
        None,
        Vec::new(),
        ProcessStdioConfig::Null,
        ProcessStdioConfig::Null,
        ProcessStdioConfig::Null,
        false,
    )
    .expect_err("empty process commands should fail before spawning");
    assert_eq!(empty_command.kind(), io::ErrorKind::InvalidInput);

    let group = TaskGroupValue::new(&CancellationContext::default());
    let cancellation = group.child_cancellation();
    group.cancel();
    let child = ProcessChildValue::spawn(
        vec![
            "/bin/sh".to_string(),
            "-c".to_string(),
            "sleep 1".to_string(),
        ],
        None,
        Vec::new(),
        ProcessStdioConfig::Null,
        ProcessStdioConfig::Null,
        ProcessStdioConfig::Null,
        false,
    )
    .expect("slow process should spawn");
    assert!(matches!(
        child.wait(Some(StdDuration::from_secs(1)), Some(&cancellation)),
        ProcessChildWaitStatus::Cancelled
    ));
    assert_variant(
        child
            .wait_or_none(Some(StdDuration::from_secs(1)), Some(&cancellation))
            .expect_err("cancelled wait_or_none should return a process error"),
        "Error",
        "Cancelled",
    );
    assert_variant(
        child
            .wait_ok(Some(StdDuration::from_secs(1)), Some(&cancellation))
            .expect_err("cancelled wait_ok should return a process error"),
        "Error",
        "Cancelled",
    );
    child.close();

    let completed_child = ProcessChildValue::spawn(
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
    .expect("short-lived process should spawn");
    let status = completed_child
        .wait_or_none(Some(StdDuration::from_secs(2)), None)
        .expect("successful wait_or_none should not produce a process error")
        .expect("completed process should return an exit status");
    assert!(status.success());
    let cached_status = completed_child
        .wait_ok(Some(StdDuration::from_secs(2)), None)
        .expect("cached successful exits should satisfy wait_ok");
    assert!(cached_status.success());
    assert!(completed_child
        .try_wait_once()
        .expect("cached try_wait_once should not fail")
        .expect("cached try_wait_once should return the prior status")
        .success());
    completed_child
        .terminate()
        .expect("terminating an already exited process should be a no-op");
    completed_child
        .kill()
        .expect("killing an already exited process should be a no-op");
    completed_child.close();
}

#[cfg(unix)]
#[test]
fn process_pipe_helpers_cover_stderr_reads_and_closed_edges() {
    let child = ProcessChildValue::spawn(
        vec![
            "/bin/sh".to_string(),
            "-c".to_string(),
            "printf 'errline\\nmore' >&2".to_string(),
        ],
        None,
        Vec::new(),
        ProcessStdioConfig::Null,
        ProcessStdioConfig::Null,
        ProcessStdioConfig::Pipe,
        false,
    )
    .expect("stderr-producing child should spawn");
    let stderr = child.stderr().expect("stderr pipe should be captured");
    assert_eq!(
        stderr
            .read_line(
                Some(StdDuration::from_secs(2)),
                Some(&CancellationContext::default())
            )
            .expect("stderr line should read")
            .as_deref(),
        Some("errline")
    );
    assert_eq!(
        stderr
            .read_bytes(
                4,
                Some(StdDuration::from_secs(2)),
                Some(&CancellationContext::default())
            )
            .expect("stderr bytes should read")
            .as_deref(),
        Some(&b"more"[..])
    );
    let _ = child.wait(Some(StdDuration::from_secs(2)), None);
    stderr
        .flush()
        .expect("output pipes should allow no-op flushes before close");

    stderr.close();
    assert_eq!(
        stderr
            .read_all_bytes(None)
            .expect_err("closed pipes should reject read_all_bytes")
            .kind(),
        io::ErrorKind::BrokenPipe
    );
    assert_eq!(
        stderr
            .read_line(None, None)
            .expect_err("closed pipes should reject read_line")
            .kind(),
        io::ErrorKind::BrokenPipe
    );
    assert_eq!(
        stderr
            .read_bytes(1, None, None)
            .expect_err("closed pipes should reject read_bytes")
            .kind(),
        io::ErrorKind::BrokenPipe
    );
    assert_eq!(
        stderr
            .write_bytes(b"x", None, None)
            .expect_err("closed pipes should reject writes before checking pipe direction")
            .kind(),
        io::ErrorKind::BrokenPipe
    );
    assert_eq!(
        stderr
            .flush()
            .expect_err("closed pipes should reject flushes")
            .kind(),
        io::ErrorKind::BrokenPipe
    );
}

#[cfg(unix)]
#[test]
fn process_pipe_helpers_cover_read_all_and_pipe_direction_errors() {
    let output_child = ProcessChildValue::spawn(
        vec![
            "/bin/sh".to_string(),
            "-c".to_string(),
            "printf 'out'; printf 'err' >&2".to_string(),
        ],
        None,
        Vec::new(),
        ProcessStdioConfig::Null,
        ProcessStdioConfig::Pipe,
        ProcessStdioConfig::Pipe,
        false,
    )
    .expect("output-producing child should spawn");
    let stdout = output_child
        .stdout()
        .expect("stdout pipe should be captured");
    let stderr = output_child
        .stderr()
        .expect("stderr pipe should be captured");

    assert_eq!(
        stdout
            .write_bytes(b"nope", None, None)
            .expect_err("process stdout pipes should reject writes")
            .kind(),
        io::ErrorKind::InvalidInput
    );
    assert_eq!(
        stdout
            .read_all_bytes(Some(&CancellationContext::default()))
            .expect("stdout read_all_bytes should drain process output"),
        b"out"
    );
    assert_eq!(
        stderr
            .read_all(Some(&CancellationContext::default()))
            .expect("stderr read_all should drain process output"),
        "err"
    );
    let _ = output_child.wait(Some(StdDuration::from_secs(2)), None);
    output_child.close();

    let input_child = ProcessChildValue::spawn(
        vec![
            "/bin/sh".to_string(),
            "-c".to_string(),
            "cat >/dev/null".to_string(),
        ],
        None,
        Vec::new(),
        ProcessStdioConfig::Pipe,
        ProcessStdioConfig::Null,
        ProcessStdioConfig::Null,
        false,
    )
    .expect("stdin-consuming child should spawn");
    let stdin = input_child.stdin().expect("stdin pipe should be captured");
    assert_eq!(
        stdin
            .read_all(None)
            .expect_err("process stdin pipes should reject read_all")
            .kind(),
        io::ErrorKind::InvalidInput
    );
    assert_eq!(
        stdin
            .read_line(None, None)
            .expect_err("process stdin pipes should reject read_line")
            .kind(),
        io::ErrorKind::InvalidInput
    );
    assert_eq!(
        stdin
            .read_bytes(1, None, None)
            .expect_err("process stdin pipes should reject read_bytes")
            .kind(),
        io::ErrorKind::InvalidInput
    );
    stdin
        .write_all("done", Some(StdDuration::from_secs(2)), None)
        .expect("stdin pipes should accept writes");
    stdin.flush().expect("stdin pipe flush should succeed");
    stdin.close();
    let _ = input_child.wait(Some(StdDuration::from_secs(2)), None);
    input_child.close();
}

#[cfg(unix)]
#[test]
fn unix_error_normalization_helpers_cover_udp_and_websocket_edges() {
    let too_large = super::normalize_udp_send_error(io::Error::from_raw_os_error(libc::EMSGSIZE));
    assert_eq!(too_large.kind(), io::ErrorKind::InvalidInput);
    assert_eq!(
        too_large.to_string(),
        "UDP datagram exceeds the platform send limit"
    );

    let other = super::normalize_udp_send_error(io::Error::new(io::ErrorKind::Other, "plain"));
    assert_eq!(other.kind(), io::ErrorKind::Other);
    assert_eq!(other.to_string(), "plain");

    let unsupported = super::unsupported_websocket_transport_error();
    assert_eq!(unsupported.kind(), io::ErrorKind::Unsupported);
    assert_eq!(unsupported.to_string(), "unsupported websocket transport");

    super::ensure_rustls_crypto_provider();
    let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("listener should bind");
    let address = listener
        .local_addr()
        .expect("listener should expose a local address");
    let client = std::net::TcpStream::connect(address).expect("client stream should connect");
    let (_server, _) = listener.accept().expect("server stream should accept");
    let raw_fd = client.as_raw_fd();
    let config = rustls::ClientConfig::builder()
        .with_root_certificates(super::load_tls_root_store(None).expect("root store should load"))
        .with_no_client_auth();
    let server_name =
        rustls::pki_types::ServerName::try_from("localhost".to_string()).expect("valid DNS name");
    let connection =
        rustls::ClientConnection::new(Arc::new(config), server_name).expect("client config");
    let stream = rustls::StreamOwned::new(connection, client);
    let maybe_tls = tungstenite::stream::MaybeTlsStream::Rustls(stream);
    assert_eq!(
        super::maybe_tls_stream_raw_fd(&maybe_tls).expect("rustls stream fd"),
        raw_fd
    );
    let websocket = tungstenite::WebSocket::from_raw_socket(
        maybe_tls,
        tungstenite::protocol::Role::Client,
        Some(super::websocket_config()),
    );
    let mut socket = super::WebSocketStateKind::MaybeTls(Box::new(websocket));
    assert_eq!(super::websocket_raw_fd(&socket).expect("raw fd"), raw_fd);
    super::websocket_set_nonblocking(&mut socket, true).expect("rustls stream should toggle flags");
    assert!(fd_is_nonblocking(raw_fd));
    super::websocket_set_nonblocking(&mut socket, false)
        .expect("rustls stream should restore blocking mode");
    assert!(!fd_is_nonblocking(raw_fd));
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
fn supervisor_delays_restarts_and_reports_restart_counts() {
    let supervisor = ProcessSupervisorValue::new();
    supervisor
        .start(
            "flaky".to_string(),
            vec![
                "/bin/sh".to_string(),
                "-c".to_string(),
                "exit 1".to_string(),
            ],
            None,
            Vec::new(),
            ProcessStdioConfig::Null,
            ProcessStdioConfig::Null,
            ProcessStdioConfig::Null,
            ProcessRestartPolicy::Always,
            StdDuration::from_millis(10),
            Some(1),
            false,
        )
        .expect("supervisor should start a short failing child");

    let restarted = match supervisor.wait(Some(StdDuration::from_secs(2)), None) {
        ProcessSupervisorWaitStatus::Event(event) => event,
        ProcessSupervisorWaitStatus::TimedOut => panic!("supervisor restart timed out"),
        ProcessSupervisorWaitStatus::Cancelled => panic!("supervisor restart was cancelled"),
    };
    let Value::EnumVariant(restarted) = restarted else {
        panic!("supervisor restart should return an event variant");
    };
    assert_eq!(restarted.enum_name, "SupervisorEvent");
    assert_eq!(restarted.variant_name, "Restarted");
    assert_eq!(restarted.payloads[0], Value::String("flaky".to_string()));
    assert_eq!(
        restarted.payloads[2],
        Value::Int(IntegerValue::from_signed(1))
    );

    let exited = match supervisor.wait(Some(StdDuration::from_secs(2)), None) {
        ProcessSupervisorWaitStatus::Event(event) => event,
        ProcessSupervisorWaitStatus::TimedOut => panic!("supervisor exit timed out"),
        ProcessSupervisorWaitStatus::Cancelled => panic!("supervisor exit was cancelled"),
    };
    let Value::EnumVariant(exited) = exited else {
        panic!("supervisor exit should return an event variant");
    };
    assert_eq!(exited.enum_name, "SupervisorEvent");
    assert_eq!(exited.variant_name, "Exited");
    assert_eq!(exited.payloads[0], Value::String("flaky".to_string()));
    assert_eq!(exited.payloads[2], Value::Int(IntegerValue::from_signed(1)));
    assert!(supervisor.is_empty());
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
        wait_task_ready(&server)?;
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
        wait_task_ready(&server)?;
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
fn lightweight_blocking_io_observes_pre_cancelled_and_wait_cancelled_contexts() {
    let result = run_lightweight_root_task(move || {
        let pre_cancelled_group = TaskGroupValue::new(&CancellationContext::default());
        let pre_cancelled = pre_cancelled_group.child_cancellation();
        pre_cancelled_group.cancel();
        let error = run_blocking_io(|| Ok::<_, io::Error>(()), Some(&pre_cancelled))
            .expect_err("pre-cancelled blocking I/O should not start");
        assert_eq!(error.kind(), io::ErrorKind::Interrupted);

        let wait_cancelled_group = TaskGroupValue::new(&CancellationContext::default());
        let wait_cancelled = wait_cancelled_group.child_cancellation();
        let canceller = spawn_lightweight_task(move || {
            sleep_with_runtime_scheduler(StdDuration::from_millis(10), None);
            wait_cancelled_group.cancel();
            Ok(Value::Unit)
        })?;
        let error = run_blocking_io(
            || {
                thread::sleep(StdDuration::from_millis(100));
                Ok::<_, io::Error>(())
            },
            Some(&wait_cancelled),
        )
        .expect_err("blocking I/O wait should observe cancellation");
        assert_eq!(error.kind(), io::ErrorKind::Interrupted);
        wait_task_ready(&canceller)?;
        Ok(Value::Unit)
    });
    assert!(
        result.is_ok(),
        "blocking I/O cancellation paths should complete: {result:?}"
    );
}

#[test]
fn tcp_connect_offloads_slow_resolution_without_starving_a_sibling_timer() {
    let listener = std::net::TcpListener::bind("127.0.0.1:0")
        .expect("loopback listener should bind for the injected resolver");
    let candidate = listener
        .local_addr()
        .expect("loopback listener should report its address");
    let resolver_finished = Arc::new(AtomicBool::new(false));
    let task_resolver_finished = resolver_finished.clone();

    let result = run_lightweight_root_task(move || {
        let connect = spawn_lightweight_task(move || {
            let resolver_finished = task_resolver_finished.clone();
            let stream = TcpStreamValue::connect_with_operations(
                "slow.injected.test:443",
                Some(StdDuration::from_secs(1)),
                None,
                move |_address| {
                    thread::sleep(StdDuration::from_millis(100));
                    resolver_finished.store(true, Ordering::SeqCst);
                    Ok(vec![candidate])
                },
                |candidate, timeout| match timeout {
                    Some(timeout) => std::net::TcpStream::connect_timeout(&candidate, timeout),
                    None => std::net::TcpStream::connect(candidate),
                },
            )
            .map_err(|error| Diagnostic::new(error.to_string()))?;
            stream.close();
            Ok(Value::Unit)
        })?;

        let sibling_resolver_finished = resolver_finished.clone();
        let sibling = spawn_lightweight_task(move || {
            sleep_with_runtime_scheduler(StdDuration::from_millis(10), None);
            if sibling_resolver_finished.load(Ordering::SeqCst) {
                return Err(Diagnostic::new(
                    "slow DNS resolution blocked the sibling timer",
                ));
            }
            Ok(Value::Unit)
        })?;

        wait_task_ready(&sibling)?;
        wait_task_ready(&connect)?;
        Ok(Value::Unit)
    });

    assert!(
        result.is_ok(),
        "slow resolution should yield the lightweight scheduler: {result:?}"
    );
}

#[test]
fn tcp_connect_timeout_budget_includes_resolution_wait() {
    let started = Instant::now();
    let result = run_lightweight_root_task(move || {
        let error = TcpStreamValue::connect_with_operations(
            "slow.injected.test:443",
            Some(StdDuration::from_millis(20)),
            None,
            |_address| {
                thread::sleep(StdDuration::from_millis(150));
                Ok(vec!["127.0.0.1:9"
                    .parse()
                    .expect("test address should parse")])
            },
            |candidate, timeout| match timeout {
                Some(timeout) => std::net::TcpStream::connect_timeout(&candidate, timeout),
                None => std::net::TcpStream::connect(candidate),
            },
        )
        .expect_err("resolution should consume the whole connect timeout budget");
        assert_eq!(error.kind(), io::ErrorKind::TimedOut);
        Ok(Value::Unit)
    });

    assert!(result.is_ok(), "timeout path should complete: {result:?}");
    assert!(
        started.elapsed() < StdDuration::from_millis(100),
        "connect timeout must not restart after DNS; elapsed {:?}",
        started.elapsed()
    );
}

#[test]
fn tcp_connect_timeout_offloads_resolution_without_a_lightweight_task_context() {
    let started = Instant::now();
    let error = TcpStreamValue::connect_with_operations(
        "slow.host-entry.test:443",
        Some(StdDuration::from_millis(20)),
        None,
        |_address| {
            thread::sleep(StdDuration::from_millis(150));
            Ok(vec!["127.0.0.1:9"
                .parse()
                .expect("test address should parse")])
        },
        |candidate, timeout| match timeout {
            Some(timeout) => std::net::TcpStream::connect_timeout(&candidate, timeout),
            None => std::net::TcpStream::connect(candidate),
        },
    )
    .expect_err("host-entry resolution should honor its timeout");

    assert_eq!(error.kind(), io::ErrorKind::TimedOut);
    assert!(
        started.elapsed() < StdDuration::from_millis(100),
        "host-entry DNS must use the blocking service; elapsed {:?}",
        started.elapsed()
    );
}

#[test]
fn tcp_connect_reports_empty_resolution_with_the_original_address() {
    let error = TcpStreamValue::connect_with_operations(
        "empty.injected.test:443",
        Some(StdDuration::from_secs(1)),
        None,
        |_address| Ok(Vec::new()),
        |_candidate, _timeout| -> io::Result<std::net::TcpStream> {
            panic!("an empty resolution must not attempt a connection")
        },
    )
    .expect_err("empty DNS results should be rejected");

    assert_eq!(error.kind(), io::ErrorKind::InvalidInput);
    assert!(
        error
            .to_string()
            .contains("`empty.injected.test:443` did not resolve"),
        "empty-resolution diagnostics should retain the requested address: {error}"
    );
}

#[test]
fn tcp_connect_cancellation_stops_waiting_for_an_inflight_resolver() {
    let result = run_lightweight_root_task(move || {
        let group = TaskGroupValue::new(&CancellationContext::default());
        let cancellation = group.child_cancellation();
        let task_cancellation = cancellation.clone();
        let resolver_started = ChannelValue::new();
        let task_resolver_started = resolver_started.clone();
        let (release_resolver, wait_for_release) = std::sync::mpsc::channel();

        let connect = spawn_lightweight_task_with_cancellation(cancellation, move || {
            let error = TcpStreamValue::connect_with_operations(
                "cancelled.injected.test:443",
                None,
                Some(&task_cancellation),
                move |_address| {
                    task_resolver_started
                        .send(Value::Unit)
                        .expect("resolver-start signal should remain open");
                    wait_for_release
                        .recv()
                        .expect("test should release the resolver worker");
                    Err(io::Error::other("resolver released after cancellation"))
                },
                |candidate, timeout| match timeout {
                    Some(timeout) => std::net::TcpStream::connect_timeout(&candidate, timeout),
                    None => std::net::TcpStream::connect(candidate),
                },
            )
            .expect_err("cancellation should end the scheduler wait");
            if error.kind() != io::ErrorKind::Interrupted {
                return Err(Diagnostic::new(format!(
                    "expected Interrupted, found {:?}",
                    error.kind()
                )));
            }
            Ok(Value::Unit)
        })?;

        resolver_started
            .recv_with_cancellation(Some(StdDuration::from_secs(1)), None)
            .ok_or_else(|| Diagnostic::new("resolver worker did not start"))?;
        group.cancel();
        let connect_result = wait_task_ready(&connect);
        release_resolver
            .send(())
            .map_err(|_| Diagnostic::new("resolver worker stopped before release"))?;
        connect_result?;
        Ok(Value::Unit)
    });

    assert!(
        result.is_ok(),
        "connect cancellation should be prompt and memory-safe: {result:?}"
    );
}

#[test]
fn blocking_service_cancellation_drops_late_results_safely() {
    #[derive(Debug)]
    struct DropProbe(Arc<AtomicBool>);

    impl Drop for DropProbe {
        fn drop(&mut self) {
            self.0.store(true, Ordering::SeqCst);
        }
    }

    let dropped = Arc::new(AtomicBool::new(false));
    let result = run_lightweight_root_task({
        let dropped = dropped.clone();
        move || {
            let group = TaskGroupValue::new(&CancellationContext::default());
            let cancellation = group.child_cancellation();
            let task_cancellation = cancellation.clone();
            let operation_started = ChannelValue::new();
            let task_operation_started = operation_started.clone();
            let (release_operation, wait_for_release) = std::sync::mpsc::channel();

            let operation = spawn_lightweight_task_with_cancellation(cancellation, move || {
                let error = super::run_blocking_io_with_deadline(
                    move || {
                        task_operation_started
                            .send(Value::Unit)
                            .expect("operation-start signal should remain open");
                        wait_for_release
                            .recv()
                            .expect("test should release the blocking operation");
                        Ok(DropProbe(dropped))
                    },
                    None,
                    Some(&task_cancellation),
                )
                .expect_err("cancellation should abandon the blocking result");
                if error.kind() != io::ErrorKind::Interrupted {
                    return Err(Diagnostic::new(format!(
                        "expected Interrupted, found {:?}",
                        error.kind()
                    )));
                }
                Ok(Value::Unit)
            })?;

            operation_started
                .recv_with_cancellation(Some(StdDuration::from_secs(1)), None)
                .ok_or_else(|| Diagnostic::new("blocking operation did not start"))?;
            group.cancel();
            wait_task_ready(&operation)?;
            release_operation
                .send(())
                .map_err(|_| Diagnostic::new("blocking operation stopped before release"))?;
            Ok(Value::Unit)
        }
    });

    assert!(
        result.is_ok(),
        "late-result cancellation path should complete: {result:?}"
    );
    let deadline = Instant::now() + StdDuration::from_secs(1);
    while !dropped.load(Ordering::SeqCst) && Instant::now() < deadline {
        thread::sleep(StdDuration::from_millis(1));
    }
    assert!(
        dropped.load(Ordering::SeqCst),
        "a result produced after cancellation must be dropped by the worker"
    );
}

#[test]
fn tcp_connect_candidates_share_one_timeout_budget() {
    let started = Instant::now();
    let deadline = started + StdDuration::from_millis(100);
    let first: std::net::SocketAddr = "127.0.0.1:1".parse().expect("address should parse");
    let second: std::net::SocketAddr = "127.0.0.1:2".parse().expect("address should parse");
    let mut clock = [
        started,
        started + StdDuration::from_millis(40),
        started + StdDuration::from_millis(40),
    ]
    .into_iter();
    let mut observed_budgets = Vec::new();
    let mut attempts = 0;

    let connected = super::connect_resolved_tcp_candidates_with_clock(
        "injected.test:443",
        vec![first, second],
        Some(deadline),
        || {
            clock
                .next()
                .expect("test clock should cover every observation")
        },
        |_candidate, timeout| {
            observed_budgets.push(timeout.expect("deadline should produce a candidate budget"));
            attempts += 1;
            if attempts == 1 {
                Err(io::Error::new(
                    io::ErrorKind::ConnectionRefused,
                    "first candidate refused",
                ))
            } else {
                Ok("connected")
            }
        },
    )
    .expect("the second candidate should connect within the shared budget");

    assert_eq!(connected, "connected");
    assert_eq!(
        observed_budgets,
        vec![StdDuration::from_millis(100), StdDuration::from_millis(60)],
        "each candidate must receive only the timeout budget that remains"
    );
}

#[cfg(unix)]
#[test]
fn unix_connect_offloads_a_slow_connect_without_starving_a_sibling_timer() {
    let connect_finished = Arc::new(AtomicBool::new(false));
    let task_connect_finished = connect_finished.clone();
    let result = run_lightweight_root_task(move || {
        let connect = spawn_lightweight_task(move || {
            let connect_finished = task_connect_finished.clone();
            let stream = UnixStreamValue::connect_with_operation(
                "/tmp/injected-slow-connect.sock",
                Some(StdDuration::from_secs(1)),
                None,
                move |_path| {
                    thread::sleep(StdDuration::from_millis(100));
                    let (stream, peer) = std::os::unix::net::UnixStream::pair()?;
                    drop(peer);
                    connect_finished.store(true, Ordering::SeqCst);
                    Ok(stream)
                },
            )
            .map_err(|error| Diagnostic::new(error.to_string()))?;
            stream.close();
            Ok(Value::Unit)
        })?;

        let sibling_connect_finished = connect_finished.clone();
        let sibling = spawn_lightweight_task(move || {
            sleep_with_runtime_scheduler(StdDuration::from_millis(10), None);
            if sibling_connect_finished.load(Ordering::SeqCst) {
                return Err(Diagnostic::new(
                    "slow Unix connect blocked the sibling timer",
                ));
            }
            Ok(Value::Unit)
        })?;

        wait_task_ready(&sibling)?;
        wait_task_ready(&connect)?;
        Ok(Value::Unit)
    });

    assert!(
        result.is_ok(),
        "Unix connect should yield the lightweight scheduler: {result:?}"
    );
}

#[test]
fn read_all_surfaces_size_limits_for_unbounded_resources() {
    // This test intentionally transfers the full 64 MiB limit. Keep a deadlock
    // guard without turning compiler-coverage instrumentation into a throughput test.
    const NETWORK_TIMEOUT: StdDuration = StdDuration::from_secs(30);

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
            .accept(Some(NETWORK_TIMEOUT), Some(&CancellationContext::default()))
            .expect("accept should succeed");
        while bytes_remaining > 0 {
            let chunk_len = chunk.len().min(bytes_remaining);
            if stream
                .write_bytes(
                    &chunk[..chunk_len],
                    Some(NETWORK_TIMEOUT),
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
        Some(NETWORK_TIMEOUT),
        Some(&CancellationContext::default()),
    )
    .expect("client should connect");
    let error = match client.read_all(Some(NETWORK_TIMEOUT), Some(&CancellationContext::default()))
    {
        Ok(contents) => panic!(
            "oversized tcp read_all should fail, but returned {} bytes",
            contents.len()
        ),
        Err(error) => error,
    };
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
fn http_helper_parsing_covers_reason_phrases_and_header_errors() {
    let reason_cases = [
        (100, "Continue"),
        (101, "Switching Protocols"),
        (200, "OK"),
        (201, "Created"),
        (202, "Accepted"),
        (204, "No Content"),
        (301, "Moved Permanently"),
        (302, "Found"),
        (304, "Not Modified"),
        (400, "Bad Request"),
        (401, "Unauthorized"),
        (403, "Forbidden"),
        (404, "Not Found"),
        (405, "Method Not Allowed"),
        (408, "Request Timeout"),
        (409, "Conflict"),
        (413, "Payload Too Large"),
        (415, "Unsupported Media Type"),
        (426, "Upgrade Required"),
        (429, "Too Many Requests"),
        (431, "Request Header Fields Too Large"),
        (500, "Internal Server Error"),
        (501, "Not Implemented"),
        (502, "Bad Gateway"),
        (503, "Service Unavailable"),
        (504, "Gateway Timeout"),
        (599, ""),
    ];
    for (status, expected) in reason_cases {
        assert_eq!(super::http_reason_phrase(status), expected);
    }

    assert!(super::parse_http_response_head(b"HTTP/1.1 200")
        .expect("partial response head should parse")
        .is_none());
    assert!(super::parse_http_request_head(b"GET / HTTP/1.1")
        .expect("partial request head should parse")
        .is_none());

    let (_, status, reason, headers, framing) =
        super::parse_http_response_head(b"HTTP/1.1 202 Accepted\r\nContent-Length: 2\r\n\r\n")
            .expect("response head should parse")
            .expect("response head should be complete");
    assert_eq!(status, 202);
    assert_eq!(reason, "Accepted");
    assert_eq!(
        headers,
        vec![("Content-Length".to_string(), "2".to_string())]
    );
    assert_eq!(framing, super::HttpBodyFraming::ContentLength(2));

    let (_, status, reason, headers, framing) =
        super::parse_http_response_head(b"HTTP/1.1 204\r\n\r\n")
            .expect("response head without explicit reason should parse")
            .expect("response head should be complete");
    assert_eq!(status, 204);
    assert_eq!(reason, "");
    assert!(headers.is_empty());
    assert_eq!(framing, super::HttpBodyFraming::UntilClose);

    let (_, method, path, headers, framing) = super::parse_http_request_head(
        b"POST /submit HTTP/1.1\r\nHost: example.test\r\nContent-Length: 0\r\n\r\n",
    )
    .expect("request head should parse")
    .expect("request head should be complete");
    assert_eq!(method, "POST");
    assert_eq!(path, "/submit");
    assert_eq!(headers[0], ("Host".to_string(), "example.test".to_string()));
    assert_eq!(framing, super::HttpBodyFraming::ContentLength(0));

    let error = super::parse_http_response_head(b"HTTP/1.1 200 OK\r\nX-Bad: \xff\r\n\r\n")
        .expect_err("non-UTF-8 header values should fail");
    assert_eq!(error.kind(), io::ErrorKind::InvalidData);

    let error = super::parse_http_response_head(b"HTTP/1.1 nope\r\n\r\n")
        .expect_err("invalid response heads should fail");
    assert_eq!(error.kind(), io::ErrorKind::InvalidData);

    let error = super::parse_http_response_head(b"HTTP/1.1 200 OK\r\nContent-Length: nope\r\n\r\n")
        .expect_err("invalid content-length should fail");
    assert_eq!(error.kind(), io::ErrorKind::InvalidData);

    assert!(super::is_http_bad_request_error(&io::Error::new(
        io::ErrorKind::InvalidData,
        "malformed request"
    )));
    assert!(super::is_http_bad_request_error(&io::Error::new(
        io::ErrorKind::UnexpectedEof,
        "truncated request"
    )));
    assert!(!super::is_http_bad_request_error(&io::Error::new(
        io::ErrorKind::Other,
        "network failure"
    )));
    assert!(!super::is_http_bad_request_error(
        &super::http_message_too_large_error()
    ));
    assert!(!super::is_http_bad_request_error(
        &super::http_headers_too_large_error()
    ));

    let conflict = vec![
        ("content-length".to_string(), "1".to_string()),
        ("Content-Length".to_string(), "2".to_string()),
    ];
    let error = super::parse_http_content_length(&conflict)
        .expect_err("direct conflicting content-length values should fail");
    assert_eq!(error.kind(), io::ErrorKind::InvalidData);

    let error = super::parse_http_response_head(
        b"HTTP/1.1 200 OK\r\nContent-Length: 1\r\nContent-Length: 2\r\n\r\n",
    )
    .expect_err("conflicting content-length values should fail");
    assert_eq!(error.kind(), io::ErrorKind::InvalidData);

    let error =
        super::parse_http_request_head(b"GET / HTTP/1.1\r\nTransfer-Encoding: gzip\r\n\r\n")
            .expect_err("unsupported transfer-encoding should fail");
    assert_eq!(error.kind(), io::ErrorKind::InvalidInput);

    let equivalent_lengths = vec![
        ("Content-Length".to_string(), "2".to_string()),
        ("content-length".to_string(), "2".to_string()),
        ("Transfer-Encoding".to_string(), "identity".to_string()),
    ];
    assert_eq!(
        super::parse_http_content_length(&equivalent_lengths)
            .expect("matching content-length headers should be accepted"),
        Some(2)
    );

    let mut oversized = vec![0; super::MAX_HTTP_MESSAGE_BYTES];
    let error = super::push_http_chunk(&mut oversized, &[1])
        .expect_err("oversized HTTP buffers should be rejected before extending");
    assert_eq!(error.kind(), io::ErrorKind::InvalidData);
}

#[test]
fn chunked_http_framing_rejects_ambiguous_malformed_and_oversized_inputs() {
    use super::HttpBodyFraming;

    assert_eq!(super::find_http_crlf(b"aa\r\nbb", 0), Some(2));
    assert_eq!(super::find_http_crlf(b"aa\r\nbb", 4), None);
    assert_eq!(super::find_http_crlf(b"aa", 3), None);

    let matching_lengths = vec![
        ("Content-Length".to_string(), "2".to_string()),
        ("content-length".to_string(), "2".to_string()),
    ];
    assert_eq!(
        super::parse_http_body_framing(&matching_lengths, HttpBodyFraming::UntilClose)
            .expect("matching content lengths should be accepted"),
        HttpBodyFraming::ContentLength(2)
    );
    let invalid_length = vec![("Content-Length".to_string(), "nope".to_string())];
    assert_eq!(
        super::parse_http_body_framing(&invalid_length, HttpBodyFraming::UntilClose)
            .expect_err("invalid content lengths should fail")
            .kind(),
        io::ErrorKind::InvalidData
    );
    let conflicting_lengths = vec![
        ("Content-Length".to_string(), "1".to_string()),
        ("Content-Length".to_string(), "2".to_string()),
    ];
    assert_eq!(
        super::parse_http_body_framing(&conflicting_lengths, HttpBodyFraming::UntilClose)
            .expect_err("conflicting content lengths should fail")
            .kind(),
        io::ErrorKind::InvalidData
    );
    let chunked_with_length = vec![
        ("Transfer-Encoding".to_string(), "chunked".to_string()),
        ("Content-Length".to_string(), "2".to_string()),
    ];
    assert_eq!(
        super::parse_http_body_framing(&chunked_with_length, HttpBodyFraming::UntilClose)
            .expect_err("chunked plus content-length should fail")
            .kind(),
        io::ErrorKind::InvalidData
    );
    let unsupported = vec![("Transfer-Encoding".to_string(), "gzip, chunked".to_string())];
    assert_eq!(
        super::parse_http_body_framing(&unsupported, HttpBodyFraming::UntilClose)
            .expect_err("stacked transfer codings should fail")
            .kind(),
        io::ErrorKind::InvalidInput
    );
    let identity = vec![("Transfer-Encoding".to_string(), " identity,  ".to_string())];
    assert_eq!(
        super::parse_http_body_framing(&identity, HttpBodyFraming::UntilClose)
            .expect("identity coding should preserve the default"),
        HttpBodyFraming::UntilClose
    );

    assert!(super::try_decode_chunked_http_body(b"", 0)
        .expect("incomplete chunk header should not fail")
        .is_none());
    assert_eq!(
        super::try_decode_chunked_http_body(b"\xff\r\n", 0)
            .expect_err("non-UTF-8 chunk sizes should fail")
            .kind(),
        io::ErrorKind::InvalidData
    );
    assert_eq!(
        super::try_decode_chunked_http_body(b"nope\r\n", 0)
            .expect_err("non-hex chunk sizes should fail")
            .kind(),
        io::ErrorKind::InvalidData
    );
    assert_eq!(
        super::try_decode_chunked_http_body(b"0\r\n\r\n", 0)
            .expect("empty chunked bodies should decode"),
        Some(Vec::new())
    );
    assert!(
        super::try_decode_chunked_http_body(b"0\r\nX-Test: yes\r\n", 0)
            .expect("incomplete trailers should not fail")
            .is_none()
    );
    assert_eq!(
        super::try_decode_chunked_http_body(b"4;kind=text\r\ntest\r\n0\r\nX-Test: yes\r\n\r\n", 0)
            .expect("extensions and trailers should decode"),
        Some(b"test".to_vec())
    );
    let oversized_size = format!("{:x}\r\n", super::MAX_HTTP_MESSAGE_BYTES + 1);
    assert_eq!(
        super::try_decode_chunked_http_body(oversized_size.as_bytes(), 0)
            .expect_err("oversized chunk declarations should fail")
            .kind(),
        io::ErrorKind::InvalidData
    );
    assert!(super::try_decode_chunked_http_body(b"4\r\nabc", 0)
        .expect("incomplete chunk data should not fail")
        .is_none());
    assert_eq!(
        super::try_decode_chunked_http_body(b"3\r\nabcXX", 0)
            .expect_err("chunk data without CRLF should fail")
            .kind(),
        io::ErrorKind::InvalidData
    );
    let mut oversized_trailer = b"0\r\n".to_vec();
    oversized_trailer.extend(std::iter::repeat_n(b'a', super::MAX_HTTP_MESSAGE_BYTES + 1));
    oversized_trailer.extend_from_slice(b"\r\n\r\n");
    assert_eq!(
        super::try_decode_chunked_http_body(&oversized_trailer, 0)
            .expect_err("oversized trailers should fail")
            .kind(),
        io::ErrorKind::InvalidData
    );
}

#[test]
fn http_request_builder_covers_host_variants_and_header_overrides() {
    fn request(url: &str, headers: Vec<(String, String)>) -> String {
        let url = url::Url::parse(url).expect("test URL should parse");
        String::from_utf8(
            super::build_http_request_bytes("POST", &url, b"ok", headers)
                .expect("request bytes should render"),
        )
        .expect("HTTP request bytes should be UTF-8")
    }

    assert!(request("http://[::1]/path", Vec::new()).contains("Host: [::1]\r\n"));
    assert!(request("http://[::1]:8080/path", Vec::new()).contains("Host: [::1]:8080\r\n"));
    assert!(request("http://127.0.0.1/path", Vec::new()).contains("Host: 127.0.0.1\r\n"));
    assert!(request("http://127.0.0.1:8080/path", Vec::new()).contains("Host: 127.0.0.1:8080\r\n"));
    assert!(request("http://example.com/path", Vec::new()).contains("Host: example.com\r\n"));
    assert!(
        request("http://example.com:8080/path", Vec::new()).contains("Host: example.com:8080\r\n")
    );
    assert!(request("https://example.com/path", Vec::new()).contains("Host: example.com\r\n"));
    assert!(request("wss://example.com:443/path", Vec::new()).contains("Host: example.com\r\n"));
    assert!(request("file:///tmp/aurora", Vec::new()).contains("Host: \r\n"));

    let ws_ipv6 = url::Url::parse("ws://[::1]:9000/socket").expect("websocket URL should parse");
    assert_eq!(
        super::websocket_host_header(&ws_ipv6).expect("websocket host should render"),
        "[::1]:9000"
    );
    let ws_ipv6_default = url::Url::parse("ws://[::1]/socket").expect("websocket URL should parse");
    assert_eq!(
        super::websocket_host_header(&ws_ipv6_default).expect("websocket host should render"),
        "[::1]"
    );
    let ws_domain_default =
        url::Url::parse("ws://example.com/socket").expect("websocket URL should parse");
    assert_eq!(
        super::websocket_host_header(&ws_domain_default).expect("websocket host should render"),
        "example.com"
    );
    let ws_missing_host =
        url::Url::parse("mailto:aurora@example.com").expect("hostless URL should parse");
    let missing_host = super::websocket_host_header(&ws_missing_host)
        .expect_err("hostless websocket URLs should fail host rendering");
    assert_eq!(missing_host.kind(), io::ErrorKind::InvalidInput);

    let with_query = request("http://example.com/search?q=aurora", Vec::new());
    assert!(with_query.starts_with("POST /search?q=aurora HTTP/1.1\r\n"));
    let root_path = request("http://example.com", Vec::new());
    assert!(root_path.starts_with("POST / HTTP/1.1\r\n"));

    let overridden = request(
        "http://example.com/path",
        vec![
            ("Host".to_string(), "custom.local".to_string()),
            ("Content-Length".to_string(), "2".to_string()),
            ("Connection".to_string(), "keep-alive".to_string()),
        ],
    );
    assert!(overridden.contains("Host: custom.local\r\n"));
    assert!(overridden.contains("Content-Length: 2\r\n"));
    assert!(overridden.contains("Connection: keep-alive\r\n"));
    assert!(!overridden.contains("Host: example.com\r\n"));
    assert!(!overridden.contains("Connection: close\r\n"));

    let error = HttpResponseValue::request_text(
        "GET",
        "ftp://example.com/",
        "",
        Vec::new(),
        Some(StdDuration::from_secs(1)),
        Some(&CancellationContext::default()),
    )
    .expect_err("unsupported URL schemes should fail before connecting");
    assert_eq!(error.kind(), io::ErrorKind::InvalidInput);
}

#[test]
fn http_stream_helpers_cover_response_without_content_length_and_custom_headers() {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("server should bind");
    let address = listener.local_addr().expect("server address should exist");
    let server = thread::spawn(move || {
        let (mut stream, _) = listener.accept().expect("server should accept");
        stream
            .write_all(b"HTTP/1.1 202 Accepted\r\nConnection: close\r\n\r\nbody")
            .expect("server response should write");
    });

    let mut client = std::net::TcpStream::connect(address).expect("client should connect");
    let response = super::read_http_response_from_stream(
        &mut client,
        Some(Instant::now() + StdDuration::from_secs(2)),
        Some(&CancellationContext::default()),
    )
    .expect("response without content-length should read until close");
    assert_eq!(response.status(), 202);
    assert_eq!(
        response.text().expect("response body should decode"),
        "body".to_string()
    );
    server.join().expect("server thread should join");

    let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("server should bind");
    let address = listener.local_addr().expect("server address should exist");
    let client = thread::spawn(move || {
        let mut stream = std::net::TcpStream::connect(address).expect("client should connect");
        let mut response = String::new();
        stream
            .read_to_string(&mut response)
            .expect("client should read response");
        response
    });
    let (mut stream, _) = listener.accept().expect("server should accept");
    super::write_http_response_to_stream(
        &mut stream,
        201,
        vec![
            ("Content-Length".to_string(), "2".to_string()),
            ("Connection".to_string(), "close".to_string()),
        ],
        b"ok",
        Some(Instant::now() + StdDuration::from_secs(2)),
        Some(&CancellationContext::default()),
    )
    .expect("response should write");
    stream
        .shutdown(std::net::Shutdown::Write)
        .expect("server write side should close");
    let response = client.join().expect("client thread should join");
    assert!(response.starts_with("HTTP/1.1 201 Created\r\n"));
    assert!(response.contains("Content-Length: 2\r\n"));
    assert!(response.contains("Connection: close\r\n"));
    assert!(response.ends_with("\r\n\r\nok"));
}

#[test]
fn http_stream_helpers_decode_chunked_requests_and_responses() {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("server should bind");
    let address = listener.local_addr().expect("server address should exist");
    let server = thread::spawn(move || {
        let (mut stream, _) = listener.accept().expect("server should accept");
        stream
            .write_all(
                b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\nConnection: close\r\n\r\n4\r\nWiki\r\n5;kind=text\r\npedia\r\n0\r\nX-Finished: yes\r\n\r\n",
            )
            .expect("chunked response should write");
    });
    let mut client = std::net::TcpStream::connect(address).expect("client should connect");
    let response = super::read_http_response_from_stream(
        &mut client,
        Some(Instant::now() + StdDuration::from_secs(2)),
        Some(&CancellationContext::default()),
    )
    .expect("chunked response should decode");
    assert_eq!(
        response.text().expect("chunked body should be UTF-8"),
        "Wikipedia"
    );
    server.join().expect("server thread should join");

    let listener = HttpListenerValue::bind("127.0.0.1:0").expect("HTTP listener should bind");
    let address = listener
        .local_addr()
        .expect("listener address should exist");
    let server_listener = listener.clone();
    let server = thread::spawn(move || {
        let exchange = server_listener
            .accept(
                Some(StdDuration::from_secs(2)),
                Some(&CancellationContext::default()),
            )
            .expect("chunked request should be accepted");
        assert_eq!(
            exchange.body_text().expect("request body should decode"),
            "aurora"
        );
        exchange
            .respond_text(204, "", Vec::new())
            .expect("response should write");
    });
    let mut client = std::net::TcpStream::connect(address).expect("client should connect");
    client
        .write_all(
            b"POST /chunked HTTP/1.1\r\nHost: local\r\nTransfer-Encoding: chunked\r\n\r\n3\r\naur\r\n3\r\nora\r\n0\r\n\r\n",
        )
        .expect("chunked request should write");
    client
        .shutdown(std::net::Shutdown::Write)
        .expect("request write side should close");
    let mut response = String::new();
    client
        .read_to_string(&mut response)
        .expect("server response should read");
    assert!(response.starts_with("HTTP/1.1 204 No Content"));
    server.join().expect("server thread should join");
    listener.close();
}

#[cfg(unix)]
#[test]
fn https_client_uses_tls_validation_and_decodes_chunked_responses() {
    let temp = TempDir::new("aurora-https-client");
    let certificate =
        generate_simple_self_signed(vec!["localhost".to_string()]).expect("cert generation");
    let cert_path = temp.path().join("cert.pem");
    let key_path = temp.path().join("key.pem");
    fs::write(&cert_path, certificate.cert.pem()).expect("certificate should write");
    fs::write(&key_path, certificate.key_pair.serialize_pem()).expect("key should write");
    let listener = TlsListenerValue::bind(
        "127.0.0.1:0",
        cert_path.to_str().expect("UTF-8 certificate path"),
        key_path.to_str().expect("UTF-8 key path"),
    )
    .expect("TLS listener should bind");
    let address = listener.local_addr().expect("TLS address should exist");
    let server_listener = listener.clone();
    let server = thread::spawn(move || {
        let stream = server_listener
            .accept(
                Some(StdDuration::from_secs(2)),
                Some(&CancellationContext::default()),
            )
            .expect("TLS server should accept");
        loop {
            let line = stream
                .read_line(
                    Some(StdDuration::from_secs(2)),
                    Some(&CancellationContext::default()),
                )
                .expect("HTTPS request line should read")
                .expect("HTTPS client should not close before headers");
            if line.is_empty() {
                break;
            }
        }
        stream
            .write_all(
                "HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\nConnection: close\r\n\r\n6\r\nsecure\r\n0\r\n\r\n",
                Some(StdDuration::from_secs(2)),
                Some(&CancellationContext::default()),
            )
            .expect("HTTPS response should write");
        stream.close();
    });
    let response = HttpResponseValue::request_text_with_ca(
        "GET",
        &format!("https://localhost:{}/", address.rsplit_once(':').unwrap().1),
        "",
        Vec::new(),
        Some(StdDuration::from_secs(2)),
        Some(&CancellationContext::default()),
        cert_path.to_str().expect("UTF-8 certificate path"),
    )
    .expect("HTTPS request should validate the configured CA and succeed");
    assert_eq!(response.status(), 200);
    assert_eq!(response.text().expect("HTTPS body should decode"), "secure");
    server.join().expect("HTTPS server should join");
    listener.close();

    let listener = TlsListenerValue::bind(
        "127.0.0.1:0",
        cert_path.to_str().expect("UTF-8 certificate path"),
        key_path.to_str().expect("UTF-8 key path"),
    )
    .expect("second TLS listener should bind");
    let address = listener
        .local_addr()
        .expect("second TLS address should exist");
    let server_listener = listener.clone();
    let server = thread::spawn(move || {
        let stream = server_listener
            .accept(
                Some(StdDuration::from_secs(2)),
                Some(&CancellationContext::default()),
            )
            .expect("second TLS server should accept");
        loop {
            let line = stream
                .read_line(
                    Some(StdDuration::from_secs(2)),
                    Some(&CancellationContext::default()),
                )
                .expect("HTTPS request line should read")
                .expect("HTTPS client should not close before headers");
            if line.is_empty() {
                break;
            }
        }
        stream
            .write_all(
                "HTTP/1.1 200 OK\r\nConnection: close\r\n\r\nsecure-close",
                Some(StdDuration::from_secs(2)),
                Some(&CancellationContext::default()),
            )
            .expect("HTTPS response without content length should write");
        stream.close();
    });
    let response = HttpResponseValue::request_text_with_ca(
        "GET",
        &format!("https://localhost:{}/", address.rsplit_once(':').unwrap().1),
        "",
        Vec::new(),
        Some(StdDuration::from_secs(2)),
        Some(&CancellationContext::default()),
        cert_path.to_str().expect("UTF-8 certificate path"),
    )
    .expect("HTTPS response without content length should read until close");
    assert_eq!(response.status(), 200);
    assert_eq!(
        response.text().expect("HTTPS body should decode"),
        "secure-close"
    );
    server.join().expect("second HTTPS server should join");
    listener.close();
}

#[test]
fn http_stream_helpers_report_unexpected_eof_for_incomplete_messages() {
    fn assert_unexpected_eof(error: io::Error) {
        assert_eq!(error.kind(), io::ErrorKind::UnexpectedEof);
    }

    let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("server should bind");
    let address = listener.local_addr().expect("server address should exist");
    let server = thread::spawn(move || {
        let (mut stream, _) = listener.accept().expect("server should accept");
        super::read_http_request_from_stream(
            &mut stream,
            Some(Instant::now() + StdDuration::from_secs(2)),
            Some(&CancellationContext::default()),
        )
        .expect_err("incomplete request head should fail with EOF")
    });
    let mut client = std::net::TcpStream::connect(address).expect("client should connect");
    client
        .write_all(b"GET /partial HTTP/1.1\r\nHost: example.test")
        .expect("partial request head should write");
    client
        .shutdown(std::net::Shutdown::Write)
        .expect("client write side should close");
    assert_unexpected_eof(server.join().expect("server thread should join"));

    let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("server should bind");
    let address = listener.local_addr().expect("server address should exist");
    let server = thread::spawn(move || {
        let (mut stream, _) = listener.accept().expect("server should accept");
        super::read_http_request_from_stream(
            &mut stream,
            Some(Instant::now() + StdDuration::from_secs(2)),
            Some(&CancellationContext::default()),
        )
        .expect_err("short request body should fail with EOF")
    });
    let mut client = std::net::TcpStream::connect(address).expect("client should connect");
    client
        .write_all(b"POST /body HTTP/1.1\r\nHost: example.test\r\nContent-Length: 4\r\n\r\nok")
        .expect("short request body should write");
    client
        .shutdown(std::net::Shutdown::Write)
        .expect("client write side should close");
    assert_unexpected_eof(server.join().expect("server thread should join"));

    let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("server should bind");
    let address = listener.local_addr().expect("server address should exist");
    let server = thread::spawn(move || {
        let (mut stream, _) = listener.accept().expect("server should accept");
        stream
            .write_all(b"HTTP/1.1 200")
            .expect("partial response head should write");
    });
    let mut client = std::net::TcpStream::connect(address).expect("client should connect");
    let error = super::read_http_response_from_stream(
        &mut client,
        Some(Instant::now() + StdDuration::from_secs(2)),
        Some(&CancellationContext::default()),
    )
    .expect_err("incomplete response head should fail with EOF");
    assert_unexpected_eof(error);
    server.join().expect("server thread should join");

    let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("server should bind");
    let address = listener.local_addr().expect("server address should exist");
    let server = thread::spawn(move || {
        let (mut stream, _) = listener.accept().expect("server should accept");
        stream
            .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 4\r\n\r\nok")
            .expect("short response body should write");
    });
    let mut client = std::net::TcpStream::connect(address).expect("client should connect");
    let error = super::read_http_response_from_stream(
        &mut client,
        Some(Instant::now() + StdDuration::from_secs(2)),
        Some(&CancellationContext::default()),
    )
    .expect_err("short response body should fail with EOF");
    assert_unexpected_eof(error);
    server.join().expect("server thread should join");
}

#[test]
fn http_stream_helpers_read_split_request_and_response_bodies() {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("server should bind");
    let address = listener.local_addr().expect("server address should exist");
    let server = thread::spawn(move || {
        let (mut stream, _) = listener.accept().expect("server should accept");
        super::read_http_request_from_stream(
            &mut stream,
            Some(Instant::now() + StdDuration::from_secs(2)),
            Some(&CancellationContext::default()),
        )
        .expect("split request body should read")
    });
    let mut client = std::net::TcpStream::connect(address).expect("client should connect");
    client
        .write_all(b"POST /split HTTP/1.1\r\nHost: example.test\r\nContent-Length: 4\r\n\r\n")
        .expect("request head should write");
    client.flush().expect("request head should flush");
    thread::sleep(StdDuration::from_millis(10));
    client
        .write_all(b"body")
        .expect("request body should write later");
    client
        .shutdown(std::net::Shutdown::Write)
        .expect("client write side should close");
    let (_, path, _, body) = server.join().expect("server thread should join");
    assert_eq!(path, "/split");
    assert_eq!(body, b"body");

    let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("server should bind");
    let address = listener.local_addr().expect("server address should exist");
    let server = thread::spawn(move || {
        let (mut stream, _) = listener.accept().expect("server should accept");
        stream
            .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 4\r\n\r\n")
            .expect("response head should write");
        stream.flush().expect("response head should flush");
        thread::sleep(StdDuration::from_millis(10));
        stream
            .write_all(b"pong")
            .expect("response body should write");
    });
    let mut client = std::net::TcpStream::connect(address).expect("client should connect");
    let response = super::read_http_response_from_stream(
        &mut client,
        Some(Instant::now() + StdDuration::from_secs(2)),
        Some(&CancellationContext::default()),
    )
    .expect("split response body should read");
    assert_eq!(
        response.text().expect("response body should decode"),
        "pong"
    );
    server.join().expect("server thread should join");
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
        assert_eq!(
            Value::HttpExchange(exchange.clone()).render(),
            "<http-exchange>"
        );
        assert_value_equals_clone(Value::HttpExchange(exchange.clone()));
        assert_cast_source_type(Value::HttpExchange(exchange.clone()), "net.HttpExchange");
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

#[test]
fn http_listener_replies_with_400_for_malformed_requests_and_continues_accepting() {
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
            .expect("server should skip the malformed request and accept the next client");
        assert_eq!(exchange.method(), "GET");
        assert_eq!(exchange.path(), "/ok");
        exchange
            .respond_text(200, "ok", Vec::new())
            .expect("server should reply to the valid request");
    });

    let mut client =
        std::net::TcpStream::connect(&address).expect("http client should connect to listener");
    client
        .write_all(b"GE T /oops HTTP/1.1\r\nHost: malformed\r\n\r\n")
        .expect("malformed request should write");
    client
        .shutdown(std::net::Shutdown::Write)
        .expect("client shutdown should succeed");

    let mut response = String::new();
    client
        .read_to_string(&mut response)
        .expect("client should receive an HTTP response");
    assert!(
        response.starts_with("HTTP/1.1 400 Bad Request\r\n"),
        "expected a 400 response, got: {response:?}"
    );

    let response = HttpResponseValue::request_text(
        "GET",
        &format!("http://{}/ok", address),
        "",
        Vec::new(),
        Some(StdDuration::from_secs(2)),
        Some(&CancellationContext::default()),
    )
    .expect("listener should continue accepting after a malformed request");
    assert_eq!(response.status(), 200);
    assert_eq!(
        response.text().expect("response body should decode"),
        "ok".to_string()
    );

    server_thread
        .join()
        .expect("malformed-request server thread should join");
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
            assert_eq!(format!("{exchange:?}"), "HttpExchangeValue(..)");
            assert_eq!(exchange, exchange.clone());
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
    let socket_path = unique_unix_socket_path("a-ut");
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
    let _ = fs::remove_file(&socket_path);

    let certificate =
        generate_simple_self_signed(vec!["localhost".to_string()]).expect("cert generation");
    let cert_pem = certificate.cert.pem();
    let key_pem = certificate.key_pair.serialize_pem();
    let cert_path = temp.path().join("cert.pem");
    let key_path = temp.path().join("key.pem");
    fs::write(&cert_path, cert_pem.as_bytes()).expect("write cert pem");
    fs::write(&key_path, key_pem.as_bytes()).expect("write key pem");
    let missing_key_path = temp.path().join("missing-key.pem");
    fs::write(&missing_key_path, b"").expect("write empty key pem");
    let missing_key_error = super::load_tls_server_config(
        cert_path.to_str().expect("cert path should be UTF-8"),
        missing_key_path
            .to_str()
            .expect("missing key path should be UTF-8"),
    )
    .expect_err("TLS server config should reject PEM files without private keys");
    assert_eq!(missing_key_error.kind(), io::ErrorKind::InvalidInput);
    assert!(missing_key_error
        .to_string()
        .contains("did not contain a key"));
    super::load_tls_root_store(Some(cert_path.to_str().expect("cert path should be UTF-8")))
        .expect("custom CA PEM should extend the TLS root store");

    let tls_listener = TlsListenerValue::bind(
        "127.0.0.1:0",
        cert_path.to_str().expect("cert path should be valid UTF-8"),
        key_path.to_str().expect("key path should be valid UTF-8"),
    )
    .expect("tls listener bind should succeed");
    assert_eq!(
        Value::TlsListener(tls_listener.clone()).render(),
        "<tls-listener>"
    );
    assert_value_equals_clone(Value::TlsListener(tls_listener.clone()));
    assert_cast_source_type(Value::TlsListener(tls_listener.clone()), "net.TlsListener");
    assert_eq!(format!("{tls_listener:?}"), "TlsListenerValue(..)");
    assert_eq!(tls_listener, tls_listener.clone());
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
    assert_eq!(
        Value::TlsStream(tls_client.clone()).render(),
        "<tls-stream>"
    );
    assert_value_equals_clone(Value::TlsStream(tls_client.clone()));
    assert_cast_source_type(Value::TlsStream(tls_client.clone()), "net.TlsStream");
    assert_eq!(format!("{tls_client:?}"), "TlsStreamValue(..)");
    assert_eq!(tls_client, tls_client.clone());
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

#[cfg(unix)]
#[test]
fn tls_listener_accept_is_not_linearly_delayed_by_multiple_stalled_peers() {
    let temp = TempDir::new("aurora-runtime-tls-multi-slowloris");
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
                Some(StdDuration::from_secs(25)),
                Some(&CancellationContext::default()),
            )
            .expect("tls listener should accept the legitimate peer without linear delay");
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

    let stalled_a = {
        let address = address.clone();
        thread::spawn(move || {
            let _client =
                std::net::TcpStream::connect(address).expect("plain tcp client should connect");
            thread::sleep(StdDuration::from_secs(11));
        })
    };
    let stalled_b = {
        let address = address.clone();
        thread::spawn(move || {
            let _client =
                std::net::TcpStream::connect(address).expect("plain tcp client should connect");
            thread::sleep(StdDuration::from_secs(11));
        })
    };

    thread::sleep(StdDuration::from_millis(100));
    let start = Instant::now();
    let tls_client = TlsStreamValue::connect(
        &address,
        "localhost",
        Some(cert_path.to_str().expect("cert path should be valid UTF-8")),
        Some(StdDuration::from_secs(25)),
        Some(&CancellationContext::default()),
    )
    .expect("tls connect should succeed after the stalled peers are queued");
    let elapsed = start.elapsed();
    assert!(
        elapsed < StdDuration::from_secs(5),
        "legitimate tls clients should not be delayed linearly by stalled peers; elapsed {:?}",
        elapsed
    );
    tls_client
        .write_all(
            "ready\n",
            Some(StdDuration::from_secs(2)),
            Some(&CancellationContext::default()),
        )
        .expect("tls client write_all should succeed");
    let reply = tls_client
        .read_exact(
            2,
            Some(StdDuration::from_secs(2)),
            Some(&CancellationContext::default()),
        )
        .expect("tls client read_exact should succeed");
    assert_eq!(reply, b"ok");

    stalled_a.join().expect("stalled tls client should join");
    stalled_b.join().expect("stalled tls client should join");
    server_thread.join().expect("tls server thread should join");
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

    let socket_path = unique_unix_socket_path("a-ev");
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
    let _ = fs::remove_file(&socket_path);

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
        assert_eq!(format!("{socket:?}"), "WebSocketValue(..)");
        assert_eq!(socket, socket.clone());
        assert_eq!(Value::WebSocket(socket.clone()).render(), "<websocket>");
        assert_value_equals_clone(Value::WebSocket(socket.clone()));
        assert_cast_source_type(Value::WebSocket(socket.clone()), "net.WebSocket");
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
    assert_eq!(format!("{ws_client:?}"), "WebSocketValue(..)");
    assert_eq!(ws_client, ws_client.clone());
    assert_eq!(Value::WebSocket(ws_client.clone()).render(), "<websocket>");
    assert_value_equals_clone(Value::WebSocket(ws_client.clone()));
    assert_cast_source_type(Value::WebSocket(ws_client.clone()), "net.WebSocket");
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
