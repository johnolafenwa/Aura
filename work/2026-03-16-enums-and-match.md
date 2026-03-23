# 2026-03-16 Enums And Match

## Goal

Implement the next language slice with test-first discipline:

- enum declarations
- variant construction
- exhaustive statement-form `match`
- editor/tooling support for the same surface

## Work Completed

- Added failing compiler fixtures first for parse, type-check, runtime, and diagnostic behavior.
- Extended the compiler AST, lexer, and parser for:
  - `enum`
  - `match`
  - `case`
  - unit variants
  - single-payload variants
- Extended semantic checking for:
  - enum type declarations
  - duplicate variant detection
  - unit variant access
  - payload variant construction
  - exhaustive `match`
  - payload bindings inside `case` arms
- Extended the interpreter for:
  - enum runtime values
  - variant rendering
  - `match` execution
- Kept MIR aligned with new AST nodes so `aura mir` still works on enum programs.
- Added the categorized example:
  - `examples/enums/result_match.au`
- Added the tutorial chapter:
  - `tutorials/08-enums-and-match.md`
- Updated the language-server analysis so VS Code now understands:
  - enum declarations and variants
  - completions after `Enum.`
  - enum/variant symbols
  - hover for variants
  - match payload bindings inside arm bodies

## Verification

- `cargo test`
- `cargo run -p aura -- run examples/enums/result_match.au`
- `npm run test:lsp`
- `npm run test:extension`
- `npm run check:extension`

## Follow-up

- Generic enums are still not implemented in the compiler.
- `Option[T]`, `Result[T, E]`, `try`, and `with` should build on top of this enum/match foundation.
- Coverage is still below the eventual enforced target on both the compiler and the LSP packages.
