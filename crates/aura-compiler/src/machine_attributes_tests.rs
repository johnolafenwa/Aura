use super::*;

#[test]
fn machine_attributes_parse_a_vector_and_reject_hosted_functions() {
    crate::parse_source(include_str!(
        "../tests/fixtures/parse-pass/machine_attributes.au"
    ))
    .unwrap();
    let source = include_str!("../tests/fixtures/check-fail/machine_attributes_hosted.au");
    assert_eq!(crate::check_source(source).unwrap_err().code, "AU2094");
}

#[test]
fn machine_attributes_place_align_retain_and_return_from_interrupt() {
    let source = include_str!("../tests/fixtures/parse-pass/machine_attributes.au");
    let target = FreestandingTarget::X86_64;
    let bytes = crate::with_freestanding_target(target, || object(source, target));
    let file = object::File::parse(bytes.as_slice()).unwrap();
    let section = file.section_by_name(".text.irq").unwrap();
    assert!(section.align() >= 64);
    assert!(
        matches!(section.flags(), object::SectionFlags::Elf { sh_flags } if sh_flags & 0x200000 != 0)
    );
    assert!(section
        .data()
        .unwrap()
        .windows(2)
        .any(|op| op == [0x48, 0xcf]));
    assert!(file
        .symbols()
        .any(|s| s.name() == Ok("timer_vector") && s.section_index() == Some(section.index())));
}

#[test]
fn machine_attributes_preserve_state_and_use_architectural_exception_returns() {
    for (target, convention, args, instructions) in [
        (
            FreestandingTarget::X86_64,
            "x86_64_error",
            "error: uint64",
            vec![
                &[0x48, 0x8b, 0x7b, 0x50][..],
                &[0x48, 0x83, 0xc4, 0x08],
                &[0x48, 0xcf],
            ],
        ),
        (
            FreestandingTarget::Aarch64,
            "aarch64",
            "",
            vec![
                &[0xe0, 0x07, 0x00, 0xa9],
                &[0xe0, 0x07, 0x40, 0xa9],
                &[0xe0, 0x03, 0x9f, 0xd6],
            ],
        ),
        (
            FreestandingTarget::Riscv64,
            "riscv64",
            "",
            vec![&[0x73, 0x00, 0x20, 0x30]],
        ),
    ] {
        let source = format!("extern \"C\" def service() -> None\n@machine.interrupt(\"{convention}\")\nexport \"C\" def irq({args}) -> None:\n    service()\n");
        let bytes = crate::with_freestanding_target(target, || object(&source, target));
        let file = object::File::parse(bytes.as_slice()).unwrap();
        let section = file.section_by_name(".text.irq").unwrap();
        let body = section.data().unwrap();
        let listing = self::instructions(target, body);
        for mnemonic in match convention {
            "x86_64_error" => &["pushq", "popq", "fxsave64", "fxrstor64", "iretq"][..],
            "aarch64" => &["stp", "ldp", "eret"],
            _ => &["fsd", "fld", "mret"],
        } {
            assert!(
                listing.contains(mnemonic),
                "{convention}: missing {mnemonic} in {listing}"
            );
        }
        for opcode in instructions {
            assert!(
                body.windows(opcode.len()).any(|word| word == opcode),
                "{convention}: missing {opcode:02x?} in {body:02x?}"
            );
        }
        assert!(
            section.relocations().count() > 0,
            "{convention} calls its handler body"
        );
    }
}

