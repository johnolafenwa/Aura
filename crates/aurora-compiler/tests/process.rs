use std::fs;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use aurora_compiler::{check_path_with_source, run_path_with_source};

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

#[test]
fn builtin_process_module_type_checks_from_path_context() {
    let temp = TempDir::new("aurora-process-check");
    let entry = temp.path().join("main.au");
    let source = r#"import process

def inspect(child: process.Child, pipe: process.Pipe, completed: process.Completed, status: process.ExitStatus, wait: process.Wait, stdio: process.Stdio, error: process.Error) -> int32:
    match child.stdin():
        case Option.Some(stdin_pipe):
            print(stdin_pipe.write_all("hello\n", timeout=10ms))
            stdin_pipe.close()
        case Option.None:
            pass
    match child.stdout():
        case Option.Some(stdout_pipe):
            print(stdout_pipe.read_line(timeout=10ms))
        case Option.None:
            pass
    match child.stderr():
        case Option.Some(stderr_pipe):
            print(stderr_pipe.read_all())
        case Option.None:
            pass
    print(child.wait(timeout=10ms))
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
    print(status)
    print(wait)
    print(stdio)
    print(error)
    return 0

def boot() -> Result[None, process.Error]:
    env: Map[String, String] = {"AURORA_PROCESS_VAR": "present"}
    with running = try process.start(["/bin/cat"], cwd=Option.None, env=env.clone(), stdin=process.pipe(), stdout=process.pipe(), stderr=process.null()):
        print(running.stdin())
        print(running.stdout())
        print(running.stderr())
        print(running.wait(timeout=10ms))

    completed = try process.run(["/usr/bin/printenv", "AURORA_PROCESS_VAR"], cwd=Option.None, env=env, stdin=process.null(), stdout=process.pipe(), stderr=process.pipe(), timeout=1s)
    print(completed.status())
    print(completed.success())
    print(completed.stdout())
    print(completed.stderr())
    print(process.inherit())
    print(process.null())
    print(process.pipe())
    return Result.Ok(None)

def main() -> int32:
    match boot():
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
    let temp = TempDir::new("aurora-process-run");
    let entry = temp.path().join("main.au");
    let cwd = fs::canonicalize(temp.path())
        .expect("temp path should canonicalize")
        .display()
        .to_string();
    let source = format!(
        r#"import process

def run_env() -> Result[None, process.Error]:
    env: Map[String, String] = {{"AURORA_PROCESS_VAR": "present"}}
    completed = try process.run(["/usr/bin/printenv", "AURORA_PROCESS_VAR"], cwd=Option.None, env=env, stdin=process.null(), stdout=process.pipe(), stderr=process.pipe(), timeout=2s)
    print(completed.stdout().trim())
    print(completed.stderr().len())
    print(completed.status())
    return Result.Ok(None)

def run_pwd(cwd: String) -> Result[None, process.Error]:
    completed = try process.run(["/bin/pwd"], cwd=Option.Some(cwd), env={{}}, stdin=process.null(), stdout=process.pipe(), stderr=process.pipe(), timeout=2s)
    print(completed.stdout().trim())
    print(completed.stderr().len())
    print(completed.status())
    return Result.Ok(None)

def echo_with_cat() -> Result[None, process.Error]:
    with child = try process.start(["/bin/cat"], stdin=process.pipe(), stdout=process.pipe(), stderr=process.null()):
        match child.stdin():
            case Option.Some(stdin_pipe):
                try stdin_pipe.write_all("echo from cat\n", timeout=500ms)
                try stdin_pipe.flush()
                stdin_pipe.close()
            case Option.None:
                print("missing stdin")
                return Result.Ok(None)

        match child.stdout():
            case Option.Some(stdout_pipe):
                line = try stdout_pipe.read_line(timeout=500ms)
                match line:
                    case Option.Some(text):
                        print(text)
                    case Option.None:
                        print("missing stdout text")
                        return Result.Ok(None)
            case Option.None:
                print("missing stdout")
                return Result.Ok(None)

        match child.wait(timeout=2s):
            case Wait.Exited(status):
                print(status)
            case Wait.TimedOut:
                print("timed out")
                return Result.Ok(None)
            case Wait.Cancelled:
                print("cancelled")
                return Result.Ok(None)
            case Wait.Failed(error):
                print(error)
                return Result.Ok(None)
    return Result.Ok(None)

def main() -> int32:
    match run_env():
        case Result.Ok(_):
            pass
        case Result.Err(error):
            print(error)
            return 1

    match run_pwd("{cwd}"):
        case Result.Ok(_):
            pass
        case Result.Err(error):
            print(error)
            return 1

    match echo_with_cat():
        case Result.Ok(_):
            return 0
        case Result.Err(error):
            print(error)
            return 1
"#,
        cwd = cwd,
    );

    let output = run_path_with_source(&entry, &source).expect("builtin process module should run");
    assert_eq!(
        output.stdout,
        format!(
            "present\n0\nExitStatus.Exited(0)\n{cwd}\n0\nExitStatus.Exited(0)\necho from cat\nExitStatus.Exited(0)\n",
            cwd = cwd,
        )
    );
}
