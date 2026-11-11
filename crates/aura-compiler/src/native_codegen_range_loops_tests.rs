use crate::lower_source_to_mir;
use crate::mir::MirFunction;
use crate::runtime_ops::ScalarElement;
use crate::sema::Type;

use super::super::list_snapshots::list_snapshot_regions;
use super::{range_proof, RangeBound, RangeGuard, RangeProof};

const SOURCE: &str = r#"
def saxpy(a: float64, xs: list[float64], ys: mut list[float64]):
    for i in range(len(xs)):
        y = ys[i]
        ys[i] = a * xs[i] + y

def scale(xs: mut list[float64], factor: float64):
    for i in range(len(xs)):
        xs[i] *= factor

def stencil(xs: list[float64], out: mut list[float64], n: int64):
    for i in range(1, n - 1):
        out[i] = xs[i - 1] + xs[i + 1]

def counted(xs: list[int64], n: int64) -> int64:
    mut total: int64 = 0
    mut i: int64 = 0
    while i < n:
        total += xs[i]
        i += 2
    return total

def after_advance(xs: list[int64], n: int64) -> int64:
    mut total: int64 = 0
    mut i: int64 = 0
    while i < n:
        i += 1
        total += xs[i]
    return total

def literal_bound(xs: list[int64]) -> int64:
    mut total: int64 = 0
    for i in range(8):
        total += xs[i]
    return total

def nested(xs: list[float64], rounds: int64) -> float64:
    mut total = 0.0
    for round in range(rounds):
        for j in range(len(xs)):
            total += xs[j]
    return total

def backwards(xs: list[int64], n: int64) -> int64:
    mut total: int64 = 0
    mut i: int64 = n - 1
    while 0 < i:
        total += xs[i]
        i -= 1
    return total

def moving_bound(xs: list[int64], n: int64) -> int64:
    mut total: int64 = 0
    mut limit: int64 = n
    mut i: int64 = 0
    while i < limit:
        total += xs[i]
        i += 1
        limit -= 1
    return total

def unrelated(xs: list[int64], n: int64, at: int64) -> int64:
    mut total: int64 = 0
    for i in range(n):
        total += xs[at]
    return total

def reset_from_local(xs: list[int64], n: int64, back: int64) -> int64:
    mut total: int64 = 0
    mut i: int64 = 0
    while i < n:
        total += xs[i]
        i += 1
        if total > 100:
            i = back
    return total

def main():
    mut ys = [1.0, 2.0, 3.0]
    xs = [0.5, 0.25, 0.125]
    saxpy(2.0, xs, ys)
    scale(ys, 3.0)
    mut out = [0.0, 0.0, 0.0]
    stencil(xs, out, 3)
    ints: list[int64] = [1, 2, 3, 4, 5, 6, 7, 8, 9]
    print(ys)
    print(out)
    print(counted(ints, 9))
    print(after_advance(ints, 8))
    print(literal_bound(ints))
    print(nested(xs, 2))
    print(backwards(ints, 9))
    print(moving_bound(ints, 9))
    print(unrelated(ints, 3, 2))
    print(reset_from_local(ints, 9, 9))
"#;

fn function(name: &str) -> MirFunction {
    lower_source_to_mir(SOURCE)
        .expect("the source should lower")
        .functions
        .into_iter()
        .find(|function| function.name == name)
        .expect("the function should exist")
}

/// The proof for each list of `name` that has a loop, by list.
fn proofs(name: &str) -> Vec<(String, Option<RangeProof>)> {
    let function = function(name);
    let is_index = |place: &str| {
        let int64 = Type::named("int64");
        function
            .local_types
            .iter()
            .any(|local| local.name == place && local.ty == int64)
            || function
                .params
                .iter()
                .any(|param| param.name == place && param.ty == int64)
    };
    let mut found = list_snapshot_regions(&function, |_| true, |_| true)
        .into_iter()
        .map(|region| {
            let proof = range_proof(&function, &region, is_index);
            (region.list, proof)
        })
        .collect::<Vec<_>>();
    found.sort_by(|left, right| left.0.cmp(&right.0));
    found
}

fn proof(name: &str, list: &str) -> RangeProof {
    proofs(name)
        .into_iter()
        .find(|(found, _)| found == list)
        .and_then(|(_, proof)| proof)
        .unwrap_or_else(|| panic!("`{list}` in `{name}` should have a range proof"))
}

fn at_least(guards: &[RangeGuard]) -> Vec<i64> {
    let mut found = guards
        .iter()
        .filter_map(|guard| match guard {
            RangeGuard::AtLeast { least, .. } => Some(*least),
            RangeGuard::Within { .. } => None,
        })
        .collect::<Vec<_>>();
    found.sort_unstable();
    found
}

fn within(guards: &[RangeGuard]) -> Vec<i64> {
    let mut found = guards
        .iter()
        .filter_map(|guard| match guard {
            RangeGuard::Within { distance, .. } => Some(*distance),
            RangeGuard::AtLeast { .. } => None,
        })
        .collect::<Vec<_>>();
    found.sort_unstable();
    found
}

