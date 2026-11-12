//! `aura build --freestanding` and `aura check --freestanding` (ADR-0094).

#![cfg(unix)]

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::atomic::{AtomicU64, Ordering};

static NEXT_DIRECTORY: AtomicU64 = AtomicU64::new(0);

fn aura_bin() -> &'static str {
    env!("CARGO_BIN_EXE_aura")
}

/// A directory of sources that is removed when the test ends.
struct Workspace {
    path: PathBuf,
}

impl Workspace {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "aura-freestanding-cli-{}-{}",
            std::process::id(),
            NEXT_DIRECTORY.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(&path).expect("the workspace should be made");
        Self { path }
    }

    fn write(&self, name: &str, text: &str) -> PathBuf {
        let path = self.path.join(name);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).expect("the directory should be made");
        }
        fs::write(&path, text).expect("the file should be written");
        path
    }

    fn aura(&self, args: &[&str]) -> Output {
        Command::new(aura_bin())
            .args(args)
            .current_dir(&self.path)
            .env("AURA_CACHE_DIR", self.path.join("cache"))
            .output()
            .expect("aura should start")
    }
}

impl Drop for Workspace {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.path);
    }
}

fn stderr(output: &Output) -> String {
    String::from_utf8_lossy(&output.stderr).into_owned()
}

fn stdout(output: &Output) -> String {
    String::from_utf8_lossy(&output.stdout).into_owned()
}

/// A program a freestanding build accepts, which also runs hosted: `main`
/// prints what the exported functions return.
const PROGRAM: &str = r#"
class Accumulator:
    total: int64
    count: int64

    def add(mut self, value: int64) -> None:
        self.total = self.total + value
        self.count = self.count + 1

    def mean(self) -> int64:
        requires self.count > 0
        return self.total // self.count

def collatz(start: uint64) -> uint32:
    mut n = start
    mut steps: uint32 = 0
    while n != 1:
        if n % 2 == 0:
            n = n // 2
        else:
            n = 3 * n + 1
        steps = steps + 1
    return steps

def find_root(limit: int32, square: int32) -> int32 | None:
    mut candidate: int32 = 0
    while candidate < limit:
        if candidate * candidate == square:
            return candidate
        candidate = candidate + 1
    return None

export "C" def checksum(seed: uint32, count: uint32) -> uint32:
    mut total: uint32 = seed
    mut i: uint32 = 0
    while i < count:
        total = (total * 31 + i) % 65521
        i = i + 1
    return total

export "C" def statistics(limit: int64, out: mut int64) -> int64:
    mut accumulator = Accumulator(total=0, count=0)
    for value in range(1, limit, 3):
        accumulator.add((collatz(value as uint64) as int64) - (value % 5))
    out = accumulator.total
    return accumulator.mean()

export "C" def roots(square: int32) -> int32:
    root = find_root(1000, square)
    if root is None:
        return -1
    return root

export "C" def blend(a: float64, b: float32, bits: uint8) -> float64:
    scaled = (a * 1.5 - (b as float64)) / 4.0
    shifted = ((bits << 1) | (bits >> 3)) & 127
    return scaled + (shifted as float64) + sqrt(a)

export "C" def quotient(a: int64, b: int64) -> int64:
    return a // b

export "C" def narrow(value: int64) -> int8:
    return value as int8

def main() -> None:
    print(checksum(7, 1000))
    mut total: int64 = 0
    print(statistics(200, total))
    print(total)
    print(roots(1764))
    print(roots(1765))
    print((blend(9.0, 2.5 as float32, 77) * 1000.0) as int64)
    print(quotient(-7, 2))
    print(narrow(-128))
"#;

const HARNESS: &str = r#"
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
uint32_t checksum(uint32_t seed, uint32_t count);
int64_t statistics(int64_t limit, int64_t *out);
int32_t roots(int32_t square);
double blend(double a, float b, uint8_t bits);
int64_t quotient(int64_t a, int64_t b);
int8_t narrow(int64_t value);
void aura_trap(uint32_t code, const char *file, uint64_t file_len, uint32_t line, uint32_t column) {
    printf("trap %u at %.*s:%u\n", code, (int)file_len, file, line);
    exit(0);
}
int main(int argc, char **argv) {
    if (argc > 1) {
        switch (argv[1][0]) {
        case 'd': quotient(1, 0); break;
        case 'o': quotient(INT64_MIN, -1); break;
        case 'c': narrow(128); break;
        case 'r': statistics(1, &(int64_t){0}); break;
        }
        printf("no trap\n");
        return 1;
    }
    printf("%u\n", checksum(7, 1000));
    int64_t total = 0;
    printf("%lld\n", (long long)statistics(200, &total));
    printf("%lld\n", (long long)total);
    printf("%d\n", roots(1764));
    printf("%d\n", roots(1765));
    printf("%lld\n", (long long)(blend(9.0, 2.5f, 77) * 1000.0));
    printf("%lld\n", (long long)quotient(-7, 2));
    printf("%d\n", narrow(-128));
    return 0;
}
"#;

