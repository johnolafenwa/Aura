# ADR-0013: Callable sequencing and ownership

- Status: Accepted
- Date: 2026-07-13
- Roadmap decision: D13

> **Amended by ADR-0022 (2026-07-27) and ADR-0037 (2026-07-30).** Amended,
> not superseded. ADR-0037 implements value-capturing expression closures:
> Copy values copy, non-Copy owned values move at creation, read-only closures
> are repeatable, and consuming closures are single-use. In-loan capture still
> waits for the separate loan/view design.

## Decision

Aurora adds capture-free function values first, by-value expression closures
next, and borrowed captures only after live-loan tracking. Captures crossing
task boundaries must satisfy transfer rules.

## Completion tests

- Parser/check/run fixtures for function types and indirect calls.
- MIR/direct callable ABI and dispatch tests.
- Closure capture, single-use, escape, and task-transfer fixtures.
- FFI callback tests only after callable ownership and ABI rules are complete.
