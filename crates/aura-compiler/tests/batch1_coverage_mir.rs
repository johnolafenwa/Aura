//! Coverage regressions for MIR lowering shapes and the shared fail-closed
//! MIR validator. Lowering cases pin a whole-program behaviour by executing
//! it on the interpreter (and inspecting the lowered module where the shape
//! matters); validator cases lower a valid program, corrupt one MIR fact
//! through its serialized form, and assert that both public boundaries (the
//! interpreter and the direct backend) reject it with the validator's message.

use aura_compiler::{emit_host_native_object, lower_source_to_mir, run_mir, run_source, MirModule};
use serde_json::{json, Value};

fn encode(source: &str) -> Value {
    serde_json::to_value(lower_source_to_mir(source).expect("source should lower")).unwrap()
}

fn function_mut<'a>(encoded: &'a mut Value, name: &str) -> &'a mut Value {
    encoded["functions"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .find(|function| function["name"] == name)
        .unwrap_or_else(|| panic!("function `{name}` should exist"))
}

/// Every instruction of `function`, in block order; `get_mut` chains never
/// insert keys into an encoded instruction.
fn instructions_mut<'a>(function: &'a mut Value) -> impl Iterator<Item = &'a mut Value> + 'a {
    function["blocks"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .flat_map(|block| block["instructions"].as_array_mut().unwrap().iter_mut())
}

/// The payload of every `Assign` rvalue of the given variant.
fn rvalues_mut<'a>(
    function: &'a mut Value,
    variant: &'a str,
) -> impl Iterator<Item = &'a mut Value> + 'a {
    instructions_mut(function).filter_map(move |instruction| {
        instruction
            .get_mut("Assign")
            .and_then(|assign| assign.get_mut("value"))
            .and_then(|value| value.get_mut(variant))
    })
}

fn local_type_mut<'a>(function: &'a mut Value, name: &str) -> &'a mut Value {
    function["local_types"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .find(|local| local["name"] == name)
        .unwrap_or_else(|| panic!("local `{name}` should be typed"))
}

/// Rewrites every string leaf of the JSON tree with `rewrite`.
fn rewrite_strings(value: &mut Value, rewrite: &dyn Fn(&str) -> Option<String>) {
    match value {
        Value::String(text) => {
            if let Some(replacement) = rewrite(text) {
                *text = replacement;
            }
        }
        Value::Array(items) => items
            .iter_mut()
            .for_each(|item| rewrite_strings(item, rewrite)),
        Value::Object(map) => map
            .values_mut()
            .for_each(|item| rewrite_strings(item, rewrite)),
        _ => {}
    }
}

/// Rewrites every `ReturnedView` contract node in the tree.
fn rewrite_returned_views(value: &mut Value, rewrite: &dyn Fn(&mut Value)) {
    match value {
        Value::Object(map) => {
            if let Some(view) = map.get_mut("ReturnedView") {
                rewrite(view);
            }
            map.values_mut()
                .for_each(|item| rewrite_returned_views(item, rewrite));
        }
        Value::Array(items) => items
            .iter_mut()
            .for_each(|item| rewrite_returned_views(item, rewrite)),
        _ => {}
    }
}

fn decode(encoded: Value) -> MirModule {
    serde_json::from_value(encoded).expect("forged MIR should deserialize")
}

fn assert_rejected(encoded: Value, expected: &str) {
    let mir = decode(encoded);
    let interpreted = run_mir(&mir).expect_err("interpreter must reject the forged module");
    assert!(
        interpreted.message.contains(expected),
        "interpreter rejection `{}` should mention `{expected}`",
        interpreted.message
    );
    let native =
        emit_host_native_object(&mir).expect_err("native emission must reject the forged module");
    assert!(
        native.contains(expected),
        "native rejection `{native}` should mention `{expected}`"
    );
}

fn assert_runs(source: &str, expected_stdout: &str) -> Value {
    let output = run_source(source).unwrap_or_else(|diag| panic!("{}", diag.message));
    assert_eq!(output.stdout, expected_stdout);
    encode(source)
}

fn module_text(encoded: &Value) -> String {
    serde_json::to_string(encoded).unwrap()
}

// ---------------------------------------------------------------------------
// Lowering shapes reached from whole programs.
// ---------------------------------------------------------------------------

#[test]
fn conditional_expressions_branch_on_none_tests() {
    let source = "def main():\n    value: int64 | None = None\n    other: int64 | None = 2\n    label = \"missing\" if value is None else \"present\"\n    print(label)\n    flag = 1 if not (value is None) else 2\n    print(flag)\n    either = 3 if value is None or other is None else 4\n    print(either)\n    both = 5 if (value is None) and (other is not None) else 6\n    print(both)\n";
    assert_runs(source, "missing\n2\n3\n5\n");
}

