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
        "export \"C\" def entry(count: int64) -> int64:\n    wide: set[int64] = {1, 2}\n    print(count)\n    return count\n",
    );
    let output = workspace.aura(&["check", "--freestanding", "hosted.au"]);
    assert_eq!(output.status.code(), Some(1));
    let text = stderr(&output);
    assert!(
        text.contains("error[AU2052]: `entry` holds `wide` of type `set[int64]`"),
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
        "public def banner(code: int64) -> int64:\n    text = f\"code {code:>4}\"\n    return len(text)\n\npublic export \"C\" def uart_irq(status: uint32) -> uint32:\n    return status & 15\n",
    );
    let main = "import uart\n\nexport \"C\" def kernel_main(code: int64) -> int64:\n    return uart.banner(code)\n";
    workspace.write("src/main.au", main);
    let output = workspace.aura(&["check", "--freestanding", "src/main.au"]);
    assert_eq!(output.status.code(), Some(1));
    let text = stderr(&output);
    assert!(text.contains("`uart::banner` builds text"), "{text}");
    assert!(text.contains("uart.au:2:"), "{text}");
    assert!(text.contains("text = f\"code {code:>4}\""), "{text}");

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
fn tuples_and_enums_of_words_compute_what_the_hosted_program_prints() {
    let Some(compiler) = c_compiler() else {
        return;
    };
    for (source, expected) in [
        (include_str!("freestanding_values.au"), "15726\n"),
        (include_str!("freestanding_try.au"), "8544\n"),
    ] {
        values_match_the_hosted_run(compiler, source, expected);
    }
}

fn values_match_the_hosted_run(compiler: &str, source: &str, expected: &str) {
    let workspace = Workspace::new();
    workspace.write("values.au", source);
    let hosted = workspace.aura(&["run", "values.au"]);
    assert!(hosted.status.success(), "{}", stderr(&hosted));
    assert_eq!(stdout(&hosted), expected);
    let build = workspace.aura(&["build", "--freestanding", "-o", "values.o", "values.au"]);
    assert!(build.status.success(), "{}", stderr(&build));
    let harness = workspace.write(
        "values.c",
        r#"
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
int64_t total(int64_t n);
void aura_trap(uint32_t code, const char *file, uint64_t file_len, uint32_t line, uint32_t column) {
    printf("trap %u at %.*s:%u:%u\n", code, (int)file_len, file, line, column);
    exit(1);
}
int main(void) {
    printf("%lld\n", (long long)total(40));
    return 0;
}
"#,
    );
    let binary = workspace.path.join("values.bin");
    let link = Command::new(compiler)
        .arg(&harness)
        .arg(workspace.path.join("values.o"))
        .arg("-o")
        .arg(&binary)
        .output()
        .expect("the C compiler should start");
    assert!(link.status.success(), "{}", stderr(&link));
    let run = Command::new(&binary)
        .output()
        .expect("the linked program should start");
    assert!(run.status.success(), "{}", stdout(&run));
    assert_eq!(stdout(&run), stdout(&hosted));
}

/// A type inside itself lives in counted blocks from the allocation hooks:
/// the program computes what the hosted one prints and frees every block.
#[test]
fn values_that_hold_themselves_free_every_block_they_allocate() {
    let Some(compiler) = c_compiler() else {
        return;
    };
    blocks_are_all_freed(
        compiler,
        include_str!("freestanding_boxes.au"),
        "total",
        "16043307\n",
    );
}

/// Text lives in counted blocks too: literals, values in classes and
/// enums, and the operations of the text library.
#[test]
fn text_computes_what_the_hosted_program_prints_and_frees_every_block() {
    let Some(compiler) = c_compiler() else {
        return;
    };
    blocks_are_all_freed(
        compiler,
        include_str!("freestanding_text.au"),
        "entry",
        "812162342\n",
    );
}

/// A list of numbers lives in a counted header and a buffer that grows.
#[test]
fn lists_of_numbers_compute_what_the_hosted_program_prints_and_free_every_block() {
    let Some(compiler) = c_compiler() else {
        return;
    };
    blocks_are_all_freed(
        compiler,
        include_str!("freestanding_list.au"),
        "entry",
        "1028420\n",
    );
}

/// A list of text, classes, enums, or lists releases what its elements
/// own when the list goes.
#[test]
fn lists_of_values_compute_what_the_hosted_program_prints_and_free_every_block() {
    let Some(compiler) = c_compiler() else {
        return;
    };
    blocks_are_all_freed(
        compiler,
        include_str!("freestanding_list_values.au"),
        "entry",
        "48407\n",
    );
}

/// A union of values that own heap memory, such as `str | None`, counts
/// its references like an enum.
#[test]
fn unions_of_heap_values_compute_what_the_hosted_program_prints_and_free_every_block() {
    let Some(compiler) = c_compiler() else {
        return;
    };
    blocks_are_all_freed(
        compiler,
        include_str!("freestanding_unions.au"),
        "entry",
        "43204124\n",
    );
}

