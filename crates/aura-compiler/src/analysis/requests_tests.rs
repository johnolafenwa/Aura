use super::*;
const SOURCE: &str = "type Doubler = Callable[def(value: int64) -> int64]\ndef double(value: int64) -> int64:\n    return value * 2\ndef main():\n    callback = Doubler(double)\n    result = callback(21)\n    print(result)\n";
fn path() -> &'static Path {
    Path::new("/tmp/aura-tooling.au")
}

#[test]
fn packed_signature_help_is_computed_by_the_compiler() {
    let help = signature_help_path_source(path(), SOURCE, 5, 23).expect("packed call signature");
    assert!(help.label.contains("value: int64"));
    assert_eq!(help.active_parameter, 0);
}

#[test]
fn references_and_rename_preserve_alias_and_result_bindings() {
    let references = references_path_source(path(), SOURCE, 4, 18, true);
    assert_eq!(references.len(), 2, "alias definition and constructor use");
    let renamed = rename_path_source(path(), SOURCE, 5, 7, "answer").expect("local rename");
    assert_eq!(renamed.edits.len(), 2);
    assert!(rename_path_source(path(), SOURCE, 5, 7, "callback").is_none());
    assert!(rename_path_source(path(), SOURCE, 5, 7, "return").is_none());
    assert!(rename_path_source(path(), SOURCE, 6, 6, "output").is_none());
}
#[test]
fn alias_definition_and_member_occurrences_are_editable() {
    let source="class Box:\n    value: int64\ntype Wrapped = Box\ndef main():\n    box = Wrapped(value=1)\n    print(box.value)\n";
    let refs = references_path_source(path(), source, 4, 12, true);
    assert_eq!(refs.len(), 2);
    assert_eq!(refs[0].start_character, 5);
    assert!(rename_path_source(path(), source, 2, 7, "Container").is_some());
    let refs = references_path_source(path(), source, 5, 15, true);
    assert!(refs.iter().any(|range| range.line == 1));
    assert!(prepare_rename_path_source(path(), source, 5, 15).is_some());
    assert!(rename_path_source(path(), source, 5, 15, "amount").is_some());
    let annotated = SOURCE.replace("callback =", "callback: Doubler =");
    assert_eq!(
        references_path_source(path(), &annotated, 0, 7, true).len(),
        3
    );
    assert!(rename_path_source(path(), &annotated, 0, 7, "Twice").is_some());
}

#[test]
fn signature_help_preserves_defaults_keyword_slots_and_incomplete_calls() {
    let source="def render(value: int64 = 1, *, prefix: str = \"x\") -> str:\n    return prefix\ndef main():\n    print(render(prefix=\"label\", value=2))\n";
    let help = signature_help_path_source(path(), source, 3, 28).expect("keyword signature");
    assert!(help.label.contains("= ..."));
    assert!(help.label.contains("*, prefix"));
    assert_eq!(help.active_parameter, 1);
    let incomplete = SOURCE
        .replace("callback(21)", "callback(")
        .replace("    print(result)\n", "");
    assert!(signature_help_path_source(path(), &incomplete, 5, 22).is_some());
}

#[test]
fn rename_rejects_imports_invalid_sources_and_binding_capture() {
    let source = "import math\ndef main():\n    value = math.sqrt(4.0)\n    print(value)\n";
    assert!(rename_path_source(path(), source, 2, 19, "root").is_none());
    assert!(prepare_rename_path_source(path(), source, 2, 19).is_none());
    assert!(rename_path_source(path(), SOURCE, 5, 7, "a b").is_none());
    assert!(references_path_source(path(), "def broken(\n", 0, 5, true).is_empty());
    assert!(signature_help_path_source(path(), SOURCE, 20, 0).is_none());
}

#[test]
fn editor_positions_use_utf16_and_preserve_multibyte_source() {
    let source = SOURCE.replace("print(result)", "print(\"😀\" + result.to_string())");
    let text = source.lines().nth(6).unwrap();
    let byte = text.find("result").unwrap();
    let position = text[..byte].encode_utf16().count();
    let edits = rename_path_source(path(), &source, 6, position, "answer")
        .expect("unicode rename")
        .edits;
    assert_eq!(edits[1].start_character, position);
    assert_eq!(edits[1].end_character, position + 6);
    assert!(references_path_source(path(), &source, 6, position, true)
        .iter()
        .any(|range| range.start_character == position));
    assert!(byte_character("😀", 0, 1).is_none());
    assert!(byte_character("x", 0, 3).is_none());
}

