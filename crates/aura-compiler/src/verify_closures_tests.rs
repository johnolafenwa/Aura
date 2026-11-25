use super::*;

/// Verifies `source` with the solver on this machine. Without one the test
/// is skipped, except under CI, which installs a pinned Z3.
fn verify(source: &str) -> Option<Report> {
    verify_with(source, &Options::default())
}

fn verify_with(source: &str, options: &Options) -> Option<Report> {
    if smt::SolverCommand::find(options.solver.as_deref()).is_err() {
        assert!(
            std::env::var_os("CI").is_none(),
            "CI must install an SMT solver for the verifier tests"
        );
        eprintln!("skipping: no SMT solver on PATH");
        return None;
    }
    let program = crate::check_source(source).expect("the program checks");
    let module = crate::mir::lower_for_verification(&program);
    Some(verify_module(&program, &module, options).expect("the verifier runs"))
}

/// Each obligation that was not proved, as `line kind status`.
fn open_checks(report: &Report) -> Vec<String> {
    report
        .obligations
        .iter()
        .filter(|obligation| obligation.status != Status::Proved)
        .map(|obligation| {
            let status = match &obligation.status {
                Status::Proved => "proved",
                Status::Failed { .. } => "failed",
                Status::Unknown => "unknown",
                Status::Unsupported(_) => "unsupported",
            };
            format!(
                "{} {} {status}",
                obligation.span.line,
                obligation.kind.name()
            )
        })
        .collect()
}

/// Verifies a program that must prove completely, and runs it to show the
/// interpreter agrees that no check fails.
fn assert_proved(source: &str) {
    let Some(report) = verify(source) else {
        return;
    };
    assert_eq!(open_checks(&report), Vec::<String>::new(), "{report:#?}");
    assert!(report.is_proved());
    assert!(!report.obligations.is_empty());
    crate::run_source(source).expect("a proved program runs without a failed check");
}

fn assert_open(source: &str, expected: &[&str]) -> Option<Report> {
    let report = verify(source)?;
    assert_eq!(open_checks(&report), expected, "{report:#?}");
    Some(report)
}

/// The function each open check is reported in.
fn open_in(report: &Report) -> Vec<&str> {
    report
        .obligations
        .iter()
        .filter(|obligation| obligation.status != Status::Proved)
        .map(|obligation| obligation.function.as_str())
        .collect()
}

#[test]
fn a_closure_runs_on_the_values_it_captured() {
    assert_proved(
        r#"
def scaled() -> int64:
    ensures result == 42
    factor: int64 = 2
    scale: def(int64) -> int64 = lambda value: value * factor
    return scale(21)

def local(k: int64) -> int64:
    requires 0 <= k and k < 100
    ensures result == k + 1
    def add_k(x: int64) -> int64:
        return x + k
    return add_k(1)

def text() -> int64:
    ensures result == 2
    name = "ab"
    size = lambda: len(name)
    return size()

def thin() -> int64:
    ensures result == 3
    inc: def(int64) -> int64 = lambda v: v + 1
    def dec(v: int64) -> int64:
        return v - 1
    return inc(dec(3))

def packed(k: int64) -> int64:
    requires 0 <= k and k < 100
    ensures result == k + 5
    f: Callable[def(v: int64) -> int64] = lambda v: v + k
    return f(5)

def copied(k: int64) -> int64:
    requires 0 <= k and k < 100
    ensures result == 10 * k
    k10 = lambda: k * 10
    again = k10
    return again()

def named(a: int64) -> int64:
    requires 0 <= a and a < 100
    ensures result == a - 3
    def minus(left: int64, *, right: int64) -> int64:
        return left - right
    return minus(a, right=3)

def main():
    print(scaled())
    print(local(4))
    print(text())
    print(thin())
    print(packed(1))
    print(copied(2))
    print(named(5))
"#,
    );
}

#[test]
fn a_wrong_claim_about_a_closure_result_fails() {
    let source = r#"
def scaled() -> int64:
    factor: int64 = 2
    scale: def(int64) -> int64 = lambda value: value * factor
    r = scale(21)
    assert r == 43
    return r

def local(k: int64) -> int64:
    requires 0 <= k and k < 100
    def add_k(x: int64) -> int64:
        return x + k
    r = add_k(1)
    assert r > k + 1
    return r

def thin() -> int64:
    inc: def(int64) -> int64 = lambda v: v + 1
    r = inc(3)
    assert r == 3
    return r

def unbounded(k: int64) -> int64:
    def triple(v: int64) -> int64:
        return v * 3
    return triple(k)

def main():
    print(local(4))
    print(unbounded(2))
"#;
    let Some(report) = assert_open(
        source,
        &[
            "6 assert failed",
            "14 assert failed",
            "20 assert failed",
            "25 overflow failed",
        ],
    ) else {
        return;
    };
    // A check in a closure body is reported where the closure is called.
    assert_eq!(open_in(&report), ["scaled", "local", "thin", "unbounded"]);
}