#[test]
fn generic_negated_none_tests_lower_to_runtime_probes() {
    let source = "def present[T](value: T) -> bool:\n    return value is not None\ndef main():\n    print(present(1))\n    print(present(None))\n";
    let encoded = assert_runs(source, "true\nfalse\n");
    let text = module_text(&encoded);
    assert!(text.contains("NoneTest"), "generic tests probe at runtime");
    assert!(text.contains("\"Not\""), "negation applies to the probe");
}

#[test]
fn explicit_specializations_are_function_values() {
    let source = "class Box:\n    def make[T](value: own T) -> T:\n        return value\ndef identity[T](value: own T) -> T:\n    return value\ndef main():\n    make = Box.make[int64]\n    print(make(3))\n    ident = identity[str]\n    print(ident(\"a\"))\n    print(identity[int64](4))\n";
    assert_runs(source, "3\na\n4\n");
}

#[test]
fn associated_method_values_lower_to_function_operands() {
    let source = "class Counter:\n    def zero() -> int64:\n        return 0\ndef main():\n    f = Counter.zero\n    print(f())\n";
    let encoded = assert_runs(source, "0\n");
    assert!(module_text(&encoded).contains("\"Function\""));
}

#[test]
fn narrowed_reads_through_views_reborrow_the_descriptor() {
    let source = "def main():\n    mut value: int64 | None = 7\n    view shared = value\n    if shared is not None:\n        total = shared + 1\n        print(total)\n";
    let encoded = assert_runs(source, "8\n");
    let text = module_text(&encoded);
    assert!(text.contains("Reborrow"));
    assert!(text.contains("ReadLoan"));
    assert!(text.contains("__union_payload_0"));
}

#[test]
fn callable_annotations_lower_written_contracts() {
    let source = "type Reader = Callable[def() -> int64]\ndef main():\n    reader: Callable[def() -> int64] = Reader(lambda: 5)\n    print(reader())\n";
    let mut encoded = assert_runs(source, "5\n");
    let reader = local_type_mut(function_mut(&mut encoded, "main"), "reader");
    assert!(
        reader["ty"].get("Callable").is_some(),
        "annotation should lower to an erased callable: {reader}"
    );
}

#[test]
fn keyword_only_callable_contracts_bind_named_arguments() {
    let source = "type Adder = Callable[def(a: int64, *, b: int64 = ...) -> int64]\ndef add(a: int64, *, b: int64 = 1) -> int64:\n    return a + b\ndef call(adder: Adder) -> int64:\n    return adder(1) + adder(1, b=2)\ndef main():\n    print(call(Adder(add)))\n";
    assert_runs(source, "5\n");
}

#[test]
fn closure_locals_and_bound_methods_start_tasks() {
    let source = "class Counter:\n    total: int64\n    def bump(mut self) -> int64:\n        self.total = self.total + 1\n        return self.total\ndef main():\n    counter = Counter(total=1)\n    worker = lambda [own counter]: counter.bump()\n    other = Counter(total=10)\n    with TaskGroup() as group:\n        task = group.start(worker)\n        print(task.result_or(-1, timeout=1s))\n        bound = group.start(other.bump)\n        print(bound.result_or(-1, timeout=1s))\n";
    let encoded = assert_runs(source, "2\n11\n");
    assert!(module_text(&encoded).contains("StartTask"));
}

#[test]
fn consuming_or_patterns_in_tuples_register_bindings() {
    let same = "def main():\n    pair = (1, \"a\")\n    match own pair:\n        case (x | x, text):\n            print(text)\n            print(x)\n";
    assert_runs(same, "a\n1\n");
    let wildcards = "def main():\n    pair = (1, \"a\")\n    match own pair:\n        case (_ | _, text):\n            print(text)\n";
    assert_runs(wildcards, "a\n");
}

#[test]
fn type_patterns_on_views_reborrow_the_view() {
    let source = "def main():\n    mut value: int64 | str = 1\n    view shared = value\n    match shared:\n        case int64 as n:\n            print(n)\n        case str as s:\n            print(s)\n";
    let encoded = assert_runs(source, "1\n");
    assert!(module_text(&encoded).contains("Reborrow"));
    let nested = "def main():\n    mut value: int64 | str = 1\n    view a = value\n    view b = a\n    match b:\n        case int64 as n:\n            print(n)\n        case str as s:\n            print(s)\n";
    assert_runs(nested, "1\n");
}

