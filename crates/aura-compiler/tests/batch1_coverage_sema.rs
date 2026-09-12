//! Checker facade coverage: union operators and dispatch, conditional
//! narrowing through loops and tuple positions, callable packing and thin
//! alias adaptation, packed callable calls, view provenance, module-qualified
//! FFI items, and builtin argument error propagation.

use std::fs;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use aura_compiler::{check_path, check_source};

fn rejects(source: &str, expected: &str) {
    let error = check_source(source).expect_err("source should be rejected");
    assert!(
        error.message.contains(expected),
        "diagnostic `{}` should mention `{expected}` for:\n{source}",
        error.message
    );
}

fn rejects_with_code(source: &str, code: &str, expected: &str) {
    let error = check_source(source).expect_err("source should be rejected");
    assert_eq!(error.code, code, "{}\n{source}", error.message);
    assert!(
        error.message.contains(expected),
        "diagnostic `{}` should mention `{expected}` for:\n{source}",
        error.message
    );
}

fn accepts(source: &str) {
    if let Err(error) = check_source(source) {
        panic!("source should check: {}\n{source}", error.message);
    }
}

struct TempDir {
    path: PathBuf,
}

impl TempDir {
    fn new(prefix: &str) -> Self {
        let unique = format!(
            "{}-{}-{}",
            prefix,
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .expect("system time should be after unix epoch")
                .as_nanos()
        );
        let path = std::env::temp_dir().join(unique);
        fs::create_dir_all(&path).expect("failed to create temp dir");
        Self { path }
    }

    fn write(&self, relative: &str, source: &str) -> PathBuf {
        let path = self.path.join(relative);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).expect("failed to create parent dirs");
        }
        fs::write(&path, source).expect("failed to write module source");
        path
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.path);
    }
}

// ---------------------------------------------------------------------------
// Union operators, equality, dispatch, and cloning
// ---------------------------------------------------------------------------

#[test]
fn union_arithmetic_and_ordering_operators_name_the_operator_symbol() {
    for operator in ["+", "-", "*", "/", "//", "%", "<", "<=", ">", ">="] {
        let source =
            format!("def main():\n    value: int64 | str = 1\n    print(value {operator} 1)\n");
        let error = check_source(&source).expect_err("unions have no arithmetic or ordering");
        assert_eq!(error.code, "AU2003", "{operator}: {}", error.message);
        assert_eq!(
            error.message,
            format!(
                "operator `{operator}` is not supported for union `int64 | str`: unions have no ordering or arithmetic"
            )
        );
    }
    for operator in ["and", "or"] {
        let source = format!(
            "def main():\n    value: bool | str = true\n    print(value {operator} value)\n"
        );
        let error = check_source(&source).expect_err("unions have no logical operators");
        assert_eq!(error.code, "AU2003", "{operator}: {}", error.message);
        assert!(
            error
                .message
                .contains(&format!("operator `{operator}` is not supported for union")),
            "{operator}: {}",
            error.message
        );
    }
}

#[test]
fn union_equality_against_an_untypable_operand_names_the_other_operand() {
    rejects_with_code(
        "def main():\n    value: int64 | str = 1\n    print(value == [])\n",
        "AU2003",
        "cannot compare `int64 | str` and `the other operand`",
    );
}

#[test]
fn comparing_none_with_none_is_admitted() {
    accepts("def main():\n    print(None == None)\n");
}

#[test]
fn union_trait_dispatch_requires_one_trait_specialization_across_members() {
    let shared = "trait Conv[T]:\n    def conv(self) -> T\nclass A:\n    value: int64\nclass B:\n    value: int64\nimpl Conv[int64] for A:\n    def conv(self) -> int64:\n        return self.value\n";
    rejects_with_code(
        &format!(
            "{shared}impl Conv[str] for B:\n    def conv(self) -> str:\n        return \"b\"\ndef main():\n    value: A | B = A(value=1)\n    print(value.conv())\n"
        ),
        "AU2999",
        "method `conv` on `A | B` has different contracts for `A` and `B`",
    );
    accepts(&format!(
        "{shared}impl Conv[int64] for B:\n    def conv(self) -> int64:\n        return 2\ndef main():\n    value: A | B = A(value=1)\n    print(value.conv())\n"
    ));
}

