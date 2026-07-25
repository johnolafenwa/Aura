use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

fn aura_bin() -> &'static str {
    env!("CARGO_BIN_EXE_aura")
}

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("aura crate should live under repo root")
        .to_path_buf()
}

struct TempDir {
    path: PathBuf,
}

impl TempDir {
    fn new(prefix: &str) -> Self {
        let unique = format!(
            "{prefix}-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .expect("system clock should follow unix epoch")
                .as_nanos()
        );
        let path = std::env::temp_dir().join(unique);
        fs::create_dir_all(&path).expect("parity temp directory should exist");
        Self { path }
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.path);
    }
}

fn fixture_files(category: &str) -> Vec<PathBuf> {
    let mut fixtures = fs::read_dir(
        repo_root()
            .join("crates/aurora-compiler/tests/fixtures")
            .join(category),
    )
    .unwrap_or_else(|error| panic!("failed to read {category} fixtures: {error}"))
    .filter_map(|entry| entry.ok().map(|entry| entry.path()))
    .filter(|path| path.extension().and_then(|extension| extension.to_str()) == Some("au"))
    .collect::<Vec<_>>();
    fixtures.sort();
    fixtures
}

fn normalize(text: &[u8]) -> String {
    String::from_utf8_lossy(text)
        .replace("\r\n", "\n")
        .trim_end()
        .to_string()
}

fn normalize_primary_runtime_diagnostic(text: &[u8]) -> String {
    // Batch 1 permits MIR-only Aurora frames until native frame capture lands
    // in Batch 3. Keep this exception limited to the three supplemental note
    // families; the primary diagnostic and every other note remain parity-gated.
    normalize(text)
        .lines()
        .filter(|line| !is_supplemental_mir_backtrace_note(line))
        .collect::<Vec<_>>()
        .join("\n")
}

fn is_supplemental_mir_backtrace_note(line: &str) -> bool {
    let line = line.trim_start();
    let line = line.strip_prefix("= ").unwrap_or(line);
    line.starts_with("note: Aurora call chain")
        || line.starts_with("note: Aurora task entry")
        || line.starts_with("note: Aurora task ancestry")
}

#[test]
fn primary_runtime_diagnostic_normalization_ignores_only_deferred_mir_backtrace_notes() {
    let rendered = b"error[AU4004]: division by zero\n  = note: keep this semantic note\n  = note: Aurora call chain (innermost first): main at 1:1\n  = note: Aurora task entry: child at 2:1\n  = note: Aurora task ancestry (youngest first): child spawned from main at 6:15\n";

    assert_eq!(
        normalize_primary_runtime_diagnostic(rendered),
        "error[AU4004]: division by zero\n  = note: keep this semantic note"
    );
}

fn command_output_with_timeout(mut command: Command, timeout: Duration) -> std::process::Output {
    command.stdout(Stdio::piped()).stderr(Stdio::piped());
    let mut child = command.spawn().expect("parity command should start");
    let started = std::time::Instant::now();
    loop {
        if child
            .try_wait()
            .expect("parity command should remain waitable")
            .is_some()
        {
            return child
                .wait_with_output()
                .expect("parity command output should collect");
        }
        if started.elapsed() >= timeout {
            let _ = child.kill();
            let output = child
                .wait_with_output()
                .expect("timed-out parity output should collect");
            panic!(
                "command timed out after {timeout:?}; stderr was:\n{}",
                String::from_utf8_lossy(&output.stderr)
            );
        }
        std::thread::sleep(Duration::from_millis(10));
    }
}

fn packaged_test_aura(temp: &TempDir) -> PathBuf {
    let root = repo_root();
    let rustc = Command::new("cargo")
        .current_dir(&root)
        .args([
            "rustc",
            "-q",
            "-p",
            "aurora-compiler",
            "--lib",
            "--",
            "--print",
            "native-static-libs",
        ])
        .output()
        .expect("cargo should report runtime link arguments");
    assert!(
        rustc.status.success(),
        "runtime static library should build, stderr was:\n{}",
        String::from_utf8_lossy(&rustc.stderr)
    );
    let stderr = String::from_utf8_lossy(&rustc.stderr);
    let native_link_args = stderr
        .lines()
        .rev()
        .find_map(|line| line.split_once("native-static-libs:"))
        .map(|(_, arguments)| arguments.split_whitespace().collect::<Vec<_>>())
        .expect("rustc should report native-static-libs");

    let prefix = temp.path.join("toolchain");
    let bin_dir = prefix.join("bin");
    let runtime_dir = prefix.join("lib/aurora");
    fs::create_dir_all(&bin_dir).expect("packaged bin dir should exist");
    fs::create_dir_all(&runtime_dir).expect("packaged runtime dir should exist");
    let packaged = bin_dir.join("aura");
    fs::copy(aura_bin(), &packaged).expect("test aura should copy into package layout");
    fs::copy(
        root.join("target/debug/libaurora_compiler.a"),
        runtime_dir.join("libaurora_compiler.a"),
    )
    .expect("debug native runtime should copy into package layout");
    fs::write(
        runtime_dir.join("native-link-args.json"),
        serde_json::to_vec(&native_link_args).expect("link arguments should serialize"),
    )
    .expect("runtime link manifest should write");
    packaged
}