#[test]
fn signature_help_covers_task_bound_and_nested_call_results() {
    let source="type Job = TaskCallable[def(value: int64) -> int64]\ndef double(value: int64) -> int64:\n    return value * 2\ndef factory() -> Job:\n    return Job(double)\nclass Counter:\n    value: int64\n    def scaled(self, *, factor: int64 = 2) -> int64:\n        return self.value * factor\ndef main():\n    job = factory()\n    print(job(value=3))\n    print(factory()(value=4))\n    counter = Counter(value=2)\n    print(counter.scaled(factor=3))\n    scaled = counter.scaled\n    print(scaled(factor=3))\n";
    assert!(analyze_path_source(path(), source).diagnostics.is_empty());
    for (line, call, expected) in [
        (11, "job(", "value: int64"),
        (12, "factory()(", "value: int64"),
        (14, "counter.scaled(", "*, factor: int64 = ..."),
        (16, "scaled(", "*, factor: int64 = ..."),
    ] {
        let character = source.lines().nth(line).unwrap().find(call).unwrap() + call.len();
        let help = signature_help_path_source(path(), source, line, character)
            .expect("callable signature");
        assert!(help.label.contains(expected), "{}", help.label);
    }
}

#[test]
fn keyword_references_follow_the_compiler_selected_declaration() {
    let source="def chosen(value: int64) -> int64:\n    return value\ndef other(value: int64) -> int64:\n    return value * 2\ndef main():\n    chosen = other\n    print(chosen(value=2))\n";
    let refs = references_path_source(path(), source, 0, 12, true);
    assert_eq!(
        refs.len(),
        3,
        "the compiler binds this named call to the declared function: {refs:?}"
    );
    assert!(
        rename_path_source(path(), source, 0, 12, "input").is_none(),
        "refuse a rename when compiler binding checks cannot preserve the call"
    );
}
#[test]
fn references_and_rename_follow_narrowed_members_and_callable_results() {
    let source="class Box:\n    value: int64\ntype Factory = Callable[def() -> Box]\ndef make() -> Box:\n    return Box(value=1)\ndef show(item: Box | None):\n    match item:\n        case Box as box:\n            print(box.value)\n        case None:\n            pass\ndef main():\n    factory = Factory(make)\n    print(factory().value)\n    show(None)\n";
    let analysis = analyze_path_source(path(), source);
    assert!(
        analysis.diagnostics.is_empty(),
        "{:?}",
        analysis.diagnostics
    );
    assert_eq!(references_path_source(path(), source, 8, 23, true).len(), 4);
    assert_eq!(
        rename_path_source(path(), source, 13, 22, "amount")
            .expect("field rename")
            .edits
            .len(),
        4
    );
}

#[test]
fn signature_help_selects_the_next_unbound_keyword_slot() {
    let source="def render(value: int64 = 1, *, prefix: str = \"x\") -> str:\n    return prefix\ndef main():\n    print(render(prefix=\"label\", value=2))\n";
    let line = source.lines().nth(3).unwrap();
    let cursor = line.find(", value").unwrap() + 2;
    let help =
        signature_help_path_source(path(), source, 3, cursor).expect("between keyword arguments");
    assert_eq!(help.active_parameter, 0);
}
#[test]
fn alias_type_patterns_participate_in_references_and_rename() {
    let source="class Box:\n    value: int64\ntype Wrapped = Box\ndef show(item: Wrapped | None):\n    match item:\n        case Wrapped as box:\n            print(box.value)\n        case None:\n            pass\n";
    assert_eq!(references_path_source(path(), source, 2, 7, true).len(), 3);
    assert!(rename_path_source(path(), source, 2, 7, "Container").is_some());
}

