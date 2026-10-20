use super::*;

const SOURCE: &str = concat!(
    "\"\"\"Shapes and <units>.\n",
    "\n",
    "    Indented detail.\n",
    "\"\"\"\n",
    "public LIMIT: int64 = 3\n",
    "HIDDEN = 1\n",
    "public type Pair[T] = (T, T)\n",
    "type Private = int64\n",
    "public class Point[T: Shaped]:\n",
    "    \"\"\"A point.\"\"\"\n",
    "    public x: T\n",
    "    \"\"\"Horizontal.\n",
    "    Second line.\"\"\"\n",
    "    public y: int64 = 0\n",
    "    secret: int64 = 0\n",
    "    public def moved(self, *, by: int64 = 1) -> int64:\n",
    "        \"\"\"Return a moved coordinate.\n",
    "\n",
    "        Args:\n",
    "            by: Distance to move.\n",
    "        \"\"\"\n",
    "        return by\n",
    "    def hidden(self):\n",
    "        pass\n",
    "class Internal:\n",
    "    \"\"\"Not listed.\"\"\"\n",
    "    value: int64\n",
    "public enum Shape[T]:\n",
    "    \"\"\"A shape.\"\"\"\n",
    "    Circle(radius: float64)\n",
    "    Pair(int64, int64)\n",
    "    Empty\n",
    "public trait Shaped:\n    pass\npublic trait Named[T]: Shaped:\n",
    "    \"\"\"   \"\"\"\n",
    "    def name(self) -> str:\n",
    "        \"\"\"Return the name.\"\"\"\n",
    "public trait Plain:\n",
    "    def plain(mut self, value: own str) -> str\n",
    "public def area[T](shape: Shape[T]) -> float64:\n",
    "    \"\"\"Area.\n",
    "\n",
    "    Args:\n",
    "        shape: Only an argument list.\n",
    "    \"\"\"\n",
    "    return 0.0\n",
    "def private_function():\n",
    "    \"\"\"Not listed.\"\"\"\n",
);

fn module() -> ApiModule {
    let program = crate::check_source(SOURCE).expect("API source should check");
    api_module_from_program("geometry", &program)
}

#[test]
fn api_docs_list_only_public_declarations_with_normalized_docs() {
    let module = module();
    assert_eq!(module.schema_version, API_DOCS_SCHEMA_VERSION);
    assert_eq!(module.module, "geometry");
    assert_eq!(
        module.doc.as_deref(),
        Some("Shapes and <units>.\n\nIndented detail.")
    );
    let names = module
        .declarations
        .iter()
        .map(|entry| (entry.kind.as_str(), entry.signature.as_str()))
        .collect::<Vec<_>>();
    assert_eq!(
        names,
        [
            ("constant", "LIMIT: int64"),
            ("type", "type Pair[T] = (T, T)"),
            ("class", "class Point[T: Shaped]"),
            ("enum", "enum Shape[T]"),
            ("trait", "trait Shaped"),
            ("trait", "trait Named[T]: Shaped"),
            ("trait", "trait Plain"),
            ("function", "def area[T](shape: Shape[T]) -> float64"),
        ]
    );
    let point = &module.declarations[2];
    assert_eq!(point.doc.as_deref(), Some("A point."));
    assert_eq!(
        point
            .fields
            .iter()
            .map(|field| (field.signature.as_str(), field.doc.as_deref()))
            .collect::<Vec<_>>(),
        [
            ("x: T", Some("Horizontal.\nSecond line.")),
            ("y: int64 = ...", None),
        ]
    );
    assert_eq!(point.methods.len(), 1);
    let moved = &point.methods[0];
    assert_eq!(
        moved.signature,
        "def moved(self, *, by: int64 = ...) -> int64"
    );
    assert_eq!(
        moved.parameters[0].doc.as_deref(),
        Some("Distance to move.")
    );
    let shape = &module.declarations[3];
    assert_eq!(
        shape
            .variants
            .iter()
            .map(|variant| variant.signature.as_str())
            .collect::<Vec<_>>(),
        ["Circle(radius: float64)", "Pair(int64, int64)", "Empty"]
    );
    let named = &module.declarations[5];
    assert_eq!(named.doc, None, "whitespace-only docstrings have no text");
    assert_eq!(named.methods[0].doc.as_deref(), Some("Return the name."));
    assert_eq!(
        module.declarations[6].methods[0].signature,
        "def plain(mut self, value: own str) -> str"
    );
    assert_eq!(
        module.declarations[7].parameters[0].doc.as_deref(),
        Some("Only an argument list.")
    );
}

