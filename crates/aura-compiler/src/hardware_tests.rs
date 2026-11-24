use super::*;

const X86: &str = include_str!("../tests/fixtures/machine-hardware/x86.au");
const ARM: &str = include_str!("../tests/fixtures/machine-hardware/arm.au");
const RV: &str = include_str!("../tests/fixtures/machine-hardware/rv.au");

fn check(source: &str) -> diag::Result<Program> {
    check_module_with_builtin_imports(parse_source(source)?)
}

#[test]
fn hardware_api_selects_target_for_each_request() {
    let wrong = include_str!("../tests/fixtures/machine-hardware/wrong-target.au");
    for (target, source) in [
        (FreestandingTarget::X86_64Kernel, X86),
        (FreestandingTarget::Aarch64, ARM),
        (FreestandingTarget::Riscv64, RV),
        (FreestandingTarget::X86_64, X86),
    ] {
        with_freestanding_target(target, || {
            check(source).unwrap();
            if target.arch() != "x86_64" {
                assert_eq!(check(wrong).unwrap_err().code, "AU2097");
            } else {
                let err = check(include_str!(
                    "../tests/fixtures/machine-hardware/wrong-width.au"
                ))
                .unwrap_err();
                assert!(err.message.contains("uint16"), "{err:?}");
            }
        });
    }
    let err = check(include_str!("../tests/fixtures/machine-hardware/hosted.au")).unwrap_err();
    assert_eq!(err.code, "AU2097");
    assert_eq!(check(wrong).unwrap_err().code, "AU2097");
}

/// Disassemble the exact ELF bytes on any compiler host using the pinned
/// llvm-tools-preview component, including on Windows and macOS.
fn listing(bytes: &[u8]) -> String {
    use std::io::Write;
    use std::process::{Command, Stdio};
    let rustc = std::env::var_os("RUSTC").unwrap_or_else(|| "rustc".into());
    let libdir = Command::new(rustc)
        .args(["--print", "target-libdir"])
        .output()
        .unwrap();
    assert!(libdir.status.success());
    let libdir = String::from_utf8(libdir.stdout).unwrap();
    let tool = Path::new(libdir.trim())
        .parent()
        .unwrap()
        .join("bin")
        .join(format!("llvm-objdump{}", std::env::consts::EXE_SUFFIX));
    let mut child = Command::new(tool)
        .args(["-d", "--no-show-raw-insn", "--no-leading-addr", "-"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child.stdin.take().unwrap().write_all(bytes).unwrap();
    let output = child.wait_with_output().unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout)
        .unwrap()
        .to_ascii_lowercase()
}

#[test]
fn hardware_objects_contain_target_instructions() {
    for (target, source, expected) in [
        (
            FreestandingTarget::X86_64Kernel,
            X86,
            &[
                "inb", "inw", "inl", "outb", "outw", "outl", "cli", "sti", "hlt", "%cr3", "rdmsr",
                "wrmsr",
            ][..],
        ),
        (
            FreestandingTarget::Aarch64,
            ARM,
            &["daifset", "daifclr", "wfi", "vbar_el1", "midr_el1"][..],
        ),
        (
            FreestandingTarget::Riscv64,
            RV,
            &["csrci", "csrsi", "wfi", "mtvec", "mhartid"][..],
        ),
    ] {
        let program = with_freestanding_target(target, || check(source)).unwrap();
        let mir = mir::lower_with_source_locations(&program);
        let bytes = emit_freestanding_object(&mir, "cpu.au", source, target).unwrap();
        let listing = listing(&bytes);
        let tokens: Vec<_> = listing
            .split(|ch: char| ch.is_whitespace() || ch == ',')
            .collect();
        for needle in expected {
            assert!(
                tokens.contains(needle),
                "{target:?} lacks {needle}: {listing}"
            );
        }
    }
}

#[test]
fn every_declared_system_register_assembles_on_its_target() {
    for target in FreestandingTarget::ALL {
        let declarations = parse_source(hardware::source(target)).unwrap();
        let mut source = String::from("import mem\nexport \"C\" def smoke() -> None:\n");
        for item in declarations.items {
            let ast::Item::Function(func) = item else {
                panic!("expected a function")
            };
            let args = vec!["0"; func.params.len()].join(", ");
            source.push_str(&format!("    mem.{}({args})\n", func.name));
        }
        let program = with_freestanding_target(target, || check(&source)).unwrap();
        let module = mir::lower_with_source_locations(&program);
        emit_freestanding_object(&module, "smoke.au", &source, target).unwrap();
    }
}

#[test]
fn unavailable_machine_members_report_the_architecture() {
    for spelling in [
        "import mem as hw\ndef f() -> None:\n    hw.read_cr3()\n",
        "import mem as hw\ndef f() -> None:\n    getter = hw.read_cr3\n",
        "from mem import read_cr3 as getter\n",
    ] {
        let error =
            with_freestanding_target(FreestandingTarget::Aarch64, || check(spelling)).unwrap_err();
        assert_eq!(error.code, "AU2097");
        assert!(error.message.contains("x86_64-unknown-none"));
    }
    for source in [
        "import mem\ndef f() -> None:\n    mem.misspelled()\n",
        "import math\ndef f() -> None:\n    math.read_cr0()\n",
    ] {
        assert_ne!(check(source).unwrap_err().code, "AU2097");
    }
}
