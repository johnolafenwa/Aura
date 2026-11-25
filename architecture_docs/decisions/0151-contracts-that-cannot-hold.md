# ADR-0151: A contract that can never hold is reported

Status: Accepted.

## Context

A function is proved under its `requires` clauses and, for a method the
class guards, its class invariants. When no input meets them, such as
`requires x > 0 and x < 0`, every check in the function is proved for
nothing, and `aura verify` reported the function as proved.

## Decision

After the entry assumptions are made, and only when they added a fact, the
verifier asks the solver once whether the facts so far can hold
(`Goal::CanHold`, `ObligationKind::Satisfiable`). An answer of
unsatisfiable is reported as `AU5120` at the function, and the function
does not count as proved. A solver that cannot tell is not a finding. The
check is listed in a report only when it fails, so the check counts of
other programs do not change.

## Consequences

- One more solver query for each function with a `requires` clause or an
  assumed class invariant.
- A loop invariant that cannot hold needs no such check: it is proved on
  entry to the loop, which fails unless the loop is unreachable.