#[test]
fn signature_help_uses_trait_contracts_for_concrete_and_bounded_receivers() {
    let source="trait Scale:\n    def scale(self, *, by: int64) -> int64\nclass Counter:\n    value: int64\nimpl Scale for Counter:\n    def scale(self, *, factor: int64) -> int64:\n        return self.value * factor\ndef apply[S: Scale](item: S) -> int64:\n    return item.scale(by=3)\ndef main():\n    counter = Counter(value=2)\n    print(counter.scale(factor=4))\n";
    let analysis = analyze_path_source(path(), source);
    assert!(
        analysis.diagnostics.is_empty(),
        "{:?}",
        analysis.diagnostics
    );
    assert_eq!(references_path_source(path(), source, 1, 23, true).len(), 2);
    assert!(rename_path_source(path(), source, 1, 23, "amount").is_some());
    for (line, expected) in [(8, "*, by: int64"), (11, "*, factor: int64")] {
        let character = source.lines().nth(line).unwrap().find("scale(").unwrap() + 6;
        let help = signature_help_path_source(path(), source, line, character)
            .expect("trait call signature");
        assert!(help.label.contains(expected));
    }
}
#[test]
fn implementation_bodies_and_specializations_keep_alias_occurrences() {
    let source="type Count = int64\ntrait Score:\n    def score(self) -> int64\nclass Counter:\n    value: int64\ndef identity[T](value: own T) -> T:\n    return value\nimpl Score for Counter:\n    def score(self) -> int64:\n        local: Count = self.value\n        return identity[Count](local)\ndef main():\n    print(Counter(value=2).score())\n";
    let refs = references_path_source(path(), source, 0, 7, true);
    assert_eq!(
        refs.len(),
        3,
        "alias declaration, implementation annotation and specialization: {refs:?}"
    );
    assert!(rename_path_source(path(), source, 0, 7, "Amount").is_some());
    assert_eq!(
        rename_path_source(path(), source, 9, 10, "result")
            .expect("implementation local rename")
            .edits
            .len(),
        2
    );
    let help =
        signature_help_path_source(path(), source, 10, 31).expect("implementation call signature");
    assert!(help.label.contains("int64"));
}

#[test]
fn unused_declarations_and_empty_editor_positions_have_precise_ranges() {
    let source = "class Spare:\n    value: int64\ndef unused() -> int64:\n    return 1\ndef main():\n    pass\n";
    let relative = Path::new("editor-unused.au");
    for (line, character, name) in [(0, 7, "Spare"), (1, 6, "value"), (2, 6, "unused")] {
        let refs = references_path_source(relative, source, line, character, true);
        assert_eq!(refs.len(), 1, "unused declaration {name}");
        let range = prepare_rename_path_source(relative, source, line, character).unwrap();
        let text = source.lines().nth(range.line).unwrap();
        assert_eq!(&text[range.start_character..range.end_character], name);
        let rename = rename_path_source(relative, source, line, character, "Renamed").unwrap();
        assert_eq!(rename.edits.len(), 1);
    }
    assert!(references_path_source(relative, source, 99, 0, true).is_empty());
    assert!(references_path_source(relative, source, 4, 0, true).is_empty());
    assert!(prepare_rename_path_source(relative, source, 4, 0).is_none());
    assert!(rename_path_source(relative, source, 4, 0, "name").is_none());
}

#[test]
fn rename_rejects_capture_even_when_the_rewritten_program_type_checks() {
    let source = "def original(value: int64) -> int64:\n    return value\ndef other(value: int64) -> int64:\n    return value + 1\ndef apply(callback: def(value: int64) -> int64) -> int64:\n    return original(1)\ndef main():\n    print(apply(other))\n";
    crate::check_source(source).unwrap();
    crate::check_source(&source.replace("original", "callback")).unwrap();
    assert!(rename_path_source(path(), source, 0, 6, "callback").is_none());
}

#[test]
fn signature_slots_ignore_quoted_commas_comments_and_completed_positional_arguments() {
    let source = r#"# ignored call(1, 2)
def render(text: str, value: int64, *, suffix: str = "!") -> str:
    return text + suffix
def main():
    print(render("quoted \", comma", 2, suffix="done"))
"#;
    crate::check_source(source).unwrap();
    let line = source.lines().nth(4).unwrap();
    let cursor = line.find("suffix=").unwrap();
    let help = signature_help_path_source(path(), source, 4, cursor).unwrap();
    assert_eq!(help.active_parameter, 2);
    assert_eq!(help.parameters[2], "*, suffix: str = ...");
    assert!(signature_help_path_source(path(), source, 0, 18).is_none());
    let open = "render(1, # comment (2, 3)";
    assert!(active_call(open, open.len()).is_none());
    let resumed = "render(1, # comment (2, 3)\n    ";
    let (_, ordinal, start) = active_call(resumed, resumed.len()).unwrap();
    assert_eq!(ordinal, 1);
    assert!(resumed[start..].starts_with(" # comment"));
}

