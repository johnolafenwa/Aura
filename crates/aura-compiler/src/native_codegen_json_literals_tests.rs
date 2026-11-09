use crate::lower_source_to_mir;
use crate::mir::{Instruction, MirFunction, Operand, Rvalue};

use super::{json_literal_nests, JsonNests, NestValue};

const SOURCE: &str = r#"
import json

def whole(i: int64, flag: bool, ratio: float64) -> json.Value:
    tags = [json.Value.String(f"t{i}"), json.Value.String("fixed")]
    return json.Value.Object(
        {
            "id": json.Value.Int(i),
            "seven": json.Value.Int(7),
            "flag": json.Value.Bool(flag),
            "ratio": json.Value.Float(ratio),
            "none": json.Value.Null,
            "tags": json.Value.Array(tags),
            "inner": json.Value.Object({"k": json.Value.Int(i + 1)}),
        }
    )

def repeated(i: int64) -> json.Value:
    return json.Value.Object({"a": json.Value.Int(i), "a": json.Value.Int(2)})

def keyed(key: own str) -> json.Value:
    return json.Value.Object({key: json.Value.Int(1)})

def holding(other: own json.Value) -> json.Value:
    return json.Value.Object(
        {"list": json.Value.Array([json.Value.Int(1)]), "other": other}
    )

def listed(items: own list[json.Value]) -> json.Value:
    return json.Value.Array(items)

def bump(n: mut int64):
    n += 1

def counted(counts: mut dict[str, int64], n: int64) -> json.Value:
    mut total = n
    bump(total)
    match mut counts.lookup("k"):
        case Lookup.Found(count):
            count += total
        case Lookup.Missing:
            counts["k"] = total
    return json.Value.Array([json.Value.Int(total), json.Value.Int(counts.len())])

def main():
    print(json.dumps(whole(1, true, 0.5)))
    print(json.dumps(repeated(1)))
    print(json.dumps(keyed("k")))
    print(json.dumps(holding(json.Value.Null)))
    print(json.dumps(listed([json.Value.Null])))
    mut counts: dict[str, int64] = {}
    print(json.dumps(counted(counts, 1)))
"#;

fn function(name: &str) -> MirFunction {
    lower_source_to_mir(SOURCE)
        .expect("the source should lower")
        .functions
        .into_iter()
        .find(|function| function.name == name)
        .expect("the function should exist")
}

fn nests(function: &MirFunction) -> JsonNests {
    json_literal_nests(function, |_, _| true)
}

/// The one literal `function` builds as a tape.
fn only_root(found: &JsonNests) -> &NestValue {
    let roots = found
        .roots
        .values()
        .flat_map(|roots| roots.values())
        .collect::<Vec<_>>();
    let [root] = roots[..] else {
        panic!("one literal should be built as a tape: {found:?}");
    };
    root
}

#[test]
fn a_literal_spelled_in_place_is_one_tape() {
    let function = function("whole");
    let found = nests(&function);
    let NestValue::Object(entries) = only_root(&found) else {
        panic!("the root should be an object: {found:?}");
    };
    let keys = entries
        .iter()
        .map(|(key, _)| key.as_str())
        .collect::<Vec<_>>();
    assert_eq!(
        keys,
        ["id", "seven", "flag", "ratio", "none", "tags", "inner"]
    );
    assert!(matches!(entries[0].1, NestValue::Int(Operand::Place(_))));
    assert!(matches!(entries[2].1, NestValue::Bool(_)));
    assert!(matches!(entries[3].1, NestValue::Float(_)));
    assert_eq!(entries[4].1, NestValue::Null);
    let NestValue::Array(tags) = &entries[5].1 else {
        panic!("the tags should be an array: {found:?}");
    };
    assert!(matches!(
        tags[..],
        [
            NestValue::String(Operand::MovePlace(_)),
            NestValue::String(_)
        ]
    ));
    assert!(matches!(&entries[6].1, NestValue::Object(inner) if inner.len() == 1));
    assert_eq!(only_root(&found).leaves().len(), 8);
    // Everything but the root is a part: the 10 values inside it, the
    // list of tags, and both dicts.
    let deferred = found.deferred.values().map(|at| at.len()).sum::<usize>();
    assert_eq!(deferred, 13, "{found:?}");
}

#[test]
fn a_literal_that_takes_a_value_made_elsewhere_is_built_as_before() {
    for name in ["repeated", "keyed", "listed"] {
        let found = nests(&function(name));
        assert!(found.roots.is_empty(), "{name}: {found:?}");
        assert!(found.deferred.is_empty(), "{name}: {found:?}");
    }
    // The array inside is still spelled whole.
    let found = nests(&function("holding"));
    assert!(matches!(only_root(&found), NestValue::Array(elements) if elements.len() == 1));
}

#[test]
fn a_literal_generated_code_cannot_build_is_built_as_before() {
    let found = json_literal_nests(&function("whole"), |_, _| false);
    assert!(found.roots.is_empty(), "{found:?}");
    assert!(found.deferred.is_empty(), "{found:?}");
}

#[test]
fn a_payload_that_may_change_before_the_root_keeps_its_variant() {
    // The first scalar payload names a local nothing assigns.
    let mut function = function("whole");
    let payload = function
        .blocks
        .iter_mut()
        .flat_map(|block| &mut block.instructions)
        .find_map(|instruction| match instruction {
            Instruction::Assign {
                value:
                    Rvalue::EnumVariant {
                        variant_name,
                        payloads,
                        ..
                    },
                ..
            } if variant_name == "Bool" => payloads.first_mut(),
            _ => None,
        })
        .expect("the literal has a `Bool`");
    *payload = Operand::Place("elsewhere.part".to_string());
    let found = nests(&function);
    // The tags and the inner object are still spelled whole.
    let roots = found
        .roots
        .values()
        .flat_map(|roots| roots.values())
        .count();
    assert_eq!(roots, 2, "{found:?}");
}

#[test]
fn a_literal_reads_what_calls_and_loans_before_it_wrote() {
    let found = nests(&function("counted"));
    let NestValue::Array(elements) = only_root(&found) else {
        panic!("the root should be an array: {found:?}");
    };
    assert!(matches!(
        elements[..],
        [NestValue::Int(_), NestValue::Int(_)]
    ));
}
