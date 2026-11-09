use crate::lower_source_to_mir;
use crate::mir::MirFunction;

use super::super::element_loans::{entry_copy_loans, fused_entry_probes};
use super::{object_view_candidates, ObjectView};

const SOURCE: &str = r#"
import json

def looked_up(doc: json.Value, name: str) -> int64:
    match doc:
        case json.Value.Object(fields):
            match fields.lookup(name):
                case Lookup.Found(value):
                    match json.as_int(value):
                        case int64 as number:
                            return number
                        case None:
                            return 0
                case Lookup.Missing:
                    return 0
        case _:
            return 0

def counted(doc: json.Value, name: str) -> int64:
    match doc:
        case json.Value.Object(fields):
            match fields.lookup(name):
                case Lookup.Found(value):
                    match json.as_int(value):
                        case int64 as number:
                            return number + fields.len()
                        case None:
                            return 0
                case Lookup.Missing:
                    return 0
        case _:
            return 0

def measured(doc: json.Value) -> int64:
    match doc:
        case json.Value.Object(fields):
            return fields.len()
        case _:
            return 0

def written(name: str) -> int64:
    mut doc = json.Value.Object({"a": json.Value.Int(1)})
    match mut doc:
        case json.Value.Object(fields):
            fields[name.clone()] = json.Value.Int(1)
            return fields.len()
        case _:
            return 0

def listed(doc: json.Value) -> int64:
    match doc:
        case json.Value.Array(items):
            return items.len()
        case _:
            return 0

def main():
    doc = json.Value.Object({"a": json.Value.Int(1)})
    print(looked_up(doc, "a"))
    print(counted(doc, "a"))
    print(measured(doc))
    print(written("a"))
    print(listed(doc))
"#;

fn function(name: &str) -> MirFunction {
    lower_source_to_mir(SOURCE)
        .expect("the source should lower")
        .functions
        .into_iter()
        .find(|function| function.name == name)
        .expect("the function should exist")
}

fn views(name: &str) -> Vec<ObjectView> {
    let function = function(name);
    let probes = fused_entry_probes(&function);
    let copies = entry_copy_loans(&function);
    object_view_candidates(&function, &probes, &copies)
        .into_values()
        .collect()
}

#[test]
fn an_object_payload_that_is_only_looked_up_is_a_view() {
    let views = views("looked_up");
    assert_eq!(views.len(), 1);
    assert_eq!(views[0].source, "doc");
    assert_eq!(views[0].loans.len(), 1);
}

#[test]
fn an_object_payload_something_else_reads_is_not_a_view() {
    assert_eq!(views("counted"), []);
}

#[test]
fn an_object_payload_without_a_lookup_is_not_a_view() {
    assert_eq!(views("measured"), []);
}

#[test]
fn an_object_payload_that_is_written_is_not_a_view() {
    assert_eq!(views("written"), []);
}

#[test]
fn another_variants_payload_is_not_a_view() {
    assert_eq!(views("listed"), []);
}
