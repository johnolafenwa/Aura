# ADR-0026: Minimal tuples

- Status: Provisional
- Date: 2026-07-24
- Roadmap decision: Phase 3.5 tuples

## Context

Aurora needs a small product type for returning and unpacking a fixed number of
heterogeneous values. The first tuple surface must fit Aurora's existing
copy/move and borrowed-match rules without silently introducing partial moves,
mutable projection writeback, a new collection protocol, or open-ended
indexing.

The Batch 2 tuple ticket therefore defines one deliberately narrow kernel. More
expressive tuple operations remain separate language decisions.

## Provisional Decision

- Tuple value expressions are parenthesized: `(left, right)` and the singleton
  `(value,)`. A comma is required, so `(value)` remains grouping.
- Tuple types use the same fixed-arity shape: `(T1, T2)` and `(T,)`.
- `indirect` does not apply to a tuple type. A recursive class link therefore
  cannot be hidden inside tuple storage and must remain a separately named
  `indirect` field.
- Empty `()` tuples are rejected. Tuple literals, tuple types, tuple targets,
  and tuple patterns accept exactly one trailing comma only for arity one.
  Multi-element trailing commas are rejected.
- Tuple values may be parameters, local values, class or enum payloads, and
  function results wherever their exact tuple type is accepted.
- Tuple construction evaluates and captures elements left to right.
- A tuple is copyable exactly when every element type is copyable. Otherwise
  it is a move value.
- Assignment and `for` binding targets may recursively unpack a value whose
  tuple shape and element types match exactly. Every leaf introduces a fresh
  name and cannot shadow a visible binding; member and index leaves are not
  tuple binding targets.
- Unpacking a copy tuple copies its elements. Unpacking a non-copy tuple
  consumes the whole source value once and gives owned leaf bindings. It does
  not create independently reusable partial source projections. Any later use
  of the source is a loud use-after-move diagnostic.
- A bare collection iteration keeps the collection and gives tuple leaves the
  same shared provenance as the yielded element. `own` collection iteration
  and Queue receive iteration give owned tuple leaves. Mutable-borrow
  iteration with a tuple target is rejected in this minimal surface; there is
  no recursive mutable tuple writeback.
- Tuple patterns are recursive and fixed-arity. A by-value `match` consumes a
  non-copy tuple scrutinee as one whole value and gives owned bindings.
  `match borrow` keeps the scrutinee and gives shared leaf provenance.
  `match borrow mut` with a tuple pattern is rejected; tuple-pattern mutation
  and reconstruction are not part of this decision.
- `tuple[INTEGER]` is supported only when the index is a non-negative integer
  literal known at compile time, is in bounds, and the selected element type
  is copyable. The operation returns a copy. Dynamic, negative, out-of-bounds,
  and non-copy-element tuple indexing are rejected. Unpack when ownership of a
  non-copy element is required.
- Tuple equality, ordering, iteration, methods, named elements, rest patterns,
  and implicit tuple/collection conversions are not introduced.

These choices remain Provisional pending the Batch 2 checkpoint review.

## Rationale

Whole-source moves give one easily explained ownership event and avoid
specifying disjoint partial-move paths for positional projections. Copy-only
constant indexing provides the common read case without a hidden clone or a
destructive read through a general index expression. Shared borrowed
destructuring composes with existing collection and match provenance, while
rejecting mutable tuple writeback prevents an implicit reconstruction protocol
from becoming part of Aurora 0.1 by accident.

## Completion Evidence

- Lexer/parser/AST tests and parse fixtures pin singleton and multi-element
  values and types, nested targets and patterns, comma boundaries, and
  constant-index syntax.
- Checker fixtures pin exact shape/type matching, recursive copy
  classification, whole-source moves, shared provenance, invalid indexing,
  and the mutable-writeback rejections.
- Run fixtures and CLI parity tests pin tuple returns, recursive assignment,
  `for` unpacking, tuple-pattern arms, evaluation order, and equal MIR/direct
  output.
- `examples/basics/tuples.au` and the executable block in
  `docs/manual/tuples.md` pin the maintained user-facing surface and exact
  output.
- The Manual, Current Limits, conformance map, executable-reference gate, and
  tutorial track document the same boundary.
