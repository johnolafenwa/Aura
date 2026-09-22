# aura-compiler

This crate is the Aura compiler bootstrap library.

Its type system includes transparent aliases, normalized unions, contextual
member injection, and exhaustive `Type as name` patterns. Shared, mutable, and
owned matches lower to common tag, loan, and take operations that both
execution backends consume. The compiler service and persisted semantic
artifacts use semantic interface schema 16. Aura has no builtin
`Option` type.

## Testing Approach

Develop this crate test-first. The tests come in layers:

- unit tests for small compiler helpers
- fixture tests for parse, check, run, and diagnostic behavior
- machine-readable analysis tests for diagnostics, symbols, hover, and definition data
- example smoke tests for maintained `.au` programs
- MIR structure tests for backend staging

MIR is the compiler's mid-level intermediate representation.

## Fixture Categories

Fixture tests live under:

- `tests/fixtures/parse-pass`
- `tests/fixtures/parse-fail`
- `tests/fixtures/check-pass`
- `tests/fixtures/check-fail`
- `tests/fixtures/run-pass`

Start a new language feature with a failing fixture in one of these
directories.

## Verification

From the repo root:

```bash
RUST_MIN_STACK=33554432 cargo test
npm run coverage:compiler
```

The test command runs:

- the compiler crate unit tests
- the maintained example smoke tests
- the fixture-based compiler tests
- the CLI product tests that exercise compiler behavior through `aura`

The coverage command uses `cargo-llvm-cov` to measure compiler production code.
It runs the workspace tests and excludes `crates/aura/**` from the report. It
also ignores the extracted `src/*_tests.rs` helper modules.

## Coverage Direction

The crate is moving toward a stricter coverage policy. For now, the priority is
fixture coverage of the behavior the language implements. Exact parse, check,
run, and diagnostic regression cases are worth more here than line coverage
for its own sake.

### Coverage Prerequisites

Compiler coverage needs:

- `cargo-llvm-cov`
- the Rust `llvm-tools-preview` component

Install them with:

```bash
cargo install cargo-llvm-cov
rustup component add llvm-tools-preview
```
