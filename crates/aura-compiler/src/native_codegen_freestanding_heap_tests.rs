use super::*;
use object::{Object, ObjectSection, ObjectSymbol};

const HEAP_NAMES: [&str; 5] = [
    "aura_alloc",
    "aura_free",
    "aura_heap_add_region",
    "aura_heap_free_bytes",
    "aura_realloc",
];

fn lower(source: &str) -> MirModule {
    let module = crate::parse_source(source).expect("source should parse");
    let program = crate::check_module_with_builtin_imports(module).expect("source should check");
    crate::mir::lower_with_source_locations(&program)
}

fn object(source: &str, target: FreestandingTarget) -> Vec<u8> {
    emit_freestanding_object(&lower(source), "kernel.au", source, target).unwrap_or_else(
        |problems| {
            panic!(
                "freestanding build failed: {:?}",
                problems
                    .iter()
                    .map(|problem| &problem.message)
                    .collect::<Vec<_>>()
            )
        },
    )
}

/// The object's heap symbols by kind: strong definitions, weak
/// definitions, and undefined names, each sorted.
fn heap_symbols(bytes: &[u8]) -> (Vec<String>, Vec<String>, Vec<String>) {
    let file = object::File::parse(bytes).expect("the object should parse");
    let (mut strong, mut weak, mut undefined) = (Vec::new(), Vec::new(), Vec::new());
    for symbol in file.symbols() {
        let name = symbol.name().unwrap_or_default().to_string();
        if !HEAP_NAMES.contains(&name.as_str()) {
            continue;
        }
        if symbol.is_undefined() {
            undefined.push(name);
        } else if symbol.is_weak() {
            weak.push(name);
        } else {
            strong.push(name);
        }
    }
    strong.sort();
    weak.sort();
    undefined.sort();
    (strong, weak, undefined)
}

const USES_HEAP: &str = r#"
import mem

export "C" def boot(start: uint64, size: uint64) -> uint64:
    mem.heap_add_region(start, size)
    block = mem.alloc(64, 16)
    wider = mem.realloc(block, 64, 16, 256)
    mem.free(wider, 256, 16)
    return mem.heap_free_bytes()
"#;

#[test]
fn a_program_that_allocates_carries_the_allocator_as_weak_symbols() {
    for target in FreestandingTarget::ALL {
        let bytes = object(USES_HEAP, target);
        let (strong, weak, undefined) = heap_symbols(&bytes);
        assert!(strong.is_empty(), "{strong:?}");
        assert_eq!(weak, HEAP_NAMES, "{}", target.name());
        assert!(undefined.is_empty(), "{undefined:?}");
        // The heap's state is zeroed memory the object reserves.
        let file = object::File::parse(&*bytes).expect("the object should parse");
        let state = file
            .symbols()
            .find(|symbol| symbol.name() == Ok("aura_heap_state"))
            .expect("the object holds the heap's state");
        assert!(!state.is_global());
        let section = file
            .section_by_index(state.section_index().expect("the state has a section"))
            .expect("the section exists");
        assert_eq!(section.kind(), object::SectionKind::UninitializedData);
        assert!(section.size() >= STATE_BYTES as u64);
    }
}

#[test]
fn the_object_with_the_allocator_is_the_same_on_every_build() {
    for target in FreestandingTarget::ALL {
        assert_eq!(object(USES_HEAP, target), object(USES_HEAP, target));
    }
}

#[test]
fn a_program_that_never_allocates_carries_no_allocator() {
    let source = r#"
import mem

export "C" def peek(address: uint64) -> uint64:
    return mem.read_uint64(address)
"#;
    let module = freestanding::prepare(&lower(source)).expect("the program compiles");
    assert!(!has_allocator(&module));
    for target in FreestandingTarget::ALL {
        let (strong, weak, undefined) = heap_symbols(&object(source, target));
        assert!(strong.is_empty() && weak.is_empty() && undefined.is_empty());
    }
}

#[test]
fn a_program_that_defines_a_hook_brings_its_own_allocator() {
    let source = r#"
import mem

export "C" def aura_alloc(size: uint64, align: uint64) -> uint64:
    return 4096

export "C" def aura_free(address: uint64, size: uint64, align: uint64) -> None:
    pass

export "C" def aura_realloc(address: uint64, old_size: uint64, align: uint64, new_size: uint64) -> uint64:
    return address

export "C" def boot() -> uint64:
    block = mem.alloc(8, 8)
    mem.free(mem.realloc(block, 8, 8, 16), 16, 8)
    return block + mem.heap_free_bytes()
"#;
    let module = freestanding::prepare(&lower(source)).expect("the program compiles");
    assert!(!has_allocator(&module));
    for target in FreestandingTarget::ALL {
        let (strong, weak, undefined) = heap_symbols(&object(source, target));
        assert_eq!(strong, ["aura_alloc", "aura_free", "aura_realloc"]);
        assert!(weak.is_empty(), "{weak:?}");
        // The program's allocator did not come with this function, so
        // whatever links the object defines it.
        assert_eq!(undefined, ["aura_heap_free_bytes"]);
    }
}

