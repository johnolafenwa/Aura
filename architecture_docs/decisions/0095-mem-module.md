# ADR-0095: The `mem` module reads and writes raw addresses

- Status: Accepted
- Date: 2026-11-12
- Builds on: ADR-0094 (freestanding builds), ADR-0074 (FFI v1)
- Design checkpoint: [Freestanding Builds](../28-freestanding-design-checkpoint.md)

## Context

A freestanding program (ADR-0094) could compute, and could reach hardware
only through C or assembly functions. A driver writes device registers, and
a kernel shares counters and locks between cores. Both need accesses the
optimizer leaves alone, and the second needs indivisible ones.

## Decision

- **A builtin module, `mem`, of functions over a `uint64` address.**
  `read_uintN` and `write_uintN`, `volatile_read_uintN` and
  `volatile_write_uintN` for 8, 16, 32, and 64 bits;
  `atomic_load`, `atomic_store`, `atomic_swap`, `atomic_add`, and
  `atomic_compare_exchange` for 32 and 64 bits; and `fence()`.
- **Importing `mem` needs `allow_ffi = true`.** Raw memory is the same
  grant as calling C: the package states that it steps outside Aura's
  memory safety. The permission follows the same package rules as an
  `extern` declaration.
- **Each call is inline code in native builds.** The backend has one
  operation, `memory_access`, that lowers to an LLVM load, store,
  `atomicrmw`, `cmpxchg`, or `fence`. Hosted and freestanding builds use
  it alike. The MIR interpreter runs the same functions as host builtins,
  so `aura run` gives the same results.
- **Plain accesses have alignment 1.** Volatile and atomic accesses have
  the natural alignment of their width, so a strict-alignment target emits
  one access of the full width and never byte pieces.
- **Atomic operations are sequentially consistent** and return the
  previous value. `atomic_add` wraps.
- **A bad address is undefined behavior.** Nothing checks an address.

## Alternatives rejected

- **A pointer type, `ptr[T]`, with a dereference.** It needs lifetime and
  aliasing rules to be more than an integer, and `ptr[T]` already means
  "a list passed to C" (ADR-0076).
- **An `unsafe` block.** It marks a region and leaves the question of what
  the region may do. The manifest permission already marks the package.
- **A separate permission, `allow_raw_memory`.** Two permissions for one
  kind of trust. A package that calls C can already do everything `mem`
  does.
- **Generic functions, `mem.read[uint32](address)`.** Host builtins are not
  generic over scalar width, and a name per width reads plainly at a
  register access.
- **Orderings as a parameter.** Weaker orderings are an optimization that
  is easy to get wrong. They can be added as new functions.
- **Signed and float accesses.** `as` converts at the access.

## Consequences

- A freestanding program can drive a memory-mapped device and implement a
  lock with no C or assembly beyond startup code.
- `aura verify` models a read as an unknown value and does not model
  writes, so a proof does not cover memory an Aura value shares with a
  `mem` write.
- x86-64 port I/O and inline assembly remain outside the language.
