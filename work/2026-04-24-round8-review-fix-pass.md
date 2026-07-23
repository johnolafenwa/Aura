# April 24 Round 8 Review Fix Pass

## Goal

Validate Claude's Round 8 report, fix confirmed defects across the supported Aurora API, and keep tests/docs/work tracking aligned with behavior changes.

## Work Completed

- Added failing-first CLI regressions for direct-backend cleanup when traps propagate from callees, direct-backend cleanup before recursion-limit diagnostics, short-form `Some` / `None` matching on `process.Completed.stdout_bytes().get(0)`, and zero-producer queue iteration.
- Added compiler fixture coverage for non-empty `{...}` Set literals under an expected `Set[T]` type, and updated an existing heterogeneous-set diagnostic fixture to use the maintained curly literal form.
- Extended direct native codegen with registered cleanup thunks for active `with` resources so runtime diagnostics raised below the current frame can unwind native resources before surfacing the trap.
- Added a direct-runtime cleanup registry keyed by lightweight task id, with recursion-depth reset while draining so recursion-limit cleanup can run user `close()` methods.
- Reused direct runtime close handling for runtime-backed resources and factored `TaskGroup` close logic for both normal close and native cleanup thunks.
- Changed queue iteration lowering to always use the task-group-aware receive helper, creating a hidden empty `TaskGroup` when no explicit group is active so zero-producer queues terminate instead of hanging.
- Registered `process.Completed` member return types in MIR lowering so `stdout_bytes()` and `stderr_bytes()` retain `Vec[uint8]` through `get(...)` and short-form `Option` matching.
- Parsed non-empty braced literals without `key: value` entries as Set literals, while preserving `{}` as the annotation-directed empty map/set literal and preserving `Set{...}` compatibility.
- Updated maintained set examples, tutorials, and LSP fallback inference/tests to lead with `{1, 2, 3}` set literals.

## Verification

- `cargo fmt --all`
- `CARGO_TARGET_DIR=/tmp/aurora-round8-target cargo check -p aurora-compiler`
- `CARGO_TARGET_DIR=/tmp/aurora-round8-target cargo test -p aurora-compiler --test fixtures -- --nocapture`
- `CARGO_TARGET_DIR=/tmp/aurora-round8-target cargo test -p aura --test cli -- --test-threads=1 --nocapture`
- `CARGO_TARGET_DIR=/tmp/aurora-round8-target cargo test -p aurora-compiler -- --nocapture`
- `npm run check --prefix tools/aurora-language-server`
- `npm test --prefix tools/aurora-language-server`
- `CARGO_TARGET_DIR=/tmp/aurora-round8-target cargo clippy -p aurora-compiler -p aura -- -D clippy::correctness`
- `cargo fmt --all --check`
- `git diff --check`

## Follow-up

- The compiler still reports existing non-correctness Clippy style/dead-code warnings; the correctness lint set is green.
