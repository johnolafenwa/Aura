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

fn assert_open(source: &str, expected: &[&str]) -> Option<Report> {
    let report = verify(source)?;
    assert_eq!(open_checks(&report), expected, "{report:#?}");
    Some(report)
}

/// The number of index checks the report holds.
fn index_checks(report: &Report) -> usize {
    report
        .obligations
        .iter()
        .filter(|obligation| obligation.kind == ObligationKind::IndexInRange)
        .count()
}

#[test]
fn a_span_parameter_is_indexed_inside_its_own_length() {
    assert_proved(
        r#"
def first(xs: Span[int64]) -> int64:
    requires len(xs) > 0
    ensures result == xs[0]
    return xs[0]

def last(xs: Span[int64]) -> int64:
    requires len(xs) > 0
    ensures result == xs[len(xs) - 1]
    return xs[-1]

def total(xs: Span[int64]) -> int64:
    mut t = 0
    for i in range(len(xs)):
        t = t.wrapping_add(xs[i])
    return t

def count_up(xs: Span[int64]) -> int64:
    mut t = 0
    for x in xs:
        t = t.wrapping_add(x)
    return t

def main():
    ys = [1, 2, 3, 4]
    a = first(ys[1:3])
    assert a == 2
    b = last(ys[1:3])
    assert b == 3
    print(a + b)
    print(total(ys[:2]))
    print(count_up(ys[2:]))
"#,
    );
    // One check for each access: the base list's own check of a position a
    // span index gave is not a second one.
    let Some(report) = verify(
        r#"
def first(xs: Span[int64]) -> int64:
    requires len(xs) > 0
    return xs[0]

def main():
    print(1)
"#,
    ) else {
        return;
    };
    assert!(report.is_proved());
    assert_eq!(index_checks(&report), 1);
}

#[test]
fn a_span_index_outside_the_span_is_not_proved() {
    // Without the `requires`, the span may be empty.
    let Some(report) = assert_open(
        r#"
def first(xs: Span[int64]) -> int64:
    return xs[0]

def main():
    print(1)
"#,
        &["3 index in range failed"],
    ) else {
        return;
    };
    // The counterexample names the span's own length.
    let text = format!("{report:?}");
    assert!(text.contains("len(xs)"), "{text}");
    assert!(text.contains("len(base list of xs)"), "{text}");
    assert!(text.contains("start of xs"), "{text}");
    // The base list is longer than the span, which does not make a
    // position past the span's end one of the span's.
    assert_open(
        r#"
def past(xs: Span[int64]) -> int64:
    return xs[len(xs)]

def before(xs: Span[int64]) -> int64:
    requires len(xs) > 0
    return xs[-len(xs) - 1]

def from_end(xs: Span[int64]) -> int64:
    requires len(xs) > 1
    return xs[-2]

def walk(xs: Span[int64]) -> int64:
    mut t = 0
    for i in range(len(xs)):
        t = t.wrapping_add(xs[i + 1])
    return t

def main():
    print(1)
"#,
        &[
            "3 index in range failed",
            "7 index in range failed",
            "16 index in range failed",
        ],
    );
}

#[test]
fn a_span_made_in_the_function_keeps_its_range() {
    assert_proved(
        r#"
def main():
    mut ys = [10, 20, 30, 40, 50]
    view part = ys[1:4]
    assert len(part) == 3
    assert part[0] == 20
    assert part[-1] == 40
    view inner = part[1:]
    assert len(inner) == 2
    assert inner[0] == 30
    view tail = ys[-2:]
    assert tail[1] == 50
    view whole = ys[:]
    assert len(whole) == 5
    print(part[2] + inner[1] + tail[0] + whole[4])
"#,
    );
    assert_open(
        r#"
def past(ys: list[int64]) -> int64:
    requires len(ys) == 5
    view part = ys[1:4]
    return part[3]

def past_inner(ys: list[int64]) -> int64:
    requires len(ys) == 5
    view part = ys[1:4]
    view inner = part[1:]
    return inner[2]

def before_inner(ys: list[int64]) -> int64:
    requires len(ys) == 5
    view part = ys[1:4]
    view inner = part[1:]
    return inner[-3]

def main():
    print(1)
"#,
        &[
            "5 index in range failed",
            "11 index in range failed",
            "17 index in range failed",
        ],
    );
    // A range outside the list, or outside the span it is taken from, and
    // a reversed range stop the program where the span is made.
    assert_open(
        r#"
def outside(ys: list[int64]) -> int64:
    requires len(ys) == 5
    view part = ys[1:9]
    return len(part)

def reversed_range(ys: list[int64]) -> int64:
    requires len(ys) == 5
    view part = ys[3:2]
    return len(part)

def nested(ys: list[int64]) -> int64:
    requires len(ys) == 5
    view part = ys[1:3]
    view inner = part[0:3]
    return len(inner)

def unknown(ys: list[int64], n: int64) -> int64:
    view part = ys[n:]
    return len(part)

def main():
    print(1)
"#,
        &[
            "4 index in range failed",
            "9 index in range failed",
            "15 index in range failed",
            "19 index in range failed",
        ],
    );
}

