//! Allocation measurements for the Batch 1 representation phase (checkpoint
//! Q9 A, Q15 A, Q16 A). Every program runs on both backends with
//! `AURA_RUNTIME_STATS=1`; the direct backend's union counts pin the ratified
//! inline layout, and the interpreter's counts are reference numbers.

use std::fs;
use std::path::PathBuf;
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

fn aura_bin() -> &'static str {
    env!("CARGO_BIN_EXE_aura")
}

struct TempDir {
    path: PathBuf,
}

impl TempDir {
    fn new(prefix: &str) -> Self {
        let unique = format!(
            "{}-{}-{}",
            prefix,
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .expect("system time should be after unix epoch")
                .as_nanos()
        );
        let path = std::env::temp_dir().join(unique);
        fs::create_dir_all(&path).expect("failed to create temp dir");
        Self { path }
    }

    fn write(&self, relative: &str, source: &str) -> PathBuf {
        let path = self.path.join(relative);
        fs::write(&path, source).expect("failed to write source");
        path
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.path);
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct Stats {
    union_payload_boxes: u64,
    closure_environments: u64,
    opaque_boxes: u64,
    callable_overflow_allocations: u64,
    cloned_elements: u64,
}

fn parse_stats(stderr: &str, backend: &str) -> Stats {
    let prefix = format!("aura runtime stats ({backend}): ");
    let line = stderr
        .lines()
        .find(|line| line.starts_with(&prefix))
        .unwrap_or_else(|| panic!("no {backend} stats line in stderr:\n{stderr}"));
    let mut stats = Stats::default();
    for field in line[prefix.len()..].split_whitespace() {
        let (name, value) = field.split_once('=').expect("stats field");
        let value: u64 = value.parse().expect("stats value");
        match name {
            "union_payload_boxes" => stats.union_payload_boxes = value,
            "closure_environments" => stats.closure_environments = value,
            "opaque_boxes" => stats.opaque_boxes = value,
            "callable_overflow_allocations" => stats.callable_overflow_allocations = value,
            "cloned_elements" => stats.cloned_elements = value,
            other => panic!("unexpected stats field `{other}`"),
        }
    }
    stats
}

fn run_with_stats(prefix: &str, source: &str, backend: &str) -> (String, Stats) {
    let temp = TempDir::new(prefix);
    let path = temp.write("main.au", source);
    let output = Command::new(aura_bin())
        .arg("run")
        .arg("--backend")
        .arg(backend)
        .arg(&path)
        .env("AURA_RUNTIME_STATS", "1")
        .output()
        .expect("failed to run aura");
    let stderr = String::from_utf8_lossy(&output.stderr).to_string();
    assert!(
        output.status.success(),
        "{backend} run should succeed, stderr was:\n{stderr}"
    );
    (
        String::from_utf8_lossy(&output.stdout).to_string(),
        parse_stats(&stderr, backend),
    )
}

fn assert_both_backends(prefix: &str, source: &str, expected_stdout: &str) -> (Stats, Stats) {
    let (mir_stdout, mir) = run_with_stats(prefix, source, "mir");
    let (direct_stdout, direct) = run_with_stats(prefix, source, "direct");
    assert_eq!(mir_stdout, expected_stdout, "interpreter stdout");
    assert_eq!(direct_stdout, expected_stdout, "direct stdout");
    (mir, direct)
}

const SCALAR_UNION_LOCAL: &str = "def main():\n    value: int64 | None = 41\n    match value:\n        case int64 as number:\n            print(number + 1)\n        case None:\n            print(\"none\")\n";

const CLASS_UNION_LOCAL: &str = "class Point:\n    x: int64\n    y: int64\n\ndef main():\n    slot: Point | None = Point(x=3, y=4)\n    match slot:\n        case Point as point:\n            print(point.x + point.y)\n        case None:\n            print(\"none\")\n";

const UNION_CLASS_FIELD: &str = "class Slot:\n    value: int64 | None\n\ndef main():\n    slot = Slot(value=None)\n    match slot.value:\n        case int64 as number:\n            print(number)\n        case None:\n            print(\"empty\")\n    filled = Slot(value=7)\n    match filled.value:\n        case int64 as number:\n            print(number)\n        case None:\n            print(\"empty\")\n";

#[test]
fn stats_report_appears_only_when_requested_on_both_backends() {
    let temp = TempDir::new("repr-stats-shape");
    let path = temp.write("main.au", SCALAR_UNION_LOCAL);
    for backend in ["mir", "direct"] {
        let silent = Command::new(aura_bin())
            .args(["run", "--backend", backend])
            .arg(&path)
            .env_remove("AURA_RUNTIME_STATS")
            .output()
            .expect("failed to run aura");
        assert!(silent.status.success());
        assert!(
            !String::from_utf8_lossy(&silent.stderr).contains("aura runtime stats"),
            "{backend} must not report stats without the request"
        );
        let (_, stats) = run_with_stats("repr-stats-shape", SCALAR_UNION_LOCAL, backend);
        assert_eq!(stats.callable_overflow_allocations, 0);
        assert_eq!(
            stats.closure_environments, 0,
            "{backend} packs no closure here"
        );
    }
}

#[test]
fn direct_scalar_union_local_allocates_no_union_box() {
    let (mir, direct) = assert_both_backends("repr-scalar-union", SCALAR_UNION_LOCAL, "42\n");
    assert!(
        mir.union_payload_boxes >= 1,
        "the interpreter boxes its union payload"
    );
    assert_eq!(
        direct.union_payload_boxes, 0,
        "an `int64 | None` local stays inline on the direct backend (Q9 A)"
    );
}

#[test]
fn direct_class_union_local_allocates_no_union_box() {
    let (_, direct) = assert_both_backends("repr-class-union", CLASS_UNION_LOCAL, "7\n");
    assert_eq!(
        direct.union_payload_boxes, 0,
        "a `Point | None` local stays inline on the direct backend (Q9 A)"
    );
}

#[test]
fn direct_union_class_field_allocates_no_union_box() {
    let (_, direct) = assert_both_backends("repr-union-field", UNION_CLASS_FIELD, "empty\n7\n");
    assert_eq!(
        direct.union_payload_boxes, 0,
        "a union class field is flattened into the plain class layout (Q9 A)"
    );
}

const UNION_EQUALITY: &str = "def main():\n    value: int64 | None = 5\n    print(value == None)\n    print(value == 5)\n    empty: int64 | None = None\n    print(empty == None)\n";

const UNION_ELEMENT_READ: &str = "def main():\n    items: list[int64 | None] = [7, None]\n    first: int64 | None = items[0]\n    match first:\n        case int64 as number:\n            print(number)\n        case None:\n            print(\"none\")\n";

#[test]
fn direct_union_equality_compares_inline_without_a_union_box() {
    let (_, direct) =
        assert_both_backends("repr-union-equality", UNION_EQUALITY, "false\ntrue\ntrue\n");
    assert_eq!(
        direct.union_payload_boxes, 0,
        "`value == None` and `value == 5` compare tags and members inline (Q9 A)"
    );
}

#[test]
fn direct_union_element_read_reenters_inline_from_the_list() {
    let (_, direct) = assert_both_backends("repr-union-element", UNION_ELEMENT_READ, "7\n");
    assert_eq!(
        direct.union_payload_boxes, 3,
        "the list boxes its two elements and hands out one copy for the read; the inline local adds none"
    );
}

const STRING_UNION_LOCAL: &str = "def main():\n    mut label: str | None = \"ada\"\n    match label:\n        case str as text:\n            print(text)\n        case None:\n            print(\"none\")\n    label = None\n    print(label == None)\n";

const LIST_UNION_LOCAL: &str = "def main():\n    items: list[int64] | None = [1, 2, 3]\n    match items:\n        case list[int64] as values:\n            print(values.len())\n        case None:\n            print(\"none\")\n";

const STRING_CLASS_UNION_TRAIT: &str = "trait Greet:\n    def greet(mut self) -> str\n\nclass Dog:\n    name: str\n    barks: int64\n\nclass Cat:\n    name: str\n\nimpl Greet for Dog:\n    def greet(mut self) -> str:\n        self.barks = self.barks + 1\n        return self.name + \" woof\"\n\nimpl Greet for Cat:\n    def greet(mut self) -> str:\n        return self.name + \" meow\"\n\ndef main():\n    mut pet: Dog | Cat = Dog(name=\"rex\", barks=0)\n    print(pet.greet())\n    match pet:\n        case Dog as dog:\n            print(dog.barks)\n        case Cat as cat:\n            print(cat.name)\n";

#[test]
fn direct_string_union_local_allocates_no_union_box() {
    let (_, direct) = assert_both_backends("repr-string-union", STRING_UNION_LOCAL, "ada\ntrue\n");
    assert_eq!(
        direct.union_payload_boxes, 0,
        "a `str | None` local holds its string as an owned handle word (Q9 A)"
    );
}

#[test]
fn direct_list_union_local_allocates_no_union_box() {
    let (_, direct) = assert_both_backends("repr-list-union", LIST_UNION_LOCAL, "3\n");
    assert_eq!(
        direct.union_payload_boxes, 0,
        "a `list[int64] | None` local holds its list as an owned handle word (Q9 A)"
    );
}

#[test]
fn direct_string_class_union_dispatches_without_a_union_box() {
    let (_, direct) = assert_both_backends(
        "repr-string-class-union",
        STRING_CLASS_UNION_TRAIT,
        "rex woof\n1\n",
    );
    assert_eq!(
        direct.union_payload_boxes, 0,
        "a `Dog | Cat` union of classes holding strings dispatches on its tag inline (Q9 A)"
    );
}

const CALLABLE_LOCAL: &str = "type Adder = Callable[def(value: int64) -> int64]

def make(offset: own int64) -> Adder:
    return Adder(lambda value: value + offset)

def main():
    mut total = 0
    mut index = 0
    while index < 100000:
        add = make(index)
        total = total + add(1)
        index = index + 1
    print(total)
";

const CALLABLE_OVERFLOW: &str = "type Adder = Callable[def(value: int64) -> int64]

def make(a: own int64, b: own int64, c: own int64, d: own int64) -> Adder:
    return Adder(lambda value: value + a + b + c + d)

def main():
    mut total = 0
    mut index = 0
    while index < 1000:
        add = make(index, 1, 2, 3)
        total = total + add(1)
        index = index + 1
    print(total)
";

const CALLABLE_BOUNDARY: &str = "type Step = Callable[def(int64) -> int64]

def main():
    base: int64 = 10
    steps: list[Step] = [Step(lambda v: v + base)]
    for step in steps:
        print(step(3))
    worker: def(int64) -> int64 = lambda value: value + base
    with TaskGroup() as group:
        task = group.start(worker, 41)
        print(task.result_or(0, timeout=1s))
";

#[test]
fn direct_callable_local_allocates_no_environment() {
    let (_, direct) = assert_both_backends("repr-callable-local", CALLABLE_LOCAL, "5000050000\n");
    assert_eq!(
        direct.closure_environments, 0,
        "a one-capture callable packs its environment into its own words (Q15 A)"
    );
    assert_eq!(direct.callable_overflow_allocations, 0);
}

#[test]
fn direct_callable_overflow_allocates_one_block_per_packing() {
    let (_, direct) = assert_both_backends("repr-callable-overflow", CALLABLE_OVERFLOW, "506500\n");
    assert_eq!(
        direct.closure_environments, 0,
        "a four-capture callable still boxes no environment (Q16 A)"
    );
    assert_eq!(
        direct.callable_overflow_allocations, 1000,
        "each four-capture packing takes one checked environment block (Q16 A)"
    );
}

#[test]
fn direct_callable_boundaries_box_once_each() {
    let (_, direct) = assert_both_backends("repr-callable-boundary", CALLABLE_BOUNDARY, "13\n51\n");
    assert_eq!(
        direct.closure_environments, 2,
        "a list store and a task start each box the callable once at the boundary"
    );
    assert_eq!(direct.callable_overflow_allocations, 0);
}

/// A class whose collection fields are read, looped over, and appended to
/// from methods and free functions. Each operation should copy only the
/// elements it hands out, never the whole field.
const COLLECTION_FIELD_ACCESS: &str = r#"class Store:
    docs: list[dict[str, int64]] = []
    by_id: dict[str, int64] = {}

