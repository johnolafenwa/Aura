# Batch 2: Control Plane, Expression Kernel, Native Path, and V6

## Session

- Started: 2026-07-14 17:18:17 BST.
- Current elapsed: 7h 08m 46s as of 2026-07-15 00:27:03 BST.
- Hard stop: 2026-07-15 05:18:17 BST after 12 continuous hours.
- Batch boundary: complete B2.0 and Phases 3, 3.5, and 4 plus V6; do not begin Phase 5.

## Goal

Close the two accepted Batch 1 implementation defects and documentation/code
cleanup first. Then add the practical control-plane surface, the Python
expression kernel, explicit/default native execution path with artifact cache
and function-level tests, diagnose V6, preserve MIR/direct parity, and finish
at the Batch 2 checkpoint with one coverage re-ratchet.

## Authorized order

1. B2.0-a nested-consumption containment for retained non-copy borrows.
2. B2.0-b builtin handle-method collision rejection and direct dispatch guard.
3. B2.0-c diagnostic-code and documentation/usage closures.
4. Phase 3 practical control plane and retrying worker gate.
5. Phase 3.5 Python expression kernel in lexer-first, tuple-second order.
6. Phase 4 native execution path, artifact cache, function-level tests, and V6.
7. Final full gates, one coverage re-ratchet, checkpoint report, and stop.

## Work completed

- Recorded the completion-bound session before implementation.
- Accepted the Batch 1 rulings: ADR-0014, ADR-0015, and ADR-0017 become
  Accepted; ADR-0016 text is accepted while B2.0-a closes its implementation
  defect.
- Added four B2.0-a check-fail regressions covering a retained builtin method
  receiver, a retained earlier parameter, and both independently reported
  backend-divergence shapes.
- Closed the nested-consumption hole for ordinary calls in source order and for
  builtin receivers through callable ownership metadata. `AU3002` now points to
  the inner consuming place and carries a secondary span for the retained
  receiver or argument.
- Corrected the normative call rule in the Functions manual and promoted
  ADR-0016 to Accepted.
- Rejected explicit and inherited trait methods that collide with builtin
  `Queue[T]`, `Task[T]`, or `TaskGroup` members under dedicated code `AU2006`.
  The direct backend now preserves builtin-member precedence even if malformed
  internal MIR bypasses the checker.
- Added Queue, Task default-method, and TaskGroup collision fixtures, a
  checker-bypass emitted-object regression, and the matching normative
  diagnostics, trait, concurrency, conformance, CLI, and freeze-guard text.
- Added dedicated `AU3005`/`AU3006` categories for non-copy indexed reads and
  indexed compound assignment, corrected the remaining mutability and String
  constructor guidance codes, and pinned all six classifications through
  compiler fixtures and the LSP bridge.
- Made the CLI's one-diagnostic compile cap explicit, corrected `aura build`
  help so required `-o <output>` is not shown as optional, pinned the current
  class-field-default user-function limit, and accepted ADR-0014, ADR-0015,
  and ADR-0017 throughout the frozen reference.
- Recorded structured diagnostic frame lists for Batch 3; the current call
  chain and task ancestry remain flat prose notes until that native-frame pass.

## Verification

- Baseline commit: `a15df0f` (`feat: freeze Batch 1 language reference`).
- Baseline working tree was clean at session start.
- Before the checker fix, the exact empty-Vec receiver repro checked successfully
  and produced `Option.None` on MIR versus `Option.Some(99)` on direct. The
  exact two-element parameter repro checked successfully and produced `2` on
  MIR versus `3` on direct.
- The focused check-fail fixture suite passes with all four new `AU3002`
  diagnostics and the strengthened pre-existing nested-consumption diagnostic.
- Coverage-only follow-up stayed behavior-focused: five additional fixtures pin
  projected places, consuming method receivers and parameters, omitted method
  defaults, wrapped arithmetic/casts, f-string interpolation, and list
  construction. No synthetic line-execution test or exclusion was added.
