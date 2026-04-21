## Goal

Review the latest April 21 follow-up fixes for regressions or remaining gaps in the checker, MIR lowering, and TLS runtime path.

## Session

- Start: 2026-04-21 13:40:52 BST
- Stop: 2026-04-21 13:50:00 BST
- Stop rule: Complete the work or reach 12 continuous hours.

## Work Completed

- Reviewed the current diffs in `sema.rs`, `mir.rs`, `runtime_value.rs`, and the new regression tests.
- Reproduced that builtin module enum constructors like `io.Error.NotFound` and `process.Error.NoCommand` still lower to bare `Error.*` values at runtime in both `aura run` and direct-built binaries.
- Confirmed the constructor identity mismatch is stronger than a display issue: a real `fs.read_to_string(...)` `io.Error.NotFound` does not compare equal to a user-constructed `io.Error.NotFound`, and `match err: case io.Error.NotFound:` is rejected while `case Error.NotFound:` matches.
- Inspected the non-Unix TLS accept helper introduced by the shared backlog refactor and confirmed it currently falls back to 50 ms scheduler/time-slice polling instead of waiting on listener/socket readiness.

## Verification

- `./target/debug/aura run` on repros for `io.Error.NotFound`, `process.Error.NoCommand`, and `fs.read_to_string(...) == io.Error.NotFound`
- `./target/debug/aura build --backend direct` on the same equality repro
- source inspection of `crates/aurora-compiler/src/mir.rs` and `crates/aurora-compiler/src/runtime_value.rs`

## Follow-up

- The next fix should thread `module_enum_type_name(...)` or the equivalent qualified builtin enum identity through MIR enum-constructor lowering so constructed builtin module enum values use `io.Error` / `process.Error` consistently.
- If the non-Unix TLS path should match the old readiness behavior more closely, the next step is a real listener/socket wait instead of the current 50 ms polling helper.