    def add(mut self, key: str, value: int64):
        add_doc(self.docs, self.by_id, key, value)

    def loop_total(self) -> int64:
        mut total = 0
        for doc in self.docs:
            total += doc.len()
        return total

    def point_total(self) -> int64:
        mut total = 0
        mut i = 0
        while i < self.docs.len():
            match self.docs.get(i):
                case Lookup.Found(doc):
                    total += doc.len()
                case Lookup.Missing:
                    pass
            if f"k{i}" in self.by_id:
                total += 1
            i += 1
        return total

    def position(self, key: str) -> int64:
        return find_position(self.by_id, key)

def add_doc(
    docs: mut list[dict[str, int64]],
    by_id: mut dict[str, int64],
    key: str,
    value: int64,
):
    mut doc: dict[str, int64] = {}
    doc["a"] = value
    doc["b"] = value
    by_id[key.clone()] = docs.len()
    docs.append(doc)

def find_position(by_id: dict[str, int64], key: str) -> int64:
    match by_id.get(key):
        case Lookup.Found(position):
            return position
        case Lookup.Missing:
            return -1

def main():
    mut store = Store()
    mut i = 0
    while i < 200:
        store.add(f"k{i}", i)
        i += 1
    print(store.loop_total())
    print(store.point_total())
    mut found = 0
    i = 0
    while i < 200:
        found += store.position(f"k{i}")
        i += 1
    print(found)
"#;

#[test]
fn direct_collection_field_access_does_not_clone_the_whole_field() {
    let (_, direct) = assert_both_backends(
        "repr-field-access",
        COLLECTION_FIELD_ACCESS,
        "400\n600\n19900\n",
    );
    // The same work on local variables copies 2,400 elements: each
    // appended document once, and each document handed out by the loop and
    // by `get`. Cloning the whole `docs` or `by_id` field per access copied
    // more than 580,000.
    assert!(
        direct.cloned_elements <= 2_400,
        "collection field access cloned {} elements on the direct backend",
        direct.cloned_elements
    );
}

/// Instances stored in a dictionary field and reached through `lookup`,
/// with shared and mutable methods called on the entry.
const LOOKUP_ENTRY_METHODS: &str = r#"class Bucket:
    items: list[int64] = []