#[test]
fn a_callee_contract_reads_a_span_argument_by_its_range() {
    // `len(xs)` and `xs[i]` in a contract are the span's, not the base
    // list's.
    assert_open(
        r#"
def first(xs: Span[int64]) -> int64:
    requires len(xs) > 0
    ensures result == xs[0]
    return xs[0]

def positive(xs: Span[int64]) -> int64:
    requires forall(i in range(len(xs)), xs[i] > 0)
    return len(xs)

def empty(ys: list[int64]) -> int64:
    requires len(ys) == 4
    return first(ys[2:2])

def other(ys: list[int64]) -> int64:
    requires len(ys) == 4 and ys[0] == 0 and ys[1] == 2
    a = first(ys[1:3])
    assert a == 0
    return a

def main():
    ys = [0, 2, 3, 4]
    print(positive(ys[1:]))
    print(positive(ys[:2]))
"#,
        &[
            "13 requires failed",
            "18 assert failed",
            "24 requires failed",
        ],
    );
    // An assertion in the body reads a span parameter the same way.
    assert_open(
        r#"
def head(xs: Span[int64]) -> int64:
    requires len(xs) > 1
    requires forall(i in range(len(xs)), xs[i] == 7)
    assert all([xs[i] == 7 for i in range(len(xs))])
    assert xs[0] == 8
    return xs[1]

def main():
    print(1)
"#,
        &["6 assert failed"],
    );
}

#[test]
fn a_write_through_a_span_reaches_its_list() {
    assert_proved(
        r#"
def clear_second(xs: mut Span[int64]) -> None:
    requires len(xs) > 1
    ensures xs[1] == 0
    xs[1] = 0

def main():
    mut ys = [1, 2, 3, 4]
    view mut m = ys[0:3]
    m[0] = 9
    m[-1] = 8
    assert ys[0] == 9
    assert ys[1] == 2
    assert ys[2] == 8
    view mut n = ys[1:]
    clear_second(n)
    assert n[1] == 0
    print(ys[3])
"#,
    );
    // The callee may write any element of the range, so the list is not
    // what it was. Its length is.
    assert_open(
        r#"
def clear_second(xs: mut Span[int64]) -> None:
    requires len(xs) > 1
    xs[1] = 0

def main():
    mut ys = [1, 2, 3, 4]
    view mut m = ys[0:3]
    clear_second(m)
    assert ys[1] == 2
    print(ys[3])
    print(ys[4])
"#,
        &["10 assert failed", "12 index in range failed"],
    );
    assert_open(
        r#"
def clear_second(xs: mut Span[int64]) -> None:
    requires len(xs) > 1
    xs[1] = 0

def main():
    mut ys = [1, 2, 3, 4]
    clear_second(ys[0:3])
    assert ys[1] == 2
    clear_second(ys[3:])
"#,
        &["9 assert failed", "10 requires failed"],
    );
}

