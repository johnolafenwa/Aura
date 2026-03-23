# 2026-03-18 MIR Runtime And `run-mir`

## Goal

Push the backend work past MIR-as-debug-output by adding a real MIR execution path for the currently straightforward subset of Aurora and exposing it through the CLI.

## Work Completed

- added a MIR runtime in `crates/aurora-compiler/src/mir_runtime.rs`
- extended MIR lowering so it now carries:
  - class method metadata
  - receiver metadata
  - associated-method lowering
  - constructor default-field expansion
  - receiver-place information for mutating method calls
- added `run_source_via_mir(...)` to the compiler public API
- added MIR parity tests for:
  - `examples/point.au`
  - `examples/classes/methods.au`
  - `examples/enums/result_match.au`
- added an explicit MIR support gate so unsupported constructs fail fast with a clear diagnostic instead of silently running with the wrong semantics
- added `aura run-mir <file.au>` to the CLI and documented the current boundary

## Verification

- `cargo test -p aurora-compiler mir_runtime_runs_`
- `cargo test -p aurora-compiler mir_runtime_rejects_try_until_supported`
- `cargo test -p aura --test cli run_mir_executes_supported_programs -- --exact`
- `cargo run -p aura -- run-mir examples/classes/methods.au`
- `cargo run -p aura -- run-mir examples/enums/result_match.au`
- `cargo run -p aura -- run-mir examples/error_handling/try_result.au`

## Follow-up

- extend MIR lowering/runtime to cover `try`, `with`, `spawn`, `select`, and the concurrency/resource builtins
- switch `aura build` from the current source-launcher path toward the MIR execution/backend path
- add broader MIR parity tests as more of the implemented surface becomes executable through MIR
