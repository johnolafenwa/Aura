#![cfg(unix)]

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

static NEXT_TEMP_PACKAGE_ID: AtomicU64 = AtomicU64::new(0);

fn aura_bin() -> &'static str {
    env!("CARGO_BIN_EXE_aura")
}

struct TempPackage {
    path: PathBuf,
}

impl TempPackage {
    fn new(allow_ffi: bool) -> Self {
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system clock should follow the Unix epoch")
            .as_nanos();
        Self::new_at_timestamp(allow_ffi, timestamp)
    }

    fn new_at_timestamp(allow_ffi: bool, timestamp: u128) -> Self {
        let path = loop {
            let sequence = NEXT_TEMP_PACKAGE_ID.fetch_add(1, Ordering::Relaxed);
            let unique = format!(
                "aura-ffi-acceptance-{}-{timestamp}-{sequence}",
                std::process::id()
            );
            let candidate = std::env::temp_dir().join(unique);
            match fs::create_dir(&candidate) {
                Ok(()) => break candidate,
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
                Err(error) => panic!("FFI acceptance package directory should exist: {error}"),
            }
        };
        fs::create_dir(path.join("src"))
            .expect("FFI acceptance package source directory should exist");
        let opt_in = if allow_ffi { "allow_ffi = true\n" } else { "" };
        fs::write(
            path.join("Aura.toml"),
            format!(
                "[package]\nname = \"ffi_acceptance\"\nversion = \"0.1.0\"\nedition = \"2026\"\n{opt_in}"
            ),
        )
        .expect("FFI acceptance manifest should be writable");
        Self { path }
    }

    fn source(&self, source: &str) -> PathBuf {
        let path = self.path.join("src/main.au");
        fs::write(&path, source).expect("FFI acceptance source should be writable");
        path
    }
}

#[test]
fn same_clock_tick_temp_packages_remain_isolated() {
    let first = TempPackage::new_at_timestamp(false, 1);
    let second = TempPackage::new_at_timestamp(false, 1);

    assert_ne!(first.path, second.path);
    let first_source = first.source("def main():\n    print(1)\n");
    let second_source = second.source("def test_only():\n    pass\n");
    assert_eq!(
        fs::read_to_string(first_source).expect("first source should remain readable"),
        "def main():\n    print(1)\n"
    );
    assert_eq!(
        fs::read_to_string(second_source).expect("second source should remain readable"),
        "def test_only():\n    pass\n"
    );
}

impl Drop for TempPackage {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.path);
    }
}

fn assert_success(output: &Output, context: &str) {
    assert!(
        output.status.success(),
        "{context} should succeed; stdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

fn run_mir(source: &Path) -> Output {
    Command::new(aura_bin())
        .arg("run")
        .arg("--backend")
        .arg("mir")
        .arg(source)
        .output()
        .expect("forced MIR FFI acceptance run should start")
}

fn build_direct(source: &Path, binary: &Path) -> Output {
    Command::new(aura_bin())
        .arg("build")
        .arg("--backend")
        .arg("direct")
        .arg("-o")
        .arg(binary)
        .arg(source)
        .output()
        .expect("direct FFI acceptance build should start")
}

fn repository_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .expect("repository root should be discoverable from the aura crate")
}

#[test]
fn getpid_binding_runs_with_backend_parity() {
    let package = TempPackage::new(true);
    let source = package.source(
        r#"public extern "C" def getpid() -> int32

def main() -> int32:
    print(getpid() > 0)
    return 0
"#,
    );

    let mir = run_mir(&source);
    assert_success(&mir, "forced MIR FFI acceptance run");
    assert_eq!(mir.stdout, b"true\n");

    let binary = package.path.join("ffi-acceptance-direct");
    let build = build_direct(&source, &binary);
    assert_success(&build, "direct FFI acceptance build");
    let direct = Command::new(binary)
        .output()
        .expect("direct FFI acceptance binary should start");
    assert_success(&direct, "direct FFI acceptance run");
    assert_eq!(direct.stdout, b"true\n");
    assert_eq!(direct.stdout, mir.stdout);
}

#[test]
fn maintained_getpid_example_runs_with_backend_parity() {
    let package = repository_root().join("examples/packages/ffi_getpid");
    let source = package.join("src/main.au");
    assert!(source.is_file(), "maintained FFI example source is missing");
    assert!(
        package.join("Aura.toml").is_file(),
        "maintained FFI example manifest is missing"
    );

    let mir = run_mir(&source);
    assert_success(&mir, "maintained FFI example MIR run");
    assert_eq!(mir.stdout, b"true\n");

    let output_dir = TempPackage::new(false);
    let binary = output_dir.path.join("ffi-getpid-example-direct");
    let build = build_direct(&source, &binary);
    assert_success(&build, "maintained FFI example direct build");
    let direct = Command::new(binary)
        .output()
        .expect("maintained FFI example direct binary should start");
    assert_success(&direct, "maintained FFI example direct run");
    assert_eq!(direct.stdout, b"true\n");
    assert_eq!(direct.stdout, mir.stdout);
}

#[test]
fn manifest_authorized_ffi_test_function_runs_through_the_trusted_path_api() {
    let package = TempPackage::new(true);
    let source = package.source(
        r#"public extern "C" def getpid() -> int32

def test_getpid():
    assert getpid() > 0
"#,
    );

    let output = Command::new(aura_bin())
        .arg("test")
        .arg(&source)
        .output()
        .expect("manifest-authorized FFI test should start");
    assert_success(&output, "manifest-authorized FFI test");
    assert!(
        String::from_utf8_lossy(&output.stdout).contains("1 passed; 0 failed"),
        "{}",
        String::from_utf8_lossy(&output.stdout)
    );
}

#[test]
fn package_ffi_requires_explicit_manifest_opt_in() {
    let package = TempPackage::new(false);
    let source = package.source(
        r#"public extern "C" def getpid() -> int32

def main() -> int32:
    return getpid()
"#,
    );

    let output = Command::new(aura_bin())
        .arg("check")
        .arg(&source)
        .output()
        .expect("FFI manifest rejection check should start");
    assert!(
        !output.status.success(),
        "FFI without opt-in must be rejected"
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("AU2999"), "{stderr}");
    assert!(stderr.contains("allow_ffi = true"), "{stderr}");
}