/// `pop`, `insert`, `swap`, `reverse`, `reserve`, and `clear` on lists
/// of numbers, text, and classes.
#[test]
fn list_methods_compute_what_the_hosted_program_prints_and_free_every_block() {
    let Some(compiler) = c_compiler() else {
        return;
    };
    blocks_are_all_freed(
        compiler,
        include_str!("freestanding_list_methods.au"),
        "entry",
        "612374928\n",
    );
}

/// A dictionary keeps its entries in a hash table written in Aura and
/// releases what its keys and its values own.
#[test]
fn dictionaries_compute_what_the_hosted_program_prints_and_free_every_block() {
    let Some(compiler) = c_compiler() else {
        return;
    };
    blocks_are_all_freed(
        compiler,
        include_str!("freestanding_dict.au"),
        "entry",
        "41207485\n",
    );
}

/// An index outside a list stops the program through the trap hook.
#[test]
fn a_list_index_out_of_range_calls_the_trap_hook() {
    let Some(compiler) = c_compiler() else {
        return;
    };
    let workspace = Workspace::new();
    workspace.write(
        "range.au",
        "export \"C\" def entry(n: int64) -> int64:\n    items = [1, 2, 3]\n    return items[n]\n",
    );
    let build = workspace.aura(&["build", "--freestanding", "-o", "range.o", "range.au"]);
    assert!(build.status.success(), "{}", stderr(&build));
    let harness = workspace.write(
        "range.c",
        r#"
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
int64_t entry(int64_t n);
void *aura_alloc(uint64_t size, uint64_t align) { (void)align; return malloc(size); }
void aura_free(void *block, uint64_t size, uint64_t align) { (void)size; (void)align; free(block); }
void *aura_realloc(void *block, uint64_t old_size, uint64_t align, uint64_t new_size) {
    (void)old_size;
    (void)align;
    return realloc(block, new_size);
}
void aura_trap(uint32_t code, const char *file, uint64_t file_len, uint32_t line, uint32_t column) {
    (void)line;
    (void)column;
    printf("trap %u at %.*s\n", code, (int)file_len, file);
    exit(0);
}
int main(void) {
    printf("%lld %lld\n", (long long)entry(2), (long long)entry(-3));
    printf("%lld\n", (long long)entry(3));
    return 0;
}
"#,
    );
    let binary = workspace.path.join("range.bin");
    let link = Command::new(compiler)
        .arg(&harness)
        .arg(workspace.path.join("range.o"))
        .arg("-o")
        .arg(&binary)
        .output()
        .expect("the C compiler should start");
    assert!(link.status.success(), "{}", stderr(&link));
    let run = Command::new(&binary)
        .output()
        .expect("the linked program should start");
    assert_eq!(stdout(&run), "3 1\ntrap 3 at aura/freestanding_list.au\n");
}

/// Runs `source` hosted and as a freestanding object linked with hooks
/// that count: both print `expected`, and no block is left.
fn blocks_are_all_freed(compiler: &str, source: &str, entry: &str, expected: &str) {
    let workspace = Workspace::new();
    workspace.write("blocks.au", source);
    let hosted = workspace.aura(&["run", "blocks.au"]);
    assert!(hosted.status.success(), "{}", stderr(&hosted));
    assert_eq!(stdout(&hosted), expected);
    let build = workspace.aura(&["build", "--freestanding", "-o", "blocks.o", "blocks.au"]);
    assert!(build.status.success(), "{}", stderr(&build));
    let harness = workspace.write(
        "blocks.c",
        &r#"
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
int64_t ENTRY(int64_t n);
static long made, live;
void *aura_alloc(uint64_t size, uint64_t align) {
    (void)align;
    made++;
    live++;
    return malloc(size);
}
void aura_free(void *block, uint64_t size, uint64_t align) {
    (void)size;
    (void)align;
    live--;
    free(block);
}
void *aura_realloc(void *block, uint64_t old_size, uint64_t align, uint64_t new_size) {
    (void)old_size;
    (void)align;
    return realloc(block, new_size);
}
void aura_trap(uint32_t code, const char *file, uint64_t file_len, uint32_t line, uint32_t column) {
    printf("trap %u at %.*s:%u:%u\n", code, (int)file_len, file, line, column);
    exit(1);
}
int main(void) {
    printf("%lld\n", (long long)ENTRY(40));
    printf("made %d live %ld\n", made > 0, live);
    return 0;
}
"#
        .replace("ENTRY", entry),
    );
    let binary = workspace.path.join("blocks.bin");
    let link = Command::new(compiler)
        .arg(&harness)
        .arg(workspace.path.join("blocks.o"))
        .arg("-o")
        .arg(&binary)
        .output()
        .expect("the C compiler should start");
    assert!(link.status.success(), "{}", stderr(&link));
    let run = Command::new(&binary)
        .output()
        .expect("the linked program should start");
    assert!(run.status.success(), "{}", stdout(&run));
    assert_eq!(stdout(&run), format!("{expected}made 1 live 0\n"));
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

    workspace.write(
        "state.au",
        r#"
mut tick: uint64 = 0

def update(value: uint64):
    tick = value

export "C" def snapshot(value: uint64):
    before = tick
    update(value)
    assert tick == before
"#,
    );
    let output = workspace.aura(&[
        "build",
        "--freestanding",
        "--remove-proved-checks",
        "-o",
        "state.o",
        "state.au",
    ]);
    assert!(output.status.success(), "{}", stderr(&output));
    assert!(
        stderr(&output).contains("0 checks proved and removed"),
        "{}",
        stderr(&output)
    );
    let object = fs::read(workspace.path.join("state.o")).unwrap();
    assert!(object
        .windows(b"aura_trap".len())
        .any(|name| name == b"aura_trap"));
}

