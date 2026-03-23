# 2026-03-18 CLI Broken Pipe And Analysis Recovery

## Goal

Address the remaining issues from broad evaluation:

- `aura ast ... | head` and `aura mir ... | head` were panicking on broken pipe
- `aura analyze` returned only a parse diagnostic and no symbols/occurrences for dangling-dot editor buffers like `counter.`

## Work Completed

- Added compiler analysis recovery in `crates/aurora-compiler/src/analysis.rs` so:
  - `complete_source(..., trigger='.')` still supports dangling-dot buffers
  - `analyze_source(...)` now also recovers symbols and occurrences for the same buffer shape
  - the original parse diagnostic is preserved in the machine-readable output
- Added a compiler regression test covering machine-readable analysis on a dangling-dot buffer.
- Added CLI product tests in `crates/aura/tests/cli.rs` covering:
  - `ast` exiting cleanly when stdout is closed early
  - `mir` exiting cleanly when stdout is closed early
  - `analyze --stdin` returning symbols and occurrences for a dangling-dot buffer
- Reworked `crates/aura/src/main.rs` stdout writes to go through an explicit helper that:
  - writes and flushes without using panic-on-broken-pipe stdout macros
  - exits cleanly with status 0 on `BrokenPipe`
  - still exits non-zero for other stdout write failures
- Updated README/tutorial wording so the CLI behavior around `complete` reflects the implemented tolerance for dangling-dot buffers.

## Verification

- `cargo test -p aurora-compiler analysis::tests::machine_readable_analysis_recovers_symbols_for_dangling_dot_buffers`
- `cargo test -p aura --test cli`
- `cargo test`
- `npm run test:lsp`
- `printf 'class Counter:\n    value: int32\n\ndef main() -> int32:\n    counter = Counter(value=1)\n    counter.\n    return 0\n' | cargo run -q -p aura -- complete --line 5 --character 12 --trigger . --stdin /virtual/counter.au`

## Follow-Up

- If more incomplete-buffer shapes become common, extend the same targeted recovery approach with explicit regression tests rather than broad parser heuristics.
