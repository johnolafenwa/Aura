use super::*;

fn verify(source: &str) -> Option<Report> {
    if smt::SolverCommand::find(None).is_err() {
        assert!(
            std::env::var_os("CI").is_none(),
            "CI must install an SMT solver for the verifier tests"
        );
        eprintln!("skipping: no SMT solver on PATH");
        return None;
    }
    let program = crate::check_source(source).expect("the program checks");
    let module = crate::mir::lower_for_verification(&program);
    Some(verify_module(&program, &module, &Options::default()).expect("the verifier runs"))
}

/// Each obligation that was not proved, as `line kind in function`.
fn open_checks(report: &Report) -> Vec<String> {
    report
        .obligations
        .iter()
        .filter(|obligation| obligation.status != Status::Proved)
        .map(|obligation| {
            format!(
                "{} {} in {}",
                obligation.span.line,
                obligation.kind.name(),
                obligation.function
            )
        })
        .collect()
}

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

/// Whether each check that was not proved failed (`true`) or was not
/// modeled (`false`), in order.
fn open_failed(report: &Report) -> Vec<bool> {
    report
        .obligations
        .iter()
        .filter(|obligation| obligation.status != Status::Proved)
        .map(|obligation| matches!(obligation.status, Status::Failed { .. }))
        .collect()
}

// ----- control.recover ---------------------------------------------------

const HALF: &str = r#"
import control

def half(x: int64) -> int64:
    requires x >= 0
    ensures result * 2 <= x
    ensures result >= 0
    return x // 2
"#;

#[test]
fn a_recovered_result_has_the_callee_ensures() {
    assert_proved(&format!(
        r#"{HALF}
def use(x: int64) -> int64:
    requires x >= 0
    ensures result >= -1
    ensures result <= x
    match control.recover(half, x):
        case Ok(v):
            assert v * 2 <= x
            return v
        case Err(trap):
            return -1

def main():
    print(use(9))
"#
    ));
}

#[test]
fn a_recovered_result_is_no_more_than_the_ensures() {
    let Some(report) = verify(&format!(
        r#"{HALF}
def wrong(x: int64) -> int64:
    requires x >= 0
    match control.recover(half, x):
        case Ok(v):
            assert v * 2 == x
            return v
        case Err(trap):
            return -1

def main():
    print(wrong(4))
"#
    )) else {
        return;
    };
    assert_eq!(open_checks(&report), ["14 assert in wrong"]);
    assert_eq!(open_failed(&report), [true]);
}

#[test]
fn a_recovered_call_may_trap() {
    // Even a call proved free of traps may run out of stack or memory.
    assert_open(
        &format!(
            r#"{HALF}
def wrong(x: int64) -> int64:
    requires x >= 0
    r = control.recover(half, x)
    assert r.is_ok()
    return 0

def main():
    print(wrong(4))
"#
        ),
        &["13 assert in wrong"],
    );
}

#[test]
fn a_recovered_call_still_proves_its_requires() {
    assert_open(
        &format!(
            r#"{HALF}
def wrong(x: int64) -> int64:
    match control.recover(half, x):
        case Ok(v):
            return v
        case Err(trap):
            return -1

def main():
    print(wrong(4))
"#
        ),
        &["11 requires in wrong"],
    );
}

#[test]
fn an_ensures_of_a_trapping_callee_is_not_assumed_on_the_err_path() {
    // `boom` traps where the encoding does not look, so its `ensures`
    // is never proved false; the caller must not rely on it after a trap.
    let Some(report) = verify(
        r#"
import control

def boom(x: int64) -> int64:
    ensures result > 0
    ensures result < 0
    items = [1, 2]
    return items[x]

def wrong(x: int64) -> int64:
    match control.recover(boom, x):
        case Ok(v):
            return v
        case Err(trap):
            assert false
            return 0

def main():
    print(wrong(4))
"#,
    ) else {
        return;
    };
    assert!(open_checks(&report).contains(&"15 assert in wrong".to_string()));
}