const MEMORY_MANIFEST: &str =
    "[package]\nname = \"board\"\nversion = \"0.1.0\"\nedition = \"2026\"\nallow_ffi = true\n";

/// A program over the `mem` module that runs hosted on a block from the C
/// allocator and exports the same routine for a freestanding build.
const MEMORY_PROGRAM: &str = r#"
import mem

extern "C" def malloc(size: uint64) -> uint64
extern "C" def free(address: uint64) -> None

export "C" def exercise(block: uint64) -> uint64:
    mem.write_uint64(block, 18446744073709551615)
    mem.write_uint8(block + 1, 0)
    mut total = mem.read_uint64(block) % 1000003
    total = total + (mem.read_uint16(block + 1) as uint64)
    mem.volatile_write_uint32(block + 8, 4000000000)
    total = total + (mem.volatile_read_uint32(block + 8) as uint64)
    mem.volatile_write_uint16(block + 12, 65535)
    total = total + (mem.volatile_read_uint8(block + 13) as uint64)
    mem.atomic_store_uint32(block + 16, 5)
    total = total + (mem.atomic_add_uint32(block + 16, 4294967295) as uint64)
    total = total + (mem.atomic_load_uint32(block + 16) as uint64)
    total = total + (mem.atomic_compare_exchange_uint32(block + 16, 4, 9) as uint64)
    total = total + (mem.atomic_compare_exchange_uint32(block + 16, 4, 11) as uint64)
    total = total + (mem.atomic_swap_uint32(block + 16, 1) as uint64)
    mem.atomic_store_uint64(block + 24, 10)
    total = total + mem.atomic_add_uint64(block + 24, 5)
    total = total + mem.atomic_swap_uint64(block + 24, 99)
    total = total + mem.atomic_compare_exchange_uint64(block + 24, 99, 100)
    mem.fence()
    return total + mem.atomic_load_uint64(block + 24)

def main() -> None:
    block = malloc(64)
    print(exercise(block))
    free(block)
"#;

#[test]
fn the_mem_module_reads_and_writes_the_same_bytes_in_every_backend() {
    let workspace = Workspace::new();
    workspace.write("Aura.toml", MEMORY_MANIFEST);
    workspace.write("src/main.au", MEMORY_PROGRAM);
    let hosted = workspace.aura(&["run", "src/main.au"]);
    assert!(hosted.status.success(), "{}", stderr(&hosted));
    let expected = stdout(&hosted);
    assert!(expected.trim().parse::<u64>().is_ok(), "{expected}");

    let build = workspace.aura(&["build", "-o", "board", "src/main.au"]);
    assert!(build.status.success(), "{}", stderr(&build));
    let native = Command::new(workspace.path.join("board"))
        .output()
        .expect("the binary should start");
    assert!(native.status.success(), "{}", stderr(&native));
    assert_eq!(stdout(&native), expected);

    for target in [
        "x86_64-unknown-none",
        "aarch64-unknown-none",
        "riscv64gc-unknown-none-elf",
    ] {
        let output = workspace.aura(&[
            "build",
            "--freestanding",
            "--target",
            target,
            "-o",
            "board.o",
            "src/main.au",
        ]);
        assert!(output.status.success(), "{target}: {}", stderr(&output));
    }

    let Some(compiler) = c_compiler() else {
        return;
    };
    let build = workspace.aura(&["build", "--freestanding", "-o", "board.o", "src/main.au"]);
    assert!(build.status.success(), "{}", stderr(&build));
    let harness = workspace.write(
        "board.c",
        "#include <stdint.h>\n#include <stdio.h>\nuint64_t exercise(uint64_t block);\nvoid aura_trap(void) { puts(\"trap\"); }\nint main(void) { uint64_t block[8] = {0}; printf(\"%llu\\n\", (unsigned long long)exercise((uint64_t)block)); return 0; }\n",
    );
    let binary = workspace.path.join("board-c");
    let link = Command::new(compiler)
        .arg(&harness)
        .arg(workspace.path.join("board.o"))
        .arg("-o")
        .arg(&binary)
        .output()
        .expect("the C compiler should start");
    assert!(link.status.success(), "{}", stderr(&link));
    let run = Command::new(&binary)
        .output()
        .expect("the linked program should start");
    assert_eq!(stdout(&run), expected);
}

