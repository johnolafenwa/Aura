use std::path::{Path, PathBuf};

use super::{format_source, verify, FormatError, Formatter, LINE_WIDTH};
use crate::lexer::lex;

fn format(source: &str) -> String {
    match format_source(source) {
        Ok(formatted) => formatted,
        Err(FormatError::Syntax(error)) => panic!("syntax error: {}", error.message),
        Err(FormatError::Unstable(message)) => panic!("{message}"),
    }
}

fn assert_formats(source: &str, expected: &str) {
    let formatted = format(source);
    assert_eq!(formatted, expected, "formatted output differs");
    assert_eq!(
        format(&formatted),
        formatted,
        "formatting is not idempotent"
    );
}

fn collect_sources(directory: &Path, extension: &str, found: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(directory) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect_sources(&path, extension, found);
        } else if path.extension().and_then(|value| value.to_str()) == Some(extension) {
            found.push(path);
        }
    }
}

fn repository_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

/// Every ```aura block in a Markdown page that is a complete program.
fn aura_blocks(markdown: &str) -> Vec<String> {
    let mut blocks = Vec::new();
    let mut current: Option<String> = None;
    for line in markdown.lines() {
        match current.as_mut() {
            Some(block) if line.trim_start().starts_with("```") => {
                blocks.push(std::mem::take(block));
                current = None;
            }
            Some(block) => {
                block.push_str(line);
                block.push('\n');
            }
            None if line.trim() == "```aura" => current = Some(String::new()),
            None => {}
        }
    }
    blocks
}

fn assert_stable(label: &str, source: &str) {
    let formatted = match format_source(source) {
        Ok(formatted) => formatted,
        Err(FormatError::Syntax(_)) => return,
        Err(FormatError::Unstable(message)) => panic!("{label}: {message}"),
    };
    let again = format_source(&formatted).unwrap_or_else(|_| panic!("{label}: reformat failed"));
    assert_eq!(again, formatted, "{label}: formatting is not idempotent");
}

#[test]
fn every_example_and_tutorial_program_formats_idempotently() {
    let root = repository_root();
    let mut sources = Vec::new();
    collect_sources(&root.join("examples"), "au", &mut sources);
    collect_sources(&root.join("tutorials"), "au", &mut sources);
    assert!(sources.len() > 50, "expected the example corpus");
    for path in &sources {
        let source = std::fs::read_to_string(path).unwrap();
        let label = path.display().to_string();
        let formatted =
            format_source(&source).unwrap_or_else(|_| panic!("{label}: example must format"));
        let again = format_source(&formatted).unwrap();
        assert_eq!(again, formatted, "{label}: formatting is not idempotent");
    }
    let mut pages = Vec::new();
    collect_sources(&root.join("tutorials"), "md", &mut pages);
    collect_sources(&root.join("docs/manual"), "md", &mut pages);
    for page in &pages {
        let markdown = std::fs::read_to_string(page).unwrap();
        for (position, block) in aura_blocks(&markdown).iter().enumerate() {
            assert_stable(&format!("{} block {position}", page.display()), block);
        }
    }
}

#[test]
fn every_compiler_fixture_that_parses_formats_idempotently() {
    let mut sources = Vec::new();
    collect_sources(
        &Path::new(env!("CARGO_MANIFEST_DIR")).join("tests"),
        "au",
        &mut sources,
    );
    assert!(sources.len() > 500, "expected the fixture corpus");
    for path in &sources {
        let source = std::fs::read_to_string(path).unwrap();
        assert_stable(&path.display().to_string(), &source);
    }
}

#[test]
fn indentation_becomes_four_spaces_per_level() {
    assert_formats(
        "def main() -> int32:\n  if true:\n        print(1)\n  return 0\n",
        "def main() -> int32:\n    if true:\n        print(1)\n    return 0\n",
    );
}

#[test]
fn spacing_follows_operators_commas_and_colons() {
    assert_formats(
        "def f(a:int32,b:int32=2)->int32:\n    x=a+b*-a\n    y = f( a , b = 3 )\n    z={ 'k' :x }\n    return x**2\n",
        "def f(a: int32, b: int32 = 2) -> int32:\n    x = a + b * -a\n    y = f(a, b=3)\n    z = {'k': x}\n    return x ** 2\n",
    );
}

