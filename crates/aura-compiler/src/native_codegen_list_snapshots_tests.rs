use crate::lower_source_to_mir;
use crate::mir::MirFunction;

use super::{list_snapshot_regions, ListSnapshotRegion};

const SOURCE: &str = r#"
class Point:
    x: float64
    y: float64

class Counter:
    name: str
    count: int64

def reads(ps: list[Point]) -> float64:
    mut total = 0.0
    mut i = 0
    while i < ps.len():
        total += ps[i].x
        i += 1
    return total

def loans(n: int64) -> int64:
    mut counters = list[Counter]()
    for i in range(n):
        counters.append(Counter(name="c", count=i))
    mut i = 0
    while i < counters.len():
        counters[i].count += 1
        i += 1
    return counters[0].count

def grows(n: int64) -> float64:
    mut ps = list[Point]()
    mut total = 0.0
    mut rounds = 0
    while rounds < n:
        mut i = 0
        while i < ps.len():
            total += ps[i].x
            i += 1
        ps.append(Point(x=1.0, y=2.0))
        rounds += 1
    return total

def iterates(ps: list[Point]) -> float64:
    mut total = 0.0
    for p in ps:
        total += p.x
    mut i = 0
    while i < ps.len():
        total += ps[i].y
        i += 1
    return total

def main():
    mut ps = [Point(x=1.0, y=2.0)]
    print(reads(ps))
    print(loans(3))
    print(grows(3))
    print(iterates(ps))
"#;

fn function(name: &str) -> MirFunction {
    lower_source_to_mir(SOURCE)
        .expect("the source should lower")
        .functions
        .into_iter()
        .find(|function| function.name == name)
        .expect("the function should exist")
}

fn regions(name: &str, pure: bool) -> Vec<ListSnapshotRegion> {
    let function = function(name);
    list_snapshot_regions(&function, |_| true, |_| pure)
}

#[test]
fn a_loop_that_measures_and_reads_a_list_reads_its_view_once() {
    let found = regions("reads", true);
    assert_eq!(found.len(), 1, "{found:?}");
    assert_eq!(found[0].list, "ps");
    assert!(!found[0].writes);
    assert_eq!(found[0].entries.len(), 1, "{found:?}");
    let (from, to) = &found[0].entries[0];
    assert!(!found[0].blocks.contains(from) && found[0].blocks.contains(to));
    // Index reads that may write the list need room.
    assert!(regions("reads", false)[0].writes);
}

#[test]
fn a_loop_that_loans_elements_may_write_the_list() {
    let found = regions("loans", true);
    assert_eq!(found.len(), 1, "{found:?}");
    assert_eq!(found[0].list, "counters");
    assert!(found[0].writes);
}

#[test]
fn only_the_inner_loop_of_a_list_that_grows_reads_its_view_once() {
    let found = regions("grows", true);
    assert_eq!(found.len(), 1, "{found:?}");
    let function = function("grows");
    let blocks = function.blocks.len();
    // The outer loop, whose block appends, is not part of it.
    assert!(found[0].blocks.len() < blocks, "{found:?}");
    assert!(!found[0].writes);
}

#[test]
fn a_for_loop_over_a_list_reads_its_view_once_too() {
    // A `for` loop reads each element by index after its bound check.
    let found = regions("iterates", true);
    assert_eq!(found.len(), 2, "{found:?}");
    assert!(found
        .iter()
        .all(|region| region.list == "ps" && !region.writes));
    // A list the caller does not let native code read this way has none.
    let function = function("reads");
    assert_eq!(
        list_snapshot_regions(&function, |_| false, |_| true),
        Vec::new()
    );
}

#[test]
fn loops_that_read_views_once_compile_with_their_copies() {
    let module = lower_source_to_mir(SOURCE).expect("the source should lower");
    crate::native_backend::without_optimization(|| super::super::emit_host_object(&module))
        .expect("the loops should compile");
}

const ARRAY_SOURCE: &str = r#"
def smooth(source: Array[float64], target: mut Array[float64], n: int64):
    for i in range(1, n - 1):
        for j in range(1, n - 1):
            target[i, j] = (source[i - 1, j] + source[i + 1, j]) * 0.5

def total(values: Array[int32], n: int64) -> int64:
    mut sum = 0
    for i in range(n):
        sum += values[i] as int64
    return sum

def mixed(cube: Array[float32], n: int64) -> float64:
    mut sum = 0.0
    for i in range(n):
        sum += cube[i, 0, 0] as float64
        sum += cube[i, 0] as float64
    return sum

def measured(values: Array[int64], n: int64) -> int64:
    mut sum = 0
    for i in range(n):
        sum += values[i] + values.len()
    return sum

def summed(values: Array[int64], n: int64) -> int64:
    mut sum = 0
    for i in range(n):
        sum += values[i] + values.sum()
    return sum

def main():
    mut grid = Array[float64].zeros([4, 4])
    smooth(grid.clone(), grid, 4)
    print(grid)
    print(total(Array[int32].zeros([3]), 3))
    print(mixed(Array[float32].zeros([2, 2, 2]), 2))
    print(measured(Array[int64].zeros([2]), 2))
    print(summed(Array[int64].zeros([2]), 2))
"#;

fn array_function(name: &str) -> MirFunction {
    lower_source_to_mir(ARRAY_SOURCE)
        .expect("the source should lower")
        .functions
        .into_iter()
        .find(|function| function.name == name)
        .expect("the function should exist")
}

#[test]
fn a_loop_of_array_element_accesses_reads_the_layout_once() {
    let smooth = array_function("smooth");
    let found = list_snapshot_regions(&smooth, |_| true, |_| true);
    let source = found
        .iter()
        .find(|region| region.list == "source")
        .expect("the loop reads `source`");
    assert!(!source.writes);
    assert_eq!(super::array_region_rank(&smooth, source, |_| true), Some(2));
    let target = found
        .iter()
        .find(|region| region.list == "target")
        .expect("the loop writes `target`");
    assert!(target.writes);
    assert_eq!(super::array_region_rank(&smooth, target, |_| true), Some(2));

    let total = array_function("total");
    let found = list_snapshot_regions(&total, |_| true, |_| true);
    let [region] = found.as_slice() else {
        panic!("one loop reads `values`: {found:?}");
    };
    assert_eq!(super::array_region_rank(&total, region, |_| true), Some(1));
    // An index that is not one word names no rank.
    assert_eq!(super::array_region_rank(&total, region, |_| false), None);
}

#[test]
fn a_loop_with_two_ranks_or_another_member_keeps_reading_the_view() {
    let mixed = array_function("mixed");
    let found = list_snapshot_regions(&mixed, |_| true, |_| true);
    for region in &found {
        assert_eq!(
            super::array_region_rank(&mixed, region, |_| true),
            None,
            "{region:?}"
        );
    }
    // The element count may be read in the loop.
    let measured = array_function("measured");
    let found = list_snapshot_regions(&measured, |_| true, |_| true);
    let [region] = found.as_slice() else {
        panic!("one loop reads `values`: {found:?}");
    };
    assert_eq!(
        super::array_region_rank(&measured, region, |_| true),
        Some(1)
    );
    let summed = array_function("summed");
    assert_eq!(
        list_snapshot_regions(&summed, |_| true, |_| true),
        Vec::new()
    );
}

#[test]
fn array_loops_compile_with_their_copies() {
    let module = lower_source_to_mir(ARRAY_SOURCE).expect("the source should lower");
    crate::native_backend::without_optimization(|| super::super::emit_host_object(&module))
        .expect("the loops should compile");
    super::super::emit_host_object(&module).expect("the loops should compile optimized");
}
