# ADR-0003: Default integer type

- Status: Accepted
- Date: 2026-07-13
- Roadmap decision: D3

## Decision

`int` aliases `int64`; unsuffixed integer literals default to `int64`.
Overflow remains checked and trapping. Direct-backend `int64` and `uint64`
unboxing is a hard prerequisite for the default flip.

## Completion tests

- Scalar ABI and code-generation tests in `native_codegen_tests.rs` and `native_runtime_tests.rs`.
- Type-inference and overflow fixtures under compiler `check-*` and `run-*` fixtures.
- Forced-backend parity plus compiler benchmarks guarding the unboxed path.
- Examples, tutorials, and Manual numeric contracts.
