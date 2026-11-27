//! Regression tests for ownership, borrowing and narrowing rules that the
//! checker must enforce before MIR or native code runs a program.

use crate::check_source;

/// Checks `source` and returns the diagnostic code it is rejected with.
fn rejected_code(name: &str, source: &str) -> String {
    match check_source(source) {
        Ok(_) => panic!("{name} should be rejected:\n{source}"),
        Err(error) => format!("{} {}", error.code, error.message),
    }
}

fn assert_rejected(cases: &[(&str, &str, &str)]) {
    for (name, code, source) in cases {
        let actual = rejected_code(name, source);
        assert!(actual.starts_with(code), "{name}: {actual}");
    }
}

fn assert_accepted(cases: &[(&str, &str)]) {
    for (name, source) in cases {
        if let Err(error) = check_source(source) {
            panic!("{name} should check: {error:?}\n{source}");
        }
    }
}

const OPT_FN: &str =
    "type OptFn = def(int64) -> int64 | None\n\ndef inc(x: int64) -> int64:\n    return x + 1\n\n";

fn with_opt_fn(body: &str) -> String {
    format!("{OPT_FN}{body}")
}

#[test]
fn a_mut_closure_capture_invalidates_union_narrowing() {
    let local_def = with_opt_fn(
        "def main():\n    mut u: OptFn = inc\n    if u is not None:\n        def nuke[mut u]() -> None:\n            u = None\n        nuke()\n        print(u(5))\n",
    );
    let lambda = with_opt_fn(
        "def clear_it(x: mut OptFn) -> int64:\n    x = None\n    return 0\n\ndef main():\n    mut u: OptFn = inc\n    if u is not None:\n        mut f = lambda [mut u]: clear_it(u)\n        _ = f()\n        print(u(5))\n",
    );
    let through_view = with_opt_fn(
        "def main():\n    mut u: OptFn = inc\n    if u is not None:\n        view mut v = u\n        v = None\n        print(u(5))\n",
    );
    assert_rejected(&[
        ("local def capture", "AU2014", &local_def),
        ("lambda capture", "AU2014", &lambda),
        ("write through a mutable view", "AU2014", &through_view),
    ]);
    let retested = with_opt_fn(
        "def main():\n    mut u: OptFn = inc\n    if u is not None:\n        def nuke[mut u]() -> None:\n            u = None\n        nuke()\n        if u is not None:\n            print(u(5))\n",
    );
    assert_accepted(&[("test again after the call", &retested)]);
}

#[test]
fn a_union_member_that_is_not_transfer_keeps_the_union_from_crossing_tasks() {
    let mutex = "import sync\nfrom iter import Generator\n\ntype OptGen = Generator[int64] | None\n\ndef counter_gen() -> Generator[int64]:\n    yield 1\n\ndef main():\n    m = sync.Mutex[OptGen](counter_gen())\n    with slot = m.lock():\n        pass\n";
    let boxed = "from iter import Generator\n\nclass GenBox:\n    g: Generator[int64]\n\ndef counter_gen() -> Generator[int64]:\n    yield 1\n\ndef work(b: own GenBox | None) -> int64:\n    return 0\n\ndef main():\n    b: GenBox | None = GenBox(g=counter_gen())\n    with group = TaskGroup():\n        t = group.start(work, b)\n        print(t.result())\n";
    assert_rejected(&[
        ("mutex of a generator union", "AU3008", mutex),
        ("task argument holding a class in a union", "AU3008", boxed),
    ]);
}

