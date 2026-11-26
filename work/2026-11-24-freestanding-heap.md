# Freestanding heap, step 1

Date: 2026-11-24. Decision: [ADR-0128](../architecture_docs/decisions/0128-freestanding-allocation-hooks.md).
Design: [checkpoint 40](../architecture_docs/40-freestanding-heap-design-checkpoint.md).

## What changed

- `mem` has five more functions: `alloc`, `free`, `realloc`,
  `heap_add_region`, and `heap_free_bytes`.
- A freestanding object calls the C symbols `aura_alloc`, `aura_free`,
  `aura_realloc`, `aura_heap_add_region`, and `aura_heap_free_bytes` for
  them. `native_codegen_freestanding_heap.rs` holds the table.
- A program that calls one and declares no hook gets the allocator in
  `crates/aura-compiler/std/freestanding_heap.au`. `prepare` lowers that
  source, renames its functions under `aura::heap::`, and adds them to the
  program. The emitter defines the five symbols as weak entries that pass
  the address of `aura_heap_state`, 784 zeroed bytes, to those functions.
- A hosted program runs the same five functions as host builtins over the
  process allocator.

## Measurements

Linux x86-64, cloud machine, one run each unless noted.

| Measure | Result |
| --- | --- |
| Code the allocator adds to an object | 4.1 KB x86-64, 6.2 KB AArch64, 5.5 KB RISC-V |
| Zeroed data | 784 bytes |
| One `alloc` or `free` of 16 to 255 bytes, 256 live blocks, 20 million steps, three runs | 38 to 41 ns |
| The same loop on glibc `malloc` through the hooks, three runs | 15 to 22 ns |
| Random workload through the hooks, 2 million steps on a 1 MB region | 1.9 s, no request refused, every byte returned |

## Compiler defect found

Not fixed here. A `uint64` compared with an arithmetic expression of two
integer literals, as in `i < 64 * 24` or `start > 0xFFFFFFFFFFFFFFFF - 32`,
passes `aura check` and then fails to lower: `uint64 has no method lt`.
`aura run` reports `AU4001` and `aura build` reports `AU2999`. One literal
or a typed module constant avoids it.

## Not done

- Heap values of the language in freestanding builds.
- `mem.copy` and `mem.fill`.
- A lock that is safe to take in an interrupt handler.
