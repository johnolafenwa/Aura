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

def counts(cs: list[Counter]) -> int64:
    mut total = 0
    for c in cs:
        total += c.count
    return total

def scalars(xs: list[int64], ys: mut list[float64], n: int64) -> int64:
    mut total: int64 = 0
    mut i = 0
    while i < n:
        total += xs[i * 2] + len(ys)
        ys[i * 2] = 1.5
        i += 1
    return total

def carried(xs: mut list[int64], n: int64) -> int64:
    mut total = 0
    for i in range(n):
        total += xs[i]
    for i in range(n):
        xs[i] += total
    return total

def appended(xs: mut list[int64], n: int64) -> int64:
    mut total = 0
    for i in range(n):
        total += xs[i]
    xs.append(total)
    for i in range(n):
        total += xs[i]
    return total

def stored(xs: mut list[int64], ys: mut list[int64], n: int64) -> int64:
    mut total = 0
    for i in range(n):
        total += xs[i]
        ys[i] = total
    xs[0] = total
    ys[0] = total
    for i in range(n):
        total += xs[i] + ys[i]
    return total

def main():
    mut ps = [Point(x=1.0, y=2.0)]
    mut ys = [0.5, 0.25, 0.125]
    mut zs = [1, 2, 3]
    mut ws = [4, 5, 6]
    print(carried(zs, 3))
    print(appended(zs, 3))
    print(stored(zs, ws, 3))
    print(scalars([1, 2, 3], ys, 2))
    print(reads(ps))
    print(loans(3))
    print(grows(3))
    print(iterates(ps))
    print(counts([Counter(name="c", count=1)]))
"#;

fn function(name: &str) -> MirFunction {
    lower_source_to_mir(SOURCE)
        .expect("the source should lower")
        .functions
        .into_iter()
        .find(|function| function.name == name)
        .expect("the function should exist")
}

fn regions(name: &str) -> Vec<ListSnapshotRegion> {
    let function = function(name);
    list_snapshot_regions(&function, |_| true)
}

#[test]
fn a_loop_that_measures_and_reads_a_list_reads_its_view_once() {
    let found = regions("reads");
    assert_eq!(found.len(), 1, "{found:?}");
    assert_eq!(found[0].list, "ps");
    assert!(!found[0].writes);
    assert_eq!(found[0].entries.len(), 1, "{found:?}");
    let (from, to) = &found[0].entries[0];
    assert!(!found[0].blocks.contains(from) && found[0].blocks.contains(to));
}

#[test]
fn a_loop_that_reads_boxes_by_index_only_reads_the_list() {
    let found = regions("counts");
    assert_eq!(found.len(), 1, "{found:?}");
    assert_eq!(found[0].list, "cs");
    assert!(!found[0].writes);
}

#[test]
fn a_loop_that_loans_elements_may_write_the_list() {
    let found = regions("loans");
    assert_eq!(found.len(), 1, "{found:?}");
    assert_eq!(found[0].list, "counters");
    assert!(found[0].writes);
}

#[test]
fn only_the_inner_loop_of_a_list_that_grows_reads_its_view_once() {
    let found = regions("grows");
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
    let found = regions("iterates");
    assert_eq!(found.len(), 2, "{found:?}");
    assert!(found
        .iter()
        .all(|region| region.list == "ps" && !region.writes));
    // A list the caller does not let native code read this way has none.
    let function = function("reads");
    assert_eq!(list_snapshot_regions(&function, |_| false), Vec::new());
}

#[test]
fn a_loop_of_scalar_accesses_names_the_kind_its_copy_runs_for() {
    use crate::runtime_ops::ScalarElement;
    let scalars = function("scalars");
    let found = regions("scalars");
    let kind = |list: &str| {
        let region = found
            .iter()
            .find(|region| region.list == list)
            .unwrap_or_else(|| panic!("`{list}` should have a loop"));
        super::region_scalar_element(&scalars, region)
    };
    assert_eq!(kind("xs"), Some(ScalarElement::Int64));
    assert_eq!(kind("ys"), Some(ScalarElement::Float64));
    // A loop that reads plain records names no scalar kind.
    let reads = regions("reads");
    assert_eq!(
        super::region_scalar_element(&function("reads"), &reads[0]),
        None
    );
}

