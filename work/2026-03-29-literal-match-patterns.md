# 2026-03-29: Literal Match Patterns

## Goal

Extend statement-form `match` beyond enum-only patterns so normal control-flow code can branch directly on `bool`, integer, and `String` literals.

## Work Completed

- Added parser and AST support for literal `case` patterns over booleans, integers, strings, and negative integer literals.
- Extended semantic checking so `match` now accepts enum scrutinees as before plus `bool`, integer, and `String` scrutinees.
- Preserved exhaustiveness rules by allowing bare two-arm boolean matches over `true` and `false`, while requiring a final wildcard arm for open-ended integer and string literal matches.
- Added duplicate-literal diagnostics and type/bounds validation for literal patterns against the scrutinee type.
- Extended interpreter execution for literal patterns and lowered non-enum literal matches through ordinary MIR comparison branches so `run-mir` and native builds stay aligned without a separate pattern runtime.
- Added compiler fixtures, a maintained control-flow example, CLI smoke coverage for `run-mir` and native build paths, and tutorial/README updates.

## Verification

- `cargo fmt --all`
- `cargo test -p aurora-compiler`
- `cargo test -p aura`
- `npm run test:lsp`
- `npm run check:extension`

## Follow-Up

- Literal pattern support is currently limited to `bool`, integer, and `String`.
- Floating-point literal patterns and nested structural patterns remain outside the bootstrap surface.
