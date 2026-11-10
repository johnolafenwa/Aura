use crate::lower_source_to_mir;

use super::host_borrowed_operand;

const SOURCE: &str = r#"
import json

def text(docs: dict[int64, json.Value], id: int64) -> str:
    return json.dumps(docs[id])

def absent(docs: dict[int64, json.Value], id: int64) -> bool:
    return json.is_null(docs[id])

def number(docs: dict[int64, json.Value], id: int64) -> int64 | None:
    return json.as_int(docs[id])

def main():
    docs = {1: json.Value.Int(1)}
    print(text(docs, 1))
    print(absent(docs, 1))
    match number(docs, 1):
        case int64 as found:
            print(found)
        case None:
            print("none")
"#;

/// How many instructions of `name` a host builtin borrows an argument in.
fn host_borrows(name: &str) -> usize {
    let module = lower_source_to_mir(SOURCE).expect("the source should lower");
    let function = module
        .functions
        .iter()
        .find(|function| function.name == name)
        .expect("the function should exist");
    function
        .blocks
        .iter()
        .flat_map(|block| &block.instructions)
        .filter(|instruction| host_borrowed_operand(instruction).is_some())
        .count()
}

#[test]
fn dumps_and_is_null_read_their_argument_where_it_lies() {
    assert_eq!(host_borrows("text"), 1);
    assert_eq!(host_borrows("absent"), 1);
}

#[test]
fn another_host_builtin_borrows_nothing() {
    assert_eq!(host_borrows("number"), 0);
}