#[test]
fn a_capture_by_reference_is_read_and_written_at_the_call() {
    assert_proved(
        r#"
def total() -> int64:
    ensures result == 12
    mut total: int64 = 0
    def add[mut total](amount: int64) -> None:
        total += amount
    add(5)
    add(7)
    return total

def pushed() -> int64:
    ensures result == 7
    mut xs: list[int64] = [1, 2]
    def push[mut xs](v: int64) -> None:
        xs.append(v)
    push(3)
    push(4)
    assert len(xs) == 4
    return xs[2] + xs[3]

def between(start: int64) -> int64:
    requires 0 <= start and start < 100
    ensures result == start + 3
    mut n: int64 = start
    def bump[mut n]() -> None:
        n += 1
    bump()
    print("between")
    bump()
    bump()
    return n

def shared() -> int64:
    ensures result == 3
    values = [1, 2, 3]
    size = lambda [values]: len(values)
    return size()

def filled() -> int64:
    ensures result == 9
    mut xs: list[int64] = []
    def fill(values: mut list[int64], *, v: int64) -> None:
        values.append(v)
    fill(xs, v=9)
    return xs[0]

def main():
    print(total())
    print(pushed())
    print(between(1))
    print(shared())
    print(filled())
"#,
    );
    assert_open(
        r#"
def total() -> int64:
    mut total: int64 = 0
    def add[mut total](amount: int64) -> None:
        total += amount
    add(5)
    add(7)
    assert total == 5
    return total

def pushed() -> int64:
    mut xs: list[int64] = [1, 2]
    def push[mut xs](v: int64) -> None:
        xs.append(v)
    push(3)
    assert len(xs) == 2
    return xs[2]

def filled() -> int64:
    mut xs: list[int64] = []
    def fill(values: mut list[int64], *, v: int64) -> None:
        values.append(v)
    fill(xs, v=9)
    return xs[1]

def main():
    print(filled())
"#,
        &[
            "8 assert failed",
            "16 assert failed",
            "24 index in range failed",
        ],
    );
}

#[test]
fn a_closure_keeps_its_own_state_between_calls() {
    assert_proved(
        r#"
def counted() -> int64:
    ensures result == 3
    mut count: int64 = 0
    def next() -> int64:
        count += 1
        return count
    a = next()
    b = next()
    assert a == 1
    assert count == 0
    return a + b

def either(flag: bool) -> int64:
    ensures result == 1 or result == 2
    mut hits: int64 = 0
    def bump() -> int64:
        hits += 1
        return hits
    if flag:
        bump()
    return bump()

def main():
    print(counted())
    print(either(true))
"#,
    );
    assert_open(
        r#"
def counted() -> int64:
    mut count: int64 = 0
    def next() -> int64:
        count += 1
        return count
    a = next()
    b = next()
    assert b == 1
    return a + b

def captured() -> int64:
    mut count: int64 = 0
    def next() -> int64:
        count += 1
        return count
    a = next()
    assert count == 1
    return a

def either(flag: bool) -> int64:
    mut hits: int64 = 0
    def bump() -> int64:
        hits += 1
        return hits
    if flag:
        bump()
    r = bump()
    assert r == 1
    return r

def main():
    print(either(true))
"#,
        &["9 assert failed", "18 assert failed", "29 assert failed"],
    );
}

#[test]
fn an_assert_in_a_local_function_is_checked_at_each_call() {
    assert_proved(
        r#"
def half(n: int64) -> int64:
    requires 0 <= n and n <= 50
    ensures result == n
    def checked(v: int64) -> int64:
        assert v >= 0 and v <= 100
        assert v % 2 == 0
        return v // 2
    return checked(2 * n)

def looped() -> int64:
    ensures result == 30
    step: int64 = 3
    def scaled(v: int64) -> int64:
        assert v >= 0 and v <= 100
        return v * step
    mut i: int64 = 0
    mut sum: int64 = 0
    while i < 10:
        invariant 0 <= i and i <= 10
        invariant sum == 3 * i
        decreases 10 - i
        sum += scaled(1)
        i += 1
    return sum

def main():
    print(half(4))
    print(looped())
"#,
    );
    let Some(report) = assert_open(
        r#"
def half(n: int64) -> int64:
    requires 0 <= n and n <= 50
    def checked(v: int64) -> int64:
        assert v % 2 == 0
        return v // 2
    return checked(2 * n + 1)

def main():
    print(half(4))
"#,
        &["5 assert failed"],
    ) else {
        return;
    };
    assert_eq!(open_in(&report), ["half"]);
}