#[test]
fn the_mem_module_needs_the_ffi_permission() {
    let workspace = Workspace::new();
    let source = "import mem\n\ndef main() -> None:\n    mem.fence()\n";
    workspace.write("loose.au", source);
    let output = workspace.aura(&["check", "loose.au"]);
    assert_eq!(output.status.code(), Some(1));
    assert!(
        stderr(&output).contains("allow_ffi = true"),
        "{}",
        stderr(&output)
    );

    workspace.write(
        "Aura.toml",
        "[package]\nname = \"board\"\nversion = \"0.1.0\"\nedition = \"2026\"\n",
    );
    workspace.write(
        "src/main.au",
        "from mem import fence\n\ndef main() -> None:\n    fence()\n",
    );
    let output = workspace.aura(&["check", "src/main.au"]);
    assert_eq!(output.status.code(), Some(1));
    assert!(
        stderr(&output).contains("allow_ffi = true"),
        "{}",
        stderr(&output)
    );

    workspace.write("Aura.toml", MEMORY_MANIFEST);
    let output = workspace.aura(&["run", "src/main.au"]);
    assert!(output.status.success(), "{}", stderr(&output));
}

#[test]
fn the_bare_metal_example_builds_for_its_boards() {
    let example = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/freestanding/kernel");
    let source = example.join("src/main.au");
    let workspace = Workspace::new();
    for target in ["riscv64gc-unknown-none-elf", "aarch64-unknown-none"] {
        let output = Command::new(aura_bin())
            .args(["build", "--freestanding", "--target", target, "-o"])
            .arg(workspace.path.join("kernel.o"))
            .arg(&source)
            .env("AURA_CACHE_DIR", workspace.path.join("cache"))
            .output()
            .expect("aura should start");
        assert!(output.status.success(), "{target}: {}", stderr(&output));
        let object = fs::read(workspace.path.join("kernel.o")).expect("the object exists");
        for symbol in [&b"kernel_main\0"[..], b"aura_trap\0", b"power_off\0"] {
            assert!(
                object.windows(symbol.len()).any(|window| window == symbol),
                "{target}: the object names {}",
                String::from_utf8_lossy(symbol)
            );
        }
    }
    // The README and the book quote the line of the overflow the example
    // makes on purpose.
    let text = fs::read_to_string(&source).expect("the example exists");
    let line = text
        .lines()
        .position(|line| line.trim() == "small = small + 1")
        .expect("the example overflows a uint8")
        + 1;
    let readme = fs::read_to_string(example.join("README.md")).expect("the README exists");
    assert!(
        readme.contains(&format!("trap 1 main.au:{line}:13")),
        "line {line}"
    );
}

#[test]
fn a_cross_target_selects_its_architecture_and_frozen_table() {
    let workspace = Workspace::new();
    workspace.write(
        "table.au",
        r#"import build
if build.arch == "riscv64" and build.freestanding:
    def make() -> list[int64]:
        return [3 * 7, 6 * 7]
    const table = make()
    export "C" def lookup(index: int64) -> int64:
        return table[index]
else:
    import missing_on_riscv
"#,
    );
    let output = workspace.aura(&[
        "build",
        "--freestanding",
        "--target",
        "riscv64gc-unknown-none-elf",
        "-o",
        "table.o",
        "table.au",
    ]);
    assert!(output.status.success(), "{}", stderr(&output));
    let object = fs::read(workspace.path.join("table.o")).unwrap();
    assert_eq!(u16::from_le_bytes([object[18], object[19]]), 243);
}

#[test]
fn target_selects_declarations_before_checking_and_building() {
    let workspace = Workspace::new();
    workspace.write(
        "main.au",
        "import build\nif build.arch == \"riscv64\":\n    export \"C\" def answer() -> int64:\n        return 42\nelse:\n    import unavailable_elsewhere\n",
    );
    let build = workspace.aura(&[
        "build",
        "main.au",
        "--freestanding",
        "--target",
        "riscv64gc-unknown-none-elf",
        "-o",
        "target.o",
    ]);
    assert!(build.status.success(), "{}", stderr(&build));
    let check = workspace.aura(&["check", "--all-targets", "main.au"]);
    assert!(!check.status.success());
    let diagnostic = stderr(&check);
    assert!(diagnostic.contains("unavailable_elsewhere"), "{diagnostic}");
    assert!(diagnostic.contains("x86_64"), "{diagnostic}");
}

