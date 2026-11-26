# ADR-0128: Freestanding programs allocate through hooks, with an allocator written in Aura

- Status: Accepted
- Date: 2026-11-24
- Builds on: ADR-0094 (freestanding builds), ADR-0095 (the `mem` module)
- Design checkpoint: [A Heap For Freestanding Builds](../40-freestanding-heap-design-checkpoint.md)

## Context

A freestanding program (ADR-0094) held only machine words. It had no way
to ask for memory at run time, so a kernel could not keep a table that
grows, and no heap value of the language could exist there. A kernel
usually owns its memory map and often its allocator, and a small program
wants one that works with no setup beyond naming a region.

## Decision

- **Three allocation hooks, C symbols with sized frees.**

  ```c
  void *aura_alloc(size_t size, size_t align);
  void aura_free(void *address, size_t size, size_t align);
  void *aura_realloc(void *address, size_t old_size, size_t align, size_t new_size);
  ```

  `aura_alloc` and `aura_realloc` return null when the memory is not
  there. `align` is a power of two. A request for no bytes is a request
  for one. Every allocation a freestanding program makes goes through
  these three symbols.
- **`mem.alloc`, `mem.free`, and `mem.realloc` call the hooks.** They take
  and return `uint64` addresses, and `mem.alloc` returns 0 on failure. In
  a hosted program they use the process allocator, so the same source
  runs under `aura run` and `aura test`.
- **Aura's allocator is the default, and it is Aura source.** It is the
  file `crates/aura-compiler/std/freestanding_heap.au`, written in the
  freestanding subset over `mem` reads and writes. A build compiles it
  into the object when the program calls a heap function of `mem` and
  declares none of the hooks. The object then defines the hooks as weak
  symbols, so a strong definition at link time replaces them.
- **The program gives the allocator its memory.**
  `mem.heap_add_region(start, size)`, or `aura_heap_add_region` from C or
  assembly, hands it a region. It may be called more than once.
  `mem.heap_free_bytes()` reports what is left. The allocator's state is
  784 zeroed bytes in the object's `.bss`.
- **The allocator.** Blocks carry one header word and are multiples of 16
  bytes. Free blocks sit in 96 lists by size, with one bit per list to
  find the first that is not empty. A freed block joins the free blocks
  on either side at once. `realloc` grows in place when the next block is
  free. A spin lock guards every call.
- **A program with its own allocator defines all three hooks.** A program
  that declares any hook, as an `export "C" def` or an `extern "C" def`,
  gets no allocator from the build.

## Alternatives rejected

- **A support library in Rust, shipped as an archive for each target.**
  A freestanding build writes one object and links nothing. An archive
  needs three more build targets in every release, an install step, and a
  way for the user's linker to find it. The allocator compiles with the
  program instead, and the object stays the only output.
- **Mirroring the hosted runtime's value layout in a second runtime.**
  Hosted native code keeps every heap value in a 144-byte box around a
  dynamically typed value, linked to its task's ledger. A copy of that for
  bare metal would carry what a kernel does not want, and it would have
  to follow every change the hosted runtime makes.
- **`free(address)` with no size.** A sized free lets an allocator keep no
  header at all, which a slab or a bump allocator in a kernel wants. The
  caller always knows the size.
- **A trap when memory runs out.** A kernel must be able to refuse a
  request and carry on. `mem.alloc` returns 0. Heap values of the
  language, which have no way to report a failure, will trap.
- **`uint64 | None` as the result of `mem.alloc`.** Every other `mem`
  function deals in plain addresses, and typed pointers with a null value
  are a separate piece of work.
- **Regions in the linker script only.** A symbol pair such as
  `__heap_start` and `__heap_end` fixes the heap at link time. A kernel
  learns its memory map at boot, so the region is a call.

## Consequences

- A freestanding object that allocates grows by 4 to 6 KB of code, by
  target, and 784 bytes of zeroed data. One that does not is unchanged.
- The allocator is ordinary Aura code. Its tests run it on the host, and
  a failed check inside it reports `aura/freestanding_heap.au` to the
  trap hook.
- On x86-64 a small allocation or free takes about 40 ns, against about
  16 ns for glibc's `malloc` in the same loop. The lock is two atomic
  operations of that.
- The hooks are the base for heap values of the language in freestanding
  builds: `str`, `list`, enums with payloads, and the rest. Those are
  not part of this decision.
- An interrupt handler that allocates while the interrupted code holds
  the allocator's lock never returns. A kernel that allocates in handlers
  turns interrupts off around its allocations or brings its own hooks.
