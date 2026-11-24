# Freestanding Tuples And Enums Of Words

Date: 2026-11-24

## Goal

Step 2 of the freestanding heap work, by route D1 of
[checkpoint 40](../architecture_docs/40-freestanding-heap-design-checkpoint.md),
which the owner chose: the values of the language in freestanding builds
through a lean layer of their own. This note covers the first part, the
values that need no heap (ADR-0129).

## Work Completed

- `native_codegen_freestanding_values.rs`: a MIR pass that `prepare` runs.
  It rewrites each tuple, enum, and `Result` whose parts are all words to a
  class of words. A `match` on such an enum becomes comparisons of a tag.
- A generic variant with no payload, such as the `None2` of an
  `Option2[int64]`, has no type arguments in lowered MIR. The pass takes
  them from the place the value goes to.
- A temporary the pass adds is named `%t` and the next free number, which
  MIR validation requires.
- A class the pass makes needs drop exactly when its type did, so the
  ownership checker accepts the rewritten module.
- The test in `native_codegen_freestanding_heap_tests.rs` that runs the
  allocator under the interpreter no longer depends on where the process
  allocator puts the region. It failed on some runs before.
- `try` on a result the pass rewrote becomes a test of the tag and an
  early return of the error, through the `From` function when the error
  types differ.
- Manual, book, changelog, ADR-0129, and checkpoint 40.

## Verification

- 8 compiler tests in `native_codegen_freestanding_values_tests.rs`: the
  rewrite leaves no tuple, enum, or `match` operation; objects build on
  x86-64, AArch64, and RISC-V and are the same on every build; drop status
  is kept; a tuple holds a union; a wildcard arm; refusals for text and
  `indirect` payloads.
- 1 command test over two programs: `crates/aura/tests/freestanding_values.au`
  prints 15726 and `freestanding_try.au` prints 8544 under `aura run`, and
  each one's freestanding object linked with a C harness prints the same.
- By hand: equality of tuples and of enum values, and tuple unpacking in a
  loop, give the hosted results.

## Follow-up

- `try` on a result that needs drop.
- Unions as enum payloads.
- Variants that share space.
- Values that need a heap block: `str`, `bytes`, `list`, enums that hold
  them, and `indirect` payloads.
