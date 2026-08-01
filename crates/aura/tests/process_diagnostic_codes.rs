use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

fn aura_bin() -> &'static str {
    env!("CARGO_BIN_EXE_aura")
}

struct TempDir {
    path: PathBuf,
}

impl TempDir {
    fn new(prefix: &str) -> Self {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system clock should follow the Unix epoch")
            .as_nanos();
        let path = std::env::temp_dir().join(format!("{prefix}-{}-{unique}", std::process::id()));
        fs::create_dir_all(&path).expect("temporary test directory should exist");
        Self { path }
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.path);
    }
}

fn generated_binary(path: &Path) -> Command {
    let mut command = Command::new(path);
    if std::env::var_os("LLVM_PROFILE_FILE").is_some() {
        command.env("LLVM_PROFILE_FILE", "/dev/null");
    }
    command
}

fn assert_primary_utf8_diagnostic(output: &std::process::Output, backend: &str, method: &str) {
    assert!(
        !output.status.success(),
        "{backend} should reject invalid UTF-8 from process.Completed.{method}()"
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    let primary = stderr.lines().next().unwrap_or_default();
    assert_eq!(
        primary,
        "error[AU4005]: received non-UTF-8 data: invalid utf-8 sequence of 1 bytes from index 0",
        "unexpected {backend} primary diagnostic for process.Completed.{method}(); stderr was:\n{stderr}"
    );
}

#[cfg(unix)]
#[test]
fn process_completed_text_decoding_is_au4005_on_mir_and_direct_backends() {
    for (method, python) in [
        (
            "stdout",
            "import sys; sys.stdout.buffer.write(bytes([255]))",
        ),
        (
            "stderr",
            "import sys; sys.stderr.buffer.write(bytes([255]))",
        ),
    ] {
        let source = format!(
            "import process\n\ndef decode() -> Result[None, process.Error]:\n    completed = try process.run([\"/usr/bin/env\", \"python3\", \"-c\", \"{python}\"], stdout=process.pipe(), stderr=process.pipe(), timeout=2s, group=true)\n    text = completed.{method}()\n    print(text)\n    return Result.Ok(None)\n\ndef main() -> int32:\n    match decode():\n        case Result.Ok(_):\n            return 0\n        case Result.Err(error):\n            print(error)\n            return 1\n"
        );
        let temp = TempDir::new(&format!("aura-process-{method}-diagnostic"));
        let source_path = temp.path.join("main.au");
        fs::write(&source_path, source).expect("temporary Aura source should write");

        let mir = Command::new(aura_bin())
            .arg("run")
            .arg(&source_path)
            .output()
            .expect("aura run should start");
        assert_primary_utf8_diagnostic(&mir, "MIR", method);

        let output_path = temp.path.join("out");
        let build = Command::new(aura_bin())
            .args(["build", "--backend", "direct", "-o"])
            .arg(&output_path)
            .arg(&source_path)
            .output()
            .expect("direct build should start");
        assert!(
            build.status.success(),
            "direct build should succeed; stderr was:\n{}",
            String::from_utf8_lossy(&build.stderr)
        );
        let direct = generated_binary(&output_path)
            .output()
            .expect("direct binary should start");
        assert_primary_utf8_diagnostic(&direct, "direct", method);
    }
}
