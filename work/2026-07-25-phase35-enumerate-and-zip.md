# Phase 3.5 `enumerate` and `zip` loop forms

## Goal

Implement the Batch 2 expression-kernel loop forms `for ... in enumerate(xs):`
and `for ... in zip(xs, ys):` as one freeze-rule packet, over the bare-loop
borrow default, with `enumerate` yielding `(int64, item)` and `zip` stopping at
the shorter sequence.

## Test-First Evidence

- The check-fail fixtures `enumerate_requires_indexable_iterable` and
  `zip_rejects_ownership_modifiers` were written before either form existed,
  when both programs still failed as unknown names rather than with the
  operand-domain and borrow-default diagnostics they pin.
- The run fixture's oracle was recorded only after the lockstep lowering
  produced it, and the six rejection categories in the focused compiler test
  were each reproduced against the CLI before being pinned.

## Work Completed

- Recognized both forms in the checker's `for` iterable position rather than as
  callable values. Both are rejected everywhere else with `AU2005` naming the
  loop spelling, and a user `def enumerate(...)` or `def zip(...)` shadows the
  form completely.
- Restricted operands to the index-addressable collections `Vec[T]` and
  `Set[T]`, with `AU2002` naming the bare `for` form for `Range` and `Queue[T]`.
  Arity and named-argument violations report `AU2004`.
- Rejected ownership modifiers with `AU3002`, froze every operand for the whole
  loop the same way a bare collection loop freezes its iterable, and bound
  non-copy elements as shared borrows that cannot be moved out.
- Lowered both forms to one lockstep loop over `__index_option`, the same
  position-indexed member the ordinary collection loop already uses, so the
  direct backend needed no change and parity follows from a single dispatch
  path. The cursor advances immediately after the operands are read, which is
  what keeps `continue` correct; any operand yielding `None` exits the loop,
  which is what makes `zip` stop at the shorter sequence.
- Yielded the `enumerate` position as `int64` through an explicit cast from the
  `int32` loop cursor, so the yielded value is program data rather than a
  bootstrap index.
- Added Provisional ADR-0029, the normative Statements and Grammar rules, the
  maintained `examples/control_flow/enumerate_and_zip.au` example with its index
  entry and smoke oracle, the control-flow tutorial section and limits, the
  maintained-surface listings, the conformance map, a verified
  reference-integrity block, and a language-server bridge test.

## Verification

- The focused compiler test pins lockstep iteration over `Vec` and `Set`, `zip`
  stopping at the shorter operand, `break` and `continue`, all six rejection
  categories, operand freezing, the non-movable element binding, and user
  shadowing.
- The nine-category fixture suite passes, including the two new check-fail
  oracles and the new run fixture.
- The run fixture's exact stdout is identical through MIR and a forced-direct
  binary.
- `npm run check:reference` passes with the new verified Manual block, and the
  69-test language-server suite and its enforced 100% coverage gate stay green.
- The 895-test compiler library suite and the 259-test CLI product suite pass.
- The complete `npm run ci` gate is green before the commit, with compiler
  coverage at 64,313/66,939 lines (96.07702535143937%), 4,154/4,291 functions
  (96.80727103239339%), and 94,351/100,058 regions (94.29630814127806%), above
  the frozen 96.06/96.79/94.15 floors. The closure is observable behavior only;
  no synthetic coverage test or exclusion was added.
- Those figures are transcribed from that gate into this note and the task
  board, and the resulting documentation-only delta was re-verified with
  `cargo fmt --all --check`, `git diff --check`, `npm run check:reference`,
  `npm run docs:build`, `npm run check:hygiene`, and `npm audit`.

## Follow-Up

- Present Provisional ADR-0029 at the Batch 2 checkpoint.
- `len` and `str` are the next ordered Phase 3.5 ticket.
