use super::*;
use object::{Object, ObjectSection, ObjectSymbol};

const COPY: &str = include_str!("../tests/fixtures/machine-asm/copy.au");

#[test]
fn parses_and_formats_templates_verbatim() {
    let parsed = parse_source(COPY).unwrap();
    assert!(parsed.top_level_stmts.is_empty());
    let formatted = format::format_source(COPY).unwrap();
    assert!(formatted.contains("machine_noop:\n    nop\n    ret"));
}

#[test]
fn inline_asm_emits_instruction_and_module_definition() {
    let target = FreestandingTarget::X86_64;
    let _mode = enter_freestanding_target(target);
    let program = check_module_with_builtin_imports(parse_source(COPY).unwrap()).unwrap();
    let mir = mir::lower_with_source_locations(&program);
    let bytes = emit_freestanding_object(&mir, "entry.au", COPY, target).unwrap();
    let file = object::File::parse(bytes.as_slice()).unwrap();
    let symbol = file.symbol_by_name("machine_noop").unwrap();
    let section = file
        .section_by_index(symbol.section_index().unwrap())
        .unwrap();
    assert_eq!(
        &section.data().unwrap()[symbol.address() as usize..][..2],
        &[0x90, 0xc3]
    );
    assert!(file.symbol_by_name("copy_bits").is_some());
}

#[test]
fn refuses_invalid_assembly_before_llvm() {
    let _mode = enter_freestanding_target(FreestandingTarget::X86_64);
    for (name, source, expected) in [
        (
            "bad-register.au",
            include_str!("../tests/fixtures/machine-asm/bad-register.au"),
            include_str!("../tests/fixtures/machine-asm/bad-register.diag"),
        ),
        (
            "bad-type.au",
            include_str!("../tests/fixtures/machine-asm/bad-type.au"),
            include_str!("../tests/fixtures/machine-asm/bad-type.diag"),
        ),
        (
            "bad-option.au",
            include_str!("../tests/fixtures/machine-asm/bad-option.au"),
            include_str!("../tests/fixtures/machine-asm/bad-option.diag"),
        ),
    ] {
        let error = check_module_with_builtin_imports(parse_source(source).unwrap()).unwrap_err();
        assert_eq!(
            error.render_with_source(name, source).trim_end(),
            expected.trim_end()
        );
    }
}

fn checked(source: &str, target: FreestandingTarget) -> diag::Result<Program> {
    with_freestanding_target(target, || {
        check_module_with_builtin_imports(parse_source(source)?)
    })
}

fn object_for(source: &str, target: FreestandingTarget) -> Vec<u8> {
    let mir = mir::lower_with_source_locations(&checked(source, target).unwrap());
    emit_freestanding_object(&mir, "assembly.au", source, target).unwrap()
}

#[test]
fn assembly_registers_options_and_bindings_are_checked() {
    for (statement, code) in [
        ("asm()", "AU2093"),
        ("asm(1)", "AU2093"),
        ("asm(t='nop')", "AU2093"),
        ("asm('nop', 1)", "AU2093"),
        ("asm('nop', other('reg'))", "AU2093"),
        ("asm('nop', input('reg'))", "AU2093"),
        ("asm('nop', output('reg', 5))", "AU3035"),
        ("asm('nop', output('reg', a))", "AU3035"),
        ("asm('nop', output('reg', b), output('reg', b))", "AU3035"),
        ("asm('nop', options('x'))", "AU2093"),
        ("asm('nop', options('pure', 'pure'))", "AU2093"),
        (
            "asm('nop', options('noreturn'), output('reg', b))",
            "AU2093",
        ),
        (
            "asm('nop', options('pure'), clobber('memory'), output('reg', b))",
            "AU2093",
        ),
        ("asm('nop', clobber('memory', 'memory'))", "AU2092"),
        ("asm('nop', input('eax', a))", "AU2091"),
        ("asm('nop', input('rax', a), clobber('eax'))", "AU2092"),
        ("asm('nop $1', input('reg', a))", "AU2093"),
        ("asm('nop $')", "AU2093"),
    ] {
        let source = format!(
            "export \"C\" def entry(a: int64) -> None:\n    mut b: int64 = 0\n    {statement}\n"
        );
        assert_eq!(
            checked(&source, FreestandingTarget::X86_64)
                .unwrap_err()
                .code,
            code,
            "{statement}"
        );
    }
    for (source, code) in [
        ("module_asm(2)", "AU2093"),
        ("module_asm('nop', 'ret')", "AU2093"),
    ] {
        assert_eq!(parse_source(source).unwrap_err().code, code);
    }
    let parsed = parse_source(COPY).unwrap();
    assert!(parsed.uses_ffi());
    assert_eq!(
        check_module_with_builtin_imports(parsed).unwrap_err().code,
        "AU2090"
    );
}

