# aurora-compiler

This crate contains the Aurora compiler bootstrap library.

## Testing Approach

This crate should be developed test-first.

The intended layers are:

- unit tests for small compiler helpers
- fixture tests for parse/check/run/diagnostic behavior
- machine-readable analysis tests for diagnostics, symbols, hover, and definition data
- example smoke tests for maintained `.au` programs
- MIR structure tests for backend staging

## Fixture Categories

Fixture tests live under:

- `tests/fixtures/parse-pass`
- `tests/fixtures/parse-fail`
- `tests/fixtures/check-pass`
- `tests/fixtures/check-fail`
- `tests/fixtures/run-pass`

When adding a new language feature, prefer starting with a failing fixture in one of those directories.

## Verification

From the repo root:

```bash
RUST_MIN_STACK=33554432 cargo test
npm run coverage:compiler
```

That runs:

- the compiler crate unit tests
- the maintained example smoke tests
- the fixture-based compiler tests
- the CLI product tests that exercise compiler behavior through `aura`

The coverage command uses `cargo-llvm-cov` to measure the current compiler production-code baseline. It runs the workspace tests, excludes `crates/aura/**` from the report, and ignores extracted `src/*_tests.rs` helper modules.

## Coverage Direction

The compiler crate is moving toward a stricter coverage policy, but the immediate priority is behavior-first fixture coverage for the implemented language subset.

For this package, exact parse/check/run/diagnostic regression cases are more valuable than chasing line coverage mechanically.

### Coverage Prerequisites

Compiler coverage depends on:

- `cargo-llvm-cov`
- the Rust `llvm-tools-preview` component

Install them with:

```bash
cargo install cargo-llvm-cov
rustup component add llvm-tools-preview
```
