//! Regressions for checker diagnostics, view forwarding, and shared MIR
//! validation paths that earlier suites did not pin. Forged-MIR cases assert
//! that the interpreter and the direct backend reject the same module.

use aura_compiler::mir::ownership::strip_ownership;
use aura_compiler::runtime_ops::RuntimeOp;
use aura_compiler::{
    check_source, emit_host_native_object, lower_source_to_mir, run_mir, run_source, MirModule,
};
use serde_json::{json, Value};

fn encode(source: &str) -> Value {
    // A forgery edits MIR that states no ownership, which each boundary
    // then fills in.
    let mut mir = lower_source_to_mir(source).expect("source should lower");
    strip_ownership(&mut mir);
    serde_json::to_value(mir).unwrap()
}

fn main_instructions(encoded: &mut Value) -> impl Iterator<Item = &mut Value> {
    encoded["functions"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .find(|function| function["name"] == "main")
        .expect("main should exist")["blocks"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .flat_map(|block| block["instructions"].as_array_mut().unwrap().iter_mut())
        .filter(|instruction| instruction.is_object())
}

/// The `Member` or `Runtime` payload of the first member call to `field`
/// in `main`; a resolved runtime operation names the member it was
/// resolved from.
fn member_callee_mut<'a>(encoded: &'a mut Value, field: &str) -> &'a mut Value {
    main_instructions(encoded)
        .find_map(|instruction| {
            let callee = instruction.pointer_mut("/Assign/value/Call/callee")?;
            if callee
                .get("Member")
                .is_some_and(|member| member["field"] == field)
            {
                return Some(&mut callee["Member"]);
            }
            let op: RuntimeOp =
                serde_json::from_value(callee.get("Runtime")?["op"].clone()).ok()?;
            (op.name() == field).then(|| &mut callee["Runtime"])
        })
        .unwrap_or_else(|| panic!("member call `{field}` not found"))
}

