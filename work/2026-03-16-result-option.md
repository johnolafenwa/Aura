# 2026-03-16 Result And Option

## Goal

Add the next foundational v1 data-model slice after enums and `match`:

- built-in `Result[T, E]`
- built-in `Option[T]`
- construction and exhaustive matching

## Work Completed

- Added failing compiler fixtures first for:
  - `Result.Ok(...)`
  - `Result.Err(...)`
  - `Option.Some(...)`
  - `Option.None`
  - exhaustive `match` over built-in generic enums
- Extended semantic checking for:
  - `Result[T, E]` and `Option[T]` type positions
  - expected-type-aware constructor checking
  - payload mismatch diagnostics
  - exhaustive `match` over built-in generic enums
- Extended the interpreter for:
  - runtime construction of built-in `Result` and `Option` variants
  - matching those values through the existing enum runtime path
- Added the maintained example:
  - `examples/enums/result_option.au`
- Added the tutorial chapter:
  - `tutorials/09-results-and-options.md`
- Extended the language-server analysis to understand:
  - built-in `Result` and `Option` names
  - built-in variant completions
  - hover and diagnostics around those variants

## Verification

- `cargo test`
- `cargo run -p aura -- run examples/enums/result_option.au`
- `npm run test:lsp`
- `npm run check:extension`
- `npm run test:extension`

## Follow-up

- `try expr` is still not implemented.
- User-defined generic enums are still not implemented.
- The current bootstrap relies on expected type context for generic built-in constructors rather than full generic inference.
