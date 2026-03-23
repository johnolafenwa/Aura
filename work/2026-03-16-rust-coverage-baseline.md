# 2026-03-16 Rust Coverage Baseline

## Goal

Add real coverage measurement for the main Aurora language implementation, not just the language server.

## Work Completed

- installed `cargo-llvm-cov`
- installed the Rust `llvm-tools-preview` component
- added a root coverage command:
  - `npm run coverage:compiler`
- documented compiler coverage in:
  - `crates/aurora-compiler/README.md`
  - `docs/testing_strategy.md`
  - `README.md`

## Current Compiler Coverage Baseline

Command:

```bash
npm run coverage:compiler
```

Current `aurora-compiler` baseline:

- regions: `77.22%`
- functions: `84.97%`
- lines: `72.64%`

Module-level highlights:

- `lib.rs`
  - strongest current coverage
- `parser.rs`
  - relatively high compared with the rest of the compiler
- `sema.rs`, `lexer.rs`, `interpreter.rs`, and `mir.rs`
  - still have substantial uncovered surface
- `ast.rs`
  - effectively uncovered as executable code because it is almost entirely data structure definitions

## Notes

This is the first honest compiler-side baseline. The repo can now track language coverage explicitly instead of relying only on behavior tests and intuition.
