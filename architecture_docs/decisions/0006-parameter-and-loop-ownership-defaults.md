# ADR-0006: Parameter and loop ownership defaults

- Status: Accepted, amended
- Date: 2026-07-13
- Amended: 2026-07-14 (G2 Queue carve-out and G3/R4 default rules)
- Roadmap decision: D6

> **Amended by ADR-0022 (2026-07-27).** Two changes. First, the
> declaration-known copy snapshot is removed: a bare parameter is now shared
> for every type, including copy types (universal logical sharing). The ABI may
> still pass copied bits, but the source-level shared-loan and sequencing rules
> apply uniformly, which is what keeps passing declaration-stable for generic
> specializations and builtin signatures. Second, the spellings changed: shared
> is bare, mutable is `mut T` (was `borrow mut T`), and consuming remains
> `own T`. Loop iteration follows the same rule: bare is shared, `mut` is
> mutable with writeback, `own` consumes.

## Decision

An ordinary parameter written `value: T` resolves at its declaration:

- a copy `T` is passed by value
- a non-copy `T` is a shared borrow
- an unresolved type parameter `T` is a shared borrow, even if a later
  specialization substitutes a copy type

Consumption is explicit as `value: own T`. `value: borrow T` remains an
explicit shared-borrow spelling and `value: borrow mut T` remains an exclusive
mutable borrow. Resolution is declaration-stable: checking, trait matching,
MIR, native ABI selection, analysis, and editor signatures use the convention
chosen at the declaration rather than recomputing it after generic
substitution.

The spelling asymmetry is intentional: parameter ownership occupies the type position as `value: own T`, parallel to `value: borrow T`, while loop ownership prefixes the iterable as `for value in own values` because loops have no type position.

Moving a defaulted borrowed parameter is rejected with guidance to declare it
as `own T` or clone before consuming it. Storage, insertion, and transfer APIs
therefore carry explicit owned-parameter metadata. This includes collection
insertion, queue sends, fallback values that may be returned, class and enum
payload construction, task captures, and supervisor configuration retained for
restart.

## Default arguments

Shared-borrow and `own` parameters may have defaults. Every omitted default is
evaluated afresh at the call, and a temporary created for a shared default lives
through completion of that call. An owned default is consumed normally.

A `borrow mut` parameter may not have a default, for copy or non-copy types. Its
target would be a caller-invisible temporary, so every mutation would be a
guaranteed silent lost write. The diagnostic offers two exits: require the
caller to pass a mutable value, or take `own T` and return the result.

## Loop ownership

Bare place iteration over `Vec[T]` and `Set[T]` is shared:

```aurora
for value in values:
    inspect(value)
```

`for value in own values:` consumes the collection and yields owned elements.
`for value in borrow values:` is the explicit shared spelling, and
`for value in borrow mut values:` retains the existing mutable Vec writeback
behavior. Mutable Set iteration remains unsupported.

`match` deliberately remains consuming by default, with `match borrow` and
`match borrow mut` as opt-ins. Local non-copy assignment also remains a move.
Loops usually inspect a collection repeatedly; matches usually destructure to
take payloads. That different default is intentional.

Queue iteration is a carve-out. It is a receive operation, not place traversal:
each received item is already owned by the loop binding, and the queue handle is
a copy value. Consequently `own`, `borrow`, and `borrow mut` modifiers are all
rejected for Queue iteration. Use the bare `for value in queue:` form.

Range modifiers retain their established 0.1 behavior; rationalizing modifiers
on non-place iterables is a separate follow-up rather than part of this
ownership migration.

## Task capture

`TaskGroup.start` and `start_soon` own every non-copy capture before the child
runs. Capture ownership is distinct from the target function's parameter ABI:
a child may read a task-owned capture through a default/shared parameter or
consume it through an `own` parameter. Mutable-borrow targets are rejected
because their writes cannot target the caller's place across the task boundary.

## Rationale

Aurora's runtime already moves or retains reference-counted values at explicit
ownership boundaries. Making ordinary readers consume values produced frequent
use-after-move errors without buying first-class-reference performance. Shared
defaults match the common Python-shaped reading style while `own` keeps every
retention and transfer visible in signatures, diagnostics, API reference, and
editor tooling.

## Completion tests

- Parser and semantic tests for all four parameter and loop source modes.
- Exact diagnostics for moving a defaulted parameter and both categories of
  rejected mutable-borrow default.
- Declaration-stability tests for generic `T`, including copy specialization.
- Directional call-boundary fixtures for copy-typed shared/mutable borrows,
  owned non-copy arguments, later plain place reads, and source-ordered named
  arguments.
- Operator-trait fixtures applying declared receiver and right-operand passing
  modes, plus a legal copy-value snapshot and the distinct TaskGroup capture
  boundary.
- Check/run fixtures for parameters, Vec/Set loops, unchanged match/local moves,
  all three rejected Queue modifiers, and bare Queue receive ownership.
- Structural metadata checks for every retaining builtin and rendered `own`
  signatures in the API reference and LSP.
- TaskGroup capture tests for shared and owned targets plus mutable rejection.
- Forced MIR/direct parity and maintained examples/tutorials.
