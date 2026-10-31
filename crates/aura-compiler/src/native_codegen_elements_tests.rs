use crate::lower_source_to_mir;
use crate::mir::{CallTarget, Instruction, MirArg, MirFunction, Operand, Rvalue, Terminator};
use crate::runtime_ops::{ListOp, RuntimeOp, SetOp, StrOp};

use super::{lent_element_reads, lent_payload_reads, LentElements};

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

def count(items: list[Item]) -> int64:
    mut sum = 0
    indices = range(len(items))
    for i in indices:
        sum += len(items[i].name)
    return sum

def main():
    items = [Item(name = "a", weight = 1)]
    print(total(items))
    print(first(items))
    print(count(items))
    # A function value keeps `first` owning, and so releasing, its list.
    pick = first
    print(pick(items))
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
    lent_element_reads(function, |place| place.starts_with("%t"), |_| Vec::new())
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
fn a_range_loop_lends_each_element_too() {
    let count = function("count");
    assert!(count
        .blocks
        .iter()
        .any(|block| matches!(block.terminator, Terminator::ForRange { .. })));
    let found = lent(&count);
    let [(_, list)] = found.lists.iter().collect::<Vec<_>>()[..] else {
        panic!("one element read should be lent: {found:?}");
    };
    assert_eq!(list, "items");
    assert!(found.promotions.is_empty(), "{found:?}");
}

