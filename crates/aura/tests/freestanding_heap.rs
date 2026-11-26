//! The heap of a freestanding program (ADR-0128): the allocation hooks,
//! Aura's allocator behind them, and the same `mem` functions in a hosted
//! program.

#![cfg(unix)]

use std::fs;
use std::path::PathBuf;
use std::process::{Command, Output};
use std::sync::atomic::{AtomicU64, Ordering};

static NEXT_DIRECTORY: AtomicU64 = AtomicU64::new(0);

/// A package that may import `mem`, removed when the test ends.
struct Workspace {
    path: PathBuf,
}

impl Workspace {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "aura-freestanding-heap-{}-{}",
            std::process::id(),
            NEXT_DIRECTORY.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(path.join("src")).expect("the workspace should be made");
        let workspace = Self { path };
        workspace.write(
            "Aura.toml",
            "[package]\nname = \"heap\"\nversion = \"0.1.0\"\nedition = \"2026\"\nallow_ffi = true\n",
        );
        workspace
    }

    fn write(&self, name: &str, text: &str) {
        fs::write(self.path.join(name), text).expect("the file should be written");
    }

    fn aura(&self, args: &[&str]) -> Output {
        Command::new(env!("CARGO_BIN_EXE_aura"))
            .args(args)
            .current_dir(&self.path)
            .env("AURA_CACHE_DIR", self.path.join("cache"))
            .output()
            .expect("aura should start")
    }

    /// Builds `src/main.au` as a freestanding object, links it with the C
    /// program `harness`, runs the result, and returns what it printed.
    fn link_and_run(&self, compiler: &str, harness: &str) -> String {
        let build = self.aura(&["build", "--freestanding", "-o", "heap.o", "src/main.au"]);
        assert!(build.status.success(), "{}", stderr(&build));
        self.write("harness.c", harness);
        let link = Command::new(compiler)
            .args(["harness.c", "heap.o", "-o", "heap.bin"])
            .current_dir(&self.path)
            .output()
            .expect("the C compiler should start");
        assert!(link.status.success(), "{}", stderr(&link));
        let run = Command::new(self.path.join("heap.bin"))
            .output()
            .expect("the linked program should start");
        assert!(run.status.success(), "{}{}", stdout(&run), stderr(&run));
        stdout(&run)
    }
}

impl Drop for Workspace {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.path);
    }
}

fn stderr(output: &Output) -> String {
    String::from_utf8_lossy(&output.stderr).into_owned()
}

fn stdout(output: &Output) -> String {
    String::from_utf8_lossy(&output.stdout).into_owned()
}

/// The C compiler that links the host's ELF objects, on Linux.
fn c_compiler() -> Option<&'static str> {
    if !cfg!(target_os = "linux") {
        return None;
    }
    ["cc", "clang", "gcc"].into_iter().find(|compiler| {
        Command::new(compiler)
            .arg("--version")
            .output()
            .is_ok_and(|output| output.status.success())
    })
}

/// The start of every harness: a trap hook that reports the failed check
/// and fails the run.
const TRAP_HOOK: &str = r#"
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
void aura_trap(uint32_t code, const char *file, uint64_t file_len, uint32_t line, uint32_t column) {
    printf("trap %u at %.*s:%u:%u\n", code, (int)file_len, file, line, column);
    exit(1);
}
"#;

/// Allocates, grows, and frees blocks of random sizes and alignments
/// through `mem`, and checks that no block's bytes change under another.
/// Returns how many requests the heap could not meet.
const CHURN: &str = r#"import mem


def next_random(seed: uint64) -> uint64:
    return (seed * 1103515245 + 12345) % 2147483648


