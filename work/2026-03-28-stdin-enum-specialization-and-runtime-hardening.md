# 2026-03-28: Stdin, Enum Specialization, And Runtime Hardening

## Goal

Close the latest compiler/runtime review findings by fixing the remaining path-aware CLI gap, explicit built-in enum specialization, runtime generic trait dispatch contamination, imported return-type visibility, recursion failure behavior, and the last docs/example drift.

## Work Completed

- Fixed `aura mir --stdin` to use the same path-aware module loading path as `run-mir --stdin`.
- Added compiler fixtures for:
  - repeated trait-bounded generic dispatch across multiple concrete types
  - explicit built-in enum constructor specialization
  - f-string interpolation overflow spans
  - mutual recursive classes without `indirect`
  - friendly recursion-depth runtime failure
- Added a path-aware compiler regression test for imported functions that return module-local classes.
- Scoped the interpreter expression-type cache per function call so generic trait dispatch no longer reuses earlier concrete receiver types.
- Added runtime call-depth guards plus larger worker stacks so recursion fails with a diagnostic instead of aborting the process.
- Added built-in enum specialization handling for `Result[...]`, `Option[...]`, and `SendError[...]` through checking plus interpreter/MIR execution.
- Qualified exported module type surfaces so imported functions, classes, enums, traits, and trait impls preserve module-local type references correctly across module boundaries.
- Rejected mutual recursive class layouts that still require `indirect`.
- Fixed f-string interpolation span remapping so diagnostics point at the embedded expression.
- Removed the stray syntax-negative file from `examples/` and added a maintained runnable example for explicit built-in enum type arguments.
- Updated tutorials and the task board to match the implemented surface.

## Verification

- `cargo test -p aurora-compiler --test fixtures`
- `cargo test -p aurora-compiler imported_function_return_types_keep_members_visible_across_modules -- --nocapture`
- `cargo test -p aura mir_stdin_resolves_local_module_imports -- --nocapture`

## Follow-Up

- Add direct-backend coverage for richer dynamic trait dispatch examples if the examples/tutorial surface grows further in that direction.
- Keep reducing the remaining runtime stack footprint so the guarded recursion limit can move higher without needing oversized worker stacks.
