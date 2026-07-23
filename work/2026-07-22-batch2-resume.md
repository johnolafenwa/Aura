# Batch 2 resume

## Session

- Started: 2026-07-22 16:10:25 BST.
- Current elapsed: 11h 28m 51s as of 2026-07-23 03:39:16 BST.
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
- Passed the exact full Duration gate and committed the complete ticket as
  `b268c72`: signed nanosecond representation, exact conversion and rendering,
  checked arithmetic and `FloorDiv` dispatch, timer policies and typed error
  carriers, MIR/direct parity, LSP, reference, examples, tutorials, and
  Provisional ADR-0019 now advance together as one logical change.
- Implemented the Phase 3 deterministic and secure randomness surface across
  the shared xoshiro256** core, semantic analysis, MIR, direct code generation,
  native runtime, LSP metadata, reference, examples, tutorials, fixtures, and
  Provisional ADR-0020. Focused probes pin the seed-42 sequence, unbiased full
  `int64` bounds, high-53-bit floats, Fisher-Yates projected mutation, secure
  byte generation, identity rendering, imported constructors, and both runtime
  diagnostic paths.
- The closure audit found and corrected an imported-constructor MIR dispatch
  defect, direct-backend RNG lowering gaps, a native invalid-bounds test leak,
  inaccurate static diagnostic codes, and public transitive cloning paths that
  could duplicate RNG state through generic collections and task observers.
  It also proved that an unrelated user class named `Rng` never acquires
  builtin random semantics in the checker, MIR, direct runtime-type boundary,
  code generator, or editor completion.
- The first unrestricted serialized compiler suite passed 614 of 615 tests.
  Its one failure was an intentional safety check being too early: a maintained
  generic `Vec.get` definition was rejected merely because `T` was unresolved,
  even though all of its actual instantiations were clone-safe. The correction
  in progress records and propagates a generic clone-safety obligation so safe
  generic definitions remain source-compatible while any concrete `Rng`
  instantiation is still rejected with `AU3007`.
- A subsequent adversarial soundness audit found that the first obligation
  draft still lost requirements across qualified same-leaf module types,
  concrete and bound-based trait dispatch, renamed trait parameters, operator
  and `From` paths, and recursively expanding generic wrappers. The closure in
  progress preserves canonical nominal identity, treats clone safety as part of
  the callable and trait contract, forbids an impl from silently strengthening
  that contract, and terminates recursive type inspection conservatively. Each
  confirmed bypass and the no-crash edge is being pinned before the Randomness
  gate is rerun.
- The next serialized compiler pass exposed the runtime half of the same
  nominal-identity issue: imported trait impls now checked under their qualified
  type, but MIR still keyed imported instances by the leaf class name. The
  maintained `trait_impl_imports.au` example is the initial regression, and an
  adversarial local/remote same-leaf pair additionally proves that dispatch
  must retain the defining module instead of relying on suffix matching.
- Method-owned clone obligations are now tested separately from impl and trait
  header obligations. The focused matrix includes inherent associated methods
  and task targets, instance and associated trait calls, bound calls with
  deliberately colliding caller type-parameter names, generic operators, and
  implicit `From.from` selected by `try`; safe `int64` instantiations remain
  valid while every concrete `random.Rng` route must diagnose `AU3007`.

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
- The exact-tree final `npm run ci` for commit `b268c72` passed formatting; all
  Rust product suites; the complete forced MIR/direct fixture sweep; all 55 LSP
  and 8 extension tests; the 29-page, 98-Aurora-block reference gate; docs;
  npm and cargo audit; strict Clippy; and hygiene. Its compiler coverage report
  is 54,488/56,713 lines (96.08%), 3,425/3,538 functions (96.81%), and
  79,310/84,154 regions (94.24%); LSP coverage remains 100%. npm reported zero
  vulnerabilities, while cargo reported no vulnerabilities and only the
  already-allowed unmaintained `rustls-pemfile` warning.
- The first complete Randomness instrumented run is behavior-green: 626
  compiler library tests and every fixture and integration target passed. Its
  exact report is 56,372/58,881 lines (95.74%), 3,579/3,713 functions (96.39%),
  and 81,968/87,217 regions (93.98%). Under the standing coverage rule, the
  frozen 96.06/96.79/94.15 floors remain unchanged and the remaining gap is
  being closed only through observable Randomness, clone-safety, imported
  nominal-identity, diagnostic, and backend-parity behavior; no synthetic
  coverage test or exclusion has been added.