    def size(self) -> int64:
        return self.items.len()

    def add(mut self, value: int64):
        self.items.append(value)

class Registry:
    buckets: dict[str, Bucket] = {}

    def add(mut self, name: str, value: int64):
        if name not in self.buckets:
            self.buckets[name.clone()] = Bucket()
        match mut self.buckets.lookup(name):
            case Lookup.Found(bucket):
                bucket.add(value)
            case Lookup.Missing:
                pass

    def size(self, name: str) -> int64:
        match self.buckets.lookup(name):
            case Lookup.Found(bucket):
                return bucket.size()
            case Lookup.Missing:
                return 0

def main():
    mut registry = Registry()
    mut i = 0
    while i < 200:
        registry.add("a", i)
        i += 1
    mut total = 0
    i = 0
    while i < 200:
        total += registry.size("a")
        i += 1
    print(total)
"#;

#[test]
fn direct_lookup_entry_methods_do_not_clone_the_entry() {
    let (_, direct) = assert_both_backends("repr-lookup-entry", LOOKUP_ENTRY_METHODS, "40000\n");
    // Each call, and the presence probe that begins each `lookup`, used to
    // copy the whole `Bucket`, up to 200 items, and the `in` test copied
    // the whole dictionary: more than 260,000 elements in all.
    assert!(
        direct.cloned_elements <= 1_000,
        "lookup entry methods cloned {} elements on the direct backend",
        direct.cloned_elements
    );
}

const NARROWED_UNION_FIELDS: &str = r#"class Bag:
    items: list[int64]
    tags: dict[str, int64]

class Shelf:
    bag: Bag | None

def main():
    mut shelf = Shelf(bag=Bag(items=[1], tags={"a": 1}))
    if shelf.bag is not None:
        shelf.bag.items.append(2)
        print(shelf.bag.items.len())
        print(len(shelf.bag.tags))
    mut bag: Bag | None = Bag(items=[3], tags={})
    if bag is not None:
        bag.items.append(4)
        print(bag.items)
"#;

#[test]
fn direct_collection_methods_on_narrowed_union_fields_run() {
    // A narrowed `Bag | None` stores its fields behind a union payload, so
    // the direct backend must not move them out of the slot by field name.
    assert_both_backends(
        "repr-narrowed-union-fields",
        NARROWED_UNION_FIELDS,
        "2\n1\n[3, 4]\n",
    );
}

const ALIASED_FIELD_OWNERS: &str = r#"class Bag:
    items: list[int64]

