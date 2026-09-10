//! Checker diagnostics for builtin collection argument mismatches and
//! closure storage rules that the fixture suites did not pin.

use aura_compiler::check_source;

fn rejects(source: &str, expected: &str) {
    let error = check_source(source).expect_err("source should be rejected");
    assert!(
        error.message.contains(expected),
        "diagnostic `{}` should mention `{expected}`",
        error.message
    );
}

#[test]
fn dict_remove_rejects_a_mismatched_key_type() {
    rejects(
        "def main():\n    mut table: dict[str, int64] = {\"a\": 1}\n    table.remove(1)\n",
        "`remove` expects",
    );
}

#[test]
fn union_field_access_requires_narrowing() {
    rejects(
        "class Holder:\n    values: list[int64]\ndef main():\n    holder: Holder | None = Holder(values=[1])\n    print(holder.values.len())\n",
        "without narrowing",
    );
}

#[test]
fn loan_closure_cannot_be_stored_in_an_annotated_list() {
    rejects(
        "def main():\n    values = [1]\n    read: def() -> int64 = lambda [values]: values.len()\n    items: list[def() -> int64] = [read]\n    print(items.len())\n",
        "cannot be stored in a collection",
    );
}

#[test]
fn union_task_results_require_all_member_transfer_validation() {
    rejects(
        "def make() -> int64 | str:\n    return 1\ndef main():\n    with TaskGroup() as group:\n        task = group.start(make)\n        print(task.result_or(2, timeout=1s))\n",
        "requires all-member Transfer validation",
    );
}

#[test]
fn union_parameters_holding_task_handles_type_check() {
    check_source(
        "def observe(handle: Task[int64] | None):\n    pass\ndef main():\n    observe(None)\n",
    )
    .expect("a union parameter with a task member is observable");
}

#[test]
fn indirect_is_rejected_on_owned_callable_types() {
    let error = aura_compiler::parse_source(
        "class Holder:\n    callback: indirect Callable[def() -> None]\n",
    )
    .expect_err("indirect callable fields are rejected");
    assert!(
        error.message.contains("not valid on owned callable types"),
        "{}",
        error.message
    );
}

#[test]
fn generic_alias_bindings_render_the_written_alias_and_its_expansion() {
    rejects(
        "type Boxes[T] = list[T]\ndef main():\n    items: Boxes[int64] = \"no\"\n    print(items)\n",
        "expected Boxes[int64] (= list[int64]), found str",
    );
}

#[test]
fn task_targets_accept_explicit_associated_method_type_arguments() {
    check_source(
        "class Box[T]:\n    value: T\n    def make[U](item: own U) -> U:\n        return item\ndef main():\n    with TaskGroup() as group:\n        task = group.start(Box[int64].make[str], \"x\")\n        print(task.result_or(\"y\", timeout=1s))\n",
    )
    .expect("explicit method type arguments specialize a task target");
}

#[test]
fn generic_alias_constructors_accept_multiple_type_arguments() {
    check_source(
        "class Pair[A, B]:\n    left: A\n    right: B\ntype Couple[A, B] = Pair[A, B]\ndef main():\n    value = Couple[int64, str](left=1, right=\"a\")\n    print(value.left)\n",
    )
    .expect("a two-argument alias constructor expands to its class");
}

#[test]
fn public_aliases_cannot_expose_private_types_through_callable_parameters() {
    rejects(
        "class Secret:\n    value: int64\npublic type Handler = def(Secret) -> None\ndef main():\n    pass\n",
        "exposes private type `Secret`",
    );
}

#[test]
fn public_aliases_cannot_expose_private_types_through_tuple_members() {
    rejects(
        "class Secret:\n    value: int64\npublic type Pair = (int64, Secret)\ndef main():\n    pass\n",
        "exposes private type `Secret`",
    );
}

#[test]
fn view_arguments_cannot_cross_a_task_boundary() {
    rejects(
        "def worker(values: list[int64]) -> int64:\n    return values.len()\ndef main():\n    items = [1, 2]\n    view shared = items\n    with TaskGroup() as group:\n        task = group.start(worker, shared)\n        print(task.result_or(0, timeout=1s))\n",
        "cannot cross a task boundary",
    );
}

