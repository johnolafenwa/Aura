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
    lent_element_reads(
        function,
        |place| place.starts_with("%t"),
        |_| false,
        |_| Vec::new(),
    )
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
    assert!(
        lent_element_reads(&first, |_| false, |_| false, |_| Vec::new())
            .lists
            .is_empty()
    );
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
            place: temp.to_string(),
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
                recover: false,
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
    let mut found = lent_element_reads(&function, |_| true, |_| false, |_| Vec::new());
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
    let mut found = lent_element_reads(
        &changed,
        |place| place.starts_with("%t"),
        |_| false,
        |_| Vec::new(),
    );
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
                recover: false,
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
        let mut found = lent_element_reads(function, |_| true, |_| false, |_| Vec::new());
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

const VIEWS: &str = r#"
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

def each(docs: list[json.Value], name: str) -> int64:
    mut sum = 0
    for doc in docs:
        match doc:
            case json.Value.Object(fields):
                match fields.lookup(name):
                    case Lookup.Found(value):
                        match json.as_int(value):
                            case int64 as number:
                                sum += number
                            case None:
                                pass
                    case Lookup.Missing:
                        pass
            case _:
                pass
    return sum

def main():
    doc = json.Value.Object({"a": json.Value.Int(1)})
    print(looked_up(doc, "a"))
    print(each([json.Value.Object({"a": json.Value.Int(1)})], "a"))
"#;

fn view_function(name: &str) -> MirFunction {
    lower_source_to_mir(VIEWS)
        .expect("the source should lower")
        .functions
        .into_iter()
        .find(|function| function.name == name)
        .expect("the function should exist")
}

/// The lent reads of `function` with the object views it adopts.
fn adopted_views(function: &MirFunction) -> (LentElements, Vec<String>) {
    use super::super::element_loans::{entry_copy_loans, fused_entry_probes};
    use super::super::json_views::object_view_candidates;
    let mut found = lent_element_reads(
        function,
        |place| place.starts_with("%t"),
        |_| false,
        |_| Vec::new(),
    );
    let views = object_view_candidates(
        function,
        &fused_entry_probes(function),
        &entry_copy_loans(function, |_| false),
    );
    let adopted = super::adopt_object_views(function, &mut found, &views, |_| Vec::new());
    let mut adopted = adopted.into_iter().collect::<Vec<_>>();
    adopted.sort();
    (found, adopted)
}

#[test]
fn an_object_view_holds_its_scrutinee_s_box() {
    let function = view_function("looked_up");
    let (found, adopted) = adopted_views(&function);
    let [view] = &adopted[..] else {
        panic!("the payload should be a view: {found:?}");
    };
    assert_eq!(found.payloads.get(view).map(String::as_str), Some("doc"));
    assert!(found.promotions.is_empty(), "{found:?}");
}

#[test]
fn an_object_view_of_a_lent_element_reads_the_element_s_box() {
    let function = view_function("each");
    let (found, adopted) = adopted_views(&function);
    let [(element, list)] = found.lists.iter().collect::<Vec<_>>()[..] else {
        panic!("the loop should lend its element: {found:?}");
    };
    assert_eq!(list, "docs");
    let [view] = &adopted[..] else {
        panic!("the payload should be a view: {found:?}");
    };
    assert_eq!(found.payloads.get(view), Some(element));
    assert!(found.promotions.is_empty(), "{found:?}");
}

#[test]
fn a_scrutinee_released_while_its_view_is_held_has_no_view() {
    let function = view_function("looked_up");
    let (_, adopted) = adopted_views(&function);
    let (block, position) = definition(&function, &adopted[0]);
    let release = Instruction::Release {
        place: "doc".to_string(),
    };

    // A view cannot take a reference of its own, so it is not adopted.
    let mut held = function.clone();
    held.blocks[block]
        .instructions
        .insert(position + 1, release.clone());
    assert_eq!(adopted_views(&held).1, Vec::<String>::new());

    // Before the payload is read, nothing is held yet.
    let mut before = function.clone();
    before.blocks[block].instructions.insert(position, release);
    assert_eq!(adopted_views(&before).1, adopted);
}