#[test]
fn a_loop_after_another_on_the_list_takes_the_words_it_read() {
    let found = regions("carried");
    assert_eq!(found.len(), 2, "{found:?}");
    let [first, second] = [0, 1].map(|later| {
        let earlier = &found[1 - later];
        found[later].carry.as_ref() == Some(&earlier.blocks)
    });
    // Only the loop entered second takes the other's words: the first is
    // entered before the other reads them.
    assert!(first != second, "{found:?}");
    // A write the earlier loop did not check room for may share the
    // storage, and an append moves it.
    let carries = |name: &str, list: &str| {
        regions(name)
            .iter()
            .filter(|region| region.list == list)
            .any(|region| region.carry.is_some())
    };
    assert!(!carries("appended", "xs"));
    assert!(!carries("stored", "xs"));
    assert!(carries("stored", "ys"));
}

#[test]
fn loops_that_read_views_once_compile_with_their_copies() {
    let module = lower_source_to_mir(SOURCE).expect("the source should lower");
    crate::native_backend::without_optimization(|| super::super::emit_host_object(&module))
        .expect("the loops should compile");
}

#[test]
fn loops_inside_and_across_each_other_compile_with_their_copies() {
    // Copies of loops inside other loops' copies, of loops that share all
    // but one block, and of a loop over a list of scalars and a list of
    // records, first for the scalars alone.
    let source =
        include_str!("../tests/fixtures/run-pass/nested_and_overlapping_loops_read_views_once.au");
    let module = lower_source_to_mir(source).expect("the source should lower");
    crate::native_backend::without_optimization(|| super::super::emit_host_object(&module))
        .expect("the loops should compile");
    super::super::emit_host_object(&module).expect("the loops should compile optimized");
}

#[test]
fn loops_after_other_loops_compile_taking_their_words() {
    // Loops that take the words of loops before them, over lists of
    // scalars and an Array, which reads its layout itself.
    let source =
        include_str!("../tests/fixtures/run-pass/sibling_loops_take_the_view_words_once.au");
    let module = lower_source_to_mir(source).expect("the source should lower");
    crate::native_backend::without_optimization(|| super::super::emit_host_object(&module))
        .expect("the loops should compile");
    super::super::emit_host_object(&module).expect("the loops should compile optimized");
    let twice = array_function("twice");
    let found = list_snapshot_regions(&twice, |_| true);
    assert!(
        found.iter().any(|region| region.carry.is_some()),
        "{found:?}"
    );
    let module = lower_source_to_mir(ARRAY_SOURCE).expect("the source should lower");
    super::super::emit_host_object(&module).expect("the Array loops should compile");
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

def twice(values: Array[int64], n: int64) -> int64:
    mut sum = 0
    for i in range(n):
        sum += values[i]
    for i in range(n):
        sum += values[i] * 2
    return sum

def main():
    mut grid = Array[float64].zeros([4, 4])
    smooth(grid.clone(), grid, 4)
    print(twice(Array[int64].zeros([2]), 2))
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
    let found = list_snapshot_regions(&smooth, |_| true);
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
    let found = list_snapshot_regions(&total, |_| true);
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
    let found = list_snapshot_regions(&mixed, |_| true);
    for region in &found {
        assert_eq!(
            super::array_region_rank(&mixed, region, |_| true),
            None,
            "{region:?}"
        );
    }
    // The element count may be read in the loop.
    let measured = array_function("measured");
    let found = list_snapshot_regions(&measured, |_| true);
    let [region] = found.as_slice() else {
        panic!("one loop reads `values`: {found:?}");
    };
    assert_eq!(
        super::array_region_rank(&measured, region, |_| true),
        Some(1)
    );
    let summed = array_function("summed");
    assert_eq!(list_snapshot_regions(&summed, |_| true), Vec::new());
}

#[test]
fn array_loops_compile_with_their_copies() {
    let module = lower_source_to_mir(ARRAY_SOURCE).expect("the source should lower");
    crate::native_backend::without_optimization(|| super::super::emit_host_object(&module))
        .expect("the loops should compile");
    super::super::emit_host_object(&module).expect("the loops should compile optimized");
}