#[test]
fn each_target_builds_one_relocatable_object() {
    let workspace = Workspace::new();
    workspace.write("kernel.au", PROGRAM);
    for (target, machine) in [
        ("x86_64-unknown-none", 62_u16),
        ("aarch64-unknown-none", 183),
        ("riscv64gc-unknown-none-elf", 243),
    ] {
        let output = workspace.aura(&[
            "build",
            "--freestanding",
            "--target",
            target,
            "-o",
            "kernel.o",
            "kernel.au",
        ]);
        assert!(output.status.success(), "{target}: {}", stderr(&output));
        assert!(
            stderr(&output).contains(&format!(
                "aura: built `kernel.o` for `{target}` (freestanding object, "
            )),
            "{}",
            stderr(&output)
        );
        let object = fs::read(workspace.path.join("kernel.o")).expect("the object exists");
        // An ELF relocatable file for the target's machine.
        assert_eq!(&object[..4], b"\x7fELF", "{target}");
        assert_eq!(u16::from_le_bytes([object[16], object[17]]), 1, "{target}");
        assert_eq!(
            u16::from_le_bytes([object[18], object[19]]),
            machine,
            "{target}"
        );
    }
}

#[test]
fn the_default_target_is_the_build_machine() {
    let workspace = Workspace::new();
    workspace.write("kernel.au", PROGRAM);
    let output = workspace.aura(&["build", "--freestanding", "-o", "kernel.o", "kernel.au"]);
    assert!(output.status.success(), "{}", stderr(&output));
    let expected = match std::env::consts::ARCH {
        "x86_64" => "x86_64-unknown-none",
        "aarch64" => "aarch64-unknown-none",
        other => panic!("no freestanding target for `{other}`"),
    };
    assert!(stderr(&output).contains(expected), "{}", stderr(&output));
}

#[test]
fn check_reports_every_construct_a_freestanding_build_cannot_compile() {
    let workspace = Workspace::new();
    workspace.write("kernel.au", PROGRAM);
    let output = workspace.aura(&["check", "--freestanding", "kernel.au"]);
    assert!(output.status.success(), "{}", stderr(&output));
    assert_eq!(stdout(&output), "ok\n");

    workspace.write(
        "hosted.au",
        "export \"C\" def entry(count: int64) -> int64:\n    text = \"hello\"\n    print(text)\n    return count\n",
    );
    let output = workspace.aura(&["check", "--freestanding", "hosted.au"]);
    assert_eq!(output.status.code(), Some(1));
    let text = stderr(&output);
    assert!(
        text.contains("error[AU2052]: `entry` holds `text` of type `str`"),
        "{text}"
    );
    assert!(
        text.contains("error[AU2052]: `entry` calls `print`"),
        "{text}"
    );
    assert!(text.contains("hosted.au:3:5"), "{text}");
    // The same program passes an ordinary check, and no object is written.
    assert!(workspace.aura(&["check", "hosted.au"]).status.success());
    let output = workspace.aura(&["build", "--freestanding", "-o", "hosted.o", "hosted.au"]);
    assert_eq!(output.status.code(), Some(1));
    assert!(!workspace.path.join("hosted.o").exists());

    let output = workspace.aura(&["check", "--freestanding", "--format", "json", "hosted.au"]);
    assert_eq!(output.status.code(), Some(1));
    let report: serde_json::Value =
        serde_json::from_str(stderr(&output).trim()).expect("the report is JSON");
    let diagnostics = report["diagnostics"].as_array().expect("a list");
    assert!(diagnostics.len() >= 2, "{report}");
    assert!(diagnostics
        .iter()
        .all(|diagnostic| diagnostic["code"] == "AU2052"));

    // A program an ordinary check rejects reports that error.
    workspace.write(
        "broken.au",
        "export \"C\" def entry() -> int64:\n    return missing\n",
    );
    let output = workspace.aura(&["check", "--freestanding", "broken.au"]);
    assert_eq!(output.status.code(), Some(1));
    assert!(stderr(&output).contains("AU2001"), "{}", stderr(&output));
}