#[test]
fn recovering_keeps_the_callers_values() {
    assert_proved(&format!(
        r#"{HALF}
def keep(x: int64) -> int64:
    requires x >= 0 and x <= 100
    ensures result == x + 1
    mut items = [x]
    n = x + 0
    match control.recover(half, n):
        case Ok(v):
            pass
        case Err(trap):
            pass
    items.append(1)
    assert len(items) == 2
    return n + 1

def main():
    print(keep(3))
"#
    ));
}

#[test]
fn recover_runs_as_its_model_says() {
    // On a spread of inputs, an `Ok` result meets the callee's `ensures`
    // and an `Err` comes from a trap.
    crate::run_source(
        r#"
import control

def pick(x: int64) -> int64:
    ensures result >= 0
    assert x % 3 != 0
    return x * x

def main():
    mut oks = 0
    mut errs = 0
    for x in range(-20, 20):
        match control.recover(pick, x):
            case Ok(v):
                assert v >= 0
                assert x % 3 != 0
                oks += 1
            case Err(trap):
                assert x % 3 == 0
                errs += 1
    assert oks == 27
    assert errs == 13
"#,
    )
    .expect("the model holds at run time");
}

// ----- State tasks share -------------------------------------------------

#[test]
fn a_shared_value_is_any_value_each_time_it_is_read() {
    // Another task may change each of these between two reads, so every
    // check fails, and none is reported as a step the verifier skips.
    let Some(report) = verify(
        r#"
import sync

def bump(counter: sync.AtomicInt) -> None:
    _ = counter.add(1)

def stored(c: sync.AtomicInt) -> None:
    c.store(5)
    assert c.load() == 5

def twice(c: sync.AtomicInt) -> None:
    x = c.load()
    assert x == c.load()

def added(c: sync.AtomicInt) -> None:
    v = c.add(1)
    assert c.load() == v

def flag(b: sync.AtomicBool) -> None:
    b.store(true)
    assert b.load()

def locked(m: sync.Mutex[int64]) -> None:
    with v = m.lock():
        v = 5
    with w = m.lock():
        assert w == 5

def set_then_lock(m: sync.Mutex[int64]) -> None:
    m.set(3)
    with w = m.lock():
        assert w == 3

def received(q: Queue[int64]) -> None:
    _ = q.put(3)
    match q.get():
        case QueueReceive.Item(x):
            assert x == 3
        case _:
            pass

def polled(q: Queue[int64]) -> None:
    match q.poll():
        case Poll.Ready(x):
            assert x == 3
        case _:
            pass

def defaulted(q: Queue[int64]) -> None:
    assert q.get_or(5) == 5

def looped(q: Queue[int64]) -> None:
    for x in q:
        assert x == 3

def not_cancelled() -> None:
    assert not cancelled()

def main():
    c = sync.AtomicInt(0)
    with TaskGroup() as group:
        group.start(bump, c)
    stored(c)
"#,
    ) else {
        return;
    };
    assert_eq!(
        open_checks(&report),
        [
            "9 assert in stored",
            "13 assert in twice",
            "17 assert in added",
            "21 assert in flag",
            "27 assert in locked",
            "32 assert in set_then_lock",
            "38 assert in received",
            "45 assert in polled",
            "50 assert in defaulted",
            "54 assert in looped",
            "57 assert in not_cancelled",
        ]
    );
    assert!(
        open_failed(&report).iter().all(|failed| *failed),
        "{report:#?}"
    );
}

#[test]
fn a_value_inside_a_held_lock_is_stable() {
    // No other task can change the guarded value while the block holds
    // the lock, even across a call that may let another task run.
    assert_proved(
        r#"
import control
import sync

def g(n: int64) -> int64:
    return n

def inside(m: sync.Mutex[int64]) -> int64:
    mut a = 0
    with v = m.lock():
        v = 3
        a = v + 0
        _ = control.recover(g, 2)
        yield_now()
        assert v == 3
        assert a == v
    return a

def grown(m: sync.Mutex[list[int64]]) -> None:
    with items = m.lock():
        items.append(1)
        assert len(items) >= 1

def main():
    m = sync.Mutex[int64](0)
    print(inside(m))
    grown(sync.Mutex[list[int64]](list[int64]()))
"#,
    );
}

