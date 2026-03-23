# 2026-03-16 Compiler Analysis And LSP Bridge

## Summary

- Added machine-readable compiler analysis output for diagnostics, symbols, hover, and definition data.
- Added `aura analyze` and `aura ast-json`, both usable from file paths or `--stdin`.
- Switched the Aurora language server to prefer compiler-owned analysis for diagnostics, document symbols, hover, and go-to-definition.
- Kept the local JS analysis layer as fallback and as the current completion engine.

## Compiler Work

- Added `crates/aurora-compiler/src/analysis.rs`.
- Added `analyze_source()` to the compiler library.
- Added serializable analysis structs for diagnostics, symbols, occurrences, and definition ranges.
- Added compiler tests covering:
  - valid example analysis
  - diagnostic JSON on invalid code
  - select-timer analysis for `after(5ms)`
- Added `serde` support for AST and span types so the CLI can emit machine-readable JSON.

## CLI Work

- Added `aura ast-json <file.au>`.
- Added `aura analyze <file.au>`.
- Added `--stdin <virtual-path>` support so editor buffers can be analyzed without saving to disk first.

## LSP Work

- Added `tools/aurora-language-server/src/compiler_bridge.js`.
- The language server now resolves compiler commands in this order:
  - `AURORA_LSP_AURA_PATH`
  - `target/debug/aura`
  - `target/release/aura`
  - `cargo run -q -p aura --`
  - `aura` on `PATH`
- The server caches compiler analysis per document version.
- Diagnostics, document symbols, hover, and definition now come from the compiler when available.
- Completions still come from the local JS analysis layer.

## Verification

- `cargo test -p aurora-compiler`
- `cargo test -p aura`
- `cargo build -p aura`
- `cargo run -p aura -- analyze examples/point.au`
- `printf 'def main():\n    print(total)\n' | ./target/debug/aura analyze --stdin /virtual/demo.au`
- `npm --prefix tools/aurora-language-server run check`
- `node` smoke test for `tools/aurora-language-server/src/compiler_bridge.js`
- `npm run check:extension`
- `npm run test:extension`
