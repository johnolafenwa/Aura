# 2026-03-18 MIR-Native `try`

## Goal

Remove `try expr` from the backend fallback surface by making the MIR runtime execute `try` with real early-return semantics.

## Work Completed

- added a failing compiler test that lowers `examples/error_handling/try_result.au` to MIR and runs it directly through `run_mir(...)`
- added `Rvalue::Try` to MIR so `try` is preserved explicitly instead of collapsing into a plain value during lowering
- implemented MIR-runtime `try` execution with:
  - `Result.Ok(payload)` unwrapping
  - `Result.Err(payload)` early return from the current function
  - runtime validation that the operand is actually a `Result`
- removed the old MIR support gate that rejected `try`
- renamed stale fallback-oriented tests and updated the backend docs/task board to reflect that `try` now runs natively through MIR

## Verification

- `cargo test -p aurora-compiler tests::mir_runtime_runs_try_example_natively -- --exact`
- `cargo test -p aura --test cli run_mir_executes_try_example -- --exact`

## Follow-up

- implement native MIR/runtime support for `with`
- implement native MIR/runtime support for `spawn`, `select`, and the concurrency/runtime surface
- keep reducing backend fallback cases until `run-mir` and `build` no longer need interpreter fallback