#[test]
fn a_scrutinee_a_task_reaches_has_no_view() {
    let mut function = view_function("looked_up");
    function.blocks[0].instructions.insert(
        0,
        Instruction::Assign {
            target: "%t_task".to_string(),
            value: Rvalue::StartTask {
                recover: false,
                returns_handle: true,
                result_is_copy: true,
                stack_size: None,
                task_group: Operand::Int(0),
                function: Operand::Place("%t_function".to_string()),
                args: vec![MirArg {
                    name: None,
                    value: Operand::Place("doc".to_string()),
                    writeback_place: None,
                }],
                span: crate::diag::Span::new(1, 1),
            },
        },
    );
    assert_eq!(adopted_views(&function).1, Vec::<String>::new());
}

const COPIES: &str = r#"
enum Shape:
    Circle(float64)
    Rect(float64, float64)

class Node:
    tags: list[int64]
    children: list[int64]

copy class Point:
    x: int64
    y: int64

    def norm(self) -> int64:
        return self.x * self.x + self.y

    def twice(self) -> int64:
        return self.norm() * 2

    def itself(self) -> Point:
        return self

class Holder:
    tags: list[int64]

    def probe(self) -> int64:
        return count(self.tags)

    def mixed(self) -> int64:
        return weigh(self.tags, self)

def count(tags: list[int64]) -> int64:
    return tags.len()

def weigh(tags: list[int64], holder: Holder) -> int64:
    return tags.len() + holder.tags.len()

def area(shape: Shape) -> float64:
    match shape:
        case Shape.Circle(r):
            return r * r
        case Shape.Rect(w, h):
            return w * h

def doubled(shape: Shape) -> float64:
    return area(shape) * 2.0

def same(a: Shape, b: Shape) -> bool:
    return a == b

def chosen(shape: Shape) -> Shape:
    return shape

def stored(shape: Shape) -> list[Shape]:
    mut shapes = list[Shape]()
    shapes.append(shape)
    return shapes

def renamed(shape: Shape) -> float64:
    mut current = shape
    current = Shape.Circle(1.0)
    return area(current)

def later(shape: Shape) -> float64:
    read: def(float64) -> float64 = lambda scale: area(shape) * scale
    return read(1.0)

def each(shapes: list[Shape]) -> float64:
    mut total = 0.0
    for i in range(shapes.len()):
        total += area(shapes[i])
    return total

def equal(shapes: list[Shape]) -> bool:
    return shapes[0] == shapes[1]

def copies(nodes: list[Node]) -> int64:
    mut total = 0
    for node in nodes:
        tags = node.tags.copy()
        total += tags.len()
    return total

def walks(nodes: list[Node]) -> int64:
    mut total = 0
    for node in nodes:
        for child in node.children:
            total += child
    return total

def main():
    shapes = [Shape.Circle(2.0), Shape.Rect(1.0, 3.0)]
    print(each(shapes))
    print(doubled(shapes[0]))
    print(same(shapes[0], shapes[1]))
    print(area(chosen(shapes[0])))
    print(stored(shapes[1]).len())
    print(renamed(shapes[0]))
    print(later(shapes[0]))
    print(equal(shapes))
    nodes = [Node(tags = [1], children = [2, 3])]
    print(copies(nodes))
    print(walks(nodes))
    holder = Holder(tags = [1, 2])
    point = Point(x = 1, y = 2)
    print(point.twice())
    print(point.itself().norm())
    print(holder.probe())
    print(holder.mixed())
"#;

/// The module of [`COPIES`], with how each function that keeps its
/// callers' shared arguments takes them once every shared parameter of a
/// Copy type is kept too.
fn copies_module() -> (
    crate::mir::MirModule,
    std::collections::BTreeMap<String, crate::mir::ownership::BorrowingCallee>,
) {
    let module = lower_source_to_mir(COPIES).expect("the source should lower");
    let mut borrowing = crate::mir::ownership::kept_arguments(&module);
    for (name, undropped) in crate::mir::ownership::undropped_shared_params(&module) {
        let callee = borrowing.get_mut(&name).expect("a recorded function");
        let slots = callee.params.iter_mut().chain(callee.receiver.as_mut());
        for (kept, undropped) in slots.zip(undropped) {
            if undropped {
                *kept = crate::mir::MirReceiverKind::Borrow;
            }
        }
    }
    (module, borrowing)
}