    def overlap(self, other: Bag) -> int64:
        return common(self.items, other.items)

    def size_with(self, other: list[int64]) -> int64:
        return self.items.len() * 10 + other.len()

class Reg:
    order: list[Bag] = []
    by: dict[str, Bag] = {}

def common(xs: list[int64], ys: list[int64]) -> int64:
    return xs.len() * 10 + ys.len()

def pair(x: Bag, y: Bag) -> int64:
    return common(x.items, y.items)

def by_index(x: Reg, y: Reg) -> int64:
    return x.order[0].size_with(y.order[0].items)

def by_entry(x: Reg, y: Reg) -> int64:
    match x.by.lookup("a"):
        case Lookup.Found(bag):
            return bag.size_with(y.by["a"].items)
        case Lookup.Missing:
            return -1

def main():
    b = Bag(items=[1, 2, 3])
    print(b.overlap(b))
    print(pair(b, b))
    mut r = Reg()
    r.order.append(Bag(items=[1, 2]))
    r.by["a"] = Bag(items=[1, 2, 3, 4])
    print(by_index(r, r))
    print(by_entry(r, r))
    print(b.items)
"#;

#[test]
fn direct_lending_does_not_empty_a_field_another_argument_reaches() {
    // Two borrowed parameters can name one instance, so `x.items` and
    // `y.items` may be the same field even though the places differ.
    assert_both_backends(
        "repr-aliased-field-owners",
        ALIASED_FIELD_OWNERS,
        "33\n33\n22\n44\n[1, 2, 3]\n",
    );
}

const NESTED_INDEX_WRITES: &str = r#"def main():
    mut d: dict[str, list[int64]] = {"a": [1]}
    d["a"][0] = 5
    mut nested: dict[str, dict[str, int64]] = {"a": {}}
    nested["a"]["k"] = 3
    mut grid = [[1], [2]]
    i = 1
    grid[i][0] += 40
    print(d)
    print(nested)
    print(grid)
"#;

#[test]
fn direct_nested_index_assignment_writes_the_collection() {
    // The direct backend used to write a temporary copy of `d["a"]`, so the
    // assignment was silently lost.
    assert_both_backends(
        "repr-nested-index-writes",
        NESTED_INDEX_WRITES,
        "{a: [5]}\n{a: {k: 3}}\n[[1], [42]]\n",
    );
}

const STRING_KEY_LOAN_BRANCHES: &str = r#"def first(d: dict[str, list[int64]], flag: bool) -> int64:
    match d.lookup("a"):
        case Lookup.Found(items):
            if flag:
                return items[0]
            print(items)
        case _:
            pass
    return -1

def main():
    mut d: dict[str, list[int64]] = {"a": [1]}
    for n in [1, 2, 3]:
        match d.lookup("a"):
            case Lookup.Found(items):
                if n > 1:
                    print(items.len())
                else:
                    print(0)
            case _:
                pass
    view sh = d["a"]
    if len(d) > 0:
        print(sh)
    else:
        print(0)
    print(first(d, true))
    print(first(d, false))
"#;

#[test]
fn direct_string_key_loan_ending_in_each_branch_releases_its_key() {
    // A loan that ends in both arms of an `if` used to release its string
    // key only on the arm compiled first, so the task leaked the key.
    assert_both_backends(
        "repr-string-key-loan-branches",
        STRING_KEY_LOAN_BRANCHES,
        "0\n1\n1\n[1]\n1\n[1]\n-1\n",
    );
}
