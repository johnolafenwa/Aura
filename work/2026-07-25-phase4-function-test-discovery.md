# Phase 4 function-level `aura test` discovery

## Goal

Report one `aura test` result per `def test_*()` function with its assertion
output, while file-level `main` tests keep working unchanged.

## Work Completed

- Added a named-entry path to the MIR runtime so a test runner can enter a
  module at a chosen parameterless top-level function. Each test therefore runs
  through exactly the same runtime, scheduler, and trap handling an ordinary
  run uses, rather than a parallel execution path that could diverge from it.
- Discovered tests by the `test_` name prefix over the lowered module, taking
  only parameterless functions with no receiver. Every other declaration in a
  test file, including helpers, is ignored.
- Kept both models side by side: a file declaring at least one `def test_*()`
  reports one result per function, labelled `path::function`; a file declaring
  none keeps the file-level model and reports one result for the path, entered
  at `main` or at its top-level statements. Adding a test function to a file
  therefore changes only that file.
- Ran each test on its own worker under the shared timeout, so one hanging test
  is reported rather than stalling the run.
- Rendered a trapped test's diagnostic against its file while labelling the
  result with the function, so a failed `assert` shows its message, span, and
  call chain instead of only a failure count.

## Verification

- A CLI product test pins per-function results, that a helper function is not
  discovered, that a file without test functions still reports one file-level
  result, that a failing assertion reports its message and exact span, the
  combined pass and fail counts, and the non-zero exit status.
- The existing file-level `aura test` coverage continues to pass unchanged,
  which is the evidence that the older model was not disturbed.
- The complete `npm run ci` gate is green before the commit, with compiler
  coverage at 64,413/67,042 lines (96.07857760806658%), 4,158/4,295 functions
  (96.81024447031432%), and 94,490/100,201 regions (94.30045608327262%), above the
  frozen 96.06/96.79/94.15 floors. No synthetic coverage test or exclusion was
  added.
- Those figures are transcribed from that gate into this note and the task
  board, and the resulting documentation-only delta was re-verified with
  `cargo fmt --all --check`, `git diff --check`, `npm run check:reference`,
  `npm run docs:build`, `npm run check:hygiene`, and `npm audit`.

## Follow-Up

- V6 is the next ordered ticket.