- The first coverage-recovery pass exposed and fixed qualified imported
  generic class and enum substitution, MIR type-argument preservation, and
  defining-module enum identity across re-exports. The corrected complete
  behavior suite is green at 244 CLI tests, 634 compiler library tests, and
  every integration target. Its exact report remains below the frozen ratchet
  at 56,417/58,954 lines (95.70%), 3,566/3,702 functions (96.33%), and
  82,001/87,264 regions (93.97%). A second narrow recovery pass is covering
  the reachable generic-bound/inference diagnostics and the normal-library
  Randomness FFI boundary while restructuring genuinely unreachable invariant
  closures; floors remain unchanged and no synthetic test or exclusion has
  been added.
- The second coverage-recovery pass removed roughly 451 duplicated semantic
  constructor lines and more than ten unreachable closure functions, then
  passed all 635 compiler library tests and every integration suite. The exact
  report reached 56,293/58,678 lines (95.94%), 3,562/3,682 functions (96.74%),
  and 81,870/86,946 regions (94.16%): the region floor is recovered, while the
  line and function floors remain the only red gates. Recovery remains limited
  to observable behavior and structural cleanup; no synthetic test or
  exclusion has been added.
- A read-only pre-commit audit then found seven concrete contract defects: an
  opaque direct-backend `clone` dispatch bypass, missing trait dispatch on
  `random.Rng`, filename-dependent builtin-origin confusion, a secure-byte
  count above the representable `Vec` length, two argument-binding diagnostics
  incorrectly classified as `AU2999`, malformed nested quoting in `AU3007`,
  and the missing `AU3007` CLI registry entry. Each is being closed with a
  failing behavioral regression before the final coverage and full-CI gates.
- All seven audit defects are now closed under focused behavioral regressions.
  The next exact instrumented pass remained behavior-green at 643 compiler
  library tests and every integration target, reaching 56,352/58,734 lines
  (95.94%), 3,565/3,684 functions (96.77%), and 81,952/87,015 regions
  (94.18%).
- A further behavior-only pass pins canonical builtin identity in reduced
  checker contexts, Queue/Task handle clone obligations, same-leaf imported
  nominal ambiguity, and secure-random validation/allocation/entropy mappings.
  All 646 compiler library tests and every integration target pass. The exact
  report is 56,391/58,734 lines (96.01%), 3,565/3,684 functions (96.77%), and
  82,006/87,015 regions (94.24%): only 29 covered lines and one function remain
  below the frozen floors. The final recovery targets observable operator
  generic-inference diagnostics and deduplicates unreachable defensive clone-
  obligation argument walks; no synthetic-coverage test or exclusion has been
  added.
- The final Randomness coverage-recovery pass adds observable AU2002 operator
  diagnostics for conflicting, uninferred, and bound-failing generic method
  inference, plus the direct host-allocation AU4005 mapping. It also
  deduplicates five equivalent clone-obligation argument walks and removes five
  unreachable direct-Randomness arity checks after the shared binder has
  already proved exact cardinality. The malformed-MIR mutable-writeback guard
  remains intact. All 647 compiler library tests and every integration target
  pass; the exact report is 56,424/58,729 lines (96.08%), 3,567/3,685
  functions (96.80%), and 82,044/87,016 regions (94.29%), above every frozen
  floor. No synthetic-coverage test or coverage exclusion was added.
- The authoritative exact-tree `npm run ci` for Randomness passes end to end:
  formatting; 244 CLI tests; 647 compiler library tests and every supporting
  Rust suite; the complete forced MIR/direct fixture sweep in 355.39s; all 56
  LSP and 8 extension tests; compiler coverage above the frozen floors; 100%
  LSP coverage; the 30-page, 184-Aurora-block reference gate; docs; npm and
  cargo audit; strict Clippy; and hygiene. npm reports zero vulnerabilities;
  cargo reports no vulnerabilities and only the allowed unmaintained
  `rustls-pemfile` warning. The Randomness coverage high-water mark remains
  56,424/58,729 lines (96.08%), 3,567/3,685 functions (96.80%), and
  82,044/87,016 regions (94.29%). No synthetic-coverage test or coverage
  exclusion was added.

## Follow-up

- Begin the Phase 3 JSON ticket test-first: establish the recursive
  `json.Value` and structured `json.Error` metadata contract, then implement
  exact numeric classification, bounded parsing and dumping, ownership-aware
  accessors, MIR/direct parity, LSP, the normative reference, maintained
  examples and tutorial, and Provisional ADR-0021 before its logical commit.
