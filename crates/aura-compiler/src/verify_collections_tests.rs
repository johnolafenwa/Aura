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
    let Some(report) = verify(source) else {
        return;
    };
    assert_eq!(open_checks(&report), expected, "{report:#?}");
}

/// Runs a program whose own `assert`s compare an operation with the facts
/// its model states, over a spread of inputs.
fn assert_runs(source: &str) {
    crate::run_source(source).expect("the model's facts hold when the program runs");
}

#[test]
fn membership_in_a_list_is_the_contract_s_exists() {
    assert_proved(
        r#"
def has(xs: list[int64], k: int64) -> bool:
    ensures result == exists(x in xs, x == k)
    return k in xs

def has_method(xs: list[int64], k: int64) -> bool:
    ensures result == (k in xs)
    return xs.contains(k)

def lacks(xs: list[str], k: str) -> bool:
    ensures result == forall(x in xs, x != k)
    return k not in xs

def found_at(xs: list[int64], i: int64) -> bool:
    requires 0 <= i and i < len(xs)
    ensures result
    return xs[i] in xs

def pick(xs: list[int64], k: int64) -> int64:
    ensures result == 1 or result == 0
    ensures (result == 1) == xs.contains(k)
    if k in xs:
        return 1
    return 0

def caller(xs: list[int64]) -> bool:
    requires len(xs) > 2
    ensures result
    return has(xs, xs[2])

def literal() -> bool:
    ensures result
    return 2 in [1, 2, 3]

def main():
    print(has([1, 2, 3], 2))
    print(has_method([1, 2, 3], 5))
    print(lacks(["a"], "b"))
    print(found_at([4, 5], 1))
    print(pick([4, 5], 4))
    print(caller([1, 2, 3]))
    print(literal())
"#,
    );
}

#[test]
fn a_wrong_claim_about_membership_still_fails() {
    assert_open(
        r#"
def always(xs: list[int64], k: int64) -> bool:
    ensures result
    return k in xs

def never(xs: list[int64], k: int64) -> bool:
    requires len(xs) > 0
    ensures not result
    return k in xs

def first_only(xs: list[int64], k: int64) -> bool:
    requires len(xs) > 0
    ensures result == (xs[0] == k)
    return k in xs

def absent() -> bool:
    ensures result
    return 4 in [1, 2, 3]

def main():
    print(always([1], 1))
    print(never([1], 2))
    print(first_only([1], 1))
    print(absent())
"#,
        &[
            "3 ensures failed",
            "8 ensures failed",
            "13 ensures failed",
            "17 ensures failed",
        ],
    );
}

#[test]
fn membership_of_a_float_stays_unmodeled() {
    assert_open(
        r#"
def has(xs: list[float64], k: float64) -> bool:
    ensures result
    return k in xs

def main():
    print(has([1.0], 1.0))
"#,
        &["3 ensures unsupported"],
    );
}

#[test]
fn membership_agrees_with_a_search_at_run_time() {
    assert_runs(
        r#"
def search(xs: list[int64], k: int64) -> bool:
    mut found = false
    for x in xs:
        if x == k:
            found = true
    return found

def check(xs: list[int64]):
    for k in range(-2, 6):
        assert (k in xs) == search(xs, k)
        assert xs.contains(k) == search(xs, k)

def main():
    check([])
    check([0])
    check([3, 1, 3])
    check([-2, 5, 4, 4, 0])
    names = ["a", "", "b"]
    assert "a" in names
    assert "" in names
    assert "c" not in names
"#,
    );
}

#[test]
fn a_slice_has_its_length_and_the_elements_it_copies() {
    assert_proved(
        r#"
def mid(xs: list[int64]) -> int64:
    requires len(xs) > 3
    ensures result == xs[1]
    ys = xs[1:3]
    assert len(ys) == 2
    assert len(xs[1:3]) == 2
    return ys[0]

def tail(xs: list[int64]) -> list[int64]:
    requires len(xs) > 0
    ensures len(result) == len(xs) - 1
    ensures forall(i in range(len(result)), result[i] == xs[i + 1])
    return xs[1:]

def head(xs: list[int64]) -> list[int64]:
    requires len(xs) >= 2
    ensures len(result) == len(xs) - 2
    return xs[:-2]

def last_two(xs: list[int64]) -> list[int64]:
    requires len(xs) >= 2
    ensures len(result) == 2
    ensures result[1] == xs[len(xs) - 1]
    ensures result[0] == xs[-2]
    return xs[-2:]

def copy(xs: list[int64]) -> list[int64]:
    ensures len(result) == len(xs)
    ensures forall(i in range(len(xs)), result[i] == xs[i])
    return xs[:]

def window(xs: list[int64], a: int64, b: int64) -> list[int64]:
    requires 0 <= a and a <= b and b <= len(xs)
    ensures len(result) == b - a
    ensures forall(i in range(b - a), result[i] == xs[a + i])
    return xs[a:b]

def changed(xs: list[int64]) -> int64:
    requires len(xs) > 2
    ensures result == xs[1]
    mut ys = xs[1:]
    kept = ys[0]
    ys[0] = kept + 0
    ys[1] = 7
    assert ys[1] == 7
    return ys[0]

def names(xs: list[(str, int64)]) -> int64:
    requires len(xs) > 1
    ensures result == xs[1][1]
    ys = xs[1:]
    return ys[0][1]

def main():
    xs = [1, 2, 3, 4, 5]
    print(mid(xs))
    print(len(tail(xs)))
    print(len(head(xs)))
    print(len(last_two(xs)))
    print(len(copy(xs)))
    print(len(window(xs, 1, 1)))
    print(changed(xs))
    print(names([("a", 1), ("b", 2)]))
"#,
    );
}

