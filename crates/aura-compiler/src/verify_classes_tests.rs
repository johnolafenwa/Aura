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

// ----- The frame of a call ---------------------------------------------

#[test]
fn a_call_keeps_the_fields_its_callee_cannot_write() {
    assert_proved(
        r#"class Stack:
    items: list[int64]
    cap: int64
    invariant 0 <= len(self.items) and len(self.items) <= self.cap
    public def push(mut self, x: int64):
        requires len(self.items) < self.cap
        ensures len(self.items) == len(old(self.items)) + 1
        self.items.append(x)
def fill(s: mut Stack):
    requires s.cap == 4 and len(s.items) == 0
    s.push(1)
    s.push(2)
    assert len(s.items) == 2
def main():
    mut s = Stack(items=[], cap=4)
    fill(s)
"#,
    );
}

#[test]
fn a_call_forgets_exactly_what_its_callee_may_write() {
    // A direct write, a write in a nested call, a write through an
    // element, and a list method each count against the field they reach.
    assert_open(
        r#"class Stack:
    items: list[int64]
    cap: int64
    invariant 0 <= len(self.items) and len(self.items) <= self.cap
    def shrink(mut self):
        requires self.cap > 0
        ensures self.cap == old(self.cap) - 1
        self.cap -= 1
    public def push(mut self, x: int64):
        requires len(self.items) < self.cap
        ensures len(self.items) == len(old(self.items)) + 1
        self.items.append(x)
    public def push_shrink(mut self, x: int64):
        requires len(self.items) + 1 < self.cap
        ensures len(self.items) == len(old(self.items)) + 1
        self.items.append(x)
        self.cap -= 1
    public def push_nested(mut self, x: int64):
        requires len(self.items) + 1 < self.cap
        ensures len(self.items) == len(old(self.items)) + 1
        self.items.append(x)
        self.shrink()
    public def poke(mut self):
        requires len(self.items) > 0
        self.items[0] = 9
    public def quiet(mut self, x: int64):
        requires len(self.items) < self.cap
        self.items.append(x)
def fill3(s: mut Stack):
    requires s.cap == 4 and len(s.items) == 0
    s.push(1)
    s.push(2)
    assert len(s.items) == 3
def fill_shrink(s: mut Stack):
    requires s.cap == 4 and len(s.items) == 0
    s.push_shrink(1)
    assert s.cap == 4
def fill_nested(s: mut Stack):
    requires s.cap == 4 and len(s.items) == 0
    s.push_nested(1)
    assert s.cap == 4
def fill_poke(s: mut Stack):
    requires s.cap == 4 and len(s.items) == 1 and s.items[0] == 1
    s.poke()
    assert s.cap == 4
    assert s.items[0] == 1
def fill_quiet(s: mut Stack):
    requires s.cap == 4 and len(s.items) == 0
    s.quiet(1)
    assert s.cap == 4
    assert len(s.items) == 0
def main():
    mut s = Stack(items=[], cap=4)
    fill3(s)
"#,
        &[
            "33 assert failed",
            "37 assert failed",
            "41 assert failed",
            "46 assert failed",
            "46 index in range failed",
            "51 assert failed",
        ],
    );
}

#[test]
fn a_frame_follows_arguments_loans_and_function_values() {
    assert_open(
        r#"trait Tick:
    def tick(mut self)

class Inner:
    n: int64
    m: int64

class Outer:
    inner: Inner
    pair: (int64, int64)
    rows: list[Inner]
    k: int64

impl Tick for Outer:
    def tick(mut self):
        pass

def bump(c: mut Inner):
    requires c.n < 100
    ensures c.n == old(c.n) + 1
    c.n += 1

def through(o: mut Outer):
    requires o.inner.n < 100
    ensures o.inner.n == old(o.inner.n) + 1
    bump(c=o.inner)

def first(o: mut Outer):
    requires o.pair[0] < 100
    o.pair = (o.pair[0] + 1, 0)

def row(o: mut Outer):
    requires len(o.rows) > 0
    view mut r = o.rows[0]
    r.n = 7

def whole(o: mut Outer):
    view mut i = o.inner
    i.m = 3

def two(a: mut Inner, b: mut Inner):
    requires a.n < 100
    ensures a.n == old(a.n) + 1
    a.n += 1

def by_closure(o: mut Outer):
    mut t = 0
    f = lambda: t
    print(f())

def ok(o: mut Outer, d: mut dyn Tick):
    requires o.inner.n == 1 and o.inner.m == 5 and o.k == 2 and o.pair[0] == 1 and o.pair[1] == 8
    requires len(o.rows) == 1 and o.rows[0].n == 1
    through(o)
    assert o.inner.n == 2
    assert o.inner.m == 5
    assert o.k == 2
    first(o)
    assert o.inner.m == 5
    whole(o)
    assert o.k == 2
    row(o)
    assert o.k == 2
    mut x = Inner(n=1, m=1)
    mut y = Inner(n=1, m=4)
    two(x, y)
    assert y.m == 4 and y.n == 1 and x.m == 1
    g = bump
    g(x)
    assert x.m == 1
    by_closure(o)
    assert o.k == 2

def bad(o: mut Outer, d: mut dyn Tick):
    requires o.inner.n == 1 and o.inner.m == 5 and o.k == 2 and o.pair[0] == 1 and o.pair[1] == 8
    requires len(o.rows) == 1 and o.rows[0].n == 1
    first(o)
    assert o.pair[1] == 8
    whole(o)
    assert o.inner.n == 1
    row(o)
    assert o.rows[0].n == 1

def bad_dyn(d: mut dyn Tick, o: mut Outer):
    requires o.k == 2
    mut e: dyn Tick = Outer(inner=Inner(n=1, m=1), pair=(1, 2), rows=[], k=2)
    e.tick()
    o.tick()
    assert o.k == 2
    d.tick()

def main():
    print(1)
"#,
        &[
            "78 assert failed",
            "80 assert failed",
            "82 assert failed",
            "82 index in range failed",
        ],
    );
}

