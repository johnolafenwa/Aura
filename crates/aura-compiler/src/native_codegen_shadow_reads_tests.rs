use std::collections::HashSet;

use crate::lower_source_to_mir;
use crate::mir::MirFunction;

use super::super::element_loans::entry_copy_loans;
use super::shadow_read_loans;

const SOURCE: &str = r#"
import json

def cloned(fields: dict[str, json.Value], name: str) -> str:
    match fields.lookup(name):
        case Lookup.Found(value):
            match value:
                case json.Value.String(text):
                    return text.clone()
                case _:
                    return ""
        case Lookup.Missing:
            return ""

def kept(fields: dict[str, json.Value], name: str) -> str:
    match fields.lookup(name):
        case Lookup.Found(value):
            match value:
                case json.Value.String(text):
                    out = text.clone()
                    print(text)
                    return out
                case _:
                    return ""
        case Lookup.Missing:
            return ""

def measured(fields: dict[str, json.Value], name: str) -> int64:
    match fields.lookup(name):
        case Lookup.Found(value):
            match value:
                case json.Value.Array(items):
                    return items.len()
                case _:
                    return 0
        case Lookup.Missing:
            return 0

def main():
    fields = {"a": json.Value.String("x")}
    print(cloned(fields, "a"))
    print(kept(fields, "a"))
    print(measured(fields, "a"))
"#;

fn function(name: &str) -> MirFunction {
    lower_source_to_mir(SOURCE)
        .expect("the source should lower")
        .functions
        .into_iter()
        .find(|function| function.name == name)
        .expect("the function should exist")
}

fn shadow_reads(name: &str) -> usize {
    let function = function(name);
    let copies = entry_copy_loans(&function);
    shadow_read_loans(&function, &copies, &HashSet::new()).len()
}

#[test]
fn a_matched_read_whose_payload_is_only_cloned_reads_in_place() {
    assert_eq!(shadow_reads("cloned"), 1);
}

#[test]
fn a_payload_read_by_anything_else_keeps_its_copy() {
    assert_eq!(shadow_reads("kept"), 0);
}

#[test]
fn a_payload_a_method_reads_keeps_its_copy() {
    assert_eq!(shadow_reads("measured"), 0);
}
