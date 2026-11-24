use super::super::*;
use crate::{current_build_config, with_build_config};

#[test]
fn conditional_reparse_keeps_declaration_module_storage() {
    let source = concat!(
        "import build\n",
        "if build.hosted:\n",
        "    mut ticks: uint64 = 3\n",
        "else:\n",
        "    mut ticks: uint64 = 7\n",
        "const width = 2\n",
    );
    let syntax = parse_all_syntax_with_module_scope(source, true).unwrap();
    assert_eq!(syntax.constants.len(), 3);
    let selected = parse_with_module_scope(source, true).unwrap();
    assert!(selected.top_level_stmts.is_empty());
    assert_eq!(selected.constants.len(), 2);
    assert!(selected.constants[0].mutable);
    assert!(!selected.constants[0].required);
    assert!(!selected.constants[1].mutable);
    assert!(selected.constants[1].required);
}

#[test]
fn guarded_entry_selects_storage_only_for_the_configuration_with_main() {
    let source = concat!(
        "import build\n",
        "mut ticks: int64 = 0\n",
        "if build.hosted:\n",
        "    def main():\n",
        "        pass\n",
    );
    let hosted = parse(source).unwrap();
    assert_eq!(hosted.constants.len(), 1);
    assert!(hosted.constants[0].mutable);
    let mut config = current_build_config();
    config.hosted = false;
    config.freestanding = true;
    with_build_config(config, || {
        let script = parse(source).unwrap();
        assert!(script.constants.is_empty());
        assert_eq!(script.top_level_stmts.len(), 1);
    });
}

#[test]
fn selected_bit_fields_use_the_target_abi_and_discard_inactive_widths() {
    let source = concat!(
        "import build\n",
        "extern \"C\" packed class Flags:\n",
        "    if build.os == \"windows\":\n",
        "        bits: uint16 bits(2)\n",
        "    else:\n",
        "        bits: uint16 bits(6)\n",
    );
    for (os, bits) in [("windows", 2), ("linux", 6)] {
        let mut config = current_build_config();
        config.os = os.into();
        with_build_config(config, || {
            let module = parse(source).unwrap();
            let Item::Class(flags) = &module.items[0] else {
                panic!("expected Flags")
            };
            assert!(flags.layout_controls.packed);
            assert_eq!(flags.fields.len(), 1);
            assert_eq!(flags.layout_controls.bit_widths["bits"], bits);
            assert_eq!(flags.layout_controls.microsoft_bit_fields, os == "windows");
        });
    }
}

#[test]
fn mutable_cells_cannot_select_a_declaration() {
    let source = concat!(
        "public mut enabled: bool = true\n",
        "if enabled:\n",
        "    def selected():\n",
        "        pass\n",
    );
    assert_eq!(
        parse_with_module_scope(source, true).unwrap_err().code,
        "AU2071"
    );
}

#[test]
fn a_binding_after_mutable_storage_can_select_the_entry_point() {
    let source = concat!(
        "import build\n",
        "mut ticks: uint64 = 0\n",
        "public const active = build.hosted\n",
        "if active:\n    def main():\n        pass\n",
    );
    parse_all_syntax(source).unwrap();
    let host = parse(source).unwrap();
    assert_eq!(host.constants.len(), 2);
    assert!(host.constants[0].mutable);
    assert!(host.constants[1].required);
    assert!(host.top_level_stmts.is_empty());
    let mut config = current_build_config();
    config.hosted = false;
    config.freestanding = true;
    with_build_config(config, || {
        assert!(parse(source).is_err());
        assert!(parse_with_module_scope(source, true).is_ok());
    });
}

#[test]
fn a_runtime_script_guard_can_still_contain_a_local_function_named_main() {
    let source = concat!(
        "import metrics\n",
        "mut n = 1\n",
        "if metrics.get(\"calls\") > 0:\n",
        "    def main():\n        pass\n",
    );
    let module = parse(source).unwrap();
    assert!(module.constants.is_empty());
    assert!(module.items.is_empty());
    assert_eq!(module.top_level_stmts.len(), 2);
}