#[test]
fn a_trait_default_copy_cannot_be_implemented_for_a_noncloneable_type() {
    let trait_decl = "trait DupT[T]:\n    def dup(self, xs: list[T]) -> list[T]:\n        return xs.copy()\n\nclass Helper:\n    id: int64\n\n";
    let callable = format!(
        "type MutTaskFn = TaskCallable[mut def() -> int64]\n\n{trait_decl}impl DupT[MutTaskFn] for Helper:\n    pass\n\ndef main():\n    pass\n"
    );
    let task = format!(
        "def work() -> str:\n    return \"x\"\n\n{trait_decl}impl DupT[Task[str]] for Helper:\n    pass\n\ndef main():\n    pass\n"
    );
    let resource = format!(
        "import fs\n\n{trait_decl}impl DupT[fs.File] for Helper:\n    pass\n\ndef main():\n    pass\n"
    );
    assert_rejected(&[
        ("task callable", "AU3007", &callable),
        ("single-consumer task", "AU3009", &task),
        ("resource", "AU3007", &resource),
    ]);
    let copyable = format!(
        "{trait_decl}impl DupT[int64] for Helper:\n    pass\n\ndef main():\n    h = Helper(id=0)\n    print(len(h.dup([1, 2])))\n"
    );
    assert_accepted(&[("copyable element", &copyable)]);
}

#[test]
fn a_returned_view_through_any_callee_cannot_become_an_owned_binding() {
    let prelude = "type MutListFn = Callable[def(xs: mut list[int64]) -> view mut list[int64] from xs]\n\nclass Holder:\n    fns: list[MutListFn]\n\ndef id_mut(xs: mut list[int64]) -> view mut list[int64] from xs:\n    return view mut xs\n\ndef pick() -> MutListFn:\n    return MutListFn(id_mut)\n\ntrait Viewer:\n    def items(mut self) -> view mut list[int64] from self\n\nclass Bag:\n    xs: list[int64]\n\nimpl Viewer for Bag:\n    def items(mut self) -> view mut list[int64] from self:\n        return view mut self.xs\n\n";
    let cases = [
        ("field element", "    h = Holder(fns=[MutListFn(id_mut)])\n    mut xs: list[int64] = [1]\n    stolen = h.fns[0](xs)\n"),
        ("call result", "    mut xs: list[int64] = [1]\n    stolen = pick()(xs)\n"),
        ("conditional", "    f1 = MutListFn(id_mut)\n    f2 = MutListFn(id_mut)\n    mut xs: list[int64] = [1]\n    stolen = (f1 if len(xs) > 0 else f2)(xs)\n"),
        ("trait object method", "    mut b: dyn Viewer = Bag(xs=[1]) as dyn Viewer\n    stolen = b.items()\n"),
    ];
    for (name, body) in cases {
        let source = format!("{prelude}def main():\n{body}    print(len(stolen))\n");
        let actual = rejected_code(name, &source);
        assert!(actual.starts_with("AU3010"), "{name}: {actual}");
    }
    let borrowed = format!(
        "{prelude}def main():\n    h = Holder(fns=[MutListFn(id_mut)])\n    mut xs: list[int64] = [1]\n    view mut v = h.fns[0](xs)\n    v.append(2)\n    print(len(xs))\n"
    );
    assert_accepted(&[("bound as a view", &borrowed)]);
}

#[test]
fn a_move_before_break_or_continue_reaches_the_loop_exit_or_next_iteration() {
    let consumer = "import random\n\ntype RngConsumer = Callable[own def() -> random.Rng]\n\nclass Pair:\n    a: str\n    b: str\n\ndef take(s: own str) -> int64:\n    return len(s)\n\n";
    let cases = [
        (
            "consuming callable before continue",
            "    rng = random.Rng(42)\n    f = RngConsumer(lambda [own rng]: rng)\n    for i in range(2):\n        if i >= 0:\n            mut r = f()\n            print(r.next_int(0, 10))\n            continue\n",
        ),
        (
            "move before continue in a match arm",
            "    s = \"text\"\n    for i in range(2):\n        match i:\n            case 0:\n                print(take(s))\n                continue\n            case _:\n                pass\n",
        ),
        (
            "move before break with a later use",
            "    s = \"text\"\n    for i in range(2):\n        if i == 0:\n            print(take(s))\n            break\n    print(s)\n",
        ),
        (
            "partial move before break",
            "    p = Pair(a=\"A\", b=\"B\")\n    mut i: int64 = 0\n    while i < 2:\n        i += 1\n        if i == 1:\n            print(take(p.a))\n            break\n    print(p)\n",
        ),
    ];
    for (name, body) in cases {
        let source = format!("{consumer}def main():\n{body}");
        let actual = rejected_code(name, &source);
        assert!(
            actual.starts_with("AU3001") || actual.starts_with("AU2999"),
            "{name}: {actual}"
        );
    }
    let moved_then_left = format!(
        "{consumer}def main():\n    s = \"text\"\n    for i in range(2):\n        if i == 0:\n            print(take(s))\n            break\n"
    );
    assert_accepted(&[("move then break without a later use", &moved_then_left)]);
}

