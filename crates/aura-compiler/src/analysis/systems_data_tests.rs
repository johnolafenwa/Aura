use super::*;
use crate::analysis::with_freestanding_analysis;

fn ffi_editor_package(source: &str) -> (TempDir, PathBuf) {
    let temp = TempDir::new("systems-data-editor");
    let file = temp.path().join("src/main.au");
    fs::create_dir_all(file.parent().unwrap()).unwrap();
    fs::write(
        temp.path().join("Aura.toml"),
        concat!(
            "[package]\n",
            "name = \"editor_example\"\n",
            "version = \"0.1.0\"\n",
            "edition = \"2026\"\n",
            "allow_ffi = true\n",
        ),
    )
    .unwrap();
    fs::write(&file, source).unwrap();
    (temp, file)
}

#[test]
fn systems_data_editor_completes_types_and_receiver_methods() {
    let program = checked_program("def main():\n    pass\n");
    let builder = AnalysisBuilder::new("", &program, Vec::new());
    let top_level = builder.top_level_completions();
    for name in ["fixed", "ptr"] {
        assert!(top_level
            .iter()
            .any(|item| item.name == name && item.kind == "type"));
    }
    for (ty, names) in [
        (
            Type::Named("ptr".into(), vec![Type::named("uint16")]),
            &["read", "write", "volatile_read", "volatile_write", "offset"][..],
        ),
        (
            Type::named("uint128"),
            &[
                "rotate_left",
                "rotate_right",
                "count_ones",
                "leading_zeros",
                "trailing_zeros",
                "swap_bytes",
            ][..],
        ),
    ] {
        let methods = builder.member_completions(&ty);
        for name in names {
            assert!(
                methods
                    .iter()
                    .any(|item| item.name == *name && item.kind == "method"),
                "missing {ty}.{name}"
            );
        }
    }
}

#[test]
fn systems_data_editor_profiles_restore_hosted_checking() {
    let source = "mut ticks: uint64 = 0\nexport \"C\" def count() -> uint64:\n    return ticks\n";
    let dir = TempDir::new("systems-data-editor-profile");
    let file = dir.path().join("kernel.au");
    fs::write(&file, source).unwrap();
    with_freestanding_analysis(true, || {
        let result = analyze_path_source(&file, source);
        assert!(result.diagnostics.is_empty(), "{:?}", result.diagnostics);
        let declaration = occurrence_at(&result, 0, 4);
        assert!(declaration.hover.contains("module binding ticks: uint64"));
        let binding = occurrence_at(&result, 2, 11);
        assert!(binding.hover.contains("module binding ticks: uint64"));
        assert_eq!(binding.definition.as_ref().unwrap().line, 0);
        assert_eq!(
            result
                .symbols
                .iter()
                .find(|s| s.name == "ticks")
                .unwrap()
                .kind,
            "variable"
        );
        with_freestanding_analysis(false, || {
            assert!(analyze_path_source(&file, source)
                .diagnostics
                .iter()
                .any(|d| d.code == "AU3030"));
        });
    });
    assert!(analyze_path_source(&file, source)
        .diagnostics
        .iter()
        .any(|d| d.code == "AU3030"));
}

#[test]
fn systems_data_editor_infers_fixed_elements_and_pointer_access() {
    let source = [
        "import mem",
        "def peek(data: fixed[uint16, 3], p: ptr[uint16]) -> uint16:",
        "    first = data[0]",
        "    for word in data:",
        "        print(word)",
        "    item = p.offset(1).read()",
        "    return first + item",
    ]
    .join("\n");
    let (_temp, file) = ffi_editor_package(&source);
    let analysis = analyze_path_source(&file, &source);
    assert!(
        analysis.diagnostics.is_empty(),
        "{:?}",
        analysis.diagnostics
    );
    for (line, start, expected) in [
        (2, 4, "binding first: uint16"),
        (4, 14, "local word: uint16"),
        (5, 4, "binding item: uint16"),
        (5, 23, "read() -> uint16"),
    ] {
        let hover = &occurrence_at(&analysis, line, start).hover;
        assert!(hover.contains(expected), "{hover}");
    }
}

#[test]
fn systems_data_editor_resolves_memory_queries_and_addressed_places() {
    let source = [
        "import mem as memory",
        "extern \"C\" packed class Header:",
        "    tag: uint16",
        "def get(header: mut Header) -> uint64:",
        "    p = memory.address_of(header.tag)",
        "    field_offset = memory.offset_of(Header, tag)",
        "    return (p.read() as uint64) + field_offset",
    ]
    .join("\n");
    let (_temp, file) = ffi_editor_package(&source);
    let analysis = analyze_path_source(&file, &source);
    assert!(
        analysis.diagnostics.is_empty(),
        "{:?}",
        analysis.diagnostics
    );
    let pointer = occurrence_at(&analysis, 4, 4);
    assert!(
        pointer.hover.contains("binding p: ptr[uint16]"),
        "{}",
        pointer.hover
    );
    let offset = occurrence_at(&analysis, 5, 4);
    assert!(
        offset.hover.contains("binding field_offset: uint64"),
        "{}",
        offset.hover
    );
    let field = occurrence_at(&analysis, 5, 44);
    assert_eq!(field.definition.as_ref().unwrap().line, 2);
    let completions = complete_path_source(&file, &source, 5, 26, Some('.')).unwrap();
    for name in ["size_of", "align_of", "offset_of", "address_of"] {
        assert!(completions.iter().any(|item| item.name == name), "{name}");
    }
}

#[test]
fn systems_data_editor_shows_declared_c_layout_in_field_order() {
    let source = [
        "extern \"C\" packed align(8) class Sample:",
        "    whole: uint32",
        "    flags: uint8 bits(3)",
        "    grid: array[array[int16, 3], 2]",
        "def first(s: Sample) -> uint32:",
        "    return s.whole",
    ]
    .join("\n");
    let (_temp, file) = ffi_editor_package(&source);
    let analysis = analyze_path_source(&file, &source);
    assert!(
        analysis.diagnostics.is_empty(),
        "{:?}",
        analysis.diagnostics
    );
    let sample = occurrence_at(&analysis, 4, 13);
    assert!(sample.hover.contains("extern \"C\" packed align(8) class Sample:\n    whole: uint32\n    flags: uint8 bits(3)\n    grid: array[array[int16, 3], 2]"), "{}", sample.hover);
}