#[test]
fn spacing_distinguishes_slices_type_parameters_and_keyword_names() {
    assert_formats(
        "trait Show:\n    def show(self) -> str\n\n\ndef first[T : Show](items: list[T]) -> list[T]:\n    print(items [1 : 2])\n    print(items[ : -1])\n    return items\n",
        "trait Show:\n    def show(self) -> str\n\n\ndef first[T: Show](items: list[T]) -> list[T]:\n    print(items[1:2])\n    print(items[:-1])\n    return items\n",
    );
    assert_formats(
        "trait Twice[T]:\n    def twice(self) -> T\n\n\nclass Box[T]:\n    value: T\n\n\nimpl [T] Twice[T] for Box[T]:\n    def twice(self) -> T:\n        return self.value\n",
        "trait Twice[T]:\n    def twice(self) -> T\n\n\nclass Box[T]:\n    value: T\n\n\nimpl[T] Twice[T] for Box[T]:\n    def twice(self) -> T:\n        return self.value\n",
    );
    assert_formats(
        "class Celsius:\n    value: int32\n\n    def from (value: int32) -> Celsius:\n        return Celsius(value = value)\n",
        "class Celsius:\n    value: int32\n\n    def from(value: int32) -> Celsius:\n        return Celsius(value=value)\n",
    );
}

#[test]
fn spacing_keeps_function_types_lambdas_and_captures_readable() {
    assert_formats(
        "def main() -> None:\n    factor: int32 = 2\n    scale: def (int32)->int32 = lambda [own factor] value : value*factor\n    print(scale (3) if true else 0)\n",
        "def main() -> None:\n    factor: int32 = 2\n    scale: def(int32) -> int32 = lambda [own factor] value: value * factor\n    print(scale(3) if true else 0)\n",
    );
}

#[test]
fn blank_lines_are_capped_and_declarations_get_two() {
    assert_formats(
        "\n\nimport math\nimport text\n\n\n\n\nx: int32 = 1\ndef helper() -> int32:\n\n    a = 1\n\n\n\n    return a\n# about main\ndef main() -> int32:\n    return helper()\n\n\n\n",
        "import math\nimport text\n\n\nx: int32 = 1\n\n\ndef helper() -> int32:\n    a = 1\n\n    return a\n\n\n# about main\ndef main() -> int32:\n    return helper()\n",
    );
}

#[test]
fn statements_after_a_declaration_are_separated_by_two_blank_lines() {
    assert_formats(
        "def helper() -> int32:\n    return 1\nprint(helper())\n# trailing note\n",
        "def helper() -> int32:\n    return 1\n\n\nprint(helper())\n# trailing note\n",
    );
}

#[test]
fn empty_and_comment_only_sources_format() {
    assert_formats("", "");
    assert_formats("\n\n   \n", "");
    assert_formats("# only a comment   \n\n\n", "# only a comment\n");
}

#[test]
fn a_line_that_fits_is_joined() {
    assert_formats(
        "def main() -> None:\n    total = max(\n        1,\n        2\n    )\n    print(\n        total\n    )\n",
        "def main() -> None:\n    total = max(1, 2)\n    print(total)\n",
    );
}

#[test]
fn a_long_line_breaks_its_outermost_bracket_first() {
    let source = "def main() -> None:\n    print(combine(first_value_name, second_value_name), combine(third_value_name, fourth_value))\n";
    assert_formats(
        source,
        "def main() -> None:\n    print(\n        combine(first_value_name, second_value_name),\n        combine(third_value_name, fourth_value)\n    )\n",
    );
}

#[test]
fn reflow_repeats_inward_until_lines_fit() {
    let source = "def main() -> None:\n    print(combine(first_value_name_that_is_long, second_value_name_that_is_long, third_value_name_long))\n";
    assert_formats(
        source,
        "def main() -> None:\n    print(\n        combine(\n            first_value_name_that_is_long,\n            second_value_name_that_is_long,\n            third_value_name_long\n        )\n    )\n",
    );
}

