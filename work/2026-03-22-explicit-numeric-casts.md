# 2026-03-22 Explicit Numeric Casts

## Goal

Add a minimal explicit conversion surface so Aurora programs can convert between numeric primitive types without relying only on contextual typing.

## Work Completed

- Added parser and AST support for `expr as Type`.
- Restricted the bootstrap cast surface to builtin numeric target types so it does not conflict with existing `with ... as name:` syntax.
- Added checker support for numeric-only casts and compile-time validation of invalid non-numeric cast attempts.
- Added interpreter and MIR runtime support for numeric casts, including range-checked integer targets.
- Added compiler fixtures for parse/check/run pass and fail cases covering numeric casts.
- Added compiler-analysis and JS fallback support for cast result typing.
- Added a maintained example and updated the numeric tutorials and example index.

## Verification

- `cargo test -p aurora-compiler --test fixtures`
- `cargo test`
- `cargo run -p aura -- run examples/numbers/numeric_casts.au`
- `cargo run -p aura -- run-mir examples/numbers/numeric_casts.au`
- `npm run test:lsp`
- `npm run check:extension`
- `npm run test:extension`

## Follow-Up

- Decide whether Aurora should eventually support non-numeric casts or keep conversions numeric-only.
- Decide whether cast syntax should remain `expr as Type` permanently or be revisited when trait-based conversions land.
