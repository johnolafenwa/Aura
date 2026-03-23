# 2026-03-18 Integer Literals And String Clone

## Goal

Address the highest-priority issues from external review:

- fixed-width integer annotations were not enforcing literal bounds
- `String.clone()` and `String.as_str()` were advertised in the current language surface even though neither was actually supported
- builtin method references like `ch.send` produced a misleading generic-type error

## Work Completed

- Added compiler fixtures for:
  - out-of-range `int32` literals
  - accepted `int64` literals when an `int64` annotation provides the expected type
  - `String.clone()` as a supported builtin string method
  - clearer diagnostics for builtin method references like `ch.send`
- Updated integer literal typing in `crates/aurora-compiler/src/sema.rs` so:
  - unannotated integer literals still default to `int32`
  - annotated/expected integer literals can adopt the expected integer type
  - out-of-range integer literals now fail with an explicit diagnostic
- Expanded binary integer support in the checker so fixed-width integer values can participate in arithmetic and comparisons without being pinned to `int32`.
- Added checker support for `String.clone()` and kept the runtime path aligned.
- Removed unsupported `String.as_str()` from:
  - compiler analysis metadata
  - JS LSP fallback completions
  - tutorials describing the current bootstrap surface
- Improved member-access diagnostics so `ch.send` reports that the method must be called with `(...)`.
- Added a maintained string example at `examples/strings/string_clone.au`.
- Added compiler and LSP regression tests to keep the string-method surface aligned.
- Clarified the documented bootstrap surface in:
  - `README.md`
  - `crates/aura/README.md`
  - `tutorials/01-running-programs.md`
  - `tutorials/04-control-flow.md`
  - `tutorials/05-classes-and-data.md`
  - `tutorials/08-enums-and-match.md`
  - `tutorials/13-current-language-surface.md`
  so the repo now explicitly documents:
  - `aura complete` zero-based positions and current parse-validity expectations
  - `range(stop)` plus `range(start, stop)`
  - keyword-only class constructors
  - current enum payload and `match` limits
  - unary minus as not yet implemented

## Verification

- `cargo test`
- `cargo test -p aurora-compiler --test fixtures`
- `cargo run -p aura -- run examples/strings/string_clone.au`
- `npm run test:lsp`

## Follow-Up

- Keep auditing the documented numeric type surface against the actual bootstrap implementation, especially around `float32` and larger unsigned integer behavior.
