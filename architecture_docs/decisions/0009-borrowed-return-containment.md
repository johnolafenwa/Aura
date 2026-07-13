# ADR-0009: Borrowed-return containment

- Status: Accepted
- Date: 2026-07-13
- Roadmap decision: D9

## Decision

Aurora 0.1 rejects calls producing non-copy borrowed results. The diagnostic
recommends returning a clone or exposing an owner method. Borrowed returns of
copy types may materialize copies. Live aliases wait for typed places,
addressable runtime storage, explicit MIR borrow operations, and loan
tracking in Phase 6.

## Completion tests

- Semantic check-fail fixtures for shared/mutable non-copy borrowed calls and exact hints.
- Check/run fixtures preserving copy borrowed-return behavior.
- Escape/container/task cases and forced-backend product tests.
- `examples/basics/borrowed_returns.au`, ownership tutorials, Manual, and conformance status.
