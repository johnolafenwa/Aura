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
    for (source, code) in [
        (
            include_str!("../tests/fixtures/machine-asm/bad-register.au"),
            "AU2092",
        ),
        (
            include_str!("../tests/fixtures/machine-asm/bad-type.au"),
            "AU2091",
        ),
        (
            include_str!("../tests/fixtures/machine-asm/bad-option.au"),
            "AU2093",
        ),
    ] {
        let error = check_module_with_builtin_imports(parse_source(source).unwrap()).unwrap_err();
        assert_eq!(error.code, code, "{error:?}");
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
