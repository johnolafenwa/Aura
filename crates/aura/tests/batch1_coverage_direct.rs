//! Whole-program direct-backend (Cranelift) execution for the Batch 1
//! coverage programs: each source is run on the MIR interpreter and built and
//! executed through `aura build --backend direct`, and both must agree.

use std::fs;
use std::path::PathBuf;
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

fn aura_bin() -> &'static str {
    env!("CARGO_BIN_EXE_aura")
}

fn generated_binary(path: &PathBuf) -> Command {
    let mut command = Command::new(path);
    if std::env::var_os("LLVM_PROFILE_FILE").is_some() {
        #[cfg(unix)]
        command.env("LLVM_PROFILE_FILE", "/dev/null");
        #[cfg(windows)]
        command.env("LLVM_PROFILE_FILE", "NUL");
    }
    command
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

    fn path(&self) -> &PathBuf {
        &self.path
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.path);
    }
}

fn write_temp_source(prefix: &str, source: &str) -> (TempDir, PathBuf) {
    let temp = TempDir::new(prefix);
    let source_path = temp.path().join("main.au");
    fs::write(&source_path, source).expect("failed to write temporary Aura source");
    (temp, source_path)
}

fn build_and_run_direct_source(
    prefix: &str,
    source: &str,
) -> (std::process::Output, std::process::Output) {
    let (temp, source_path) = write_temp_source(prefix, source);
    let output_path = temp.path().join("out");

    let build = Command::new(aura_bin())
        .arg("build")
        .arg("--backend")
        .arg("direct")
        .arg("-o")
        .arg(&output_path)
        .arg(&source_path)
        .output()
        .expect("failed to run aura build --backend direct");

    assert!(
        build.status.success(),
        "direct backend build should succeed, stderr was:\n{}",
        String::from_utf8_lossy(&build.stderr)
    );

    let run = generated_binary(&output_path)
        .output()
        .expect("failed to run direct-backend binary");

    (build, run)
}

/// Runs `source` on the MIR interpreter and through the direct backend and
/// requires identical successful stdout.
fn assert_backends_agree(prefix: &str, source: &str, expected_stdout: &str) {
    let (_temp, source_path) = write_temp_source(prefix, source);
    let interpreted = Command::new(aura_bin())
        .arg("run")
        .arg("--backend")
        .arg("mir")
        .arg(&source_path)
        .output()
        .expect("failed to run aura run --backend mir");
    assert!(
        interpreted.status.success(),
        "interpreter run should succeed, stderr was:\n{}",
        String::from_utf8_lossy(&interpreted.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&interpreted.stdout),
        expected_stdout
    );

    let (_build, run) = build_and_run_direct_source(prefix, source);
    assert!(
        run.status.success(),
        "direct binary should succeed, stderr was:\n{}",
        String::from_utf8_lossy(&run.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&run.stdout), expected_stdout);
}

#[test]
fn direct_backend_dispatches_trait_methods_on_union_receivers() {
    assert_backends_agree(
        "aura-cov-union-dispatch",
        "trait Speak:\n    def speak(self) -> str\nclass Dog:\n    name: str\nclass Cat:\n    name: str\nimpl Speak for Dog:\n    def speak(self) -> str:\n        return \"woof\"\nimpl Speak for Cat:\n    def speak(self) -> str:\n        return \"meow\"\ndef talk(pet: Dog | Cat) -> str:\n    return pet.speak()\ndef main():\n    print(talk(Dog(name=\"d\")))\n    print(talk(Cat(name=\"c\")))\n",
        "woof\nmeow\n",
    );
}

#[test]
fn direct_backend_writes_mutable_resource_projections_back_before_cleanup() {
    assert_backends_agree(
        "aura-cov-resource-sink",
        "class Handle:\n    name: str\n    items: list[int64]\n    def close(mut self):\n        print(\"closed \" + self.name)\ndef push(items: mut list[int64]):\n    items.append(1)\ndef main():\n    with handle = Handle(name=\"h\", items=[]):\n        push(handle.items)\n        push(handle.items)\n        print(handle.items.len())\n",
        "2\nclosed h\n",
    );
}

#[test]
fn direct_backend_runs_array_operators_with_scalar_operands() {
    assert_backends_agree(
        "aura-cov-array-scalar",
        "def main():\n    values = Array[int64].from_list([1, 2, 3], [3])\n    doubled = values + values\n    scaled = 2 * values\n    print(doubled.sum())\n    print(scaled.sum())\n    print(scaled.len())\n",
        "12\n12\n3\n",
    );
}

#[test]
fn direct_backend_runs_closures_tuples_durations_and_unit_variants() {
    assert_backends_agree(
        "aura-cov-temporaries",
        "enum Mode:\n    Fast\n    Slow\ndef main():\n    mode = Mode.Fast\n    flag: int64 | None = 3\n    factor = 2\n    scale: def(int64) -> int64 = lambda value: value * factor\n    plain: def(int64) -> int64 = lambda value: value + 1\n    print(scale(2))\n    print(plain(2))\n    pair = (1, \"a\")\n    print(pair[0])\n    wait = Duration.seconds(1) + Duration.seconds(2)\n    print(wait.to_ms())\n    match flag:\n        case int64 as number:\n            print(number)\n        case None:\n            print(0)\n    match mode:\n        case Mode.Fast:\n            print(\"fast\")\n        case Mode.Slow:\n            print(\"slow\")\n",
        "4\n3\n1\n3000.0\n3\nfast\n",
    );
}
