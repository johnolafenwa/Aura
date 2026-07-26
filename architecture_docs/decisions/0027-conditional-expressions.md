# ADR-0027: Conditional expressions

- Status: Accepted
- Date: 2026-07-24
- Roadmap decision: Batch 2, Phase 3.5 Python expression kernel

## Context

Batch 2 requires the familiar Python form `value if condition else
alternative`, with both arms unified to one type. The roadmap fixes the source
shape but leaves precedence, evaluation order, contextual typing, and
ownership-flow details to the provisional-decision protocol.

## Decision

- The syntax is `value if condition else alternative`.
- It has lower precedence than `or` and associates to the right. The condition
  position parses an `or` expression; a conditional expression there requires
  grouping.
- The condition is evaluated first, exactly once, and must have static type
  `bool`. Aurora does not apply truthiness conversion.
- Exactly one value arm is evaluated. The unselected arm has no runtime effects
  and cannot trap.
- Both arms must produce one exact static type. Surrounding expected context
  flows into both arms. Without surrounding context, contextual literals may
  adopt the type established by the other arm, including recursively inside a
  tuple or generic container; bound values are never widened or converted
  implicitly.
- The checker analyzes both arms from the post-condition ownership state and
  conservatively merges their move and partial-move states. A value moved on
  either arm is unavailable after the expression, even when the condition is a
  literal.
- The surrounding use still controls ownership. Passing the result through a
  shared-borrow slot borrows the selected non-copy arm and leaves both source
  owners valid; assigning, returning, or passing the result to an `own` slot
  moves the selected arm at runtime and applies the conservative static merge.
- Lowering uses an explicit condition branch and one typed join value on both
  maintained backends.

## Consequences

The surface remains directly familiar to Python users while preserving
Aurora's exact boolean, type, and ownership rules. Condition-first evaluation
is explicit even though the selected true value appears first in source.
Conservative ownership merging may reject a later use that one concrete
runtime path would preserve; this is the same safety rule used for other
branching forms.

## Completion tests

- Parser tests pin lowest precedence, right associativity, and the required
  `else` diagnostic.
- Check fixtures pin exact-bool conditions, contextual arm typing, arm
  mismatch, and conditional moves.
- The run fixture pins condition-first evaluation, one-arm laziness, nesting,
  and grouping through MIR and the forced direct parity matrix.
- MIR and compiler-analysis tests pin the typed branch/join form, structural
  contextual inference, and complete operand traversal.
- The maintained example, tutorial, normative Manual, reference-integrity
  gate, and language-server bridge are updated in the same freeze-rule pass.

## Checkpoint

The Batch 3 entry checkpoint accepted the precedence, contextual-literal
behavior, and conservative ownership merge together; changing any one of them
changes observable source compatibility.
