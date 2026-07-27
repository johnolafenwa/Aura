# 2026-03-29: Vec Collections

## Goal

Add a real built-in owned collection surface so the maintained Aurora subset can express ordinary sequence-processing programs instead of stopping at scalars, classes, enums, and channels.

## Work Completed

- Added builtin `Vec[T]` support across parsing, checking, interpretation, MIR lowering/runtime, direct native codegen, and direct native runtime helpers.
- Added list literals such as `[1, 2, 3]` with element-type consistency checks and explicit diagnostics for empty literals that lack an expected `Vec[T]` type.
- Added indexed reads and indexed assignments for vectors, with span-aware runtime diagnostics for out-of-bounds indexing.
- Made indexed reads borrow-safe for non-copy element types so repeated reads like `names[0]` and `names[1]` do not consume the whole vector.
- Added `Vec[T]()` as the explicit empty-vector constructor.
- Added maintained vector methods: `len`, `is_empty`, `clone`, `push`, `pop`, `get`, `set`, `remove`, `swap`, `contains`, and `extend`.
- Changed `Vec.len()` to return `int32`, so ordinary index loops can use `range(values.len())` directly.
- Added `for value in vec:`, `for value in vec:`, and `for value in mut vec:` iteration support for `Vec[T]`.
- Fixed `for value in mut vec:` so immutable vector bindings now fail during checking instead of being mutated through the loop body.
- Added `Vec[T]` equality and inequality for same-typed vectors across checking and all maintained runtimes/backends.
- Fixed mixed-construction `Vec[T]` equality in the interpreter and MIR runtime so vectors compare by element contents rather than leaking internal runtime element-type markers from empty annotated literals.
- Fixed follow-up checker/runtime parity gaps so `for value in mut vec:` now rejects immutable vector places and `Vec[T]()` constructor locals keep their vector type through MIR/direct-backend `for` lowering.
- Added compiler-backed completion support for `Vec[T]` members and regression coverage in both the JS fallback analysis tests and compiler bridge tests.
- Added maintained example programs under `examples/collections/` and updated the README/tutorial track to teach the new surface.
- Added CLI regression coverage for `run-mir` and native builds over vector programs and example-file smoke coverage for the maintained collection examples.

## Verification

- `cargo fmt --all`
- `cargo test -p aurora-compiler`
- `cargo test -p aura`
- `npm run test:lsp`
- `npm run check:extension`

## Follow-Up

- Aurora still does not have user-defined iterable protocols or broader standard-library collection types beyond builtin `Vec[T]`.
