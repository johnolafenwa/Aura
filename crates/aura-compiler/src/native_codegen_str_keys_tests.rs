use crate::lower_source_to_mir;
use crate::mir::MirFunction;

use super::{str_key_tests, StrKeySource, StrKeyTest};

const SOURCE: &str = r#"
def in_dict(keys: list[str], table: dict[str, int64]) -> int64:
    mut found = 0
    for i in range(len(keys)):
        if keys[i] in table:
            found += 1
    return found

def in_set(keys: list[str], seen: set[str]) -> int64:
    mut found = 0
    for i in range(len(keys)):
        if keys[i] not in seen:
            found += 1
    return found

def viewed(keys: list[str], table: dict[str, int64]) -> int64:
    mut found = 0
    for i in range(len(keys)):
        view key = keys[i]
        if key in table:
            found += 1
    return found

def owned(keys: list[str], table: dict[str, int64]) -> int64:
    mut found = 0
    for i in range(len(keys)):
        key = keys[i].clone()
        if key in table:
            found += key.len()
    return found

def nested(keys: list[str], tables: list[dict[str, int64]]) -> int64:
    mut found = 0
    for i in range(len(keys)):
        if keys[i] in tables[0]:
            found += 1
    return found

def main():
    keys = ["a"]
    table: dict[str, int64] = {"a": 1}
    seen: set[str] = {"a"}
    print(in_dict(keys, table) + in_set(keys, seen) + viewed(keys, table))
    other: dict[str, int64] = {"b": 2}
    print(owned(keys, table) + nested(keys, [other]))
"#;

fn function<'m>(mir: &'m crate::mir::MirModule, name: &str) -> &'m MirFunction {
    mir.functions
        .iter()
        .find(|function| function.name == name)
        .expect("the function should lower")
}

fn only(function: &MirFunction) -> StrKeyTest {
    let tests = str_key_tests(function);
    assert_eq!(tests.len(), 1, "{}: {tests:?}", function.name);
    tests.into_values().next().expect("one test")
}

#[test]
fn a_test_on_an_indexed_element_reads_the_list() {
    let mir = lower_source_to_mir(SOURCE).expect("the source should lower");
    let test = only(function(&mir, "in_dict"));
    assert_eq!(test.receiver, "table");
    assert!(!test.set);
    assert!(matches!(
        &test.source,
        StrKeySource::Element { list, .. } if list == "keys"
    ));

    let test = only(function(&mir, "in_set"));
    assert_eq!(test.receiver, "seen");
    assert!(test.set);
}

#[test]
fn a_test_on_a_view_reads_its_loan() {
    let mir = lower_source_to_mir(SOURCE).expect("the source should lower");
    let test = only(function(&mir, "viewed"));
    assert!(matches!(&test.source, StrKeySource::Loan(_)));
    assert_eq!(test.receiver, "table");
}

#[test]
fn a_key_used_again_or_a_borrowed_receiver_keeps_the_copy() {
    let mir = lower_source_to_mir(SOURCE).expect("the source should lower");
    for name in ["owned", "nested", "main"] {
        let tests = str_key_tests(function(&mir, name));
        assert!(tests.is_empty(), "{name}: {tests:?}");
    }
}

#[test]
fn a_tested_key_is_also_an_element_slot_candidate() {
    // Both ways claim the temporary of `keys[i] in table`, so the function
    // compiler drops its element slot (see `retain_str_element_slots`) and
    // the test reads the key where the list holds it.
    let mir = lower_source_to_mir(SOURCE).expect("the source should lower");
    let in_dict = function(&mir, "in_dict");
    let tests = str_key_tests(in_dict);
    let slots = super::super::str_slots::element_slots(
        in_dict,
        |_| true,
        |_| Vec::new(),
        &std::collections::HashMap::new(),
    );
    let key = tests.keys().next().expect("one test");
    assert_eq!(slots.lists.get(key).map(String::as_str), Some("keys"));
}
