# 2026-03-18 Boolean Logic, Moves, And Runtime Safety

## Goal

Close the high-severity review findings around boolean parsing, use-after-move checking, and unsafe runtime arithmetic behavior, then sync the maintained repo surface.

## Work Completed

- Added parser, checker, interpreter, MIR lowering, and MIR runtime support for `and`, `or`, `not`, and unary minus.
- Added checker-level use-after-move diagnostics for the straight-line move cases covered by the new regression fixtures.
- Added early semantic validation for `main` so both `run` and `run-mir` reject parameterized `main` consistently.
- Replaced raw divide-by-zero and integer-overflow panics with Aurora diagnostics in both execution paths.
- Added runtime enforcement for annotated fixed-width integer bindings and assignments.
- Added fixture coverage for parse/check/run pass and check/run fail cases covering boolean logic, unary minus, use-after-move, division by zero, integer overflow, and invalid `main`.
- Added maintained examples for boolean logic and unary minus.
- Updated tutorials, examples, README content, language-server fallback keywords, and VS Code syntax highlighting to match the implemented surface.

## Verification

- `cargo test -p aurora-compiler --test fixtures`
- `cargo test`
- `npm run test:lsp`
- `npm run check:extension`
- `npm run test:extension`

## Follow-Up

- Extend move-checking beyond straight-line cases into more precise branch and loop flow analysis.
- Decide whether Aurora should eventually support implicit numeric widening or explicit cast syntax.
- Keep narrowing the remaining JS fallback in the language server now that compiler-backed analysis owns the main semantic path.
