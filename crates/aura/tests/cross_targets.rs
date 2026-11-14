//! `aura build --target` for Linux machines other than the build machine
//! (ADR-0101).
//!
//! The builds need a runtime library and a linker for each target, so
//! they run for the targets `AURA_TEST_CROSS_TARGETS` lists, separated by
//! commas. The CI job that installs the toolchains sets it. A program for
//! another architecture runs under the QEMU user-mode emulator.

#![cfg(target_os = "linux")]

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::atomic::{AtomicU64, Ordering};

static NEXT_DIRECTORY: AtomicU64 = AtomicU64::new(0);

/// A directory of sources that is removed when the test ends.
struct Workspace {
    path: PathBuf,
}

impl Workspace {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "aura-cross-targets-{}-{}",
            std::process::id(),
            NEXT_DIRECTORY.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(&path).expect("the workspace should be made");
        Self { path }
    }

    fn write(&self, name: &str, text: &str) -> PathBuf {
        let path = self.path.join(name);
        fs::write(&path, text).expect("the file should be written");
        path
    }

    fn aura(&self, args: &[&str]) -> Output {
        Command::new(env!("CARGO_BIN_EXE_aura"))
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

/// A program that uses the parts of the runtime that lean on the C
/// library and the kernel: its arguments, tasks on several threads, a
/// loopback socket, files, the clock, and formatted floats.
const PROGRAM: &str = r#"
import fs
import io
import net
import sys
import time


def square(value: int64) -> int64:
    return value * value


def echo_once(addresses: Queue[str]) -> Result[int64, io.Error]:
    with listener = try net.listen("127.0.0.1:0"):
        _ = addresses.put(try listener.local_addr())
        with conn = try listener.accept():
            match own try conn.read_line():
                case str as line:
                    try conn.write_all("echo " + line + "\n")
                case None:
                    pass
    return Result.Ok(1)


def exchange() -> Result[str, io.Error]:
    mut reply = ""
    with TaskGroup() as group:
        addresses = Queue[str](capacity=1)
        _ = group.start(echo_once, addresses)
        match own addresses.get(timeout=5s):
            case QueueReceive.Item(address):
                with conn = try net.connect(address):
                    try conn.write_all("over loopback\n")
                    match own try conn.read_line():
                        case str as line:
                            reply = line
                        case None:
                            pass
            case _:
                return Result.Err(io.Error.TimedOut)
    return Result.Ok(reply)


def main() -> int32:
    args = sys.args()
    print(f"args {args.len()} {args[0]} {args[1]}")
    mut total = 0
    with TaskGroup() as group:
        mut handles: list[Task[int64]] = []
        for i in range(64):
            handles.append(group.start(square, i))
        for handle in handles:
            match handle.result():
                case TaskResult.Ready(value):
                    total += value
                case _:
                    return 1
    print(f"squares {total}")
    match exchange():
        case Result.Ok(reply):
            print(reply)
        case Result.Err(_):
            print("exchange failed")
            return 1
    match fs.write_string("note.txt", "kept"):
        case Result.Ok(_):
            pass
        case Result.Err(_):
            return 1
    match fs.read_to_string("note.txt"):
        case Result.Ok(text):
            print(text)
        case Result.Err(_):
            return 1
    started = time.monotonic()
    ratios = {"pi": 3.25, "half": 0.5}
    print(ratios["pi"] * ratios["half"])
    print(time.monotonic() >= started)
    return 0
"#;

const PROGRAM_OUTPUT: &str =
    "args 2 first second arg\nsquares 85344\necho over loopback\nkept\n1.625\ntrue\n";

const PROGRAM_ARGS: &[&str] = &["first", "second arg"];

fn build(workspace: &Workspace, target: Option<&str>, output: &str) -> Output {
    workspace.write("program.au", PROGRAM);
    let mut args = vec!["build", "-o", output, "program.au"];
    if let Some(target) = target {
        args.extend(["--target", target]);
    }
    workspace.aura(&args)
}

fn run(workspace: &Workspace, program: &Path, target: &str, args: &[&str]) -> Output {
    let arch = target
        .split('-')
        .next()
        .expect("a target names its architecture");
    let mut command = if arch == std::env::consts::ARCH {
        Command::new(program)
    } else {
        let mut command = Command::new(format!("qemu-{arch}-static"));
        if !target.ends_with("-musl") {
            // The emulator loads the target's dynamic linker and C
            // library from the cross toolchain.
            command.arg("-L").arg(format!("/usr/{arch}-linux-gnu"));
        }
        command.arg(program);
        command
    };
    command
        .args(args)
        .current_dir(&workspace.path)
        .output()
        .expect("the program should start")
}

/// The ELF header fields that tell the targets apart: the machine, and
/// whether a program header asks for a dynamic linker.
fn elf_machine_and_interpreter(bytes: &[u8]) -> (u16, bool) {
    assert_eq!(&bytes[..4], b"\x7fELF", "the program should be an ELF file");
    let machine = u16::from_le_bytes([bytes[18], bytes[19]]);
    let header_offset = u64::from_le_bytes(bytes[32..40].try_into().unwrap()) as usize;
    let header_size = u16::from_le_bytes([bytes[54], bytes[55]]) as usize;
    let header_count = u16::from_le_bytes([bytes[56], bytes[57]]) as usize;
    let interpreter = (0..header_count).any(|index| {
        let start = header_offset + index * header_size;
        // PT_INTERP
        u32::from_le_bytes(bytes[start..start + 4].try_into().unwrap()) == 3
    });
    (machine, interpreter)
}

#[test]
fn the_program_builds_and_runs_for_the_build_machine() {
    let workspace = Workspace::new();
    let host = format!("{}-unknown-linux-gnu", std::env::consts::ARCH);
    let output = build(&workspace, Some(&host), "program");
    assert!(output.status.success(), "{}", stderr(&output));
    // The build machine's own target is an ordinary build.
    assert!(
        !stderr(&output).contains("direct backend for "),
        "{}",
        stderr(&output)
    );
    let ran = run(
        &workspace,
        &workspace.path.join("program"),
        &host,
        PROGRAM_ARGS,
    );
    assert!(ran.status.success(), "{}", stderr(&ran));
    assert_eq!(String::from_utf8_lossy(&ran.stdout), PROGRAM_OUTPUT);
}

#[test]
fn programs_build_and_run_for_the_listed_targets() {
    let listed = std::env::var("AURA_TEST_CROSS_TARGETS").unwrap_or_default();
    for target in listed.split(',').filter(|target| !target.is_empty()) {
        let workspace = Workspace::new();
        let output = build(&workspace, Some(target), "program");
        assert!(output.status.success(), "{target}: {}", stderr(&output));
        assert!(
            stderr(&output).contains(&format!("with direct backend for {target}")),
            "{target}: {}",
            stderr(&output)
        );
        let program = workspace.path.join("program");
        let bytes = fs::read(&program).expect("the program should be written");
        let (machine, interpreter) = elf_machine_and_interpreter(&bytes);
        let expected_machine = if target.starts_with("aarch64") {
            183
        } else {
            62
        };
        assert_eq!(machine, expected_machine, "{target}");
        // A musl program is static: it names no dynamic linker.
        assert_eq!(interpreter, !target.ends_with("-musl"), "{target}");
        let ran = run(&workspace, &program, target, PROGRAM_ARGS);
        assert!(ran.status.success(), "{target}: {}", stderr(&ran));
        assert_eq!(
            String::from_utf8_lossy(&ran.stdout),
            PROGRAM_OUTPUT,
            "{target}"
        );

        let debug = workspace.aura(&[
            "build",
            "--debug",
            "--target",
            target,
            "-o",
            "program-debug",
            "program.au",
        ]);
        assert!(debug.status.success(), "{target}: {}", stderr(&debug));
        let ran = run(
            &workspace,
            &workspace.path.join("program-debug"),
            target,
            PROGRAM_ARGS,
        );
        assert!(ran.status.success(), "{target}: {}", stderr(&ran));
        assert_eq!(
            String::from_utf8_lossy(&ran.stdout),
            PROGRAM_OUTPUT,
            "{target}"
        );

        // A failed check unwinds to the runtime's report. With no
        // arguments the program reads past the end of the list.
        let failed = run(&workspace, &program, target, &[]);
        assert_eq!(
            failed.status.code(),
            Some(1),
            "{target}: {}",
            stderr(&failed)
        );
        assert!(
            stderr(&failed).contains("error[AU4003]: list index `0` is out of bounds"),
            "{target}: {}",
            stderr(&failed)
        );
    }
}

#[test]
fn an_unknown_target_is_refused_with_the_list() {
    let workspace = Workspace::new();
    let output = build(&workspace, Some("sparc-sun-solaris"), "program");
    assert_eq!(output.status.code(), Some(2));
    assert_eq!(
        stderr(&output),
        "aura: unknown target `sparc-sun-solaris`; the targets are \
         `x86_64-unknown-linux-gnu`, `aarch64-unknown-linux-gnu`, \
         `x86_64-unknown-linux-musl`, `aarch64-unknown-linux-musl`\n"
    );
    assert!(!workspace.path.join("program").exists());
}

#[test]
fn a_freestanding_target_needs_the_freestanding_flag() {
    let workspace = Workspace::new();
    let output = build(&workspace, Some("aarch64-unknown-none"), "program");
    assert_eq!(output.status.code(), Some(2));
    assert_eq!(
        stderr(&output),
        "aura: `aarch64-unknown-none` is a freestanding target; pass `--freestanding` to build for it\n"
    );
}

#[test]
fn a_target_option_without_a_name_is_refused() {
    let workspace = Workspace::new();
    workspace.write("program.au", PROGRAM);
    let output = workspace.aura(&["build", "program.au", "--target"]);
    assert_eq!(output.status.code(), Some(2));
    assert_eq!(stderr(&output), "aura: `--target` needs a target name\n");
}

#[test]
fn the_native_cpu_does_not_apply_to_another_machine() {
    let workspace = Workspace::new();
    workspace.write("program.au", PROGRAM);
    let other = if std::env::consts::ARCH == "x86_64" {
        "aarch64-unknown-linux-gnu"
    } else {
        "x86_64-unknown-linux-gnu"
    };
    let output = workspace.aura(&[
        "build",
        "--target",
        other,
        "--target-cpu",
        "native",
        "program.au",
    ]);
    assert_eq!(output.status.code(), Some(2));
    assert_eq!(
        stderr(&output),
        "aura: target CPU `native` names the build machine's CPU and does not apply with `--target`\n"
    );
}
