# ADR-0004: String semantics

- Status: Accepted
- Date: 2026-07-13
- Roadmap decision: D4

## Decision

`String.len()` counts Unicode scalar values in O(n); `byte_len()` is O(1).
Single-quoted and double-quoted strings share escape semantics. Aurora 0.1
does not support integer String indexing. Negative indexing is the policy for
`Vec` and future slices. `chars()`, `ord()`, `chr()`, and explicit-encoding
String/bytes conversion land with the control-plane surface.

## Completion tests

- Lexer escape/span tests and single-quoted parse/run fixtures.
- String builtin unit tests in MIR and native runtimes.
- Vec negative-index check/run fixtures on both forced backends.
- API/reference, tutorial, example, and LSP completion coverage.
