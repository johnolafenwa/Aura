## Goal

Add a small maintained convenience layer over the explicit queue/task/process result enums so common Aurora code reads more linearly and less match-heavy, while keeping the explicit enum-based APIs for callers that need full state distinctions.

This pass should also update the maintained examples, tutorials, and editor/tooling metadata so the default documented style is more Pythonic.

## Work Completed

- Added a maintained convenience layer over the explicit queue/task/process wait enums:
  - `Queue.get_or_none(timeout=...) -> Option[T]`
  - `Queue.get_or(default, timeout=...) -> T`
  - `Task.result_or_none(timeout=...) -> Option[T]`
  - `Task.result_or(default, timeout=...) -> T`
  - `process.Child.wait_or_none(timeout=...) -> Result[Option[process.ExitStatus], process.Error]`
  - `process.Child.wait_ok(timeout=...) -> Result[process.ExitStatus, process.Error]`
  - `process.Completed.check() -> Result[None, process.Error]`
- Added failing-first compiler fixtures and process regressions covering the new queue/task/process helpers.
- Wired the new surface through the checker, MIR runtime, direct native runtime, direct backend codegen, compiler analysis, and fallback LSP analysis/completions.
- Reworked the maintained concurrency and process examples to prefer the linear helper style for ordinary code while keeping the explicit enum APIs available where the examples genuinely need the full state distinctions.
- Updated the maintained tutorial and README surface so the default documented queue/task/process style is now `get_or*`, `result_or*`, `wait_or_none`, `wait_ok`, and `check()`, with the explicit enum-returning methods documented as the lower-level forms.

## Verification

- `cargo fmt --all`
- `cargo test -p aurora-compiler`
- `cargo test -p aura`
- `npm run test:lsp`
- `npm run check:extension`
- `cargo clippy -p aurora-compiler -p aura -- -D clippy::correctness`

## Follow-up

- None.