#[test]
fn a_signature_breaks_its_parameters_before_its_result_type() {
    let source = "def dispatch(request_value: Request, registry_value: dict[str, Tool]) -> Result[Answer, Error]:\n    pass\n";
    assert_formats(
        source,
        "def dispatch(\n    request_value: Request,\n    registry_value: dict[str, Tool]\n) -> Result[Answer, Error]:\n    pass\n",
    );
}

#[test]
fn only_a_signature_prefers_its_parameters_over_its_result_type() {
    let source = "def main() -> None:\n    handler: def(int64) -> None = lambda [mut profile] amount: profile.add_visits(amount + 1)\n";
    assert_formats(
        source,
        "def main() -> None:\n    handler: def(int64) -> None = lambda [mut profile] amount: profile.add_visits(\n        amount + 1\n    )\n",
    );
}

#[test]
fn a_line_without_brackets_stays_long() {
    let long_name = "a".repeat(LINE_WIDTH);
    let source = format!("{long_name} = 1\n");
    assert_formats(&source, &source);
}

#[test]
fn deep_nesting_breaks_level_by_level() {
    let mut expression = "value".to_string();
    for level in 0..30 {
        expression = format!("wrap{level}({expression}, {level})");
    }
    let source = format!("def main() -> None:\n    print({expression})\n");
    let formatted = format(&source);
    assert_eq!(format(&formatted), formatted);
    assert!(formatted.lines().count() > 30);
    for line in formatted.lines() {
        assert!(
            line.trim_start().len() <= LINE_WIDTH,
            "line too long: {line}"
        );
    }
}

#[test]
fn comments_stay_on_their_lines() {
    assert_formats(
        "# heading\nimport math   \n\n\ndef main() -> None:\n        # first comment in body\n        x = 1    # trailing\n        # before return\n        print(x)\n    # dedented comment\n",
        "# heading\nimport math\n\n\ndef main() -> None:\n    # first comment in body\n    x = 1  # trailing\n    # before return\n    print(x)\n    # dedented comment\n",
    );
}

#[test]
fn a_comment_inside_brackets_keeps_the_list_exploded() {
    assert_formats(
        "def main() -> None:\n    values = [1,   # one\n        # the second\n        2,\n        3]\n    print(max(1, 2  # pick\n    ))\n",
        "def main() -> None:\n    values = [\n        1,  # one\n        # the second\n        2,\n        3\n    ]\n    print(\n        max(\n            1,\n            2  # pick\n        )\n    )\n",
    );
    assert_formats(
        "def main() -> None:\n    print(len([  # empty\n    ]))\n",
        "def main() -> None:\n    print(\n        len(\n            [  # empty\n            ]\n        )\n    )\n",
    );
    assert_formats(
        "def main() -> None:\n    print(max(1,\n        2\n        # closing note\n    ))\n",
        "def main() -> None:\n    print(\n        max(\n            1,\n            2\n            # closing note\n        )\n    )\n",
    );
}

#[test]
fn a_comment_that_cannot_move_keeps_the_original_lines() {
    let source = "def main() -> None:\n  print(max(1, 2)  # after call\n    + 3)\n";
    assert_formats(
        source,
        "def main() -> None:\n    print(max(1, 2)  # after call\n      + 3)\n",
    );
}

#[test]
fn string_literals_are_copied_byte_for_byte() {
    let source = "def main() -> None:\n  text = \"\"\"line one   \n\tindented with a tab  \n    \"\"\"\n  print(text, 'single  quoted', r\"raw\\d\", f\"{text}  {1+2}\")\n";
    assert_formats(
        source,
        "def main() -> None:\n    text = \"\"\"line one   \n\tindented with a tab  \n    \"\"\"\n    print(text, 'single  quoted', r\"raw\\d\", f\"{text}  {1+2}\")\n",
    );
}