#[test]
fn union_clone_rejects_arguments_and_non_cloneable_members() {
    rejects_with_code(
        "def main():\n    value: int64 | None = None\n    other = value.clone(1)\n    print(1)\n",
        "AU2004",
        "`clone` does not take arguments",
    );
    rejects_with_code(
        "class Box:\n    value: int64\ndef main():\n    value: Box | None = None\n    other = value.clone()\n    print(1)\n",
        "AU3007",
        "cannot clone `Box | None` because a member does not support cloning",
    );
}

// ---------------------------------------------------------------------------
// Conditional narrowing
// ---------------------------------------------------------------------------

#[test]
fn while_body_that_invalidates_its_entry_fact_is_rechecked_without_it() {
    accepts(
        "def main():\n    mut value: int64 | None = 3\n    while value is not None:\n        print(value)\n        value = None\n",
    );
    rejects_with_code(
        "def main():\n    mut value: int64 | None = 3\n    while value is not None:\n        print(value + 1)\n        value = None\n",
        "AU2003",
        "operator `+` is not supported for union `int64 | None`",
    );
}

#[test]
fn narrowing_facts_attach_to_tuple_positions_and_grouped_places() {
    accepts(
        "def main():\n    pair: (int64 | None, str) = (1, \"a\")\n    if pair[0] is not None:\n        print(pair[0] + 1)\n    if (pair[0]) is not None:\n        print(1)\n",
    );
    accepts("def main():\n    value = None\n    if value is None:\n        print(1)\n");
}

#[test]
fn conditional_expressions_take_the_typed_branch_for_literal_operands() {
    accepts(
        "def main():\n    flag = true\n    value: int64 | None = 1 if flag else (None)\n    other = 1.5 if flag else (2.5)\n    fvalue = 2.5\n    a = 1 if flag else fvalue\n    b = fvalue if flag else 1\n    c = 1.5 if flag else fvalue\n    d = fvalue if flag else 1.5\n    print(a + b + c + d + other)\n",
    );
}

#[test]
fn if_statements_whose_every_branch_leaves_the_loop_diverge() {
    accepts(
        "def main():\n    mut count = 0\n    while count < 3:\n        count += 1\n        if count == 1:\n            continue\n        else:\n            break\n",
    );
}

// ---------------------------------------------------------------------------
// Views and loans
// ---------------------------------------------------------------------------

#[test]
fn view_sources_must_have_a_stable_place_type() {
    rejects_with_code(
        "def helper() -> int64:\n    return 1\ndef main():\n    view v = helper\n    print(1)\n",
        "AU3004",
        "a view source must have a known stable place type",
    );
}

#[test]
fn indexed_assignment_through_a_mutable_view_writes_back() {
    accepts(
        "def main():\n    mut values = [1, 2]\n    view mut v = values\n    v[0] = 5\n    print(values[0])\n",
    );
}

#[test]
fn loan_closures_cannot_be_stored_in_inferred_collections_or_enum_payloads() {
    let prelude = "def main():\n    values = [1]\n    read = lambda [values]: values.len()\n";
    for tail in [
        "    items = [read]\n    print(1)\n",
        "    items = {read}\n    print(1)\n",
        "    items = {read: 1}\n    print(1)\n",
        "    items = {\"a\": read}\n    print(1)\n",
    ] {
        rejects_with_code(
            &format!("{prelude}{tail}"),
            "AU3010",
            "a closure containing a live view cannot be stored in a collection",
        );
    }
    rejects_with_code(
        "enum Holder:\n    Fn(def() -> int64)\ndef main():\n    values = [1]\n    read = lambda [values]: values.len()\n    holder = Holder.Fn(read)\n    print(1)\n",
        "AU3010",
        "a closure containing a live view cannot be stored in an enum payload",
    );
}

#[test]
fn explicit_loan_captures_respect_live_mutable_views() {
    rejects_with_code(
        "def main():\n    mut values = [1]\n    view mut editor = values\n    read: def() -> int64 = lambda [values]: values.len()\n    editor.append(2)\n    print(read())\n",
        "AU3002",
        "cannot create shared view of `values` while mutable loan held by `editor` remains live",
    );
}

