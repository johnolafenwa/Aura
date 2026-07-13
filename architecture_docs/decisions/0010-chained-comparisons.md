# ADR-0010: Chained comparisons

- Status: Accepted
- Date: 2026-07-13
- Roadmap decision: D10

## Decision

Chained comparisons are rejected in 0.1 with a focused diagnostic. Phase 3.5
implements Python semantics, including evaluating each middle operand once.

## Completion tests

- Parser/check-fail fixtures for the temporary rejection and exact hint.
- Phase 3.5 run fixtures for ordering, equality mixtures, side effects, and short-circuiting.
- MIR/direct parity and LSP diagnostic coverage.
