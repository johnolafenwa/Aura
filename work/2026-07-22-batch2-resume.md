# Batch 2 resume

## Session

- Started: 2026-07-22 16:10:25 BST.
- Current elapsed: 5h 13m 31s as of 2026-07-22 21:23:56 BST.
- Hard stop: 2026-07-23 04:10:25 BST after 12 continuous hours.
- Target: resume the preserved Batch 2 checkout, full-gate and commit the
  Phase 3 builtin-metadata foundation and fixed resource caps, then continue
  Phase 3, Phase 3.5, Phase 4, and V6 without entering Phase 5.

## Starting state

- B2.0-a, B2.0-b, and B2.0-c remain committed at `8bca972`, `8590cc3`, and
  `19d8de6`.
- The behavior-neutral builtin metadata foundation and the fixed-resource-cap
  ticket are preserved as uncommitted changes.
- The previous temporary CI worktree no longer exists, so no interrupted gate
  result will be used as completion evidence.

## Work completed

- Reconciled the live checkout, commit history, worktrees, and active-session
  record before resuming implementation.
- Centralized builtin receiver, fixed-parameter, variadic-parameter, and
  passing-mode metadata across semantic analysis and both execution backends.
- Added observable coverage for reversed named host arguments and variadic
  task-group binding, including positional order, missing required arguments,
  and keyword rejection with its source span.
- Removed unreachable instrumented branches by construction or by reusing the
  canonical parameter-passing resolver. No synthetic coverage test or coverage
  exclusion was added.
- Committed that metadata foundation as `e95799c` after its uninterrupted full
  gate, leaving only the fixed-resource-cap ticket dirty.
- Split whole-resource ceilings into a 256 MiB filesystem limit, retained
  64 MiB stream and TLS-configuration limits, and a 16 MiB incoming HTTP wire
  limit while preserving typed errors and MIR/direct parity.
- Corrected the cap reference status sections so every normative page identifies
  ADR-0018 as Provisional pending the Batch 2 checkpoint review, and added
  integrity guards for that status.
- Committed the full-gated fixed-resource-cap ticket as `97d0c7c` and began
  the Phase 3 Duration ticket: signed `i128` nanoseconds, exact literal and
  two-limb direct ABI propagation, constructors, conversions, arithmetic,
  `FloorDiv` dispatch, comparison semantics, and finite timer validation are
  now under integrated compiler, runtime, LSP, fixture, and reference testing.
- Added MIR/direct-parity fixtures for Duration arithmetic, negative floor
  division, conversion rendering, overflow, division by zero, and explicit
  negative process timeouts. A sink audit then found downstream checked-deadline
  and error-carrier gaps beyond the first conversion boundary; the affected
  shared, MIR, and direct slices now use checked deadlines and the declared
  typed result or diagnostic carrier.
- Fixed the final supervisor carrier defect found during the sign-off review:
  `Supervisor.wait_or_none` no longer reclassifies a deadline-construction
  failure as `Ok(Some(SupervisorEvent.Failed(...)))`; its shared wait core now
  returns `Err(Error.Io(io.Error.InvalidInput))`, while `Supervisor.wait`
  intentionally retains its documented synthetic failed-event carrier.

## Verification

- The first isolated full `npm run ci` reached every product gate successfully
  and stopped only at the frozen compiler coverage ratchet: 96.01% lines,
  96.71% functions, and 94.12% regions. The initial sandbox-only attempt had
  failed before testing because macOS Clang could not create a temporary file;
  rerunning with the normal elevated build permission resolved that environment
  restriction.
- After the behavior-focused test and structural cleanup,
  `npm run coverage:compiler:check` passes at 96.07% lines (55,863 total, 2,198
  missed), 96.82% functions (3,456 total, 110 missed), and 94.16% regions
  (82,964 total, 4,847 missed).
