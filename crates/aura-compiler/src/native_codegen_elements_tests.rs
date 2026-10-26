use crate::lower_source_to_mir;
use crate::mir::{CallTarget, Instruction, MirArg, MirFunction, Operand, Rvalue};
use crate::runtime_ops::{ListOp, RuntimeOp, StrOp};

use super::{lent_element_reads, LentElements};

const SOURCE: &str = r#"
class Item:
    name: str
    weight: int64

def total(items: list[Item]) -> int64:
    mut sum = 0
    for item in items:
        sum += item.weight + len(item.name)
    return sum

def first(items: list[Item]) -> int64:
    return len(items[0].name)

def main():
    items = [Item(name = "a", weight = 1)]
    print(total(items))
    print(first(items))
"#;

fn function(name: &str) -> MirFunction {
    lower_source_to_mir(SOURCE)
        .expect("the source should lower")
        .functions
        .into_iter()
        .find(|function| function.name == name)
        .expect("the function should exist")
}

fn lent(function: &MirFunction) -> LentElements {
    lent_element_reads(function, |place| place.starts_with("%t"))
}

/// The block and position of the read that assigns `temp`.
fn definition(function: &MirFunction, temp: &str) -> (usize, usize) {
    function
        .blocks
        .iter()
        .enumerate()
        .find_map(|(block, instructions)| {
            instructions
                .instructions
                .iter()
                .position(|instruction| {
                    matches!(instruction, Instruction::Assign { target, .. } if target == temp)
                })
                .map(|position| (block, position))
        })
        .expect("the temporary should be assigned")
}

#[test]
fn a_loop_lends_each_element_and_never_takes_a_reference() {
    let total = function("total");
    let found = lent(&total);
    let [(temp, list)] = found.lists.iter().collect::<Vec<_>>()[..] else {
        panic!("one element read should be lent: {found:?}");
    };
    assert!(temp.starts_with("%t"));
    assert_eq!(list, "items");
    assert!(found.promotions.is_empty(), "{found:?}");
}

#[test]
fn a_released_list_first_gives_its_live_element_a_reference() {
    let first = function("first");
    let found = lent(&first);
    let [(temp, _)] = found.lists.iter().collect::<Vec<_>>()[..] else {
        panic!("one element read should be lent: {found:?}");
    };
    let (block, position) = definition(&first, temp);
    let label = first.blocks[block].label.clone();
    let release = first.blocks[block].instructions[position..]
        .iter()
        .position(
            |instruction| matches!(instruction, Instruction::Release { place } if place == "items"),
        )
        .expect("the list should be released after the read")
        + position;
    assert_eq!(
        found.promotions.get(&(label, release)),
        Some(&vec![temp.clone()])
    );
    // Locals that may not hold a lent box are left alone.
    assert!(lent_element_reads(&first, |_| false).lists.is_empty());
}

/// `target = receiver.op()`, a runtime call that names its receiver as
/// the place to store it back to.
fn runtime_call(target: &str, op: RuntimeOp, receiver: &str, args: Vec<MirArg>) -> Instruction {
    Instruction::Assign {
        target: target.to_string(),
        value: Rvalue::Call {
            callee: CallTarget::Runtime {
                op,
                receiver: Some(Operand::Place(receiver.to_string())),
                receiver_place: Some(receiver.to_string()),
            },
            args,
            type_args: Default::default(),
        },
    }
}

/// The lent temporaries of `first` after `change` edits the block that
/// reads the element, after the read.
fn lent_after(change: impl FnOnce(&mut Vec<Instruction>, &str)) -> LentElements {
    let mut first = function("first");
    let found = lent(&first);
    let temp = found.lists.keys().next().expect("a lent read").clone();
    let (block, position) = definition(&first, &temp);
    let mut tail = first.blocks[block].instructions.split_off(position + 1);
    change(&mut tail, &temp);
    first.blocks[block].instructions.extend(tail);
    lent(&first)
}

#[test]
fn an_element_read_that_is_kept_or_written_is_copied() {
    assert!(!lent_after(|_, _| {}).lists.is_empty());
    for kept in [
        |temp: &str| Instruction::Retain {
            place: temp.to_string(),
        },
        |temp: &str| Instruction::Assign {
            target: format!("{temp}.weight"),
            value: Rvalue::Use(Operand::Int(1)),
        },
        |temp: &str| Instruction::Assign {
            target: "%t_copy".to_string(),
            value: Rvalue::Use(Operand::MovePlace(temp.to_string())),
        },
        |temp: &str| Instruction::Assign {
            target: "%t_copy".to_string(),
            value: Rvalue::Use(Operand::Place(temp.to_string())),
        },
        |temp: &str| Instruction::Lend {
            place: format!("{temp}.name"),
            temp: "%t_lent".to_string(),
            mutable: true,
        },
        |temp: &str| Instruction::BeginLoan {
            loan: "%t_loan".to_string(),
            source: temp.to_string(),
            mutable: false,
        },
        |temp: &str| {
            runtime_call(
                "%t_unit",
                RuntimeOp::List(ListOp::Push),
                &format!("{temp}.tags"),
                vec![MirArg {
                    name: None,
                    value: Operand::Int(1),
                    writeback_place: None,
                }],
            )
        },
        |temp: &str| {
            runtime_call(
                "%t_len",
                RuntimeOp::Str(StrOp::Len),
                "%t_other",
                vec![MirArg {
                    name: None,
                    value: Operand::Place(format!("{temp}.name")),
                    writeback_place: Some(format!("{temp}.name")),
                }],
            )
        },
    ] {
        let found = lent_after(|tail, temp| tail.insert(0, kept(temp)));
        assert!(found.lists.is_empty(), "{found:?}");
    }
}

#[test]
fn a_list_another_place_can_change_is_never_lent_from() {
    let found = lent_after(|tail, _| {
        tail.insert(
            0,
            Instruction::BeginLoan {
                loan: "%t_loan".to_string(),
                source: "items".to_string(),
                mutable: true,
            },
        )
    });
    assert!(found.lists.is_empty(), "{found:?}");
    // Reading a part of the element, even into a lend of it, keeps it lent.
    let found = lent_after(|tail, temp| {
        tail.insert(
            0,
            Instruction::Lend {
                place: format!("{temp}.name"),
                temp: "%t_lent".to_string(),
                mutable: false,
            },
        );
        tail.insert(
            1,
            Instruction::EndLend {
                place: format!("{temp}.name"),
                temp: "%t_lent".to_string(),
                mutable: false,
            },
        );
        tail.insert(
            2,
            Instruction::Retain {
                place: format!("{temp}.name"),
            },
        );
    });
    assert!(!found.lists.is_empty(), "{found:?}");
    // So does a read of a part's own elements, which stores nothing back.
    let found = lent_after(|tail, temp| {
        tail.insert(
            0,
            runtime_call(
                "%t_len",
                RuntimeOp::List(ListOp::Len),
                &format!("{temp}.tags"),
                Vec::new(),
            ),
        )
    });
    assert!(!found.lists.is_empty(), "{found:?}");
}