#[test]
fn view_arguments_cannot_cross_a_task_boundary() {
    rejects_with_code(
        "def consume(values: list[int64]) -> int64:\n    return values.len()\ndef main():\n    values = [1, 2]\n    view v = values\n    with TaskGroup() as group:\n        task = group.start(consume, v)\n        print(task.result_or(0, timeout=1s))\n",
        "AU3008",
        "cannot cross a task boundary",
    );
}

#[test]
fn mutable_returned_views_are_rejected_in_assert_messages_and_elif_conditions() {
    let prelude = "class Counter:\n    value: int64\ndef value_mut(counter: mut Counter) -> view mut int64 from counter:\n    return view mut counter.value\ndef main():\n    mut counter = Counter(value=1)\n";
    rejects_with_code(
        &format!("{prelude}    assert counter.value == 1, value_mut(counter)\n"),
        "AU3010",
        "a mutable returned view requires a mutable view binding",
    );
    rejects_with_code(
        &format!(
            "{prelude}    if counter.value == 0:\n        print(0)\n    elif value_mut(counter):\n        print(1)\n"
        ),
        "AU3010",
        "a mutable returned view requires a mutable view binding",
    );
}

#[test]
fn comprehensions_freeze_field_places_they_iterate() {
    accepts(
        "class Holder:\n    values: list[int64]\ndef main():\n    holder = Holder(values=[1, 2])\n    doubled = [value * 2 for value in holder.values]\n    flat = [value for i in range(2) for value in holder.values]\n    print(doubled.len() + flat.len())\n",
    );
}

#[test]
fn borrow_and_move_of_one_place_cannot_mix_in_one_expression() {
    let prelude = "def read(values: list[int64]) -> int64:\n    return values.len()\n";
    rejects_with_code(
        &format!(
            "{prelude}def consume(values: own list[int64]) -> bool:\n    return true\ndef main():\n    values = [1]\n    print(read(values) > 0 and consume(values))\n"
        ),
        "AU3002",
        "cannot mix a move of `values` with a borrow of `values`",
    );
    rejects_with_code(
        &format!(
            "{prelude}def consume(values: own list[int64]) -> int64:\n    return 1\ndef main():\n    values = [1]\n    print(read(values) + consume(values))\n"
        ),
        "AU3002",
        "cannot mix a move of `values` with a borrow of `values`",
    );
}

// ---------------------------------------------------------------------------
// Callable packing and thin alias adaptation
// ---------------------------------------------------------------------------

#[test]
fn callable_pack_constructor_argument_shape_is_checked() {
    let reader = "type Reader = Callable[def() -> int64]\n";
    rejects_with_code(
        &format!(
            "{reader}def main():\n    reader = Reader(lambda: 1, lambda: 2)\n    print(reader())\n"
        ),
        "AU2004",
        "`Reader(...)` packs exactly one callable value, found 2 arguments",
    );
    rejects_with_code(
        &format!("{reader}def main():\n    reader = Reader(f=lambda: 1)\n    print(reader())\n"),
        "AU2004",
        "`Reader(...)` takes its callable value positionally",
    );
    rejects_with_code(
        &format!("{reader}def main():\n    reader = Reader(1)\n    print(reader())\n"),
        "AU2002",
        "`Reader(...)` expects a function value, lambda, or packed callable, found `int64`",
    );
    rejects_with_code(
        "type Gen[T] = Callable[def() -> T]\ndef main():\n    reader = Gen(lambda: 1)\n    print(1)\n",
        "AU2002",
        "callable alias `Gen` is generic",
    );
}

