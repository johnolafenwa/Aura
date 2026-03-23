# 2026-03-16 Compiler Completions

## Summary

- Added compiler-owned completions via `aura complete`.
- Switched the language server to prefer compiler completions before falling back to the local JS analysis layer.
- Added compiler and bridge regression tests for completions.

## Compiler Work

- Added `complete_source()` in `crates/aurora-compiler/src/analysis.rs`.
- Added serializable `AnalysisCompletion` items.
- Added scope-at-position logic for:
  - function params
  - method receivers
  - local bindings introduced by assignment
  - `for` loop bindings
  - `with` bindings
  - `match` payload bindings
  - `select` arm bindings
- Added receiver extraction and expression parsing for member completion after `.`.
- Added completion coverage for:
  - keywords
  - user classes, enums, and functions
  - built-in enums and functions
  - class fields and methods
  - enum variants
  - built-in members like `sqrt`, channel methods, task methods, and task-group methods

## CLI Work

- Added `aura complete --line <n> --character <n> [--trigger .] <file.au>`.
- Added stdin support for completions:
  - `aura complete --line <n> --character <n> [--trigger .] --stdin <virtual-path>`

## LSP Work

- Added `completeWithCompiler()` to `tools/aurora-language-server/src/compiler_bridge.js`.
- Updated `tools/aurora-language-server/src/server.js` so `onCompletion` prefers compiler completions first and falls back only when the compiler path is unavailable or the current buffer cannot be analyzed.

## Verification

- `cargo test -p aurora-compiler`
- `cargo test -p aura`
- `cargo build -p aura`
- `cargo run -p aura -- complete --line 5 --character 11 --trigger . examples/point.au`
- `./target/debug/aura complete --line 5 --character 11 --trigger . examples/point.au`
- node bridge smoke test against `completeWithCompiler()`
- `npm run check:lsp`
- `npm run test:lsp`
- `npm run check:extension`
- `npm run test:extension`