#[test]
fn a_problem_in_an_imported_module_shows_that_file() {
    let workspace = Workspace::new();
    workspace.write(
        "Aura.toml",
        "[package]\nname = \"kernel\"\nversion = \"0.1.0\"\nedition = \"2026\"\n",
    );
    workspace.write(
        "src/uart.au",
        "public def banner(code: int64) -> int64:\n    text = f\"code {code}\"\n    return len(text)\n\npublic export \"C\" def uart_irq(status: uint32) -> uint32:\n    return status & 15\n",
    );
    let main = "import uart\n\nexport \"C\" def kernel_main(code: int64) -> int64:\n    return uart.banner(code)\n";
    workspace.write("src/main.au", main);
    let output = workspace.aura(&["check", "--freestanding", "src/main.au"]);
    assert_eq!(output.status.code(), Some(1));
    let text = stderr(&output);
    assert!(text.contains("`uart::banner` builds text"), "{text}");
    assert!(text.contains("uart.au:2:"), "{text}");
    assert!(text.contains("text = f\"code {code}\""), "{text}");

    // Without the call, the module's own export is still defined.
    workspace.write(
        "src/main.au",
        "import uart\n\nexport \"C\" def kernel_main(code: int64) -> int64:\n    return code + 1\n",
    );
    let output = workspace.aura(&["build", "--freestanding", "-o", "k.o", "src/main.au"]);
    assert!(output.status.success(), "{}", stderr(&output));
    let object = fs::read(workspace.path.join("k.o")).expect("the object exists");
    for symbol in [&b"kernel_main\0"[..], &b"uart_irq\0"[..]] {
        assert!(
            object.windows(symbol.len()).any(|window| window == symbol),
            "the object names {}",
            String::from_utf8_lossy(symbol)
        );
    }
}

#[test]
fn bad_arguments_are_usage_errors() {
    let workspace = Workspace::new();
    workspace.write("kernel.au", PROGRAM);
    let output = workspace.aura(&[
        "build",
        "--freestanding",
        "--target",
        "wasm32-unknown-unknown",
        "-o",
        "kernel.o",
        "kernel.au",
    ]);
    assert_eq!(output.status.code(), Some(2));
    let text = stderr(&output);
    assert!(
        text.contains("unknown freestanding target `wasm32-unknown-unknown`"),
        "{text}"
    );
    assert!(text.contains("`riscv64gc-unknown-none-elf`"), "{text}");

    let output = workspace.aura(&[
        "build",
        "--freestanding",
        "--debug",
        "-o",
        "kernel.o",
        "kernel.au",
    ]);
    assert_eq!(output.status.code(), Some(2));
    assert!(stderr(&output).contains("`--debug` does not apply"));

    let output = workspace.aura(&["build", "--freestanding", "--target"]);
    assert_eq!(output.status.code(), Some(2));

    workspace.write("script.au", "def main() -> None:\n    print(1)\n");
    let output = workspace.aura(&["build", "--freestanding", "-o", "s.o", "script.au"]);
    assert_eq!(output.status.code(), Some(1));
    assert!(stderr(&output).contains("needs at least one `export \"C\" def`"));

    let output = workspace.aura(&[
        "build",
        "--freestanding",
        "-o",
        "missing/kernel.o",
        "kernel.au",
    ]);
    assert_eq!(output.status.code(), Some(1));
    assert!(
        stderr(&output).contains("failed to write"),
        "{}",
        stderr(&output)
    );
}