#[test]
fn linked_frozen_collections_use_aura_indexing_and_lookup_rules() {
    let Some(compiler) = c_compiler() else { return };
    let workspace = Workspace::new();
    workspace.write(
        "table.au",
        include_str!(
            "../../aura-compiler/tests/fixtures/check-pass/compile_time_frozen_projections.au"
        ),
    );
    let build = workspace.aura(&["build", "--freestanding", "-o", "table.o", "table.au"]);
    assert!(build.status.success(), "{}", stderr(&build));
    workspace.write(
        "harness.c",
        r#"
#include <stdint.h>
#include <stdlib.h>
int64_t total(int64_t i, int64_t key);
int64_t nested(int64_t i);
int64_t details(int64_t i);
int64_t title_len(void);
uint64_t wide_words(int64_t i);
int64_t wide_keys(int64_t i);
void aura_trap(uint32_t code, const char *file, size_t len, uint32_t line, uint32_t col) {
    exit(code);
}
int main(int argc, char **argv) {
    if (argc > 1) { total(-3, 10); return 1; }
    return (total(-1, 40) == 126 && total(0, 30) == 111
        && nested(0) == 47 && nested(1) == 52
        && details(0) == 15 && title_len() == 5
        && wide_words(0) == 18 && wide_words(1) == 30
        && wide_keys(5) == 3 && wide_keys(0) == 7 && wide_keys(2) == -1) ? 0 : 1;
}
"#,
    );
    let path = workspace.path.join("lookup");
    let link = Command::new(compiler)
        .current_dir(&workspace.path)
        .args(["harness.c", "table.o", "-o"])
        .arg(&path)
        .output()
        .unwrap();
    assert!(link.status.success(), "{}", stderr(&link));
    assert_eq!(Command::new(&path).status().unwrap().code(), Some(0));
    assert_eq!(
        Command::new(&path).arg("trap").status().unwrap().code(),
        Some(10)
    );
}

#[test]
fn kernel_profile_emits_integer_register_float_code() {
    let workspace = Workspace::new();
    workspace.write(
        "kernel.au",
        include_str!("../../aura-compiler/tests/fixtures/check-pass/kernel_soft_float.au"),
    );
    for command in ["check", "build"] {
        let mut args = vec![command, "--freestanding", "--kernel", "--target"];
        if command == "build" {
            args.extend(["x86_64-unknown-none", "-o", "kernel.o", "kernel.au"]);
        } else {
            args.extend(["x86_64-unknown-none", "kernel.au"]);
        }
        let result = workspace.aura(&args);
        assert!(result.status.success(), "{}", stderr(&result));

        let target_index = args.iter().position(|arg| *arg == "--target").unwrap() + 1;
        args[target_index] = "aarch64-unknown-none";
        let result = workspace.aura(&args);
        assert_eq!(result.status.code(), Some(2));
        assert!(stderr(&result).contains("--kernel"), "{}", stderr(&result));
    }

    let bytes = fs::read(workspace.path.join("kernel.o")).unwrap();
    assert!(bytes.windows(8).any(|window| window == b"__adddf3"));
    if cfg!(all(target_os = "linux", target_arch = "x86_64")) {
        let disassembly = Command::new("objdump")
            .args(["-dr", "kernel.o"])
            .current_dir(&workspace.path)
            .output()
            .expect("GNU objdump should disassemble the x86-64 ELF object");
        assert!(disassembly.status.success(), "{}", stderr(&disassembly));
        let text = stdout(&disassembly);
        assert!(text.contains("__adddf3"), "{text}");
        for register in ["%xmm", "%ymm", "%zmm", "%mm", "%st("] {
            assert!(!text.contains(register), "{text}");
        }
    }
}

#[test]
#[cfg(all(target_os = "linux", target_arch = "x86_64"))]
fn kernel_profile_reserves_space_for_leaf_spills() {
    let workspace = Workspace::new();
    workspace.write("Aura.toml", MEMORY_MANIFEST);
    // Twenty independent volatile reads force a leaf to spill registers.
    let mut source = String::from("import mem\n\nexport \"C\" def spill(port: uint64) -> None:\n");
    for index in 0..20 {
        source.push_str(&format!(
            "    value{index} = mem.volatile_read_uint64(port)\n"
        ));
    }
    for index in (0..20).rev() {
        source.push_str(&format!(
            "    mem.volatile_write_uint64(port, value{index})\n"
        ));
    }
    workspace.write("src/kernel.au", &source);
    for kernel in [false, true] {
        let mut args = vec!["build", "--freestanding", "--target", "x86_64-unknown-none"];
        if kernel {
            args.push("--kernel");
        }
        args.extend(["-o", "spill.o", "src/kernel.au"]);
        let build = workspace.aura(&args);
        assert!(build.status.success(), "{}", stderr(&build));
        let disassembly = Command::new("objdump")
            .args(["-d", "-j", ".text.spill", "spill.o"])
            .current_dir(&workspace.path)
            .output()
            .expect("GNU objdump should disassemble the leaf");
        assert!(disassembly.status.success(), "{}", stderr(&disassembly));
        let text = stdout(&disassembly);
        assert!(text.contains("(%rsp)"), "expected stack spills: {text}");
        let red_zone = text
            .lines()
            .any(|line| line.contains("-0x") && line.contains("(%rsp)"));
        // Every freestanding x86-64 function avoids the red zone: an
        // interrupt may arrive in any of them, kernel profile or not.
        assert!(!red_zone, "{text}");
    }
}

