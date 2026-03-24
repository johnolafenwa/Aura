# 2026-03-23: Free-Function Borrow Parameters

## Goal

Bring the compiler and tooling into line with the Aurora v1 proposal by supporting ordinary function parameters written with `borrow` and `borrow mut`, instead of limiting borrowing syntax to method receivers.

## Work Completed

- Added compiler fixtures first for successful borrowed free-function calls plus failing mutable-place and move-out cases.
- Extended the parser and AST so ordinary parameters carry a passing mode and accept both:
  - `value: borrow T`
  - `borrow value: T`
- Updated semantic checking so:
  - borrowed parameters do not consume caller-owned values
  - `borrow mut` parameters require a mutable place at the call site
  - moving a non-copy field out of a borrowed ordinary parameter is rejected
  - borrowed parameters may not have default values
- Updated the interpreter so `borrow mut` parameters write caller-visible mutations back to the original argument place.
- Updated MIR lowering and the MIR runtime so borrowed parameter semantics match the interpreter path, including mutable write-back.
- Added runtime guards so spawned functions still reject borrowed parameters rather than silently faking by-value task capture semantics.
- Updated the JS LSP fallback parser so borrowed free-function parameters still provide correct hover/completion/scope information when compiler-backed analysis is unavailable.
- Added a maintained example at `examples/basics/borrow_parameters.au`.
- Updated tutorials to document the implemented borrowed-parameter surface and removed stale “not implemented” notes.

## Verification

- `cargo test`
- `npm run test:lsp`

## Follow-Up

- Borrowed return types and explicit lifetime syntax are still outside the bootstrap compiler.
- The compiler still does not implement expression-level `borrow` / `borrow mut` forms from the full proposal.
