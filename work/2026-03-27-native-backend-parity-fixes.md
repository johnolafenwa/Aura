# 2026-03-27 Native Backend Parity Fixes

## Goal

Close the direct-backend behavior gaps found in external testing so native binaries match the interpreter more closely on real Aurora programs.

## Work Completed

- Added CLI regression tests for:
  - float comparisons in conditions
  - float modulo
  - normal-scope `with` cleanup
  - `with` plus scalar early return
  - boolean printing
  - narrow integer overflow in direct-built binaries
  - trait impl dispatch on builtin `int32`
- Fixed MIR type inference for comparison expressions so temporary values lower as `bool` instead of inheriting operand numeric types.
- Fixed MIR lowering for `with` so an early return inside a `with` block stops later statements from overwriting the terminator.
- Fixed direct codegen cleanup handling so normal `PopCleanup` paths actually invoke `close(...)`.
- Added direct float modulo support by routing `%` on floats through the native runtime binary-value helper and unboxing the result.
- Fixed direct backend coercion for opaque numeric targets so assignments like `int8`, `uint8`, and other fixed-width integer targets go through checked runtime casts instead of silently widening.
- Fixed direct scalar trait dispatch so builtin scalar receivers like `int32` can call trait methods in native binaries.
- Fixed interpreter float rendering for round-tripped `float32` values so Aurora prints `3.14` instead of the raw expanded `f32` representation.
- Added a compiler fixture covering the `float32` display regression.

## Verification

- `cargo fmt`
- `cargo test -q -p aura --test cli`
- `cargo test -q -p aurora-compiler --test fixtures`
- `cargo test -q`
- `npm run test:lsp`
- `npm run ci`

## Follow-up

- Keep widening direct-backend regression coverage whenever external testing finds interpreter/native mismatches.
- Continue pushing compiler coverage toward enforced 100%, especially in `native_codegen.rs` and `native_runtime.rs`.