#[test]
fn a_program_that_imports_a_hook_leaves_every_heap_symbol_to_its_environment() {
    let source = r#"
import mem

extern "C" def aura_alloc(size: uint64, align: uint64) -> uint64

export "C" def boot() -> uint64:
    block = aura_alloc(8, 8)
    mem.free(block, 8, 8)
    return mem.alloc(8, 8)
"#;
    for target in FreestandingTarget::ALL {
        let (strong, weak, undefined) = heap_symbols(&object(source, target));
        assert!(strong.is_empty() && weak.is_empty(), "{strong:?} {weak:?}");
        assert_eq!(undefined, ["aura_alloc", "aura_free"]);
    }
}

#[test]
fn every_heap_function_of_mem_has_a_symbol_and_a_runtime_entry() {
    let namespace = crate::builtin_modules::builtin_module_namespace(&["mem".to_string()])
        .expect("`mem` is a builtin module");
    for symbol in HEAP_SYMBOLS {
        let name = symbol.builtin.strip_prefix("mem::").expect("a `mem` name");
        let function = &namespace.all_functions[name];
        assert_eq!(function.signature.params.len(), symbol.params, "{name}");
        assert!(
            crate::runtime_ops::HostBuiltin::from_name(symbol.builtin).is_some(),
            "`{name}` has a runtime entry"
        );
        assert_eq!(heap_operation(symbol.builtin), Some(&symbol));
        assert!(is_heap_symbol(symbol.symbol));
        assert!(symbol.passes.iter().all(|&index| index < symbol.params));
    }
    assert_eq!(heap_operation("mem::read_uint8"), None);
    assert_eq!(heap_operation("alloc"), None);
    assert!(!is_heap_symbol("aura_trap"));
}

#[test]
fn the_allocator_adds_only_its_own_renamed_functions() {
    let functions = allocator_functions();
    assert!(functions.len() > 20, "{}", functions.len());
    for function in &functions {
        assert!(
            function.name.starts_with(ALLOCATOR_PREFIX),
            "{}",
            function.name
        );
        assert_eq!(function.source_path.as_deref(), Some(ALLOCATOR_PATH));
    }
    for symbol in HEAP_SYMBOLS {
        let function = functions
            .iter()
            .find(|function| function.name == allocator_name(symbol.function))
            .unwrap_or_else(|| panic!("the allocator defines `{}`", symbol.function));
        // The state's address, then the words the symbol passes on.
        assert_eq!(function.params.len(), 1 + symbol.passes.len());
    }
    // A program function with an allocator function's source name is
    // another function.
    let source = r#"
import mem

def heap_alloc(state: uint64, size: uint64, align: uint64) -> uint64:
    return state + size + align

export "C" def boot() -> uint64:
    return heap_alloc(1, 2, 3) + mem.alloc(8, 8)
"#;
    let module = freestanding::prepare(&lower(source)).expect("the program compiles");
    assert!(has_allocator(&module));
    assert!(module.functions.iter().any(|f| f.name == "heap_alloc"));
    for target in FreestandingTarget::ALL {
        object(source, target);
    }
}

#[test]
fn a_hosted_build_calls_the_runtime_for_a_heap_function() {
    let source = r#"
import mem

def main() -> None:
    block = mem.alloc(16, 8)
    mem.free(block, 16, 8)
"#;
    let module = lower(source);
    assert!(uses_heap(&module));
    let object = emit_host_object(&module).expect("the program compiles");
    let file = object::File::parse(&*object).expect("the object should parse");
    let undefined = file
        .symbols()
        .filter(|symbol| symbol.is_undefined())
        .filter_map(|symbol| symbol.name().ok())
        .collect::<Vec<_>>();
    assert!(undefined.contains(&"aura_host_mem_alloc"), "{undefined:?}");
    assert!(!undefined.contains(&"aura_alloc"), "{undefined:?}");
}

/// A program that runs Aura's allocator on memory it gets from the host
/// and checks every rule of the heap after each step.
const CHECKED_RUN: &str = r#"