#[test]
#[ignore = "full forced-backend fixture matrix; invoked by npm run test:backend-parity"]
fn forced_mir_and_direct_backends_match_every_runtime_fixture() {
    // `aura run` is the forced MIR product path today. When Phase 4 gives
    // `run` a backend selector, keep this gate explicit with `--backend mir`.
    let root = repo_root();
    let temp = TempDir::new("aurora-backend-parity");
    let aura = packaged_test_aura(&temp);

    for (index, fixture) in fixture_files("run-pass").into_iter().enumerate() {
        let relative = fixture
            .strip_prefix(&root)
            .expect("fixture should live under repo root");
        let mut mir = Command::new(&aura);
        mir.current_dir(&root)
            .arg("run")
            .arg("--backend")
            .arg("mir")
            .arg(relative);
        let mir = command_output_with_timeout(mir, Duration::from_secs(10));
        assert!(
            mir.status.success(),
            "forced MIR run failed for {}:\n{}",
            relative.display(),
            String::from_utf8_lossy(&mir.stderr)
        );
        let expected = fs::read(fixture.with_extension("stdout"))
            .expect("run-pass fixture should have expected stdout");
        assert_eq!(
            normalize(&mir.stdout),
            normalize(&expected),
            "forced MIR stdout and fixture oracle diverged for {}",
            relative.display()
        );

        let output_path = temp.path.join(format!("run-pass-{index}"));
        let mut build = Command::new(&aura);
        build
            .current_dir(&root)
            .env("CARGO", temp.path.join("missing-cargo"))
            .args(["build", "--backend", "direct", "-o"])
            .arg(&output_path)
            .arg(relative);
        let build = command_output_with_timeout(build, Duration::from_secs(30));
        assert!(
            build.status.success(),
            "direct build failed for {}:\n{}",
            relative.display(),
            String::from_utf8_lossy(&build.stderr)
        );

        let mut direct = Command::new(&output_path);
        direct.current_dir(&root);
        let direct = command_output_with_timeout(direct, Duration::from_secs(10));
        assert!(
            direct.status.success(),
            "direct run failed for {}:\n{}",
            relative.display(),
            String::from_utf8_lossy(&direct.stderr)
        );
        assert_eq!(
            normalize(&direct.stdout),
            normalize(&expected),
            "forced MIR/direct stdout diverged for {}",
            relative.display()
        );
    }

    for (index, fixture) in fixture_files("run-fail").into_iter().enumerate() {
        let relative = fixture
            .strip_prefix(&root)
            .expect("fixture should live under repo root");
        let mut mir = Command::new(&aura);
        mir.current_dir(&root)
            .arg("run")
            .arg("--backend")
            .arg("mir")
            .arg(relative);
        let mir = command_output_with_timeout(mir, Duration::from_secs(10));
        assert!(
            !mir.status.success(),
            "forced MIR run unexpectedly succeeded for {}",
            relative.display()
        );
        let expected = fs::read(fixture.with_extension("diag"))
            .expect("run-fail fixture should have expected diagnostic");
        assert_eq!(
            normalize_primary_runtime_diagnostic(&mir.stderr),
            normalize_primary_runtime_diagnostic(&expected),
            "forced MIR diagnostic and fixture oracle diverged for {}",
            relative.display()
        );

        let output_path = temp.path.join(format!("run-fail-{index}"));
        let mut build = Command::new(&aura);
        build
            .current_dir(&root)
            .env("CARGO", temp.path.join("missing-cargo"))
            .args(["build", "--backend", "direct", "-o"])
            .arg(&output_path)
            .arg(relative);
        let build = command_output_with_timeout(build, Duration::from_secs(30));
        assert!(
            build.status.success(),
            "direct build failed for runtime-fail fixture {}:\n{}",
            relative.display(),
            String::from_utf8_lossy(&build.stderr)
        );

        let mut direct = Command::new(&output_path);
        direct.current_dir(&root);
        let direct = command_output_with_timeout(direct, Duration::from_secs(10));
        assert!(
            !direct.status.success(),
            "direct run unexpectedly succeeded for {}",
            relative.display()
        );
        assert_eq!(
            normalize_primary_runtime_diagnostic(&direct.stderr),
            normalize_primary_runtime_diagnostic(&expected),
            "forced MIR/direct diagnostic diverged for {}",
            relative.display()
        );
    }
}