fn kept_copies(name: &str) -> Vec<bool> {
    let (module, borrowing) = copies_module();
    let function = module
        .functions
        .iter()
        .find(|function| function.name == name)
        .expect("the function should exist");
    let receiver = usize::from(function.receiver.is_some());
    let candidates = vec![true; function.params.len() + receiver];
    super::kept_copy_params(function, &candidates, |instruction| {
        crate::mir::ownership::callee_borrowed_operands(instruction, |name| borrowing.get(name))
    })
}

#[test]
fn a_copy_parameter_only_read_stays_with_the_caller() {
    let undropped = crate::mir::ownership::undropped_shared_params(
        &lower_source_to_mir(COPIES).expect("the source should lower"),
    );
    assert_eq!(undropped.get("area"), Some(&vec![true]));
    assert_eq!(undropped.get("each"), Some(&vec![false]));
    // A match and payload reads, a call that borrows it, a comparison.
    // The MIR copies the left side of a comparison into a temporary.
    assert_eq!(kept_copies("area"), vec![true]);
    assert_eq!(kept_copies("doubled"), vec![true]);
    assert_eq!(kept_copies("same"), vec![false, true]);
    // A non-candidate stays owned.
    let (module, _) = copies_module();
    let area = module
        .functions
        .iter()
        .find(|function| function.name == "area")
        .expect("the function should exist");
    assert_eq!(
        super::kept_copy_params(area, &[false], |_| Vec::new()),
        vec![false]
    );
    // Without a callee that borrows it, a call takes the parameter.
    let doubled = module
        .functions
        .iter()
        .find(|function| function.name == "doubled")
        .expect("the function should exist");
    assert_eq!(
        super::kept_copy_params(doubled, &[true], |_| Vec::new()),
        vec![false]
    );
}

#[test]
fn a_copy_parameter_returned_stored_reassigned_or_captured_is_owned() {
    for name in ["chosen", "stored", "renamed", "later"] {
        assert_eq!(kept_copies(name), vec![false], "{name}");
    }
}

#[test]
fn a_copy_element_only_read_is_lent() {
    let (module, borrowing) = copies_module();
    let find = |name: &str| {
        module
            .functions
            .iter()
            .find(|function| function.name == name)
            .expect("the function should exist")
    };
    let borrowed = |instruction| {
        crate::mir::ownership::callee_borrowed_operands(instruction, |name| borrowing.get(name))
    };
    // The left side of the comparison is copied into a temporary first.
    for (name, reads) in [("each", 1), ("equal", 1)] {
        let found = lent_element_reads(
            find(name),
            |place| place.starts_with("%t"),
            |_| true,
            borrowed,
        );
        assert_eq!(found.lists.len(), reads, "{name}: {found:?}");
        assert!(found.promotions.is_empty(), "{found:?}");
    }
    // A call that owns its argument takes the element.
    let found = lent_element_reads(
        find("each"),
        |place| place.starts_with("%t"),
        |_| true,
        |_| Vec::new(),
    );
    assert!(found.lists.is_empty(), "{found:?}");
}

#[test]
fn a_copy_of_a_part_or_a_loop_over_one_reads_the_element() {
    let (module, _) = copies_module();
    for name in ["copies", "walks"] {
        let function = module
            .functions
            .iter()
            .find(|function| function.name == name)
            .expect("the function should exist");
        let found = lent(function);
        assert!(
            found.lists.values().any(|list| list == "nodes"),
            "{name}: {found:?}"
        );
    }
}