#[test]
fn a_released_list_first_gives_its_live_element_a_copy() {
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
    assert!(lent_element_reads(&first, |_| false, |_| Vec::new())
        .lists
        .is_empty());
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
fn a_list_another_place_can_change_copies_its_lent_element_or_never_lends() {
    // A mutable loan of the list names it when it begins, so the element
    // still to be read is copied there instead of the list never lending.
    let unchanged = lent_after(|_, _| {}).promotions.len();
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
    assert!(!found.lists.is_empty(), "{found:?}");
    assert_eq!(found.promotions.len(), unchanged + 1, "{found:?}");
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

fn arg(value: Operand) -> MirArg {
    MirArg {
        name: None,
        value,
        writeback_place: None,
    }
}

#[test]
fn reads_of_the_list_itself_leave_its_element_lent() {
    // `first` copies its element only before the list's release.
    assert_eq!(lent_after(|_, _| {}).promotions.len(), 1);
    let found = lent_after(|tail, _| {
        tail.insert(
            0,
            Instruction::IndexInRange {
                list: "items".to_string(),
                index: "%t_i".to_string(),
            },
        );
        tail.insert(
            1,
            runtime_call("%t_n", RuntimeOp::List(ListOp::Len), "items", Vec::new()),
        );
    });
    assert_eq!(found.promotions.len(), 1, "{found:?}");
    // A call that takes the list as an argument may keep it, so the element
    // is copied before it.
    let found = lent_after(|tail, _| {
        tail.insert(
            0,
            runtime_call(
                "%t_n",
                RuntimeOp::List(ListOp::Count),
                "items",
                vec![arg(Operand::Place("items".to_string()))],
            ),
        );
    });
    assert_eq!(found.promotions.len(), 2, "{found:?}");
    // A set read of a part of the element stores nothing back.
    let found = lent_after(|tail, temp| {
        tail.insert(
            0,
            runtime_call(
                "%t_has",
                RuntimeOp::Set(SetOp::Contains),
                &format!("{temp}.tags"),
                vec![arg(Operand::Int(1))],
            ),
        )
    });
    assert!(!found.lists.is_empty(), "{found:?}");
}

#[test]
fn native_code_lends_handles_and_copies_records_kept_in_place() {
    let source = format!(
        "{SOURCE}
class Point:
    x: int64
    y: int64

def sum_x(points: list[Point]) -> int64:
    mut total = 0
    for i in range(len(points)):
        point = points[i]
        total += point.x
    return total
"
    );
    let module = lower_source_to_mir(&source).expect("the source should lower");
    let object = crate::native_backend::without_optimization(|| {
        crate::native_codegen::emit_host_object(&module)
    })
    .expect("the source should compile");
    let lends = b"aura_direct_vec_lend";
    assert!(object.windows(lends.len()).any(|window| window == lends));
}

const FIELD_SOURCE: &str = r#"
class Book:
    levels: list[list[int64]]
    count: int64

    def top(mut self, p: int64) -> int64:
        return self.levels[p][0]

def main():
    mut book = Book(levels = [[1]], count = 0)
    print(book.top(0))
"#;

/// `Book.top` and the lent element read in it, with the list temporary
/// the immutable lend of `self.levels` fills.
fn field_read() -> (MirFunction, String, String) {
    let top = lower_source_to_mir(FIELD_SOURCE)
        .expect("the source should lower")
        .functions
        .into_iter()
        .find(|function| function.name == "Book.top")
        .expect("the method should exist");
    let found = lent(&top);
    let [(temp, list)] = found.lists.iter().collect::<Vec<_>>()[..] else {
        panic!("one element read should be lent: {found:?}");
    };
    assert!(found.promotions.is_empty(), "{found:?}");
    (top, temp.clone(), list.clone())
}

/// The promotions of `Book.top` once `extra` runs right after the lend of
/// `self.levels` ends.
fn field_read_promotions_after(extra: Instruction) -> Vec<(String, usize)> {
    let (mut top, _, list) = field_read();
    let (block, position) = top
        .blocks
        .iter()
        .enumerate()
        .find_map(|(block, instructions)| {
            instructions
                .instructions
                .iter()
                .position(|instruction| {
                    matches!(instruction, Instruction::EndLend { temp, .. } if *temp == list)
                })
                .map(|position| (block, position))
        })
        .expect("the lend should end");
    top.blocks[block].instructions.insert(position + 1, extra);
    let mut promotions = lent(&top).promotions.into_keys().collect::<Vec<_>>();
    promotions.sort();
    promotions
}

#[test]
fn an_element_of_a_lent_field_stays_lent_after_the_field_goes_back() {
    let (top, _, list) = field_read();
    assert!(top
        .blocks
        .iter()
        .any(|block| block.instructions.iter().any(|instruction| {
            matches!(
                instruction,
                Instruction::Lend { place, temp, .. } if place == "self.levels" && *temp == list
            )
        })));
    // Reading the root, lending a part apart from the list, and writing
    // another field change nothing the element depends on.
    for kept in [
        Instruction::Lend {
            place: "self.levels".to_string(),
            temp: "%t_levels".to_string(),
            mutable: false,
        },
        Instruction::Lend {
            place: "self.count".to_string(),
            temp: "%t_count".to_string(),
            mutable: true,
        },
        Instruction::Assign {
            target: "%t_read".to_string(),
            value: Rvalue::Use(Operand::Place("self.count".to_string())),
        },
        Instruction::Assign {
            target: "self.count".to_string(),
            value: Rvalue::Use(Operand::Place("self.count".to_string())),
        },
    ] {
        let shown = format!("{kept:?}");
        let promotions = field_read_promotions_after(kept);
        assert!(promotions.is_empty(), "{shown}: {promotions:?}");
    }
    // Anything that can change or free the list first copies the element.
    for change in [
        Instruction::Assign {
            target: "self".to_string(),
            value: Rvalue::Use(Operand::Place("%t2".to_string())),
        },
        Instruction::Assign {
            target: "self.levels".to_string(),
            value: Rvalue::Use(Operand::Place("%t2".to_string())),
        },
        Instruction::Assign {
            target: "%t_task".to_string(),
            value: Rvalue::Use(Operand::MovePlace("self.count".to_string())),
        },
        Instruction::Release {
            place: "self".to_string(),
        },
        Instruction::Lend {
            place: "self.levels".to_string(),
            temp: "%t_levels".to_string(),
            mutable: true,
        },
        Instruction::Assign {
            target: "%t_moved".to_string(),
            value: Rvalue::Use(Operand::MovePlace("self.levels".to_string())),
        },
    ] {
        assert_eq!(field_read_promotions_after(change).len(), 1);
    }
}

/// The position of the `EndLend` of `list` in `function`.
fn end_of_lend(function: &MirFunction, list: &str) -> (String, usize) {
    function
        .blocks
        .iter()
        .find_map(|block| {
            block
                .instructions
                .iter()
                .position(|instruction| {
                    matches!(instruction, Instruction::EndLend { temp, .. } if temp == list)
                })
                .map(|index| (block.label.clone(), index))
        })
        .expect("the lend should end")
}

#[test]
fn a_loan_of_the_lent_field_ends_the_element_loan_where_it_is_used() {
    let (mut top, _, list) = field_read();
    // A loan of another field changes nothing; one of the list's field
    // ends the loan where it is used, not where it begins.
    for (source, ends) in [("self.count", false), ("self.levels", true)] {
        let mut changed = top.clone();
        changed.blocks[0].instructions.insert(
            0,
            Instruction::BeginLoan {
                loan: "%t_loan".to_string(),
                source: source.to_string(),
                mutable: true,
            },
        );
        let (label, end) = end_of_lend(&changed, &list);
        changed.blocks[0].instructions.insert(
            end + 1,
            Instruction::EndLoan {
                loan: "%t_loan".to_string(),
            },
        );
        let found = lent(&changed);
        assert_eq!(
            found.promotions.keys().cloned().collect::<Vec<_>>(),
            if ends {
                vec![(label, end + 1)]
            } else {
                Vec::new()
            },
            "{source}"
        );
    }
    // A task that captures the root may reach the field at any time, so the
    // element is the list's only until the list goes back.
    let end = end_of_lend(&top, &list);
    top.blocks[0].instructions.insert(
        0,
        Instruction::Assign {
            target: "%t_closure".to_string(),
            value: Rvalue::StartTask {
                returns_handle: true,
                result_is_copy: true,
                stack_size: None,
                task_group: Operand::Int(0),
                function: Operand::Place("%t_function".to_string()),
                args: vec![MirArg {
                    name: None,
                    value: Operand::Place("self".to_string()),
                    writeback_place: None,
                }],
                span: crate::diag::Span::new(1, 1),
            },
        },
    );
    let found = lent(&top);
    assert!(
        found.promotions.contains_key(&(end.0, end.1 + 1)),
        "{found:?}"
    );
}

const PAYLOADS: &str = r#"
enum Op:
    Add
    Mul

enum Expr:
    Num(int64)
    Var(str)
    Binary(Op, Expr, Expr)

def eval(expr: Expr, names: dict[str, int64]) -> int64:
    match expr:
        case Expr.Num(value):
            return value
        case Expr.Var(name):
            return names[name]
        case Expr.Binary(op, left, right):
            a = eval(left, names)
            b = eval(right, names)
            match op:
                case Op.Add:
                    return a + b
                case Op.Mul:
                    return a * b

def sizes(exprs: list[Expr]) -> int64:
    mut sum = 0
    for expr in exprs:
        match expr:
            case Expr.Var(name):
                sum += name.len()
            case _:
                pass
    return sum

def through_a_value(expr: Expr) -> int64:
    run = sizes
    match expr:
        case Expr.Var(name):
            count = run([Expr.Num(1)])
            return count + name.len()
        case _:
            return 0

def copied(expr: Expr) -> int64:
    match expr:
        case Expr.Binary(op, _, _):
            other = op
            match other:
                case Op.Add:
                    return 1
                case Op.Mul:
                    return 2
        case _:
            return 0

def nested(expr: Expr) -> int64:
    match expr:
        case Expr.Binary(_, Expr.Var(name), _):
            return name.len()
        case _:
            return 0

def main():
    names = {"x": 3}
    expr = Expr.Binary(Op.Add, Expr.Var("x"), Expr.Num(2))
    print(eval(expr, names))
    print(sizes([Expr.Var("ab")]))
    print(through_a_value(Expr.Var("abc")))
    print(copied(expr))
    print(nested(expr))
"#;

/// The lent element and payload reads of `name` in [`PAYLOADS`], which
/// lend from `expr` and the temporaries.
fn lent_payloads(name: &str) -> (MirFunction, LentElements) {
    let module = lower_source_to_mir(PAYLOADS).expect("the source should lower");
    let borrowing = crate::mir::ownership::kept_arguments(&module);
    let function = module
        .functions
        .iter()
        .find(|function| function.name == name)
        .expect("the function should exist")
        .clone();
    let mut found = lent_element_reads(&function, |_| true, |_| Vec::new());
    lent_payload_reads(
        &function,
        &mut found,
        |_| true,
        |place| place == "expr" || place.starts_with("%t"),
        |instruction| {
            crate::mir::ownership::callee_borrowed_operands(instruction, |name| borrowing.get(name))
        },
    );
    (function, found)
}

/// The variant and payload index each payload read of `function` reads.
fn payload_reads(function: &MirFunction) -> Vec<(String, (String, usize))> {
    function
        .blocks
        .iter()
        .flat_map(|block| &block.instructions)
        .filter_map(|instruction| match instruction {
            Instruction::Assign {
                target,
                value:
                    Rvalue::VariantPayload {
                        variant_name,
                        index,
                        ..
                    },
            } => Some((target.clone(), (variant_name.clone(), *index))),
            _ => None,
        })
        .collect()
}

#[test]
fn payloads_only_read_where_they_lie_are_lent() {
    let (function, found) = lent_payloads("eval");
    let lent = payload_reads(&function)
        .into_iter()
        .filter(|(temp, _)| found.payloads.get(temp).map(String::as_str) == Some("expr"))
        .map(|(_, read)| read)
        .collect::<Vec<_>>();
    // The lookup reads `name`, the recursive calls borrow `left` and
    // `right`, and the match reads `op`.
    for (variant, index) in [("Var", 0), ("Binary", 0), ("Binary", 1), ("Binary", 2)] {
        assert!(
            lent.contains(&(variant.to_string(), index)),
            "{variant} payload {index} should be lent: {found:?}"
        );
    }
    assert!(found.promotions.is_empty(), "{found:?}");
}

#[test]
fn a_payload_of_a_lent_element_is_lent_from_it() {
    let (_, found) = lent_payloads("sizes");
    let [(element, list)] = found.lists.iter().collect::<Vec<_>>()[..] else {
        panic!("the loop should lend its element: {found:?}");
    };
    assert_eq!(list, "exprs");
    let [(_, source)] = found.payloads.iter().collect::<Vec<_>>()[..] else {
        panic!("the binding should be lent: {found:?}");
    };
    assert_eq!(source, element);
    assert!(found.promotions.is_empty(), "{found:?}");
}

#[test]
fn a_payload_takes_a_reference_before_its_scrutinee_is_handed_on() {
    let (function, found) = lent_payloads("through_a_value");
    let [(temp, source)] = found.payloads.iter().collect::<Vec<_>>()[..] else {
        panic!("the binding should be lent: {found:?}");
    };
    assert_eq!(source, "expr");
    // The call through a function value takes its own reference to the
    // list it builds, not to `expr`, so nothing makes `name` take one.
    assert!(found.promotions.is_empty(), "{found:?}");

    // A call that may keep `expr` makes the binding take its reference
    // before it.
    let mut changed = function.clone();
    let (block, position) = definition(&changed, temp);
    changed.blocks[block].instructions.insert(
        position + 1,
        Instruction::Release {
            place: "expr".to_string(),
        },
    );
    let mut found = lent_element_reads(&changed, |place| place.starts_with("%t"), |_| Vec::new());
    lent_payload_reads(
        &changed,
        &mut found,
        |_| true,
        |place| place == "expr",
        |_| Vec::new(),
    );
    assert_eq!(
        found
            .promotions
            .get(&(changed.blocks[block].label.clone(), position + 1)),
        Some(&vec![temp.clone()]),
        "{found:?}"
    );
}

#[test]
fn a_payload_of_a_lent_payload_is_lent_from_it() {
    let (function, found) = lent_payloads("nested");
    let reads = payload_reads(&function);
    let source_of = |read: (&str, usize)| {
        let (temp, _) = reads
            .iter()
            .find(|(_, (variant, index))| (variant.as_str(), *index) == read)
            .expect("the payload should be read");
        found.payloads.get(temp).cloned()
    };
    let (inner, _) = reads
        .iter()
        .find(|(_, (variant, _))| variant == "Binary")
        .expect("the inner expression should be read");
    assert_eq!(source_of(("Binary", 1)), Some("expr".to_string()));
    assert_eq!(source_of(("Var", 0)), Some(inner.clone()));
    assert!(found.promotions.is_empty(), "{found:?}");
}

#[test]
fn a_payload_copied_whole_is_not_lent() {
    let (function, found) = lent_payloads("copied");
    let reads = payload_reads(&function);
    // The pattern reads `op` alone and ignores the other payloads.
    let [(op, read)] = &reads[..] else {
        panic!("only `op` should be read: {reads:?}");
    };
    assert_eq!(read, &("Binary".to_string(), 0));
    assert!(!found.payloads.contains_key(op), "{found:?}");
}

#[test]
fn a_payload_whose_scrutinee_a_task_reaches_is_not_lent() {
    let (mut function, _) = lent_payloads("eval");
    function.blocks[0].instructions.insert(
        0,
        Instruction::Assign {
            target: "%t_task".to_string(),
            value: Rvalue::StartTask {
                returns_handle: true,
                result_is_copy: true,
                stack_size: None,
                task_group: Operand::Int(0),
                function: Operand::Place("%t_function".to_string()),
                args: vec![MirArg {
                    name: None,
                    value: Operand::Place("expr".to_string()),
                    writeback_place: None,
                }],
                span: crate::diag::Span::new(1, 1),
            },
        },
    );
    let mut found = LentElements::default();
    lent_payload_reads(&function, &mut found, |_| true, |_| true, |_| Vec::new());
    assert!(
        found.payloads.values().all(|source| source != "expr"),
        "{found:?}"
    );
}

#[test]
fn a_read_of_a_union_member_s_payload_is_lent_from_the_union() {
    let module = lower_source_to_mir(
        r#"def digits_of(reply: str) -> int64:
    match reply.strip_prefix("INT "):
        case str as digits:
            match parse_int64(digits):
                case Result.Ok(value):
                    return value + digits.len()
                case Result.Err(_):
                    return -1
        case None:
            return -2

def main():
    print(digits_of("INT 42"))
"#,
    )
    .expect("the source should lower");
    let borrowing = crate::mir::ownership::kept_arguments(&module);
    let lent = |name: &str| {
        let function = module
            .functions
            .iter()
            .find(|function| function.name == name)
            .expect("the function should exist");
        let mut found = lent_element_reads(function, |_| true, |_| Vec::new());
        lent_payload_reads(
            function,
            &mut found,
            |_| true,
            |place| place.starts_with("%t"),
            |instruction| {
                crate::mir::ownership::callee_borrowed_operands(instruction, |name| {
                    borrowing.get(name)
                })
            },
        );
        let reads = function
            .blocks
            .iter()
            .flat_map(|block| &block.instructions)
            .filter_map(|instruction| match instruction {
                Instruction::ReadLoan { target, .. } => Some(target.clone()),
                _ => None,
            })
            .collect::<Vec<_>>();
        (reads, found)
    };
    // `parse_int64` and `len` only read the string, so each read of the
    // loan holds the union's box.
    let (reads, found) = lent("digits_of");
    assert!(!reads.is_empty());
    for read in &reads {
        assert!(found.payloads.contains_key(read), "{read}: {found:?}");
    }
}
