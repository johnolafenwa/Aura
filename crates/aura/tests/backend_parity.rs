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
            .join("crates/aura-compiler/tests/fixtures")
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
            "-p",
            "aura-compiler",
            "--lib",
            "--message-format=json",
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
    let stdout = String::from_utf8_lossy(&rustc.stdout);
    let messages = stdout
        .lines()
        .filter_map(|line| serde_json::from_str::<serde_json::Value>(line).ok())
        .collect::<Vec<_>>();
    let native_link_args = messages
        .iter()
        .rev()
        .find(|message| message["reason"] == "compiler-message")
        .and_then(|message| message["message"]["message"].as_str())
        .and_then(|message| message.split_once("native-static-libs:"))
        .map(|(_, arguments)| arguments.split_whitespace().collect::<Vec<_>>())
        .expect("rustc should report native-static-libs");
    let runtime_archive = messages
        .iter()
        .rev()
        .find(|message| {
            message["reason"] == "compiler-artifact" && message["target"]["name"] == "aura_compiler"
        })
        .and_then(|message| message["filenames"].as_array())
        .and_then(|filenames| {
            filenames.iter().find_map(|filename| {
                let filename = filename.as_str()?;
                filename
                    .ends_with("libaura_compiler.a")
                    .then(|| PathBuf::from(filename))
            })
        })
        .expect("Cargo should report the emitted aura-compiler static archive");

    let prefix = temp.path.join("toolchain");
    let bin_dir = prefix.join("bin");
    let runtime_dir = prefix.join("lib/aura");
    fs::create_dir_all(&bin_dir).expect("packaged bin dir should exist");
    fs::create_dir_all(&runtime_dir).expect("packaged runtime dir should exist");
    let packaged = bin_dir.join("aura");
    fs::copy(aura_bin(), &packaged).expect("test aura should copy into package layout");
    fs::copy(runtime_archive, runtime_dir.join("libaura_compiler.a"))
        .expect("debug native runtime should copy into package layout");
    fs::write(
        runtime_dir.join("native-link-args.json"),
        serde_json::to_vec(&native_link_args).expect("link arguments should serialize"),
    )
    .expect("runtime link manifest should write");
    packaged
}

#[test]
fn packaged_parity_aura_uses_cargo_reported_runtime_archive() {
    let temp = TempDir::new("aura-packaged-parity-path");
    let packaged = packaged_test_aura(&temp);
    assert!(packaged.is_file());
    assert!(
        temp.path
            .join("toolchain/lib/aura/libaura_compiler.a")
            .is_file(),
        "the Cargo-reported runtime archive should be copied into the test toolchain"
    );
}

#[test]
fn equality_obligation_rejections_are_identical_across_forced_backends() {
    let root = repo_root();
    let fixtures = [
        "callable_equality_capturing_closure.au",
        "equality_callable_list_remove.au",
        "equality_callable_list_index.au",
        "equality_callable_list_count.au",
        "equality_callable_membership.au",
        "equality_callable_set_add.au",
        "equality_callable_dict_key.au",
        "equality_rng_list_remove.au",
        "equality_rng_list_index.au",
        "equality_rng_list_count.au",
        "equality_rng_membership.au",
        "equality_rng_set_add.au",
        "equality_rng_dict_key.au",
    ];

    for fixture_name in fixtures {
        let fixture = root
            .join("crates/aura-compiler/tests/fixtures/check-fail")
            .join(fixture_name);
        let mut diagnostics = Vec::new();

        for backend in ["mir", "direct"] {
            let output = Command::new(aura_bin())
                .current_dir(&root)
                .args(["run", "--backend", backend])
                .arg(&fixture)
                .output()
                .unwrap_or_else(|error| panic!("forced {backend} command should start: {error}"));
            assert!(
                !output.status.success(),
                "forced {backend} execution must reject {fixture_name}"
            );
            assert!(
                output.stdout.is_empty(),
                "forced {backend} rejection must not execute {fixture_name}"
            );
            let diagnostic = normalize(&output.stderr);
            assert!(
                diagnostic.starts_with("error[AU2008]:"),
                "forced {backend} diagnostic must name the equality obligation for {fixture_name}:\n{diagnostic}"
            );
            diagnostics.push(diagnostic);
        }

        assert_eq!(
            diagnostics[0], diagnostics[1],
            "{fixture_name} must be rejected before backend selection can change its diagnostic"
        );
    }
}

#[test]
#[ignore = "full forced-backend fixture matrix; invoked by npm run test:backend-parity"]
fn forced_mir_and_direct_backends_match_every_runtime_fixture() {
    // Phase 4 landed the `run` backend selector, so both sides of the matrix
    // are forced explicitly: `run --backend mir` against `build --backend
    // direct`. Neither side may fall back to `auto`.
    let root = repo_root();
    let temp = TempDir::new("aura-backend-parity");
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
            normalize(&mir.stderr),
            normalize(&expected),
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
            normalize(&direct.stderr),
            normalize(&expected),
            "forced MIR/direct diagnostic diverged for {}",
            relative.display()
        );
    }
}