#[test]
fn a_field_lent_to_a_call_that_borrows_it_lends_its_box() {
    let (module, borrowing) = copies_module();
    let method = |name: &str| {
        module
            .functions
            .iter()
            .find(|function| function.name.ends_with(name))
            .expect("the method should exist")
    };
    let borrows = |instruction: &Instruction| {
        matches!(
            instruction,
            Instruction::Assign {
                value: Rvalue::Call {
                    callee: CallTarget::Name(name),
                    ..
                },
                ..
            } if borrowing.contains_key(name)
        )
    };
    let lent = crate::native_codegen::classes::field_lend_temps(method("probe"), borrows);
    assert_eq!(lent.len(), 1, "{lent:?}");
    // A call that owns its arguments may keep the field.
    assert!(
        crate::native_codegen::classes::field_lend_temps(method("probe"), |_| false).is_empty()
    );
    // A call that is also passed the instance may reach the field.
    assert!(crate::native_codegen::classes::field_lend_temps(method("mixed"), borrows).is_empty());
    // Anything else between the lend and its end may reach the field.
    let mut probe = method("probe").clone();
    let lend = probe.blocks[0]
        .instructions
        .iter()
        .position(|instruction| matches!(instruction, Instruction::Lend { .. }))
        .expect("the field is lent");
    probe.blocks[0]
        .instructions
        .insert(lend + 1, Instruction::Safepoint);
    assert!(crate::native_codegen::classes::field_lend_temps(&probe, |_| true).is_empty());
    // A further argument that is a constant leaves the field lent; one that
    // moves the instance, or names the field's temporary, may reach it.
    let with_arg = |value: Operand| {
        let mut probe = method("probe").clone();
        let call = probe.blocks[0]
            .instructions
            .iter_mut()
            .find_map(|instruction| match instruction {
                Instruction::Assign {
                    value:
                        Rvalue::Call {
                            callee: CallTarget::Name(name),
                            args,
                            ..
                        },
                    ..
                } if borrowing.contains_key(name) => Some(args),
                _ => None,
            })
            .expect("the field is passed to a call");
        call.push(arg(value));
        crate::native_codegen::classes::field_lend_temps(&probe, borrows).len()
    };
    assert_eq!(with_arg(Operand::Int(1)), 1);
    assert_eq!(with_arg(Operand::MovePlace("self".to_string())), 0);
    assert_eq!(with_arg(Operand::Place("self.next".to_string())), 0);
}

#[test]
fn a_mutable_lend_of_a_part_only_read_as_a_list_reads_the_element() {
    let lend = |temp: &str, mutable| Instruction::Lend {
        place: format!("{temp}.tags"),
        temp: "%t_lent".to_string(),
        mutable,
    };
    let end = |temp: &str| Instruction::EndLend {
        place: format!("{temp}.tags"),
        temp: "%t_lent".to_string(),
        mutable: true,
    };
    // The lent part is only read: its length.
    let found = lent_after(|tail, temp| {
        tail.insert(0, lend(temp, true));
        tail.insert(
            1,
            runtime_call(
                "%t_len",
                RuntimeOp::List(ListOp::Len),
                "%t_lent",
                Vec::new(),
            ),
        );
        tail.insert(2, end(temp));
    });
    assert!(!found.lists.is_empty(), "{found:?}");
    // The lent part is changed, which a copy of the part would lose.
    let found = lent_after(|tail, temp| {
        tail.insert(0, lend(temp, true));
        tail.insert(
            1,
            runtime_call(
                "%t_unit",
                RuntimeOp::List(ListOp::Push),
                "%t_lent",
                vec![MirArg {
                    name: None,
                    value: Operand::Int(1),
                    writeback_place: None,
                }],
            ),
        );
        tail.insert(2, end(temp));
    });
    assert!(found.lists.is_empty(), "{found:?}");
}

#[test]
fn a_copy_of_a_part_made_in_place_reads_the_element() {
    let found = lent_after(|tail, temp| {
        tail.insert(
            0,
            runtime_call(
                "%t_copy",
                RuntimeOp::Value(crate::runtime_ops::ValueOp::Copy),
                &format!("{temp}.tags"),
                Vec::new(),
            ),
        )
    });
    assert!(!found.lists.is_empty(), "{found:?}");
}