#[test]
fn callable_pack_checks_the_source_contract() {
    let reader = "type Reader = Callable[def() -> int64]\n";
    rejects_with_code(
        &format!(
            "{reader}def other(value: int64) -> int64:\n    return value\ndef main():\n    reader = Reader(other)\n    print(reader())\n"
        ),
        "AU2002",
        "expected `def() -> int64`, found `def(value: int64) -> int64`",
    );
    rejects_with_code(
        &format!(
            "{reader}type Other = Callable[def() -> str]\ndef main():\n    other = Other(lambda: \"x\")\n    reader = Reader(other)\n    print(reader())\n"
        ),
        "AU2002",
        "expected `def() -> int64`, found `def() -> str`",
    );
    rejects_with_code(
        &format!("{reader}def main():\n    text = lambda: \"x\"\n    reader = Reader(text)\n    print(1)\n"),
        "AU2002",
        "expected `def() -> int64`, found `def() -> str`",
    );
    rejects_with_code(
        "type Reader = Callable[def(value: int64) -> int64]\ndef main():\n    reader = Reader(lambda other: other)\n    print(reader(1))\n",
        "AU2015",
        "lambda parameter `other` must be named `value`",
    );
    rejects_with_code(
        "type Reader = Callable[def(value: int64) -> int64]\ndef named(other: int64) -> int64:\n    return other\ndef main():\n    reader = Reader(named)\n    print(reader(1))\n",
        "AU2015",
        "parameter `other` would be renamed to `value`",
    );
}

#[test]
fn callable_pack_rejects_erased_task_sources_and_live_loans() {
    rejects_with_code(
        "type Reader = Callable[def() -> int64]\ntype TaskReader = TaskCallable[def() -> int64]\ndef main():\n    reader = Reader(lambda: 1)\n    task_reader = TaskReader(reader)\n    print(1)\n",
        "AU3008",
        "an erased `Callable` cannot become `TaskReader`",
    );
    rejects_with_code(
        "type Reader = Callable[def() -> int64]\ndef main():\n    mut values = [1]\n    reader = Reader(lambda [mut values]: values.len())\n    print(1)\n",
        "AU3010",
        "capture `values` is a live mutable loan and cannot be packed into `Reader`",
    );
}

#[test]
fn thin_alias_adapter_argument_shape_and_contract_are_checked() {
    let thin = "type Thin = def() -> int64\n";
    rejects_with_code(
        &format!("{thin}def main():\n    thin = Thin()\n    print(1)\n"),
        "AU2004",
        "`Thin(...)` adapts exactly one callable value, found 0 arguments",
    );
    rejects_with_code(
        &format!("{thin}def main():\n    thin = Thin(f=lambda: 1)\n    print(1)\n"),
        "AU2004",
        "`Thin(...)` takes its callable value positionally",
    );
    rejects_with_code(
        "type Thin[T] = def() -> T\ndef main():\n    thin = Thin(lambda: 1)\n    print(1)\n",
        "AU2002",
        "thin callable alias `Thin` is generic",
    );
    rejects_with_code(
        &format!("{thin}def main():\n    offset = 1\n    thin = Thin(lambda [own offset]: offset)\n    print(1)\n"),
        "AU2015",
        "thin alias `Thin` cannot hold a capturing closure",
    );
    rejects_with_code(
        &format!(
            "{thin}def main():\n    text = lambda: \"x\"\n    thin = Thin(text)\n    print(1)\n"
        ),
        "AU2002",
        "expected `def() -> int64`, found `def() -> str`",
    );
    rejects_with_code(
        "type Thin = def(value: int64) -> int64\ndef main():\n    thin = Thin(lambda other: other)\n    print(1)\n",
        "AU2015",
        "lambda parameter `other` must be named `value`",
    );
}

// ---------------------------------------------------------------------------
// Packed callable calls
// ---------------------------------------------------------------------------

#[test]
fn packed_callable_fields_are_called_by_their_kind() {
    rejects_with_code(
        "type Reader = Callable[def() -> int64]\nclass Holder:\n    reader: Reader\ndef main():\n    holder = Holder(reader=Reader(lambda: 1))\n    print(holder.reader[int64]())\n",
        "AU2005",
        "packed callable values have a concrete contract and do not take explicit type arguments",
    );
    accepts(
        "type Finish = Callable[own def() -> int64]\nclass Holder:\n    finish: Finish\ndef main():\n    holder = Holder(finish=Finish(lambda: 1))\n    print(holder.finish())\n",
    );
    rejects_with_code(
        "type Bumper = Callable[mut def() -> int64]\nclass Holder:\n    bumper: Bumper\ndef main():\n    holder = Holder(bumper=Bumper(lambda: 1))\n    print(holder.bumper())\n",
        "AU3003",
        "a Mutable callable field must be called through a mutable place",
    );
}