#[test]
fn generic_method_signatures_agree_for_call_ast_and_editor_prefix() {
    let source = "class Box:\n    value: int64\n    def pick[T](self, item: own T) -> T:\n        return item\ndef pair[A, B](first: own A, second: own B) -> (A, B):\n    return (first, second)\ndef main():\n    box = Box(value=1)\n    print(box.pick[int64](2))\n    print(pair[int64, str](3, \"x\"))\n";
    let program = crate::check_source(source).unwrap();
    let builder = AnalysisBuilder::new(source, &program, Vec::new());
    let scope = builder.scope_for_position(8, 25);
    for (expression, expected) in [
        ("box.pick[int64](2)", "def(item: own int64) -> int64"),
        (
            "pair[int64, str](3, \"x\")",
            "def(first: own int64, second: own str) -> (int64, str)",
        ),
    ] {
        let call = parser::parse_expression(expression).unwrap();
        let ExprKind::Call { callee, .. } = call.kind else {
            panic!("call expression")
        };
        assert_eq!(
            call_site_contract(&builder, &callee, &scope)
                .unwrap()
                .to_string(),
            expected
        );
    }
    for (line, call, expected) in [
        (8, "box.pick[int64](", "item: own int64"),
        (9, "pair[int64, str](", "second: own str"),
    ] {
        let character = source.lines().nth(line).unwrap().find(call).unwrap() + call.len();
        let help = signature_help_path_source(path(), source, line, character).unwrap();
        assert!(help.label.contains(expected), "{}", help.label);
    }
}

#[test]
fn imported_declarations_keep_external_ranges_during_local_rename() {
    let unique = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let root = std::env::temp_dir().join(format!(
        "aura-editor-import-{}-{unique}",
        std::process::id()
    ));
    std::fs::create_dir_all(root.join("src")).unwrap();
    std::fs::write(
        root.join("Aura.toml"),
        "[package]\nname = \"editor_import\"\nversion = \"0.1.0\"\nedition = \"2026\"\n",
    )
    .unwrap();
    let helper = "public type Scalar = int64\npublic def twice(value: int64) -> int64:\n    return value * 2\npublic class Factory:\n    public def pick[T](value: own T) -> T:\n        return value\n";
    std::fs::write(root.join("src/helper.au"), helper).unwrap();
    let source = "import helper\ndef main():\n    answer: helper.Scalar = helper.twice(2)\n    print(answer)\n";
    let path = root.join("src/main.au");
    std::fs::write(&path, source).unwrap();
    let line = source.lines().nth(2).unwrap();
    let references = references_path_source(&path, source, 2, line.find("twice").unwrap(), true);
    assert_eq!(references.len(), 2, "{references:?}");
    let declaration = references
        .iter()
        .find(|range| range.file_path.is_some())
        .unwrap();
    assert_eq!(
        (
            declaration.line,
            declaration.start_character,
            declaration.end_character
        ),
        (1, 11, 16)
    );
    assert!(prepare_rename_path_source(&path, source, 2, line.find("twice").unwrap()).is_none());
    let renamed = rename_path_source(&path, source, 2, 6, "result").unwrap();
    assert_eq!(renamed.edits.len(), 2);
    assert_eq!(
        references_path_source(&path, source, 2, line.find("Scalar").unwrap(), true).len(),
        2
    );
    let specialized = source.replace("helper.twice(2)", "helper.Factory.pick[int64](value=2)");
    crate::check_path_with_source_without_lockfile(&path, &specialized).unwrap();
    let line = specialized.lines().nth(2).unwrap();
    let cursor = line.find("value=2").unwrap();
    let help = signature_help_path_source(&path, &specialized, 2, cursor).unwrap();
    assert!(help.label.contains("value: own int64"), "{}", help.label);
    assert_eq!(help.active_parameter, 0);
    let task_source = specialized + "    with group = TaskGroup():\n        task = group.start(helper.Factory.pick[int64], value=2)\n        print(task.result_or(0, timeout=1s))\n";
    std::fs::write(&path, &task_source).unwrap();
    let mir = crate::lower_path_to_mir(&path).unwrap();
    assert_eq!(crate::run_mir(&mir).unwrap().stdout, "2\n2\n");
    crate::emit_host_native_object(&mir).unwrap();
    std::fs::remove_dir_all(root).unwrap();
}