// ----- Class invariants in trait implementations ------------------------

#[test]
fn a_trait_implementation_method_assumes_its_class_invariant() {
    assert_proved(
        r#"trait Shape:
    def area(self) -> int64:
        ensures result >= 0
class Sq:
    side: int64
    invariant 0 <= self.side and self.side <= 1000
impl Shape for Sq:
    def area(self) -> int64:
        return self.side * self.side
def main():
    print(Sq(side=2).area())
"#,
    );
    // A call on a trait object relies on the object holding the invariant.
    assert_proved(
        r#"trait Shape:
    def area(self) -> int64:
        ensures result >= 0

class Sq:
    side: int64
    invariant 0 <= self.side and self.side <= 1000

    public def double(self) -> int64:
        return self.side * 2

impl Shape for Sq:
    def area(self) -> int64:
        return self.side * self.side

def total(shapes: list[dyn Shape]) -> int64:
    ensures result >= 0
    mut sum = 0
    for s in shapes:
        invariant sum >= 0
        a = s.area()
        if sum < 1000000:
            sum += a % 1000
    return sum

def main():
    print(Sq(side=2).area())
"#,
    );
    // A generic function is verified as the copy each call uses, where
    // the call names the method and proves the invariant.
    assert_proved(
        r#"trait Shape:
    def area(self) -> int64:
        ensures result >= 0
class Sq:
    side: int64
    invariant 0 <= self.side and self.side <= 1000
class Ci:
    r: int64
impl Shape for Sq:
    def area(self) -> int64:
        return self.side * self.side
impl Shape for Ci:
    def area(self) -> int64:
        return 1
def twice[T: Shape](x: T) -> int64:
    ensures result >= 0
    return x.area() % 100 + x.area() % 100
def boxed[T: Shape](x: own T) -> dyn Shape:
    return x
def main():
    print(twice(Sq(side=2)))
    print(boxed(Sq(side=3)).area())
"#,
    );
}

#[test]
fn a_trait_implementation_method_of_a_class_without_invariant_assumes_nothing() {
    assert_open(
        r#"trait Shape:
    def area(self) -> int64:
        ensures result >= 0
class Sq:
    side: int64
impl Shape for Sq:
    def area(self) -> int64:
        return self.side * self.side
def main():
    print(Sq(side=2).area())
"#,
        &["8 overflow failed"],
    );
}

#[test]
fn each_way_to_a_trait_implementation_method_proves_the_invariant() {
    // A call on `self` while the invariant is broken, a `mut self` method
    // that breaks it, a call on a broken object, and a trait object made
    // from one.
    assert_open(
        r#"trait Shape:
    def area(self) -> int64:
        ensures result >= 0
    def grow(mut self)
    def shrink(mut self)
class Sq:
    side: int64
    invariant 0 <= self.side and self.side <= 1000
    def poke(mut self) -> int64:
        self.side = 0 - 5
        r = self.area()
        self.side = 5
        return r
impl Shape for Sq:
    def area(self) -> int64:
        return self.side * self.side
    def grow(mut self):
        self.side += 1
    def shrink(mut self):
        if self.side > 0:
            self.side -= 1
def broken_static() -> int64:
    mut s = Sq(side=2)
    s.side = 5000000000
    return s.area()
def broken_object() -> int64:
    mut s = Sq(side=2)
    s.side = 5000000000
    d: dyn Shape = s
    return d.area()
def fine(shapes: mut list[dyn Shape]) -> int64:
    requires len(shapes) > 0
    shapes[0].shrink()
    mut d: dyn Shape = Sq(side=3)
    d.shrink()
    return d.area() % 7 + shapes[0].area() % 7
def main():
    print(broken_static())
"#,
        &[
            "11 class invariant failed",
            "18 class invariant failed",
            "25 class invariant failed",
            "29 class invariant failed",
            "30 class invariant failed",
        ],
    );
}

