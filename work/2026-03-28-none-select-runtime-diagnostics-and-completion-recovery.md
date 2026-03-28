# 2026-03-28: None, Select, Runtime Diagnostics, And Completion Recovery

## Goal

Close the latest externally reported regressions that were still live in the maintained Aurora surface:

- bare `None` failing in `run-mir` and native builds
- closed-channel `select` parity mismatches between interpreter/MIR and built binaries
- compiler-backed recovery failing when a buffer contains multiple dangling member accesses
- built-binary arithmetic runtime diagnostics dropping file and span context

## Work Completed

- Added a maintained run-pass fixture for bare unit `None` values and `return None`.
- Added CLI regression coverage for:
  - direct/default builds of bare `None`
  - direct/default `select` timeout behavior over closed channels
  - file-backed `analyze` / `complete` recovery with multiple dangling member accesses plus imports
  - source-aware runtime diagnostics from built binaries
- Fixed MIR lowering so bare `None` becomes a real MIR unit operand instead of a synthetic local place.
- Fixed compiler-backed recovery so `analyze` and `complete` can iteratively patch multiple dangling member-access parse errors in one buffer.
- Fixed the direct backend `select` recv path to distinguish `empty`, `closed`, and `value` results, matching interpreter/MIR timeout behavior.
- Embedded program path/source metadata into direct-built binaries and restored source-aware arithmetic runtime diagnostics for native failures.
- Added a maintained `examples/basics/none_values.au` example and refreshed README/tutorial/task-board text for the new behavior.

## Verification

- `cargo fmt --all`
- `cargo test -p aurora-compiler`
- `cargo test -p aura`

## Follow-Up

- Native runtime diagnostics are now source-aware for arithmetic failures covered by direct codegen spans. Broader source-aware native diagnostics for other runtime helper paths would still be useful if more dynamic runtime failures become part of the maintained surface.