#[test]
fn indexed_collection_elements_cannot_be_returned_as_views() {
    rejects(
        "def first[T](items: list[T]) -> view T from items:\n    return view items[0]\ndef main():\n    values = [1, 2]\n    view head = first(values)\n    print(head)\n",
        "do not have stable view identity",
    );
}

#[test]
fn consuming_an_argument_while_its_field_is_borrowed_by_another_argument_is_rejected() {
    rejects(
        "class Holder:\n    name: str\ndef consume(holder: own Holder) -> int64:\n    return 1\ndef pair(text: str, count: int64) -> int64:\n    return count\ndef main():\n    holder = Holder(name=\"a\")\n    print(pair(holder.name, consume(holder)))\n",
        "remains shared-borrowed",
    );
}

#[test]
fn union_values_compare_and_flow_through_generic_choices() {
    check_source(
        "def choose[T](first: own T, second: own T, flag: bool) -> T:\n    return first if flag else second\ndef main():\n    tagged: int64 | str = 1\n    other: int64 | str = 1\n    print(tagged == other)\n    print([tagged] == [other])\n    match choose(tagged, other, true):\n        case int64 as number:\n            print(number)\n        case str as text:\n            print(text)\n",
    )
    .expect("union operands compare and specialize generic choices");
}

#[test]
fn match_expressions_reject_duplicate_type_arms() {
    rejects(
        "def main():\n    value: int64 | str = 1\n    result = match value:\n        case int64 as number: number\n        case int64 as again: again\n        case str as text: text.len()\n    print(result)\n",
        "duplicate or unreachable type arm",
    );
}

#[test]
fn match_expressions_require_complete_type_coverage() {
    rejects(
        "def main():\n    value: int64 | str = 1\n    result = match value:\n        case int64 as number: number\n    print(result)\n",
        "does not cover",
    );
}

#[test]
fn or_patterns_with_differently_typed_type_arms_are_rejected() {
    rejects(
        "def main():\n    value: int64 | str = 1\n    match value:\n        case int64 as item | str as item:\n            print(1)\n",
        "must bind the same names with identical types",
    );
}

#[test]
fn mutable_match_cannot_overlap_a_live_shared_view() {
    rejects(
        "def main():\n    mut value: int64 | str = 1\n    view shared = value\n    match mut value:\n        case int64 as number:\n            number += 1\n        case str as text:\n            print(text)\n    print(shared)\n",
        "while shared view `shared` remains live",
    );
}

#[test]
fn local_aliases_reject_the_wrong_number_of_type_arguments() {
    rejects(
        "type Boxes[T] = list[T]\ndef main():\n    items: Boxes[int64, str] = [1]\n    print(items.len())\n",
        "expects exactly 1 type argument, found 2",
    );
}

#[test]
fn union_alias_type_arms_must_select_one_direct_member() {
    rejects(
        "type Numbers = int64 | float64\ndef main():\n    value: Numbers | str = \"text\"\n    match value:\n        case Numbers as number:\n            print(1)\n        case str as text:\n            print(text)\n",
        "denotes several union members",
    );
}

#[test]
fn generic_functions_return_unions_of_their_type_parameters() {
    check_source(
        "def wrap[T](value: own T) -> T | None:\n    return value\ndef main():\n    match wrap(1):\n        case int64 as number:\n            print(number)\n        case None:\n            print(0)\n",
    )
    .expect("a union return type may mention the type parameter");
}

#[test]
fn owned_callable_type_forms_require_batch_one_lowering() {
    rejects(
        "def main():\n    callback: Callable[def() -> None] = print\n",
        "requires Batch 1 semantic lowering",
    );
}

#[test]
fn len_requires_a_value_with_a_len_member() {
    rejects(
        "def main():\n    print(len((1, 2)))\n",
        "expects a value with a `len()` member",
    );
}

#[test]
fn loan_closures_cannot_be_stored_in_enum_payloads() {
    rejects(
        "enum Job:\n    Run(def() -> int64)\ndef main():\n    values = [1]\n    read: def() -> int64 = lambda [values]: values.len()\n    job = Job.Run(read)\n",
        "cannot be stored in an enum payload",
    );
}

#[test]
fn calls_cannot_return_loan_closures_through_generic_results() {
    rejects(
        "def same[T](value: own T) -> T:\n    return value\ndef main():\n    values = [1]\n    read: def() -> int64 = lambda [values]: values.len()\n    again = same(read)\n    print(again())\n",
        "closure containing a live view",
    );
}