- The post-follow-up instrumented report clears the frozen floors at 53,857 /
  56,066 lines (`96.06%`), 3,332 / 3,442 functions (`96.80%`), and 78,333 /
  83,196 regions (`94.15%`).
- The first clean full `npm run ci` attempt passed behavior, forced backend
  parity, LSP, extension, the instrumented floor, reference integrity, docs,
  and audit, then found one Clippy-only interface issue: the recursive method
  access collector had eight arguments against the enforced maximum of seven.
  The helper now derives its receiver from the member-call AST, preserving the
  behavior while reducing the interface; focused formatting and strict Clippy
  pass before the required clean full-gate rerun.
- The second clean full-gate attempt passed behavior, the 212-case forced
  backend parity matrix, LSP, extension, and all instrumented tests, then
  stopped only at the exact line floor: the interface refactor measured
  53,855 / 56,065 lines (`96.058146%`), just below the frozen `96.06%`
  threshold, while functions and regions still passed. The uncovered path was
  an unreachable defensive branch introduced solely by that refactor, so it
  was restructured away rather than tested synthetically.
- Added one observable `AU3002` regression for nested consumption inside a
  `try` expression on a retained method receiver. This pins a real expression
  wrapper supported by the recursive checker path and raises the accumulated
  report to 53,857 / 56,065 lines (`96.06%`), 3,332 / 3,442 functions
  (`96.80%`), and 78,335 / 83,197 regions (`94.16%`). No synthetic coverage
  test or exclusion was added.
- The final clean full `npm run ci` gate exits zero with the 242-test CLI
  product suite, 552-test compiler library suite, all fixtures and integration
  suites, the 212-case forced MIR/direct parity matrix, LSP and extension
  suites, clean compiler coverage at the frozen floors, 100% LSP coverage,
  29-page reference integrity, docs build, audit, strict Clippy, and hygiene.
- B2.0-b was pinned failing-first: the new fixtures were initially accepted,
  and the forged MIR collision initially selected the trait path. The focused
  fixture suite, emitted-object regression, diagnostic-registry unit test,
  reference-integrity gate, formatting, strict compiler Clippy, and diff check
  all pass after the fix.
- B2.0-b's clean full `npm run ci` gate exits zero with 242 CLI tests, 553
  compiler-library tests, the 212-case forced MIR/direct parity matrix, LSP and
  extension suites, 29-page reference integrity, docs build, audit, strict
  Clippy, and hygiene. The accumulated compiler report is 53,892 / 56,100 lines
  (`96.06%`), 3,334 / 3,444 functions (`96.81%`), and 78,384 / 83,247 regions
  (`94.16%`). No synthetic coverage test or exclusion was added.
- B2.0-c was pinned failing-first: the fixture suite reported `AU2999` where
  the new ownership/mutability/guidance oracles require dedicated codes, the
  CLI help regression showed `[-o <output>]`, and the focused LSP bridge test
  observed `AU2999` instead of `AU3005`.
- After implementation, the full check-fail fixture oracle, diagnostic-registry
  unit, CLI help test, two-error structured-diagnostic test across `check`,
  `run`, and `build`, and focused LSP `AU3005`/`AU3006` bridge test pass.
  Reference integrity passes all 29 pages, the VitePress build exits zero,
  strict compiler/CLI Clippy and formatting pass, and `git diff --check` is
  clean.
- B2.0-c's first full gate passed every behavior, parity, LSP, extension, and
  coverage check, then stopped only because the newly added reference guard
  searched for prose split across two Markdown lines. Narrowing the guard to a
  stable contiguous phrase made the standalone 29-page reference gate pass.
- The required clean full `npm run ci` rerun exits zero end to end. Its compiler
  report is 53,893 / 56,102 lines (`96.06%`), 3,334 / 3,444 functions
  (`96.81%`), and 78,384 / 83,247 regions (`94.16%`); LSP coverage remains
  100%. No synthetic coverage test or exclusion was added.

## Follow-up

- B2.0-a is committed at `8bca972` and B2.0-b at `8590cc3`. Commit the now
  full-gated B2.0-c cleanup, then begin the Phase 3 builtin metadata foundation.