fn assert_rejected(encoded: Value, expected: &str) {
    let mir: MirModule = serde_json::from_value(encoded).expect("forged MIR should deserialize");
    let interpreted = run_mir(&mir).expect_err("the interpreter must reject the forged module");
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

fn rejects(source: &str, code: &str, expected: &str) {
    let error = check_source(source).expect_err(source);
    assert_eq!(error.code, code, "{}\n{source}", error.message);
    assert!(
        error.message.contains(expected),
        "diagnostic `{}` should mention `{expected}` for:\n{source}",
        error.message
    );
}

/// Runs `source` on the MIR interpreter and checks that the direct backend
/// also accepts the lowered module.
fn runs_on_both_backends(source: &str, expected: &str) {
    let output = run_source(source)
        .unwrap_or_else(|error| panic!("{source}\nfailed with {}: {}", error.code, error.message));
    assert_eq!(output.stdout, expected, "{source}");
    let mir = lower_source_to_mir(source).expect("source should lower");
    let object = emit_host_native_object(&mir)
        .unwrap_or_else(|error| panic!("{source}\nnative emission failed: {error}"));
    assert!(!object.is_empty());
}

const SORTING: &str = "def main():\n    mut values = [3, 1, 2]\n    mut others = [9, 8]\n    values.append(4)\n    others.append(7)\n    print(values)\n    print(others)\n    print(values.len())\n";

#[test]
fn forged_member_calls_cannot_redirect_or_drop_receiver_writeback() {
    let mut encoded = encode(SORTING);
    member_callee_mut(&mut encoded, "append")["receiver_place"] = json!("others");
    assert_rejected(
        encoded,
        "invalid MIR member call `append` in `main` redirects receiver `values` to unrelated writeback place `others`",
    );

    let mut encoded = encode(SORTING);
    member_callee_mut(&mut encoded, "len")["receiver_place"] = json!("others");
    assert_rejected(
        encoded,
        "invalid MIR member call `len` in `main` redirects receiver `values` to unrelated writeback place `others`",
    );

    let mut encoded = encode(SORTING);
    member_callee_mut(&mut encoded, "append")["receiver_place"] = json!(null);
    assert_rejected(
        encoded,
        "invalid MIR member call `append` in `main` requires a mutable receiver writeback place",
    );
}

const HOLDER: &str = "class Holder:\n    name: str\n\n    def name_view(self) -> view str from self:\n        return view self.name\n\n";

#[test]
fn view_returns_forward_through_method_and_trait_calls() {
    runs_on_both_backends(
        &format!("{HOLDER}def forward(h: Holder) -> view str from h:\n    return view h.name_view()\n\ndef main():\n    h = Holder(name=\"aura\")\n    print(forward(h))\n"),
        "aura\n",
    );
    runs_on_both_backends(
        "trait Named:\n    def label(self) -> view str from self\n\nclass Holder:\n    name: str\n\nimpl Named for Holder:\n    def label(self) -> view str from self:\n        return view self.name\n\ndef forward(h: Holder) -> view str from h:\n    return view h.label()\n\ndef main():\n    h = Holder(name=\"trait\")\n    print(forward(h))\n",
        "trait\n",
    );
}

#[test]
fn checker_rejects_view_method_values_and_conflicting_generic_arguments() {
    rejects(
        &format!("{HOLDER}def main():\n    h = Holder(name=\"aura\")\n    f = h.name_view\n"),
        "AU3010",
        "view-returning method `Holder.name_view` cannot become a bound method value",
    );
    rejects(
        "def same[T](a: T, b: T) -> T:\n    return a\n\ndef main():\n    print(same(1, \"x\"))\n",
        "AU2002",
        "conflicting inferred types for `T`: `int64` and `str`",
    );
    rejects(
        "class Point:\n    x: int64\n    y: int64\n\ndef main():\n    value: Point | None = Point(x=1, y=2)\n    match value:\n        case Point(x=1):\n            print(1)\n        case Point(x=1):\n            print(2)\n        case _:\n            print(3)\n",
        "AU2013",
        "duplicate or unreachable type arm",
    );
}

const LOAN_CLOSURE: &str =
    "enum Holder:\n    Fn(def() -> int64)\n\ndef take(items: list[def() -> int64]):\n    pass\n\ndef take_holder(h: Holder):\n    pass\n\ndef main():\n    values = [1]\n    read = lambda [values]: values.len()\n";

#[test]
fn loan_closures_cannot_escape_through_nested_or_argument_collections() {
    for tail in [
        "    take([read])\n",
        "    items = ([read], 1)\n",
        "    items = [[read]]\n",
        "    items = [{read}]\n",
        "    items = [{\"a\": read}]\n",
    ] {
        rejects(
            &format!("{LOAN_CLOSURE}{tail}"),
            "AU3010",
            "a closure containing a live view cannot be stored in a collection",
        );
    }
    for tail in [
        "    items = [Holder.Fn(read)]\n",
        "    take_holder(Holder.Fn(read))\n",
    ] {
        rejects(
            &format!("{LOAN_CLOSURE}{tail}"),
            "AU3010",
            "a closure containing a live view cannot be stored in an enum payload",
        );
    }
}

#[test]
fn callable_lists_map_into_callable_results_and_trait_methods_bind() {
    runs_on_both_backends(
        "def inc(x: int64) -> int64:\n    return x + 1\n\ndef wrap(f: def(x: int64) -> int64) -> def(x: int64) -> int64:\n    return f\n\ndef main():\n    fs = [inc]\n    gs = fs.map(wrap)\n    match gs.get(0):\n        case Lookup.Found(g):\n            print(g(1))\n        case Lookup.Missing:\n            print(0)\n",
        "2\n",
    );
    runs_on_both_backends(
        "trait Named:\n    def label(self) -> str\n\nclass Holder:\n    name: str\n\nimpl Named for Holder:\n    def label(self) -> str:\n        return self.name.clone()\n\ndef main():\n    h = Holder(name=\"bound\")\n    f = h.label\n    print(f())\n",
        "bound\n",
    );
    runs_on_both_backends(
        "trait Counter:\n    def bump(mut self) -> int64\n\nclass Tally:\n    count: int64\n\nimpl Counter for Tally:\n    def bump(mut self) -> int64:\n        self.count += 1\n        return self.count\n\ndef main():\n    mut t = Tally(count=0)\n    t.bump()\n    print(t.bump())\n    print(t.count)\n",
        "2\n2\n",
    );
}

#[test]
fn view_returns_project_through_method_trait_and_generic_calls() {
    runs_on_both_backends(
        "class Holder:\n    name: str\n\n    def me(self) -> view Holder from self:\n        return view self\n\ndef outer(h: Holder) -> view str from h:\n    return view h.me().name\n\ndef main():\n    h = Holder(name=\"nested\")\n    print(outer(h))\n",
        "nested\n",
    );
    runs_on_both_backends(
        "class Pair:\n    left: str\n    right: str\n\nclass Holder:\n    pair: Pair\n\n    def inner(self) -> view Pair from self:\n        return view self.pair\n\ndef outer(h: Holder) -> view str from h:\n    return view h.inner().left\n\ndef main():\n    h = Holder(pair=Pair(left=\"l\", right=\"r\"))\n    print(outer(h))\n",
        "l\n",
    );
    runs_on_both_backends(
        "trait Wrapped:\n    def inner(self) -> view Pair from self\n\nclass Pair:\n    left: str\n    right: str\n\nclass Holder:\n    pair: Pair\n\nimpl Wrapped for Holder:\n    def inner(self) -> view Pair from self:\n        return view self.pair\n\ndef outer(h: Holder) -> view str from h:\n    return view h.inner().right\n\ndef main():\n    h = Holder(pair=Pair(left=\"l\", right=\"r\"))\n    print(outer(h))\n",
        "r\n",
    );
    runs_on_both_backends(
        "trait Wrapped:\n    def inner(self) -> view Pair from self\n\nclass Pair:\n    left: str\n    right: str\n\nclass Holder:\n    pair: Pair\n\nimpl Wrapped for Holder:\n    def inner(self) -> view Pair from self:\n        return view self.pair\n\ndef outer[T: Wrapped](h: T) -> view str from h:\n    return view h.inner().right\n\ndef main():\n    h = Holder(pair=Pair(left=\"l\", right=\"r\"))\n    print(outer(h))\n",
        "r\n",
    );
}

#[test]
fn mutable_closures_capture_returned_view_slots() {
    runs_on_both_backends(
        "class Choice:\n    left: int64\n    right: int64\n\ndef choose(choice: mut Choice, left: bool) -> view mut int64 from choice:\n    if left:\n        return view mut choice.left\n    return view mut choice.right\n\ndef add(slot: mut int64, amount: int64):\n    slot += amount\n\ndef main():\n    mut c = Choice(left=1, right=2)\n    view mut slot = choose(c, false)\n    mut bump: def(int64) -> None = lambda [mut slot] amount: add(slot, amount)\n    bump(10)\n    bump(5)\n    print(c.right)\n    print(c.left)\n",
        "17\n1\n",
    );
    runs_on_both_backends(
        "def first(values: mut list[int64], index: int64) -> view mut int64 from values:\n    return view mut values[index]\n\ndef add(slot: mut int64, amount: int64):\n    slot += amount\n\ndef main():\n    mut values = [1, 2, 3]\n    mut i = 1\n    view mut slot = first(values, i)\n    mut bump: def(int64) -> None = lambda [mut slot] amount: add(slot, amount)\n    bump(10)\n    print(values)\n",
        "[1, 12, 3]\n",
    );
}

#[test]
fn views_of_string_indexes_have_no_stable_identity() {
    rejects(
        "def pick(s: str) -> view str from s:\n    return view s[0]\n\ndef main():\n    print(pick(\"ab\"))\n",
        "AU3004",
        "indexed collection elements do not have stable view identity; only fixed tuple positions are supported",
    );
    rejects(
        "def main():\n    s = \"abc\"\n    view c = s[0]\n    print(c)\n",
        "AU3004",
        "indexed collection elements do not have stable view identity; only fixed tuple positions are supported",
    );
}

#[test]
fn task_targets_reject_value_specializations() {
    rejects(
        "def job[T](value: own T) -> T:\n    return value\n\ndef main():\n    with TaskGroup() as group:\n        task = group.start(job[(1, 2)], 3)\n",
        "AU2002",
        "function specialization expects type arguments",
    );
    rejects(
        "def job[T](value: own T) -> T:\n    return value\n\ndef main():\n    with TaskGroup() as group:\n        task = group.start(job[(int64, 2)], 3)\n",
        "AU2002",
        "function specialization expects type arguments",
    );
}

#[test]
fn conditional_collection_and_float_literals_join_branch_types() {
    runs_on_both_backends(
        "def main():\n    flag = true\n    s = {1, 2} if flag else {3, 4}\n    d = {\"a\": 1} if flag else {\"b\": 2}\n    print(s)\n    print(d)\n",
        "{1, 2}\n{a: 1}\n",
    );
    runs_on_both_backends(
        "def main():\n    flag = false\n    y: float32 = 2.5\n    x = 1.5 if flag else y\n    z = y if flag else 0.5\n    print(x)\n    print(z)\n",
        "2.5\n0.5\n",
    );
}

#[test]
fn patterns_bind_through_views_temporaries_and_owned_payloads() {
    runs_on_both_backends(
        "class Pair:\n    left: str\n    right: int64\n\nclass Holder:\n    pair: Pair\n\ndef main():\n    h = Holder(pair=Pair(left=\"l\", right=2))\n    view v = h.pair\n    match v:\n        case Pair(left=l, right=r):\n            print(l)\n            print(r)\n",
        "l\n2\n",
    );
    runs_on_both_backends(
        "class Point:\n    x: int64\n    y: int64\n\ndef main():\n    match Point(x=1, y=2):\n        case Point(x=1, y=b):\n            print(b)\n        case _:\n            print(0)\n",
        "2\n",
    );
    runs_on_both_backends(
        "def main():\n    match [1, 2, 3]:\n        case [first, *rest]:\n            print(first)\n            print(rest)\n        case []:\n            print(0)\n",
        "1\n[2, 3]\n",
    );
    runs_on_both_backends(
        "class Point:\n    x: int64\n    y: int64\n\ndef make(flag: bool) -> (int64 | str, Point):\n    if flag:\n        return (1, Point(x=1, y=2))\n    return (\"s\", Point(x=3, y=4))\n\ndef main():\n    value = make(true)\n    match own value:\n        case (int64 as n, Point(x=a, y=b)):\n            print(n + a + b)\n        case (str as s, p):\n            print(s)\n    match own make(false):\n        case (int64 as n, Point(x=a, y=b)):\n            print(n + a + b)\n        case (str as s, Point(x=c, y=d)):\n            print(s)\n            print(c + d)\n",
        "4\ns\n7\n",
    );
    runs_on_both_backends(
        "class Point:\n    x: int64\n    y: int64\n\ndef make(flag: bool) -> Point | str:\n    if flag:\n        return Point(x=1, y=2)\n    return \"s\"\n\ndef main():\n    match own make(true):\n        case Point(x=a, y=b):\n            print(a + b)\n        case str as s:\n            print(s)\n    match make(true):\n        case Point(x=1, y=b):\n            print(b)\n        case _:\n            print(0)\n",
        "3\n2\n",
    );
}

#[test]
fn specialized_function_and_method_values_lower_on_both_backends() {
    runs_on_both_backends(
        "def ident[T](value: own T) -> T:\n    return value\n\ndef main():\n    f = ident[int64]\n    print(f(3))\n    print(ident[str](\"s\"))\n",
        "3\ns\n",
    );
    runs_on_both_backends(
        "class Util:\n    base: int64\n\n    def scale[T](self, value: own T) -> T:\n        return value\n\ndef main():\n    u = Util(base=1)\n    f = u.scale[int64]\n    print(f(4))\n",
        "4\n",
    );
}

const COUNTERS: &str = "class Counter:\n    n: int64\n\n    def bump(mut self):\n        self.n += 1\n\ntrait Bump:\n    def poke(mut self)\n\nimpl Bump for Counter:\n    def poke(mut self):\n        self.n += 2\n\ndef go[T: Bump](x: mut T, y: mut T):\n    x.poke()\n    y.poke()\n\nenum Slot[T]:\n    Empty\n    Full(T)\n\ndef main():\n    mut a = Counter(n=0)\n    mut b = Counter(n=0)\n    a.bump()\n    b.bump()\n    go(a, b)\n    s = Slot.Full(1)\n    print(a.n)\n";

fn function_mut<'a>(encoded: &'a mut Value, name: &str) -> &'a mut Value {
    encoded["functions"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .find(|function| function["name"] == name)
        .unwrap_or_else(|| panic!("function `{name}` should exist"))
}