#[test]
fn packed_callable_elements_are_called_through_the_collection() {
    rejects_with_code(
        "type Reader = Callable[def() -> int64]\ndef main():\n    table: dict[str, Reader] = {\"a\": Reader(lambda: 1)}\n    print(table[1]())\n",
        "AU2002",
        "dict index has type `int64`, expected `str`",
    );
    rejects_with_code(
        "type Finish = Callable[own def() -> int64]\ndef main():\n    items: list[Finish] = [Finish(lambda: 1)]\n    print(items[0]())\n",
        "AU3001",
        "a Consuming callable cannot be consumed inside a collection element",
    );
    rejects_with_code(
        "type Bumper = Callable[mut def() -> int64]\ndef main():\n    items: list[Bumper] = [Bumper(lambda: 1)]\n    print(items[0]())\n",
        "AU3003",
        "a Mutable callable element must be called through a mutable collection",
    );
}

#[test]
fn packed_callable_locals_take_no_explicit_type_arguments() {
    rejects_with_code(
        "type Reader = Callable[def() -> int64]\ndef main():\n    reader = Reader(lambda: 1)\n    print(reader[int64]())\n",
        "AU2005",
        "packed callable values have a concrete contract and do not take explicit type arguments",
    );
}

// ---------------------------------------------------------------------------
// Specialization and explicit type arguments
// ---------------------------------------------------------------------------

#[test]
fn specialization_index_outside_call_position_requires_type_arguments() {
    rejects(
        "def identity[T](value: own T) -> T:\n    return value\ndef main():\n    f = identity[1]\n    print(1)\n",
        "function specialization expects type arguments",
    );
}

#[test]
fn grouped_specialization_of_a_view_returning_function_requires_type_arguments() {
    rejects(
        "class User:\n    name: str\ndef name(user: User) -> view str from user:\n    return view user.name\ndef main():\n    user = User(name=\"a\")\n    view current = (name[1])(user)\n    print(current)\n",
        "function specialization expects type arguments",
    );
}

#[test]
fn explicit_type_argument_arity_is_checked_for_every_callee_shape() {
    rejects_with_code(
        "def identity[T](value: own T) -> T:\n    return value\ndef main():\n    print(identity[int64, str](1))\n",
        "AU2002",
        "function `identity` expects 1 type argument, found 2",
    );
    rejects_with_code(
        "class Box[T]:\n    value: T\ndef main():\n    box = Box[int64, str](value=1)\n    print(1)\n",
        "AU2002",
        "class constructor `Box` expects 1 type argument, found 2",
    );
    rejects_with_code(
        "enum Maybe[T]:\n    Just(T)\n    Nothing\ndef main():\n    value = Maybe[int64, str].Just(1)\n    print(1)\n",
        "AU2002",
        "enum `Maybe` expects 1 type argument, found 2",
    );
    rejects_with_code(
        "class Box[T]:\n    value: T\n    def make(value: own T) -> Box[T]:\n        return Box(value=value)\ndef main():\n    box = Box[int64, str].make(1)\n    print(1)\n",
        "AU2002",
        "class `Box` expects 1 type argument, found 2",
    );
}

// ---------------------------------------------------------------------------
// Names, members, and patterns
// ---------------------------------------------------------------------------

#[test]
fn type_parameter_field_access_without_a_trait_method_is_rejected() {
    rejects(
        "def peek[T](value: T):\n    print(value.missing)\ndef main():\n    peek(1)\n",
        "cannot access field `missing` on `T`",
    );
}

#[test]
fn aliases_of_classes_and_enums_resolve_as_type_values() {
    accepts("class Box:\n    value: int64\ntype Alias = Box\ndef main():\n    print(Alias)\n");
    accepts(
        "enum Shape:\n    Circle\n    Square\ntype Choice = Shape\ndef main():\n    print(Choice)\n",
    );
}

