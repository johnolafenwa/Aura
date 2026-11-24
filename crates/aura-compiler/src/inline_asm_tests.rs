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
    let section = file.section_by_index(symbol.section_index().unwrap()).unwrap();
    assert_eq!(&section.data().unwrap()[symbol.address() as usize..][..2], &[0x90, 0xc3]);
    assert!(file.symbol_by_name("copy_bits").is_some());
}

#[test]
fn refuses_invalid_assembly_before_llvm() {
    let _mode = enter_freestanding_target(FreestandingTarget::X86_64);
    for (source, code) in [
        (include_str!("../tests/fixtures/machine-asm/bad-register.au"), "AU2092"),
        (include_str!("../tests/fixtures/machine-asm/bad-type.au"), "AU2091"),
        (include_str!("../tests/fixtures/machine-asm/bad-option.au"), "AU2093"),
    ] {
        let error = check_module_with_builtin_imports(parse_source(source).unwrap()).unwrap_err();
        assert_eq!(error.code, code, "{error:?}");
    }
}