fn callee_in_mut<'a>(
    encoded: &'a mut Value,
    function: &str,
    kind: &str,
    field: &str,
) -> &'a mut Value {
    function_mut(encoded, function)["blocks"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .flat_map(|block| block["instructions"].as_array_mut().unwrap().iter_mut())
        .filter(|instruction| instruction.is_object())
        .find(|instruction| {
            instruction["Assign"]["value"]["Call"]["callee"][kind]["field"] == field
        })
        .map(|instruction| &mut instruction["Assign"]["value"]["Call"]["callee"][kind])
        .unwrap_or_else(|| panic!("{kind} call `{field}` not found in `{function}`"))
}

#[test]
fn forged_class_and_trait_receivers_cannot_redirect_or_drop_writeback() {
    for (function, kind, field, other) in [
        ("main", "Member", "bump", "b"),
        ("go", "TraitMember", "poke", "y"),
    ] {
        let receiver = if function == "main" { "a" } else { "x" };
        let mut encoded = encode(COUNTERS);
        callee_in_mut(&mut encoded, function, kind, field)["receiver_place"] = json!(other);
        assert_rejected(
            encoded,
            &format!(
                "invalid MIR member call `{field}` in `{function}` redirects receiver `{receiver}` to unrelated writeback place `{other}`"
            ),
        );

        let mut encoded = encode(COUNTERS);
        callee_in_mut(&mut encoded, function, kind, field)["receiver_place"] = json!(null);
        assert_rejected(
            encoded,
            &format!(
                "invalid MIR member call `{field}` in `{function}` requires a mutable receiver writeback place"
            ),
        );
    }
}

