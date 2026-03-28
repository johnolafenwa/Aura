# 2026-03-28 Specialized Trait Dispatch, Select Timeouts, And Partial Moves

## Goal

Close the latest language/runtime regression pass around timed `select`, specialized generic trait impl dispatch, trait-associated methods, built-in enum constructor typing, and partial field moves.

## Work Completed

- fixed module-path lookup in checker, interpreter, and MIR lowering so nested imported namespaces and transitive re-exports resolve through the importing module's scope
- fixed MIR/runtime parity for nested `for range` loops and mixed built-in enum specializations
- fixed `Option.None` inference for explicit type arguments and generic class static methods
- fixed specialized generic trait impl dispatch across `run`, `run-mir`, and native `build`
- fixed associated trait methods in `impl Trait for Type` blocks so `Type.method()` now works in all maintained execution paths
- fixed timed `select` loops so closed `recv()` arms do not starve `after(...)` timeout arms
- finished wiring partial field-move tracking and added a regression fixture for reusing a moved field
- added compiler fixtures, module regressions, and CLI product coverage for the new and repaired paths
- added maintained examples and tutorial/README updates for specialized trait dispatch and trait-associated methods

## Verification

- `cargo fmt --all`
- `cargo test -p aurora-compiler`
- `cargo test -p aura`

## Follow-Up

- keep expanding product coverage around direct-backend dynamic dispatch for more nested generic shapes
- keep narrowing remaining documentation gaps between the proposal text and the implemented bootstrap surface
