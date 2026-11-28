use crate::lower_source_to_mir;

use super::{split_loops, SplitLoop};

const SOURCE: &str = r#"
def count_lines(text: str) -> int64:
    mut total = 0
    for line in text.split("\n"):
        if line == "":
            continue
        fields = line.split(" ")
        total += fields[1].byte_len()
    return total

def words_of(text: str) -> list[str]:
    mut out: list[str] = []
    for word in text.split_whitespace():
        out.append(word.to_upper())
    return out

def lines_of(text: str) -> int64:
    mut n = 0
    for line in text.lines():
        for token in line.split_whitespace():
            n += token.byte_len() * 10 + 1
    return n

def owned_parts() -> int64:
    owned = "p,q,,r".clone()
    mut n = 0
    for part in owned.split(","):
        n += part.byte_len()
    return n

def main():
    print(count_lines("a bb\n\nccc dddd\nx y"))
    print(words_of("  hello  big world "))
    print(lines_of("one two\r\nthree\n\nfour five six\n"))
    print(owned_parts())
"#;

fn loops(source: &str, name: &str) -> Vec<SplitLoop> {
    let function = lower_source_to_mir(source)
        .expect("the source should lower")
        .functions
        .into_iter()
        .find(|function| function.name == name)
        .expect("the function should exist");
    split_loops(&function)
}

#[test]
fn a_loop_over_the_parts_of_a_text_walks_the_text() {
    let found = loops(SOURCE, "count_lines");
    assert_eq!(found.len(), 1, "{found:?}");
    assert_eq!(found[0].receiver, "text");
    assert_eq!(found[0].separator, "\n");
    assert!(found[0].receiver_release.is_none(), "{found:?}");
    assert_eq!(found[0].list_releases.len(), 1, "{found:?}");
    let found = loops(SOURCE, "words_of");
    assert_eq!(found.len(), 1, "{found:?}");
    let found = loops(SOURCE, "lines_of");
    assert_eq!(found.len(), 2, "{found:?}");
    let found = loops(SOURCE, "owned_parts");
    assert_eq!(found.len(), 1, "{found:?}");
    assert!(found[0].receiver_release.is_some(), "{found:?}");
}

const FIXTURE: &str = include_str!("../tests/fixtures/run-pass/loops_over_parts_walk_the_text.au");

#[test]
fn a_walk_holds_a_field_and_leaves_a_changed_text_alone() {
    for (name, held) in [("of_page", vec![true, true]), ("pages", vec![true])] {
        let found = loops(FIXTURE, name);
        let is_held: Vec<bool> = found.iter().map(|found| found.held).collect();
        assert_eq!(is_held, held, "{name}: {found:?}");
        assert!(
            found.iter().all(|found| found.receiver_release.is_none()),
            "{name}: {found:?}"
        );
    }
    for name in ["until_stop", "sums", "wide"] {
        let found = loops(FIXTURE, name);
        assert_eq!(found.len(), 1, "{name}: {found:?}");
        assert!(!found[0].held, "{name}: {found:?}");
    }
    assert!(loops(FIXTURE, "until_stop")[0].list_releases.len() > 1);
    for name in ["fresh_sums_task", "owned"] {
        let found = loops(FIXTURE, name);
        assert_eq!(found.len(), 1, "{name}: {found:?}");
        assert!(found[0].receiver_release.is_some(), "{name}: {found:?}");
    }
    assert_eq!(loops(FIXTURE, "page_sums_task").len(), 1);
    // A text or a field the loop changes, and a list read after the
    // loop, keep the list.
    for name in ["changed", "renamed", "kept"] {
        let found = loops(FIXTURE, name);
        assert!(found.is_empty(), "{name}: {found:?}");
    }
}
