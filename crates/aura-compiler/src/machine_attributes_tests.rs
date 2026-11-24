use super::*;

#[test]
fn machine_attributes_parse_a_vector_and_reject_hosted_functions() {
    crate::parse_source(include_str!(
        "../tests/fixtures/parse-pass/machine_attributes.au"
    ))
    .unwrap();
    let source = include_str!("../tests/fixtures/check-fail/machine_attributes_hosted.au");
    let error = crate::check_source(source).unwrap_err();
    assert_eq!(error.code, "AU2094");
    assert_eq!(
        error
            .render_with_source(
                "crates/aura-compiler/tests/fixtures/check-fail/machine_attributes_hosted.au",
                source
            )
            .trim_end(),
        include_str!("../tests/fixtures/check-fail/machine_attributes_hosted.diag").trim_end()
    );
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
    let source = "@machine.used\n@machine.link_name(\"private_isr\")\n@machine.section(\".vectors\")\ndef extra() -> None:\n    pass\n";
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

#[test]
fn naked_entry_has_no_generated_prologue_or_return() {
    let source = "@machine.naked\nexport \"C\" def identity(x: uint64) -> uint64:\n    asm(\"movq %rdi, %rax\\nretq\")\n";
    let target = FreestandingTarget::X86_64Kernel;
    let _mode = crate::enter_freestanding_target(target);
    let bytes = object(source, target);
    let file = object::File::parse(bytes.as_slice()).unwrap();
    let body = file
        .section_by_name(".text.identity")
        .unwrap()
        .data()
        .unwrap();
    assert_eq!(
        body,
        [0x48, 0x89, 0xf8, 0xc3],
        "{}",
        instructions(target, body)
    );
}

#[test]
fn kernel_interrupt_entry_never_touches_fp_state() {
    let source = "@machine.interrupt(\"x86_64\")\nexport \"C\" def irq() -> None:\n    pass\n";
    let target = FreestandingTarget::X86_64Kernel;
    let _mode = crate::enter_freestanding_target(target);
    let bytes = object(source, target);
    let file = object::File::parse(bytes.as_slice()).unwrap();
    let listing = instructions(
        target,
        file.section_by_name(".text.irq").unwrap().data().unwrap(),
    );
    assert!(listing.contains("iretq"));
    assert!(
        !["fxsave", "fxrstor", "xmm", "ymm"]
            .iter()
            .any(|op| listing.contains(op)),
        "{listing}"
    );
}

#[test]
fn explicit_inline_policy_overrides_small_leaf_heuristics() {
    let source = "extern \"C\" def notify() -> None\n@machine.noinline\ndef slow() -> None:\n    notify()\n@machine.always_inline\ndef fast() -> None:\n    slow()\nexport \"C\" def entry() -> None:\n    fast()\n";
    let target = FreestandingTarget::X86_64;
    let _mode = crate::enter_freestanding_target(target);
    let bytes = object(source, target);
    let file = object::File::parse(bytes.as_slice()).unwrap();
    assert!(file.symbol_by_name("aura_fn_slow").is_some());
    assert!(file.symbol_by_name("aura_fn_fast").is_none());
}

#[cfg(all(target_os = "linux", target_arch = "x86_64"))]
#[test]
fn used_symbol_survives_linker_garbage_collection() {
    let source = "@machine.used\n@machine.section(\".retained\")\nexport \"C\" def keep() -> None:\n    pass\nexport \"C\" def drop_me() -> None:\n    pass\nexport \"C\" def entry() -> None:\n    pass\n";
    let target = FreestandingTarget::X86_64;
    let _mode = crate::enter_freestanding_target(target);
    let bytes = object(source, target);
    let dir = std::env::temp_dir().join(format!(
        "aura-machine-link-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir(&dir).unwrap();
    let obj = dir.join("kernel.o");
    let elf = dir.join("kernel.elf");
    std::fs::write(&obj, bytes).unwrap();
    let output = std::process::Command::new("ld")
        .arg("--gc-sections")
        .args(["-e", "entry", "-o"])
        .arg(&elf)
        .arg(&obj)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let bytes = std::fs::read(elf).unwrap();
    std::fs::remove_dir_all(dir).unwrap();
    let file = object::File::parse(bytes.as_slice()).unwrap();
    assert!(file.symbol_by_name("keep").is_some());
    assert!(file.symbol_by_name("drop_me").is_none());
}

#[test]
fn data_initializers_encode_the_checked_scalar_width() {
    let source = "@machine.section(\".a\")\nA: int8 = -1\n@machine.align(2)\nB: uint16 = 513\n@machine.align(8)\nC: int64 = -258\n@machine.section(\".d\")\nD: bool = true\n@machine.section(\".e\")\nE: float32 = -0.5\n@machine.align(8)\nF: float64 = (2.5)\n";
    let program =
        crate::with_freestanding_target(FreestandingTarget::X86_64, || crate::check_source(source))
            .unwrap();
    let mir = crate::mir::lower_with_source_locations(&program);
    let values: Vec<&[u8]> = mir
        .foreign_link
        .placed_data
        .iter()
        .map(|d| d.bytes.as_slice())
        .collect();
    assert_eq!(
        values,
        [
            vec![255],
            513u16.to_le_bytes().to_vec(),
            (-258i64).to_le_bytes().to_vec(),
            vec![1],
            (-0.5f32).to_le_bytes().to_vec(),
            2.5f64.to_le_bytes().to_vec(),
        ]
    );
    for (source, target, code) in [
        ("@machine.align(64)\nN = 1", None, "AU2094"),
        (
            "@machine.align(64)\nN = 1 + 2",
            Some(FreestandingTarget::X86_64),
            "AU2095",
        ),
        (
            "@machine.link_name(\"wrong\")\nN = 1",
            Some(FreestandingTarget::X86_64),
            "AU2095",
        ),
        (
            "@machine.align(8)\n@machine.align(16)\nN = 1",
            Some(FreestandingTarget::X86_64),
            "AU2095",
        ),
    ] {
        let _mode = target.map(crate::enter_freestanding_target);
        let error = crate::check_source(source).unwrap_err();
        assert_eq!(error.code, code, "{error:?}");
    }
}

#[test]
fn machine_special_entries_reject_unrepresentable_contracts() {
    for source in [
        "class Entry:\n    @machine.noinline\n    def run(self) -> None:\n        pass",
        "@machine.noinline\n@wrapper\ndef f() -> None:\n    pass",
        "@machine.interrupt(\"arm\")\ndef f() -> None:\n    pass",
        "@machine.naked\ndef f() -> None:\n    asm(\"nop\", input(\"reg\", 0))",
        "@machine.naked\ndef f() -> None:\n    value()",
        "@machine.interrupt(\"x86_64\")\n@machine.naked\ndef f() -> None:\n    asm(\"iretq\")",
        "@machine.interrupt(\"x86_64\")\n@machine.always_inline\ndef f() -> None:\n    pass",
        "@machine.align(8)\ndef f[T](value: T) -> T:\n    return value",
        "@machine.interrupt(\"x86_64_error\")\ndef f(code: mut uint64) -> None:\n    pass",
        "@machine.interrupt(\"x86_64_error\")\ndef f(code: int64) -> None:\n    pass",
        "@machine.interrupt(\"x86_64_error\")\ndef f() -> None:\n    pass",
        "@machine.link_name(\"llvm.fake\")\ndef f() -> None:\n    pass",
        "@machine.section(section=\".text\")\ndef f() -> None:\n    pass",
    ] {
        let code = crate::with_freestanding_target(FreestandingTarget::X86_64, || {
            crate::parse_source(source).and_then(crate::check_module_with_builtin_imports)
        })
        .unwrap_err()
        .code;
        assert_eq!(code, "AU2095", "{source}");
    }
}

#[cfg(all(target_os = "linux", target_arch = "x86_64"))]
#[test]
fn aura_calls_a_naked_export_with_its_declared_c_abi() {
    let source = "@machine.naked\nexport \"C\" def update(value: float32, cell: mut float32) -> float32:\n    asm(\"addss (%rdi), %xmm0\\nmovss %xmm0, (%rdi)\\nretq\")\nexport \"C\" def probe() -> float32:\n    mut cell: float32 = 2.0\n    x = update(4.0, cell)\n    return x + cell\n";
    let target = FreestandingTarget::X86_64;
    let _mode = crate::enter_freestanding_target(target);
    let bytes = object(source, target);
    let dir = std::env::temp_dir().join(format!("aura-naked-abi-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("entry.o"), bytes).unwrap();
    std::fs::write(
        dir.join("test.c"),
        "extern float probe(void); int main(void) { return probe() == 12.0f ? 0 : 1; }",
    )
    .unwrap();
    let cc = std::process::Command::new("cc")
        .current_dir(&dir)
        .args(["test.c", "entry.o", "-o", "test"])
        .output()
        .unwrap();
    assert!(
        cc.status.success(),
        "{}",
        String::from_utf8_lossy(&cc.stderr)
    );
    let status = std::process::Command::new(dir.join("test"))
        .status()
        .unwrap();
    std::fs::remove_dir_all(dir).unwrap();
    assert!(status.success(), "{status}");
}

#[test]
fn imported_placement_and_entry_conventions_follow_the_defining_symbol() {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/machine-attributes/src/main.au");
    let target = FreestandingTarget::Aarch64;
    let _mode = crate::enter_freestanding_target(target);
    let mir = crate::lower_path_to_mir_with_source_locations(&path).unwrap();
    let source = std::fs::read_to_string(&path).unwrap();
    let bytes = emit_freestanding_object(&mir, path.to_str().unwrap(), &source, target).unwrap();
    let file = object::File::parse(bytes.as_slice()).unwrap();
    let irq = file.symbol_by_name("board_irq").unwrap();
    assert_eq!(
        file.section_by_index(irq.section_index().unwrap())
            .unwrap()
            .name(),
        Ok(".text.board_irq")
    );
    let invalid = source.replace("    pass", "    vectors.external_irq()");
    assert_eq!(
        crate::check_path_with_source(&path, &invalid)
            .unwrap_err()
            .code,
        "AU2096"
    );
}