#[test]
fn a_range_loop_covers_reads_and_writes_at_its_counter() {
    let xs = proof("saxpy", "xs");
    assert_eq!(xs.element, ScalarElement::Float64);
    assert_eq!(xs.covered.len(), 1, "{xs:?}");
    assert_eq!(
        (at_least(&xs.guards), within(&xs.guards)),
        (vec![0], vec![0])
    );
    // The read and the write of `ys[i]`.
    let ys = proof("saxpy", "ys");
    assert_eq!(ys.covered.len(), 2, "{ys:?}");
    assert_eq!(ys.guards, xs.guards);

    // A compound assignment reads and writes the element.
    let scaled = proof("scale", "xs");
    assert_eq!(scaled.covered.len(), 2, "{scaled:?}");
}

#[test]
fn an_access_beside_the_counter_guards_its_distance() {
    let xs = proof("stencil", "xs");
    assert_eq!(xs.covered.len(), 2, "{xs:?}");
    // `xs[i - 1]` needs the counter to start at 1, and `xs[i + 1]` needs
    // the bound to stop one short of the length.
    assert_eq!(at_least(&xs.guards), vec![-1, 1]);
    assert_eq!(within(&xs.guards), vec![-1, 1]);
    let out = proof("stencil", "out");
    assert_eq!(
        (at_least(&out.guards), within(&out.guards)),
        (vec![0], vec![0])
    );
}

#[test]
fn a_while_loop_covers_accesses_before_and_after_its_advance() {
    let xs = proof("counted", "xs");
    assert_eq!(xs.covered.len(), 1);
    assert_eq!(
        (at_least(&xs.guards), within(&xs.guards)),
        (vec![0], vec![0])
    );
    assert!(xs.guards.iter().any(|guard| matches!(
        guard,
        RangeGuard::Within { bound: RangeBound::Place(place), .. } if place == "n"
    )));

    // After `i += 1` the index is one past the counter the branch proved.
    let later = proof("after_advance", "xs");
    assert_eq!(
        (at_least(&later.guards), within(&later.guards)),
        (vec![-1], vec![1])
    );
}

#[test]
fn a_constant_bound_is_guarded_against_the_length() {
    let xs = proof("literal_bound", "xs");
    assert_eq!(xs.covered.len(), 1);
    assert!(xs.guards.iter().any(|guard| matches!(
        guard,
        RangeGuard::Within {
            bound: RangeBound::Int(8) | RangeBound::Place(_),
            distance: 0
        }
    )));
}

#[test]
fn an_inner_loop_over_the_lists_own_length_is_covered_in_the_outer_loop() {
    let xs = proof("nested", "xs");
    assert_eq!(xs.covered.len(), 1, "{xs:?}");
    // The inner counter starts at a constant and its bound is a length
    // of the list, so only what they hold at the entry is guarded.
    assert!(within(&xs.guards).iter().all(|distance| *distance == 0));
    assert_eq!(at_least(&xs.guards), vec![0]);
}

#[test]
fn loops_without_a_steady_counter_and_bound_have_no_proof() {
    for name in ["backwards", "moving_bound", "unrelated", "reset_from_local"] {
        let found = proofs(name);
        assert!(!found.is_empty(), "`{name}` should have a loop over a list");
        assert!(
            found.iter().all(|(_, proof)| proof.is_none()),
            "`{name}` should have no range proof: {found:?}"
        );
    }
}

#[test]
fn loops_with_range_proofs_compile_with_and_without_optimization() {
    let module = lower_source_to_mir(SOURCE).expect("the source should lower");
    crate::native_backend::without_optimization(|| super::super::emit_host_object(&module))
        .expect("the loops should compile unoptimized");
    super::super::emit_host_object(&module).expect("the loops should compile");
}

#[test]
fn sums_of_a_place_in_range_and_a_constant_are_exact() {
    // `i += 2`.
    let counted = proof("counted", "xs");
    assert_eq!(counted.exact.len(), 1, "{counted:?}");
    // `i += 1`, whose result is still within reach of the counter.
    let later = proof("after_advance", "xs");
    assert_eq!(later.exact.len(), 1, "{later:?}");
    // `i - 1` and `i + 1`, beside whatever advances the counter.
    let stencil = proof("stencil", "xs");
    assert!(stencil.exact.len() >= 2, "{stencil:?}");
    for (label, position) in &stencil.exact {
        let function = function("stencil");
        let block = function
            .blocks
            .iter()
            .find(|block| block.label == *label)
            .expect("an exact sum is in a block of the function");
        assert!(matches!(
            &block.instructions[*position],
            crate::mir::Instruction::Assign {
                value: crate::mir::Rvalue::Binary {
                    trap: crate::mir::ArithmeticTrap::Overflow,
                    ..
                },
                ..
            }
        ));
    }
}