#[test]
fn a_while_condition_that_moves_runs_on_every_iteration() {
    let source = "type Consumer = Callable[own def() -> str]\n\ndef main():\n    s = \"SECRET\"\n    f = Consumer(lambda [own s]: s)\n    mut c: int64 = 0\n    while c < 2 and len(f()) > 0:\n        c += 1\n";
    let actual = rejected_code("consuming while condition", source);
    assert!(
        actual.starts_with("AU3001") || actual.starts_with("AU2999"),
        "{actual}"
    );
}

#[test]
fn a_loan_used_in_a_loop_stays_live_for_the_whole_loop_body() {
    let for_loop = "def main():\n    mut s: str = \"A\".repeat(8)\n    read = lambda [s]: len(s)\n    for i in range(2):\n        print(read())\n        s = \"B\".repeat(16)\n";
    let while_branches = "def main():\n    mut s: str = \"A\".repeat(8)\n    f = lambda [s]: print(s[0:4])\n    mut i: int64 = 0\n    while i < 2:\n        i += 1\n        if i == 1:\n            s = \"B\".repeat(8)\n        else:\n            f()\n";
    let while_match = "def main():\n    mut s: str = \"A\".repeat(8)\n    f = lambda [s]: print(s[0:4])\n    mut i: int64 = 0\n    while i < 2:\n        i += 1\n        match i:\n            case 1:\n                s = \"B\".repeat(8)\n            case _:\n                f()\n";
    let guard = "def main():\n    mut s: str = \"A\".repeat(8)\n    f = lambda [s]: len(s)\n    s = \"B\".repeat(16)\n    r = match 1:\n        case 1 if f() == 8: 10\n        case _: 20\n    print(r)\n";
    let span_guard = "def main():\n    mut xs: list[int64] = [1, 2, 3]\n    view sp = xs[0:2]\n    xs.clear()\n    r = match 1:\n        case 1 if len(sp) > 0: 1\n        case _: 2\n    print(r)\n";
    for (name, source) in [
        ("for loop", for_loop),
        ("while with branches", while_branches),
        ("while with match arms", while_match),
        ("match expression guard", guard),
        ("span in a match expression guard", span_guard),
    ] {
        let actual = rejected_code(name, source);
        assert!(
            actual.starts_with("AU3002") || actual.starts_with("AU3011"),
            "{name}: {actual}"
        );
    }
    let after_loop = "def main():\n    mut s: str = \"A\".repeat(8)\n    read = lambda [s]: len(s)\n    for i in range(2):\n        print(read())\n    s = \"B\".repeat(16)\n    print(s)\n";
    assert_accepted(&[("rebind after the loop", after_loop)]);
}

#[test]
fn a_returned_view_below_a_collection_element_borrows_the_collection() {
    let list = "class User:\n    name: str\n\ndef first_name(xs: list[User]) -> view str from xs:\n    return view xs[0].name\n\ndef main():\n    mut xs: list[User] = [User(name=\"ALICE\")]\n    view n = first_name(xs)\n    xs[0] = User(name=\"BOB\")\n    print(n)\n";
    let dict = "class User:\n    name: str\n\ndef entry_name(d: dict[str, User]) -> view str from d:\n    return view d[\"a\"].name\n\ndef main():\n    mut d: dict[str, User] = {\"a\": User(name=\"ALICE\")}\n    view n = entry_name(d)\n    d[\"a\"] = User(name=\"BOB\")\n    print(n)\n";
    for (name, source) in [("list element field", list), ("dict entry field", dict)] {
        let actual = rejected_code(name, source);
        assert!(
            actual.starts_with("AU3011") || actual.starts_with("AU3002"),
            "{name}: {actual}"
        );
    }
}

