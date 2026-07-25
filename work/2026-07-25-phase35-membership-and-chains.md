# Phase 3.5 membership operators and comparison chains

## Goal

Implement the Batch 2 expression-kernel forms `value in container`,
`value not in container`, and Python-style chained comparisons as one
freeze-rule packet, and retire the five `AU2005` hints they replace.

## Test-First Evidence

- The parser oracle that pinned the old rejection —
  `chained_comparison_diagnostics_point_at_the_second_operator` — was replaced
  by one that pins the accepted shape: one precedence level, the operator kinds
  and spans of each chain link, and the `Membership` form of `in` and `not in`.
  It failed until the parser produced those nodes.
- The `python_in`, `python_chained_comparison`, `python_chained_equality`,
  `python_mixed_comparison_equality_first`, and
  `python_mixed_comparison_ordering_first` fixtures were converted to
  acceptance before the feature existed, so the hint family failed until the
  Python spelling type-checked.
- The CLI oracle that required the JSON migration diagnostic across `check`,
  `run`, and `build` was replaced by one that requires all three to accept the
  new forms and still reject an unsupported container.

## Work Completed

- Merged the equality and ordering parser levels into one Python-shaped
  comparison level that also accepts `in` and the two-token `not in`. A single
  comparison keeps its existing `Binary` form; a single membership test becomes
  `Membership`; two or more operators become one `CompareChain`.
- Typed membership from the container: `Vec[T]` and `Set[T]` compare elements,
  `Map[K, V]` compares keys, and `String` compares substrings. The container
  supplies the value's expected type through a speculative pass that leaves move
  state alone, so an unsuffixed literal adopts the element or key type while the
  value is still evaluated first, in source order.
- Typed chains by checking each adjacent pair under the existing comparison
  rules, with the same numeric-literal adoption a single comparison performs in
  both directions.
- Lowered membership to the builtin member the container supplies —
  `contains` or `contains_key` — so both backends reuse one dispatch path, and
  lowered `not in` as its negation rather than a separate member.
- Lowered chains to explicit branches over one typed join, evaluating each
  operand into a temporary exactly once and short-circuiting at the first false
  link. No direct-backend change was needed, because both forms lower to MIR
  shapes the backend already supports.
- Extended compiler analysis and the language-server bridge so every operand of
  both forms keeps hover and definition coverage.
- Added Provisional ADR-0028, the normative Manual and Grammar sections, the
  maintained `examples/control_flow/membership_and_chains.au` example with its
  index entry and smoke oracle, the control-flow tutorial sections, the
  maintained-surface listings, the conformance map, and two verified
  reference-integrity blocks.
- Retired the five hints to pass-through. The fixture family keeps every
  `.au` file and now asserts acceptance through a new `.accept` marker; the
  three mixed-type fixtures were rewritten to well-typed chains of the same
  shape, because Aurora's exact types make `1 == 1 == true` a type error rather
  than Python's truthiness coincidence.

## Verification

- Focused parser, semantic, and language-server tests pass, including operator
  spans, container coverage, the two rejection categories, single evaluation of
  middle operands, and short-circuiting.
- The nine-category fixture suite passes, including the two new check-fail
  oracles, the new check-pass fixture, the new run fixture, and the five
  retired hints now asserting acceptance.
- The 894-test compiler library suite and the 259-test CLI product suite pass.
- The run fixture's exact stdout is identical through MIR and a forced-direct
  binary, including the traced evaluation order that proves each middle operand
  runs once and that a false link skips the operands after it.
- `npm run check:reference` passes with the new verified Manual blocks, and the
  language-server coverage gate stays at enforced 100%.
- The complete `npm run ci` gate is green before the commit, with compiler
  coverage at 64,028/66,649 lines (96.06745787633723%), 4,145/4,281 functions
  (96.82317215603831%), and 93,930/99,630 regions (94.27883167720566%), above
  the frozen 96.06/96.79/94.15 floors.
- The coverage closure is entirely observable behavior: container coverage and
  rejections, chain evaluation order and short-circuiting, per-link literal
  adoption and range checking, unresolved-operand diagnostics, membership and
  chain operands inside f-strings, default-argument parameter references,
  argument-read conflicts, analysis hover coverage, and the AST JSON shapes.
  Two branches the walk could never take were removed with their invariants
  stated in the source: the right operand is already typed under the left
  operand's type, and every comparison operator produces `bool`. No synthetic
  coverage test or exclusion was added.
- Those figures are transcribed from that gate into this note and the task
  board, and the resulting documentation-only delta was re-verified with
  `cargo fmt --all --check`, `git diff --check`, `npm run check:reference`,
  `npm run docs:build`, `npm run check:hygiene`, and `npm audit`.

## Follow-Up

- Present Provisional ADR-0028 at the Batch 2 checkpoint.
- `enumerate` and `zip` are the next ordered Phase 3.5 ticket, followed by
  `len` and `str`.
