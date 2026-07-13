# ADR-0011: Typed errors and assertions

- Status: Accepted
- Date: 2026-07-13
- Roadmap decision: D11

## Decision

Recoverable errors remain typed `Result` values; Aurora does not add
exceptions for surface familiarity. Phase 3 adds `assert expression,
"message"`, which traps with the supplied message.

## Completion tests

- Existing Result/`try` checker and runtime fixtures remain the error contract.
- Parser/check/run fixtures for assertion success, failure, message, and source span.
- MIR/direct trap parity, CLI rendering, test-runner, and LSP coverage.