#[test]
fn a_literal_and_a_dynamic_dict_key_may_name_the_same_entry() {
    let source = "def bump(a: mut int64, b: mut int64):\n    a += 10\n    b += 100\n\ndef main():\n    mut d: dict[str, int64] = {\"a\": 1}\n    k = \"a\"\n    bump(d[\"a\"], d[k])\n    print(d[\"a\"])\n";
    let actual = rejected_code("literal and dynamic keys", source);
    assert!(actual.starts_with("AU3002"), "{actual}");
    let distinct = "def bump(a: mut int64, b: mut int64):\n    a += 10\n    b += 100\n\ndef main():\n    mut d: dict[str, int64] = {\"a\": 1, \"b\": 2}\n    bump(d[\"a\"], d[\"b\"])\n    print(d[\"a\"])\n";
    assert_accepted(&[("two literal keys", distinct)]);
}

#[test]
fn a_returned_view_receiver_borrows_its_origin_for_the_call() {
    let source = "class Acc:\n    v: int64\n    def add_from(mut self, other: Acc):\n        self.v += other.v\n\ndef get_mut(a: mut Acc) -> view mut Acc from a:\n    return view mut a\n\ndef main():\n    mut a = Acc(v=5)\n    get_mut(a).add_from(a)\n    print(a.v)\n";
    let actual = rejected_code("receiver and argument alias", source);
    assert!(actual.starts_with("AU3002"), "{actual}");
    let separate = "class Acc:\n    v: int64\n    def add_from(mut self, other: Acc):\n        self.v += other.v\n\ndef get_mut(a: mut Acc) -> view mut Acc from a:\n    return view mut a\n\ndef main():\n    mut a = Acc(v=5)\n    b = Acc(v=1)\n    get_mut(a).add_from(b)\n    print(a.v)\n";
    assert_accepted(&[("separate argument", separate)]);
}

#[test]
fn an_index_that_mutates_its_collection_through_a_builtin_method_is_rejected() {
    let read = "def main():\n    mut xs: list[int64] = [10, 0]\n    print(xs[xs.pop()])\n";
    let write =
        "def main():\n    mut xs: list[int64] = [10, 0]\n    xs[xs.pop()] = 99\n    print(xs[0])\n";
    for (name, source) in [("indexed read", read), ("indexed write", write)] {
        let actual = rejected_code(name, source);
        assert!(actual.starts_with("AU3002"), "{name}: {actual}");
    }
    let separate = "def main():\n    mut xs: list[int64] = [10, 0]\n    mut ys: list[int64] = [1]\n    print(xs[ys.pop()])\n";
    assert_accepted(&[("another list's method", separate)]);
}

#[test]
fn a_dropped_generator_whose_exit_traps_traps_the_caller() {
    let source = "import control\nfrom iter import Generator\n\nclass Boom:\n    def __enter__(mut self):\n        print(\"enter\")\n    def __exit__(mut self):\n        print(\"exit\")\n        assert false, \"boom in exit\"\n\ndef gen() -> Generator[int64]:\n    with b = Boom():\n        yield 1\n        yield 2\n\ndef work() -> int64:\n    mut gs: list[Generator[int64]] = [gen()]\n    for g in mut gs:\n        _ = g.next()\n    gs.clear()\n    return 99\n\ndef main():\n    match control.recover(work):\n        case Result.Ok(v):\n            print(f\"ok:{v}\")\n        case Result.Err(t):\n            print(f\"err:{t.code}:{t.message}\")\n";
    let output = crate::run_source(source).unwrap_or_else(|error| panic!("{error:?}"));
    assert_eq!(output.stdout, "enter\nexit\nerr:AU4001:boom in exit\n");
}

const NUKE: &str = "def nuke(x: mut OptFn) -> bool:\n    x = None\n    return true\n\ndef nuke_false(x: mut OptFn) -> bool:\n    x = None\n    return false\n\ndef take(s: own str) -> bool:\n    return len(s) > 0\n\nenum Tag:\n    One(int64)\n    Two(int64)\n\n";

