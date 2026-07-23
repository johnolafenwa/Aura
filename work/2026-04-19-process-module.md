## Goal

Add the first maintained Aurora `process` module as a narrow, explicit subprocess API:

- `process.start(...)`
- `process.run(...)`
- `process.inherit()`
- `process.null()`
- `process.pipe()`
- `process.Child`
- `process.Pipe`
- `process.ExitStatus`
- `process.Completed`
- `process.Error`
- timeout-aware child waiting plus explicit terminate/kill

This pass should stay shell-free and should not try to implement restart supervisors, PTYs, process groups, or higher-level orchestration helpers.

## Work Completed

- Added the first maintained shell-free `process` builtin module with:
  - `process.start(...)`
  - `process.run(...)`
  - `process.inherit()`
  - `process.null()`
  - `process.pipe()`
  - `process.Child`
  - `process.Pipe`
  - `process.ExitStatus`
  - `process.Completed`
  - `process.Wait`
  - `process.Error`
- Implemented the process surface across the checker, MIR runtime, direct runtime, native backend, compiler-owned analysis, and the LSP fallback analysis layer.
- Added timeout-aware child waiting plus explicit `terminate()`, `kill()`, and `close()` behavior, with `close()` performing the maintained structured-cleanup policy for subprocesses.
- Added pipe APIs for text and byte reads/writes, flush, and close.
- Added maintained compiler and CLI regressions for type checking, subprocess execution, stdio piping, and native direct-backend behavior.
- Added maintained example smoke coverage for:
  - `examples/io/process_run.au`
  - `examples/io/process_pipes.au`
- Updated the root README, CLI README, examples index, and tutorials so the documented surface matches the implementation.
- Fixed the builtin default-argument path for `process.start(...)` / `process.run(...)` so omitted stdio defaults resolve through qualified `process.null()`, `process.pipe()`, and `process.inherit()` calls.
- Extended compiler-backed completion metadata so process module values and resource methods complete correctly in the maintained editor path.

## Verification

- `cargo fmt --all`
- `cargo test -p aurora-compiler`
- `cargo test -p aura`
- `npm run test:lsp`
- `npm run check:extension`
- `cargo clippy -p aurora-compiler -p aura -- -D clippy::correctness`

## Follow-up

- Keep the ML systems roadmap focused on future work that is intentionally still out of scope for this maintained process surface: PTYs, process groups, restart supervisors, richer orchestration helpers, and broader host-interop layers.
