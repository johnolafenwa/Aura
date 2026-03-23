# 2026-03-16 Builtin Named Arguments

## Goal

Extend Aurora's named-argument support beyond ordinary functions and methods so supported builtins follow the same call model when they expose real parameter names.

## Work Completed

- Added a shared call-binding layer in `crates/aurora-compiler/src/call.rs` for:
  - common positional/named argument ordering
  - builtin function signature metadata
  - builtin member signature metadata
- Moved compiler checking and runtime dispatch for supported builtins onto that shared binding path in:
  - `crates/aurora-compiler/src/sema.rs`
  - `crates/aurora-compiler/src/interpreter.rs`
- Synced compiler-owned analysis metadata and completions in `crates/aurora-compiler/src/analysis.rs`.
- Updated the JS LSP fallback metadata in `tools/aurora-language-server/src/analysis.js`.
- Added compiler fixtures for:
  - successful builtin named-argument calls
  - named `after(duration=...)` in `select`
  - unknown builtin parameter failure
  - positional-after-named builtin failure
- Added regression tests for compiler analysis and JS LSP analysis around builtin named arguments.
- Added maintained examples:
  - `examples/basics/named_builtin_arguments.au`
  - `examples/concurrency/select_timeout_named.au`
- Updated tutorials and example indexes so the documented surface matches the implemented one.

## Supported Builtin Named Calls

- `print(value=...)`
- `range(stop=...)`
- `range(start=..., stop=...)`
- `after(duration=...)`
- `Channel.send(value=...)`

## Verification

- `cargo test`
- `cargo run -p aura -- run examples/basics/named_builtin_arguments.au`
- `cargo run -p aura -- run examples/concurrency/select_timeout_named.au`
- `npm run test:lsp`
- `npm run check:extension`
- `npm run test:extension`

## Follow-Up

- Keep narrowing the JS fallback so builtin signature metadata eventually comes from the compiler only.
- Decide whether additional bootstrap builtins should expose named arguments or stay zero-argument/positional by design.
