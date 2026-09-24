//! The audited builtin receiver table is the single source of receiver
//! capability (ADR-0061 H1). This characterization test pins the checker to
//! it: a method whose table entry is `BorrowMut` needs a mutable receiver
//! place, and every other method accepts a shared one.

use aura_compiler::ast::ReceiverKind;
use aura_compiler::call::BuiltinMember;

const PRELUDE: &str = "    values: list[int64] = [3, 1]\n    table: dict[str, int64] = {\"a\": 1}\n    tags: set[str] = {\"x\"}\n    queue = Queue[int64](2)\n";

fn program(mutable: bool, call: &str) -> String {
    let prelude = if mutable {
        PRELUDE
            .replace("    values:", "    mut values:")
            .replace("    table:", "    mut table:")
            .replace("    tags:", "    mut tags:")
            .replace("    queue =", "    mut queue =")
    } else {
        PRELUDE.to_string()
    };
    format!("def main():\n{prelude}    {call}\n")
}

#[test]
fn builtin_receiver_table_matches_the_checker() {
    let cases = [
        (BuiltinMember::VecPush, "values.append(2)"),
        (BuiltinMember::VecPop, "values.pop()"),
        (BuiltinMember::VecSet, "values.set(0, 5)"),
        (BuiltinMember::VecRemove, "values.remove(3)"),
        (BuiltinMember::VecSwap, "values.swap(0, 1)"),
        (BuiltinMember::VecExtend, "values.extend([4])"),
        (BuiltinMember::VecInsert, "values.insert(0, 9)"),
        (BuiltinMember::VecClear, "values.clear()"),
        (BuiltinMember::VecReverse, "values.reverse()"),
        (BuiltinMember::VecSort, "values.sort()"),
        (BuiltinMember::VecReserve, "values.reserve(2)"),
        (BuiltinMember::VecLen, "print(values.len())"),
        (BuiltinMember::VecIsEmpty, "print(values.is_empty())"),
        (BuiltinMember::VecIndex, "print(values.index(3))"),
        (BuiltinMember::VecCount, "print(values.count(3))"),
        (BuiltinMember::VecGet, "print(values.get(0))"),
        (BuiltinMember::VecClone, "print(values.copy())"),
        (BuiltinMember::MapRemove, "print(table.remove(\"a\"))"),
        (BuiltinMember::MapClear, "table.clear()"),
        (BuiltinMember::MapExtend, "table.update({\"c\": 3})"),
        (BuiltinMember::MapReserve, "table.reserve(1)"),
        (BuiltinMember::MapGet, "print(table.get(\"a\"))"),
        (BuiltinMember::SetInsert, "tags.add(\"y\")"),
        (BuiltinMember::SetRemove, "tags.remove(\"x\")"),
        (BuiltinMember::SetDiscard, "tags.discard(\"x\")"),
        (BuiltinMember::SetClear, "tags.clear()"),
        (BuiltinMember::SetReserve, "tags.reserve(1)"),
        (BuiltinMember::QueuePut, "print(queue.put(1))"),
        (BuiltinMember::QueueTryPut, "print(queue.try_put(1))"),
        (BuiltinMember::QueueClose, "queue.close()"),
    ];
    for (member, call) in cases {
        let shared = aura_compiler::check_source(&program(false, call));
        match member.receiver_passing() {
            ReceiverKind::BorrowMut => {
                let error = shared.expect_err(call);
                assert_eq!(error.code, "AU3003", "{call}: {error:?}");
                assert!(
                    error.message.contains("requires a mutable receiver"),
                    "{call}: {}",
                    error.message
                );
            }
            _ => {
                shared.unwrap_or_else(|error| {
                    panic!("{call} takes a shared receiver in the table: {error:?}")
                });
            }
        }
        aura_compiler::check_source(&program(true, call))
            .unwrap_or_else(|error| panic!("{call} with a mutable receiver: {error:?}"));
    }
}
