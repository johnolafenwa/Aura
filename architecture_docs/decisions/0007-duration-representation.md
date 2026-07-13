# ADR-0007: Duration representation

- Status: Accepted
- Date: 2026-07-13
- Roadmap decision: D7

## Decision

`Duration` preserves its signed 128-bit language representation. Direct code
passes two 64-bit limbs to the native runtime. Host timer conversion limits
are distinct runtime/API errors. Arithmetic, ordering, and integer
constructors land with computed backoff support.

## Completion tests

- Literal boundary tests in lexer/checker, MIR, native codegen, and native runtime units.
- Forced-backend run fixtures at and beyond `i64::MAX` and near the signed-128-bit limit.
- Separate timer-conversion failure tests and Duration API documentation.