export "C" def churn(region: uint64, region_size: uint64, slots: uint64, rounds: uint64) -> uint64:
    mem.heap_add_region(region, region_size)
    total = mem.heap_free_bytes()
    mut i: uint64 = 0
    while i < 1536:
        mem.write_uint64(slots + i, 0)
        i = i + 8
    mut seed: uint64 = 42
    mut round: uint64 = 0
    mut failed: uint64 = 0
    while round < rounds:
        seed = next_random(seed)
        slot = slots + ((seed // 7) % 64) * 24
        address = mem.read_uint64(slot)
        seed = next_random(seed)
        action = (seed // 11) % 3
        seed = next_random(seed)
        mut size = (seed // 13) % 600
        if (seed // 5) % 17 == 0:
            size = size * 20
        seed = next_random(seed)
        mut align: uint64 = 1
        align = align << ((seed // 3) % 9)
        if address == 0:
            got = mem.alloc(size, align)
            if got == 0:
                failed = failed + 1
            else:
                assert got % align == 0, "an address is misaligned"
                mut j: uint64 = 0
                while j < size:
                    mem.write_uint8(got + j, ((round + j) % 251) as uint8)
                    j = j + 1
                mem.write_uint64(slot, got)
                mem.write_uint64(slot + 8, size)
                mem.write_uint64(slot + 16, round)
        else:
            old_size = mem.read_uint64(slot + 8)
            stamp = mem.read_uint64(slot + 16)
            mut j: uint64 = 0
            while j < old_size:
                assert mem.read_uint8(address + j) == ((stamp + j) % 251) as uint8, "memory changed"
                j = j + 1
            if action == 0:
                moved = mem.realloc(address, old_size, 16, size)
                if moved == 0:
                    failed = failed + 1
                else:
                    mut kept = old_size
                    if size < kept:
                        kept = size
                    j = 0
                    while j < kept:
                        assert mem.read_uint8(moved + j) == ((stamp + j) % 251) as uint8, "realloc lost bytes"
                        j = j + 1
                    j = 0
                    while j < size:
                        mem.write_uint8(moved + j, ((round + j) % 251) as uint8)
                        j = j + 1
                    mem.write_uint64(slot, moved)
                    mem.write_uint64(slot + 8, size)
                    mem.write_uint64(slot + 16, round)
            else:
                mem.free(address, old_size, 16)
                mem.write_uint64(slot, 0)
        round = round + 1
    i = 0
    while i < 64:
        mem.free(mem.read_uint64(slots + i * 24), mem.read_uint64(slots + i * 24 + 8), 16)
        i = i + 1
    assert mem.heap_free_bytes() == total, "the heap lost bytes"
    return failed
"#;

#[test]
fn the_allocator_serves_a_random_workload_through_the_hooks() {
    let Some(compiler) = c_compiler() else {
        return;
    };
    let workspace = Workspace::new();
    workspace.write("src/main.au", CHURN);
    let harness = format!(
        "{TRAP_HOOK}{}",
        r#"
uint64_t churn(uint64_t region, uint64_t size, uint64_t slots, uint64_t rounds);
uint64_t aura_heap_free_bytes(void);
void *aura_alloc(uint64_t size, uint64_t align);
void aura_free(void *address, uint64_t size, uint64_t align);
int main(void) {
    static char region[1 << 20];
    static uint64_t slots[192];
    uint64_t failed = churn((uint64_t)region, sizeof region, (uint64_t)slots, 200000);
    printf("failed %llu\n", (unsigned long long)failed);
    // C code reaches the same heap through the same symbols.
    uint64_t before = aura_heap_free_bytes();
    void *block = aura_alloc(100, 64);
    printf("aligned %d inside %d\n", (int)((uintptr_t)block % 64 == 0),
           (int)((char *)block >= region && (char *)block < region + sizeof region));
    aura_free(block, 100, 64);
    printf("returned %d\n", (int)(aura_heap_free_bytes() == before));
    return 0;
}
"#
    );
    assert_eq!(
        workspace.link_and_run(compiler, &harness),
        "failed 0\naligned 1 inside 1\nreturned 1\n"
    );
}

/// Aura's allocator with a function that walks the heap and checks each
/// of its rules, and a workload that calls it as blocks come and go.
const CHECKED: &str = concat!(
    include_str!("../../aura-compiler/std/freestanding_heap.au"),
    r#"
# Walks every block of the region that starts at `first` and checks the
# heap's rules. Returns the free bytes it counted.
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
            # The block is on the list for its size.
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
    assert mem.read_uint64(end) & SIZE_MASK == 0, "the end block has a size"
    assert (mem.read_uint64(end) & PREVIOUS_USED != 0) == previous_used, "the end flag is wrong"
    return free_bytes


def next_random(seed: uint64) -> uint64:
    return (seed * 1103515245 + 12345) % 2147483648


export "C" def checked(state: uint64, region: uint64, region_size: uint64, slots: uint64, rounds: uint64) -> uint64:
    mut i: uint64 = 0
    while i < 784:
        mem.write_uint64(state + i, 0)
        i = i + 8
    i = 0
    while i < 1536:
        mem.write_uint64(slots + i, 0)
        i = i + 8
    heap_add_region(state, region + 3, region_size - 5)
    first = heap_align_up(region + 3 + 8, 16) - 8
    end = ((region + region_size - 2) & SIZE_MASK) - 8
    total = end - first
    assert heap_free_bytes(state) == total
    assert check_region(state, first, end) == total
    mut seed: uint64 = 42
    mut round: uint64 = 0
    mut failed: uint64 = 0
    while round < rounds:
        seed = next_random(seed)
        slot = slots + ((seed // 7) % 64) * 24
        address = mem.read_uint64(slot)
        seed = next_random(seed)
        action = (seed // 11) % 3
        seed = next_random(seed)
        mut size = (seed // 13) % 600
        if (seed // 5) % 17 == 0:
            size = size * 20
        seed = next_random(seed)
        mut align: uint64 = 1
        align = align << ((seed // 3) % 9)
        if address == 0:
            got = heap_alloc(state, size, align)
            if got == 0:
                failed = failed + 1
            else:
                assert got % align == 0, "an address is misaligned"
                assert got % 16 == 0
                assert got >= first + 8 and got + size <= end
                mut j: uint64 = 0
                while j < size:
                    mem.write_uint8(got + j, ((round + j) % 251) as uint8)
                    j = j + 1
                mem.write_uint64(slot, got)
                mem.write_uint64(slot + 8, size)
                mem.write_uint64(slot + 16, round)
        else:
            old_size = mem.read_uint64(slot + 8)
            stamp = mem.read_uint64(slot + 16)
            mut j: uint64 = 0
            while j < old_size:
                assert mem.read_uint8(address + j) == ((stamp + j) % 251) as uint8, "memory changed"
                j = j + 1
            if action == 0:
                moved = heap_realloc(state, address, 16, size)
                if moved == 0:
                    failed = failed + 1
                else:
                    assert moved % 16 == 0
                    mut kept = old_size
                    if size < kept:
                        kept = size
                    j = 0
                    while j < kept:
                        assert mem.read_uint8(moved + j) == ((stamp + j) % 251) as uint8, "realloc lost bytes"
                        j = j + 1
                    j = 0
                    while j < size:
                        mem.write_uint8(moved + j, ((round + j) % 251) as uint8)
                        j = j + 1
                    mem.write_uint64(slot, moved)
                    mem.write_uint64(slot + 8, size)
                    mem.write_uint64(slot + 16, round)
            else:
                heap_free(state, address)
                mem.write_uint64(slot, 0)
        if round % 50 == 0:
            assert check_region(state, first, end) == heap_free_bytes(state)
        round = round + 1
    i = 0
    while i < 64:
        heap_free(state, mem.read_uint64(slots + i * 24))
        i = i + 1
    assert check_region(state, first, end) == total
    assert heap_free_bytes(state) == total
    # Everything joined back into one block.
    assert mem.read_uint64(first) == total | PREVIOUS_USED
    # Requests the heap cannot meet.
    assert heap_alloc(state, total, 16) == 0
    assert heap_alloc(state, 16, 3) == 0
    assert heap_alloc(state, 16, 0) == 0
    assert heap_alloc(state, ALL_BITS, 16) == 0
    whole = heap_alloc(state, total - 8, 16)
    assert whole == first + 8
    assert heap_free_bytes(state) == 0
    assert heap_alloc(state, 1, 1) == 0
    heap_free(state, whole)
    assert heap_free_bytes(state) == total
    return failed
"#
);

#[test]
fn the_allocator_keeps_every_rule_of_the_heap_under_a_random_workload() {
    let Some(compiler) = c_compiler() else {
        return;
    };
    let workspace = Workspace::new();
    workspace.write("src/main.au", CHECKED);
    let harness = format!(
        "{TRAP_HOOK}{}",
        r#"
uint64_t checked(uint64_t state, uint64_t region, uint64_t size, uint64_t slots, uint64_t rounds);
int main(void) {
    static uint64_t state[98];
    static char region[1 << 16];
    static uint64_t slots[192];
    uint64_t failed = checked((uint64_t)state, (uint64_t)region, sizeof region, (uint64_t)slots, 20000);
    // A heap this small turns some requests down, and most succeed.
    printf("%d\n", (int)(failed > 0 && failed < 10000));
    return 0;
}
"#
    );
    assert_eq!(workspace.link_and_run(compiler, &harness), "1\n");
}

/// A program that allocates through `mem` and never names a hook.
const COUNTER: &str = r#"
import mem

export "C" def sum_to(limit: uint64) -> uint64:
    block = mem.alloc(limit * 8, 8)
    if block == 0:
        return 0
    mut i: uint64 = 0
    while i < limit:
        mem.write_uint64(block + i * 8, i + 1)
        i = i + 1
    grown = mem.realloc(block, limit * 8, 8, limit * 16)
    mut total: uint64 = 0
    i = 0
    while i < limit:
        total = total + mem.read_uint64(grown + i * 8)
        i = i + 1
    mem.free(grown, limit * 16, 8)
    return total
"#;

#[test]
fn hooks_the_environment_defines_replace_the_allocator() {
    let Some(compiler) = c_compiler() else {
        return;
    };
    let workspace = Workspace::new();
    workspace.write("src/main.au", COUNTER);
    let harness = format!(
        "{TRAP_HOOK}{}",
        r#"
#include <string.h>
uint64_t sum_to(uint64_t limit);
static int allocs, frees, reallocs;
void *aura_alloc(uint64_t size, uint64_t align) { allocs++; return aligned_alloc(align, size); }
void aura_free(void *address, uint64_t size, uint64_t align) { frees++; free(address); }
void *aura_realloc(void *address, uint64_t old_size, uint64_t align, uint64_t new_size) {
    reallocs++;
    return realloc(address, new_size);
}
int main(void) {
    unsigned long long sum = sum_to(100);
    printf("%llu %d %d %d\n", sum, allocs, frees, reallocs);
    return 0;
}
"#
    );
    assert_eq!(workspace.link_and_run(compiler, &harness), "5050 1 1 1\n");
}

#[test]
fn a_heap_with_no_region_turns_every_request_down() {
    let Some(compiler) = c_compiler() else {
        return;
    };
    let workspace = Workspace::new();
    workspace.write("src/main.au", COUNTER);
    let harness = format!(
        "{TRAP_HOOK}{}",
        r#"
uint64_t sum_to(uint64_t limit);
void aura_heap_add_region(uint64_t start, uint64_t size);
int main(void) {
    static char region[4096];
    printf("%llu\n", (unsigned long long)sum_to(100));
    aura_heap_add_region((uint64_t)region, sizeof region);
    printf("%llu\n", (unsigned long long)sum_to(100));
    return 0;
}
"#
    );
    assert_eq!(workspace.link_and_run(compiler, &harness), "0\n5050\n");
}

/// The same functions in a hosted program, where the operating system
/// provides the memory.
const HOSTED: &str = r#"
import mem

def main() -> None:
    mem.heap_add_region(4096, 4096)
    print(mem.heap_free_bytes())
    block = mem.alloc(24, 8)
    print(block != 0)
    print(block % 8)
    mem.write_uint64(block + 16, 77)
    grown = mem.realloc(block, 24, 8, 4000)
    print(mem.read_uint64(grown + 16))
    mem.free(grown, 4000, 8)
    print(mem.alloc(8, 3))
    mem.free(0, 8, 8)
    wide = mem.alloc(0, 4096)
    print(wide % 4096)
    mem.free(wide, 0, 4096)
    fresh = mem.realloc(0, 0, 16, 64)
    print(fresh % 16)
    print(mem.realloc(fresh, 64, 3, 64))
    mem.free(fresh, 64, 16)
"#;

#[test]
fn a_hosted_program_allocates_from_the_process() {
    let workspace = Workspace::new();
    workspace.write("src/main.au", HOSTED);
    let expected = "0\ntrue\n0\n77\n0\n0\n0\n0\n";
    let run = workspace.aura(&["run", "src/main.au"]);
    assert!(run.status.success(), "{}", stderr(&run));
    assert_eq!(stdout(&run), expected);
    let build = workspace.aura(&["build", "-o", "hosted", "src/main.au"]);
    assert!(build.status.success(), "{}", stderr(&build));
    let built = Command::new(workspace.path.join("hosted"))
        .output()
        .expect("the built program should start");
    assert!(built.status.success(), "{}", stderr(&built));
    assert_eq!(stdout(&built), expected);
}