#[test]
fn a_pointer_updates_an_array_inside_a_packed_module_cell() {
    let workspace = Workspace::new();
    workspace.write("Aura.toml", MEMORY_MANIFEST);
    workspace.write(
        "src/main.au",
        include_str!("../../aura-compiler/tests/fixtures/freestanding/systems/header.au"),
    );
    for target in [
        "x86_64-unknown-none",
        "aarch64-unknown-none",
        "riscv64gc-unknown-none-elf",
    ] {
        let built = workspace.aura(&[
            "build",
            "--freestanding",
            "--target",
            target,
            "-o",
            "header.o",
            "src/main.au",
        ]);
        assert!(built.status.success(), "{target}: {}", stderr(&built));
    }
    let Some(cc) = c_compiler() else {
        return;
    };
    let built = workspace.aura(&["build", "--freestanding", "-o", "header.o", "src/main.au"]);
    assert!(built.status.success(), "{}", stderr(&built));
    workspace.write(
        "header.c",
        r#"#include <stdint.h>
#include <stdlib.h>
uint16_t advance_header(uint16_t delta);
uint16_t header_flags(void);
void aura_trap(uint32_t c, const char *f, size_t n, uint32_t l, uint32_t k) { abort(); }
int main(void) {
    return header_flags() != 195 || advance_header(18) != 42 ||
        header_flags() != 197 || advance_header(1) != 43;
}
"#,
    );
    let link = Command::new(cc)
        .current_dir(&workspace.path)
        .args(["header.c", "header.o", "-o", "header"])
        .output()
        .unwrap();
    assert!(link.status.success(), "{}", stderr(&link));
    assert!(Command::new(workspace.path.join("header"))
        .status()
        .unwrap()
        .success());
}

