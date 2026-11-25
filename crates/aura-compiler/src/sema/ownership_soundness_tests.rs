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