#[test]
fn docstrings_keep_their_bytes_when_the_body_is_reindented() {
    let source = "def main() -> None:\n  '''Docstring   \n     keeps this indentation\n  '''\n  print(\"\"\"a\n  b\"\"\", 1)\n";
    assert_formats(
        source,
        "def main() -> None:\n    '''Docstring   \n     keeps this indentation\n  '''\n    print(\"\"\"a\n  b\"\"\", 1)\n",
    );
}

#[test]
fn a_block_match_inside_brackets_keeps_its_layout() {
    let source = "def emit(value: int32) -> None:\n  print(value)\n\n\ndef main() -> None:\n  value: int32 = 1\n  emit(match value:\n      case 1: 10   \n      case _: 20)\n";
    assert_formats(
        source,
        "def emit(value: int32) -> None:\n    print(value)\n\n\ndef main() -> None:\n    value: int32 = 1\n    emit(match value:\n        case 1: 10\n        case _: 20)\n",
    );
}

#[test]
fn line_endings_and_byte_order_marks_are_normalized() {
    assert_formats(
        "\u{feff}x: int32 = 1\r\nprint(x)",
        "x: int32 = 1\nprint(x)\n",
    );
}

#[test]
fn numbers_and_durations_keep_their_spelling() {
    assert_formats(
        "def main() -> None:\n    print(0xFF, 1_000, 1.5e-3, 2E+2, 10ms, 3s, 2m)\n",
        "def main() -> None:\n    print(0xFF, 1_000, 1.5e-3, 2E+2, 10ms, 3s, 2m)\n",
    );
}

#[test]
fn invalid_source_is_a_syntax_error() {
    assert!(matches!(
        format_source("def main(:\n"),
        Err(FormatError::Syntax(_))
    ));
    assert!(matches!(
        format_source("x = \"open\n"),
        Err(FormatError::Syntax(_))
    ));
}

#[test]
fn the_token_guard_rejects_changed_output() {
    let original = lex("print(1)\n").unwrap();
    assert!(verify(&original, "print(1)\n").is_ok());
    for (output, expected) in [
        ("print(2)\n", "changes the token stream at formatted line 1"),
        (
            "print(1)\nprint(1)\n",
            "changes the token stream at formatted line 2",
        ),
        ("print(1\n", "does not lex"),
        ("print(1))\n", "does not lex"),
        ("print(1) 2\n", "does not parse"),
    ] {
        match verify(&original, output) {
            Err(FormatError::Unstable(message)) => {
                assert!(message.contains(expected), "{output:?}: {message}");
                assert!(message.contains("the source was left unchanged"));
            }
            other => panic!("{output:?}: expected a guard failure, got {other:?}"),
        }
    }
}

#[test]
fn the_token_guard_ignores_trailing_commas_except_one_element_tuples() {
    let original = lex("print([1, 2], (3, 4))\n").unwrap();
    assert!(
        verify(&original, "print([1, 2,], (3, 4,))\n").is_err_and(|error| {
            // The current parser rejects these commas, so the guard reports a
            // parse failure rather than a token difference.
            matches!(error, FormatError::Unstable(message) if message.contains("does not parse"))
        })
    );
    let tuple = lex("x = (1,)\n").unwrap();
    assert!(verify(&tuple, "x = (1)\n").is_err());
    assert!(verify(&tuple, "x = (\n    1,\n)\n").is_ok());
}

#[test]
fn a_magic_trailing_comma_keeps_a_list_exploded() {
    // The parser does not accept these trailing commas yet, so this drives
    // the layout engine directly.
    let source = "values = [1, 2,]\npair = (1,)\ncall(a, b=[3,],)\n";
    assert_eq!(
        Formatter::new(source).format(),
        "values = [\n    1,\n    2,\n]\npair = (1,)\ncall(\n    a,\n    b=[\n        3,\n    ],\n)\n"
    );
}

#[test]
fn trailing_tabs_and_spaces_are_removed_outside_strings() {
    assert_formats(
        "def main() -> None:\t \n    text = \"\"\"keep\t \n\"\"\"\t\n    print(text)\t\n",
        "def main() -> None:\n    text = \"\"\"keep\t \n\"\"\"\n    print(text)\n",
    );
}