#[test]
fn a_write_through_a_list_view_reaches_its_list() {
    // A whole-list view passed to a `mut` parameter.
    assert_open(
        r#"
def clear_second(xs: mut list[int64]) -> None:
    requires len(xs) > 1
    xs[1] = 0

def main():
    mut ys = [1, 2, 3, 4]
    view mut m = ys
    clear_second(m)
    assert ys[1] == 2
"#,
        &["10 assert failed", "10 index in range failed"],
    );
    // A view of one element passed to a `mut` parameter.
    assert_open(
        r#"
def clear_second(xs: mut list[int64]) -> None:
    requires len(xs) > 1
    xs[1] = 0

def main():
    mut grid = [[1, 2], [3, 4]]
    view mut row = grid[0]
    clear_second(row)
    assert grid[0][1] == 2
"#,
        // The row is read through the view, and the call's write reaches
        // it: the callee says nothing of the row's length or elements.
        &["10 assert failed", "10 index in range failed"],
    );
    // A view a function returned, which the encoding does not follow.
    assert_open(
        r#"
def pick(rows: mut list[list[int64]]) -> view mut list[int64] from rows:
    requires len(rows) > 0
    return view mut rows[0]

def clear_second(xs: mut list[int64]) -> None:
    requires len(xs) > 1
    xs[1] = 0

def main():
    mut grid = [[1, 2], [3, 4]]
    view mut row = pick(grid)
    clear_second(row)
    assert grid[0][1] == 2
"#,
        &[
            "13 requires failed",
            // What the write left in the row is not modeled, so these
            // are open without a counterexample.
            "14 assert unsupported",
            "14 index in range unsupported",
            "14 index in range unsupported",
        ],
    );
}

#[test]
fn span_members_that_write_keep_the_list_length() {
    assert_proved(
        r#"
def main():
    mut ys = [4, 3, 2, 1]
    view mut m = ys[1:]
    m.sort()
    m.reverse()
    m.fill(7)
    m.swap(0, 2)
    print(3 in m)
    print(m.count(7))
    print(m[2])
    print(ys[3])
"#,
    );
    assert_open(
        r#"
def main():
    mut ys = [4, 3, 2, 1]
    view mut m = ys[1:]
    m.fill(7)
    assert ys[1] == 3
    view mut n = ys[1:]
    n.sort()
    assert ys[0] == 4
    view k = ys[1:]
    print(k[3])
"#,
        &[
            "6 assert failed",
            "9 assert failed",
            "11 index in range failed",
        ],
    );
}

#[test]
fn an_array_index_needs_one_coordinate_in_range_for_each_axis() {
    assert_proved(
        r#"
def dot(a: Array[float64], b: Array[float64]) -> float64:
    requires len(a.shape()) == 1 and len(b.shape()) == 1
    requires len(a) == len(b)
    mut t = 0.0
    for i in range(len(a)):
        t += a[i] * b[i]
    return t

def corner(g: mut Array[int64]) -> int64:
    requires len(g.shape()) == 2 and g.shape()[0] > 1 and g.shape()[1] > 2
    g[0, 0] = 5
    g[-2, -3] = 6
    return g[1, 2].wrapping_add(g[-1, -1])

def deep(g: mut Array[int64]) -> int64:
    requires len(g.shape()) == 5
    requires forall(k in range(5), g.shape()[k] == 2)
    g[1, 1, 1, 1, 1] = 3
    return g[0, 1, 0, 1, -2]

def last(a: Array[int64]) -> int64:
    requires len(a.shape()) == 1 and a.len() > 0
    return a[len(a) - 1].wrapping_add(a[-1])

def main():
    a = Array[float64].from_list([1.0, 2.0, 3.0], [3])
    b = Array[float64].full([3], 0.5)
    print(dot(a, b))
    mut g = Array[int64].zeros([2, 3])
    print(corner(g))
    mut d = Array[int64].zeros([2, 2, 2, 2, 2])
    print(deep(d))
    c = Array[int64].from_list([4, 5], [2])
    assert len(c) == 2
    print(last(c))
    print(c[1])
"#,
    );
    // Every element access is a check.
    let Some(report) = verify(
        r#"
def dot(a: Array[float64], b: Array[float64]) -> float64:
    requires len(a.shape()) == 1 and len(b.shape()) == 1
    requires len(a) == len(b)
    mut t = 0.0
    for i in range(len(a)):
        t += a[i] * b[i]
    return t

def main():
    print(1)
"#,
    ) else {
        return;
    };
    assert!(report.is_proved());
    assert_eq!(index_checks(&report), 2);
}