#[test]
fn match_mut_arms_track_trait_receiver_writes() {
    let generic = "trait Bump:\n    def bump(mut self)\nclass Counter:\n    total: int64\nimpl Bump for Counter:\n    def bump(mut self):\n        self.total = self.total + 1\nenum Slot[T]:\n    Filled(T)\n    Empty\ndef poke[T: Bump](slot: mut Slot[T]):\n    match mut slot:\n        case Slot.Filled(counter):\n            counter.bump()\n        case Slot.Empty:\n            pass\ndef main():\n    mut slot: Slot[Counter] = Slot.Filled(Counter(total=1))\n    poke(slot)\n    match slot:\n        case Slot.Filled(counter):\n            print(counter.total)\n        case Slot.Empty:\n            print(0)\n";
    let encoded = assert_runs(generic, "2\n");
    let text = module_text(&encoded);
    assert!(text.contains("TraitMember"));
    assert!(text.contains("match_writeback"));
    let union = "trait Bump:\n    def bump(mut self)\nclass A:\n    total: int64\nclass B:\n    total: int64\nimpl Bump for A:\n    def bump(mut self):\n        self.total = self.total + 1\nimpl Bump for B:\n    def bump(mut self):\n        self.total = self.total + 2\nenum Slot:\n    Filled(A | B)\n    Empty\ndef main():\n    mut slot = Slot.Filled(A(total=1))\n    match mut slot:\n        case Slot.Filled(counter):\n            counter.bump()\n        case Slot.Empty:\n            pass\n    match slot:\n        case Slot.Filled(counter):\n            print(counter)\n        case Slot.Empty:\n            print(0)\n";
    assert_runs(union, "A(total=2)\n");
}

#[test]
fn returns_inside_match_mut_arms_apply_writebacks() {
    let source = "class A:\n    total: int64\nenum Slot:\n    Filled(A)\n    Empty\ndef poke(slot: mut Slot) -> int64:\n    match mut slot:\n        case Slot.Filled(counter):\n            counter.total = 5\n            return counter.total\n        case Slot.Empty:\n            return 0\ndef main():\n    mut slot = Slot.Filled(A(total=1))\n    print(poke(slot))\n    match slot:\n        case Slot.Filled(counter):\n            print(counter.total)\n        case Slot.Empty:\n            print(0)\n";
    assert_runs(source, "5\n5\n");
}

#[test]
fn map_callbacks_returning_callables_keep_element_identities() {
    let source = "def inc(x: int64) -> int64:\n    return x + 1\ndef wrap(f: def(x: int64) -> int64) -> def(x: int64) -> int64:\n    return f\ndef main():\n    fs = [inc]\n    gs = fs.map(wrap)\n    print(gs[0](1))\n";
    assert_runs(source, "2\n");
}

#[test]
fn empty_callable_containers_join_and_copy() {
    let join = "def inc(x: int64) -> int64:\n    return x + 1\ndef main():\n    flag = true\n    mut fs: list[def(x: int64) -> int64] = []\n    if flag:\n        fs = [inc]\n    else:\n        fs = []\n    print(fs.len())\n    mut gs: list[def(x: int64) -> int64] = []\n    if flag:\n        gs = []\n    print(gs.len())\n";
    assert_runs(join, "1\n0\n");
    let copy = "def main():\n    fs: list[def(x: int64) -> int64] = []\n    gs = fs.copy()\n    print(gs.len())\n";
    assert_runs(copy, "0\n");
}

#[test]
fn unsigned_union_members_accept_integer_literals() {
    let source = "def main():\n    value: uint8 | None = 7\n    print(value)\n";
    let encoded = assert_runs(source, "7\n");
    assert!(module_text(&encoded).contains("UnionInject"));
}

#[test]
fn grouped_array_indexes_use_scalar_coordinates() {
    let source = "def main():\n    source: list[int32] = [1, 2, 3, 4]\n    mut vec = Array[int32].from_list(source, [4])\n    vec[(1)] = 9\n    print(vec[(1)])\n";
    assert_runs(source, "9\n");
}

#[test]
fn payload_type_arms_read_variant_projections() {
    let source = "enum Shape:\n    Circle(int64)\n    Square(int64)\ndef main():\n    shape = Shape.Circle(3)\n    match shape:\n        case Shape.Circle(int64 as radius):\n            print(radius)\n        case Shape.Square(side):\n            print(side)\n";
    let encoded = assert_runs(source, "3\n");
    assert!(module_text(&encoded).contains("__variant_payload_Circle_0"));
}

#[test]
fn tuple_patterns_mix_unit_variants_and_type_arms() {
    let source = "enum Shape:\n    Circle(int64)\n    Empty\ndef main():\n    shape = Shape.Empty\n    value: int64 | str = 1\n    pair = (shape, value)\n    match pair:\n        case (Shape.Empty, int64 as n):\n            print(n)\n        case _:\n            print(0)\n";
    assert_runs(source, "1\n");
}

#[test]
fn own_union_matches_take_payloads() {
    let source = "def main():\n    value: int64 | str = 1\n    match own value:\n        case int64 as n:\n            print(n)\n        case str as s:\n            print(s)\n";
    let encoded = assert_runs(source, "1\n");
    assert!(module_text(&encoded).contains("UnionTakePayload"));
}

