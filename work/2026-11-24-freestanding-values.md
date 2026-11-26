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

## Enums that hold themselves (ADR-0160)

- An `indirect` payload of an enum's own type is the `uint64` address of a
  block: a reference count, then the payload's words. The block functions
  are Aura source in `crates/aura-compiler/std/freestanding_values.au`.
- The pass makes store, load, drop, retain, and release functions for
  each class that owns a block, calls retain and release where the
  ownership pass stated a reference, reads a moved part as a copy with a
  reference, and replaces a lend by a reference for the call.
- `try` now consumes a result that needs drop.
- The last variant of a `match` is a jump, not a test.
- 5 more compiler tests and 1 more command test.
  `crates/aura/tests/freestanding_boxes.au` (a binary tree, an expression
  tree, a list, a class, a tuple, a generic enum, and results that hold
  them) prints 16043307 hosted and freestanding, and the linked program
  ends with every block freed.

## Text (ADR-0160, section Text)

- `str` is a class of one word, the address of a block of a count, a
  byte length, the bytes, and a zero byte. The library is
  `crates/aura-compiler/std/freestanding_text.au`.
- The pass rewrites literals, `+`, comparisons, `len`, `byte_len`,
  `starts_with`, `ends_with`, `contains`, f-strings of text, integers and `bool`, and `str` arguments of foreign
  calls. Assertion text is stripped before the pass.
- 3 more compiler tests, 1 more for foreign calls, and 1 more command
  test: `crates/aura/tests/freestanding_text.au` prints 812162342 hosted
  and freestanding with every block freed.

## Lists (ADR-0160, section Lists)

- `list[T]` for a scalar `T` is a class of one word, the address of a
  header of a count, a length, a capacity, and a buffer address. The
  library is `crates/aura-compiler/std/freestanding_list.au`.
- The pass rewrites literals, `append`, `len`, `is_empty`, `clear`, and
  element reads and writes, and drops the range facts they used.
- 2 more compiler tests and 2 more command tests:
  `crates/aura/tests/freestanding_list.au` prints 1028420 hosted and
  freestanding with every block freed, and an index out of range calls
  the trap hook.

## Follow-up

- Lists of text, classes, and other values; the other list methods.
- A trap code and the program's position for an index out of range.
- Floats and format specs in f-strings, indexing, and slices of `str`.
- `str | None`.
- Literal blocks in the program's data.
- Unions as enum payloads, and unions of values that own a block.
- A write to a field that owns a block.
- A part moved out without a retain and a release.
- A trap code of its own for a heap that is full.
- Variants that share space.
- Values with operations of their own: `str`, `bytes`, `list`, and enums
  that hold them.