#[test]
fn another_task_changes_what_a_lock_block_sees() {
    // At run time the second block sees what the task wrote meanwhile,
    // which is why the verifier assumes nothing about it.
    crate::run_source(
        r#"
import sync

def write(m: sync.Mutex[int64], q: Queue[int64], done: Queue[int64]) -> None:
    match q.get():
        case QueueReceive.Item(x):
            m.set(x)
        case _:
            pass
    _ = done.put(1)

def main():
    m = sync.Mutex[int64](0)
    q = Queue[int64](1)
    done = Queue[int64](1)
    with TaskGroup() as group:
        group.start(write, m, q, done)
        with v = m.lock():
            v = 5
        _ = q.put(7)
        _ = done.get()
        with w = m.lock():
            assert w == 7
"#,
    )
    .expect("the other task's write is seen");
}

// ----- Tensor shapes -----------------------------------------------------

const MATMUL: &str = r#"
from tensor import Device, Tensor

def mm(a: Tensor[float32], b: Tensor[float32]) -> Tensor[float32]:
    requires len(a.shape()) == 2 and len(b.shape()) == 2
    requires a.shape()[1] == b.shape()[0]
    ensures len(result.shape()) == 2
    ensures result.shape()[0] == a.shape()[0]
    ensures result.shape()[1] == b.shape()[1]
    return a.matmul(b)
"#;

#[test]
fn a_matrix_product_proves_from_its_shapes() {
    assert_proved(&format!(
        r#"{MATMUL}
def chain(a: Tensor[float32], b: Tensor[float32], c: Tensor[float32]) -> int64:
    requires len(a.shape()) == 2 and len(b.shape()) == 2 and len(c.shape()) == 2
    requires a.shape()[1] == b.shape()[0] and b.shape()[1] == c.shape()[0]
    ensures result == a.shape()[0]
    return mm(mm(a, b), c).shape()[0]

def main():
    d = Device.cpu()
    a = Tensor[float32].zeros([2, 3], d)
    b = Tensor[float32].zeros([3, 4], d)
    c = Tensor[float32].zeros([4, 5], d)
    print(chain(a, b, c))
"#
    ));
}

#[test]
fn a_matrix_product_of_wrong_shapes_fails() {
    // Without the rank, `a.shape()[1]` is not the inner dimension of a
    // batch of matrices.
    assert_open(
        r#"
from tensor import Device, Tensor

def no_rank(a: Tensor[float32], b: Tensor[float32]) -> Tensor[float32]:
    requires a.shape()[1] == b.shape()[0]
    return a.matmul(b)

def wrong_axis(a: Tensor[float32], b: Tensor[float32]) -> Tensor[float32]:
    requires len(a.shape()) == 2 and len(b.shape()) == 2
    requires a.shape()[0] == b.shape()[0]
    return a.matmul(b)

def vectors(a: Tensor[float32], b: Tensor[float32]) -> Tensor[float32]:
    requires len(a.shape()) == 1 and len(b.shape()) == 1
    requires a.shape()[0] == b.shape()[0]
    return a.matmul(b)

def literal() -> None:
    d = Device.cpu()
    print(Tensor[float32].zeros([2, 3], d).matmul(Tensor[float32].zeros([2, 3], d)).shape())

def main():
    literal()
"#,
        &[
            "6 requires in no_rank",
            "11 requires in wrong_axis",
            "16 requires in vectors",
            "20 requires in literal",
        ],
    );
}

#[test]
fn a_wrong_shape_after_a_matrix_product_fails() {
    assert_open(
        &format!(
            r#"{MATMUL}
def wrong(a: Tensor[float32], b: Tensor[float32]) -> int64:
    requires len(a.shape()) == 2 and len(b.shape()) == 2
    requires a.shape()[1] == b.shape()[0]
    ensures result == a.shape()[1]
    return mm(a, b).shape()[1]

def main():
    d = Device.cpu()
    print(wrong(Tensor[float32].zeros([2, 2], d), Tensor[float32].zeros([2, 2], d)))
"#
        ),
        &["15 ensures in wrong"],
    );
}