#[test]
fn match_expressions_with_alternatives_and_guards() {
    let source = "def main():\n    value: int64 = 2\n    text = match value:\n        case 1 | 2: \"low\"\n        case _: \"high\"\n    print(text)\n    other: int64 | str = 1\n    label = match own other:\n        case int64 as n if n > 0: \"pos\"\n        case int64 as n: \"neg\"\n        case str as s: s\n    print(label)\n";
    assert_runs(source, "low\npos\n");
}

#[test]
fn mutable_self_views_return_through_receivers() {
    let source = "class Counter:\n    value: int64\n    def value_mut(mut self) -> view mut int64 from self:\n        return view mut self.value\ndef main():\n    mut counter = Counter(value=1)\n    view mut editable = counter.value_mut()\n    editable = 5\n    print(counter.value)\n";
    assert_runs(source, "5\n");
}

#[test]
fn bound_methods_keep_parameter_view_contracts() {
    let source = "class Pair:\n    left: str\nclass Picker:\n    label: str\n    def pick(self, pair: mut Pair) -> view mut str from pair:\n        return view mut pair.left\ndef main():\n    picker = Picker(label=\"p\")\n    mut pair = Pair(left=\"a\")\n    bound = picker.pick\n    view mut chosen = bound(pair)\n    chosen = \"z\"\n    print(pair.left)\n";
    let encoded = assert_runs(source, "z\n");
    assert!(module_text(&encoded).contains("Closure"));
}

#[test]
fn views_ending_inside_with_bodies_close_before_exit() {
    let source = "def main():\n    values = [1, 2]\n    view shared = values\n    with group = TaskGroup():\n        print(shared.len())\n    print(values.len())\n";
    assert_runs(source, "2\n2\n");
}

#[test]
fn nested_views_resolve_to_physical_sources() {
    let source = "class Inner:\n    value: int64\nclass Outer:\n    inner: Inner\ndef main():\n    outer = Outer(inner=Inner(value=1))\n    view a = outer.inner\n    view b = a.value\n    view c = b\n    print(c)\n    print(outer.inner.value)\n";
    assert_runs(source, "1\n1\n");
}

#[test]
fn view_last_uses_in_else_branches_and_guards() {
    let else_branch = "def main():\n    values = [1, 2]\n    view shared = values\n    flag = false\n    if flag:\n        print(1)\n    else:\n        print(shared.len())\n    print(values.len())\n";
    assert_runs(else_branch, "2\n2\n");
    let guard = "def main():\n    values = [1, 2]\n    view shared = values\n    n = 1\n    match n:\n        case 1 if shared.len() > 1:\n            print(\"big\")\n        case _:\n            print(\"other\")\n    print(values.len())\n";
    assert_runs(guard, "big\n2\n");
}

#[test]
fn nested_tuple_destructuring_registers_element_targets() {
    let source =
        "def main():\n    value = (1, (2, 3))\n    (a, (b, c)) = value\n    print(a + b + c)\n";
    assert_runs(source, "6\n");
}

#[test]
fn returned_view_callees_with_defaults_specialize_from_arguments() {
    let generic = "class Pair:\n    left: str\n    right: str\ndef pick[T](pair: Pair, flag: bool = true) -> view str from pair:\n    return view pair.left\ndef main():\n    pair = Pair(left=\"a\", right=\"b\")\n    view v = pick[int64](pair)\n    print(v)\n";
    assert_runs(generic, "a\n");
    let plain = "class Pair:\n    left: str\n    right: str\ndef pick(pair: Pair, flag: bool = true) -> view str from pair:\n    return view pair.left\ndef main():\n    pair = Pair(left=\"a\", right=\"b\")\n    view v = pick(pair)\n    print(v)\n";
    assert_runs(plain, "a\n");
}

#[test]
fn from_self_views_compose_with_returned_views() {
    let source = "class User:\n    name: str\n    def name_view(self) -> view str from self:\n        return view self.name\nclass Holder:\n    user: User\ndef pick_user(holder: Holder) -> view User from holder:\n    return view holder.user\ndef main():\n    holder = Holder(user=User(name=\"ada\"))\n    view v = pick_user(holder).name_view()\n    print(v)\n";
    assert_runs(source, "ada\n");
}

#[test]
fn forwarded_returned_views_project_through_child_fields() {
    let source = "class Pair:\n    left: str\n    right: str\nclass Holder:\n    pair: Pair\ndef pick_pair(holder: Holder) -> view Pair from holder:\n    return view holder.pair\ndef forward(holder: Holder) -> view str from holder:\n    return view pick_pair(holder).left\ndef main():\n    holder = Holder(pair=Pair(left=\"a\", right=\"b\"))\n    view chosen = forward(holder)\n    print(chosen)\n";
    assert_runs(source, "a\n");
}

