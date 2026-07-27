# ADR-0013: Callable sequencing and ownership

- Status: Accepted
- Date: 2026-07-13
- Roadmap decision: D13

> **Amended by ADR-0022 (2026-07-27).** Amended, not superseded. Future
> in-loan closure captures use the implicit bare shared capability; move-only
> `FnOnce` captures and every non-copy task-crossing capture remain explicit
> ownership transfers. The full capture design is settled in Batch 5.

## Decision

Aurora adds capture-free function values first, move-only `FnOnce` closures
next, and borrowed captures only after live-loan tracking. Captures crossing
task boundaries must satisfy transfer rules.

## Completion tests

- Parser/check/run fixtures for function types and indirect calls.
- MIR/direct callable ABI and dispatch tests.
- Closure capture, single-use, escape, and task-transfer fixtures.
- FFI callback tests only after callable ownership and ABI rules are complete.
