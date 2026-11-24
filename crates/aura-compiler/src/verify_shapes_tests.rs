use super::*;

/// Verifies `source` with the solver on this machine. Without one the test
/// is skipped, except under CI, which installs a pinned Z3.
fn verify(source: &str) -> Option<Report> {
    let options = Options::default();
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
    Some(verify_module(&program, &module, &options).expect("the verifier runs"))
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

fn assert_open(source: &str, expected: &[&str]) {
    if let Some(report) = verify(source) {
        assert_eq!(open_checks(&report), expected, "{report:#?}");
    }
}

/// The messages of the checks that were not proved.
fn open_messages(report: &Report) -> Vec<String> {
    report
        .obligations
        .iter()
        .filter(|obligation| obligation.status != Status::Proved)
        .map(|obligation| match &obligation.status {
            Status::Unsupported(why) => format!("{}: {why}", obligation.message),
            _ => obligation.message.clone(),
        })
        .collect()
}

const ACCOUNT: &str = r#"
class Account:
    balance: int64
    invariant self.balance >= 0
"#;

// ----- Objects inside other values ---------------------------------------

#[test]
fn an_object_inside_a_value_keeps_its_invariant_across_a_boundary() {
    assert_proved(&format!(
        r#"{ACCOUNT}
class Bank:
    vault: Account
    n: int64

    public def total(self) -> int64:
        ensures result >= 0
        return self.vault.balance

    public def swap(mut self, other: Account):
        self.vault = Account(balance=other.balance)

enum Slot:
    Full(Account)
    Empty

def first(xs: list[Account]) -> int64:
    requires len(xs) > 0
    ensures result >= 0
    return xs[0].balance

def fixer(xs: mut list[Account]):
    requires len(xs) > 0
    ensures len(xs) == len(old(xs))
    xs[0].balance = 5

def make_ok(n: int64) -> Result[Account, str]:
    requires n >= 0
    mut a = Account(balance=1)
    a.balance = n
    return Ok(a)

def use_make() -> int64:
    ensures result >= 0
    match make_ok(4):
        case Ok(a):
            return a.balance
        case Err(e):
            return 0

def maybe(n: int64) -> Account | None:
    if n < 0:
        return None
    return Account(balance=n)

def use_maybe(n: int64) -> int64:
    ensures result >= 0
    a = maybe(n)
    if a is None:
        return 0
    return a.balance

def slot(s: Slot) -> int64:
    ensures result >= 0
    match s:
        case Slot.Full(a):
            return a.balance
        case Slot.Empty:
            return 0

def pair(p: (Account, int64)) -> int64:
    ensures result >= 0
    return p[0].balance

def main():
    xs = [Account(balance=1), Account(balance=4)]
    print(first(xs))
    print(use_make())
    print(use_maybe(3))
    print(slot(Slot.Full(Account(balance=2))))
    mut ys = [Account(balance=1)]
    fixer(ys)
    print(first(ys))
    print(pair((Account(balance=1), 2)))
    mut b = Bank(vault=Account(balance=1), n=0)
    b.swap(Account(balance=7))
    print(b.total())
"#
    ));
}

#[test]
fn an_element_does_not_hold_more_than_its_invariant() {
    assert_open(
        &format!(
            r#"{ACCOUNT}
def strict(xs: list[Account]) -> int64:
    requires len(xs) > 0
    ensures result > 0
    return xs[0].balance

def main():
    print(strict([Account(balance=1)]))
"#
        ),
        &["8 ensures failed"],
    );
}

#[test]
fn a_broken_object_is_found_where_its_holder_leaves() {
    let Some(report) = verify(&format!(
        r#"{ACCOUNT}
class Bank:
    vault: Account

    public def total(self) -> int64:
        ensures result >= 0
        return self.vault.balance

    public def drain(mut self):
        self.vault.balance = -1

def breaker(xs: mut list[Account]):
    requires len(xs) > 0
    xs[0].balance = -5

def make(n: int64) -> Result[Account, str]:
    mut a = Account(balance=1)
    a.balance = n
    return Ok(a)

def maybe(n: int64) -> Account | None:
    mut a = Account(balance=1)
    a.balance = n
    return a

def first(xs: list[Account]) -> int64:
    requires len(xs) > 0
    ensures result >= 0
    return xs[0].balance

def bad_caller() -> int64:
    mut a = Account(balance=1)
    a.balance = -1
    xs = [a]
    return first(xs)

def bad_bank() -> int64:
    mut b = Bank(vault=Account(balance=1))
    b.vault.balance = -1
    return b.total()

def bad_pair() -> (Account, int64):
    mut a = Account(balance=1)
    a.balance = -1
    return (a, 1)

def main():
    print(1)
"#
    )) else {
        return;
    };
    assert_eq!(
        open_messages(&report),
        [
            "class invariant holds for `self.vault` on return: `self.balance >= 0`",
            "class invariant holds for `xs[..]` on return: `self.balance >= 0`",
            "class invariant holds for `result.Ok` on return: `self.balance >= 0`",
            "class invariant holds for `result` on return: `self.balance >= 0`",
            "class invariant holds for `xs[..]` passed to `first`: `self.balance >= 0`",
            "class invariant holds for `self.vault` passed to `Bank.total`: `self.balance >= 0`",
            "class invariant holds for `result[0]` on return: `self.balance >= 0`",
        ],
        "{report:#?}"
    );
    assert!(report
        .obligations
        .iter()
        .all(|obligation| !matches!(obligation.status, Status::Unknown | Status::Unsupported(_))));
}

#[test]
fn a_borrowed_element_is_read_and_written_in_its_list() {
    assert_proved(
        r#"
class Account:
    balance: int64
    invariant 0 <= self.balance and self.balance <= 1000

    public def get(self) -> int64:
        ensures 0 <= result and result <= 1000
        return self.balance

    public def add(mut self, n: int64):
        requires 0 <= n and self.balance + n <= 1000
        ensures self.balance == old(self.balance) + n
        self.balance += n

def process(a: Account) -> int64:
    ensures 0 <= result and result <= 1000
    return a.balance

def total(xs: list[Account]) -> int64:
    requires len(xs) <= 1000
    ensures result >= 0
    mut t = 0
    for i in range(len(xs)):
        invariant 0 <= t and t <= i * 1000
        t += process(xs[i])
    return t

def total2(xs: list[Account]) -> int64:
    ensures result >= 0
    mut t = 0
    for a in xs:
        invariant 0 <= t
        if t < 100000:
            t += a.get()
    return t

def bump(xs: mut list[Account]):
    requires len(xs) > 0
    ensures len(xs) == len(old(xs))
    ensures xs[0].balance >= old(xs[0].balance)
    if xs[0].balance < 500:
        xs[0].add(1)

def set(xs: mut list[Account]):
    requires len(xs) > 1
    ensures xs[1].balance == 8
    xs[1].balance = 8

def main():
    mut xs = [Account(balance=1), Account(balance=2)]
    print(total(xs))
    print(total2(xs))
    bump(xs)
    set(xs)
    assert xs[1].balance == 8
"#,
    );
}

#[test]
fn a_write_to_a_borrowed_element_is_not_lost() {
    assert_open(
        r#"
class Account:
    balance: int64
    invariant 0 <= self.balance and self.balance <= 1000

    public def add(mut self, n: int64):
        requires 0 <= n and self.balance + n <= 1000
        ensures self.balance == old(self.balance) + n
        self.balance += n

    def wreck(mut self):
        self.balance = -1

def set(xs: mut list[Account]):
    requires len(xs) > 1
    ensures xs[1].balance == 9
    xs[1].balance = 8

def bump(xs: mut list[Account]):
    requires len(xs) > 0
    ensures xs[0].balance == old(xs[0].balance)
    if xs[0].balance < 500:
        xs[0].add(1)

def over(xs: mut list[Account]):
    requires len(xs) > 0
    xs[0].add(1)

def main():
    mut xs = [Account(balance=1), Account(balance=2)]
    set(xs)
    bump(xs)
"#,
        &[
            "16 ensures failed",
            "21 ensures failed",
            "27 requires failed",
        ],
    );
}

#[test]
fn a_part_the_encoding_leaves_out_is_not_modeled_at_a_boundary() {
    let Some(report) = verify(&format!(
        r#"{ACCOUNT}
class Tagged:
    balance: int64
    note: int64 | None
    invariant self.balance >= 0

class Node:
    value: int64
    next: indirect Node | None
    invariant self.value >= 0

class Tree:
    next: indirect Tree | None
    n: int64

class Odd:
    items: list[int64]
    invariant sum(self.items) >= 0

class Holder:
    odd: Odd

class L1:
    acc: Account
class L2:
    inner: L1
class L3:
    inner: L2
class L4:
    inner: L3
class L5:
    inner: L4

def total(xs: list[Tagged]) -> int64:
    return len(xs)

def pass_on(xs: list[Tagged]) -> int64:
    return total(xs)

def head(n: Node) -> int64:
    ensures result >= 0
    return n.value

def walk(n: Node) -> int64:
    if n.next is None:
        return 0
    return head(n.next)

def shallow(v: L1) -> int64:
    ensures result >= 0
    return v.acc.balance

def deep(v: L5) -> int64:
    return shallow(v.inner.inner.inner.inner)

def keep(v: L5) -> int64:
    return 1

def same(v: L5) -> int64:
    return keep(v)

def tree(t: Tree) -> int64:
    return t.n

def pass_tree(t: Tree) -> int64:
    return tree(t)

def hold(h: Holder) -> int64:
    return 1

def pass_hold(h: Holder) -> int64:
    return hold(h)

def count(xs: list[Account]) -> int64:
    return len(xs)

def vals(d: dict[str, Account]) -> int64:
    return count(d.values())

def rows(xs: list[list[Account]]) -> int64:
    requires len(xs) > 0
    return count(xs[0])

def pick() -> int64:
    ys = [Account(balance=1) for i in range(3)]
    return count(ys)

def main():
    print(pass_on([Tagged(balance=1, note=None)]))
    print(walk(Node(value=1, next=Node(value=2, next=None))))
    print(pick())
"#
    )) else {
        return;
    };
    assert_eq!(
        open_messages(&report),
        [
            "class invariant holds for each object in `n.next` passed to `head`: \
             the verifier does not model what `n.next` holds",
            "class invariant holds for each object in `v` passed to `shallow`: \
             the verifier does not model what `v` holds",
            "class invariant holds for `h.odd` passed to `hold`: `sum(self.items) >= 0`: \
             `sum` is not modeled in contracts yet",
            "class invariant holds for each object in `xs` passed to `count`: \
             the verifier does not model what `xs` holds",
            "class invariant holds for each object in `xs` passed to `count`: \
             the verifier does not model what `xs` holds",
            "class invariant holds for `xs[..]` passed to `count`: `self.balance >= 0`: \
             it depends on a comprehension or built-in loop at line 89, \
             which the verifier does not model",
        ],
        "{report:#?}"
    );
}

#[test]
fn a_list_built_in_a_loop_needs_an_invariant_about_its_elements() {
    let program = |invariant: &str| {
        format!(
            r#"{ACCOUNT}
def count(xs: list[Account]) -> int64:
    return len(xs)

def built(n: int64) -> int64:
    requires 0 <= n and n <= 10
    mut ys: list[Account] = []
    for i in range(n):
        {invariant}
        ys.append(Account(balance=i))
    return count(ys)

def main():
    print(built(3))
"#
        )
    };
    assert_proved(&program("invariant forall(a in ys, a.balance >= 0)"));
    assert_open(
        &program("invariant len(ys) == i"),
        &["15 class invariant failed"],
    );
}

#[test]
fn an_element_with_a_list_or_a_tuple_keeps_its_invariant() {
    let program = |bound: &str| {
        format!(
            r#"
class Stack:
    items: list[int64]
    cap: int64
    pos: (int64, bool)
    invariant len(self.items) <= self.cap
    invariant self.pos[0] >= 0

def cap(xs: list[Stack]) -> int64:
    requires len(xs) > 0
    ensures result {bound} 0
    return xs[0].cap

def tip(xs: list[Stack]) -> int64:
    requires len(xs) > 0
    ensures result {bound} 0
    return xs[0].pos[0]

def main():
    print(cap([Stack(items=[1], cap=2, pos=(0, true))]))
    print(tip([Stack(items=[1], cap=2, pos=(0, true))]))
"#
        )
    };
    assert_proved(&program(">="));
    assert_open(&program(">"), &["11 ensures failed", "16 ensures failed"]);
}

#[test]
fn a_borrowed_element_of_a_list_that_is_not_modeled_is_any_value() {
    let program = |claim: &str| {
        format!(
            r#"
class Tagged:
    balance: int64
    note: int64 | None

class Bag:
    items: list[int64]
    n: int64

def note(xs: list[Tagged]) -> int64:
    requires len(xs) > 0
    {claim}
    return xs[0].balance

def renote(xs: mut list[Tagged]):
    requires len(xs) > 0
    xs[0].balance = 1
    view mut first = xs[0]
    first.balance = 2

def refill(xs: mut list[Bag]):
    requires len(xs) > 0
    ensures xs[0].n == 3
    xs[0].n = 3
    xs[0].items = [i for i in range(3)]

def main():
    print(note([Tagged(balance=1, note=None)]))
    mut ts = [Tagged(balance=1, note=None)]
    renote(ts)
    mut bs = [Bag(items=[], n=0)]
    refill(bs)
"#
        )
    };
    assert_proved(&program("requires true"));
    assert_open(&program("ensures result >= 0"), &["12 ensures failed"]);
}

// ----- Unions and enums in contracts ----------------------------------------

#[test]
fn a_contract_reads_an_optional_result_and_an_enum_variant() {
    assert_proved(&format!(
        r#"{ACCOUNT}
enum Light:
    Red
    Green
    Off(int64)

def next(l: Light) -> Light:
    ensures l != Light.Red or result == Light.Green
    match l:
        case Light.Red:
            return Light.Green
        case Light.Green:
            return Light.Red
        case Light.Off(n):
            return Light.Off(n)

def after_red() -> int64:
    r = next(Light.Red)
    match r:
        case Light.Green:
            return 1
        case _:
            assert false
            return 0

def find(xs: list[Account], k: int64) -> Account | None:
    ensures result is None or result.balance == k
    for a in xs:
        if a.balance == k:
            return Account(balance=a.balance)
    return None

def use(xs: list[Account]) -> int64:
    ensures result >= 0
    r = find(xs, 3)
    if r is None:
        return 0
    assert r.balance == 3
    return r.balance

def pos(n: int64) -> int64 | None:
    ensures result is None or result > 0
    ensures n <= 0 or result == n
    if n > 0:
        return n
    return None

def chain(n: int64) -> int64 | None:
    ensures result is None or 0 < result <= 10
    if 0 < n and n <= 10:
        return n
    return None

def use_pos(n: int64) -> int64:
    requires n > 0
    ensures result == n
    r = pos(n)
    if r is None:
        assert false
        return 0
    return r

def same(a: int64 | None, b: int64 | None) -> bool:
    ensures result == (a == b)
    return a == b

def is5(a: int64 | None) -> bool:
    ensures result == (a == 5)
    return a == 5

def main():
    print(after_red())
    print(use([Account(balance=3)]))
    print(use_pos(4))
    print(same(None, None))
    print(same(1, None))
    print(is5(5))
    print(chain(4) is None)
"#
    ));
}

#[test]
fn a_wrong_contract_over_a_union_or_an_enum_fails() {
    assert_open(
        r#"
enum Light:
    Red
    Green
    Off(int64)

def bad_next(l: Light) -> Light:
    ensures result == Light.Green
    return l

def bad_pos(n: int64) -> int64 | None:
    ensures result is None or result > 0
    return n

def bad_same(a: int64 | None, b: int64 | None) -> bool:
    ensures result == (a == b)
    return true

def bad_off(l: Light, m: Light) -> bool:
    ensures result == (l == m)
    match l:
        case Light.Off(n):
            return true
        case _:
            return false

def pos(n: int64) -> int64 | None:
    ensures result is None or result > 0
    if n > 0:
        return n
    return None

def bad_use(n: int64) -> int64:
    r = pos(n)
    if r is None:
        return 0
    assert r > 1
    return r

def main():
    print(bad_use(1))
"#,
        &[
            "8 ensures failed",
            "12 ensures failed",
            "16 ensures failed",
            "20 ensures failed",
            "37 assert failed",
        ],
    );
}

#[test]
fn a_counterexample_names_the_variant_of_an_enum() {
    let Some(report) = verify(
        r#"
enum Light:
    Red
    Green
    Off(int64)

def lit(l: Light) -> int64:
    ensures result > 0
    match l:
        case Light.Off(n):
            return 0
        case _:
            return 1

def main():
    print(lit(Light.Red))
"#,
    ) else {
        return;
    };
    let failed = report
        .obligations
        .iter()
        .find_map(|obligation| match &obligation.status {
            Status::Failed { counterexample, .. } => Some(counterexample.clone()),
            _ => None,
        })
        .expect("the `ensures` fails");
    assert!(
        failed.contains(&("variant of l".to_string(), "Off".to_string())),
        "{failed:?}"
    );
}

// ----- Matches ----------------------------------------------------------------

const MATCHES: &str = r#"
enum Shape:
    Circle(int64)
    Rect(int64, int64)
    Empty

class Point:
    x: int64
    y: int64

def un(v: int64 | str | None) -> int64:
    ensures 1 <= result and result <= 3
    return match v:
        case int64 as i: 1
        case str as s: 2
        case None: 3

def flag(b: bool) -> int64:
    ensures result == (1 if b else 2)
    return match b:
        case true: 1
        case false: 2

def nested(r: Result[Shape, str]) -> int64:
    ensures 0 <= result and result <= 3
    return match r:
        case Ok(Shape.Circle(q)): 1
        case Ok(Shape.Rect(w, h)): 2
        case Ok(Shape.Empty): 3
        case Err(e): 0

def tup(p: (Shape, bool)) -> int64:
    ensures 0 <= result and result <= 2
    return match p:
        case (Shape.Empty, flag): 0
        case (other, true): 1
        case (other, false): 2

def guard(s: Shape) -> int64:
    ensures result >= 0
    return match s:
        case Shape.Circle(r) if r > 0: r
        case Shape.Rect(w, h) if w > 0 and h > 0 and w < 100 and h < 100: w * h
        case _: 0

def cls(p: Point) -> int64:
    ensures result >= 0
    return match p:
        case Point(x=0, y=0): 0
        case Point(x=x, y=y) if x > 0: x
        case _: 1

def lit(n: int64) -> int64:
    ensures result == (10 if n == 0 else (20 if n == 1 else 30))
    return match n:
        case 0: 10
        case 1: 20
        case _: 30

def text(s: str) -> int64:
    ensures result == (1 if s == "a" else 2)
    return match s:
        case "a": 1
        case _: 2

def seq(xs: list[int64]) -> int64:
    ensures result >= 0
    return match xs:
        case []: 0
        case [only]: 1
        case [first, *_, last]: 2

def quoted(s: Shape, t: str) -> int64:
    ensures result >= 0
    if t == "a (b":
        return match s:
            case Shape.Circle(r): 1
            case Shape.Rect(w, h): 2
            case Shape.Empty: 3
    return 0
"#;

#[test]
fn a_match_expression_has_the_value_of_the_arm_it_takes() {
    assert_proved(&format!(
        r#"{MATCHES}
def main():
    print(un(None))
    print(flag(true))
    print(nested(Ok(Shape.Empty)))
    print(tup((Shape.Empty, true)))
    print(guard(Shape.Circle(3)))
    print(cls(Point(x=1, y=2)))
    print(lit(1))
    print(text("a"))
    print(seq([1, 2]))
    print(quoted(Shape.Empty, "a (b"))
"#
    ));
}

#[test]
fn a_match_expression_does_not_have_the_value_of_another_arm() {
    assert_open(
        r#"
enum Shape:
    Circle(int64)
    Rect(int64, int64)
    Empty

def bad(s: Shape) -> int64:
    ensures result >= 1
    return match s:
        case Shape.Circle(r): 1
        case Shape.Rect(w, h): 2
        case Shape.Empty: 0

def bad_guard(s: Shape) -> int64:
    ensures result >= 1
    return match s:
        case Shape.Circle(r) if r > 0: r
        case Shape.Rect(w, h): 2
        case _: 0

def bad_un(v: int64 | str | None) -> int64:
    ensures result <= 2
    return match v:
        case int64 as i: 1
        case str as s: 2
        case None: 3

def main():
    print(bad(Shape.Empty))
"#,
        &["8 ensures failed", "15 ensures failed", "22 ensures failed"],
    );
}

#[test]
fn a_match_on_an_enum_deep_inside_a_parameter_takes_one_arm() {
    let program = |bound: &str| {
        format!(
            r#"
enum Shape:
    Circle(int64)
    Empty

class D1:
    e: Shape
class D2:
    d: D1
class D3:
    d: D2
class D4:
    d: D3
class D5:
    d: D4

def deep_match(a: D5) -> int64:
    ensures result >= {bound}
    match a.d.d.d.d.e:
        case Shape.Circle(r):
            return 1
        case Shape.Empty:
            return 2

def main():
    print(deep_match(D5(d=D4(d=D3(d=D2(d=D1(e=Shape.Empty)))))))
"#
        )
    };
    assert_proved(&program("1"));
    assert_open(&program("2"), &["18 ensures failed"]);
}

// ----- Calls on a union -------------------------------------------------------

const COUNTERS: &str = r#"
trait Counter:
    def bump(mut self, by: int64):
        requires 0 < by and by <= 10

    def read(self) -> int64:
        ensures result >= 0

class Up:
    n: int64

class Twice:
    n: int64

impl Counter for Up:
    def bump(mut self, by: int64):
        requires 0 < by and by <= 10
        ensures old(self.n) >= 1000 or self.n == old(self.n) + by
        if self.n < 1000:
            self.n += by

    def read(self) -> int64:
        ensures result == 1
        return 1

impl Counter for Twice:
    def bump(mut self, by: int64):
        requires 0 < by and by <= 10
        ensures old(self.n) >= 1000 or self.n == old(self.n) + 2 * by
        if self.n < 1000:
            self.n = self.n + 2 * by

    def read(self) -> int64:
        ensures result == 2
        return 2
"#;

#[test]
fn a_call_on_a_union_is_a_call_on_the_member_it_holds() {
    assert_proved(&format!(
        r#"{COUNTERS}
def go(c: Up | Twice) -> int64:
    ensures 1 <= result and result <= 2
    return c.read()

def local() -> int64:
    ensures result == 4
    mut u: Up | Twice = Up(n=1)
    u.bump(3)
    match u:
        case Up as x:
            return x.n
        case Twice as t:
            return 0

def twice() -> int64:
    ensures result == 7
    mut u: Up | Twice = Twice(n=1)
    u.bump(3)
    match u:
        case Up as x:
            return 0
        case Twice as t:
            return t.n

def main():
    mut u: Up | Twice = Up(n=1)
    u.bump(2)
    print(go(u))
    print(local())
    print(twice())
"#
    ));
}

#[test]
fn a_call_on_a_union_proves_the_requires_of_each_member() {
    let Some(report) = verify(&format!(
        r#"{COUNTERS}
def step(c: mut Up | Twice):
    c.bump(3)

def far(c: mut Up | Twice):
    c.bump(11)

def exact(c: Up | Twice) -> int64:
    ensures result == 1
    return c.read()

def main():
    mut u: Up | Twice = Up(n=1)
    step(u)
"#
    )) else {
        return;
    };
    assert_eq!(
        open_messages(&report),
        [
            "`requires` of `Counter for Twice.bump` holds: `0 < by and by <= 10`",
            "`requires` of `Counter for Up.bump` holds: `0 < by and by <= 10`",
            "`ensures` clause `result == 1`",
        ],
        "{report:#?}"
    );
}

// ----- Trait objects in a list ------------------------------------------------

#[test]
fn a_trait_object_read_from_a_list_meets_the_trait_contract() {
    let program = |bound: &str| {
        format!(
            r#"
trait Shape:
    def area(self) -> int64:
        ensures result >= 0

class Sq:
    side: int64
    invariant 0 <= self.side and self.side <= 1000

impl Shape for Sq:
    def area(self) -> int64:
        return self.side * self.side

def first(shapes: list[dyn Shape]) -> int64:
    requires len(shapes) > 0
    ensures result >= {bound}
    return shapes[0].area()

def total(shapes: list[dyn Shape]) -> int64:
    ensures result >= {bound}
    mut sum = 0
    for s in shapes:
        invariant sum >= 0
        a = s.area()
        if sum < 1000000:
            sum += a % 1000
    return sum

def main():
    print(Sq(side=2).area())
"#
        )
    };
    assert_proved(&program("0"));
    assert_open(&program("1"), &["16 ensures failed", "20 ensures failed"]);
}

// ----- The largest and smallest of a list in a field --------------------------

#[test]
fn the_largest_of_a_list_in_a_field_needs_the_list_non_empty() {
    assert_proved(
        r#"
class Box:
    items: list[int64]

    public def top(self) -> int64:
        requires len(self.items) > 0
        ensures result >= min(self.items)
        return max(self.items)

def top(b: Box) -> int64:
    requires len(b.items) > 0
    ensures result >= min(b.items)
    return max(b.items)

def low(b: Box) -> int64:
    requires len(b.items) > 0
    ensures result <= b.items[0]
    return min(b.items)

def main():
    print(top(Box(items=[1, 2])))
    print(low(Box(items=[1, 2])))
    print(Box(items=[3]).top())
"#,
    );
    assert_open(
        r#"
class Box:
    items: list[int64]

def top(b: Box) -> int64:
    return max(b.items)

def low(b: Box) -> int64:
    requires len(b.items) > 0
    ensures result < b.items[0]
    return min(b.items)

def main():
    print(top(Box(items=[1, 2])))
"#,
        &["6 assert failed", "10 ensures failed"],
    );
}

// ----- Recursive spec defs ----------------------------------------------------

#[test]
fn a_spec_def_past_its_unfolding_limit_is_not_modeled() {
    let Some(report) = verify(
        r#"
spec def tri(n: int64) -> int64:
    requires 0 <= n and n <= 1000
    return 0 if n == 0 else n + tri(n - 1)

def far(n: int64) -> int64:
    requires 12 <= n and n <= 20
    ensures tri(n) >= 78
    return 1

def near() -> int64:
    ensures result == tri(4)
    return 10

def wrong() -> int64:
    ensures result == tri(4)
    return 11

def main():
    print(far(12))
    print(near())
"#,
    ) else {
        return;
    };
    assert_eq!(
        open_messages(&report),
        [
            "`ensures` clause `tri(n) >= 78`: it depends on `spec def tri` past 8 levels of \
             recursion at line 9, which the verifier does not model",
            "`spec def tri` needs at most 8 levels of recursion here: it depends on \
             `spec def tri` past 8 levels of recursion at line 9, which the verifier does not \
             model",
            "`ensures` clause `result == tri(4)`",
        ],
        "{report:#?}"
    );
    assert_eq!(
        open_checks(&report),
        [
            "8 ensures unsupported",
            "9 assert unsupported",
            "16 ensures failed"
        ]
    );
}

// ----- The models against the interpreter -------------------------------------

#[test]
fn the_shape_models_agree_with_the_interpreter() {
    assert_proved(
        r#"
enum Light:
    Red
    Green
    Off(int64)

def code(l: Light) -> int64:
    ensures l != Light.Red or result == 0
    ensures l != Light.Green or result == 1
    ensures l == Light.Red or l == Light.Green or result >= 2
    return match l:
        case Light.Red: 0
        case Light.Green: 1
        case Light.Off(n): 2 if n < 0 or n > 100 else 2 + n

def is_red(l: Light) -> bool:
    ensures result == (l == Light.Red)
    match l:
        case Light.Red:
            return true
        case _:
            return false

def off_is(l: Light, n: int64) -> bool:
    r = l == Light.Off(n)
    match l:
        case Light.Off(m):
            assert r == (m == n)
        case _:
            assert not r
    return r

def same(a: int64 | None, b: int64 | None) -> bool:
    ensures result == (a == b)
    if a is None:
        return b is None
    if b is None:
        return false
    return a == b

def is5(a: int64 | None) -> bool:
    ensures result == (a == 5)
    if a is None:
        return false
    return a == 5

def clamp(a: int64 | None) -> int64 | None:
    ensures result is None or 0 <= result <= 10
    ensures a is not None or result is None
    if a is None:
        return None
    if a < 0 or a > 10:
        return None
    return a

def kind(v: int64 | str | None) -> int64:
    ensures 1 <= result and result <= 3
    return match v:
        case int64 as i: 1
        case str as s: 2
        case None: 3

def sign(n: int64) -> int64:
    ensures result == (0 if n == 0 else (1 if n > 0 else -1))
    return match n:
        case 0: 0
        case k if k > 0: 1
        case _: -1

def main():
    lights = [Light.Red, Light.Green, Light.Off(-1), Light.Off(0), Light.Off(7), Light.Off(1000)]
    for l in lights:
        print(code(l))
        print(is_red(l))
        for n in range(-2, 9):
            print(off_is(l, n))
    for i in range(-3, 13):
        print(same(i, None))
        print(same(None, i))
        print(is5(i))
        print(clamp(i) is None)
        print(kind(i))
        print(sign(i))
        for j in range(-3, 13):
            assert same(i, j) == (i == j)
    assert same(None, None)
    assert not is5(None)
    assert clamp(None) is None
    print(kind("a"))
    print(kind(None))
    assert Light.Red != Light.Green
    assert Light.Off(1) == Light.Off(1)
    assert Light.Off(1) != Light.Off(2)
"#,
    );
}

#[test]
fn a_union_call_with_an_arm_that_is_no_function_is_not_modeled() {
    use crate::mir::{CallTarget, Instruction, MethodDispatch, Rvalue};
    let source = r#"
trait Get:
    def get(self) -> int64:
        ensures result >= 0

class A:
    x: int64
class B:
    y: int64
impl Get for A:
    def get(self) -> int64:
        return 1
impl Get for B:
    def get(self) -> int64:
        return 2

def call(v: A | B) -> int64:
    ensures result >= 0
    return v.get()

def main():
    print(call(A(x=1)))
"#;
    let options = Options::default();
    if smt::SolverCommand::find(options.solver.as_deref()).is_err() {
        return;
    }
    let program = crate::check_source(source).expect("the program checks");
    let mut module = crate::mir::lower_for_verification(&program);
    let instructions = module
        .functions
        .iter_mut()
        .flat_map(|function| &mut function.blocks)
        .flat_map(|block| &mut block.instructions);
    let mut changed = 0;
    for instruction in instructions {
        if let Instruction::Assign {
            value:
                Rvalue::Call {
                    callee:
                        CallTarget::TraitMember {
                            dispatch: Some(MethodDispatch::Union(arms)),
                            ..
                        },
                    ..
                },
            ..
        } = instruction
        {
            arms[0] = MethodDispatch::Nothing;
            changed += 1;
        }
    }
    assert_eq!(changed, 1);
    let report = verify_module(&program, &module, &options).expect("the verifier runs");
    assert_eq!(
        open_checks(&report),
        ["18 ensures unsupported"],
        "{report:#?}"
    );
}
