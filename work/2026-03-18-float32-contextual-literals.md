# 2026-03-18 Float32 Contextual Literals

## Goal

Close the remaining implemented-surface gap where `float32` existed as a declared primitive type but floating-point literals could not be used in normal typed `float32` contexts.

## Work Completed

- Added fixture coverage for `float32` values across annotated bindings, binary expressions, function parameters, function returns, and class fields.
- Updated the checker so floating-point literals adopt an expected float type when the surrounding context provides one.
- Extended numeric type checking so `float32` participates in the existing arithmetic and comparison rules alongside `float64`.
- Updated class constructor checking to validate arguments with field-type expectations instead of treating all float literals as `float64`.
- Added a maintained example for `float32` values and synced the tutorials and example index.

## Verification

- `cargo test -p aurora-compiler --test fixtures`
- `cargo test`
- `npm run test:lsp`
- `npm run check:extension`
- `npm run test:extension`

## Follow-Up

- Decide whether Aurora should gain explicit numeric cast syntax in the bootstrap compiler.
- Decide whether `float32` should eventually grow its own builtin math methods or stay limited compared with `float64` in v1.
