# 2026-03-28 Partial Moves And Select Timeout Regressions

## Goal

Close the remaining ownership/runtime regressions from the latest Aurora language sweep, with
maintained regression coverage and docs updates in the same pass.

## Work Completed

- Added and kept green compiler regressions for:
  - nested `for range` MIR break state
  - transitive module re-exports in MIR
  - namespace imports inside imported modules
  - built-in `Option.None` typing in explicit and generic-static contexts
  - mixed built-in enum specializations
  - multiple specialized trait-impl dispatch
  - trait-impl associated methods
  - closed-channel `select` with `after(...)`
  - partial field moves on owned values
- Hardened checker field-move tracking so non-copy fields moved out of owned values cannot be read
  again until they are reinitialized, while sibling fields remain accessible.
- Finished the runtime/lowering fixes for generic instance type recovery so specialized trait impls
  now dispatch correctly in both the interpreter and MIR runtime.
- Finished the associated-method lowering/runtime path for trait impl methods invoked as
  `Type.method()`.
- Updated the class and concurrency tutorials to document the maintained behavior around partial
  moves and timer-backed `select`.

## Verification

- `cargo test -q -p aurora-compiler --test fixtures -- --nocapture`
- `cargo test -q -p aurora-compiler --test modules -- --nocapture`
- `cargo run -q -p aura -- run crates/aurora-compiler/tests/fixtures/run-pass/multiple_specialized_trait_impl_dispatch.au`
- `cargo run -q -p aura -- run-mir crates/aurora-compiler/tests/fixtures/run-pass/multiple_specialized_trait_impl_dispatch.au`
- `cargo run -q -p aura -- run crates/aurora-compiler/tests/fixtures/run-pass/trait_impl_associated_method.au`
- `cargo run -q -p aura -- run-mir crates/aurora-compiler/tests/fixtures/run-pass/trait_impl_associated_method.au`

## Follow-Up

- The cancellation-order complaint from the external report was not reproducible as a correctness
  failure on the current tree; both runtimes now observe cancellation, but the first worker to
  report remains scheduler-dependent.
- Direct-backend parity for every new runtime regression should continue to be watched as more
  generic type information moves through the native path.
