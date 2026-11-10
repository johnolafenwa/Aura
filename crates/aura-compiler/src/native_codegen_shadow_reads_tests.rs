use std::collections::HashSet;

use crate::lower_source_to_mir;
use crate::mir::{Instruction, MirFunction};

use super::super::element_loans::entry_copy_loans;
use super::{node_read_loans, payload_read, shadow_read_loans, NodePayload};

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

const NODE_SOURCE: &str = r#"
import json

def cloned_twice(fields: dict[str, json.Value], name: str) -> str:
    match fields.lookup(name):
        case Lookup.Found(value):
            match value:
                case json.Value.String(text):
                    first = text.clone()
                    return first + text.clone()
                case _:
                    return ""
        case Lookup.Missing:
            return ""

def dumped(fields: dict[str, json.Value], name: str) -> str:
    match fields.lookup(name):
        case Lookup.Found(value):
            match value:
                case json.Value.Object(inner):
                    return f"{inner.len()}"
                case _:
                    return ""
        case Lookup.Missing:
            return ""

def main():
    fields = {"a": json.Value.String("x")}
    print(cloned_twice(fields, "a"))
    print(dumped(fields, "a"))
"#;

fn node_reads(source: &str, name: &str) -> Vec<Vec<NodePayload>> {
    let function = lower_source_to_mir(source)
        .expect("the source should lower")
        .functions
        .into_iter()
        .find(|function| function.name == name)
        .expect("the function should exist");
    let copies = entry_copy_loans(&function);
    node_read_loans(&function, &copies, &HashSet::new())
        .into_values()
        .map(|read| {
            // Each payload local's one kind of use is the read it makes.
            for (payload, made) in &read.payloads {
                let reads = function
                    .blocks
                    .iter()
                    .flat_map(|block| &block.instructions)
                    .filter_map(|instruction| match instruction {
                        Instruction::Assign { value, .. } => payload_read(value),
                        _ => None,
                    })
                    .filter(|(read, _)| read == payload)
                    .collect::<Vec<_>>();
                assert!(!reads.is_empty(), "{payload}");
                assert!(reads.iter().all(|(_, read)| read == made), "{payload}");
            }
            read.payloads.into_values().collect::<Vec<_>>()
        })
        .collect()
}

#[test]
fn a_string_cloned_once_and_an_array_only_measured_read_their_node() {
    assert_eq!(node_reads(SOURCE, "cloned"), [[NodePayload::Text]]);
    assert_eq!(node_reads(SOURCE, "measured"), [[NodePayload::Len]]);
}

#[test]
fn a_payload_used_another_way_reads_no_node() {
    assert!(node_reads(SOURCE, "kept").is_empty());
    assert!(node_reads(NODE_SOURCE, "cloned_twice").is_empty());
    assert!(node_reads(NODE_SOURCE, "dumped").is_empty());
}

#[test]
fn only_a_clone_or_a_length_of_a_place_is_a_payload_read() {
    let function = function("kept");
    let reads = function
        .blocks
        .iter()
        .flat_map(|block| &block.instructions)
        .filter_map(|instruction| match instruction {
            Instruction::Assign { value, .. } => Some(payload_read(value).map(|(_, read)| read)),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(
        reads.iter().flatten().collect::<Vec<_>>(),
        [&NodePayload::Text]
    );
    assert!(reads.contains(&None));
}
