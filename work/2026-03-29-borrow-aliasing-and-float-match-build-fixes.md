# 2026-03-29: Borrow Aliasing And Float Match Build Fixes

## Goal

Close the reported soundness gap around aliased borrowed call arguments and fix direct/default
native builds for `float64` returns from enum `match` arms with payload destructuring.

## Work Completed

- Added checker regression fixtures for:
  - two `borrow mut` parameters receiving the same mutable place
  - overlapping `borrow` plus `borrow mut` parameters receiving the same place
- Added checker regression fixtures for method calls where a borrowed receiver aliases another
  borrowed argument, covering both `borrow mut self` + `borrow` and `borrow mut self` +
  `borrow mut`.
- Updated semantic checking so borrowed call arguments are tracked by place path during argument
  validation, and any overlap is rejected when a mutable borrow participates, including the
  implicit `self` receiver in method calls.
- Added CLI regression coverage for direct and default native builds on a function returning
  `float64` from an enum `match`.
- Fixed MIR default-return lowering to use a type-appropriate placeholder operand for scalar
  returns, which prevents unreachable synthetic `match_end` blocks from forcing invalid
  `None -> float64` coercions during direct native code generation.
- Updated the maintained tutorials to state that overlapping borrowed call arguments are rejected
  when a `borrow mut` parameter is involved.

## Verification

- `cargo fmt --all`
- `cargo test -p aurora-compiler`
- `cargo test -p aura`
- `npm run test:lsp`
- `npm run check:extension`

## Follow-Up

- Collection types remain the larger language-surface gap from the latest review, but these fixes
  close the higher-priority correctness issues first.
