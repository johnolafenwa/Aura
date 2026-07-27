# 2026-03-27 Spec Surface Alignment

## Goal

Verify Gemini's review against the current Aurora proposal and bring the maintained compiler/tooling surface into line with the proposal where the feature was already part of the frozen v1 design.

## Work Completed

- Added failing compiler fixtures for:
  - `copy class`
  - `indirect Node?`
  - `str`
  - `match ...`
  - unqualified enum variants in match arms
  - `for` iteration over `Channel[T]`
  - f-strings
  - explicit generic constructor specialization such as `Box[int32](...)`
- Implemented parser, AST, checker, interpreter, MIR, and MIR runtime support for the proposal features above.
- Made `copy` contextual so `copy class Name:` works without breaking existing identifiers like `copy = text.clone()`.
- Added `str -> String` aliasing in the implemented string-view path used by `str` parameters.
- Added channel iteration through ordinary `for value in jobs:` loops.
- Added borrowed `match` plus unqualified variant patterns such as `case Ok(value):`.
- Added compiler acceptance for explicit constructor specialization like `Box[int32](...)` and `Channel[int32](capacity=4)`.
- Added runnable maintained examples for the new surface under `examples/`.
- Updated the tutorial chapters and current-surface reference so they teach the implemented subset accurately.
- Added LSP fallback regression coverage so the editor no longer reports stale false diagnostics for f-strings, `copy class`, borrowed matches, or channel iteration.
- Fixed MIR runtime matching for unqualified enum variants so MIR execution stays aligned with interpreter behavior.

## Verification

- `cargo test -q`
- `npm run test:lsp`
- `npm run check:extension`
- `npm run ci`
- `cargo run -q -p aura -- run examples/strings/f_strings.au`
- `cargo run -q -p aura -- run examples/strings/borrow_str.au`
- `cargo run -q -p aura -- run examples/enums/match_borrow.au`
- `cargo run -q -p aura -- run examples/classes/indirect_recursive.au`
- `cargo run -q -p aura -- run examples/concurrency/channel_iteration.au`
- `cargo run -q -p aura -- run examples/generics/generic_constructor_specialization.au`

## Follow-up

- Context-sensitive bare `None` lowering in every MIR expected-type position still deserves a cleaner dedicated implementation instead of relying on explicit `Option.None` in the recursive-class example.
- The remaining known gaps are the proposal items still listed in `tutorials/13-current-language-surface.md`, not the Gemini parser-front-end items from this pass.
