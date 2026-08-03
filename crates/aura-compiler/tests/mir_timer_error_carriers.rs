use aura_compiler::run_source;

#[test]
fn mir_network_timer_conversion_errors_stay_inside_the_io_result_carrier() {
    let source = r#"
import net

def main() -> int32:
    print(net.connect_timeout("127.0.0.1:1", Duration.ms(-1)))
    print(net.unix_connect_timeout("/unused/aura.sock", Duration.ms(-1)))
    print(net.tls_connect_timeout("127.0.0.1:1", "localhost", "/unused/ca.pem", Duration.ms(-1)))
    print(net.http_request_text_timeout("GET", "http://127.0.0.1:1/", "", {}, Duration.ms(-1)))
    print(net.http_request_bytes_timeout("GET", "http://127.0.0.1:1/", list[uint8](), {}, Duration.ms(-1)))
    print(net.websocket_connect_timeout("ws://127.0.0.1:1/", Duration.ms(-1)))
    print(net.connect_timeout("127.0.0.1:1", Duration.minutes(9223372036854775807)))
    print(net.connect_timeout("127.0.0.1:1", Duration.seconds(9223372036854775807)))
    print(net.unix_connect_timeout("/unused/aura.sock", Duration.seconds(9223372036854775807)))
    print(net.tls_connect_timeout("127.0.0.1:1", "localhost", "/unused/ca.pem", Duration.seconds(9223372036854775807)))
    print(net.http_request_text_timeout("GET", "http://127.0.0.1:1/", "", {}, Duration.seconds(9223372036854775807)))
    print(net.http_request_bytes_timeout("GET", "http://127.0.0.1:1/", list[uint8](), {}, Duration.seconds(9223372036854775807)))
    print(net.websocket_connect_timeout("ws://127.0.0.1:1/", Duration.seconds(9223372036854775807)))
    return 0
"#;

    let output = run_source(source).expect("invalid network timers should be typed values");
    assert_eq!(
        output.stdout,
        "Result.Err(io.Error.InvalidInput)\n".repeat(13)
    );
}

#[test]
fn mir_supervisor_timer_conversion_uses_each_declared_process_carrier() {
    let source = r#"
import process

def main() -> int32:
    supervisor = process.supervisor()
    print(supervisor.wait(timeout=Duration.ms(-1)))
    print(supervisor.wait(timeout=Duration.seconds(9223372036854775807)))
    print(supervisor.wait_or_none(timeout=Duration.minutes(9223372036854775807)))
    print(supervisor.wait_or_none(timeout=Duration.seconds(9223372036854775807)))
    supervisor.close()
    return 0
"#;

    let output = run_source(source).expect("invalid supervisor timers should be typed values");
    let mut lines = output.stdout.lines();
    for label in ["conversion", "deadline"] {
        let wait = lines.next().expect("wait output");
        assert!(
            wait.starts_with("SupervisorWait.Event(SupervisorEvent.Failed("),
            "unexpected {label} wait carrier: {wait}"
        );
        assert!(
            wait.contains("Error.Io(io.Error.InvalidInput)"),
            "unexpected {label} wait error: {wait}"
        );
    }
    for _ in 0..2 {
        assert_eq!(
            lines.next(),
            Some("Result.Err(Error.Io(io.Error.InvalidInput))")
        );
    }
    assert_eq!(lines.next(), None);
}

#[test]
fn mir_no_carrier_timer_failures_are_explicit_au4001_traps() {
    let negative_sleep = r#"
def main() -> int32:
    sleep(Duration.ms(-1))
    return 0
"#;
    let error = run_source(negative_sleep).expect_err("negative sleep must trap");
    assert_eq!(error.code, "AU4001");

    for (label, source) in [
        (
            "put(timeout=...)",
            r#"
def main() -> int32:
    queue = Queue[int32]()
    queue.put(1, timeout=Duration.ms(-1))
    return 0
"#,
        ),
        (
            "get(timeout=...)",
            r#"
def main() -> int32:
    queue = Queue[int32]()
    print(queue.get(timeout=Duration.ms(-1)))
    return 0
"#,
        ),
        (
            "get_or_none(timeout=...)",
            r#"
def main() -> int32:
    queue = Queue[int32]()
    print(queue.get_or_none(timeout=Duration.ms(-1)))
    return 0
"#,
        ),
        (
            "get_or(timeout=...)",
            r#"
def main() -> int32:
    queue = Queue[int32]()
    print(queue.get_or(0, timeout=Duration.ms(-1)))
    return 0
"#,
        ),
        (
            "result(timeout=...)",
            r#"
def worker() -> int32:
    return 1

def main() -> int32:
    with TaskGroup() as group:
        task = group.start(worker)
        print(task.result(timeout=Duration.ms(-1)))
    return 0
"#,
        ),
        (
            "result_or_none(timeout=...)",
            r#"
def worker() -> int32:
    return 1

def main() -> int32:
    with TaskGroup() as group:
        task = group.start(worker)
        print(task.result_or_none(timeout=Duration.ms(-1)))
    return 0
"#,
        ),
        (
            "result_or(timeout=...)",
            r#"
def worker() -> int32:
    return 1

def main() -> int32:
    with TaskGroup() as group:
        task = group.start(worker)
        print(task.result_or(0, timeout=Duration.ms(-1)))
    return 0
"#,
        ),
        (
            "wait_any(timeout=...)",
            r#"
def main() -> int32:
    tasks = list[Task[int32]]()
    print(wait_any(tasks, timeout=Duration.ms(-1)))
    return 0
"#,
        ),
        (
            "wait_all(timeout=...)",
            r#"
def main() -> int32:
    tasks = list[Task[int32]]()
    print(wait_all(tasks, timeout=Duration.ms(-1)))
    return 0
"#,
        ),
    ] {
        let error = run_source(source).expect_err("negative timer must trap");
        assert_eq!(error.code, "AU4001", "unexpected code for {label}");
        assert!(
            error.message.contains(label),
            "diagnostic should identify {label}: {}",
            error.message
        );
    }

    let sleep_deadline_overflow = r#"
def main() -> int32:
    sleep(Duration.seconds(9223372036854775807))
    return 0
"#;
    let error = run_source(sleep_deadline_overflow).expect_err("sleep deadline must trap");
    assert_eq!(error.code, "AU4001");

    let queue_deadline_overflow = r#"
def main() -> int32:
    queue = Queue[int32]()
    print(queue.get(timeout=Duration.seconds(9223372036854775807)))
    return 0
"#;
    let error = run_source(queue_deadline_overflow).expect_err("queue deadline must trap");
    assert_eq!(error.code, "AU4001");

    let task_deadline_overflow = r#"
def worker() -> int32:
    return 1

def main() -> int32:
    with TaskGroup() as group:
        task = group.start(worker)
        print(task.result(timeout=Duration.seconds(9223372036854775807)))
    return 0
"#;
    let error = run_source(task_deadline_overflow).expect_err("task deadline must trap");
    assert_eq!(error.code, "AU4001");

    let deadline_overflow = r#"
def main() -> int32:
    tasks: list[Task[int32]] = list[Task[int32]]()
    print(wait_all(tasks, timeout=Duration.seconds(9223372036854775807)))
    return 0
"#;
    let error = run_source(deadline_overflow).expect_err("host deadline overflow must trap");
    assert_eq!(error.code, "AU4001");
    assert!(
        error.message.contains("deadline range"),
        "unexpected deadline diagnostic: {}",
        error.message
    );
}