#[test]
fn a_copy_receiver_only_read_stays_with_the_caller() {
    let undropped = crate::mir::ownership::undropped_shared_params(
        &lower_source_to_mir(COPIES).expect("the source should lower"),
    );
    // A method's receiver comes after its parameters.
    assert_eq!(undropped.get("Point.norm"), Some(&vec![true]));
    assert_eq!(undropped.get("Holder.probe"), Some(&vec![false]));
    // Reads of its parts, and a method that borrows it.
    assert_eq!(kept_copies("Point.norm"), vec![true]);
    assert_eq!(kept_copies("Point.twice"), vec![true]);
    // Returned, so the method owns it.
    assert_eq!(kept_copies("Point.itself"), vec![false]);
}

#[test]
fn a_copy_parameter_lent_is_owned() {
    let (module, _) = copies_module();
    let mut area = module
        .functions
        .iter()
        .find(|function| function.name == "area")
        .expect("the function should exist")
        .clone();
    area.blocks[0].instructions.insert(
        0,
        Instruction::Lend {
            place: "shape".to_string(),
            temp: "%t_lent".to_string(),
            mutable: false,
        },
    );
    assert_eq!(
        super::kept_copy_params(&area, &[true], |_| Vec::new()),
        vec![false]
    );
}

const APPENDS: &str = r#"
enum Shape:
    Circle(float64)
    Empty

def fills(n: int64) -> int64:
    mut shapes: list[Shape] = []
    for i in range(n):
        shapes.append(Shape.Circle(i as float64))
        shapes.append(Shape.Empty)
    kept = Shape.Circle(1.0)
    shapes.append(kept)
    mut other: list[Shape] = []
    other.append(kept)
    return shapes.len() + other.len()

def main():
    print(fills(2))
"#;

#[test]
fn a_copy_temporary_only_appended_hands_its_box_to_the_list() {
    let mir = lower_source_to_mir(APPENDS).expect("the source should lower");
    let fills = mir
        .functions
        .iter()
        .find(|function| function.name == "fills")
        .expect("the function should exist");
    let temps = super::appended_temps(fills, |_| true);
    // The two variants built for their appends, and the two copies of the
    // local made for its appends, each of which holds a box of its own.
    assert_eq!(temps.len(), 4, "{temps:?}");
    assert!(temps.iter().all(|temp| temp.starts_with("%t")));
    // A temporary the MIR releases itself keeps its value.
    assert!(super::appended_temps(fills, |_| false).is_empty());
    // One named again after the append is read there.
    let mut named = fills.clone();
    let temp = temps.iter().next().expect("a temporary").to_string();
    let block = named
        .blocks
        .iter_mut()
        .find(|block| {
            block.instructions.iter().any(
                |instruction| matches!(instruction, Instruction::Assign { target, .. } if *target == temp),
            )
        })
        .expect("the temporary is assigned");
    block.instructions.push(Instruction::Eval {
        value: Operand::Place(temp.clone()),
    });
    assert!(!super::appended_temps(&named, |_| true).contains(temp.as_str()));
}

const FIELD_READS: &str = r#"
class Book:
    qty: list[int64]
    total: int64

    def fill(mut self, at: int64, amount: int64) -> int64:
        taken = min(amount, self.qty[at])
        self.qty[at] -= taken
        self.total += self.qty.len()
        return taken

def main():
    mut book = Book(qty=[5, 6], total=0)
    print(book.fill(1, 2))
"#;

#[test]
fn a_field_lent_only_to_be_read_comes_back_as_it_went() {
    let mir = lower_source_to_mir(FIELD_READS).expect("the source should lower");
    let module =
        crate::native_codegen::validated_for_emission(&mir).expect("the source should validate");
    let fill = module
        .functions
        .iter()
        .find(|function| function.name == "Book.fill")
        .expect("the module has `Book.fill`");
    let temps = crate::native_codegen::classes::field_lend_temps(fill, |_| false);
    let read_only = crate::native_codegen::classes::read_only_field_lends(fill, &temps);
    // The element read, its read again for `-=`, and the length read only
    // read the list; the element write changes it.
    assert_eq!(temps.len(), 4, "{temps:?}");
    assert_eq!(read_only.len(), 3, "{read_only:?}");
    assert!(read_only.is_subset(&temps));
}