#[test]
fn a_slice_outside_the_list_and_a_wrong_claim_about_one_still_fail() {
    assert_open(
        r#"
def short(xs: list[int64]) -> int64:
    requires len(xs) > 1
    ys = xs[1:3]
    return len(ys)

def backwards(xs: list[int64]) -> int64:
    requires len(xs) > 3
    return len(xs[2:1])

def past_the_end(xs: list[int64]) -> int64:
    requires len(xs) > 3
    ys = xs[1:3]
    return ys[2]

def negative(xs: list[int64]) -> int64:
    requires len(xs) > 0
    return len(xs[-2:])

def wrong_element(xs: list[int64]) -> int64:
    requires len(xs) > 3
    ensures result == xs[0]
    ys = xs[1:3]
    return ys[0]

def wrong_length(xs: list[int64]) -> int64:
    requires len(xs) > 3
    ensures result == 3
    return len(xs[1:3])

def main():
    xs = [1, 2, 3, 4, 5]
    print(short(xs))
    print(wrong_element(xs))
    print(wrong_length(xs))
"#,
        &[
            "4 index in range failed",
            "9 index in range failed",
            "14 index in range failed",
            "18 index in range failed",
            "22 ensures failed",
            "28 ensures failed",
        ],
    );
}

#[test]
fn a_stepped_slice_stays_unmodeled() {
    assert_open(
        r#"
def evens(xs: list[int64]) -> int64:
    ensures result >= 0
    ensures result == 3
    return len(xs[::2])

def main():
    print(evens([1, 2, 3, 4, 5]))
"#,
        &["4 ensures unsupported"],
    );
}

#[test]
fn a_slice_agrees_with_its_model_at_run_time() {
    assert_runs(
        r#"
def check(xs: list[int64], a: int64, b: int64):
    n = len(xs)
    mut low = a
    if a < 0:
        low = a + n
    mut high = b
    if b < 0:
        high = b + n
    if 0 <= low and low <= high and high <= n:
        ys = xs[a:b]
        assert len(ys) == high - low
        for i in range(len(ys)):
            assert ys[i] == xs[low + i]
    if 0 <= low and low <= n:
        rest = xs[a:]
        assert len(rest) == n - low
        for i in range(len(rest)):
            assert rest[i] == xs[low + i]
    if 0 <= high and high <= n:
        first = xs[:b]
        assert len(first) == high
        for i in range(len(first)):
            assert first[i] == xs[i]

def sweep(xs: list[int64]):
    for a in range(-7, 8):
        for b in range(-7, 8):
            check(xs, a, b)

def main():
    sweep([])
    sweep([4])
    sweep([4, 5, 6])
    sweep([9, 8, 7, 6, 5, 4])
"#,
    );
    // Bounds out of order or outside the list stop the program: they are
    // not clamped.
    for slice in ["xs[2:1]", "xs[1:4]", "xs[-4:]", "xs[:-4]", "xs[4:]"] {
        let source = format!("def main():\n    xs = [1, 2, 3]\n    print(len({slice}))\n");
        assert!(crate::run_source(&source).is_err(), "{slice}");
    }
}

#[test]
fn a_sorted_list_is_ordered_and_holds_the_same_elements() {
    assert_proved(
        r#"
def smallest(xs: mut list[int64]) -> int64:
    requires len(xs) > 0
    ensures forall(x in xs, result <= x)
    ensures len(xs) == len(old(xs))
    ensures forall(x in old(xs), result <= x)
    ensures result in old(xs)
    xs.sort()
    return xs[0]

def largest(xs: list[int64]) -> int64:
    requires len(xs) > 0
    ensures forall(x in xs, result >= x)
    ensures exists(x in xs, x == result)
    ys = sorted(xs, reverse=true)
    return ys[0]

def ordered(xs: list[int64]) -> list[int64]:
    ensures len(result) == len(xs)
    ensures forall(i in range(len(result)), forall(j in range(i, len(result)), result[i] <= result[j]))
    ensures forall(x in xs, x in result)
    ensures forall(x in result, x in xs)
    return sorted(xs)

def either(xs: mut list[int64], down: bool):
    requires len(xs) > 1
    ensures down or xs[0] <= xs[1]
    ensures not down or xs[0] >= xs[1]
    xs.sort(reverse=down)

def middle(xs: list[int64]) -> int64:
    requires len(xs) == 3
    ensures exists(x in xs, x == result)
    ys = sorted(xs)
    assert ys[0] <= ys[1] and ys[1] <= ys[2]
    return ys[1]

def caller() -> int64:
    ensures result <= 4
    mut xs = [4, 9, 6]
    low = smallest(xs)
    return low

def main():
    mut xs = [3, 1, 2]
    print(smallest(xs))
    print(largest([3, 1, 2]))
    print(len(ordered([3, 1, 2])))
    either(xs, true)
    print(middle([3, 1, 2]))
    print(caller())
"#,
    );
}