const FIELD_SOURCE: &str = r#"
class Ball:
    weight: float64
    size: int64

class Shelf:
    weights: list[int64]
    balls: list[Ball]
    seen: int64
    best: int64
    open: bool

    def heavy(self, weight: float64) -> bool:
        return weight > 1.0 and self.best >= 0

    def total(mut self) -> int64:
        mut sum = 0
        for i in range(self.weights.len()):
            sum += self.weights[i]
            self.seen += 1
        return sum

    def count_heavy(mut self) -> int64:
        mut n = 0
        for i in range(self.balls.len()):
            if self.heavy(self.balls[i].weight):
                n += self.balls[i].size
        return n

    def grows(mut self, n: int64) -> int64:
        mut sum = 0
        for i in range(n):
            self.weights.append(i + 100)
            sum += self.weights[i]
        return sum

    def add(mut self, w: int64):
        self.weights.append(w)

    def calls(mut self, n: int64) -> int64:
        mut sum = 0
        for i in range(n):
            self.add(i)
            sum += self.weights[i]
        return sum

    def swaps(mut self, n: int64) -> int64:
        mut sum = 0
        for i in range(n):
            sum += self.weights[0]
            self.weights = [i + 1, i + 2]
        return sum

    def resizes(mut self, n: int64) -> int64:
        mut sum = 0
        for i in range(n):
            sum += self.balls[i].size
        self.balls[0].size += sum
        return sum

    def untouched(mut self, n: int64) -> int64:
        return n + 1

    def flagged(mut self) -> int64:
        if self.open:
            return 1
        return 0

    def peeks(self, n: int64) -> int64:
        mut sum = 0
        for i in range(n):
            sum += self.weights[i]
        return sum

def owner(n: int64) -> int64:
    mut s = Shelf(weights=[1, 2], balls=[Ball(weight=1.5, size=2)], seen=0, best=0, open=true)
    mut sum = 0
    for round in range(n):
        for i in range(s.weights.len()):
            sum += s.weights[i] * round
        s.seen += 1
    return sum + s.total() + s.count_heavy() + s.grows(1) + s.calls(1) + s.swaps(1) + s.resizes(1) + s.peeks(1) + s.untouched(1) + s.flagged()

def main():
    print(owner(2))
"#;

/// The loops of `name` in `FIELD_SOURCE` that read a field's list once,
/// when `keeps` says whether a callee keeps an instance and `exclusive`
/// whether the function holds an instance alone.
fn field_regions(name: &str, keeps: bool, exclusive: bool) -> Vec<ListSnapshotRegion> {
    all_field_regions(name, keeps, exclusive)
        .into_iter()
        .filter(|region| !region.method)
        .collect()
}

/// The whole body of method `name` in `FIELD_SOURCE` when it keeps its
/// receiver (see `field_regions`).
fn method_region(name: &str, keeps: bool, exclusive: bool) -> Option<ListSnapshotRegion> {
    all_field_regions(name, keeps, exclusive)
        .into_iter()
        .find(|region| region.method)
}

fn all_field_regions(name: &str, keeps: bool, exclusive: bool) -> Vec<ListSnapshotRegion> {
    let function = lower_source_to_mir(FIELD_SOURCE)
        .expect("the source should lower")
        .functions
        .into_iter()
        .find(|function| function.name == name)
        .expect("the function should exist");
    let lend_temps = super::super::classes::field_lend_temps(&function, |_| false);
    super::field_list_snapshot_regions(
        &function,
        |_| exclusive,
        &super::FieldListRules {
            lend_temps: &lend_temps,
            candidate: &|_, _| true,
            scalar_field: &|place| {
                place.ends_with(".seen") || place.ends_with(".best") || place.ends_with(".open")
            },
            keeps_instances: &|_| keeps,
            keeps_receiver: &|_| None,
        },
    )
}

