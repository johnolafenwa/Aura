# ADR-0009: Borrowed-return containment

- Status: Superseded in part by ADR-0022
- Date: 2026-07-13
- Roadmap decision: D9

> **Superseded syntax (2026-07-27).** ADR-0022 removes the borrowed-return
> syntax this ADR introduced: `-> borrow T`, `-> borrow mut T`, and every
> `[label]` form no longer parse, and writing one reports `borrowed returns
> were removed; return an owned value instead`. Copy-valued borrowed returns
> become ordinary owned returns.
>
> The *containment semantics* below survive unchanged, now enforced by the
> ordinary move rules: exposing non-copy internal state still requires a clone,
> an index, a handle, or an owner operation. Returning a non-copy field of a
> shared parameter is rejected as a move out of a shared value.
>
> The reserved Phase-6 live-alias contract is accepted as lost. Batch 5's alias
> milestone is re-scoped to a designed-from-scratch loan/view proposal with
> explicit place identity, lifetimes, and escape rules, reviewed at that
> batch's checkpoint. It is not a revival of `borrow[label]`.

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
- `examples/basics/copy_field_returns.au`, ownership tutorials, Manual, and conformance status.