def check_region(state: uint64, first: uint64, end: uint64) -> uint64:
    mut block = first
    mut previous_used = true
    mut free_bytes: uint64 = 0
    while block != end:
        header = mem.read_uint64(block)
        size = header & SIZE_MASK
        assert size >= 32, "a block is too small"
        assert (block + 8) % 16 == 0, "a block is misaligned"
        assert (header & PREVIOUS_USED != 0) == previous_used, "a previous flag is wrong"
        if header & USED == 0:
            assert previous_used, "two free blocks touch"
            assert mem.read_uint64(block + size - 8) == size, "a footer is wrong"
            free_bytes = free_bytes + size
            mut walk = mem.read_uint64(heap_list_head(state, heap_list_of(size)))
            mut found = false
            while walk != 0:
                if walk == block:
                    found = true
                walk = mem.read_uint64(walk + 8)
            assert found, "a free block is on no list"
        previous_used = header & USED != 0
        block = block + size
        assert block <= end, "a block runs past the region"
    assert (mem.read_uint64(end) & PREVIOUS_USED != 0) == previous_used, "the end flag is wrong"
    return free_bytes


def main() -> None:
    region_size: uint64 = 4096
    state = mem.alloc(784, 8)
    mut i: uint64 = 0
    while i < 784:
        mem.write_uint64(state + i, 0)
        i = i + 8
    region = mem.alloc(region_size, 1)
    heap_add_region(state, region + 3, region_size - 5)
    first = heap_align_up(region + 3 + 8, 16) - 8
    end = ((region + region_size - 2) & SIZE_MASK) - 8
    total = end - first
    assert heap_free_bytes(state) == total
    # A region too small for a block changes nothing.
    heap_add_region(state, region, 24)
    heap_add_region(state, ALL_BITS - 8, 64)
    assert check_region(state, first, end) == total
    a = heap_alloc(state, 100, 8)
    b = heap_alloc(state, 40, 64)
    c = heap_alloc(state, 1500, 16)
    d = heap_alloc(state, 0, 1)
    assert a != 0 and b != 0 and c != 0 and d != 0
    assert b % 64 == 0 and a % 16 == 0
    mem.write_uint64(a + 88, 7)
    assert check_region(state, first, end) == heap_free_bytes(state)
    # Growing into the free block after `c`, then moving `a`.
    assert heap_realloc(state, c, 16, 1800) == c
    moved = heap_realloc(state, a, 8, 600)
    assert moved != 0 and moved != a
    assert mem.read_uint64(moved + 88) == 7
    assert heap_realloc(state, moved, 8, 50) == moved
    assert heap_realloc(state, 0, 8, 0) != 0
    assert check_region(state, first, end) == heap_free_bytes(state)
    heap_free(state, b)
    heap_free(state, 0)
    heap_free(state, c)
    assert check_region(state, first, end) == heap_free_bytes(state)
    # Requests the heap cannot meet leave it as it was.
    free = heap_free_bytes(state)
    assert heap_alloc(state, total, 16) == 0
    assert heap_alloc(state, 16, 3) == 0
    assert heap_alloc(state, 16, 0) == 0
    assert heap_alloc(state, ALL_BITS, 16) == 0
    assert heap_alloc(state, 2000, 4096) == 0
    assert heap_realloc(state, moved, 8, total) == 0
    assert heap_realloc(state, moved, 8, ALL_BITS) == 0
    assert heap_free_bytes(state) == free
    print(heap_list_of(32))
    print(heap_list_of(1040))
    print(heap_list_of(1056))
    print(heap_list_of(4096))
    print(heap_list_of(SIZE_LIMIT))
    print("ok")
"#;

#[test]
fn the_allocator_keeps_its_rules_when_run_as_an_ordinary_program() {
    let source = format!("{ALLOCATOR_SOURCE}{CHECKED_RUN}");
    let module = crate::parse_source(&source).expect("source should parse");
    let program = crate::check_module_with_builtin_imports(module).expect("source should check");
    let captured = std::sync::Arc::new(std::sync::Mutex::new(String::new()));
    let sink_capture = captured.clone();
    let sink: crate::StdoutSink = std::sync::Arc::new(move |chunk| {
        sink_capture
            .lock()
            .expect("the sink is not poisoned")
            .push_str(chunk);
    });
    crate::run_mir_with_stdout_sink(&crate::mir::lower_implicit(&program), Some(sink))
        .expect("the checked run passes");
    let output = captured.lock().expect("the sink is not poisoned").clone();
    assert_eq!(output, "0\n63\n64\n66\n95\nok\n");
}

#[test]
fn a_heap_function_the_program_exports_is_the_programs() {
    let source = r#"
import mem

export "C" def aura_heap_free_bytes() -> uint64:
    return 7

export "C" def boot() -> uint64:
    return mem.alloc(8, 8) + mem.heap_free_bytes()
"#;
    for target in FreestandingTarget::ALL {
        let (strong, weak, undefined) = heap_symbols(&object(source, target));
        assert_eq!(strong, ["aura_heap_free_bytes"]);
        assert_eq!(
            weak,
            [
                "aura_alloc",
                "aura_free",
                "aura_heap_add_region",
                "aura_realloc"
            ]
        );
        assert!(undefined.is_empty(), "{undefined:?}");
    }
}
