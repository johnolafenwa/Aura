use std::collections::HashSet;

use crate::lower_source_to_mir;
use crate::mir::MirFunction;

use super::super::element_loans::fused_entry_probes;
use super::{entry_key_loops, KeyLoop};

const SOURCE: &str = r#"
def show(value: list[int64]) -> int64:
    return len(value)

def walk(fields: dict[str, list[int64]]) -> int64:
    mut total = 0
    for key in fields.keys():
        match fields.lookup(key):
            case Lookup.Found(item):
                total += show(item)
            case Lookup.Missing:
                pass
    return total

def keep_keys(fields: dict[str, list[int64]]) -> list[str]:
    mut out: list[str] = []
    for key in fields.keys():
        match fields.lookup(key):
            case Lookup.Found(item):
                out.append(key.clone())
            case Lookup.Missing:
                pass
    return out

def change(fields: mut dict[str, list[int64]]) -> int64:
    mut total = 0
    for key in fields.keys():
        match fields.lookup(key):
            case Lookup.Found(item):
                total += show(item)
            case Lookup.Missing:
                pass
        fields["other"] = [1]
    return total

def main():
    mut fields = {"a": [1, 2], "b": [3]}
    print(walk(fields))
    print(len(keep_keys(fields)))
    print(change(fields))
"#;

fn function(name: &str) -> MirFunction {
    lower_source_to_mir(SOURCE)
        .expect("the source should lower")
        .functions
        .into_iter()
        .find(|function| function.name == name)
        .expect("the function should exist")
}

/// The loops of `name`, taking every fused test's loan as one only
/// borrowers read.
fn loops(name: &str) -> Vec<KeyLoop> {
    let function = function(name);
    let fused = fused_entry_probes(&function);
    let shadow = fused
        .values()
        .map(|probe| probe.loan.clone())
        .collect::<HashSet<_>>();
    entry_key_loops(&function, &fused, &shadow, |_| true)
        .into_values()
        .collect()
}

#[test]
fn a_keys_loop_that_looks_each_key_up_walks_the_entries() {
    let found = loops("walk");
    assert_eq!(found.len(), 1, "{found:?}");
    assert_eq!(found[0].dict, "fields");
    assert!(found[0].key.starts_with("%t"), "{found:?}");
}

#[test]
fn a_key_that_escapes_or_a_dict_that_changes_keeps_the_copies() {
    assert_eq!(loops("keep_keys"), Vec::new());
    assert_eq!(loops("change"), Vec::new());
}

#[test]
fn a_dict_no_fused_test_reads_in_place_keeps_the_copies() {
    let function = function("walk");
    let fused = fused_entry_probes(&function);
    // No loan only borrowers read.
    assert!(entry_key_loops(&function, &fused, &HashSet::new(), |_| true).is_empty());
    let shadow = fused
        .values()
        .map(|probe| probe.loan.clone())
        .collect::<HashSet<_>>();
    assert!(entry_key_loops(&function, &fused, &shadow, |_| false).is_empty());
}