#[test]
fn api_docs_render_markdown_pages_and_json() {
    let markdown = render_api_markdown(&module());
    assert!(
        markdown.starts_with("# Module `geometry`\n\nShapes and &lt;units>.\n\nIndented detail.\n")
    );
    assert!(markdown
        .contains("\n## Class `Point`\n\n```aura\nclass Point[T: Shaped]\n```\n\nA point.\n"));
    assert!(markdown
        .contains("\nFields:\n\n- `x: T`: Horizontal.\n  Second line.\n- `y: int64 = ...`\n"));
    assert!(markdown.contains("\n### Method `moved`\n"));
    assert!(markdown.contains("\nParameters:\n\n- `by: int64 = ...`: Distance to move.\n"));
    assert!(markdown.contains("\nReturn a moved coordinate.\n\nParameters:\n"));
    assert!(!markdown.contains("Args:"));
    assert!(markdown.contains("\nVariants:\n\n- `Circle(radius: float64)`\n"));
    assert!(markdown.contains("\n## Constant `LIMIT`\n"));
    assert!(markdown.contains(
        "```aura\ndef area[T](shape: Shape[T]) -> float64\n```\n\nArea.\n\nParameters:\n"
    ));
    assert!(!markdown.contains("Internal"));
    assert!(!markdown.contains("private_function"));

    let json = serde_json::to_value(module()).expect("API docs serialize");
    assert_eq!(json["declarations"][2]["fields"][0]["name"], "x");
    assert!(json["declarations"][0].get("fields").is_none());

    let empty = crate::check_source("def main():\n    pass\n").expect("checks");
    let page = render_api_markdown(&api_module_from_program("empty", &empty));
    assert_eq!(
        page,
        "# Module `empty`\n\nThis module has no public declarations.\n"
    );
}

#[test]
fn api_docs_include_public_extern_declarations() {
    let root = std::env::temp_dir().join(format!("aura-api-docs-ffi-{}", std::process::id()));
    let source_dir = root.join("src");
    std::fs::create_dir_all(&source_dir).expect("temp package");
    std::fs::write(
        root.join("Aura.toml"),
        "[package]\nname = \"native\"\nversion = \"0.1.0\"\nedition = \"2026\"\nallow_ffi = true\n",
    )
    .expect("manifest");
    let path = source_dir.join("native.au");
    std::fs::write(
        &path,
        concat!(
            "public extern \"C\" opaque class Handle\n",
            "public extern \"C\" def acquire(flags: int32) -> Handle\n",
            "extern \"C\" def hidden() -> int32\n",
        ),
    )
    .expect("source");
    let program = crate::check_path(&path);
    let _ = std::fs::remove_dir_all(&root);
    let module = api_module_from_program("native", &program.expect("FFI package checks"));
    assert_eq!(
        module
            .declarations
            .iter()
            .map(|entry| (entry.kind.as_str(), entry.signature.as_str()))
            .collect::<Vec<_>>(),
        [
            ("extern class", "extern \"C\" opaque class Handle"),
            (
                "extern function",
                "extern \"C\" def acquire(flags: int32) -> Handle"
            ),
        ]
    );
}

#[test]
fn api_docs_list_a_decorated_function_with_its_decorators_and_docstring() {
    let source = concat!(
        "public type Step = Callable[def(n: int64) -> int64]\n",
        "public def traced(f: own Step) -> Step:\n",
        "    return f\n",
        "@traced\n",
        "# A comment inside the group.\n",
        "@traced\n",
        "public def fact(n: int64) -> int64:\n",
        "    \"\"\"Multiplies 1 to n.\"\"\"\n",
        "    return n\n",
        "@traced\n",
        "def hidden(n: int64) -> int64:\n",
        "    return n\n",
    );
    let program = crate::check_source(source).expect("decorated source should check");
    let module = api_module_from_program("tools", &program);
    let names = module
        .declarations
        .iter()
        .map(|entry| (entry.kind.as_str(), entry.name.as_str()))
        .collect::<Vec<_>>();
    assert_eq!(
        names,
        [
            ("type", "Step"),
            ("function", "traced"),
            ("function", "fact")
        ]
    );
    let fact = &module.declarations[2];
    assert_eq!(
        fact.signature,
        "@traced\n@traced\ndef fact(n: int64) -> int64"
    );
    assert_eq!(fact.doc.as_deref(), Some("Multiplies 1 to n."));
}