#[test]
fn an_array_index_that_may_stop_the_program_is_not_proved() {
    // With no `requires`, `b` may be shorter than `a`, and either may have
    // two axes, where one coordinate is not an index.
    assert_open(
        r#"
def dot(a: Array[float64], b: Array[float64]) -> float64:
    mut t = 0.0
    for i in range(len(a)):
        t += a[i] * b[i]
    return t

def same_length(a: Array[float64], b: Array[float64]) -> float64:
    requires len(a) == len(b)
    mut t = 0.0
    for i in range(len(a)):
        t += a[i] * b[i]
    return t

def one_axis(a: Array[float64], b: Array[float64]) -> float64:
    requires len(a.shape()) == 1 and len(b.shape()) == 1
    mut t = 0.0
    for i in range(len(a)):
        t += a[i] * b[i]
    return t

def past(a: Array[float64]) -> float64:
    requires len(a.shape()) == 1
    return a[len(a)]

def main():
    print(1)
"#,
        &[
            "5 index in range failed",
            "5 index in range failed",
            "12 index in range failed",
            "12 index in range failed",
            "19 index in range failed",
            "24 index in range failed",
        ],
    );
    assert_open(
        r#"
def low(g: mut Array[int64]) -> None:
    requires len(g.shape()) == 2 and g.shape()[0] == 2 and g.shape()[1] == 3
    g[2, 0] = 5

def high(g: mut Array[int64]) -> None:
    requires len(g.shape()) == 2 and g.shape()[0] == 2 and g.shape()[1] == 3
    g[0, -4] = 5

def few(g: mut Array[int64]) -> None:
    requires len(g.shape()) == 2 and g.shape()[0] == 2 and g.shape()[1] == 3
    g[0] = 5

def read(g: Array[int64]) -> int64:
    requires len(g.shape()) == 2 and g.shape()[0] == 2 and g.shape()[1] == 3
    return g[1, 3]

def many(g: Array[int64]) -> int64:
    requires len(g.shape()) == 2 and g.shape()[0] == 2 and g.shape()[1] == 3
    return g[0, 0, 0]

def deep_write(g: mut Array[int64]) -> None:
    requires len(g.shape()) == 5
    g[1, 1, 1, 1, 1] = 3

def deep_read(g: Array[int64]) -> int64:
    requires len(g.shape()) == 5
    return g[0, 0, 0, 0, 0]

def made() -> int64:
    a = Array[int64].zeros([3])
    return a[3]

def listed() -> int64:
    n = Array[int64].from_list([1, 2], [2])
    return n[2]

def main():
    mut n = Array[int64].full([2], 1)
    low(n)
"#,
        &[
            "4 index in range failed",
            "8 index in range failed",
            "12 index in range failed",
            "16 index in range failed",
            "20 index in range failed",
            "24 index in range failed",
            "28 index in range failed",
            "32 index in range failed",
            "36 index in range failed",
            "40 requires failed",
        ],
    );
}

#[test]
fn an_array_selection_is_reported_as_not_modeled() {
    // A slice bound or a `set` coordinate outside the Array stops the
    // program, and the encoding does not read them.
    assert_open(
        r#"
def cut(a: mut Array[float64], n: int64) -> float64:
    requires len(a.shape()) == 1 and len(a) > 2
    rest = a[n:]
    a[1:] = 0.5
    a[:2] += 1.0
    old_value = a.set([n], 2.0)
    kept = a.get([n])
    a.fill(1.0)
    total = a.sum() + rest.sum()
    return total + a[2]

def main():
    print(1)
"#,
        &[
            "4 index in range unsupported",
            "5 index in range unsupported",
            "6 index in range unsupported",
            "7 index in range unsupported",
        ],
    );
    // A contract reads an Array's element count and shape, and no other
    // member.
    assert_open(
        r#"
def total(a: Array[float64]) -> float64:
    requires a.mean() > 0.0
    return a.sum()

def count(d: dict[float64, int64]) -> int64:
    requires len(d) > 0
    return len(d)

def main():
    a = Array[float64].zeros([2])
    print(total(a))
    d: dict[float64, int64] = {1.0: 1}
    print(count(d))
"#,
        &["12 requires unsupported", "14 requires unsupported"],
    );
}