#[test]
fn class_scrutinees_reject_variant_shaped_or_and_tuple_patterns() {
    let prelude = "enum Shape:\n    Circle\n    Square\nclass Box:\n    value: int64\ndef main():\n    box = Box(value=1)\n    match box:\n";
    rejects_with_code(
        &format!("{prelude}        case Shape.Circle | Shape.Square:\n            print(1)\n        case _:\n            print(0)\n"),
        "AU2999",
        "class patterns are not supported",
    );
    rejects_with_code(
        &format!("{prelude}        case (int64 as number, _):\n            print(number)\n        case _:\n            print(0)\n"),
        "AU2999",
        "class patterns are not supported",
    );
}

#[test]
fn mutable_receiver_methods_require_a_mutable_receiver_place() {
    rejects_with_code(
        "class Counter:\n    total: int64\n    def bump(mut self) -> int64:\n        self.total = self.total + 1\n        return self.total\ndef main():\n    counter = Counter(total=0)\n    print(counter.bump())\n",
        "AU3003",
        "method `bump` requires a mutable receiver",
    );
    rejects_with_code(
        "trait Bump:\n    def bump(mut self) -> int64\nclass Counter:\n    total: int64\nimpl Bump for Counter:\n    def bump(mut self) -> int64:\n        self.total = self.total + 1\n        return self.total\ndef main():\n    counter = Counter(total=0)\n    print(counter.bump())\n",
        "AU3003",
        "method `bump` requires a mutable receiver",
    );
}

#[test]
fn operators_on_unbounded_type_parameters_and_bitwise_not_on_classes_are_rejected() {
    rejects_with_code(
        "def negate[T](value: own T) -> T:\n    return -value\ndef main():\n    print(negate(1))\n",
        "AU2003",
        "unary `-` expects a numeric value, found `T`",
    );
    rejects_with_code(
        "def total[T](left: own T, right: own T) -> T:\n    return left + right\ndef main():\n    print(total(1, 2))\n",
        "AU2999",
        "unsupported operands for binary expression: `T`",
    );
    rejects_with_code(
        "class Box:\n    value: int64\ndef main():\n    box = Box(value=1)\n    print(~box)\n",
        "AU2003",
        "unary `~` expects an integer value, found `Box`",
    );
}

#[test]
fn tuple_member_objects_require_literal_indices() {
    rejects_with_code(
        "class Box:\n    value: int64\ndef main():\n    pair = (Box(value=1), Box(value=2))\n    i = 0\n    print(pair[i].value)\n",
        "AU2003",
        "tuple indices must be non-negative integer literals",
    );
}

#[test]
fn generic_constructor_fields_report_the_hinted_argument_error() {
    let holder = "class Holder[T]:\n    value: T\ndef main():\n";
    rejects(
        &format!("{holder}    holder = Holder(value=[])\n    print(1)\n"),
        "empty list literals require an expected `list[T]` type annotation",
    );
    rejects_with_code(
        &format!("{holder}    holder = Holder(value=missing())\n    print(1)\n"),
        "AU2999",
        "unsupported call target",
    );
    rejects_with_code(
        &format!("{holder}    holder = Holder(value=(missing()))\n    print(1)\n"),
        "AU2999",
        "unsupported call target",
    );
}

#[test]
fn conditional_collection_literal_branches_share_the_first_branch_element_type() {
    let prelude = "def main():\n    flag = true\n";
    rejects_with_code(
        &format!("{prelude}    value = [1] if flag else [\"a\"]\n    print(1)\n"),
        "AU2002",
        "list literal elements must all have type `int64`, found `str`",
    );
    rejects_with_code(
        &format!("{prelude}    value = {{1}} if flag else {{\"a\"}}\n    print(1)\n"),
        "AU2002",
        "set literal elements must all have type `int64`, found `str`",
    );
    rejects_with_code(
        &format!("{prelude}    value = {{\"a\": 1}} if flag else {{1: 2}}\n    print(1)\n"),
        "AU2002",
        "map literal keys must all have type `str`, found `int64`",
    );
    rejects_with_code(
        &format!("{prelude}    value = (1, \"a\") if flag else (\"a\", 1)\n    print(1)\n"),
        "AU2002",
        "conditional expression arms must have one type; expected `(int64, str)`, found `(str, int64)`",
    );
    rejects_with_code(
        &format!("{prelude}    value = (1, \"a\") if flag else (1, \"a\", 2)\n    print(1)\n"),
        "AU2002",
        "conditional expression arms must have one type",
    );
    accepts(&format!(
        "{prelude}    value = [1] if flag else [2]\n    other = {{1}} if flag else {{2}}\n    table = {{\"a\": 1}} if flag else {{\"b\": 2}}\n    pair = (1, \"a\") if flag else (2, \"b\")\n    print(value.len() + other.len() + table.len() + pair[0])\n"
    ));
}

