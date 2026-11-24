use crate::lower_source_to_mir;
use crate::mir::MirModule;
use crate::runtime_abi::RuntimeFn;
use crate::runtime_ops::{ListOp, RuntimeOp, StrOp, ValueOp};

use super::super::validated_for_emission;
use super::{plan_views, receiver_view_read, ViewParam, ViewRead};

const SOURCE: &str = r#"
def kind(text: str) -> int64:
    if text == "a":
        return 1
    if "b" != text:
        return 2
    return 3

def size(text: str) -> int64:
    return text.byte_len()

def same(left: str, right: str, count: int64) -> bool:
    return left == right and count > 0

def find(name: str, row: dict[str, str]) -> str:
    match row.get(name):
        case Lookup.Found(value):
            return value.clone()
        case Lookup.Missing:
            return name.to_upper()

def passes(text: str) -> int64:
    return kind(text)

def relays(text: str, row: dict[str, str]) -> str:
    return find(text, row)

def shouts(text: str) -> str:
    return text.to_upper()

def lonely(text: str) -> bool:
    return text == "alone"

def taken(text: own str) -> bool:
    return text == "taken"

def loops(text: str, count: int64) -> int64:
    if count == 0:
        return shouts(text).len()
    return loops(text, count - 1)

def main():
    words = ["a", "b"]
    row: dict[str, str] = {}
    local = words[0].to_upper()
    print(kind(words[0]))
    print(kind("b"))
    print(size(words[1]))
    print(same(words[0], "a", 1))
    print(find(words[0], row))
    print(passes("x"))
    print(relays(words[1], row))
    print(shouts(words[0]))
    print(lonely(local))
    print(taken("taken"))
    print(loops(words[0], 2))
"#;

fn module() -> MirModule {
    let mir = lower_source_to_mir(SOURCE).expect("the source should lower");
    validated_for_emission(&mir).expect("the source should validate")
}

fn view(position: usize, pure: bool) -> ViewParam {
    ViewParam { position, pure }
}

#[test]
fn a_function_that_reads_the_bytes_of_a_str_parameter_gets_a_view_twin() {
    let views = plan_views(&module());
    let mut twins = views.keys().map(String::as_str).collect::<Vec<_>>();
    twins.sort_unstable();
    assert_eq!(twins, ["find", "kind", "passes", "relays", "same", "size"]);
    assert_eq!(views["kind"], [view(0, true)]);
    assert_eq!(views["size"], [view(0, true)]);
    assert_eq!(views["same"], [view(0, true), view(1, true)]);
    assert_eq!(views["passes"], [view(0, true)]);
}

#[test]
fn a_parameter_some_path_loads_as_a_box_is_not_pure() {
    let views = plan_views(&module());
    assert_eq!(views["find"], [view(0, false)]);
    assert_eq!(views["relays"], [view(0, false)]);
}

#[test]
fn a_function_gets_no_view_twin_without_a_read_of_the_bytes_or_a_call_with_words() {
    let views = plan_views(&module());
    // `shouts` only passes its text to an operation that takes the box,
    // no call of `lonely` has its argument as words, `taken` owns its
    // parameter, and `loops` only passes its text on to `shouts`.
    for function in ["shouts", "lonely", "taken", "loops", "main"] {
        assert!(!views.contains_key(function), "`{function}` has no twin");
    }
}

#[test]
fn a_module_without_str_parameters_has_no_view_twins() {
    let mir = lower_source_to_mir("def main():\n    print(1)\n").expect("lowers");
    let module = validated_for_emission(&mir).expect("validates");
    assert!(plan_views(&module).is_empty());
}

#[test]
fn the_runtime_operations_that_read_a_str_receiver_from_its_bytes() {
    assert_eq!(
        receiver_view_read(RuntimeOp::Value(ValueOp::Clone)),
        Some(ViewRead::NewBox(RuntimeFn::StringFromView))
    );
    assert_eq!(
        receiver_view_read(RuntimeOp::Str(StrOp::ByteLen)),
        Some(ViewRead::ByteLen)
    );
    assert_eq!(receiver_view_read(RuntimeOp::Str(StrOp::ToUpper)), None);
    assert_eq!(receiver_view_read(RuntimeOp::List(ListOp::Len)), None);
}
