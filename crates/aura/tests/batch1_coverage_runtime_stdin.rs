//! Standard-input contracts for the MIR backend: `io.read_line()` consumes
//! piped lines, strips one trailing line ending, and reports EOF as `None`.

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

#[test]
fn io_read_line_consumes_piped_stdin_until_eof_on_the_mir_backend() {
    let temp = TempDir::new("aura-cov-stdin");
    let source = temp.write(
        "main.au",
        r#"import io

def main():
    mut lines = 0
    while true:
        match io.read_line():
            case Result.Ok(Option.Some(line)):
                lines += 1
                print("got " + line)
            case Result.Ok(Option.None):
                print("eof")
                break
            case Result.Err(error):
                print(error)
                break
    print(lines)
"#,
    );
    let mut child = Command::new(aura_bin())
        .args(["run", "--backend", "mir"])
        .arg(&source)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("failed to spawn aura run");
    child
        .stdin
        .take()
        .expect("stdin should be piped")
        .write_all(b"first line\r\nsecond\n")
        .expect("failed to write stdin");
    let output = child
        .wait_with_output()
        .expect("failed to collect aura run output");
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
