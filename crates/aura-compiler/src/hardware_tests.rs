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
                let err = check(include_str!("../tests/fixtures/machine-hardware/wrong-width.au")).unwrap_err();
                assert!(err.message.contains("uint16"), "{err:?}");
            }
        });
    }
    let err = check(include_str!("../tests/fixtures/machine-hardware/hosted.au")).unwrap_err();
    assert_eq!(err.code, "AU2097");
    assert_eq!(check(wrong).unwrap_err().code, "AU2097");
}

#[test]
fn hardware_objects_contain_target_instructions() {
    use object::{Object, ObjectSection};
    for (target, source, patterns) in [
        (FreestandingTarget::X86_64Kernel, X86, &[
            &b"\xec"[..], b"\x66\xed", b"\xed", b"\xee", b"\x66\xef", b"\xef",
            b"\xfa", b"\xfb", b"\xf4", b"\x0f\x20", b"\x0f\x22", b"\x0f\x32", b"\x0f\x30"
        ][..]),
        (FreestandingTarget::Aarch64, ARM, &[&b"\x9f\x42\x03\xd5"[..], b"\xdf\x42\x03\xd5", b"\x7f\x20\x03\xd5"][..]),
        (FreestandingTarget::Riscv64, RV, &[&b"\x73\x70\x04\x30"[..], b"\x73\x60\x04\x30", b"\x73\x00\x50\x10"][..]),
    ] {
        let program = with_freestanding_target(target, || check(source)).unwrap();
        let mir = mir::lower_with_source_locations(&program);
        let bytes = emit_freestanding_object(&mir, "cpu.au", source, target).unwrap();
        let obj = object::File::parse(bytes.as_slice()).unwrap();
        let code: Vec<_> = obj.sections().filter(|s| s.kind()==object::SectionKind::Text).map(|s| s.data().unwrap()).collect();
        for insn in patterns {
            assert!(code.iter().any(|s| s.windows(insn.len()).any(|w| w==*insn)), "{target:?} lacks {insn:02x?}");
        }
    }
}