- The final exact full `npm run ci` then passed format, all Rust product tests,
  exhaustive MIR/direct parity in 262.90s, all 54 LSP tests, extension build and
  all 8 extension tests, compiler coverage at the exact totals above, 100% LSP
  coverage, the 29-page reference-integrity gate, and the documentation build.
  It stopped at `check:audit` because a newly published high-severity
  `brace-expansion` advisory covered the two transitive versions in the lockfile;
  Clippy and hygiene therefore did not run in that attempt.
- Updated only `package-lock.json` to patched in-range releases 2.1.2 and 5.0.7.
  `npm audit fix --package-lock-only` reported zero remaining vulnerabilities,
  and the isolated security update was committed as `059d269` before the
  language-foundation commit.
- After explicit authorization, `npm ci --ignore-scripts` installed the patched
  dependency tree and reported zero vulnerabilities. The isolated-worktree
  dependency links were repaired after that clean install removed Vite's
  ignored temporary directory.
- The final uninterrupted exact `npm run ci` passes end to end: format; 242 CLI
  tests; 555 compiler tests and every supporting Rust suite; exhaustive
  MIR/direct parity in 229.43s; 54 LSP and 8 extension tests; compiler coverage;
  100% LSP coverage; the 29-page reference gate; docs; npm and cargo audit;
  Clippy with warnings denied; and hygiene. Final compiler coverage is
  53,665/55,863 lines (96.065374219%), 3,346/3,456 functions (96.817129630%),
  and 78,117/82,964 regions (94.157706957%). No synthetic-coverage test or
  coverage exclusion was added.
- For the cap ticket, the focused exact-limit and HTTP overflow tests pass. The
  first instrumented cap report was green on behavior but just below the frozen
  region floor; an observable append-at-limit/limit-plus-one test closed that
  branch without synthetic line execution. `npm run coverage:compiler:check`
  now passes at 53,718/55,918 lines (96.065667585%), 3,353/3,463 functions
  (96.823563384%), and 78,183/83,037 regions (94.154413093%).
- `npm run check:reference` passes all nine integrity tests across the 29-page
  manual after the Provisional-status correction. No coverage exclusion was
  added.
- The exact cap-ticket `npm run ci` passes end to end: format; 244 CLI tests;
  556 compiler tests and every supporting Rust suite; exhaustive MIR/direct
  parity in 253.98s; 54 LSP and 8 extension tests; 100% LSP coverage; the
  29-page reference gate; docs; npm and cargo audit; strict Clippy; and hygiene.
  Its authoritative compiler report is 53,717/55,918 lines (96.063879252%),
  3,353/3,463 functions (96.823563384%), and 78,182/83,037 regions
  (94.153208811%). The one-line/one-region variance from the standalone run is
  in scheduler-sensitive runtime coverage and remains above every frozen floor.
- The Duration compiler library suite passed all 580 tests before the final
  supervisor regression. The focused shared-runtime regression, direct invalid
  timer-carrier test, and all three MIR timer-carrier integration tests pass
  after that correction; formatting and diff hygiene are clean.
- An unrestricted Duration backend-parity sweep passed all fixtures in 279.88s
  before the final shared supervisor correction. Because the runtime tree then
  changed, this result is retained only as interim evidence and will be rerun
  before sign-off.
- The final authoritative instrumented Duration run passes all 583 compiler
  unit tests and every integration suite. Coverage is 54,489/56,713 lines
  (96.08%), 3,425/3,538 functions (96.81%), and 79,311/84,154 regions
  (94.25%), above all frozen floors. The gap was closed exclusively with
  observable Duration/FloorDiv/timer semantics, typed carriers, exact
  diagnostics, and direct ABI boundary tests; no synthetic coverage test,
  exclusion, or defensive-branch probe was added.

## Follow-up

- Pass the exact full `npm run ci` gate and commit the completed Duration ticket
  with its Provisional policy ADR before proceeding to the next Phase 3
  control-plane ticket.