#[test]
fn tensor_shapes_follow_each_member() {
    assert_proved(
        r#"
import tensor
from tensor import Device, Tensor

def shapes(values: Array[float32]) -> None:
    requires len(values.shape()) == 2
    d = Device.cpu()
    a = Tensor[float32].zeros([2, 3], d)
    b = Tensor[float32].full([3, 4], 1.0, d)
    c = a.matmul(b)
    assert len(c.shape()) == 2 and c.shape()[0] == 2 and c.shape()[1] == 4
    assert c.len() == 8
    r = c.reshape([4, -1])
    assert r.shape()[1] == 2
    assert r.flatten().shape()[0] == 8
    t = tensor.randn([3, 2, 5], 1, d).transpose()
    assert t.shape()[0] == 5 and t.shape()[2] == 3
    s = c.sum(1)
    assert len(s.shape()) == 1 and s.shape()[0] == 2
    k = c.mean(-2, true)
    assert k.shape()[0] == 1 and k.shape()[1] == 4
    w = (c + Tensor[float32].zeros([4], d)).relu() * 2.0
    assert w.shape()[0] == 2 and w.shape()[1] == 4
    m = 1.0 - c
    assert m.shape()[1] == 4
    v = c.sum(0, false).max(0)
    assert v.shape()[0] == 1
    all = c.sum(None, true)
    assert all.shape()[0] == 1 and all.shape()[1] == 1
    batched = Tensor[float32].zeros([5, 2, 3], d).matmul(b)
    assert batched.shape()[0] == 5 and batched.shape()[2] == 4
    vector = c.matmul(Tensor[float32].zeros([4], d))
    assert len(vector.shape()) == 1 and vector.shape()[0] == 2
    rows = Tensor[float32].zeros([3], d).matmul(b)
    assert len(rows.shape()) == 1 and rows.shape()[0] == 4
    x = Tensor[float32].from_array(values, d)
    assert len(x.shape()) == 2
    print(c.sum().item())

def main():
    shapes(Array[float32].zeros([2, 2]))
"#,
    );
}

#[test]
fn a_wrong_tensor_shape_fails_at_its_member() {
    assert_open(
        r#"
import tensor
from tensor import Device, Tensor

def wrong(n: int64) -> None:
    d = Device.cpu()
    a = Tensor[float32].zeros([2, 3], d)
    r = a.reshape([4, -1])
    q = a.reshape([-1, -1])
    p = a.reshape(list[int64]())
    s = a.sum(2)
    e = a + Tensor[float32].zeros([2], d)
    z = Tensor[float32].zeros([n, 2], d)
    print(a.item())

def main():
    wrong(1)
"#,
        &[
            "8 requires in wrong",
            "9 requires in wrong",
            "10 requires in wrong",
            "11 requires in wrong",
            "12 requires in wrong",
            "13 requires in wrong",
            "14 requires in wrong",
        ],
    );
}

#[test]
fn a_tensor_member_the_verifier_does_not_model_is_reported() {
    let Some(report) = verify(
        r#"
from tensor import Device, Tensor

def other(a: Tensor[float32]) -> None:
    requires len(a.shape()) == 2
    p = a.permute([1, 0])
    assert len(p.shape()) == 2

def main():
    other(Tensor[float32].zeros([2, 3], Device.cpu()))
"#,
    ) else {
        return;
    };
    assert_eq!(
        open_checks(&report),
        ["6 requires in other", "7 assert in other"]
    );
    assert_eq!(open_failed(&report), [false, false]);
}

