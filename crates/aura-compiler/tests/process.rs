use std::fs;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use aura_compiler::{check_path_with_source, run_path_with_source};

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
fn wait_for_path(path: &std::path::Path, timeout_ms: u64) -> bool {
    let deadline = std::time::Instant::now() + std::time::Duration::from_millis(timeout_ms);
    while std::time::Instant::now() < deadline {
        if path.exists() {
            return true;
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    path.exists()
}

#[cfg(unix)]
fn process_alive(pid: i32) -> bool {
    let result = unsafe { libc::kill(pid, 0) };
    if result == 0 {
        true
    } else {
        let error = std::io::Error::last_os_error();
        match error.raw_os_error() {
            Some(libc::EPERM) => true,
            Some(libc::ESRCH) => false,
            _ => false,
        }
    }
}

#[cfg(unix)]
fn kill_process_group(pid: i32) {
    unsafe {
        libc::kill(-pid, libc::SIGKILL);
    }
}

#[test]
fn builtin_process_module_type_checks_from_path_context() {
    let temp = TempDir::new("aura-process-check");
    let entry = temp.path().join("main.au");
    let source = r#"import process

def inspect(child: process.Child, pipe: process.Pipe, completed: process.Completed, status: process.ExitStatus, wait: process.Wait, stdio: process.Stdio, error: process.Error, supervisor: process.Supervisor, event: process.SupervisorEvent, supervisor_wait: process.SupervisorWait, restart: process.RestartPolicy) -> int32:
    match own child.stdin():
        case Option.Some(stdin_pipe):
            print(stdin_pipe.write_all("hello\n", timeout=10ms))
            stdin_pipe.close()
        case Option.None:
            pass
    match own child.stdout():
        case Option.Some(stdout_pipe):
            print(stdout_pipe.read_line(timeout=10ms))
        case Option.None:
            pass
    match own child.stderr():
        case Option.Some(stderr_pipe):
            print(stderr_pipe.read_all())
        case Option.None:
            pass
    print(child.wait(timeout=10ms))
    print(child.wait_or_none(timeout=10ms))
    print(child.wait_ok(timeout=10ms))
    print(child.kill())
    print(child.terminate())
    print(pipe.read_all())
    print(pipe.read_line(timeout=10ms))
    print(pipe.read_bytes(32, timeout=10ms))
    print(pipe.write_all("hello", timeout=10ms))
    print(pipe.write_bytes([65 as uint8], timeout=10ms))
    print(pipe.flush())
    pipe.close()
    print(completed.status())
    print(completed.success())
    print(completed.stdout())
    print(completed.stderr())
    print(completed.check())
    print(status)
    print(wait)
    print(stdio)
    print(error)
    print(supervisor.start(name="checker", command=["/usr/bin/false"], restart=process.RestartPolicy.OnFailure, backoff=10ms, max_restarts=1, group=true))
    print(supervisor.wait(timeout=10ms))
    print(supervisor.wait_or_none(timeout=10ms))
    print(supervisor.stop())
    print(supervisor.is_empty())
    supervisor.close()
    print(event)
    print(supervisor_wait)
    print(restart)
    return 0

def boot() -> Result[None, process.Error]:
    env: Map[String, String] = {"AURA_PROCESS_VAR": "present"}
    with running = try process.start(["/bin/cat"], cwd=Option.None, env=env.clone(), stdin=process.pipe(), stdout=process.pipe(), stderr=process.null(), group=true):
        print(running.stdin())
        print(running.stdout())
        print(running.stderr())
        print(running.wait(timeout=10ms))
        print(running.wait_or_none(timeout=10ms))
        print(running.wait_ok(timeout=10ms))

    completed = try process.run(["/usr/bin/printenv", "AURA_PROCESS_VAR"], cwd=Option.None, env=env, stdin=process.null(), stdout=process.pipe(), stderr=process.pipe(), timeout=1s, group=true)
    print(completed.status())
    print(completed.success())
    print(completed.stdout())
    print(completed.stderr())
    print(process.inherit())
    print(process.null())
    print(process.pipe())
    print(process.supervisor())
    return Result.Ok(None)

def main() -> int32:
    match own boot():
        case Result.Ok(_):
            return 0
        case Result.Err(error):
            print(error)
            return 1
"#;

    check_path_with_source(&entry, source).expect("builtin process module should type-check");
}

#[test]
fn builtin_process_module_runs_through_public_api() {
    let temp = TempDir::new("aura-process-run");
    let entry = temp.path().join("main.au");
    let cwd = fs::canonicalize(temp.path())
        .expect("temp path should canonicalize")
        .display()
        .to_string();
    let source = format!(
        r#"import process

def run_env() -> Result[None, process.Error]:
    env: Map[String, String] = {{"AURA_PROCESS_VAR": "present"}}
    completed = try process.run(["/usr/bin/printenv", "AURA_PROCESS_VAR"], cwd=Option.None, env=env, stdin=process.null(), stdout=process.pipe(), stderr=process.pipe(), timeout=2s, group=true)
    print(completed.stdout().trim())
    print(completed.stderr().len())
    print(completed.status())
    return Result.Ok(None)

def run_pwd(cwd: own String) -> Result[None, process.Error]:
    completed = try process.run(["/bin/pwd"], cwd=Option.Some(cwd), env={{}}, stdin=process.null(), stdout=process.pipe(), stderr=process.pipe(), timeout=2s, group=true)
    print(completed.stdout().trim())
    print(completed.stderr().len())
    print(completed.status())
    return Result.Ok(None)

def echo_with_cat() -> Result[None, process.Error]:
    with child = try process.start(["/bin/cat"], stdin=process.pipe(), stdout=process.pipe(), stderr=process.null()):
        match own child.stdin():
            case Option.Some(stdin_pipe):
                try stdin_pipe.write_all("echo from cat\n", timeout=500ms)
                try stdin_pipe.flush()
                stdin_pipe.close()
            case Option.None:
                print("missing stdin")
                return Result.Ok(None)

        match own child.stdout():
            case Option.Some(stdout_pipe):
                line = try stdout_pipe.read_line(timeout=500ms)
                match own line:
                    case Option.Some(text):
                        print(text)
                    case Option.None:
                        print("missing stdout text")
                        return Result.Ok(None)
            case Option.None:
                print("missing stdout")
                return Result.Ok(None)

        print(try child.wait_ok(timeout=2s))
    return Result.Ok(None)

def supervise_flaky_process() -> Result[None, process.Error]:
    with supervisor = process.supervisor():
        try supervisor.start(name="flaky", command=["/usr/bin/false"], restart=process.RestartPolicy.OnFailure, backoff=10ms, max_restarts=1, group=true)
        match own try supervisor.wait_or_none(timeout=500ms):
            case Option.Some(event):
                print(event)
            case Option.None:
                print("missing first supervisor event")
                return Result.Ok(None)
        match own try supervisor.wait_or_none(timeout=500ms):
            case Option.Some(event):
                print(event)
            case Option.None:
                print("missing second supervisor event")
                return Result.Ok(None)
        print(supervisor.is_empty())
        try supervisor.start(name="sleeper", command=["/bin/sleep", "1"], restart=process.RestartPolicy.Never, group=true)
        print(supervisor.is_empty())
        try supervisor.stop()
        print(supervisor.is_empty())
    return Result.Ok(None)

def main() -> int32:
    match own run_env():
        case Result.Ok(_):
            pass
        case Result.Err(error):
            print(error)
            return 1

    match own run_pwd("{cwd}"):
        case Result.Ok(_):
            pass
        case Result.Err(error):
            print(error)
            return 1

    match own run_checked_echo():
        case Result.Ok(_):
            pass
        case Result.Err(error):
            print(error)
            return 1

    match own wait_for_sleep_timeout():
        case Result.Ok(_):
            pass
        case Result.Err(error):
            print(error)
            return 1

    match own echo_with_cat():
        case Result.Ok(_):
            pass
        case Result.Err(error):
            print(error)
            return 1

    match own supervise_flaky_process():
        case Result.Ok(_):
            return 0
        case Result.Err(error):
            print(error)
            return 1

def run_checked_echo() -> Result[None, process.Error]:
    completed = try process.run(["/bin/echo", "checked"], stdout=process.pipe(), stderr=process.pipe(), timeout=2s, group=true)
    try completed.check()
    print(completed.stdout().trim())
    return Result.Ok(None)

def wait_for_sleep_timeout() -> Result[None, process.Error]:
    with sleeping = try process.start(["/bin/sleep", "1"], stdin=process.null(), stdout=process.null(), stderr=process.null()):
        print(try sleeping.wait_or_none(timeout=1ms))
    return Result.Ok(None)
"#,
        cwd = cwd,
    );

    let output = run_path_with_source(&entry, &source).expect("builtin process module should run");
    assert_eq!(
        output.stdout,
        format!(
            "present\n0\nExitStatus.Exited(0)\n{cwd}\n0\nExitStatus.Exited(0)\nchecked\nOption.None\necho from cat\nExitStatus.Exited(0)\nSupervisorEvent.Restarted(flaky, ExitStatus.Exited(1), 1)\nSupervisorEvent.Exited(flaky, ExitStatus.Exited(1), 1)\ntrue\nfalse\ntrue\n",
            cwd = cwd,
        )
    );
}

#[cfg(unix)]
#[test]
fn zero_sized_process_pipe_read_returns_typed_invalid_input_without_consuming() {
    let temp = TempDir::new("aura-process-zero-read");
    let entry = temp.path().join("main.au");
    let source = r#"import process

def probe() -> Result[None, process.Error]:
    with child = try process.start(["/bin/sh", "-c", "printf x"], stdin=process.null(), stdout=process.pipe(), stderr=process.null()):
        match own child.stdout():
            case Option.Some(pipe):
                with output = pipe:
                    print(output.read_bytes(0, timeout=1s))
                    print(output.read_bytes(1, timeout=1s))
            case Option.None:
                pass
    return Result.Ok(None)

def main() -> int32:
    match own probe():
        case Result.Ok(_):
            return 0
        case Result.Err(error):
            print(error)
            return 1
"#;

    let output = run_path_with_source(&entry, source)
        .expect("zero-sized process reads should be represented as typed results");
    assert_eq!(
        output.stdout,
        "Result.Err(Error.Io(io.Error.InvalidInput))\nResult.Ok(Option.Some([120]))\n"
    );
}

#[cfg(unix)]
#[test]
fn grouped_process_close_terminates_descendants() {
    let temp = TempDir::new("aura-process-group");
    let entry = temp.path().join("main.au");
    let script = temp.path().join("spawn_sleeping_child.py");
    let pid_file = temp.path().join("descendant.pid");
    let ready_file = temp.path().join("ready.txt");

    fs::write(
        &script,
        r#"import subprocess
import sys
import time

pid_file = sys.argv[1]
ready_file = sys.argv[2]
child = subprocess.Popen(["/bin/sleep", "30"])
with open(pid_file, "w", encoding="utf-8") as handle:
    handle.write(str(child.pid))
with open(ready_file, "w", encoding="utf-8") as handle:
    handle.write("ready")
time.sleep(30)
"#,
    )
    .expect("failed to write process group helper script");

    let source = format!(
        r#"import fs
import process

def run_group_cleanup() -> Result[None, process.Error]:
    with child = try process.start(["/usr/bin/env", "python3", "{script}", "{pid_file}", "{ready_file}"], stdin=process.null(), stdout=process.null(), stderr=process.null(), group=true):
        for _ in range(200):
            if fs.exists("{ready_file}"):
                child.close()
                return Result.Ok(None)
            sleep(10ms)
        child.close()
        return Result.Ok(None)

def main() -> int32:
    match own run_group_cleanup():
        case Result.Ok(_):
            return 0
        case Result.Err(error):
            print(error)
            return 2
"#,
        script = script.display(),
        pid_file = pid_file.display(),
        ready_file = ready_file.display(),
    );

    let output = run_path_with_source(&entry, &source).expect("grouped child program should run");
    assert_eq!(output.value.render(), "0");
    assert!(
        wait_for_path(&pid_file, 1000),
        "descendant pid file should be written before group shutdown"
    );

    let pid_text = fs::read_to_string(&pid_file).expect("pid file should be readable");
    let pid: i32 = pid_text
        .trim()
        .parse()
        .expect("pid file should contain a valid pid");

    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(2);
    while process_alive(pid) && std::time::Instant::now() < deadline {
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    assert!(
        !process_alive(pid),
        "grouped child close should terminate descendants in the same process group"
    );
}

#[cfg(unix)]
#[test]
fn supervisor_duplicate_name_does_not_leave_unmanaged_child_running() {
    let temp = TempDir::new("aura-supervisor-duplicate");
    let entry = temp.path().join("main.au");
    let first_pid_file = temp.path().join("first.pid");
    let second_pid_file = temp.path().join("second.pid");

    let source = format!(
        r#"import process

def run_duplicate(first_pid: String, second_pid: String) -> Result[None, process.Error]:
    with supervisor = process.supervisor():
        try supervisor.start(name="dup", command=["/bin/sh", "-c", "echo $$ > " + first_pid + "; sleep 30"], stdout=process.null(), stderr=process.null(), group=true)
        match own supervisor.start(name="dup", command=["/bin/sh", "-c", "echo $$ > " + second_pid + "; sleep 30"], stdout=process.null(), stderr=process.null(), group=true):
            case Result.Ok(_):
                print("unexpected duplicate success")
            case Result.Err(_):
                print("duplicate rejected")
        try supervisor.stop()
    return Result.Ok(None)

def main() -> int32:
    match own run_duplicate("{first_pid}", "{second_pid}"):
        case Result.Ok(_):
            return 0
        case Result.Err(error):
            print(error)
            return 1
"#,
        first_pid = first_pid_file.display(),
        second_pid = second_pid_file.display(),
    );

    let output = run_path_with_source(&entry, &source)
        .expect("duplicate supervisor program should complete");
    assert_eq!(output.stdout, "duplicate rejected\n");

    let second_pid = fs::read_to_string(&second_pid_file)
        .ok()
        .and_then(|text| text.trim().parse::<i32>().ok());
    let second_alive = second_pid.map(process_alive).unwrap_or(false);
    if second_alive {
        if let Some(pid) = second_pid {
            kill_process_group(pid);
        }
    }
    assert!(
        !second_alive,
        "duplicate-name supervisor error should not leave the rejected child running"
    );
}
