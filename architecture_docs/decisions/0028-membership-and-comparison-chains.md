# ADR-0028: Membership operators and comparison chains

- Status: Accepted
- Date: 2026-07-25
- Roadmap decision: Batch 2, Phase 3.5 Python expression kernel

## Context

Batch 2 requires `in` / `not in` over the maintained containers and
Python-identical chained comparisons. The roadmap fixes the source shapes and
the delegation targets but leaves precedence, container coverage, evaluation
order, ownership, and the treatment of short-circuited operands to the
provisional-decision protocol.

Before this decision, both forms were rejected with the `AU2005` migration
hints `python_in`, `python_chained_comparison`, `python_chained_equality`,
`python_mixed_comparison_equality_first`, and
`python_mixed_comparison_ordering_first`.

## Decision

- Equality, ordering, and membership occupy one precedence level, below prefix
  `not` and above `+`/`-`. `not a == b` is still `not (a == b)`, while
  `a not in b` is one comparison operator.
- That level chains rather than left-folding. `a < b <= c` is one chain of two
  links over three operands, not a comparison whose left operand is a
  comparison.
- A chain of `n` operators means the conjunction of its `n` adjacent
  comparisons, with one difference from the spelled-out conjunction: every
  operand expression is evaluated at most once. Operands are evaluated left to
  right, and the first `false` link short-circuits the rest.
- `in` and `not in` are total over four containers and rejected elsewhere:
  `Vec[T]` and `Set[T]` test element membership through `contains`,
  `Map[K, V]` tests key membership through `contains_key`, and `String` tests
  substring containment through `contains`. `not in` is exactly the negation of
  `in`, never a separate member.
- The container establishes the value's expected type, so an unsuffixed numeric
  literal may adopt the element or key type. An unsupported container reports
  `AU2003`; a value of the wrong type reports `AU2002`.
- Membership reads both operands and moves neither, because the members it
  delegates to take a shared borrow of the receiver and a shared borrow of the
  value. The value is evaluated before the container, matching source order.
- Each chain operand is checked as if it were evaluated, even where
  short-circuiting would skip it, so a chain reports an ownership conflict that
  only one runtime path would reach. This is the conservative rule the other
  branching forms already use.
- A single comparison keeps its existing binary form, so only a genuine chain
  introduces new lowering. Lowering emits explicit branches and one typed join,
  shared by MIR execution and direct native code generation.

## Consequences

Range checks and containment tests read the way Python users expect while
Aurora's exact types are preserved: `1 == 1 == true` is a type error rather
than a truthiness coincidence, and `x in xs` requires `x` to have the
container's exact element type.

Evaluating each operand once is observable whenever an operand has side
effects, and it is the property that makes the chain worth having over the
spelled-out conjunction.

Conservative chain checking may reject an ownership pattern that one concrete
runtime path would preserve. Spell the comparison out with `and` when the
skipped evaluation is the point.

## Completion tests

- Parser tests pin one precedence level, chain link operators and spans, and
  the `Membership` shape for `in` and `not in`.
- Check fixtures pin the unsupported-container and wrong-value-type
  rejections; a check-pass fixture pins every accepted container and chain
  shape.
- The run fixture pins container coverage, `not in`, single evaluation of
  middle operands, and short-circuiting, through MIR and the forced direct
  parity matrix.
- The five retired `AU2005` fixtures remain in the python-hint family and now
  assert that the Python spelling type-checks.
- The maintained example, tutorial, normative Manual and Grammar sections,
  reference-integrity gate, and language-server bridge are updated in the same
  freeze-rule pass.

## Checkpoint

The Batch 3 entry checkpoint accepted the single precedence level, the four
supported containers, the at-most-once evaluation rule, and the conservative
treatment of short-circuited operands together; changing any one of them
changes observable source compatibility.
