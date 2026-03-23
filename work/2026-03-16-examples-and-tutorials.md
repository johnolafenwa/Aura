# 2026-03-16 Examples And Tutorials Pass

## Goal

Build out a more comprehensive example library for the implemented subset of Aurora and add a tutorial track that can evolve alongside the language.

## Work Completed

- Added categorized examples under:
  - `examples/basics/`
  - `examples/classes/`
  - `examples/control_flow/`
  - `examples/numbers/`
  - `examples/strings/`
- Kept the original top-level bootstrap examples in place as stable references.
- Added runnable coverage for:
  - top-level scripts
  - omitted `None` return types
  - mutable bindings and reassignment
  - classes and member access
  - field defaults
  - `if` / `elif` / `else`
  - `while`, `break`, and `continue`
  - strings, comparisons, and concatenation
  - `float64.sqrt()`
- Added compiler tests that type-check and run the categorized example set.
- Fixed the semantic checker to accept `String + String`, matching the already-supported runtime behavior and the new example coverage.
- Added a `tutorials/` directory with a book-style Markdown sequence covering the implemented fundamentals.
- Documented the rule that tutorials and examples should be updated alongside compiler changes.

## Verification

- `cargo test`
  - passed
- selected categorized examples run under `aura run`
  - passed

## Notes

The tutorials intentionally stop at the implemented compiler boundary. They do not try to teach proposal-only features that are not yet executable in the repo.
