# 2026-03-18 MIR-First Build Path

## Goal

Move the product-facing backend commands onto one shared backend path so `aura build` and `aura run-mir` both exercise the MIR-first runtime path before falling back to the interpreter.

## Work Completed

- added a failing unit test in `crates/aura/src/main.rs` to assert that the generated bootstrap runner uses `aurora_compiler::run_source_via_mir(...)`
- switched the generated `aura build` launcher from `run_source(...)` to `run_source_via_mir(...)`
- kept the existing CLI regression coverage for:
  - a native-MIR-backed build (`examples/point.au`)
  - a fallback-backed build (`examples/error_handling/try_result.au`)
  - native-MIR `run-mir`
  - fallback `run-mir`
- updated the repo, CLI, examples, tutorial, and task-board docs so they now describe the backend honestly as MIR-first with interpreter fallback, not as a reject-only MIR subset

## Verification

- `cargo test -p aura tests::bootstrap_runner_uses_mir_first_backend_path -- --exact`
- `cargo test -p aura --test cli build_produces_a_runnable_binary -- --exact`
- `cargo test -p aura --test cli build_handles_backend_fallback_examples -- --exact`
- `cargo test -p aura --test cli run_mir_executes_supported_programs -- --exact`
- `cargo test -p aura --test cli run_mir_falls_back_for_try_example -- --exact`

## Follow-up

- extend the MIR runtime so `try`, `with`, `spawn`, `select`, and the concurrency/runtime surface stop using backend fallback
- add broader product-level backend smoke coverage if `aura build` grows more flags or profiles
- replace the bootstrap launcher approach with real MIR-native code generation