#[test]
fn inline_assembly_returns_values_on_all_three_architectures() {
    for (arch, template, in_reg, out_reg) in [
        (FreestandingTarget::X86_64, "movq $1, $0", "r10", "r11"),
        (FreestandingTarget::Aarch64, "mov $0, $1", "x9", "x10"),
        (FreestandingTarget::Riscv64, "mv $0, $1", "a2", "x13"),
    ] {
        let source = format!("export \"C\" def entry(value: uint64) -> uint64:\n    mut out: uint64 = 0\n    asm(\"{template}\", input(\"{in_reg}\", value), output(\"{out_reg}\", out), clobber(\"memory\"))\n    return out\n");
        let bytes = object_for(&source, arch);
        let file = object::File::parse(bytes.as_slice()).unwrap();
        let sym = file.symbol_by_name("entry").unwrap();
        let section = file.section_by_index(sym.section_index().unwrap()).unwrap();
        assert!(section.size() != 0);
        for name in ["ra", "rsp", "x029", "x18", "x8", "s0", "x31", "w29", "bad"] {
            let bad = source.replace(out_reg, name);
            if !matches!(
                (arch, name),
                (FreestandingTarget::Riscv64, "x18" | "x31") | (FreestandingTarget::Aarch64, "x8")
            ) {
                assert!(checked(&bad, arch).is_err(), "{arch:?} {name}");
            }
        }
    }
}

#[test]
fn multiple_outputs_keep_narrow_signed_and_unsigned_types() {
    let source = r#"
export "C" def entry() -> int64:
    mut a: int16 = 0
    mut b: uint8 = 0
    asm("movw $$-2, $0\nmovb $$17, $1", output("ax", a), output("cl", b), options("pure", "preserves_flags"))
    return (a as int64) + (b as int64)
"#;
    let bytes = object_for(source, FreestandingTarget::X86_64);
    assert!(object::File::parse(bytes.as_slice())
        .unwrap()
        .symbol_by_name("entry")
        .is_some());
}

#[test]
fn module_assembly_can_supply_the_only_entry_and_assembler_errors_are_recoverable() {
    let source = "module_asm(\".text\\n.globl only_entry\\nonly_entry: ret\")\n";
    let bytes = object_for(source, FreestandingTarget::X86_64);
    assert!(object::File::parse(bytes.as_slice())
        .unwrap()
        .symbol_by_name("only_entry")
        .is_some());
    for source in [
        "module_asm(\".text\\nnot_an_instruction\")\n",
        "export \"C\" def entry() -> None:\n    asm(\"not_an_instruction\")\n",
    ] {
        let mir =
            mir::lower_with_source_locations(&checked(source, FreestandingTarget::X86_64).unwrap());
        let diag = emit_freestanding_object(&mir, "bad.au", source, FreestandingTarget::X86_64)
            .unwrap_err();
        assert!(diag[0].message.contains("not_an_instruction"), "{diag:?}");
        let wrong = emit_freestanding_object(&mir, "bad.au", source, FreestandingTarget::Aarch64)
            .unwrap_err();
        assert!(wrong[0].message.contains("target"), "{wrong:?}");
    }
}

#[test]
fn float_and_boolean_registers_keep_their_aura_types() {
    for (target, ty, op, input, output) in [
        (
            FreestandingTarget::X86_64,
            "float32",
            "movss $1, $0",
            "xmm1",
            "fpreg",
        ),
        (
            FreestandingTarget::X86_64,
            "float64",
            "movsd $1, $0",
            "fpreg",
            "xmm2",
        ),
        (
            FreestandingTarget::X86_64,
            "bool",
            "movb ${1:b}, ${0:b}",
            "reg",
            "al",
        ),
        (
            FreestandingTarget::Aarch64,
            "float32",
            "fmov ${0:s}, ${1:s}",
            "s1",
            "fpreg",
        ),
        (
            FreestandingTarget::Aarch64,
            "float64",
            "fmov ${0:d}, ${1:d}",
            "fpreg",
            "d2",
        ),
        (
            FreestandingTarget::Aarch64,
            "uint32",
            "mov ${0:w}, ${1:w}",
            "w1",
            "reg",
        ),
        (
            FreestandingTarget::Riscv64,
            "float32",
            "fmv.s $0, $1",
            "ft1",
            "fpreg",
        ),
        (
            FreestandingTarget::Riscv64,
            "float64",
            "fmv.d $0, $1",
            "fpreg",
            "f2",
        ),
    ] {
        let source = format!("export \"C\" def entry(value: {ty}) -> {ty}:\n    mut out = value\n    asm(\"{op}\", input(\"{input}\", value), output(\"{output}\", out))\n    return out\n");
        let bytes = object_for(&source, target);
        let listing = crate::tests::disassemble_asm_entry(&bytes);
        assert!(listing.contains(op.split_once(' ').unwrap().0), "{listing}");
        if target == FreestandingTarget::X86_64 && ty.starts_with("float") {
            assert!(checked(&source, FreestandingTarget::X86_64Kernel).is_err());
        }
    }
}

