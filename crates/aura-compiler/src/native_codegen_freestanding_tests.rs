use super::freestanding::{self, TRAP_HOOK, UNSUPPORTED};
use super::*;
use crate::diag::Diagnostic;
use object::{Object, ObjectSection, ObjectSymbol};

const TARGETS: [FreestandingTarget; 3] = FreestandingTarget::ALL;

/// The MIR of `source`, which may declare foreign functions, lowered with
/// the source positions the subset check reports.
fn lower(source: &str) -> MirModule {
    let module = crate::parse_source(source).expect("source should parse");
    let program = crate::check_module_with_builtin_imports(module).expect("source should check");
    crate::mir::lower_with_source_locations(&program)
}

fn build(
    source: &str,
    target: FreestandingTarget,
) -> std::result::Result<Vec<u8>, Vec<Diagnostic>> {
    emit_freestanding_object(&lower(source), "kernel.au", source, target)
}

fn object(source: &str, target: FreestandingTarget) -> Vec<u8> {
    build(source, target).unwrap_or_else(|problems| {
        panic!(
            "freestanding build failed: {:?}",
            problems
                .iter()
                .map(|problem| &problem.message)
                .collect::<Vec<_>>()
        )
    })
}

/// The messages of the problems a freestanding build of `source` reports.
fn problems(source: &str) -> Vec<String> {
    let found = check_freestanding(&lower(source));
    assert!(
        found.iter().all(|problem| problem.code == UNSUPPORTED),
        "{found:?}"
    );
    found.into_iter().map(|problem| problem.message).collect()
}

fn assert_problem(source: &str, expected: &str) {
    let found = problems(source);
    assert!(
        found.iter().any(|message| message.contains(expected)),
        "expected a problem with `{expected}`, found {found:?}"
    );
}

/// The object's symbols by kind: defined globals, weak definitions, and
/// undefined names, each sorted.
fn symbols(bytes: &[u8]) -> (Vec<String>, Vec<String>, Vec<String>) {
    let file = object::File::parse(bytes).expect("the object should parse");
    let (mut global, mut weak, mut undefined) = (Vec::new(), Vec::new(), Vec::new());
    for symbol in file.symbols() {
        let name = symbol.name().unwrap_or_default().to_string();
        if name.is_empty() {
            continue;
        }
        if symbol.is_undefined() {
            undefined.push(name);
        } else if symbol.is_weak() {
            weak.push(name);
        } else if symbol.is_global() {
            global.push(name);
        }
    }
    global.sort();
    weak.sort();
    undefined.sort();
    (global, weak, undefined)
}

const ARITHMETIC: &str = r#"
class Point:
    x: int32
    y: int32

    def manhattan(self) -> int32:
        return abs(self.x) + abs(self.y)

class Counter:
    count: int64

    def bump(mut self, by: int64) -> None:
        self.count = self.count + by

def clamp(value: int64, low: int64, high: int64) -> int64:
    requires low <= high
    return max(low, min(value, high))

def first_set(word: uint64) -> uint8 | None:
    if word == 0:
        return None
    mut index: uint8 = 0
    mut rest = word
    while rest & 1 == 0:
        rest = rest >> 1
        index = index + 1
    return index

def add_to(total: mut int64, value: int64) -> None:
    total = total + value