#[test]
fn forged_class_receiver_constants_are_rejected_by_both_backends() {
    let mut encoded = encode(COUNTERS);
    callee_in_mut(&mut encoded, "main", "Member", "bump")["object"] = json!({"Int": 1});
    let mir: MirModule = serde_json::from_value(encoded).expect("forged MIR should deserialize");
    let interpreted = run_mir(&mir).expect_err("the interpreter must reject a constant receiver");
    assert_eq!(
        interpreted.message,
        "unsupported MIR member call `bump` on `1`"
    );
    let native = emit_host_native_object(&mir).expect_err("native emission must reject it too");
    assert_eq!(
        native,
        "direct backend does not support member call `.bump` on `int64`"
    );
}

#[test]
fn callable_lists_map_through_local_callbacks_and_copies() {
    runs_on_both_backends(
        "def inc(x: int64) -> int64:\n    return x + 1\n\ndef wrap(f: def(x: int64) -> int64) -> def(x: int64) -> int64:\n    return f\n\ndef main():\n    fs = [inc, inc]\n    w = wrap\n    gs = fs.map(w)\n    total = 0\n    for g in gs:\n        print(g(1))\n    hs = fs.copy()\n    match hs.get(1):\n        case Lookup.Found(h):\n            print(h(5))\n        case Lookup.Missing:\n            print(0)\n",
        "2\n2\n6\n",
    );
}