#[test]
fn module_state_keeps_its_value_between_calls() {
    let workspace = Workspace::new();
    workspace.write(
        "state.au",
        include_str!("../../aura-compiler/tests/fixtures/freestanding/module_state/counter.au"),
    );
    for backend in ["mir", "direct"] {
        let output = workspace.aura(&["run", "--backend", backend, "state.au"]);
        assert!(!output.status.success());
        assert!(stderr(&output).contains("AU3030"), "{}", stderr(&output));
        assert!(stderr(&output).contains("sync.Mutex"));
    }
    let output = workspace.aura(&["build", "--freestanding", "-o", "state.o", "state.au"]);
    assert!(output.status.success(), "{}", stderr(&output));
    if cfg!(target_os = "linux") {
        workspace.write("state.c", r#"
#include <stdint.h>
#include <stdlib.h>
extern uint32_t advance(uint32_t n);
void aura_trap(uint32_t code, const char *file, size_t len, uint32_t line, uint32_t col) { abort(); }
int main(void) { return advance(1) == 12 && advance(3) == 17 ? 0 : 1; }
"#);
        let cc = Command::new("cc")
            .current_dir(&workspace.path)
            .args(["state.c", "state.o", "-o", "test-state"])
            .output()
            .unwrap();
        assert!(cc.status.success(), "{}", stderr(&cc));
        assert!(Command::new(workspace.path.join("test-state"))
            .status()
            .unwrap()
            .success());
    }
}

#[test]
fn assembly_stops_proofs_and_preserves_runtime_checks() {
    let workspace = Workspace::new();
    workspace.write(
        "Aura.toml",
        "[package]\nname = \"proof\"\nversion = \"0.1.0\"\nedition = \"2026\"\nallow_ffi = true\n",
    );
    workspace.write(
        "src/main.au",
        "export \"C\" def entry(x: uint64) -> uint64:\n    mut copy = x\n    asm(\"movq $1, $0\", output(\"reg\", copy), input(\"reg\", x), options(\"pure\", \"preserves_flags\"))\n    assert copy == x\n    return copy\n",
    );
    for args in [
        &["verify", "src/main.au"][..],
        &[
            "verify",
            "--freestanding",
            "--target",
            "x86_64-unknown-none",
            "--kernel",
            "src/main.au",
        ][..],
    ] {
        let verify = workspace.aura(args);
        assert!(!verify.status.success());
        assert!(stderr(&verify).contains("AU4035"), "{}", stderr(&verify));
        assert!(stderr(&verify).contains("assembly"), "{}", stderr(&verify));
    }

    let build = workspace.aura(&[
        "build",
        "--freestanding",
        "--target",
        "x86_64-unknown-none",
        "--remove-proved-checks",
        "-o",
        "proof.o",
        "src/main.au",
    ]);
    assert!(build.status.success(), "{}", stderr(&build));
    assert!(
        stderr(&build).contains("0 checks proved and removed"),
        "{}",
        stderr(&build)
    );
    assert!(workspace.path.join("proof.o").is_file());
}

#[test]
fn ordered_subword_atomics_match_in_the_interpreter_and_native_code() {
    let workspace = Workspace::new();
    workspace.write("Aura.toml", MEMORY_MANIFEST);
    let program =
        include_str!("../../aura-compiler/tests/fixtures/systems-integers/atomic_orders.au");
    workspace.write("src/main.au", &format!("{program}\nextern \"C\" def malloc(n: uint64) -> uint64\nextern \"C\" def free(p: uint64) -> None\n\ndef main() -> None:\n    base = malloc(8)\n    print(exchange(base, base + 2))\n    free(base)\n"));
    for backend in ["mir", "direct"] {
        let run = workspace.aura(&["run", "--backend", backend, "src/main.au"]);
        assert!(run.status.success(), "{backend}: {}", stderr(&run));
        assert_eq!(stdout(&run), "255\n", "{backend}");
    }
}

#[test]
fn linked_wide_integers_preserve_high_bits_and_trap_on_signed_overflow() {
    let Some(cc) = c_compiler() else {
        return;
    };
    let workspace = Workspace::new();
    workspace.write(
        "wide.au",
        include_str!("../../aura-compiler/tests/fixtures/systems-integers/wide_vectors.au"),
    );
    let built = workspace.aura(&["build", "--freestanding", "-o", "wide.o", "wide.au"]);
    assert!(built.status.success(), "{}", stderr(&built));
    let harness = workspace.write(
        "wide.c",
        r#"#include <stdint.h>
#include <stdlib.h>
uint32_t check_vectors(uint64_t high, uint64_t low);
int64_t signed_remainder_min(int64_t minus_one);
int64_t overflow_divide(int64_t minus_one);
void aura_trap(uint32_t code, const char *file, uint64_t len, uint32_t line, uint32_t column) {
    exit(file && len && line && column ? 60 + (int)code : 1);
}
int main(int argc, char **argv) {
    if (argc == 2) return (int)overflow_divide(-1);
    return check_vectors(UINT64_C(1) << 63, 3) != 42 || signed_remainder_min(-1) != 0;
}
"#,
    );
    let binary = workspace.path.join("wide-test");
    let link = Command::new(cc)
        .arg(harness)
        .arg(workspace.path.join("wide.o"))
        .arg("-o")
        .arg(&binary)
        .output()
        .unwrap();
    assert!(link.status.success(), "{}", stderr(&link));
    assert!(Command::new(&binary).status().unwrap().success());
    assert_eq!(
        Command::new(&binary)
            .arg("overflow")
            .status()
            .unwrap()
            .code(),
        Some(61)
    );
}

#[test]
fn a_constant_scale_and_offset_share_one_overflow_check() {
    let workspace = Workspace::new();
    let source = workspace.write(
        "scale.au",
        "export \"C\" def scale(x: uint64) -> uint64:\n    return 3 * x + 1\n",
    );
    let build = Command::new(aura_bin())
        .args([
            "build",
            "--freestanding",
            "--target",
            "x86_64-unknown-none",
            "-o",
        ])
        .arg(workspace.path.join("scale.o"))
        .arg(source)
        .env("AURA_DUMP_LLVM_IR", workspace.path.join("scale"))
        .output()
        .expect("aura should start");
    assert!(build.status.success(), "{}", stderr(&build));
    let ir = fs::read_to_string(workspace.path.join("scale.0.ll")).unwrap();
    assert!(!ir.contains("with.overflow"), "{ir}");
    assert!(
        ir.contains("6148914691236517204") || ir.contains("6148914691236517205"),
        "{ir}"
    );
}

#[test]
#[cfg(target_os = "linux")]
fn module_array_index_runs_once_and_rhs_updates_survive() {
    let workspace = Workspace::new();
    workspace.write(
        "ring.au",
        include_str!("../../aura-compiler/tests/fixtures/freestanding/module_state/fixed.au"),
    );
    let build = workspace.aura(&["build", "--freestanding", "-o", "ring.o", "ring.au"]);
    assert!(build.status.success(), "{}", stderr(&build));
    workspace.write("test.c",r#"
#include <stdint.h>
#include <stdlib.h>
extern int32_t write_ring(void);
void aura_trap(uint32_t code, const char *file, size_t len, uint32_t line, uint32_t col) { abort(); }
int main(void) { return write_ring() == 19 && write_ring() == 38 ? 0 : 1; }
"#);
    let cc = Command::new("cc")
        .current_dir(&workspace.path)
        .args(["test.c", "ring.o", "-o", "test-ring"])
        .output()
        .unwrap();
    assert!(cc.status.success(), "{}", stderr(&cc));
    assert!(Command::new(workspace.path.join("test-ring"))
        .status()
        .unwrap()
        .success());
}

#[test]
fn typed_pointers_run_on_both_hosted_backends() {
    let workspace = Workspace::new();
    workspace.write("Aura.toml", MEMORY_MANIFEST);
    for source in [
        include_str!("../../aura-compiler/tests/fixtures/typed-pointers/borrowed_owner.au"),
        include_str!("../../aura-compiler/tests/fixtures/typed-pointers/scalars.au"),
        include_str!("../../aura-compiler/tests/fixtures/typed-pointers/foreign.au"),
        include_str!("../../aura-compiler/tests/fixtures/typed-pointers/foreign_array.au"),
        include_str!("../../aura-compiler/tests/fixtures/typed-pointers/aggregate.au"),
        include_str!("../../aura-compiler/tests/fixtures/typed-pointers/links.au"),
        include_str!("../../aura-compiler/tests/fixtures/typed-pointers/widths.au"),
        include_str!("../../aura-compiler/tests/fixtures/typed-pointers/nested.au"),
        include_str!("../../aura-compiler/tests/fixtures/typed-pointers/wide.au"),
        include_str!("../../aura-compiler/tests/fixtures/typed-pointers/fixed.au"),
    ] {
        workspace.write("src/main.au", source);
        for backend in ["mir", "direct"] {
            let run = workspace.aura(&["run", "--backend", backend, "src/main.au"]);
            assert!(run.status.success(), "{backend}: {}", stderr(&run));
        }
    }
}

#[test]
fn typed_pointer_module_addresses_are_persistent() {
    let workspace = Workspace::new();
    workspace.write("Aura.toml", MEMORY_MANIFEST);
    workspace.write(
        "src/main.au",
        include_str!("../../aura-compiler/tests/fixtures/typed-pointers/state.au"),
    );
    for target in [
        "x86_64-unknown-none",
        "aarch64-unknown-none",
        "riscv64gc-unknown-none-elf",
    ] {
        let build = workspace.aura(&[
            "build",
            "--freestanding",
            "--target",
            target,
            "-o",
            "state.o",
            "src/main.au",
        ]);
        assert!(build.status.success(), "{target}: {}", stderr(&build));
    }
}

#[test]
fn typed_pointer_volatile_wide_accesses_keep_their_llvm_width() {
    let workspace = Workspace::new();
    workspace.write("Aura.toml", MEMORY_MANIFEST);
    let source = workspace.write(
        "src/main.au",
        include_str!("../../aura-compiler/tests/fixtures/typed-pointers/wide.au"),
    );
    let build = Command::new(aura_bin())
        .current_dir(&workspace.path)
        .args([
            "build",
            "--freestanding",
            "--target",
            "x86_64-unknown-none",
            "-o",
            "wide.o",
        ])
        .arg(source)
        .env("AURA_DUMP_LLVM_IR", workspace.path.join("pointer"))
        .output()
        .unwrap();
    assert!(build.status.success(), "{}", stderr(&build));
    let ir = fs::read_to_string(workspace.path.join("pointer.0.ll")).unwrap();
    assert!(ir.contains("load volatile i128"), "{ir}");
    assert!(ir.contains("store volatile i128"), "{ir}");
}

#[test]
fn a_freestanding_union_updates_its_overlapping_c_representation() {
    let Some(cc) = c_compiler() else {
        return;
    };
    let workspace = Workspace::new();
    workspace.write("Aura.toml", MEMORY_MANIFEST);
    workspace.write(
        "src/main.au",
        r#"import mem
extern "C" union Word:
    whole: uint32
    low: uint8

mut WORD_SIZE: uint64 = mem.size_of(Word)
mut LAYOUT: fixed[uint64, 2] = [mem.align_of(Word), mem.size_of(Word)]

export "C" def word_size() -> uint64:
    return WORD_SIZE + LAYOUT[0] + LAYOUT[1]

export "C" def replace_low(original: uint32, low: uint8) -> uint32:
    mut word = Word.from_whole(original)
    word.low = low
    return word.whole

export "C" def from_zero(low: uint8) -> uint32:
    mut word = Word()
    word.low = low
    return word.whole
"#,
    );
    let build = workspace.aura(&["build", "--freestanding", "-o", "word.o", "src/main.au"]);
    assert!(build.status.success(), "{}", stderr(&build));
    workspace.write(
        "word.c",
        r#"#include <stdint.h>
#include <stdlib.h>
uint64_t word_size(void);
uint32_t replace_low(uint32_t, uint8_t);
uint32_t from_zero(uint8_t);
void aura_trap(uint32_t code, const char *file, uint64_t n, uint32_t line, uint32_t column) {
    abort();
}
int main(void) {
    union { uint32_t whole; uint8_t low; } w = { .whole = 0x12345678 };
    if (word_size() != 2 * sizeof(w) + _Alignof(uint32_t)) return 1;
    w.low = 10;
    if (replace_low(0x12345678, 10) != w.whole) return 1;
    w.whole = 0;
    w.low = 15;
    return from_zero(15) != w.whole;
}
"#,
    );
    let linked = Command::new(cc)
        .current_dir(&workspace.path)
        .args(["word.c", "word.o", "-o", "word-test"])
        .output()
        .unwrap();
    assert!(linked.status.success(), "{}", stderr(&linked));
    let run = Command::new(workspace.path.join("word-test"))
        .output()
        .unwrap();
    assert!(run.status.success(), "{run:?}");
}