#[test]
fn a_closure_in_a_method_changes_the_receiver() {
    assert_proved(
        r#"
class Acc:
    total: int64

    public def bump(mut self, by: int64) -> int64:
        requires by >= 0 and by <= 10
        requires self.total >= 0 and self.total <= 100
        ensures self.total == old(self.total) + by
        ensures result == self.total
        mut step: def(int64) -> None = lambda[mut self] n: self.add(n)
        step(by)
        return self.total

    def add(mut self, n: int64) -> None:
        requires n >= 0 and n <= 10
        requires self.total >= 0 and self.total <= 100
        ensures self.total == old(self.total) + n
        self.total = self.total + n

def nested(x: int64) -> int64:
    requires x >= 0 and x < 100
    ensures result == x
    outer: int64 = 5
    def make(y: int64) -> int64:
        inner = lambda: y + outer
        return inner()
    return make(x) - 5

def main():
    mut a = Acc(total=1)
    print(a.bump(2))
    print(nested(3))
"#,
    );
    assert_open(
        r#"
class Acc:
    total: int64

    public def bump(mut self, by: int64) -> int64:
        requires self.total >= 0 and self.total <= 100
        ensures self.total == old(self.total) + by
        mut step: def(int64) -> None = lambda[mut self] n: self.add(n)
        step(by)
        return self.total

    def add(mut self, n: int64) -> None:
        requires n >= 0 and n <= 10
        requires self.total >= 0 and self.total <= 100
        ensures self.total == old(self.total) + n
        self.total = self.total + n

def main():
    mut a = Acc(total=1)
    print(a.bump(2))
"#,
        &["8 requires failed"],
    );
}

#[test]
fn an_integer_a_closure_receives_is_in_the_range_of_its_type() {
    // The conversion on line 3 may not fit, and is reported there. The
    // closure runs only when it did, so its own conversion always fits.
    assert_open(
        r#"
def widened(total: int64) -> int64:
    small = (total + 1) as int32
    widen = lambda: small as int64
    return widen()

def main():
    print(widened(1))
"#,
        &["3 overflow failed", "3 overflow failed"],
    );
}

#[test]
fn a_bound_method_call_has_the_contract_of_its_method() {
    let methods = r#"
class Acc:
    total: int64

    def get(self, n: int64) -> int64:
        requires n >= 0 and n < 100
        requires self.total >= 0 and self.total < 100
        ensures result == self.total + n
        return self.total + n

    def add(mut self, n: int64) -> int64:
        requires n >= 0 and n < 100
        requires self.total >= 0 and self.total < 100
        ensures self.total == old(self.total) + n
        ensures result == self.total
        self.total = self.total + n
        return self.total
"#;
    assert_proved(&format!(
        r#"{methods}
def read() -> int64:
    ensures result == 5
    a = Acc(total=1)
    f = a.get
    return f(4)

def write() -> int64:
    ensures result == 6
    mut a = Acc(total=1)
    mut g = a.add
    g(2)
    return g(3)

def main():
    print(read())
    print(write())
"#
    ));
    // The `requires` of the method is proved where the bound method is
    // called, and its `ensures` is all that is known of the result.
    assert_open(
        &format!(
            r#"{methods}
def read() -> int64:
    a = Acc(total=1)
    f = a.get
    return f(-1)

def write() -> int64:
    mut a = Acc(total=1)
    mut g = a.add
    g(2)
    r = g(3)
    assert r == 5
    return r

def main():
    print(write())
"#
        ),
        &["21 requires failed", "29 assert failed"],
    );
}