#[test]
fn a_method_reached_without_proof_assumes_nothing() {
    // A call on a union runs the method with no proof about the receiver.
    assert_open(
        r#"trait Shape:
    def area(self) -> int64:
        ensures result >= 0
class Sq:
    side: int64
    invariant 0 <= self.side and self.side <= 1000
class Ci:
    r: int64
impl Shape for Sq:
    def area(self) -> int64:
        return self.side * self.side
impl Shape for Ci:
    def area(self) -> int64:
        return 1
def main():
    u: Sq | Ci = Sq(side=2)
    print(u.area())
"#,
        &["11 overflow failed"],
    );
    // So does a generic original that a call could not copy.
    assert_open(
        r#"trait Shape:
    def area(self) -> int64:
        ensures result >= 0
class Sq:
    side: int64
    invariant 0 <= self.side and self.side <= 1000
class Ci:
    r: int64
impl Shape for Sq:
    def area(self) -> int64:
        return self.side * self.side
impl Shape for Ci:
    def area(self) -> int64:
        return 1
def pick[T: Shape, U](x: T, y: U | int64) -> int64:
    return x.area() % 100
def main():
    print(pick[Sq, int64](Sq(side=2), 5))
"#,
        &["11 overflow failed"],
    );
    // The renderer calls `display` on any value it prints.
    assert_open(
        r#"trait Shape:
    def area(self) -> int64:
        ensures result >= 0
class Sq:
    side: int64
    invariant 0 <= self.side and self.side <= 1000
class Ci:
    r: int64
impl Shape for Sq:
    def area(self) -> int64:
        return self.side * self.side
impl Shape for Ci:
    def area(self) -> int64:
        return 1
impl Display for Sq:
    def display(self) -> str:
        x = self.side * self.side
        return f"sq {x}"
def main():
    print(Sq(side=2))
"#,
        &["17 overflow failed"],
    );
}

#[test]
fn an_importing_module_answers_for_calls_that_prove_nothing() {
    if smt::SolverCommand::find(None).is_err() {
        return;
    }
    let dir = std::env::temp_dir().join(format!("aura_verify_guard_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("src")).expect("src");
    std::fs::write(
        dir.join("Aura.toml"),
        "[package]\nname = \"guard\"\nversion = \"0.1.0\"\nedition = \"2026\"\n",
    )
    .expect("manifest");
    std::fs::write(
        dir.join("src/shapes.au"),
        r#"public trait Shape:
    def area(self) -> int64:
        ensures result >= 0

public class Sq:
    public side: int64
    invariant 0 <= self.side and self.side <= 1000

public class Ci:
    public r: int64

impl Shape for Sq:
    def area(self) -> int64:
        return self.side * self.side

impl Shape for Ci:
    def area(self) -> int64:
        return 1
"#,
    )
    .expect("shapes.au");
    let main = dir.join("src/main.au");
    std::fs::write(
        &main,
        r#"from shapes import Shape, Sq, Ci

def on_union(u: Sq | Ci) -> int64:
    return u.area()

def broken() -> int64:
    mut s = Sq(side=2)
    s.side = 5000000000
    return s.area()

def kept() -> int64:
    return Sq(side=2).area()

def main():
    print(kept())
"#,
    )
    .expect("main.au");
    // The module that declares the method sees nothing that reaches it
    // without proof, so the method assumes its invariant.
    let program = crate::check_path(&dir.join("src/shapes.au")).expect("shapes checks");
    let module = crate::mir::lower_for_verification(&program);
    let report = verify_module(&program, &module, &Options::default()).expect("verifies");
    assert_eq!(open_checks(&report), Vec::<String>::new(), "{report:#?}");
    // The importing module cannot withdraw that, so its call on a union
    // is an open obligation, and its call on a broken object fails.
    let program = crate::check_path(&main).expect("main checks");
    let module = crate::mir::lower_for_verification(&program);
    let report = verify_module(&program, &module, &Options::default()).expect("verifies");
    assert_eq!(
        open_checks(&report),
        ["3 class invariant unsupported", "9 class invariant failed"],
        "{report:#?}"
    );
    let _ = std::fs::remove_dir_all(&dir);
}
