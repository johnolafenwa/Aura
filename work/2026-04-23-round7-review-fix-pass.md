# April 23 Round 7 Review Fix Pass

## Session

- Start time: 2026-04-23 21:24:13 BST
- Stop time: 2026-04-23 22:00:57 BST
- Total elapsed: 0h 37m
- Stop rule: Complete the Round 7 review fix pass or reach 12 continuous hours.

## Goal

Validate Claude's Round 7 report, fix confirmed defects across the supported Aurora API, and clean up slop introduced or exposed by the pass.

## Work Completed

- Fixed direct-backend cleanup parity by emitting pending `with` cleanups before generated runtime trap paths for division/int32 overflow, vector indexed read/write failures, and unresolved dynamic member dispatch.
- Made direct recursion-limit failures use source-rendered diagnostics with the Aurora function name instead of backend-specific wording.
- Closed the queue-iteration clean-return wakeup case by tracking task-group completion wake flags and treating an empty queue as closed once all registered producers finish cleanly.
- Accepted `{}` for an annotated `Set[T]` target and preserved the Set element type through MIR lowering.
- Improved the borrowed-`self` match diagnostic to suggest `match borrow self.field:`.
- Added streamed stdout support for `aura run` so printed output reaches the process pipe before external termination.
- Added raw `process.Completed.stdout_bytes()` and `stderr_bytes()` across sema, MIR runtime, native runtime/codegen, compiler analysis, LSP fallback metadata, tests, examples, and tutorials.

## Verification

- `CARGO_TARGET_DIR=/tmp/aurora-round7-target cargo check -p aurora-compiler -p aura`
- `CARGO_TARGET_DIR=/tmp/aurora-round7-target cargo test -p aurora-compiler --test fixtures -- --nocapture`
- `CARGO_TARGET_DIR=/tmp/aurora-round7-target cargo test -p aurora-compiler --lib -- --nocapture`
- `CARGO_TARGET_DIR=/tmp/aurora-round7-target cargo test -p aura --test cli -- --nocapture --test-threads=1`
- `npm --prefix tools/aurora-language-server test`
- `CARGO_TARGET_DIR=/tmp/aurora-round7-target cargo clippy -p aurora-compiler -p aura -- -D clippy::correctness`
- `git diff --check`

## Follow-up

- Clippy still reports existing non-correctness warnings across older code paths; this pass kept the existing `-D clippy::correctness` gate green.