#[test]
fn assembly_options_distinguish_effects_and_terminate_control_flow() {
    let source = r#"
export "C" def entry(value: int8) -> int8:
    mut out: int8 = 0
    asm("movb $1, $0", input("reg", value), output("reg", out), options("pure"))
    asm("pause", options("preserves_flags"))
    asm("ud2", options("noreturn"))
"#;
    let bytes = object_for(source, FreestandingTarget::X86_64);
    let listing = crate::tests::disassemble_asm_entry(&bytes);
    assert!(
        listing.contains("pause") && listing.contains("ud2"),
        "{listing}"
    );
    assert!(
        !listing.contains("movb") && !listing.contains("retq"),
        "{listing}"
    );
    let file = object::File::parse(bytes.as_slice()).unwrap();
    assert!(file.symbol_by_name("aura_trap").is_none());
}

#[test]
fn module_assembly_accepts_declared_external_relocations() {
    let source = "extern \"C\" def finish() -> None\nmodule_asm(\".text\\n.globl entry\\nentry: jmp finish\")\n";
    let bytes = object_for(source, FreestandingTarget::X86_64);
    let file = object::File::parse(bytes.as_slice()).unwrap();
    assert!(file.symbol_by_name("finish").unwrap().is_undefined());
}

#[test]
fn rejects_mismatched_float_register_classes_aliases_and_modifiers() {
    for (target, clause, template, code) in [
        (
            FreestandingTarget::X86_64,
            "input('fpreg', i)",
            "nop",
            "AU2091",
        ),
        (
            FreestandingTarget::X86_64,
            "input('xmm16', f)",
            "nop",
            "AU2092",
        ),
        (
            FreestandingTarget::Aarch64,
            "input('w2', i)",
            "nop",
            "AU2091",
        ),
        (
            FreestandingTarget::Aarch64,
            "input('d2', f), clobber('s2')",
            "nop",
            "AU2092",
        ),
        (
            FreestandingTarget::Aarch64,
            "input('d2', f)",
            "${0:w}",
            "AU2093",
        ),
        (
            FreestandingTarget::Aarch64,
            "input('reg', i)",
            "${0:d}",
            "AU2093",
        ),
        (
            FreestandingTarget::Riscv64,
            "input('reg', i)",
            "${0:w}",
            "AU2093",
        ),
        (
            FreestandingTarget::X86_64Kernel,
            "clobber('xmm1')",
            "nop",
            "AU2092",
        ),
        (
            FreestandingTarget::X86_64,
            "input('reg', i)",
            "${n:w}",
            "AU2093",
        ),
        (
            FreestandingTarget::X86_64,
            "input('reg', i)",
            "${0:",
            "AU2093",
        ),
        (
            FreestandingTarget::X86_64,
            "input('reg', i)",
            "${0}",
            "AU2093",
        ),
    ] {
        let source = format!("export \"C\" def entry(i: int64, f: float64) -> None:\n    asm('{template}', {clause})\n");
        assert_eq!(checked(&source, target).unwrap_err().code, code, "{source}");
    }
}

#[test]
fn hosted_entry_points_reject_module_assembly_and_module_scope_requires_a_declaration() {
    let error = parse_source("asm('nop')\n").unwrap_err();
    assert_eq!(error.code, "AU2093");
    assert!(error.message.contains("module_asm"));
    let source = "module_asm('.text')\nexport \"C\" def entry() -> None:\n    pass\n";
    let mir =
        mir::lower_with_source_locations(&checked(source, FreestandingTarget::X86_64).unwrap());
    assert!(native_codegen::emit_host_object(&mir)
        .unwrap_err()
        .contains("freestanding"));
    assert_eq!(run_mir(&mir).unwrap_err().code, "AU2090");
}

#[test]
fn assembly_output_writes_through_mut_parameters_and_views() {
    let source = r#"
export "C" def entry(output_word: mut uint64) -> None:
    asm("movq $$7, $0", output("reg", output_word))
    view mut alias = output_word
    asm("movq $$9, $0", output("reg", alias))
"#;
    let mir =
        mir::lower_with_source_locations(&checked(source, FreestandingTarget::X86_64).unwrap());
    assert!(mir
        .functions
        .iter()
        .flat_map(|f| &f.blocks)
        .flat_map(|b| &b.instructions)
        .any(|i| matches!(i, mir::Instruction::WriteLoan { .. })));
    let bytes =
        emit_freestanding_object(&mir, "output.au", source, FreestandingTarget::X86_64).unwrap();
    assert!(crate::tests::disassemble_asm_entry(&bytes).contains("movq"));
}
