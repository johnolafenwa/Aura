//! Direct-backend coverage for the `T | None` library results that only a
//! native run exercises: piped stdin lines, child pipe byte reads, and
//! supervisor events (Batch 1 phase 2, D3 rows 10, 37, and 39).

use std::fs;
use std::io::Write;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::time::{SystemTime, UNIX_EPOCH};

fn aura_bin() -> &'static str {
    env!("CARGO_BIN_EXE_aura")
}

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

    fn write(&self, relative: &str, source: &str) -> PathBuf {
        let path = self.path.join(relative);
        fs::write(&path, source).expect("failed to write source");
        path
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.path);
    }
}

fn run_direct(source: &std::path::Path, stdin: Option<&[u8]>) -> std::process::Output {
    let mut command = Command::new(aura_bin());
    command
        .args(["run", "--backend", "direct"])
        .arg(source)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    command.stdin(if stdin.is_some() {
        Stdio::piped()
    } else {
        Stdio::null()
    });
    let mut child = command.spawn().expect("failed to spawn aura run");
    if let Some(bytes) = stdin {
        child
            .stdin
            .take()
            .expect("stdin should be piped")
            .write_all(bytes)
            .expect("failed to write stdin");
    }
    child
        .wait_with_output()
        .expect("failed to collect aura run output")
}

#[test]
fn io_read_line_consumes_piped_stdin_until_eof_on_the_direct_backend() {
    let temp = TempDir::new("aura-cov-direct-stdin");
    let source = temp.write(
        "main.au",
        r#"import io

def main():
    mut lines = 0
    while true:
        match io.read_line():
            case Result.Ok(str as line):
                lines += 1
                print("got " + line)
            case Result.Ok(None):
                print("eof")
                break
            case Result.Err(error):
                print(error)
                break
    print(lines)
"#,
    );
    let output = run_direct(&source, Some(b"first line\r\nsecond\n"));
    assert!(
        output.status.success(),
        "aura run should succeed, stderr was:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&output.stdout),
        "got first line\ngot second\neof\n2\n"
    );
}

#[test]
fn child_pipe_read_bytes_yields_bytes_then_none_on_the_direct_backend() {
    let temp = TempDir::new("aura-cov-direct-pipe");
    let source = temp.write(
        "main.au",
        r#"import process

def read_output() -> Result[None, process.Error]:
    child = try process.start(["/bin/sh", "-c", "printf hello"], stdin=process.null(), stdout=process.pipe(), stderr=process.null())
    match own child.stdout():
        case process.Pipe as out:
            match try out.read_bytes(3, timeout=2s):
                case list[uint8] as bytes:
                    print(bytes.len())
                case None:
                    print("eof")
            match try out.read_bytes(8, timeout=2s):
                case list[uint8] as bytes:
                    print(bytes.len())
                case None:
                    print("eof")
            match try out.read_bytes(8, timeout=2s):
                case list[uint8] as bytes:
                    print(bytes.len())
                case None:
                    print("eof")
        case None:
            print("no pipe")
    print(try child.wait_ok(timeout=2s))
    return Result.Ok(None)

def main():
    match read_output():
        case Result.Ok(_):
            pass
        case Result.Err(error):
            print(error)
"#,
    );
    let output = run_direct(&source, None);
    assert!(
        output.status.success(),
        "aura run should succeed, stderr was:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&output.stdout),
        "3\n2\neof\nExitStatus.Exited(0)\n"
    );
}

#[test]
fn supervisor_wait_or_none_reports_events_on_the_direct_backend() {
    let example = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../examples/io/process_supervisor.au");
    let output = run_direct(&example, None);
    assert!(
        output.status.success(),
        "aura run should succeed, stderr was:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&output.stdout),
        "SupervisorEvent.Restarted(flaky, ExitStatus.Exited(1), 1)\nSupervisorEvent.Exited(flaky, ExitStatus.Exited(1), 1)\ntrue\nfalse\ntrue\n"
    );
}

#[test]
fn child_pipes_report_present_and_absent_handles_on_the_direct_backend() {
    let temp = TempDir::new("aura-cov-direct-child-pipes");
    let source = temp.write(
        "main.au",
        r#"import process

def probe() -> Result[None, process.Error]:
    child = try process.start(["/bin/sh", "-c", "printf err 1>&2"], stdin=process.null(), stdout=process.null(), stderr=process.pipe())
    print(child.stdin() is None)
    print(child.stdout() is None)
    match own child.stderr():
        case process.Pipe as err:
            match try err.read_line(timeout=2s):
                case str as line:
                    print(line)
                case None:
                    print("eof")
        case None:
            print("no stderr")
    print(try child.wait_ok(timeout=2s))
    quiet = try process.start(["/bin/sh", "-c", "true"], stdin=process.pipe(), stdout=process.pipe(), stderr=process.null())
    print(quiet.stderr() is None)
    match own quiet.stdin():
        case process.Pipe as input:
            input.close()
            print("stdin pipe")
        case None:
            print("no stdin")
    match own quiet.stdout():
        case process.Pipe as output:
            match try output.read_line(timeout=2s):
                case str as line:
                    print(line)
                case None:
                    print("stdout eof")
        case None:
            print("no stdout")
    print(try quiet.wait_ok(timeout=2s))
    return Result.Ok(None)

def main():
    match probe():
        case Result.Ok(_):
            pass
        case Result.Err(error):
            print(error)
"#,
    );
    let output = run_direct(&source, None);
    assert!(
        output.status.success(),
        "aura run should succeed, stderr was:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&output.stdout),
        "true\ntrue\nerr\nExitStatus.Exited(0)\ntrue\nstdin pipe\nstdout eof\nExitStatus.Exited(0)\n"
    );
}