#[test]
fn a_closure_the_verifier_cannot_follow_is_not_modeled() {
    // State kept across a loop, a recursive call, and a view chosen at
    // run time are not followed.
    let Some(report) = assert_open(
        r#"
class Pair:
    left: list[int64]
    right: list[int64]

def pick(p: mut Pair, flag: bool) -> view mut list[int64] from p:
    if flag:
        return view mut p.left
    return view mut p.right

def run(n: int64) -> int64:
    requires n >= 1
    mut count: int64 = 0
    def next() -> int64:
        count += 1
        return count
    mut i: int64 = 0
    mut last: int64 = 0
    while i < n:
        invariant 0 <= i
        decreases n - i
        last = next()
        assert last >= 1
        i += 1
    return last

def fact(n: int64) -> int64:
    def go(m: int64) -> int64:
        if m <= 1:
            return 1
        return m * go(m - 1)
    r = go(n)
    assert r >= 1
    return r

def chosen() -> int64:
    mut pair = Pair(left=[1], right=[2])
    view mut side = pick(pair, true)
    mut update: def(int64) -> None = lambda [mut side] item: side.append(item)
    update(3)
    assert len(pair.left) == 1
    return len(pair.left)

def main():
    print(run(3))
    print(fact(3))
    print(chosen())
"#,
        &[
            "22 assert unsupported",
            "23 assert unsupported",
            "31 overflow unsupported",
            "33 assert unsupported",
            "40 assert unsupported",
            "41 assert unsupported",
        ],
    ) else {
        return;
    };
    let reason = |line: usize| {
        let check = report
            .obligations
            .iter()
            .find(|obligation| obligation.span.line == line)
            .expect("a check on the line");
        match &check.status {
            Status::Unsupported(reason) => reason.clone(),
            status => panic!("{status:?}"),
        }
    };
    assert!(reason(22).contains("does not follow the closure to this call"));
    assert!(reason(23).contains("a call of a closure the verifier does not follow"));
    assert!(reason(33).contains("a recursive call of a local function"));
    assert!(reason(41).contains("a call of a closure the verifier does not follow"));
}

#[test]
fn a_closure_with_another_use_is_still_verified_alone() {
    let Some(report) = assert_open(
        r#"
class Holder:
    double: Callable[def(value: int64) -> int64]

    def __init__(self) -> None:
        self.double = lambda value: value * 2

def inc(v: int64) -> int64:
    requires v < 1000
    return v + 1

def apply(f: def(int64) -> int64, x: int64) -> int64:
    return f(x)

def plain() -> def(int64) -> int64:
    return inc

def passed(x: int64) -> int64:
    requires 0 <= x and x < 10
    mut seen: list[int64] = []
    def note[mut seen](v: int64) -> int64:
        seen.append(v)
        return v
    r = apply(lambda v: v + 1, x)
    k = note(x)
    assert k == x and len(seen) == 1
    return r

def either(flag: bool, other: def(int64) -> int64) -> int64:
    mut f: def(int64) -> int64 = lambda v: v + 1
    if flag:
        f = lambda v: v + 2
    r = f(1)
    assert r == 2
    mut g: def(int64) -> int64 = lambda v: v + 3
    if flag:
        g = other
    return g(r)

def inside(x: int64) -> int64:
    requires 0 <= x and x < 10
    inner = lambda: x + 1
    outer = lambda[inner]: inner() + 1
    r = outer()
    assert r == x + 2
    return r

def counter() -> Callable[mut def() -> int64]:
    mut count: int64 = 0
    def next() -> int64:
        count += 1
        return count
    first = next()
    assert first == 1
    return next

def main():
    print(passed(1))
    print(either(false, inc))
    print(inside(1))
    mut c = counter()
    print(c())
    print(Holder().double(2))
    print(plain()(1))
"#,
        &[
            "6 overflow failed",
            "16 requires unsupported",
            "24 overflow failed",
            "30 overflow failed",
            "32 overflow failed",
            "34 assert unsupported",
            "35 overflow failed",
            "42 overflow failed",
            "43 overflow unsupported",
            "45 assert unsupported",
            "51 overflow failed",
            "54 assert unsupported",
            "59 requires unsupported",
        ],
    ) else {
        return;
    };
    assert_eq!(
        open_in(&report),
        [
            "Holder.__init__::__lambda_6_23",
            "plain",
            "passed::__lambda_24_15",
            "either::__lambda_30_34",
            "either::__lambda_32_13",
            "either",
            "either::__lambda_35_34",
            "inside::__lambda_42_13",
            "inside",
            "inside",
            "counter::__lambda_50_5",
            "counter",
            "main",
        ]
    );
}