#[test]
fn callable_view_footprints_cover_tuple_positions_and_skip_indirect_fields() {
    let tuple = "class Pair:\n    items: (str, str)\n    label: str\ndef pick(pair: Pair) -> view str from pair:\n    return view pair.label\ndef main():\n    pair = Pair(items=(\"a\", \"b\"), label=\"c\")\n    chooser: def(pair: Pair) -> view str from pair = pick\n    view head = chooser(pair)\n    print(head)\n";
    let mut encoded = assert_runs(tuple, "c\n");
    let projections = instructions_mut(function_mut(&mut encoded, "main"))
        .find_map(|instruction| instruction.get("BeginReturnedLoan").cloned())
        .expect("the callable call binds a returned loan")["projections"]
        .clone();
    assert_eq!(projections, json!(["items.0", "items.1", "label"]));
    let indirect = "class Node:\n    value: int64\n    next: indirect Option[Node] = Option.None\ndef pick(node: Node) -> view int64 from node:\n    return view node.value\ndef main():\n    node = Node(value=3)\n    chooser: def(node: Node) -> view int64 from node = pick\n    view head = chooser(node)\n    print(head)\n";
    assert_runs(indirect, "3\n");
}

#[test]
fn user_functions_shadow_builtin_iteration_helpers() {
    let source = "def zip(values: list[int64]) -> list[int64]:\n    return values.copy()\ndef main():\n    values = [1, 2]\n    for x in zip(values):\n        print(x)\n";
    assert_runs(source, "1\n2\n");
}

#[test]
fn indexing_and_membership_on_call_results() {
    let source = "def make() -> list[int64]:\n    return [4, 5]\ndef main():\n    print(make()[0])\n    print(1 in make())\n    print(0 < 1 in make())\n";
    assert_runs(source, "4\nfalse\nfalse\n");
}

#[test]
fn contextual_none_without_an_expected_type() {
    let source = "def main():\n    print(None)\n    x = None\n    print(x)\n";
    assert_runs(source, "\n\n");
}

#[test]
fn borrowed_capture_closures_are_recorded_and_callable() {
    let source = "def helper(f: def() -> int64) -> int64:\n    return f()\ndef main():\n    values = [1, 2]\n    counter = lambda [values]: values.len()\n    plain = lambda: 3\n    print(helper(plain))\n    print(counter())\n";
    assert_runs(source, "3\n2\n");
}

#[test]
fn empty_dictionary_callable_views_keep_declared_contracts() {
    let source = "def main():\n    d: dict[str, def(x: int64) -> int64] = {}\n    print(d.keys().len())\n    print(d.values().len())\n    print(d.items().len())\n    e = d.copy()\n    print(e.len())\n";
    assert_runs(source, "0\n0\n0\n0\n");
}

#[test]
fn operator_traits_on_call_results_dispatch_through_temporaries() {
    let source = "trait Neg[Out]:\n    def neg(self) -> Out\nclass Point:\n    x: int64\nimpl Neg[Point] for Point:\n    def neg(self) -> Point:\n        return Point(x=0 - self.x)\ndef make() -> Point:\n    return Point(x=3)\ndef main():\n    print((-make()).x)\n";
    let encoded = assert_runs(source, "-3\n");
    assert!(module_text(&encoded).contains("TraitMember"));
}

#[test]
fn wildcard_arms_in_match_mut_return_without_a_writeback() {
    let source = "class A:\n    total: int64\nenum Slot:\n    Filled(A)\n    Empty\ndef poke(slot: mut Slot) -> int64:\n    match mut slot:\n        case Slot.Filled(counter):\n            counter.total = 5\n            return 1\n        case _:\n            return 0\ndef main():\n    mut slot = Slot.Filled(A(total=1))\n    print(poke(slot))\n    mut empty = Slot.Empty\n    print(poke(empty))\n";
    assert_runs(source, "1\n0\n");
}

#[test]
fn match_expression_guards_release_pattern_views() {
    let source = "def main():\n    other: int64 | str = 1\n    label = match other:\n        case int64 as n if n > 0: n\n        case int64 as n: 0 - n\n        case str as s: s.len()\n    print(label)\n";
    assert_runs(source, "1\n");
}

#[test]
fn duration_literal_receivers_type_as_durations() {
    let source = "def main():\n    print(1s.to_ms())\n    total = 1s + 2s\n    print(total)\n";
    assert_runs(source, "1000.0\n3000ms\n");
}

#[test]
fn callables_nested_beyond_the_identity_walk_depth_fail_closed() {
    // The validator's callable-identity walk stops eight class levels deep;
    // a checked program storing a callable deeper than that is rejected
    // rather than trusted (recorded as a suspected defect, see the coverage
    // report), so this pins the current fail-closed boundary.
    let source = "type Reader = Callable[def() -> int64]\nclass C1:\n    f: Reader\nclass C2:\n    c: C1\nclass C3:\n    c: C2\nclass C4:\n    c: C3\nclass C5:\n    c: C4\nclass C6:\n    c: C5\nclass C7:\n    c: C6\nclass C8:\n    c: C7\nclass C9:\n    c: C8\nclass C10:\n    c: C9\ndef use(value: C10) -> int64:\n    return value.c.c.c.c.c.c.c.c.c.f()\ndef main():\n    print(use(C10(c=C9(c=C8(c=C7(c=C6(c=C5(c=C4(c=C3(c=C2(c=C1(f=Reader(lambda: 5)))))))))))))\n";
    let error = run_source(source).expect_err("the nested callable has no recorded identity");
    assert!(
        error
            .message
            .contains("invalid MIR indirect call in `use` has no authoritative callable contract"),
        "{}",
        error.message
    );
}