#[test]
fn a_wrong_claim_about_a_sorted_list_still_fails() {
    assert_open(
        r#"
def descending(xs: mut list[int64]) -> bool:
    requires len(xs) > 1
    ensures result
    xs.sort()
    return xs[0] > xs[1]

def same_place(xs: mut list[int64]) -> bool:
    requires len(xs) > 1
    ensures result
    first = xs[0]
    xs.sort()
    return xs[0] == first

def shrinks(xs: list[int64]) -> int64:
    ensures result == len(xs) - 1
    return len(sorted(xs))

def new_element(xs: list[int64]) -> bool:
    requires len(xs) > 0
    ensures result
    ys = sorted(xs)
    return ys[0] == 7

def past_the_end(xs: list[int64]) -> int64:
    ys = sorted(xs)
    return ys[len(xs)]

def main():
    mut xs = [3, 1, 2]
    print(descending(xs))
    mut ys = [3, 1, 2]
    print(same_place(ys))
    print(shrinks([3, 1]))
    print(new_element([3, 1]))
"#,
        &[
            "4 ensures failed",
            "10 ensures failed",
            "16 ensures failed",
            "21 ensures failed",
            "27 index in range failed",
        ],
    );
}

#[test]
fn a_sort_the_encoding_cannot_order_keeps_the_elements_and_no_order() {
    // The elements are still those of the list, in an order the encoding
    // does not state.
    assert_proved(
        r#"
def by_key(xs: list[int64]) -> list[int64]:
    ensures len(result) == len(xs)
    ensures forall(x in result, x in xs)
    return sorted(xs, key=lambda x: x // 2)

def names(xs: mut list[str]):
    requires len(xs) > 0
    ensures len(xs) == len(old(xs))
    ensures xs[0] in old(xs)
    xs.sort()

def main():
    print(len(by_key([3, 1, 2])))
    mut xs = ["b", "a"]
    names(xs)
"#,
    );
    assert_open(
        r#"
def by_key(xs: list[int64]) -> bool:
    requires len(xs) > 1
    ensures result
    ys = sorted(xs, key=lambda x: x // 2)
    return ys[0] >= ys[1]

def main():
    print(by_key([3, 1, 2]))
"#,
        &["4 ensures unsupported"],
    );
}

#[test]
fn a_reversed_list_holds_the_elements_from_the_end() {
    assert_proved(
        r#"
def backwards(xs: mut list[int64]):
    ensures len(xs) == len(old(xs))
    ensures forall(i in range(len(xs)), xs[i] == old(xs)[len(xs) - 1 - i])
    xs.reverse()

def last(xs: list[int64]) -> int64:
    requires len(xs) > 0
    ensures result == xs[len(xs) - 1]
    mut ys = xs[:]
    ys.reverse()
    return ys[0]

def twice(xs: list[int64]) -> int64:
    requires len(xs) > 2
    ensures result == xs[2]
    mut ys = xs[:]
    ys.reverse()
    ys.reverse()
    return ys[2]

def main():
    mut xs = [3, 1, 2]
    backwards(xs)
    print(last(xs))
    print(twice(xs))
"#,
    );
    assert_open(
        r#"
def first(xs: list[int64]) -> int64:
    requires len(xs) > 1
    ensures result == xs[0]
    mut ys = xs[:]
    ys.reverse()
    return ys[0]

def past_the_end(xs: list[int64]) -> int64:
    mut ys = xs[:]
    ys.reverse()
    return ys[len(xs)]

def main():
    print(first([3, 1, 2]))
"#,
        &["4 ensures failed", "12 index in range failed"],
    );
}

#[test]
fn sort_and_reverse_agree_with_their_models_at_run_time() {
    assert_runs(
        r#"
def check(xs: list[int64]):
    n = len(xs)
    up = sorted(xs)
    down = sorted(xs, reverse=true)
    assert len(up) == n and len(down) == n
    for i in range(n):
        assert xs[i] in up and up[i] in xs
        assert xs[i] in down and down[i] in xs
        for j in range(i, n):
            assert up[i] <= up[j]
            assert down[i] >= down[j]
    mut back = xs[:]
    back.reverse()
    assert len(back) == n
    for i in range(n):
        assert back[i] == xs[n - 1 - i]
    keyed = sorted(xs, key=lambda x: x // 2)
    assert len(keyed) == n
    for i in range(n):
        assert xs[i] in keyed and keyed[i] in xs

def main():
    check([])
    check([5])
    check([2, 1])
    check([3, 3, 1, 2, 3])
    check([-4, 9, 0, -4, 7, 7, 1])
    mut names = ["b", "c", "a"]
    names.sort()
    assert len(names) == 3 and "a" in names and "b" in names and "c" in names
"#,
    );
}
