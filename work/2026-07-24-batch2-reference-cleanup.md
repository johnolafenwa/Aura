# Batch 2 Reference Cleanup

## Goal

Close the remaining documentation and reference-integrity gaps found in the
current conditional-expression baseline without changing language behavior.

## Work completed

- Propagated conditional expressions through the compact tutorial, tutorial
  scope, Manual index, and provisional compatibility surface.
- Aligned `AU3005` with non-copy Vec/Map reads and non-copy constant tuple
  indexing.
- Fixed the derived class/enum schema and generated-codec roadmap boundary at
  beyond Phase 6, completed the compact builtin namespace list, and expanded
  the retained-borrow conformance map with the four B2.0 regressions.
- Corrected the historical Batch 1 checkpoint record to reflect that
  ADR-0014 through ADR-0017 are Accepted.

## Verification

- `npm run check:reference`
- `npm run docs:build`
- `cargo fmt --all --check`
- `git diff --check`

## Follow-up

Include this documentation-only cleanup in the conditional-expression
admission audit. When the overlapping B2.0 handle-collision patch is admitted,
reconcile the shared conformance, reference-guard, and task-board context as one
final reference map.