fn with_guards(body: &str) -> String {
    format!("{OPT_FN}{NUKE}{body}")
}

#[test]
fn an_owned_match_guard_keeps_its_moves_and_mutations() {
    let mutation = with_guards(
        "def main():\n    mut u: OptFn = inc\n    t = Tag.One(1)\n    if u is not None:\n        match own t:\n            case Tag.One(_) if nuke(u):\n                print(u(5))\n            case _:\n                pass\n",
    );
    let moved = with_guards(
        "def main():\n    s = \"text\"\n    t = Tag.One(1)\n    match own t:\n        case Tag.One(_) if take(s):\n            print(s)\n        case _:\n            pass\n",
    );
    assert_rejected(&[
        ("owned guard mutation", "AU2014", &mutation),
        ("owned guard move", "AU3001", &moved),
    ]);
}

#[test]
fn a_failed_match_guard_still_affects_later_arms() {
    let statement = with_guards(
        "def main():\n    mut u: OptFn = inc\n    n: int64 = 1\n    if u is not None:\n        match n:\n            case 1 if nuke_false(u):\n                pass\n            case _:\n                print(u(5))\n",
    );
    let expression = with_guards(
        "def main():\n    mut u: OptFn = inc\n    n: int64 = 1\n    if u is not None:\n        r = match n:\n            case 1 if nuke_false(u): 0\n            case _: u(5)\n        print(r)\n",
    );
    let enum_arms = with_guards(
        "def main():\n    mut u: OptFn = inc\n    t = Tag.One(1)\n    if u is not None:\n        match t:\n            case Tag.One(_) if nuke_false(u):\n                pass\n            case _:\n                print(u(5))\n",
    );
    let moved = with_guards(
        "def main():\n    s = \"text\"\n    n: int64 = 1\n    match n:\n        case 1 if take(s):\n            pass\n        case _:\n            print(s)\n",
    );
    assert_rejected(&[
        ("statement arms", "AU2014", &statement),
        ("expression arms", "AU2014", &expression),
        ("enum arms", "AU2014", &enum_arms),
        ("moved in a failed guard", "AU3001", &moved),
    ]);
    let read_only = with_guards(
        "def main():\n    s = \"text\"\n    t = Tag.One(1)\n    match t:\n        case Tag.One(n) if n > 0:\n            print(s)\n        case _:\n            print(s)\n",
    );
    assert_accepted(&[("a guard that only reads", &read_only)]);
}

#[test]
fn the_mir_validator_drops_a_tag_proof_after_a_mut_call() {
    let source = with_guards(
        "def main():\n    mut u: OptFn = inc\n    if u is not None:\n        print(u(5))\n        _ = nuke(u)\n",
    );
    let mut module = crate::lower_source_to_mir(&source).expect("the program lowers");
    crate::mir::validate_loan_flow(&module).expect("the lowered program validates");
    // Move the `nuke(u)` call ahead of the payload read in the same block.
    let main = module
        .functions
        .iter_mut()
        .find(|function| function.name == "main")
        .expect("main is lowered");
    let block = main
        .blocks
        .iter_mut()
        .find(|block| {
            block.instructions.iter().any(|instruction| {
                matches!(
                    instruction,
                    crate::mir::Instruction::Assign {
                        value: crate::mir::Rvalue::Call { callee: crate::mir::CallTarget::Name(name), .. },
                        ..
                    } if name == "nuke"
                )
            })
        })
        .expect("the nuke call is lowered");
    let call = block
        .instructions
        .iter()
        .position(|instruction| {
            matches!(
                instruction,
                crate::mir::Instruction::Assign {
                    value: crate::mir::Rvalue::Call { callee: crate::mir::CallTarget::Name(name), .. },
                    ..
                } if name == "nuke"
            )
        })
        .expect("the nuke call is in the block");
    let moved = block.instructions.remove(call);
    block.instructions.insert(0, moved);
    let error = crate::mir::validate_loan_flow(&module).expect_err("the forged order is rejected");
    assert!(
        error.contains("has no active matching tag proof"),
        "{error}"
    );
}
