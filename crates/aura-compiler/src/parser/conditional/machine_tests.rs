use super::super::*;

#[test]
fn guarded_module_assembly_is_a_declaration_and_keeps_source_order() {
    let source = concat!(
        "import build\n",
        "module_asm(\"first\")\n",
        "if build.hosted:\n    module_asm(\"chosen\")\n",
        "else:\n    module_asm(\"discarded\")\n",
        "module_asm(\"last\")\n",
    );
    let syntax = parse_all_syntax(source).unwrap();
    assert_eq!(syntax.module_asm, ["first", "chosen", "discarded", "last"]);
    let selected = parse(source).unwrap();
    assert_eq!(selected.module_asm, ["first", "chosen", "last"]);
    assert!(selected.top_level_stmts.is_empty());
    assert_eq!(selected.asm_span.unwrap().line, 2);
    let invalid = "if dynamic:\n    module_asm('nop')\n";
    assert_eq!(parse(invalid).unwrap_err().code, "AU2071");
}

#[test]
fn inactive_assembly_requires_no_ffi_and_the_first_active_use_keeps_its_span() {
    let source = concat!(
        "import build\n",
        "if build.freestanding:\n    module_asm('nop')\n",
        "def main():\n",
        "    if build.freestanding:\n        asm('nop')\n",
        "    print(17)\n",
    );
    let module = parse(source).unwrap();
    assert!(module.asm_span.is_none());
    assert!(!module.uses_ffi());
    assert_eq!(crate::run_source(source).unwrap().stdout, "17\n");
    let active = format!("{source}    asm('nop')\n");
    assert_eq!(parse(&active).unwrap().asm_span.unwrap().line, 8);
    assert_eq!(crate::check_source(&active).unwrap_err().code, "AU2999");
}

#[test]
fn a_machine_placement_inside_a_selected_declaration_retains_its_constant_marker() {
    let source = concat!(
        "import build\n",
        "if build.hosted:\n",
        "    @machine.section('.selected')\n",
        "    const table: uint32 = 12\n",
        "else:\n",
        "    @machine.section('.other')\n",
        "    const table: uint32 = 15\n",
    );
    let module = parse(source).unwrap();
    assert_eq!(module.constants.len(), 1);
    let table = &module.constants[0];
    assert!(table.required);
    assert_eq!(
        table.placement.as_ref().unwrap().section.as_deref(),
        Some(".selected")
    );
}

#[test]
fn naked_functions_validate_the_one_assembly_statement_in_the_selected_body() {
    let source = concat!(
        "import build\n",
        "@machine.naked\n",
        "export \"C\" def entry() -> None:\n",
        "    if build.hosted:\n",
        "        if build.hosted:\n",
        "            asm('hosted')\n",
        "    else:\n        asm('bare')\n",
    );
    parse_all_syntax(source).unwrap();
    for hosted in [true, false] {
        let mut config = crate::current_build_config();
        config.hosted = hosted;
        crate::with_build_config(config, || {
            let module = parse(source).unwrap();
            let Item::Function(entry) = &module.items[0] else {
                panic!("expected entry")
            };
            assert_eq!(
                entry.machine.as_ref().unwrap().naked.as_deref(),
                Some(if hosted { "hosted" } else { "bare" })
            );
            assert_eq!(entry.body.len(), 1);
            assert_eq!(module.asm_span.unwrap().line, if hosted { 6 } else { 8 });
        });
    }
    let dynamic = source.replacen("if build.hosted:", "if dynamic:", 1);
    assert_eq!(parse(&dynamic).unwrap_err().code, "AU2095");
}