#[test]
fn array_constructor_arguments_cannot_move_a_sibling_borrow() {
    rejects_with_code(
        "def take(values: own list[float64]) -> list[int64]:\n    return [1]\ndef main():\n    values = [1.0]\n    array = Array[float64].from_list(values, take(values))\n    print(1)\n",
        "AU3002",
        "cannot consume `values` while `values` remains shared-borrowed",
    );
}

// ---------------------------------------------------------------------------
// Task group members
// ---------------------------------------------------------------------------

#[test]
fn task_group_stack_overrides_and_unknown_members_are_checked() {
    let prelude = "def work() -> int64:\n    return 1\ndef main():\n";
    rejects_with_code(
        &format!("{prelude}    with TaskGroup() as group:\n        task = group.start_with_stack(1024)\n"),
        "AU2004",
        "`start_with_stack` expects a stack size in bytes and a target function followed by its arguments",
    );
    accepts(&format!(
        "{prelude}    size = 3\n    with TaskGroup() as group:\n        task = group.start_with_stack(-size, work)\n        print(task.result_or(0, timeout=1s))\n"
    ));
    rejects_with_code(
        "def main():\n    with TaskGroup() as group:\n        group.unknown()\n",
        "AU2999",
        "unsupported method call `unknown` on `TaskGroup`",
    );
}

// ---------------------------------------------------------------------------
// Builtin argument errors propagate from every checked position
// ---------------------------------------------------------------------------

#[test]
fn builtin_argument_expressions_propagate_their_own_diagnostics() {
    let task_prelude = "def work() -> int64:\n    return 1\ndef main():\n    with TaskGroup() as group:\n        task = group.start(work)\n";
    for source in [
        "def main():\n    print(str.from_bytes(missing))\n".to_string(),
        "def main():\n    print(parse_int32(missing))\n".to_string(),
        "def main():\n    print(parse_int64(missing))\n".to_string(),
        "def main():\n    print(parse_float64(missing))\n".to_string(),
        "def main():\n    values = list[int64].with_capacity(missing)\n    print(1)\n".to_string(),
        "def main():\n    values = [1]\n    print(values.count(missing))\n".to_string(),
        "def main():\n    values = [1]\n    print(values.contains(missing))\n".to_string(),
        "def main():\n    mut values = [1]\n    values.extend(missing)\n".to_string(),
        "def main():\n    mut values = [1]\n    values.insert(0, missing)\n".to_string(),
        "def main():\n    mut values = [1]\n    values.reserve(missing)\n".to_string(),
        "def main():\n    text = \"a\"\n    print(text.starts_with(missing))\n".to_string(),
        "def main():\n    text = \"a\"\n    print(text.split(missing))\n".to_string(),
        "def main():\n    text = \"a\"\n    print(text.replace(missing, \"b\"))\n".to_string(),
        "def main():\n    text = \"a\"\n    print(text.replace(\"a\", missing))\n".to_string(),
        "def main():\n    text = \",\"\n    print(text.join(missing))\n".to_string(),
        "def main():\n    text = \"a\"\n    print(text.strip_prefix(missing))\n".to_string(),
        "def main():\n    table: dict[str, int64] = {\"a\": 1}\n    print(table.get(missing))\n".to_string(),
        "def main():\n    mut table: dict[str, int64] = {\"a\": 1}\n    table.remove(missing)\n".to_string(),
        "def main():\n    mut table: dict[str, int64] = {\"a\": 1}\n    table.update(missing)\n".to_string(),
        "def main():\n    mut table: dict[str, int64] = {\"a\": 1}\n    table.reserve(missing)\n".to_string(),
        "def main():\n    mut items: set[int64] = {1}\n    items.add(missing)\n".to_string(),
        "def main():\n    mut items: set[int64] = {1}\n    items.reserve(missing)\n".to_string(),
        "def main():\n    queue = Queue[int64](4)\n    queue.put(missing)\n".to_string(),
        "def main():\n    queue = Queue[int64](4)\n    queue.put(1, timeout=missing)\n".to_string(),
        "def main():\n    queue = Queue[int64](4)\n    print(queue.get(timeout=missing))\n".to_string(),
        "def main():\n    queue = Queue[int64](4)\n    print(queue.get_or(missing))\n".to_string(),
        "def main():\n    queue = Queue[int64](4)\n    print(queue.get_or(1, timeout=missing))\n".to_string(),
        format!("{task_prelude}        print(task.result(timeout=missing))\n"),
        format!("{task_prelude}        print(task.result_or(missing))\n"),
        format!("{task_prelude}        print(task.result_or(1, timeout=missing))\n"),
        format!("{task_prelude}        tasks = [group.start(work)]\n        print(wait_any(tasks, timeout=missing))\n"),
        "def work() -> int64:\n    return 1\ndef main():\n    with TaskGroup() as group:\n        task = group.start_with_stack(missing, work)\n".to_string(),
        "def main():\n    values = Array[float64].zeros(missing)\n".to_string(),
        "def main():\n    values = Array[float64].full([2], missing)\n".to_string(),
        "def main():\n    values = Array[float64].from_list(missing, [2])\n".to_string(),
        "def main():\n    values = Array[float64].zeros([2])\n    print(values.get(missing))\n".to_string(),
        "import random\ndef main():\n    mut rng = random.Rng(1)\n    print(rng.next_int(missing, 2))\n".to_string(),
    ] {
        rejects_with_code(&source, "AU2001", "unknown name `missing`");
    }
    rejects_with_code(
        "def main():\n    values = list[int64].with_capacity()\n    print(1)\n",
        "AU2004",
        "`list.with_capacity` is missing required argument `minimum`",
    );
    rejects_with_code(
        "def main():\n    mut values = [1]\n    values.pop(\"x\")\n",
        "AU2002",
        "list indices must have type `int64`",
    );
}