#[test]
fn a_decorated_function_is_not_modeled() {
    // The decorated name is a function value the decorator returned, so a
    // call of it has no contract: nothing about it is proved or refuted.
    let source = r#"
def same(f: own Callable[def(n: int64) -> int64]) -> Callable[def(n: int64) -> int64]:
    return f

def negate(f: own Callable[def(n: int64) -> int64]) -> Callable[def(n: int64) -> int64]:
    return lambda[own f] n: 0 - f(n)

@negate
def below(n: int64) -> int64:
    requires 0 <= n and n < 1000
    ensures result >= 1
    return n + 1

@same
def above(n: int64) -> int64:
    requires 0 <= n and n < 1000
    ensures result >= 1
    return n + 1

def main():
    r = below(3)
    assert r >= 1
    s = above(3)
    assert s >= 1
    t = above(-5)
    print(t)
"#;
    let Some(report) = assert_open(
        source,
        &[
            "6 overflow unsupported",
            "9 requires unsupported",
            "15 requires unsupported",
            "22 assert unsupported",
            "24 assert unsupported",
        ],
    ) else {
        return;
    };
    assert!(!report.is_proved());
    // The first assertion is false when the program runs, and the call
    // with -5 breaks the `requires`.
    assert!(crate::run_source(source).is_err());
}

#[test]
fn a_lambda_is_not_checked_against_a_callable_contract() {
    assert_open(
        r#"
type Step = def(n: int64) -> int64:
    requires 0 <= n and n < 100
    ensures result > n

def twice(f: Step, x: int64) -> int64:
    requires 0 <= x and x < 50
    ensures result > x
    return f(x)

def main():
    k: int64 = 1
    print(twice(lambda n: n + 1, 3))
    step: def(n: int64) -> int64 = lambda n: n + k
    print(step(4))
"#,
        &["13 requires unsupported", "13 overflow failed"],
    );
}

#[test]
fn a_build_still_asks_which_checks_of_a_closure_always_hold() {
    let source = r#"
def main():
    xs = [1, 2, 3]
    k: int64 = 1
    def at[xs](i: int64) -> int64:
        return xs[i] + xs[0]
    def first[xs, k]() -> int64:
        if len(xs) > 0:
            return xs[0]
        return k
    print(at(1))
    print(first())
"#;
    let options = Options {
        prove_sites: true,
        ..Options::default()
    };
    let Some(report) = verify_with(source, &options) else {
        return;
    };
    assert_eq!(open_checks(&report), Vec::<String>::new(), "{report:#?}");
    // The guarded read in `first` holds for any list, so a build drops
    // its check. The reads in `at` hold only for this call.
    let sites: Vec<&str> = report
        .proved_sites
        .iter()
        .map(|(function, ..)| function.as_str())
        .filter(|function| function.contains("::__lambda_"))
        .collect();
    assert_eq!(sites, ["main::__lambda_7_5"], "{:?}", report.proved_sites);
    // Without the build's question the closures are checked only at their
    // calls.
    let report = verify(source).expect("a solver ran above");
    assert_eq!(report.functions, 1);
    assert!(report.is_proved());
}

#[test]
fn closures_agree_with_the_interpreter() {
    // Every call is followed, so each assertion is proved from the
    // closure bodies, and the interpreter must agree on every input.
    assert_proved(
        r#"
def affine(a: int64, b: int64, x: int64) -> int64:
    requires -100 <= a and a <= 100 and -100 <= b and b <= 100
    requires -100 <= x and x <= 100
    ensures result == a * x + b
    line: def(int64) -> int64 = lambda v: a * v + b
    return line(x)

def tally(a: int64, b: int64) -> int64:
    requires -100 <= a and a <= 100 and -100 <= b and b <= 100
    ensures result == 2 * a + b
    mut total: int64 = 0
    def add[mut total](amount: int64) -> None:
        total += amount
    add(a)
    add(b)
    add(a)
    return total

def stepped(start: int64, flag: bool) -> int64:
    requires -100 <= start and start <= 100
    ensures flag or result == start + 3
    ensures not flag or result == start + 6
    mut state: int64 = start
    def step(by: int64) -> int64:
        state += by
        return state
    step(1)
    if flag:
        step(3)
    return step(2)

def collected(a: int64, b: int64) -> int64:
    ensures result == b
    mut xs: list[int64] = []
    def push[mut xs](v: int64) -> int64:
        xs.append(v)
        return len(xs)
    assert push(a) == 1
    assert push(b) == 2
    return xs[1]

def main():
    for a in range(-3, 4):
        for b in range(-2, 3):
            assert tally(a, b) == 2 * a + b
            assert collected(a, b) == b
            for x in range(-2, 3):
                assert affine(a, b, x) == a * x + b
        assert stepped(a, false) == a + 3
        assert stepped(a, true) == a + 6
    print("ok")
"#,
    );
}