export "C" def mix(a: int64, b: int64, scale: float64) -> float64:
    point = Point(x=a as int32, y=b as int32)
    mut counter = Counter(count=0)
    mut total: int64 = 0
    for i in range(clamp(a, 0, 100)):
        add_to(total, i)
        counter.bump(i % 3)
    assert total >= 0, "the total went negative"
    bit = first_set(b as uint64)
    mut shift: int64 = 0
    if bit is not None:
        shift = bit as int64
    whole = (a // 3) + (a % 7) + (total << 2) + counter.count + shift
    return (point.manhattan() as float64) * scale + sqrt(2.0) + (whole as float64) / 3.0

export "C" def narrow(value: float64, wide: int64) -> int16:
    return (value as int16) + ((wide as float32) as int16)

export "C" def update(seed: uint32, out: mut uint32) -> bool:
    out = seed * 31 + 7
    return out > seed
"#;

#[test]
fn every_target_writes_an_object_that_needs_only_the_trap_hook() {
    let machines = [
        object::Architecture::X86_64,
        object::Architecture::Aarch64,
        object::Architecture::Riscv64,
    ];
    for (target, machine) in TARGETS.into_iter().zip(machines) {
        let bytes = object(ARITHMETIC, target);
        let file = object::File::parse(&*bytes).expect("the object should parse");
        assert_eq!(file.architecture(), machine, "{}", target.name());
        assert_eq!(file.kind(), object::ObjectKind::Relocatable);
        let (global, weak, undefined) = symbols(&bytes);
        assert_eq!(global, ["mix", "narrow", "update"], "{}", target.name());
        assert_eq!(weak, ["memcmp", "memcpy", "memmove", "memset"]);
        assert_eq!(undefined, [TRAP_HOOK], "{}", target.name());
        // No unwind tables: nothing unwinds a freestanding program.
        assert!(
            file.sections()
                .all(|section| section.name() != Ok(".eh_frame")),
            "{}",
            target.name()
        );
    }
}

#[test]
fn an_object_is_the_same_on_every_build() {
    for target in TARGETS {
        assert_eq!(object(ARITHMETIC, target), object(ARITHMETIC, target));
    }
}

#[test]
fn a_program_with_no_check_needs_no_symbol() {
    let source = r#"
export "C" def both(a: bool, b: bool) -> bool:
    return a and not b
"#;
    for target in TARGETS {
        let (global, _, undefined) = symbols(&object(source, target));
        assert_eq!(global, ["both"]);
        assert!(undefined.is_empty(), "{undefined:?}");
    }
}

#[test]
fn traits_generics_unions_and_constants_compile_to_words() {
    let source = r#"
LIMIT: int64 = 40
SCALE: float64 = 2.5

trait Shape:
    def area(self) -> int64

copy class Square:
    side: int64

copy class Rect:
    w: int64
    h: int64
    invariant self.w >= 0

    def grow(mut self, by: int64) -> None:
        self.w = self.w + by

impl Shape for Square:
    def area(self) -> int64:
        return self.side * self.side

impl Shape for Rect:
    def area(self) -> int64:
        return self.w * self.h

def twice[T: Shape](shape: T) -> int64:
    return shape.area() * 2

def pick(flag: bool, a: int64) -> int64 | float64 | None:
    if flag:
        return a
    if a > 3:
        return 1.5
    return None

export "C" def run(a: int64, flag: bool) -> int64:
    requires a < LIMIT
    mut rect = Rect(w=a, h=3)
    rect.grow(2)
    total = twice(Square(side=a)) + twice(rect) + min(a, 7) + max(a, 2) + abs(a)
    match pick(flag, a):
        case int64 as n:
            return total + n
        case float64 as f:
            return total + ((f * SCALE) as int64)
        case None:
            return total
"#;
    for target in TARGETS {
        let (global, _, undefined) = symbols(&object(source, target));
        assert_eq!(global, ["run"]);
        assert_eq!(undefined, ["aura_trap"]);
    }
}

#[test]
fn float32_results_and_wide_integer_casts_compile_on_every_target() {
    let source = r#"
export "C" def half(a: float32) -> float32:
    return a / (2.0 as float32)

export "C" def widen(a: uint64, b: int64) -> float64:
    return (a as float64) + (b as float64) + ((a as float32) as float64)

export "C" def narrow(a: float64) -> uint32:
    return a as uint32
"#;
    for target in TARGETS {
        let (global, _, undefined) = symbols(&object(source, target));
        assert_eq!(global, ["half", "narrow", "widen"]);
        assert_eq!(undefined, ["aura_trap"]);
    }
}

#[test]
fn a_generator_a_for_loop_consumes_compiles_to_a_plain_loop() {
    let source = r#"
from iter import Generator

def count(n: int64) -> Generator[int64]:
    mut i: int64 = 0
    while i < n:
        yield i
        i = i + 1

export "C" def run(a: int64) -> int64:
    mut total: int64 = 0
    for v in count(a):
        total = total + v
    return total
"#;
    for target in TARGETS {
        let (global, _, undefined) = symbols(&object(source, target));
        assert_eq!(global, ["run"]);
        assert_eq!(undefined, ["aura_trap"]);
    }
}

#[test]
fn hosted_constructs_in_helpers_are_reported() {
    for (source, expected) in [
        (
            r#"
def seed() -> int64:
    return 41
START: int64 = seed() + 1
export "C" def get() -> int64:
    return START
"#,
            &["reads the module constant"][..],
        ),
        (
            r#"
class Guard:
    code: int64
    def __enter__(mut self) -> None:
        pass
    def __exit__(mut self) -> None:
        pass
export "C" def run(a: int64) -> int64:
    with Guard(code=a) as guard:
        return a
"#,
            &["uses a `with` block"],
        ),
        (
            r#"
def total(values: list[int64]) -> int64:
    mut sum: int64 = 0
    for value in values:
        sum = sum + value
    return sum
export "C" def run(a: int64) -> int64:
    return total([a, a])
"#,
            &[
                "`run` builds a collection",
                "`total` takes `values` of type `list[int64]`",
                "reads an element of a collection",
            ],
        ),
        (
            r#"
trait Shape:
    def area(self) -> int64
copy class Square:
    side: int64
impl Shape for Square:
    def area(self) -> int64:
        return self.side
export "C" def run(a: int64) -> int64:
    shape: dyn Shape = Square(side=a)
    return shape.area()
"#,
            &["uses a trait object", "calls the method `area`"],
        ),
        (
            r#"
def parse(a: int64) -> Result[int64, str]:
    if a > 0:
        return Ok(a)
    return Err("no")
def inner(a: int64) -> Result[int64, str]:
    value = try parse(a)
    return Ok(value + 1)
export "C" def run(a: int64) -> int64:
    match inner(a):
        case Ok(value):
            return value
        case Err(_):
            return 0
"#,
            &[
                "`inner` uses `try`",
                "`parse` returns `Result[int64, str]`",
                "`run` matches on an enum value",
            ],
        ),
        (
            "export \"C\" def run(a: float64) -> float64:\n    return a % 2.0 + 3.0 // a\n",
            &["uses `%` on floats", "uses `//` on floats"],
        ),
        (
            r#"
export "C" def run(a: int64) -> int64:
    step = a + 1
    add: def(int64) -> int64 = lambda v: v + step
    return add(2)
"#,
            &["makes a function value"],
        ),
        (
            r#"
def work(a: int64) -> int64:
    return a + 1
export "C" def run(a: int64) -> int64:
    with group = TaskGroup():
        task = group.start(work, a)
    return a
"#,
            &["starts a task", "uses a `with` block"],
        ),
        (
            "import sys\nexport \"C\" def run(a: int64) -> int64:\n    return len(sys.args()) + a\n",
            &["calls `sys::args`"],
        ),
        (
            "export \"C\" def run(a: int64) -> int64:\n    return len(f\"{a}\")\n",
            &["builds text", "calls `len`"],
        ),
    ] {
        for expected in expected {
            assert_problem(source, expected);
        }
    }
}

#[test]
fn two_exports_with_one_symbol_are_refused() {
    let mut module = lower("export \"C\" def tick(a: int64) -> int64:\n    return a\n");
    let again = module.foreign_link.exports[0].clone();
    module.foreign_link.exports.push(again);
    let found = check_freestanding(&module);
    assert_eq!(found.len(), 1, "{found:?}");
    assert_eq!(found[0].code, UNSUPPORTED);
    assert!(found[0].message.contains("`tick`"), "{}", found[0].message);
}

#[test]
fn memory_functions_compile_to_instructions_on_every_target() {
    let source = r#"
import mem

UART: uint64 = 268435456

export "C" def put(byte: uint8) -> None:
    mem.volatile_write_uint8(UART, byte)

export "C" def status() -> uint32:
    return mem.volatile_read_uint32(UART + 4) | mem.volatile_read_uint32(UART + 4)

export "C" def copy_word(source: uint64, target: uint64) -> uint64:
    value = mem.read_uint64(source)
    mem.write_uint64(target, value)
    mem.write_uint16(target + 8, mem.read_uint16(source + 8))
    mem.write_uint32(target + 12, mem.read_uint32(source + 12))
    mem.write_uint8(target + 10, mem.read_uint8(source + 10))
    return value

export "C" def lock(cell: uint64) -> bool:
    return mem.atomic_compare_exchange_uint32(cell, 0, 1) == 0

export "C" def bump(cell: uint64) -> uint64:
    previous = mem.atomic_add_uint64(cell, 1)
    mem.fence()
    old = mem.atomic_swap_uint32(cell + 8, 7)
    mem.atomic_store_uint64(cell + 16, previous)
    mem.atomic_store_uint32(cell + 24, mem.atomic_load_uint32(cell + 8))
    mem.volatile_write_uint16(cell + 28, mem.volatile_read_uint16(cell + 30))
    mem.volatile_write_uint64(cell + 32, mem.volatile_read_uint64(cell + 40))
    mem.volatile_write_uint32(cell + 48, mem.volatile_read_uint8(cell + 49) as uint32)
    grown = mem.atomic_add_uint32(cell + 8, 1) as uint64
    kept = mem.atomic_swap_uint64(cell, 3) + mem.atomic_compare_exchange_uint64(cell, 3, 4)
    return mem.atomic_load_uint64(cell) + (old as uint64) + grown + kept
"#;
    for target in TARGETS {
        let (global, _, undefined) = symbols(&object(source, target));
        assert_eq!(global, ["bump", "copy_word", "lock", "put", "status"]);
        assert_eq!(undefined, ["aura_trap"]);
    }
}

#[test]
fn every_mem_function_is_a_host_builtin_the_emitter_compiles_inline() {
    let namespace = crate::builtin_modules::builtin_module_namespace(&["mem".to_string()])
        .expect("`mem` is a builtin module");
    assert_eq!(namespace.all_functions.len(), 27);
    for name in namespace.all_functions.keys() {
        let qualified = format!("mem::{name}");
        assert!(
            crate::runtime_ops::HostBuiltin::from_name(&qualified).is_some(),
            "`{qualified}` has a runtime entry"
        );
        let (access, _) = super::freestanding::memory_operation(&qualified)
            .unwrap_or_else(|| panic!("`{qualified}` is a memory operation"));
        let returns_none = !access.has_result();
        let declared_none = name.contains("write") || name.contains("store") || name == "fence";
        assert_eq!(returns_none, declared_none, "{qualified}");
    }
    for name in [
        "mem::read_uint128",
        "mem::peek_uint8",
        "math::floor",
        "read_uint8",
    ] {
        assert_eq!(super::freestanding::memory_operation(name), None, "{name}");
    }
}

#[test]
fn a_generic_function_reports_a_problem_once() {
    let found = problems(
        r#"
def show[T](value: own T) -> T:
    print(1)
    return value

export "C" def run(a: int64, b: float64) -> int64:
    show(b)
    return show(a)
"#,
    );
    assert_eq!(found.len(), 1, "{found:?}");
}

#[test]
fn only_the_functions_exports_reach_are_compiled_and_checked() {
    let source = r#"
def unused() -> str:
    return "a freestanding program never calls this"

def helper(value: int64) -> int64:
    return value + 1

def main() -> None:
    print(unused())

export "C" def entry(value: int64) -> int64:
    return helper(value)
"#;
    assert!(problems(source).is_empty());
    let prepared = freestanding::prepare(&lower(source)).expect("the program is in the subset");
    let mut names = prepared
        .functions
        .iter()
        .map(|function| function.name.as_str())
        .collect::<Vec<_>>();
    names.sort_unstable();
    assert_eq!(names, ["entry", "helper"]);
    assert!(prepared.top_level.is_none());
}

#[test]
fn the_program_may_define_the_trap_hook_and_a_memory_routine() {
    let source = r#"
export "C" def aura_trap(code: uint32, file: uint64, file_len: uint64, line: uint32, column: uint32) -> None:
    while true:
        pass

export "C" def memset(target: uint64, byte: uint64, count: uint64) -> uint64:
    return target

export "C" def half(value: int32) -> int32:
    return value // 2 + value * 3
"#;
    for target in TARGETS {
        let (global, weak, undefined) = symbols(&object(source, target));
        assert_eq!(global, ["aura_trap", "half", "memset"]);
        assert_eq!(weak, ["memcmp", "memcpy", "memmove"]);
        assert!(undefined.is_empty(), "{undefined:?}");
    }
}

#[test]
fn foreign_calls_pass_scalars_structs_text_and_callbacks() {
    let source = r#"
extern "C" class Pair:
    left: int32
    right: float64

extern "C" def scale_pair(pair: Pair, by: int32) -> Pair
extern "C" def apply(function: def(value: int32) -> int32, value: int32) -> int32
extern "C" def frac(value: float64, whole: mut int32) -> float64
extern "C" def log_line(text: cstr, level: uint8) -> bool

def twice(value: int32) -> int32:
    return value * 2

export "C" def entry(seed: int32) -> float64:
    pair = scale_pair(Pair(left=seed, right=1.5), 3)
    mut whole: int32 = 0
    part = frac(3.75, whole)
    logged = log_line("started", 2)
    mut bonus: int32 = 0
    if logged:
        bonus = apply(twice, seed)
    return pair.right + part + ((pair.left + whole + bonus) as float64)
"#;
    for target in [FreestandingTarget::X86_64, FreestandingTarget::Aarch64] {
        let (global, _, undefined) = symbols(&object(source, target));
        assert_eq!(global, ["entry"]);
        assert_eq!(
            undefined,
            ["apply", "aura_trap", "frac", "log_line", "scale_pair"]
        );
    }
    // The emitter has no rule yet for a struct by value on RISC-V.
    let refused = build(source, FreestandingTarget::Riscv64)
        .map(|object| object.len())
        .expect_err("no struct rule");
    assert!(
        refused[0]
            .message
            .contains("C struct by value on `riscv64`"),
        "{refused:?}"
    );
}

#[test]
fn scalar_foreign_calls_and_callbacks_compile_for_riscv() {
    let source = r#"
extern "C" def apply(function: def(value: uint32, out: mut int16) -> uint32, value: uint32) -> uint32
extern "C" def put(byte: uint8) -> None

def step(value: uint32, out: mut int16) -> uint32:
    out = out + 1
    return value + 1

export "C" def entry(seed: uint32, flag: bool, small: int8, ratio: float32) -> uint32:
    put(seed as uint8)
    mut bonus: uint32 = 0
    if flag and small < 0 and ratio > 0.5:
        bonus = 1
    return apply(step, seed) + bonus
"#;
    for target in TARGETS {
        let (global, _, undefined) = symbols(&object(source, target));
        assert_eq!(global, ["entry"]);
        assert_eq!(undefined, ["apply", "aura_trap", "put"]);
    }
}

#[test]
fn a_program_with_no_export_is_refused() {
    let found = check_freestanding(&lower("def main() -> None:\n    print(\"hi\")\n"));
    assert_eq!(found.len(), 1);
    assert!(found[0].message.contains("at least one `export \"C\" def`"));
}

#[test]
fn values_that_need_the_runtime_are_reported_by_construct() {
    let prelude = "export \"C\" def entry(count: int64) -> int64:\n";
    for (body, expected) in [
        ("    text = \"hello\"\n    return count\n", "holds `text` of type `str`"),
        (
            "    mut values: list[int64] = []\n    values.append(count)\n    return count\n",
            "builds a collection",
        ),
        (
            "    mut values: list[int64] = [count]\n    return values[0]\n",
            "reads or changes a collection",
        ),
        ("    print(count)\n    return count\n", "calls `print`"),
        ("    return 2 ** count\n", "uses `**`"),
        (
            "    pair = (count, count)\n    return pair[0]\n",
            "uses a tuple",
        ),
        (
            "    double: def(value: int64) -> int64 = lambda value: value * 2\n    return double(count)\n",
            "function value",
        ),
        (
            "    wide: int128 = 5\n    return count\n",
            "holds `wide` of type `int128`",
        ),
    ] {
        assert_problem(&format!("{prelude}{body}"), expected);
    }
    assert_problem(
        "export \"C\" def entry(a: float64, b: float64) -> float64:\n    return a % b + a // b\n",
        "uses `%` on floats",
    );
    assert_problem(
        "export \"C\" def entry(a: float64, b: float64) -> float64:\n    return a // b\n",
        "uses `//` on floats",
    );
}

#[test]
fn problems_name_the_function_and_the_line() {
    let source = r#"
def describe(code: int64) -> str:
    return f"code {code}"

enum Shape:
    Circle(radius: float64)
    Square(side: float64)

def area(shape: Shape) -> float64:
    match shape:
        case Shape.Circle(radius):
            return radius * radius
        case Shape.Square(side):
            return side * side

export "C" def entry(count: int64) -> int64:
    print(describe(count))
    return area(Shape.Square(side=2.0)) as int64
"#;
    let found = check_freestanding(&lower(source));
    let at = |text: &str| {
        found
            .iter()
            .find(|problem| problem.message.contains(text))
            .unwrap_or_else(|| panic!("no problem with `{text}`: {found:?}"))
            .span
            .map(|span| span.line)
    };
    assert_eq!(at("`describe` builds text"), Some(3));
    assert_eq!(at("`describe` returns `str`"), Some(2));
    assert_eq!(at("`area` takes `shape` of type `Shape`"), Some(9));
    assert_eq!(at("`area` matches on an enum value"), Some(10));
    assert_eq!(at("`entry` calls `print`"), Some(17));
    assert_eq!(at("`entry` uses an enum value"), Some(18));
    // One report for each construct and position.
    let mut seen = found
        .iter()
        .map(|problem| (problem.message.clone(), problem.span))
        .collect::<Vec<_>>();
    let count = seen.len();
    seen.dedup();
    assert_eq!(seen.len(), count);
}

#[test]
fn a_foreign_call_with_a_runtime_value_is_reported() {
    let source = r#"
extern "C" def length(text: cstr) -> uint64
extern "C" def name() -> cstr

def pass_on(text: str) -> uint64:
    return length(text)

export "C" def entry() -> uint64:
    return length("literal") + pass_on("held") + length(name())
"#;
    assert_problem(
        source,
        "passes `text` of type `str` to the foreign function `length`",
    );
    assert_problem(source, "takes a `str` from the foreign function `name`");
}

#[test]
fn math_the_emitter_computes_in_words_is_allowed_and_the_rest_reported() {
    let source = r#"
export "C" def entry(a: uint64, b: int64, c: float64) -> uint64:
    return max(a, b as uint64) + (min(b, 3) as uint64) + (abs(b) as uint64) + (sqrt(c) as uint64)
"#;
    assert!(problems(source).is_empty());
    let (_, _, undefined) = symbols(&object(source, FreestandingTarget::Aarch64));
    assert_eq!(undefined, [TRAP_HOOK]);
    assert_problem(
        "export \"C\" def entry(c: float64) -> int64:\n    return round(c)\n",
        "calls `round`",
    );
}

#[test]
fn an_object_that_names_a_runtime_symbol_is_refused() {
    let module = crate::lower_source_to_mir("def main() -> None:\n    print(\"hi\")\n")
        .expect("source should lower");
    let host = emit_host_object(&module).expect("the host object should emit");
    // A Mach-O or COFF host object names the runtime too.
    let error = freestanding::check_object(&host, &module).expect_err("it calls the runtime");
    assert!(error.starts_with("internal error:"), "{error}");
    assert!(error.contains("aura_direct_"), "{error}");
    assert!(freestanding::check_object(b"not an object", &module)
        .expect_err("not an object")
        .contains("failed to read"));
}

#[test]
fn a_trap_names_its_file_relative_to_the_program() {
    let directory = std::env::temp_dir().join(format!("aura-freestanding-{}", std::process::id()));
    let nested = directory.join("drivers");
    std::fs::create_dir_all(&nested).expect("the directory should be made");
    let program = directory.join("main.au");
    let module = nested.join("uart.au");
    std::fs::write(&program, "").expect("the file should be written");
    std::fs::write(&module, "").expect("the file should be written");
    let program = program.to_string_lossy().into_owned();
    assert_eq!(freestanding::trap_path(&program, &program), "main.au");
    assert_eq!(
        freestanding::trap_path(&module.to_string_lossy(), &program),
        "drivers/uart.au"
    );
    // A file outside the program's directory keeps the path it has.
    assert_eq!(
        freestanding::trap_path("/elsewhere/lib.au", &program),
        "/elsewhere/lib.au"
    );
    std::fs::remove_dir_all(&directory).expect("the directory should be removed");
    // A bare file name stands for a file in the working directory.
    assert_eq!(
        freestanding::trap_path("kernel.au", "kernel.au"),
        "kernel.au"
    );
}

#[test]
fn the_trap_hook_gets_the_program_file_name() {
    let source = "export \"C\" def half(value: int32) -> int32:\n    return 100 // value\n";
    let bytes = object(source, FreestandingTarget::X86_64);
    let file = object::File::parse(&*bytes).expect("the object should parse");
    let constants = file
        .sections()
        .filter(|section| section.name().is_ok_and(|name| name.starts_with(".rodata")))
        .flat_map(|section| section.data().expect("section data").to_vec())
        .collect::<Vec<_>>();
    assert!(
        constants
            .windows(b"kernel.au".len())
            .any(|window| window == b"kernel.au"),
        "the path is in the object's constants"
    );
}

#[test]
fn target_names_round_trip() {
    for target in TARGETS {
        assert_eq!(FreestandingTarget::parse(target.name()), Some(target));
    }
    assert_eq!(FreestandingTarget::parse("x86_64-unknown-linux-gnu"), None);
    assert_eq!(
        TARGETS.map(FreestandingTarget::arch),
        ["x86_64", "aarch64", "riscv64"]
    );
    // CI runs on x86-64 and AArch64, which both have a freestanding target.
    assert!(FreestandingTarget::host().is_some());
}

#[test]
fn an_emitter_fault_is_not_a_subset_report() {
    let fault = freestanding::emitter_diagnostic("the backend failed".to_string());
    assert_ne!(fault.code, UNSUPPORTED);
    assert_eq!(fault.message, "the backend failed");
    let refusal = freestanding::emitter_diagnostic(freestanding::refusal("`f` does x".to_string()));
    assert_eq!(refusal.code, UNSUPPORTED);
    assert_eq!(
        refusal.message,
        "`f` does x, which a freestanding build cannot compile"
    );
}