#[test]
fn a_loop_that_only_reads_a_list_in_a_field_reads_its_view_once() {
    let found = field_regions("Shelf.total", false, true);
    assert_eq!(found.len(), 1, "{found:?}");
    assert_eq!(found[0].list, "self.weights");
    assert!(!found[0].writes);
    assert!(!found[0].temps.is_empty());
    assert_eq!(found[0].entries.len(), 1, "{found:?}");
    // An instance something else may reach keeps its checks.
    assert_eq!(field_regions("Shelf.total", false, false), Vec::new());
    // A local that holds the instance is read the same way: the outer
    // loop, which also writes a scalar field, holds the inner one.
    let found = field_regions("owner", false, true);
    assert_eq!(found.len(), 1, "{found:?}");
    assert_eq!(found[0].list, "s.weights");
    assert_eq!(found[0].temps.len(), 2, "{found:?}");
}

#[test]
fn a_loop_may_pass_the_instance_to_a_function_that_keeps_it() {
    let found = field_regions("Shelf.count_heavy", true, true);
    assert_eq!(found.len(), 1, "{found:?}");
    assert_eq!(found[0].list, "self.balls");
    assert_eq!(found[0].temps.len(), 2, "{found:?}");
    assert_eq!(field_regions("Shelf.count_heavy", false, true), Vec::new());
}

#[test]
fn a_loop_that_may_change_the_field_or_its_list_keeps_reading_them() {
    // An append to the list, and a store of another list in the field.
    for name in ["Shelf.grows", "Shelf.swaps"] {
        assert_eq!(field_regions(name, true, true), Vec::new(), "{name}");
    }
    // A call of a method that may change the instance.
    assert_eq!(field_regions("Shelf.calls", false, true), Vec::new());
    // A loan of an element of the list anywhere in the function.
    assert_eq!(field_regions("Shelf.resizes", true, true), Vec::new());
}

#[test]
fn a_method_that_keeps_its_receiver_in_its_layout_is_one_region() {
    // Scalar fields, lends of field boxes, and calls of functions that
    // only read scalar fields keep the receiver in its layout.
    let total = method_region("Shelf.total", false, true).expect("`total` keeps `self`");
    assert_eq!(total.list, "self");
    assert!(total.entries.is_empty() && total.temps.is_empty() && !total.writes);
    let function = lower_source_to_mir(FIELD_SOURCE)
        .expect("the source should lower")
        .functions
        .into_iter()
        .find(|function| function.name == "Shelf.total")
        .expect("the function should exist");
    assert_eq!(total.blocks.len(), function.blocks.len());
    assert!(method_region("Shelf.count_heavy", true, true).is_some());
    // A receiver something else may reach, a call that may change it, a
    // store of another list in a field, and a function with no receiver.
    assert_eq!(method_region("Shelf.total", false, false), None);
    assert_eq!(method_region("Shelf.calls", false, true), None);
    assert_eq!(method_region("Shelf.swaps", true, true), None);
    assert_eq!(method_region("owner", true, true), None);
    // A method that never names its receiver has nothing to keep.
    assert_eq!(method_region("Shelf.untouched", true, true), None);
    // A branch on a scalar field keeps it too.
    assert!(method_region("Shelf.flagged", true, true).is_some());
}

const KEEPER_SOURCE: &str =
    include_str!("../tests/fixtures/run-pass/loops_that_call_methods_keeping_their_receiver.au");

/// The methods of `KEEPER_SOURCE` that keep their receiver's layout, with
/// the fields whose lists each may store into, and the regions of
/// `name` given them.
fn keeper_regions(
    name: &str,
) -> (
    std::collections::HashMap<String, std::collections::HashSet<String>>,
    Vec<ListSnapshotRegion>,
) {
    let functions = lower_source_to_mir(KEEPER_SOURCE)
        .expect("the source should lower")
        .functions;
    let scalar = |place: &str| place == "self.total";
    // `label` holds a `str`, which is not a list.
    let list = |place: &str| ["self.counts", "self.marks"].contains(&place);
    let keepers = super::layout_keepers(
        &functions,
        |function| function.name.starts_with("Tally."),
        |_, place| scalar(place),
        |_, place| list(place),
        &|_| false,
    );
    let function = functions
        .iter()
        .find(|function| function.name == name)
        .expect("the function should exist");
    let lend_temps = super::super::classes::field_lend_temps(function, |_| false);
    let regions = super::field_list_snapshot_regions(
        function,
        |_| true,
        &super::FieldListRules {
            lend_temps: &lend_temps,
            candidate: &|_, _| true,
            scalar_field: &scalar,
            keeps_instances: &|_| false,
            keeps_receiver: &|callee| keepers.get(callee),
        },
    )
    .into_iter()
    .filter(|region| !region.method)
    .collect();
    (keepers, regions)
}