// ---------------------------------------------------------------------------
// Validator branches reached by forging a lowered module.
// ---------------------------------------------------------------------------

const VIEW_SOURCE: &str = "class User:\n    name: str\n    age: int64\n\ndef name(user: User) -> view str from user:\n    return view user.name\n\ndef main():\n    user = User(name=\"ada\", age=3)\n    view current = name(user)\n    print(current)\n";

const KEYWORD_PARAM: &str = "type Adder = Callable[def(a: int64, *, b: int64 = ...) -> int64]\ndef add(a: int64, *, b: int64 = 1) -> int64:\n    return a + b\ndef call(adder: Adder) -> int64:\n    return adder(1)\ndef main():\n    print(call(Adder(add)))\n";

const VIEW_PARAM: &str = "class Pair:\n    left: str\n    count: int64\n\ndef choose(chooser: def(pair: Pair) -> view str from pair, pair: Pair) -> int64:\n    view head = chooser(pair)\n    return head.len()\n\ndef main():\n    print(1)\n";

const FIELD_VIEW: &str = "class User:\n    name: str\n    age: int64\ndef main():\n    user = User(name=\"ada\", age=3)\n    view age = user.age\n    print(age)\n";

const PAYLOAD_TYPE_ARM: &str = "enum Shape:\n    Circle(int64)\n    Square(int64)\ndef main():\n    shape = Shape.Circle(3)\n    match shape:\n        case Shape.Circle(int64 as radius):\n            print(radius)\n        case Shape.Square(side):\n            print(side)\n";

const NARROWED_VIEW: &str = "def main():\n    mut value: int64 | None = 7\n    view shared = value\n    if shared is not None:\n        total = shared + 1\n        print(total)\n";

const OWN_UNION: &str = "def main():\n    value: int64 | str = 1\n    match own value:\n        case int64 as n:\n            print(n)\n        case str as s:\n            print(s)\n";

const TWO_UNIONS: &str = "def main():\n    value: uint8 | None = 7\n    other: str | None = \"a\"\n    print(value)\n    print(other)\n";

const CLOSURE_CAPTURE: &str = "def helper(f: def() -> int64) -> int64:\n    return f()\ndef main():\n    values = [1, 2]\n    counter = lambda [values]: values.len()\n    plain = lambda: 3\n    print(helper(plain))\n    print(counter())\n";

const ENUM_MATCH: &str = "enum Shape:\n    Circle(int64)\n    Square(int64)\ndef main():\n    shape = Shape.Circle(3)\n    match shape:\n        case Shape.Circle(radius):\n            print(radius)\n        case Shape.Square(side):\n            print(side)\n";

fn indirect_call_mut(function: &mut Value) -> &mut Value {
    rvalues_mut(function, "Call")
        .find(|call| call["callee"].get("Value").is_some())
        .expect("an indirect call should exist")
}

#[test]
fn cyclic_reborrow_descriptors_are_rejected() {
    let mut encoded = encode(VIEW_SOURCE);
    let name = function_mut(&mut encoded, "name");
    let entry = name["entry"].as_str().unwrap().to_owned();
    for block in name["blocks"].as_array_mut().unwrap() {
        if block["label"] == entry {
            let instructions = block["instructions"].as_array_mut().unwrap();
            instructions.insert(
                0,
                json!({"Reborrow": {"loan": "a", "parent": "b", "projection": "", "mutable": false}}),
            );
            instructions.insert(
                1,
                json!({"Reborrow": {"loan": "b", "parent": "a", "projection": "", "mutable": false}}),
            );
        }
    }
    for instruction in instructions_mut(name) {
        if let Some(ret) = instruction.get_mut("ReturnLoan") {
            ret["loan"] = json!("a");
        }
    }
    assert_rejected(
        encoded,
        "returned-view contract contains a cyclic loan descriptor rooted at `a`",
    );
}

#[test]
fn positional_arguments_skip_keyword_only_contract_slots() {
    let mut encoded = encode(KEYWORD_PARAM);
    let call = indirect_call_mut(function_mut(&mut encoded, "call"));
    call["args"].as_array_mut().unwrap().push(json!({
        "name": null, "value": {"Int": 2}, "writeback_place": null
    }));
    assert_rejected(
        encoded,
        "invalid MIR indirect call from `call` has too many positional arguments",
    );
}

