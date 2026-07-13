# ADR-0006: Parameter and loop ownership defaults

- Status: Accepted
- Date: 2026-07-13
- Roadmap decision: D6

## Decision

Unannotated non-copy parameters are shared borrows; `own T` requests
consumption. `for x in xs` iterates by shared borrow, `for x in own xs`
consumes, and `for x in borrow mut xs` mutates. `match` continues to consume
by default, while `match borrow` opts into borrowing. Local non-copy
assignment remains a move. The `own` spelling must pass a keyword-collision
audit before implementation.

## Completion tests

- Parser/semantic ownership unit tests and exact `own` diagnostic fixtures.
- Check/run fixtures for parameters, loops, match, Queue iteration, and local moves.
- Forced MIR/direct parity plus maintained ownership examples and tutorials.
- LSP syntax, completion, hover, and diagnostic fixtures.