#[test]
fn a_loop_may_call_methods_that_keep_their_receiver() {
    let (keepers, scan) = keeper_regions("Tally.scan");
    // A store into a list, a scalar field, and a call of another keeper.
    let stores = |name: &str| {
        keepers
            .get(name)
            .map(|fields| fields.iter().cloned().collect::<Vec<_>>())
    };
    assert_eq!(stores("Tally.bump"), Some(vec!["counts".to_string()]));
    assert_eq!(stores("Tally.note"), Some(vec!["counts".to_string()]));
    // An append, another list in a field, `self` handed to a function
    // that writes it back, and a call of a method that appends.
    // A field that holds something other than a list keeps its own view.
    for name in [
        "Tally.grow",
        "Tally.swap",
        "Tally.hand",
        "Tally.deep",
        "Tally.labelled",
    ] {
        assert!(!keepers.contains_key(name), "{name}: {keepers:?}");
    }
    // The loop reads `marks` once, which its calls store nothing into.
    let [region] = scan.as_slice() else {
        panic!("one loop reads `self.marks`: {scan:?}");
    };
    assert_eq!(region.list, "self.marks");
    assert!(!region.writes);
    // A loop whose calls store into its list checks it holds its
    // storage alone, as one that stores into it itself does.
    for name in ["Tally.scan_counts", "Tally.doubles"] {
        let (_, found) = keeper_regions(name);
        assert!(found.len() == 1 && found[0].writes, "{name}: {found:?}");
    }
    // A loop that calls one of the others reads the view again after the
    // call: no copy holds the block that makes it.
    let functions = lower_source_to_mir(KEEPER_SOURCE)
        .expect("the source should lower")
        .functions;
    for (name, callee) in [
        ("Tally.scan_grow", "grow"),
        ("Tally.scan_swap", "swap"),
        ("Tally.scan_hand", "hand"),
        ("Tally.scan_deep", "deep"),
    ] {
        let function = functions
            .iter()
            .find(|function| function.name == name)
            .expect("the function should exist");
        let calling = function
            .blocks
            .iter()
            .find(|block| {
                format!("{:?}", block.instructions).contains(&format!("field: \"{callee}\""))
            })
            .expect("a block calls the method");
        let (_, found) = keeper_regions(name);
        assert!(
            found
                .iter()
                .all(|region| !region.blocks.contains(&calling.label)),
            "{name}: {found:?}"
        );
    }
}

#[test]
fn loops_that_call_methods_keeping_their_receiver_compile_with_their_copies() {
    let module = lower_source_to_mir(KEEPER_SOURCE).expect("the source should lower");
    crate::native_backend::without_optimization(|| super::super::emit_host_object(&module))
        .expect("the loops should compile");
    super::super::emit_host_object(&module).expect("the loops should compile optimized");
}

#[test]
fn methods_that_keep_their_receiver_compile_with_their_copies() {
    // A method's copy, one through a field and a list element, and a
    // class that keeps its fields in a map, which has no copy.
    let source = include_str!("../tests/fixtures/run-pass/methods_that_keep_their_receiver.au");
    let module = lower_source_to_mir(source).expect("the source should lower");
    crate::native_backend::without_optimization(|| super::super::emit_host_object(&module))
        .expect("the methods should compile");
    super::super::emit_host_object(&module).expect("the methods should compile optimized");
}

#[test]
fn loops_over_field_lists_compile_with_their_copies() {
    let module = lower_source_to_mir(FIELD_SOURCE).expect("the source should lower");
    crate::native_backend::without_optimization(|| super::super::emit_host_object(&module))
        .expect("the loops should compile");
    super::super::emit_host_object(&module).expect("the loops should compile optimized");
}