#[test]
fn indirect_returned_view_origin_beyond_the_contract_is_rejected() {
    let mut encoded = encode(VIEW_PARAM);
    rewrite_returned_views(function_mut(&mut encoded, "choose"), &|view| {
        view["origin"] = json!(5);
    });
    assert_rejected(
        encoded,
        "invalid MIR indirect call in `choose` names returned-view origin 5 beyond its contract",
    );
}

#[test]
fn indirect_returned_view_origin_binds_by_contract_name() {
    let mut encoded = encode(VIEW_PARAM);
    indirect_call_mut(function_mut(&mut encoded, "choose"))["args"][0]["name"] = json!("pair");
    let mir = decode(encoded);
    let output = run_mir(&mir).expect("named origin arguments bind the returned view");
    assert_eq!(output.stdout, "1\n");
    emit_host_native_object(&mir).expect("the direct backend accepts named origin arguments");
}

#[test]
fn indirect_returned_view_origin_must_be_a_place() {
    let mut encoded = encode(VIEW_PARAM);
    indirect_call_mut(function_mut(&mut encoded, "choose"))["args"][0]["value"] = json!({"Int": 1});
    assert_rejected(
        encoded,
        "invalid MIR indirect call in `choose` binds its returned-view origin to a non-place operand",
    );
}

#[test]
fn indirect_mutable_returned_views_need_a_matching_writeback() {
    let mut encoded = encode(VIEW_PARAM);
    rewrite_returned_views(function_mut(&mut encoded, "choose"), &|view| {
        view["mutable"] = json!(true);
    });
    assert_rejected(
        encoded,
        "invalid MIR indirect call in `choose` returns a mutable view of `pair` without a matching mutable writeback place",
    );
}

#[test]
fn places_cannot_project_through_returned_view_roots() {
    let mut encoded = encode(FIELD_VIEW);
    local_type_mut(function_mut(&mut encoded, "main"), "user")["ty"] = json!({
        "ReturnedView": {"mutable": false, "origin": 0, "pointee": {"Named": ["User", []]}}
    });
    assert_rejected(
        encoded,
        "invalid MIR place `user.age` projects through a returned-view contract",
    );
}

#[test]
fn enum_payload_projections_name_declared_variants() {
    let mut encoded = encode(PAYLOAD_TYPE_ARM);
    rewrite_strings(function_mut(&mut encoded, "main"), &|text| {
        text.contains("__variant_payload_Circle_0")
            .then(|| text.replace("__variant_payload_Circle_0", "__variant_payload_Nope_0"))
    });
    assert_rejected(
        encoded,
        "invalid MIR enum payload projection `__variant_payload_Nope_0` in `shape.__variant_payload_Nope_0` names unknown variant `Nope`",
    );
}

#[test]
fn union_payload_projections_stay_within_the_member_list() {
    let mut encoded = encode(NARROWED_VIEW);
    rewrite_strings(function_mut(&mut encoded, "main"), &|text| {
        text.contains("__union_payload_0")
            .then(|| text.replace("__union_payload_0", "__union_payload_9"))
    });
    assert_rejected(
        encoded,
        "invalid MIR union payload projection `__union_payload_9` in `value.__union_payload_9` in `main` is out of bounds",
    );
}

#[test]
fn union_tag_tests_require_union_types_and_valid_members() {
    let mut encoded = encode(OWN_UNION);
    for test in rvalues_mut(function_mut(&mut encoded, "main"), "UnionTagTest") {
        test["union_type"] = json!({"Named": ["int64", []]});
    }
    assert_rejected(encoded, "invalid MIR union tag test requires a union type");

    let mut encoded = encode(OWN_UNION);
    for test in rvalues_mut(function_mut(&mut encoded, "main"), "UnionTagTest") {
        test["member_index"] = json!(9);
    }
    assert_rejected(
        encoded,
        "invalid MIR union tag test member index is out of bounds",
    );
}

#[test]
fn union_payload_takes_require_union_types() {
    let mut encoded = encode(OWN_UNION);
    for take in rvalues_mut(function_mut(&mut encoded, "main"), "UnionTakePayload") {
        take["union_type"] = json!({"Named": ["int64", []]});
    }
    assert_rejected(
        encoded,
        "invalid MIR union payload take requires a union type",
    );
}

fn uint8_injection_mut(main: &mut Value) -> &mut Value {
    rvalues_mut(main, "UnionInject")
        .find(|inject| inject["member_type"] == json!({"Named": ["uint8", []]}))
        .expect("the `uint8 | None` injection exists")
}

#[test]
fn integer_literal_injections_respect_unsigned_member_bounds() {
    let mut encoded = encode(TWO_UNIONS);
    uint8_injection_mut(function_mut(&mut encoded, "main"))["value"] = json!({"Int": 7});
    let mir = decode(encoded);
    let output = run_mir(&mir).expect("an in-range literal injects directly");
    assert_eq!(output.stdout, "7\na\n");
    emit_host_native_object(&mir).expect("the direct backend accepts the literal injection");

    let mut encoded = encode(TWO_UNIONS);
    uint8_injection_mut(function_mut(&mut encoded, "main"))["value"] = json!({"Int": 300});
    assert_rejected(
        encoded,
        "invalid MIR union injection operand does not have its selected member type",
    );
}

