use aura_core::types::Type;

use crate::lower_source_to_mir;
use crate::mir::{Instruction, MirFunction, Operand, Rvalue};

use super::{entry_loops, EntryLoop, EntryPart};

const SOURCE: &str = r#"
class Point:
    x: int64

def show(entry: (str, int64)) -> int64:
    return entry[1]

def items(counts: dict[str, int64]) -> int64:
    mut total = 0
    for name, count in counts.items():
        total += name.len() + count
    return total

def keys(counts: dict[str, int64]) -> int64:
    mut total = 0
    for name in counts.keys():
        total += name.len()
    return total

def values(counts: dict[str, int64]) -> int64:
    mut total = 0
    for count in counts.values():
        total += count
    return total

def changes(counts: mut dict[str, int64]):
    for name, count in counts.items():
        counts[name + "!"] = count

def two(counts: dict[int64, float64]) -> float64:
    mut total = 0.0
    for id, size in counts.items():
        for other in counts.values():
            total += size * other + (id as float64)
    return total

def whole(counts: dict[str, int64]) -> int64:
    mut total = 0
    for entry in counts.items():
        total += show(entry)
    return total

def kept(counts: dict[str, int64]) -> list[str]:
    names = counts.keys()
    return names

def classes(points: dict[str, Point]) -> int64:
    mut total = 0
    for name, point in points.items():
        total += point.x + name.len()
    return total

def class_values(points: dict[str, Point]) -> int64:
    mut total = 0
    for point in points.values():
        total += point.x
    return total

def class_keys(points: dict[str, Point]) -> int64:
    mut total = 0
    for name in points.keys():
        total += name.len()
    return total

def main():
    mut counts: dict[str, int64] = {"a": 1}
    print(items(counts) + keys(counts) + values(counts) + whole(counts))
    changes(counts)
    print(len(kept(counts)))
    print(two({1: 2.0}))
    points: dict[str, Point] = {"p": Point(x=1)}
    print(classes(points) + class_values(points) + class_keys(points))
"#;

fn function(name: &str) -> MirFunction {
    lower_source_to_mir(SOURCE)
        .expect("the source should lower")
        .functions
        .into_iter()
        .find(|function| function.name == name)
        .expect("the function should exist")
}

/// The element type of the list local `list` of `function`.
fn element_type(function: &MirFunction, list: &str) -> Option<Type> {
    function
        .local_types
        .iter()
        .find(|local| local.name == list)
        .and_then(|local| match &local.ty {
            Type::Named(name, args) if name == "list" && args.len() == 1 => Some(args[0].clone()),
            _ => None,
        })
}

fn loops_of(function: &MirFunction) -> Vec<EntryLoop> {
    let mut loops = entry_loops(function, |list| element_type(function, list))
        .into_values()
        .collect::<Vec<_>>();
    loops.sort_by_key(|entry_loop| entry_loop.part as u8);
    loops
}

fn parts(name: &str) -> Vec<EntryPart> {
    loops_of(&function(name))
        .into_iter()
        .map(|entry_loop| entry_loop.part)
        .collect()
}

#[test]
fn loops_over_plain_items_keys_and_values_walk_the_entries() {
    assert_eq!(parts("items"), [EntryPart::Both]);
    assert_eq!(parts("keys"), [EntryPart::Key]);
    assert_eq!(parts("values"), [EntryPart::Value]);
    // The walk reads a snapshot, so the body may change the dict.
    assert_eq!(parts("changes"), [EntryPart::Both]);
    assert_eq!(parts("two"), [EntryPart::Value, EntryPart::Both]);
}

#[test]
fn a_list_used_as_a_list_is_made() {
    // The tuple is passed on whole.
    assert_eq!(parts("whole"), []);
    // The list is a named local the function returns.
    assert_eq!(parts("kept"), []);
    assert_eq!(parts("main"), []);
}

#[test]
fn only_plain_keys_and_values_are_copied_out() {
    assert_eq!(parts("classes"), []);
    assert_eq!(parts("class_values"), []);
    assert_eq!(parts("class_keys"), [EntryPart::Key]);
}

/// The `items` loop of `items` after `change`, which is given the list
/// and the tuple read from it.
fn items_after(change: impl FnOnce(&mut MirFunction, &str, &str)) -> Vec<EntryLoop> {
    let mut function = function("items");
    let found = entry_loops(&function, |list| element_type(&function, list));
    let (list, entry_loop) = found.into_iter().next().expect("the items loop");
    change(&mut function, &list, &entry_loop.element);
    loops_of(&function)
}

fn push(function: &mut MirFunction, instruction: Instruction) {
    function.blocks[0].instructions.push(instruction);
}

#[test]
fn any_other_use_of_the_list_or_the_tuple_keeps_the_list() {
    assert_eq!(items_after(|_, _, _| {}).len(), 1);
    // A second release is still a release.
    let released = items_after(|function, list, tuple| {
        for place in [list, tuple] {
            push(
                function,
                Instruction::Release {
                    place: place.to_string(),
                },
            );
        }
    });
    assert_eq!(released.len(), 1);
    for whole in [true, false] {
        let copied = items_after(|function, list, tuple| {
            let source = if whole { list } else { tuple };
            push(
                function,
                Instruction::Assign {
                    target: "%copy".to_string(),
                    value: Rvalue::Use(Operand::Place(source.to_string())),
                },
            );
        });
        assert_eq!(
            copied,
            [],
            "a copy of the {}",
            if whole { "list" } else { "tuple" }
        );
    }
    // A second assignment of the list, and a list the block returns.
    let reassigned = items_after(|function, list, _| {
        let made = function
            .blocks
            .iter()
            .flat_map(|block| &block.instructions)
            .find(|instruction| {
                matches!(instruction, Instruction::Assign { target, .. } if target == list)
            })
            .cloned()
            .expect("the list is made");
        push(function, made);
    });
    assert_eq!(reassigned, []);
    let returned = items_after(|function, list, _| {
        function.blocks[0].terminator =
            crate::mir::Terminator::Return(Operand::Place(list.to_string()));
    });
    assert_eq!(returned, []);
    let tuple_returned = items_after(|function, _, tuple| {
        function.blocks[0].terminator =
            crate::mir::Terminator::Return(Operand::Place(tuple.to_string()));
    });
    assert_eq!(tuple_returned, []);
    // A second read of the list into another local.
    let read_twice = items_after(|function, list, tuple| {
        let read = function
            .blocks
            .iter()
            .flat_map(|block| &block.instructions)
            .find(|instruction| {
                matches!(instruction, Instruction::Assign { target, .. } if target == tuple)
            })
            .cloned()
            .expect("the tuple is read");
        let Instruction::Assign { value, .. } = read else {
            unreachable!("an assignment was found");
        };
        let _ = list;
        push(
            function,
            Instruction::Assign {
                target: "%again".to_string(),
                value,
            },
        );
    });
    assert_eq!(read_twice, []);
}

#[test]
fn a_list_without_a_known_element_type_is_made() {
    let function = function("items");
    assert!(entry_loops(&function, |_| None).is_empty());
    assert!(entry_loops(&function, |_| Some(Type::named("Unknown"))).is_empty());
}