#[test]
fn a_program_from_stdin_builds() {
    let workspace = Workspace::new();
    let mut child = Command::new(aura_bin())
        .args([
            "build",
            "--freestanding",
            "-o",
            "k.o",
            "--stdin",
            "kernel.au",
        ])
        .current_dir(&workspace.path)
        .stdin(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .expect("aura should start");
    {
        use std::io::Write;
        child
            .stdin
            .take()
            .expect("stdin is piped")
            .write_all(PROGRAM.as_bytes())
            .expect("the source should be written");
    }
    let output = child.wait_with_output().expect("aura should finish");
    assert!(output.status.success(), "{}", stderr(&output));
    assert!(workspace.path.join("k.o").exists());

    let mut child = Command::new(aura_bin())
        .args(["check", "--freestanding", "--stdin", "kernel.au"])
        .current_dir(&workspace.path)
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .spawn()
        .expect("aura should start");
    {
        use std::io::Write;
        child
            .stdin
            .take()
            .expect("stdin is piped")
            .write_all(PROGRAM.as_bytes())
            .expect("the source should be written");
    }
    let output = child.wait_with_output().expect("aura should finish");
    assert_eq!(stdout(&output), "ok\n");
}

/// The C compiler that links the host's ELF objects, on Linux.
fn c_compiler() -> Option<&'static str> {
    if !cfg!(target_os = "linux") {
        return None;
    }
    ["cc", "clang", "gcc"].into_iter().find(|compiler| {
        Command::new(compiler)
            .arg("--version")
            .output()
            .is_ok_and(|output| output.status.success())
    })
}

fn link_and_run(workspace: &Workspace, compiler: &str, object: &str, args: &[&str]) -> String {
    let harness = workspace.write("harness.c", HARNESS);
    let binary = workspace.path.join(format!("{object}.bin"));
    let link = Command::new(compiler)
        .arg(&harness)
        .arg(workspace.path.join(object))
        .args(["-lm", "-o"])
        .arg(&binary)
        .output()
        .expect("the C compiler should start");
    assert!(link.status.success(), "{}", stderr(&link));
    let run = Command::new(Path::new(&binary))
        .args(args)
        .output()
        .expect("the linked program should start");
    assert!(run.status.success(), "{}", stderr(&run));
    stdout(&run)
}

#[test]
fn a_freestanding_object_computes_what_the_hosted_program_prints() {
    let Some(compiler) = c_compiler() else {
        return;
    };
    let workspace = Workspace::new();
    workspace.write("kernel.au", PROGRAM);
    let hosted = workspace.aura(&["run", "kernel.au"]);
    assert!(hosted.status.success(), "{}", stderr(&hosted));
    let build = workspace.aura(&["build", "--freestanding", "-o", "kernel.o", "kernel.au"]);
    assert!(build.status.success(), "{}", stderr(&build));
    assert_eq!(
        link_and_run(&workspace, compiler, "kernel.o", &[]),
        stdout(&hosted)
    );
}

#[test]
fn a_failed_check_calls_the_trap_hook_with_its_code_and_position() {
    let Some(compiler) = c_compiler() else {
        return;
    };
    let workspace = Workspace::new();
    workspace.write("kernel.au", PROGRAM);
    let build = workspace.aura(&["build", "--freestanding", "-o", "kernel.o", "kernel.au"]);
    assert!(build.status.success(), "{}", stderr(&build));
    let line_of = |text: &str| {
        PROGRAM
            .lines()
            .position(|line| line.contains(text))
            .expect("the program holds the line")
            + 1
    };
    for (argument, code, line) in [
        ("d", 2, line_of("return a // b")),
        ("o", 1, line_of("return a // b")),
        ("c", 5, line_of("return value as int8")),
        ("r", 4, line_of("requires self.count > 0")),
    ] {
        assert_eq!(
            link_and_run(&workspace, compiler, "kernel.o", &[argument]),
            format!("trap {code} at kernel.au:{line}\n"),
            "{argument}"
        );
    }
}

#[test]
fn proved_checks_are_removed_from_a_freestanding_object() {
    let solver = Command::new("z3").arg("--version").output();
    if !solver.is_ok_and(|output| output.status.success()) {
        return;
    }
    let workspace = Workspace::new();
    workspace.write(
        "kernel.au",
        "export \"C\" def halve(value: uint8) -> uint8:\n    return value // 2 + 1\n",
    );
    let output = workspace.aura(&[
        "build",
        "--freestanding",
        "--remove-proved-checks",
        "-o",
        "kernel.o",
        "kernel.au",
    ]);
    assert!(output.status.success(), "{}", stderr(&output));
    assert!(
        stderr(&output).contains("checks proved and removed"),
        "{}",
        stderr(&output)
    );
    let object = fs::read(workspace.path.join("kernel.o")).expect("the object exists");
    // With every check proved, nothing calls the trap hook.
    assert!(!object
        .windows(b"aura_trap".len())
        .any(|window| window == b"aura_trap"));
}