// ---------------------------------------------------------------------------
// Module-qualified FFI items
// ---------------------------------------------------------------------------

fn write_ffi_package(temp: &TempDir, main_source: &str) -> PathBuf {
    temp.write(
        "Aura.toml",
        "[package]\nname = \"ffi_members\"\nversion = \"0.1.0\"\nedition = \"2026\"\nallow_ffi = true\n",
    );
    temp.write(
        "src/ffi.au",
        "public extern \"C\" def scalar(value: int32) -> int64\npublic extern \"C\" opaque class Handle\npublic type Reader = Callable[def() -> int64]\npublic class Box:\n    value: int64\n",
    );
    temp.write("src/main.au", main_source)
}

#[test]
fn module_qualified_extern_items_are_not_member_objects() {
    for (body, code, expected) in [
        (
            "    print(ffi.scalar.name)\n",
            "AU2999",
            "extern function `scalar` from module `ffi` is direct-call-only",
        ),
        (
            "    print(ffi.Handle.name)\n",
            "AU2005",
            "opaque FFI handle type `Handle` from module `ffi` is not a value",
        ),
        (
            "    print(ffi.Box.name)\n",
            "AU2999",
            "class `Box` from module `ffi` must be constructed with `(...)`",
        ),
    ] {
        let temp = TempDir::new("aura-sema-ffi-members");
        let main_path = write_ffi_package(&temp, &format!("import ffi\ndef main():\n{body}"));
        let error = check_path(&main_path).expect_err("module items are not member objects");
        assert_eq!(error.code, code, "{}", error.message);
        assert!(
            error.message.contains(expected),
            "diagnostic `{}` should mention `{expected}`",
            error.message
        );
    }
}

#[test]
fn module_qualified_callable_aliases_pack_values() {
    let temp = TempDir::new("aura-sema-module-alias-pack");
    let main_path = write_ffi_package(
        &temp,
        "import ffi\ndef main():\n    reader = ffi.Reader(lambda: 1)\n    print(reader())\n",
    );
    check_path(&main_path).expect("a module-qualified callable alias packs a lambda");
}
