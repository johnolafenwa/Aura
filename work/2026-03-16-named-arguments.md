# 2026-03-16 Named Arguments

## Goal

Support named arguments for ordinary functions and methods so Aurora does not force class construction into a keyword style while leaving the rest of callable syntax positional-only.

## Work Completed

- Added compiler fixtures for:
  - successful named-argument calls
  - positional-after-named failure
  - unknown-parameter failure
- Added runtime and type-checker support for named arguments on:
  - top-level functions
  - associated methods
  - instance methods
  - `spawn` and `TaskGroup.spawn(...)` target-function arguments
- Kept class constructors keyword-oriented as before.
- Added a categorized example under `examples/basics/named_arguments.au`.
- Updated the function and class tutorials to document the new call rules.

## Implemented Call Rules

- Positional arguments must come before named arguments.
- Named arguments must match declared parameter names exactly.
- A parameter cannot be provided more than once.
- All parameters must still be provided because default parameter values are not implemented yet.

## Verification

- `cargo test`
- `cargo run -p aura -- run examples/basics/named_arguments.au`

## Notes

At the end of this pass, builtin helpers were still positional-only. That later changed in [2026-03-16 Builtin Named Arguments](./2026-03-16-builtin-named-arguments.md), which added named-call support for selected builtins like `print`, `range`, `after`, and `Channel.send()`.