#[test]
fn tensor_shapes_run_as_their_model_says() {
    // Over a spread of shapes, each member traps exactly where its check
    // fails, and otherwise gives the shape the model gives.
    crate::run_source(
        r#"
import control
from tensor import Device, Tensor

def zeros(shape: list[int64]) -> Tensor[float32]:
    return Tensor[float32].zeros(shape, Device.cpu())

def product(a: list[int64], b: list[int64]) -> list[int64]:
    return zeros(a).matmul(zeros(b)).shape()

def added(a: list[int64], b: list[int64]) -> list[int64]:
    return (zeros(a) + zeros(b)).shape()

def reshaped(a: list[int64], b: list[int64]) -> list[int64]:
    return zeros(a).reshape(b).shape()

def summed(a: list[int64], axis: int64, keep: bool) -> list[int64]:
    return zeros(a).sum(axis, keep).shape()

def dim(shape: list[int64], back: int64) -> int64:
    if back < len(shape):
        return shape[len(shape) - 1 - back]
    return 1

def broadcast(a: list[int64], b: list[int64]) -> list[int64] | None:
    rank = max(len(a), len(b))
    mut out = list[int64]()
    for back in range(rank):
        x = dim(a, back)
        y = dim(b, back)
        if x != y and x != 1 and y != 1:
            return None
        if x == 1:
            out.insert(0, y)
        else:
            out.insert(0, x)
    return out

def batch(shape: list[int64]) -> list[int64]:
    if len(shape) < 2:
        return list[int64]()
    return shape[0:len(shape) - 2]

def matmul_shape(a: list[int64], b: list[int64]) -> list[int64] | None:
    if len(a) == 1 and len(b) == 1:
        return None
    inner_a = a[len(a) - 1]
    mut inner_b = b[0]
    if len(b) >= 2:
        inner_b = b[len(b) - 2]
    if inner_a != inner_b:
        return None
    match broadcast(batch(a), batch(b)):
        case None:
            return None
        case list[int64] as out:
            mut shape = out.copy()
            if len(a) >= 2:
                shape.append(a[len(a) - 2])
            if len(b) >= 2:
                shape.append(b[len(b) - 1])
            return shape

def reshape_shape(a: list[int64], b: list[int64]) -> list[int64] | None:
    mut count = 1
    for d in a:
        count *= d
    mut known = 1
    mut inferred = -1
    for at in range(len(b)):
        if b[at] == -1:
            if inferred >= 0:
                return None
            inferred = at
        elif b[at] < 0:
            return None
        else:
            known *= b[at]
    if len(b) == 0:
        return None
    mut out = b.copy()
    if inferred >= 0:
        if known == 0 or count % known != 0:
            return None
        out[inferred] = count // known
    elif known != count:
        return None
    return out

def sum_shape(a: list[int64], axis: int64, keep: bool) -> list[int64] | None:
    rank = len(a)
    if axis < -rank or axis >= rank:
        return None
    mut at = axis
    if at < 0:
        at += rank
    mut out = a.copy()
    if keep:
        out[at] = 1
        return out
    if rank == 1:
        return [1]
    _ = out.pop(at)
    return out

def same(got: Result[list[int64], control.Trap], want: list[int64] | None) -> bool:
    match got:
        case Ok(shape):
            match want:
                case None:
                    return false
                case list[int64] as expected:
                    return shape == expected
        case Err(trap):
            return want is None

def main():
    shapes = [[1], [3], [2, 3], [3, 2], [1, 3], [3, 1], [2, 1, 3], [4, 2, 3], [4, 3, 2], [2, 3, 3], [6], [0, 3]]
    mut checked = 0
    for a in shapes:
        for b in shapes:
            assert same(control.recover(product, a.copy(), b.copy()), matmul_shape(a, b))
            assert same(control.recover(added, a.copy(), b.copy()), broadcast(a, b))
            checked += 2
    targets = [[6], [-1], [2, -1], [-1, 3], [3, -1, 1], [-1, -1], [4, -1], [0, -1], [2, 3], [6, 1], [-2, -3]]
    for a in shapes:
        for b in targets:
            assert same(control.recover(reshaped, a.copy(), b.copy()), reshape_shape(a, b))
            checked += 1
        for axis in range(-4, 4):
            assert same(control.recover(summed, a.copy(), axis, false), sum_shape(a, axis, false))
            assert same(control.recover(summed, a.copy(), axis, true), sum_shape(a, axis, true))
            checked += 2
    assert checked > 500
"#,
    )
    .expect("the shape model holds at run time");
}