#[test]
fn machine_attributes_reject_conflicts_wrong_target_and_calls() {
    let target = FreestandingTarget::Aarch64;
    for (attribute, function, code) in [
        (
            "@machine.align(3)",
            "def irq() -> None:\n    pass",
            "AU2095",
        ),
        ("@machine.unknown", "def irq() -> None:\n    pass", "AU2095"),
        (
            "@machine.section(\".bad section\")",
            "def irq() -> None:\n    pass",
            "AU2095",
        ),
        (
            "@machine.align(8)\n@machine.align(16)",
            "def irq() -> None:\n    pass",
            "AU2095",
        ),
        (
            "@machine.always_inline\n@machine.noinline",
            "def irq() -> None:\n    pass",
            "AU2095",
        ),
        ("@machine.naked", "def irq() -> None:\n    pass", "AU2095"),
        (
            "@machine.interrupt(\"aarch64\")",
            "def irq(code: uint64) -> None:\n    pass",
            "AU2095",
        ),
        (
            "@machine.interrupt(\"x86_64_error\")",
            "def irq(code: uint64) -> None:\n    pass",
            "AU2096",
        ),
        (
            "@machine.interrupt(\"aarch64\")",
            "def irq() -> None:\n    pass\ndef entry() -> None:\n    irq()",
            "AU2096",
        ),
        (
            "@machine.interrupt(\"aarch64\")",
            "def irq() -> None:\n    pass\ndef entry() -> None:\n    value = irq",
            "AU2096",
        ),
    ] {
        let source = format!("{attribute}\n{function}\n");
        let diagnostic =
            crate::with_freestanding_target(target, || crate::check_source(&source)).unwrap_err();
        assert_eq!(diagnostic.code, code, "{source}: {diagnostic:?}");
    }
}

#[test]
fn machine_attributes_keep_a_private_unreferenced_routine() {
    let source = "@machine.used\n@machine.link_name(\"private_isr\")\n@machine.section(\".vectors\")\ndef extra() -> None:\n    pass\nexport \"C\" def entry() -> None:\n    pass\n";
    let target = FreestandingTarget::X86_64;
    let bytes = crate::with_freestanding_target(target, || object(source, target));
    let file = object::File::parse(bytes.as_slice()).unwrap();
    let symbol = file
        .symbols()
        .find(|s| s.name() == Ok("private_isr"))
        .unwrap();
    assert!(symbol.is_local());
    assert_eq!(
        file.section_by_index(symbol.section_index().unwrap())
            .unwrap()
            .name(),
        Ok(".vectors")
    );
}

#[test]
fn machine_attributes_place_module_data_with_scalar_layout() {
    let source = "@machine.section(\".board.config\")\n@machine.align(128)\nMAGIC: uint32 = 3735928559\nexport \"C\" def magic() -> uint32:\n    return MAGIC\n";
    let target = FreestandingTarget::Aarch64;
    let bytes = crate::with_freestanding_target(target, || object(source, target));
    let file = object::File::parse(bytes.as_slice()).unwrap();
    let section = file.section_by_name(".board.config").unwrap();
    assert_eq!(section.data().unwrap(), [0xef, 0xbe, 0xad, 0xde]);
    assert_eq!(section.align(), 128);
}

/// Decode the actual target's bytes, including compressed RV64GC instructions.
fn instructions(target: FreestandingTarget, section: &[u8]) -> String {
    use inkwell::llvm_sys::disassembler::*;
    use std::ffi::CStr;
    let (triple, features) = match target.arch() {
        "x86_64" => (c"x86_64-unknown-none", c""),
        "aarch64" => (c"aarch64-unknown-none", c"+neon"),
        "riscv64" => (c"riscv64-unknown-elf", c"+m,+a,+f,+d,+c"),
        other => panic!("unexpected target {other}"),
    };
    // SAFETY: the preceding build registered this disassembler. All
    // strings, source bytes and the output buffer live throughout each call.
    unsafe {
        let decoder = LLVMCreateDisasmCPUFeatures(
            triple.as_ptr(),
            c"".as_ptr(),
            features.as_ptr(),
            std::ptr::null_mut(),
            0,
            None,
            None,
        );
        assert!(!decoder.is_null());
        let mut bytes = section.to_vec();
        let mut at = 0usize;
        let mut result = String::new();
        while at < bytes.len() {
            let mut buf = [0; 256];
            let size = LLVMDisasmInstruction(
                decoder,
                bytes[at..].as_mut_ptr(),
                (bytes.len() - at) as u64,
                at as u64,
                buf.as_mut_ptr(),
                buf.len(),
            );
            assert!(size > 0, "undecodable instruction at {at} in {bytes:02x?}");
            let text = CStr::from_ptr(buf.as_ptr()).to_str().unwrap();
            result.push_str(text.trim());
            result.push('\n');
            at += size;
        }
        LLVMDisasmDispose(decoder);
        result
    }
}