#[test]
fn integer_literals_cannot_inject_into_non_integer_members() {
    let mut encoded = encode(TWO_UNIONS);
    let main = function_mut(&mut encoded, "main");
    let text_union = rvalues_mut(main, "UnionInject")
        .find(|inject| inject["member_type"] == json!({"Named": ["str", []]}))
        .expect("the `str | None` injection exists")["union_type"]
        .clone();
    let inject = uint8_injection_mut(main);
    inject["value"] = json!({"Int": 7});
    inject["member_type"] = json!({"Named": ["str", []]});
    inject["union_type"] = text_union;
    assert_rejected(
        encoded,
        "invalid MIR union injection operand does not have its selected member type",
    );
}

#[test]
fn variant_payloads_name_declared_variants() {
    let mut encoded = encode(ENUM_MATCH);
    for payload in rvalues_mut(function_mut(&mut encoded, "main"), "VariantPayload") {
        payload["variant_name"] = json!("Nope");
    }
    assert_rejected(
        encoded,
        "invalid MIR variant payload in `main` names unknown variant `Nope` of `Shape`",
    );
}

#[test]
fn untyped_variant_payloads_need_an_active_matching_proof() {
    let mut encoded = encode(ENUM_MATCH);
    let main = function_mut(&mut encoded, "main");
    let mut scrutinee = None;
    for payload in rvalues_mut(main, "VariantPayload") {
        payload["variant_name"] = json!("Nope");
        scrutinee = payload["scrutinee"]
            .get("Place")
            .or_else(|| payload["scrutinee"].get("MovePlace"))
            .and_then(Value::as_str)
            .map(str::to_owned);
    }
    let scrutinee = scrutinee.expect("the arm reads a variant payload");
    main["local_types"]
        .as_array_mut()
        .unwrap()
        .retain(|local| local["name"] != scrutinee);
    assert_rejected(
        encoded,
        &format!(
            "invalid MIR variant payload from `{scrutinee}` in `main` has no active matching variant proof"
        ),
    );
}

#[test]
fn closures_must_name_a_declared_function() {
    let mut encoded = encode(CLOSURE_CAPTURE);
    for closure in rvalues_mut(function_mut(&mut encoded, "main"), "Closure") {
        closure["function"] = json!("ghost");
    }
    assert_rejected(
        encoded,
        "invalid MIR closure in `main` names unknown function `ghost`",
    );
}

#[test]
fn closure_view_results_need_a_declaration_contract() {
    let mut encoded = encode(CLOSURE_CAPTURE);
    let main = function_mut(&mut encoded, "main");
    let view = json!({
        "ReturnedView": {"mutable": false, "origin": 0, "pointee": {"Named": ["int64", []]}}
    });
    for closure in rvalues_mut(main, "Closure") {
        closure["signature"]["Closure"]["return_type"] = view.clone();
    }
    local_type_mut(main, "counter")["ty"]["Closure"]["return_type"] = view;
    assert_rejected(encoded, "changes its declared callable contract");
}

#[test]
fn borrowed_closure_captures_cannot_cross_call_boundaries() {
    let mut encoded = encode(CLOSURE_CAPTURE);
    let call = rvalues_mut(function_mut(&mut encoded, "main"), "Call")
        .find(|call| call["callee"] == json!({"Name": "helper"}))
        .expect("`helper` is called");
    call["args"][0]["value"] = json!({"Place": "counter"});
    assert_rejected(
        encoded,
        "invalid MIR call from `main` passes borrowed closure capture `counter` across a call boundary without owned authority",
    );
}

#[test]
fn returned_loans_cannot_return_arm_local_payload_projections() {
    let mut encoded = encode(VIEW_SOURCE);
    for instruction in instructions_mut(function_mut(&mut encoded, "name")) {
        if let Some(ret) = instruction.get_mut("ReturnLoan") {
            ret["loan"] = json!("user.__variant_payload_Some_0");
        }
    }
    assert_rejected(
        encoded,
        "invalid returned MIR loan `user.__variant_payload_Some_0` in `name` returns an arm-local payload projection",
    );
}

#[test]
fn member_calls_on_untyped_locals_are_tolerated() {
    let mut encoded = encode("def main():\n    values = [1]\n    print(values.len())\n");
    function_mut(&mut encoded, "main")["local_types"]
        .as_array_mut()
        .unwrap()
        .retain(|local| local["name"] != "values");
    let mir = decode(encoded);
    let output = run_mir(&mir).expect("an untyped receiver falls back to runtime dispatch");
    assert_eq!(output.stdout, "1\n");
    emit_host_native_object(&mir).expect("the direct backend accepts the untyped receiver");
}
