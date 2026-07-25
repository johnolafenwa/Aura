# ADR-0029: `enumerate` and `zip` loop forms

- Status: Provisional
- Date: 2026-07-25
- Roadmap decision: Batch 2, Phase 3.5 Python expression kernel

## Context

Batch 2 requires `enumerate(xs)` and `zip(xs, ys)` "as compiler-known loop
forms over the bare-loop borrow default", with `enumerate` yielding
`(int64, item)` and `zip` stopping at the shorter sequence. The roadmap fixes
those two behaviors and the borrow default, and leaves the operand domain,
the status of the names outside a loop, shadowing, and the lowering strategy
to the provisional-decision protocol.

## Decision

- `enumerate` and `zip` are recognized only in the iterable position of a `for`
  statement. They are loop forms, not callable values: naming either one
  anywhere else reports `AU2005` and names the loop spelling. Aurora 0.1 gains
  no iterator protocol, no lazy pair sequence, and no first-class value for
  either name.
- A user declaration of either name shadows the loop form completely, so an
  existing `def zip(...)` keeps its ordinary call meaning at every call site.
- Both forms take positional operands only: `enumerate` exactly one and `zip`
  exactly two. Any other arity, or a named argument, reports `AU2004`.
- Each operand must be index-addressable, which in Aurora 0.1 means `Vec[T]` or
  `Set[T]`. A `Range` or `Queue[T]` operand reports `AU2002` and names the bare
  `for` form. Neither of those is index-addressable, and `range` already yields
  its own counter.
- Both iterate over the bare-loop borrow default. An ownership modifier on the
  loop reports `AU3002`; every operand is frozen for the whole loop the same
  way a bare collection loop freezes its iterable; and a non-copy element
  binding is a shared borrow that cannot be moved out.
- `enumerate` yields `(int64, element)` with positions counted from zero. The
  position is an `int64` even though the internal loop cursor is an `int32`,
  because the yielded value is program data rather than a bootstrap index.
- `zip` yields one pair per shared position and stops as soon as any operand
  has no value at that position, performing `min(len(first), len(second))`
  iterations. The longer sequence's tail is never observed.
- Both lower to one lockstep loop over the same position-indexed member the
  ordinary collection loop already uses, so the direct backend needs no new
  support and parity follows from a single dispatch path. The cursor advances
  immediately after the operands are read, which is what keeps `continue`
  correct.

## Consequences

The two most common Python loop shapes read the same way in Aurora, without
introducing an iterator protocol the language cannot yet type or an
allocation-shaped pair sequence.

Restricting operands to `Vec[T]` and `Set[T]` is the narrow, honest domain for
a position-indexed lowering. Extending either form to a lazy or user-defined
iterable is a later decision that depends on an iterator protocol, not a
change to this one.

Because both forms borrow, a loop that wants to consume its elements must use
the bare `for value in own values:` form and count positions itself. That is
the same trade the bare-loop borrow default already makes.

## Completion tests

- Focused compiler tests pin lockstep iteration over `Vec` and `Set`, `zip`
  stopping at the shorter operand, `break` and `continue`, and the six
  rejection categories.
- Ownership tests pin that operands stay frozen during the loop and that a
  non-copy element binding cannot be moved out.
- A shadowing test pins that a user `def zip(...)` keeps its call meaning.
- The run fixture and maintained example pin exact stdout through MIR and the
  forced direct parity matrix.
- The normative Statements and Grammar sections, the tutorial, the
  maintained-surface listings, the conformance map, and a verified
  reference-integrity block are updated in the same freeze-rule pass.

## Checkpoint

Review this Provisional decision at the Batch 2 checkpoint. Ratification should
confirm the loop-form-only status, the `Vec`/`Set` operand domain, the
`int64` position type, and the borrow default together; changing any one of
them changes observable source compatibility.
