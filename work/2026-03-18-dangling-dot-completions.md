# 2026-03-18 Dangling Dot Completions

## Goal

Fix compiler-backed completions for the common editor state where a user has typed a receiver and a trailing dot, but has not typed the member name yet.

## Work Completed

- Updated `crates/aurora-compiler/src/analysis.rs` so `complete_source(..., trigger='.')` falls back to a sanitized parse path when the current buffer fails only because of a dangling member access like `counter.`.
- Added a compiler regression test covering a buffer with:
  - a valid class declaration
  - a valid local receiver binding
  - a trailing `counter.` line
- Verified the CLI path directly with:
  - `aura complete --line ... --character ... --trigger . --stdin ...`
- Updated the CLI/tutorial docs so they now describe:
  - zero-based positions
  - the `--trigger .` expectation
  - support for the common dangling-dot editor state

## Verification

- `cargo test`
- `printf 'class Counter:\n    value: int32\n\ndef main() -> int32:\n    counter = Counter(value=1)\n    counter.\n    return 0\n' | cargo run -q -p aura -- complete --line 5 --character 12 --trigger . --stdin /virtual/counter.au`
- `npm run test:lsp`

## Follow-Up

- The compiler-backed completion path is still intentionally stricter than a full incremental parser. If more incomplete-buffer shapes become common, add targeted sanitization and regression coverage rather than relying on ad hoc editor fallback.
