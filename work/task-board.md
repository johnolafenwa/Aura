# Task Board

Last updated: 2026-07-27

## Batch 4 of 6 (active)

- Authorized target: close B4.0-a through B4.0-d, then implement Phase 5's
  scalable runtime in the ratified strict order: reactor, public
  `yield_now()`, compiler safepoints, stack diet, scheduler soundness,
  structural Transfer rules, pinned-worker multicore, typed select,
  configurable blocking pool, and native structured frames. Stop at the Batch
  4 checkpoint without beginning Phase 6.
- Entry state: Batch 3 is accepted and committed at `1c249ab`. The Batch 4
  worktree opened from that clean checkpoint with compiler coverage floors
  frozen at `96.13/96.89/94.35`; see
  `work/2026-07-27-batch4-scalable-runtime.md`.
- Current stage: B4.0 implementation and its exact repository gates are
  complete. Cross-process runtime-identity and per-content-key locks now give
  concurrent cold direct runs one builder plus verified consumers without
  blocking established warm hits. Human mode flushes the exact wait/rebuild
  notices before the long operation; JSON mode provisionally buffers those
  notices to preserve one structured stderr document and retains them with the
  direct failure when `auto` falls back to MIR. Installed immutable runtimes
  remain able to build with caching disabled or unavailable.
  Capability diagnostic polish is committed at `4f0461e`, and suite-count
  precision is committed at `5cb4476`.
- B4.0 verification so far: all five `native_run_cache_*` tests, the full
  274-test CLI integration suite, and the complete Rust workspace pass under
  default parallelism. The
  deterministic four-process regression proves one rebuild, four successful
  results, one published entry, and a later verified hit with both `CC` and
  `CARGO` unavailable. Broad serialization has been removed from the ordinary
  Rust gate. The compiler-coverage gate retains a narrow single-threaded
  constraint after default-parallel instrumentation passed behavior but
  undercounted function coverage at 96.86%; the serialized run restored the
  stable pre-closure result to 4201/4336 functions (96.886531%) while
  retaining the 15 known LLVM profile-data warnings. Dedicated parity, stress,
  and sanitizer ordering also remains. Behavior-focused AU2999 and canonical
  generic-analysis tests then produced final exact coverage of 96.142124%
  lines, 96.909594% functions, and 94.360014% regions, clearing the frozen
  96.13/96.89/94.35 floors with no synthetic coverage test.
  The timed warm-hit regression uses an installed immutable runtime so
  parallel Cargo activity cannot change its runtime identity while it holds an
  exact key lock; production identity remains strict and content-derived. The
  exact final-tree `npm run ci` is green across format, default-parallel Rust,
  the 529.82-second forced-backend matrix, all 79 LSP tests at 100% coverage,
  all 13 extension tests, compiler coverage, reference and stale-syntax
  integrity, docs, both audits, warning-denied Clippy, and hygiene. This
  checkpoint change lands B4.0-a/b and its behavior-focused coverage closure
  as one isolated commit family. Phase 5 proper has not started; the next
  action is the before-reactor benchmark baseline.
- Benchmark host: Mac14,9 Apple M2 Pro, 10 logical CPUs, 16 GiB RAM, macOS
  26.5.2 (25F84). Contractual measurements require the dedicated quiet-machine
  protocol and per-stage before/after evidence. B4.0 is committed at `665d540`,
  and the dedicated harness is committed at `850e906`. The contractual
  before-reactor baseline is complete from a clean tree with both process
  inventories empty: 10,000 sleepers pass at 189.641 MiB worst peak RSS; idle
  passes at 0.018886% worst CPU; all five timer runs fail the overlap gate at
  13–15 ms arm spans with diagnostic raw p99 overshoot of 8–10 ms; V6 medians
  are 32.734250 ms for int32 and 10.248625 ms for int64. Raw JSON and hashes are
  recorded in `work/2026-07-27-phase5-runtime-benchmarks.md`. No Phase 5.1
  runtime implementation edit preceded the baseline; failing reactor tests are
  next.
- Standing rules: behavior-focused coverage only; floors remain frozen through
  the batch; one truncated re-ratchet at sign-off; contained semantic
  gap-fills may proceed provisionally, but larger language/runtime questions
  stop for review; reference and parity surfaces move with behavior.

## Batch 3 of 6 (complete)

- Authorized target: close B3.0-a through B3.0-e in separate test-first
  commits, then perform the ratified ADR-0022 capability-syntax migration,
  complete the post-migration reference/parity/coverage/full-CI checkpoint, and
  stop without beginning Phase 5.
- Required order: artifact-cache integrity; heterogeneous `enumerate`/`zip`
  direct lowering; tuple equality; `int64` length-surface unification; the
  diagnostic/comment polish packet; ADR-0022 inventory and migration; final
  checkpoint gates and one coverage re-ratchet.
- Entry state: clean at `4929bab`. Old coverage-only build output was cleaned
  under the repository hygiene rule before the repeated Batch 3 gates.
  Prerequisite hygiene repair `18b7f00`, Part-0 ratification commit `19a10f4`,
  completed B3.0-a commit `6afe47c`, and completed B3.0-b commit `fc22696` are
  isolated. B3.0-c is exact-tree green and isolated in `e05c5e6`; B3.0-d and
  B3.0-e are both exact-tree green and committed in isolation. B3.0 is closed,
  and the first ADR-0022 capability-syntax migration landed across §1-§7.
  A line-by-line audit after checkpoint commit `91e0d5f` found binding
  ADR-0022 gaps; the corrective pass closed them and a fresh exact-tree
  `npm run ci` is green. Batch 3 implementation and verification are complete
  at the requested checkpoint; see `work/2026-07-27-batch3-checkpoint.md`.
  Post-gate coverage cleanup leaves `target/` at 6.8 GiB with 193 GiB free.
  The corrective tree is committed at `1c249ab`; nothing is pushed.
- Batch 2 ADR disposition: ADR-0018, ADR-0019, ADR-0020, ADR-0021, ADR-0023,
  ADR-0024, ADR-0025, ADR-0027, and ADR-0028 are Accepted as implemented.
  ADR-0026 and ADR-0030 become Accepted with their required B3.0 amendments;
  ADR-0029 is Accepted with the B3.0-b function-wide per-loop binding-slot
  isolation amendment. ADR-0031 remains Accepted. Acceptance does not by itself
  claim implementation or gate completion.
- ADR-0022 is Accepted with all ten binding answers and the Range ruling.
  ADR-0009 is superseded in part; ADR-0005, ADR-0006, ADR-0013, ADR-0016,
  and ADR-0017 are amended. The first coordinated source flip and cache-v4
  invalidation are committed. The corrective worktree adds independent
  semantic-interface schema version 2, a complete manifest-v2 preservation
  ledger, strict builtin inventory, and the missed binding semantics.
- B3.0-a implementation: cached native entries now atomically publish a
  platform-native `program`, its `program.sha256`, and a key-bound unique
  `entry-id`; every hit uses bounded no-follow reads and verifies identity,
  digest, regular-file/execute state, size, and native launch shape. Aurora
  materializes the verified bytes into a private per-launch executable and
  invokes it with raw `execv`, preventing both cache-path substitution and the
  macOS ENOEXEC shell fallback. Exact-entry quarantine makes corruption and
  executable-format failure rebuild without racing a concurrent replacement;
  environmental launch failures preserve valid cache state. Private cache-root
  trust, exact `0700` creation under a permissive umask, lease-protected stale
  launch cleanup, owner-aware cache-stage cleanup, and runtime-archive memo
  invalidation are pinned. Cold publication is keyed by the exact archive
  bytes and ordered native link arguments used by the link, so an immediate
  warm run no longer needs the old settle-and-delete workaround. The cache
  format tag is bumped to v3, and behavioral regressions cover the verified
  hit, corruption, non-regular members, cleanup, and preservation paths.
- B3.0-a first-pass evidence: all behavioral, parity, LSP, extension, coverage,
  reference, docs, audit, and Clippy gates passed. Final hygiene exposed
  committed trailing whitespace in `personal/file_ops.au`; prerequisite commit
  `18b7f00` repairs that non-semantic baseline so a pre-commit full gate can
  genuinely pass. The cache review then strengthened launch isolation and the
  regression matrix.
- B3.0-a disposition: complete and exact-tree decision-gate green. `npm run ci`
  passed 265 CLI tests, 897 compiler tests, forced MIR/direct parity, all 70
  LSP tests, all 13 extension tests, reference integrity, docs, audits, Clippy,
  hygiene, and both coverage gates. Compiler coverage is `64,410/67,039`
  lines (96.08%), `4,158/4,295` functions (96.81%), and
  `94,473/100,184` regions (94.30%); LSP coverage remains 100%. No synthetic
  coverage test, exclusion, or coverage-only branch was added. The exact
  network cases also passed outside the restrictive sandbox. Post-gate
  `target/` size was 14 GiB with 157 GiB free, so no cleanup threshold was
  crossed.
- B3.0-b disposition: complete. ADR-0029 now records function-wide target-slot
  isolation for every
  `for` branch, so later loops may reuse source names with different types.
  `zip(numbers, words)` followed by `zip(words, numbers)` reusing
  `number, word` is the mandated acceptance case. The run fixture extension is
  the required red repro: MIR succeeds while the pre-fix forced direct path
  traps with `AU4001`. Fresh typed target slots are implemented for lockstep,
  Range, Queue, Vec, Set, and recursive tuple targets, with iterable evaluation
  outside the target scope and the same physical slot threaded through
  mutable-Vec writeback. All 55 focused MIR tests pass, and forced MIR/direct
  runs match the `enumerate_and_zip`, `tuple_for_pattern_queue`, and
  `vec_borrow_mut_iteration` oracles. Compiler coverage is green at
  `64,476/67,106` lines (96.08%), `4,162/4,299` functions (96.81%), and
  `94,558/100,270` regions (94.30%), with no synthetic coverage test or
  exclusion. The exact `npm run ci` decision gate passed 265 CLI tests, 900
  compiler tests, forced backend parity, all 70 LSP tests, all 13 extension
  tests, both coverage gates, reference integrity, docs, audits, Clippy, and
  hygiene.
- B3.0-c disposition: complete. Structural `==` and `!=` are implemented for
  tuples whose elements are equatable, while tuple ordering remains rejected.
  Symmetric recursive tuple-literal context, exact same-static-type checking,
  non-consuming retained operands, first-false comparison-chain behavior,
  chain mutation-conflict diagnostics, and metadata-independent runtime
  equality all pass focused compiler tests.
  Forced MIR and direct runs match the nested `Option`/`float32`, non-copy
  `(String,)`, generic-float32, `==`/`!=`, once-only, and short-circuit fixture
  oracle. ADR-0026 is Accepted; the Manual, tutorial, maintained example,
  analysis/LSP regression, reference gate, and executable reference are
  aligned. Compiler coverage is green at `64,588/67,216` lines (96.09%),
  `4,176/4,313` functions (96.82%), and `94,731/100,444` regions (94.31%),
  above the frozen `96.07/96.81/94.29` floors with no synthetic test or
  exclusion. The exact `npm run ci` decision gate passed 265 CLI tests, 905
  compiler tests, forced backend parity, all 71 LSP tests, all 13 extension
  tests, both coverage gates, reference integrity, docs, audits, Clippy, and
  hygiene. Post-gate `target/` size is 19 GiB with 149 GiB free, below both
  cleanup thresholds.
- B3.0-d disposition: complete. `String.len()`,
  `String.byte_len()`, `Vec.len()`, `Map.len()`, and `Set.len()` must return
  `int64` consistently with builtin `len(...)`, with compatibility narrowing,
  LSP, examples, tutorials, reference, and resource-cap wording updated in the
  same test-first decision commit. Implementation, focused behavior, both
  backends, all 72 LSP tests at 100% coverage, all 13 extension tests,
  reference/docs gates, and compiler coverage are green. Coverage is
  `64,612/67,239` lines (96.09%), `4,179/4,315` functions (96.85%), and
  `94,761/100,470` regions (94.32%), above the frozen floors without synthetic
  tests or exclusions. An earlier gate attempt passed all code, parity, LSP,
  extension, and coverage stages before finding a line-wrap-sensitive reference
  assertion; that guard is repaired to pin the same normative statement without
  depending on its wrapping. The exact full-repository `npm run ci` decision
  gate is now green end to end: formatting, 916 compiler tests, 265 CLI tests,
  every fixture and package suite, the 516.80-second forced MIR/direct parity
  matrix, all 72 LSP tests, all 13 extension tests, compiler coverage, 100% LSP
  coverage, reference integrity, docs build, npm and Rust audits, Clippy with
  warnings denied, and hygiene.
- B3.0-e closed the four polish items in one isolated commit: clone-safety-aware
  `AU3005` guidance, the dedicated `AU2007` builtin-redefinition code,
  access-kind-specific `AU3002` recovery help, and the stale pre-selector
  comment in `backend_parity.rs`. Its full `npm run ci` was green end to end:
  918 compiler unit tests, 265 CLI tests, the forced parity matrix in 529.30s,
  73 language-server tests, 13 extension tests, coverage at 96.12/96.85/94.32
  against the frozen 96.07/96.81/94.29 floors, reference integrity, the docs
  build, both audits, Clippy with warnings denied, and hygiene.
- Batch 3 corrective disposition: complete and exact-tree gate green. The
  Range modifier ruling, capability-position diagnostics, retained
  shared-match places, mutable-source alias rejection, borrowed-return
  containment docs, semantic-interface schema version 2, retired-syntax gate,
  manifest-v2 preservation ledger, strict builtin inventory, and release notes
  are integrated.
- Migration accounting: 1,260 semantic occurrences and 832 findings are
  recorded with zero unresolved. All 773 pre-flip bare matches were reviewed:
  416 of 417 place matches became `match own` and one fixture was deleted;
  among 356 temporary matches, 22 became `match own` and 334 stayed bare.
  All 468 bare copy parameters were reviewed: 466 remain bare shared, 2 were
  deleted, and none required `own`. Of 19 borrowed returns, 11 copy returns
  became ordinary owned returns; 8 non-copy/unresolved redesign findings were
  resolved through 6 maintained-fixture redesigns and 2 obsolete deletions.
  The final manifest spans 683 files and all 59 migrator tests pass.
- Strict inventory status: zero rendered-signature/metadata mismatches, zero
  missing sibling-retention applications, zero missing rendered builtin
  variants, zero missing structured call shapes, and zero unlinked signatures.
  The retired-syntax gate has no active finding outside the four exact
  retirement fixtures.
- Final verification: one fresh `npm run ci` passes after exposing and fixing
  a TaskGroup named-argument forwarding regression. It includes 23 Aura unit
  tests, 268 CLI tests, 928 compiler tests, the 732.74-second forced
  MIR/direct parity matrix, 79/79 LSP tests, 13/13 extension tests, reference,
  docs, audits, warning-denied Clippy, hygiene, compiler coverage, and 100% LSP
  coverage. The 928/268 suite counts are gate-condition observations from the
  debug profile with Rust tests run single-threaded; alternate invocations can
  report 927 compiler and 265 CLI tests. Compiler coverage is 64,645/67,244
  lines (96.134971%),
  4,200/4,335 functions (96.885813%), and 94,962/100,649 regions
  (94.349671%); final floors are `96.13/96.89/94.35`. No synthetic coverage
  test, exclusion, or coverage-only branch was added.
- Remaining mechanical closeout: post-gate disposable-artifact cleanup and
  the corrective commit. The initial `cargo clean` removed 56.0 GiB and raised
  free space to 199 GiB. Phase 5 remains unstarted.

## Batch 2 Checkpoint (complete)

- Result: Batch 2 of 5 is complete at its requested checkpoint. Phase 5 was not
  started. B2.0 is fully closed, Phase 3 was already complete on entry, Phase
  3.5 is complete through conditional expressions, membership and comparison
  chains, `enumerate`/`zip`, and `len`/`str`, and Phase 4 is complete through
  the `aura run` backend selector, the content-addressed artifact cache, and
  function-level `aura test` discovery. V6 is diagnosed and halved.
- The full checkpoint report is `work/2026-07-25-batch2-checkpoint-report.md`.
  It carries the B2.0 disposition with repro results, per-phase evidence, the
  Provisional ADR list, the retired-hint list, the worker example path, the
  backend-default decision and its measurements, the V6 findings, coverage per
  logical decision commit, the re-ratcheted floors, and the recommended
  movements between Batches 3 to 5.
- Coverage floors are re-ratcheted once, by downward truncation from the final
  measurement, to lines/functions/regions `96.07/96.81/94.29`. The
  language-server gate remains enforced at 100%.
- The Batch 3 entry ruling accepts ADR-0018, ADR-0019, ADR-0020, ADR-0021,
  ADR-0023, ADR-0024, ADR-0025, ADR-0027, and ADR-0028 as implemented.
  ADR-0026, ADR-0029, and ADR-0030 advance with their named B3.0 amendments.
- Accepted checkpoint amendment: ADR-0031 ratifies `mir` as the `aura run`
  default for the edit-run path and retains `auto` as the `aura build` default.
  It explicitly amends the original interim-`auto` roadmap clause without
  weakening forced-backend parity. The blocker for a native `run` default is
  binary size, not correctness or compile time; a direct hello-world executable
  is about 57 MB, so a first launch costs about 0.8s even on a cache hit.
- Corrected checkpoint coverage is 64,409/67,039 lines, 4,158/4,295 functions,
  and 94,472/100,184 regions. The enforced floors remain
  `96.07/96.81/94.29`, with LSP coverage at 100%.

## Batch 2 Implementation Record (completed)

- Result: Phase 3 is complete and committed through `9ff7e82`, including Duration, deterministic and secure Randomness, recursive JSON, Bytes/codecs/SHA-256, assertion statements, and the maintained application-level retry worker. The editor completion/package repair remains committed at `f34b4de`, the ownership tutorial correction at `6665090`, and proposed future capability-syntax ADR-0022 at `929c0b8`; ADR-0022 is not implemented or mixed into Batch 2 semantics. Phase 3.5 newline continuation is complete and decision-gate green: the lexer tracks and validates nested `()`, `[]`, and `{}`, suppresses ordinary continuation layout while retaining physical spans, preserves delimited expression-`match` layout islands, and reports source-related pairing diagnostics. Parser coverage pins multiline signatures, calls, type arguments, grouping, indexing, and collection literals without changing the trailing-comma, backslash, or single-line string/f-string boundaries. Compiler analysis, the language server, and VS Code newline indentation preserve editor behavior across continued and incomplete source. The normative reference, now-Accepted ADR-0025, maintained example/tutorial, executable-reference gate, frozen coverage floors, forced-backend parity, and exact full-CI gate are aligned.
- Current verification: focused Bytes tests, all fixture categories, language-server regression, executable reference integrity, docs build, `git diff --check`, and the complete exact-tree `npm run ci` gate pass. The exact Bytes-era `npm run coverage:compiler:check` gate passes all 251 instrumented CLI tests, 781 compiler library tests, and supporting suites at 60,768/63,252 lines (96.072851451337%), 3,968/4,091 functions (96.993400146663%), and 88,637/94,027 regions (94.267603986089%), above the frozen 96.06/96.79/94.15 floors. The Bytes coverage gap was closed with observable behavior/diagnostic/backend tests plus removal of unreachable validated-decoder and duplicate adapter branches; no synthetic test or coverage exclusion was added. For `assert`, all nine fixture categories, the focused 12-test compiler assertion suite, the CLI behavior matrix, the full 60-test language-server suite, the full 10-test extension suite, the 33-page executable reference-integrity gate, the docs build, the maintained example smoke, and the complete exact-tree `npm run ci` decision gate pass. The focused compiler coverage includes a source-starting lazy-message ownership regression, and editor coverage pins invalid `assert` diagnostics at the keyword. The exact `assert` coverage gate passes all 256 instrumented CLI tests, 795 compiler library tests, and supporting suites at 60,904/63,399 lines (96.06460669726651%), 3,976/4,099 functions (96.99926811417419%), and 88,875/94,275 regions (94.27207637231504%). Its five-line-only first-pass shortfall was closed with observable exported-runtime diagnostic and refcount tests; no synthetic test or coverage exclusion was added. The retry-worker CLI regression passes its exact 15-line oracle through both MIR execution and a forced-direct binary; it pins recovery, terminal `429`, final-attempt no-sleep/no-RNG ordering, explicit timeouts, and seven real loopback requests. Its exact coverage gate passes all 257 instrumented CLI tests, 795 compiler library tests, and supporting suites at 60,904/63,399 lines (96.06460669726651%), 3,976/4,099 functions (96.99926811417419%), and 88,875/94,275 regions (94.27207637231504%). No synthetic-coverage test, exclusion, or coverage-only production restructuring was added. Lightweight reference and diff checks plus the complete exact-tree `npm run ci` decision gate pass.
- The exact newline-continuation compiler coverage gate passes at
  61,133/63,639 lines (96.06216313895567%), 3,992/4,116 functions
  (96.98736637512148%), and 89,215/94,642 regions
  (94.26575938800956%). Its initial four-line floor gap was closed with an
  observable typed-completion recovery test for an escaped quote inside a
  continued call. No synthetic-coverage test, exclusion, or coverage-only
  production restructuring was added.
- Completed tuple ticket (`1380b8d`): the parenthesized fixed-arity tuple implementation,
  Provisional ADR-0026, normative Manual, maintained example/tutorial, and
  executable-reference packet are integrated. Compiler, fixture, exact
  MIR/direct behavior, language-server, reference-integrity, docs-build, and
  diff checks pass. Product-aware boolean/enum/nested-tuple pattern
  exhaustiveness and teaching diagnostics for recursive tuple fields are
  included. Mutable tuple writeback, equality/order, tuple iteration/methods,
  empty tuples, multi-element trailing commas, and dynamic indexing remain
  outside this minimal ticket.
- Exact tuple coverage passes at 62,917/65,489 lines
  (96.072622883232299%), 4,097/4,225 functions (96.970414201183431%), and
  92,077/97,666 regions (94.277435340855575%). The closure uses observable
  diagnostic, exhaustiveness, ownership, import/generic, runtime, and native
  dispatch behavior only. No synthetic-coverage test or exclusion was added;
  parser/checker/MIR-validation-proven defensive branches were collapsed with
  their invariant checks retained and justified in the dated work note.
- Its complete `npm run ci` decision gate passed before commit, including
  forced MIR/direct parity, 100% LSP coverage, compiler coverage, reference,
  docs, audits, Clippy, and hygiene.
- Completed conditional-expression ticket: Python-style `a if condition else b`
  is integrated with now-Accepted ADR-0027, exact-bool checking, contextual arm
  unification, lazy one-arm execution, conservative ownership-state merging,
  MIR/direct lowering, compiler analysis/LSP coverage, fixtures, maintained
  example/tutorial, and the normative reference packet.
- A full-suite audit of the corrected pre-expression ownership replay found and
  closed three reachable regressions before admission: enum-variant and
  module-qualified paths such as `io.Error.NotFound` and `json.Value.Null` were
  rejected as field reads; module-rooted namespace paths were resolved as
  call-argument places; and copy-typed `mut ` arguments lost their
  retained access while the new source-ordered rejection displaced the
  parameter-aware same-level overlap diagnostic. The complete compiler,
  fixture, and 259-test CLI product suites are green on the repaired tree.
- Exact conditional coverage passes at 63,752/66,360 lines
  (96.06992163954189%), 4,137/4,268 functions (96.9306466729147%), and
  93,478/99,158 regions (94.27176828899331%). The closure uses observable
  semantic, diagnostic, ownership, editor, runtime, and backend-parity tests;
  no synthetic test or exclusion was added. Two duplicated ownership walks
  introduced by the replay repair were collapsed with their invariants retained
  and stated in the source.
- Completed B2.0-b generalization: the ratified builtin no-shadowing rule now
  covers every builtin target rather than the four named in the original repro.
  `impl Sized for Vec[int32]` with a `len` method and `impl Probe for String`
  with a `contains` method were reproduced as accepted programs whose trait body
  was silently unreachable on both backends; both are now `AU2006` at check
  time. The message generalizes to "builtin method", the direct backend's
  existing `BuiltinMember` precedence guard already covers every receiver base,
  and a noncolliding trait method still dispatches on a builtin target.
  Fixtures, a cross-target compiler regression, the maintained example and
  smoke oracle, the traits tutorial, the normative rule, the `AU2006` category
  text, the conformance map, and the reference guard ride the same commit. Its
  exact coverage gate passes at 63,750/66,358 lines (96.06980318876398%),
  4,137/4,268 functions (96.9306466729147%), and 93,474/99,154 regions
  (94.27153720475219%); no synthetic coverage test or exclusion was added.
- Completed membership/comparison-chain ticket: `in`, `not in`, and Python-style
  chained comparisons are integrated with now-Accepted ADR-0028. Equality,
  ordering, and membership now share one precedence level and chain rather than
  left-folding; membership delegates to `contains` or `contains_key` over
  `Vec`, `Set`, `Map` keys, and `String`; chains evaluate every operand at most
  once and short-circuit at the first false link. Five `AU2005` hints are
  retired to pass-through acceptance through a new `.accept` fixture marker.
  Fixtures, the maintained example and tutorial, the normative Manual and
  Grammar, the conformance map, verified reference blocks, and the
  language-server bridge ride the same commit. Its exact coverage gate passes at
  64,028/66,649 lines (96.06745787633723%), 4,145/4,281 functions
  (96.82317215603831%), and 93,930/99,630 regions (94.27883167720566%); the
  closure is observable behavior only, and two branches the replay walk could
  never take were removed with their invariants stated in the source.
- Decision condition: the complete `npm run ci` gate is the final pre-commit
  proof for each ticket.
- Completed `enumerate`/`zip` ticket: both are compiler-known `for` iterable
  forms with Provisional ADR-0029, restricted to `Vec[T]` and `Set[T]` operands
  over the bare-loop borrow default. `enumerate` yields `(int64, element)` and
  `zip` stops at the shorter operand. Both lower to one lockstep loop over the
  position-indexed member the ordinary collection loop already uses, so the
  direct backend needed no change. Fixtures, the maintained example and
  tutorial, the normative Statements and Grammar rules, the conformance map, a
  verified reference block, and the language-server bridge ride the same commit.
  Its exact coverage gate passes at 64,313/66,939 lines (96.07702535143937%),
  4,154/4,291 functions (96.80727103239339%), and 94,351/100,058 regions
  (94.29630814127806%); no synthetic coverage test or exclusion was added.
- Completed `len`/`str` ticket: both are maintained builtin functions with
  Provisional ADR-0030. `len` delegates to the value's own `len()` member and
  produces `int64`, with its domain defined by that member rather than a list;
  `str` is total over the renderable surface and produces the same `String` as
  `print` and f-string interpolation. Both lower by delegation, so the direct
  backend needed no change. Both names are now reserved, which is recorded as a
  source-compatibility change on the status page. This completes Phase 3.5. Its
  exact coverage gate passes at 64,387/67,014 lines (96.07992359805414%),
  4,154/4,291 functions (96.80727103239339%), and 94,439/100,150 regions
  (94.29755366949576%); no synthetic coverage test or exclusion was added.
- Completed Phase 4 selector ticket: `aura run --backend mir|direct|auto` is
  implemented, with `direct` reporting build and launch failures rather than
  degrading and `auto` degrading visibly. Both MIR legs of `backend_parity.rs`
  now pass `--backend mir` explicitly. The default stays `mir`: `auto` pays a
  full compile and link on every run, measured at 1.385s against 0.012s for
  hello-world, so the artifact cache is the precondition for changing it. The
  default lives in one named constant with that reasoning attached. Its exact
  coverage gate passes at 64,388/67,014 lines (96.08141582355925%), 4,154/4,291
  functions (96.80727103239339%), and 94,440/100,150 regions
  (94.29855217174239%); no synthetic coverage test or exclusion was added.
- Completed native artifact cache ticket: `aura run`'s direct path is
  content-addressed on compiler version, host target, backend, runtime archive
  content, and the complete lowered program, which already covers the entry
  source and every dependency. The runtime identity is a content hash memoized
  against a cheap stamp, because a direct build can restamp an unchanged
  archive. Entries publish atomically under `programs/`, `AURORA_CACHE_DIR`
  overrides the location, and a cache failure degrades to an ordinary build.
  Benchmarks: MIR 0.00s, cold compile+link 1.31s, warm first touch 0.81s, warm
  resident 0.01s, with a 57 MB hello-world binary. The default stays `mir`; the
  remaining blocker for a native default is binary size, not compile time. Its
  exact coverage gate passes at 64,396/67,022 lines (96.08188356062189%),
  4,155/4,292 functions (96.80801491146319%), and 94,455/100,165 regions
  (94.29940598013278%); no synthetic coverage test or exclusion was added.
- Completed function-level `aura test` discovery: a file declaring
  parameterless `def test_*()` functions reports one result per function through
  a new named-entry path in the MIR runtime, so each test uses the same runtime,
  scheduler, and trap handling as an ordinary run. Helpers and parameterized
  functions are not discovered, a failing assertion reports its message and span
  against the file, and a file declaring no test functions keeps the file-level
  model unchanged. Its exact coverage gate passes at 64,413/67,042 lines
  (96.07857760806658%), 4,158/4,295 functions (96.81024447031432%), and
  94,490/100,201 regions (94.30045608327262%); no synthetic coverage test or
  exclusion was added.
- Completed V6: the direct backend's narrow-width range check was a two-sided
  signed comparison costing five instructions plus a branch on the result of
  every `int32` operation, against `int64`'s single overflow-producing
  instruction plus a branch. The check is now one biased unsigned comparison,
  which took the ten-million iteration loop from 0.0697s to 0.0327s with
  `int64` unchanged at 0.0111s, so the ratio moved from 6.05x to 2.95x. The
  residual gap is the separate branch itself; closing it means giving narrow
  widths their own arithmetic width, which is a backend representation change
  rather than a contained fix. Both numbers are recorded in
  `benchmarks/direct_integer_loops/README.md` and the benchmark is runnable as
  `npm run bench:direct-integer-loops`. Its exact coverage gate passes at
  64,409/67,039 lines (96.07691045510822%), 4,158/4,295 functions (96.81024447031432%),
  and 94,472/100,184 regions (94.29849077697038%); no synthetic coverage test or
  exclusion was added.
- Resume point: Batch 2 is complete at its checkpoint and every landed ticket is
  full-gated. The only untracked files are the two user-created files
  `hello.text` and `personal/file_ops.au`, which were never staged.
- Phase 4 note: the prepared Phase 4/V6 scratch history under `/private/tmp` was
  based on a commit predating every ticket landed in this batch. It was treated
  as reference material rather than applied; the selector, cache, function-test
  discovery, and V6 work in this batch were derived against the current tree and
  gated here.
- Freeze rule: every semantic addition or correction must update its ADR/reference, fixtures, examples, and tutorials in the same logical commit; full `npm run ci` must be green before each commit.
- Coverage rule: floors stayed frozen at lines/functions/regions `96.06/96.79/94.15` through the batch, with behavior-focused tests only. The one downward-truncated re-ratchet at sign-off has been applied, raising them to `96.07/96.81/94.29`.

## Batch 1 Reference-Freeze Checkpoint (historical)

- Result: Batch 1 of 5 is complete at the reference-freeze checkpoint: P1-P5, the shared structured diagnostic system, MIR call/task backtraces, the executable normative Manual, four provisional semantic gap-fill ADRs, and the one-time coverage re-ratchet all landed together. No Batch 2 or Phase 3 work started.
- Checkpoint disposition (historical): ADR-0014 through ADR-0017 are Accepted.
  ADR-0014, ADR-0015, and ADR-0017 were accepted at the Batch 2 entry gate;
  ADR-0016's text was accepted there and its status became Accepted when
  B2.0-a closed the recorded implementation defect.
- Final compiler coverage: 53,769/55,971 lines (96.065820%), 3,324/3,434 functions (96.796738%), and 78,212/83,064 regions (94.158721%). Enforced floors are 96.06% / 96.79% / 94.15% by downward truncation; LSP coverage remains 100%.
- Quality result: the exact full `npm run ci` gate passes, including the 242-test CLI product suite, 552-test compiler library suite, forced MIR/direct parity matrix, LSP and extension suites, instrumented tests, 29-page reference integrity, docs build, audit, Clippy, and hygiene. No synthetic-coverage test or coverage exclusion was added.
- Historical next step (completed): the ADR-0014 through ADR-0017 disposition
  gate was completed before Batch 2 implementation continued. V6 remains in
  Batch 2 with Phase 4 native work; native backtraces remain in Batch 3 frame
  work.

## Previous Completed Milestone

- Result: Phase 1.5 D3 -> D2 -> D4 -> D5 -> D6 is complete with one full-gated decision commit each. D6 is `683b0cf`; the one-time sign-off coverage ratchet is included in the sign-off commit.
- Final compiler coverage: 51,977/54,114 lines (96.050930%), 3,217/3,326 functions (96.722790%), and 75,590/80,357 regions (94.067723%). Enforced floors are now 96.05% / 96.72% / 94.06% using the established two-decimal downward-truncation policy.
- Quality result: no synthetic-coverage test or coverage exclusion was added. All behavior, backend parity, LSP, extension, instrumented, reference, docs, audit, Clippy, and hygiene gates pass.
- Next: investigate V6's int32/int64 direct-loop inversion before or with Phase 4; Phase 2 has not started.

## Earlier Work Record (stale record recovered 2026-07-10)

- Target: Continue the v1 release-readiness push by auditing the current repo state, then fixing the next concrete gap in coverage, CI, docs/book, release packaging, or hygiene; current pass has validated CI/release/docs workflows, fixed package-example lockfile drift in tests, passed the exact full repo `npm run ci` gate after the latest runtime/native-runtime/integer coverage batch, raised package-manager/runtime/native-runtime/integer coverage, trimmed unused runtime task-join scaffolding, fixed exact integer-to-float conversion for saturating wide integer casts, raised LSP statements/functions/lines to enforced 100%, raised the LSP branch gate to 97%, then closed the remaining LSP fallback-analysis branch gaps and raised the LSP coverage gate to enforced 100% across statements/branches/functions/lines, removed the stale tracked LSP coverage summary artifact from the maintained surface, raised the compiler coverage gate from `80/82/80` to `81/83/81`, added public compiler-surface coverage for escape diagnostics, call arity diagnostics, builtin member mutability metadata, stdout-sink wrappers, builtin `from` imports, imported-function entrypoint handling, and lexer escape/f-string/float edge paths, fixed imported parameterized `main` handling so imported functions are not treated as entrypoints, fixed a runtime-scheduler lost-wakeup race exposed by the compiler coverage gate, passed the exact full repo `npm run ci` gate after those fixes, revalidated compiler coverage after the duplicate builtin import regression, added focused builtin file/network/process member metadata and binding coverage, raised `call.rs` to 99.48% line coverage, added no-manifest symlink import escape coverage and parser edge coverage, raised `parser.rs` to 97.98% line coverage, raised `runtime_value.rs` source-type/wrapper coverage to 78.25% line coverage, raised `native_runtime.rs` resource metadata, cleanup/diagnostic guard, direct opcode wrapper, direct resource type-match/metadata, and arithmetic diagnostic coverage to 72.65% line coverage, raised `builtin_modules.rs` to 100% function / 99.92% line coverage and `integer.rs` to 96.50% line coverage, passed the exact full repo `npm run ci` gate with compiler coverage at 81.97% regions / 83.74% functions / 82.16% lines and LSP coverage at enforced 100%, raised the compiler line coverage gate to `82/83/81`, resolved the npm audit transitive `brace-expansion` advisory by updating the lockfile to 5.0.6, passed the exact full repo `npm run ci` gate with compiler coverage at 82.06% regions / 83.74% functions / 82.24% lines and LSP coverage at enforced 100%, completed a Clippy hygiene pass so the repo Clippy command is now quiet, passed full `npm run ci` again with compiler coverage at 82.07% regions / 83.77% functions / 82.22% lines and LSP coverage at enforced 100%, strengthened `check:clippy` to fail on all warnings with `-D warnings`, passed full `npm run ci` again under that stricter lint gate with compiler coverage at 82.06% regions / 83.77% functions / 82.22% lines and LSP coverage at enforced 100%, fixed the incorrect integer-to-nonnumeric runtime cast diagnostic, added direct-codegen named builtin argument binding coverage and runtime-value resource source-type/wrapper coverage, passed `npm run coverage:compiler:check` at 82.20% regions / 84.22% functions / 82.41% lines, and is continuing through compiler coverage/readiness gaps.
- Latest verified status: added semantic member-call success coverage for the maintained String, Vec, Map, Set, Queue, Task, and `fs.File` builtin method surfaces, then regenerated `target/compiler-coverage.json`, `target/compiler-coverage.txt`, `target/compiler-coverage.lcov`, and `target/compiler-coverage-missing.txt`. The focused semantic test, full serialized `npm run coverage:compiler:check`, exact coverage floor report, `cargo fmt --all --check`, and `git diff --check` pass; `coverage:compiler:check` is now raised to lines/functions/regions `96.01/96.71/93.94`. Current exact compiler coverage is 93.9438% regions / 96.7186% functions / 96.0184% lines, with 3 remaining llvm-cov mismatched-function warnings.
- Previous coverage checkpoint: extended MIR runtime collection helper coverage, analysis source-range and enum inference coverage, package resolver coverage, MIR receiver-mutation detection, defensive MIR builtin collection/queue return-type fallbacks, MIR pattern/function/specialization fallbacks, imported namespace aggregate-map fallback resolution, nested borrow-mut vector return redirection, runtime-value WebSocket host-header edge coverage, runtime-value task-group wake-flag registration coverage, lightweight scheduler external-event fd polling coverage, Rustls WebSocket raw-fd/nonblocking coverage, lightweight scheduler completion/waiter/unbounded-wait coverage, runtime-value process-pipe stderr read plus closed-pipe edge coverage, runtime-value HTTP bad-request/root-path/split-body stream coverage, MIR mutable member-call receiver/borrowed-param writeback coverage, MIR channel `get_or*` / internal queue-iteration member helper coverage, maintained direct-codegen object emission for supported IO/process/network examples, direct native runtime queue wrapper coverage for closed/nonblocking/timeout channel paths, direct native runtime diagnostic coverage for invalid arg buffers, cleanup registrations, queue receivers/timeouts, wait timeouts, and task-group receiver types, native-codegen cleanup-place type resolution coverage for receivers, params, locals, inferred values, unknown-field diagnostics, native-codegen opaque file/process member success-surface coverage, runtime-value lightweight scheduler missing-result defensive-exit coverage, MIR task `result_or_none` / `result_or` nonblocking shortcut coverage for ready, cancelled, and already-cancelled runtime paths, MIR process builtin spawn-failure, timeout, and cancelled-context coverage, MIR filesystem/network builtin `Result.Err` coverage for write, directory, open, connect, listener, UDP, and Unix-socket errors, MIR process-child timeout, cancellation, `wait_or_none`, `wait_ok`, kill, terminate, close, and unsupported-method coverage, MIR process-supervisor start/default-argument, duplicate-name, event wait, empty `wait_or_none`, stop/close, and cancellation coverage, and MIR process-supervisor explicit optional-argument plus `wait_or_none(Some(event))` coverage; regenerated `target/compiler-coverage.json`, `target/compiler-coverage.txt`, `target/compiler-coverage.lcov`, and `target/compiler-coverage-missing.txt`; `cargo fmt --all` passes; the focused MIR, runtime-value, native-codegen, and native-runtime tests pass; and `npm run coverage:compiler:check` passes under the newly ratcheted lines/functions/regions gate `94.33/92.53/92.70`. Current exact compiler coverage is 92.7024% regions / 92.5392% functions / 94.3326% lines, with 3 remaining llvm-cov mismatched-function warnings.
- Current coverage checkpoint: extended MIR runtime helper/operator coverage, native-runtime cleanup/task-boundary coverage, Unix/TLS test socket-path hygiene, runtime-value lightweight-scheduler edge coverage, package graph/cache edge coverage, integer helper simplification, analysis recovery no-progress coverage, parser closure flattening and public lex-error coverage, native-codegen fallback/binder coverage, semantic checker coverage across command/byte/header/timeouts/range/type-substitution helpers, native-codegen positional wait, HTTP bytes timeout, match/branch, receiver-helper, scalar-coercion, boolean-lowering edges, runtime-value process-pipe/process-child/HTTP parser/WebSocket host plus package cache-root edge coverage, native-codegen required-argument/helper thunk coverage, direct cleanup-pop diagnostics, builtin enum payload/type helper coverage, semantic builtin argument helper coverage, native-codegen lookup/default helper closure cleanup, runtime mutex/opaque-value guard closure cleanup, MIR stdout-lock poison-recovery closure cleanup, MIR duration diagnostic edge coverage, MIR runtime helper closure cleanup, runtime-value condvar poison-recovery coverage, semantic grouped/negated const-bool loop coverage, semantic task-start callable-resolution coverage, semantic member-object type-resolution coverage, semantic module namespace/task-call wrapper coverage, semantic borrowed-return diagnostic coverage, semantic generic-enum/module-qualified member fallback coverage, semantic direct plural specialization coverage, MIR inference fallback coverage, and semantic builtin member-call success-surface coverage. Regenerated `target/compiler-coverage.json`, `target/compiler-coverage.txt`, `target/compiler-coverage.lcov`, and `target/compiler-coverage-missing.txt`; `cargo fmt --all`, the focused semantic/native-codegen/runtime-value/package/parser/MIR-runtime/MIR inference tests, full `npm run coverage:compiler:check`, and exact coverage floor reports pass; the focused coverage-only `native_runtime_ffi` target passes from the prior resource-wrapper batch; the normal non-coverage `native_runtime_ffi` target compiles to zero tests; and the compiler coverage floor is report-verified at lines/functions/regions `96.01/96.71/93.94`. Current exact compiler coverage is 93.9438% regions / 96.7186% functions / 96.0184% lines, with 3 remaining llvm-cov mismatched-function warnings.

## In Progress

- Stabilize the frozen 0.1 technical-preview surface through compatibility fixes, parity regressions, and preview-user feedback rather than adding more language syntax.
- Keep the categorized example library, manual, and `tutorials/` synchronized with the implemented language subset whenever behavior changes.
- Preserve the Batch 2 checkpoint compiler lines/functions/regions floor at `96.07/96.81/94.29` and the LSP at 100%; add behavior-focused regression tests without treating marginal compiler coverage growth as the product roadmap.

## Todo

- V6 is complete: the narrow-width range check was halved and both measurements are retained in `benchmarks/direct_integer_loops/README.md`. The remaining lead is narrow-width arithmetic in its own width, which is a backend representation change.
- In Batch 3 frame work, add native direct-backend Aurora call-chain and task-ancestry backtraces, then remove the temporary parity normalization for the three supplemental MIR backtrace note families; primary trap code/message/span parity remains mandatory meanwhile.
- In the same Batch 3 frame work, replace the current flat prose call-chain/task-ancestry `notes` entries with explicit structured frame lists in the diagnostic schema and its CLI/LSP bridges.
- Publish signed 0.1 preview archives for every supported platform after the release workflow has passed on each target.
- Use the host-array / tensor-lite layer as the next ML systems milestone, starting with a small dtype and shape surface before tensor or accelerator syntax.
- Expand control-plane serialization and networking from the current honest baseline only when real agent-service examples require nested schemas, pooling, redirects, HTTP/2, or server-side TLS.

## Done

- Fixed the reported VS Code completion crash on temporarily incomplete function parameter annotations: proved the installed 0.1.0 VSIX contained a stale language-server bundle, added a real stdio protocol regression for the exact editing state, bumped and rebuilt extension 0.1.1 with the current compiler-backed server, force-installed that package locally, and documented the exact compiler build, VSIX packaging, installation, and reload workflow. The 57-test LSP suite, 100% LSP coverage gate, extension checks, 9-test extension suite, packaged-server regression, and installed-server regression all pass.
- Corrected `process.Completed.stdout()` and `stderr()` invalid-UTF-8 traps to the runtime I/O band `AU4005` on both MIR and direct backends; the parity-focused regression executes both methods through both products and pins the unchanged primary message.
- Added MIR runtime trap backtraces with function names and source spans, exact TaskGroup child entry and spawn-site ancestry, and once-only diagnostic annotation; focused compiler and `aura run` regressions pin both structured notes and human rendering. Native backtraces remain explicitly deferred to the Batch 3 frame work, with the forced parity gate temporarily ignoring only the three supplemental MIR note prefixes while still requiring exact primary trap parity.
- Closed Batch 1 P3's previously untracked Queue/Task trait-dispatch parity gap through the preferred contained path: MIR runtime member dispatch now falls back to the sema-resolved user trait implementation for non-builtin `Queue[T]` and `Task[T]` member names, while generic run-pass fixtures keep both handles in the forced MIR/direct parity matrix; also recorded P4's intentional parameter-versus-loop `own` spelling asymmetry in ADR-0006 and the normative Manual.
- Completed the July 13 ratified trust-recovery Phase 1 tickets 1-8: recorded accepted ADRs for D1-D13; added forced-MIR/forced-direct runtime-fixture parity with fallback disabled; implemented contextual `None` and unit equality; contained non-copy borrowed returns; replaced dotted semantic places with root-plus-projection paths; isolated direct-runtime call depth, diagnostics, cancellation fallback, and cleanup state per task with a 1,000-suspended-task regression; moved DNS/connect setup to the bounded blocking service under one deadline; removed environment spoofing from `sys.args()`; corrected runtime, architecture, tutorial, example, and manual claims; independently reviewed and fixed nested-`None`, operator-trait borrowed-return, projected-borrow sibling, diagnostic-ordering, and direct generated-stack unwind regressions; measured the Phase 1.5 migration surface and confirmed `own` is cleanly reservable; and passed the exact full `npm run ci` gate at 96.02% compiler lines / 96.90% functions / 93.96% regions and 100% LSP coverage.
- Completed the July 12-13 language-reference pass: established the Manual as the normative Aurora 0.1 specification; added the complete grammar, names/scopes, static semantics, execution model, diagnostics, and conformance chapters; expanded the declaration, ownership, package, CLI, runtime, limit, and API contracts enough to derive a future language book; added the reference-integrity CI gate; corrected stale Learn/tutorial/backend/API claims; fixed parser, checker, bounded-read, metrics, and hover-contract defects exposed by the audit with unit, fixture, MIR, and direct-backend regressions; and passed the exact full `npm run ci` gate at 96.05% compiler lines / 96.87% functions / 93.95% regions and 100% LSP coverage.
- Completed the July 10 directions 1-5 pass: froze the 0.1 syntax and compiler coverage floor; established a relocatable technical-preview release, CI, documentation, and hygiene surface; added parity, fuzz, scheduler-model/stress, sanitizer, audit, and benchmark safety gates; replaced per-request LSP compiler processes with a persistent, cancellable, dependency-aware service and a small lexical recovery layer; added the ML/agent control-plane foundations (`sys`, `path`, JSON/TOML string maps, logs/traces/metrics, HTTPS/chunked HTTP, and `new`/`fmt`/`test` workflows); fixed all parity and TLS-close regressions discovered by the new gates; eliminated llvm-cov ABI map collisions without changing shipped symbols; and passed the exact full `npm run ci` gate at 96.05% compiler lines / 96.86% functions / 93.94% regions and 100% LSP coverage.
- Finished the April 24 book correctness pass: validated the external first-time-developer review against the VitePress book; removed invalid call-site `` / `mut ` from examples; collapsed runnable Aurora calls and collection literals to current single-line syntax; replaced fragile short-form `Some` / `None` examples with qualified `Option.Some` / `Option.None`; rewrote top-level `try` snippets into function or match shapes; corrected `Vec.insert` / `Vec.swap` contracts; expanded install, current limits, homepage positioning, detached-task wording, and syntax-highlighting tags; and reverified with representative snippet run/checks, `npm run docs:build`, `npm audit --audit-level=moderate`, and `git diff --check`.
- Finished the April 24 native trap-parity follow-up: validated the native direct-backend divergences where cleanup traps replaced the original body trap diagnostic and recursive `with` frames unwound one extra cleanup compared with `aura run`; added failing-first CLI regressions; fixed the direct runtime to preserve the primary runtime diagnostic while draining cleanup and to skip the saturated recursion-depth cleanup registration; and reverified with focused direct-backend cleanup/recursion tests, `cargo fmt --all --check`, and `git diff --check`.
- Finished the April 24 VitePress book depth pass: rewrote the Aurora book toward deeper, human-written language documentation with a stronger home page, a project-driven Learn track, expanded ownership/data modeling/collections/concurrency/I/O/package case-study lessons, richer process/log/worker-pool case studies, and contract-style Manual pages for types, functions, classes, ownership, collections, concurrency, I/O, filesystem, networking, process, packages, CLI/tooling, and the full API index; scrubbed the rendered docs and related proposal text of "new language" framing; corrected stale CLI/API claims such as `run-mir` and `WebSocketListener.close`; and reverified with `npm run docs:build`, `npm audit --audit-level=moderate`, `git diff --check`, `cargo run -p aura -- help`, and a local preview smoke test at `http://127.0.0.1:5173/`.
- Finished the April 24 VitePress book pass: added a maintained VitePress documentation book under `docs/` with a use-case-driven Learn track, a Python-docs-style Manual/API reference track for the current Aurora surface, local search, navigation/sidebar configuration, docs scripts, root README guidance, and clean package metadata; pinned the docs toolchain to the VitePress 2 alpha line to avoid the stable Vite/esbuild audit advisory; and reverified with `npm run docs:build`, `npm audit --audit-level=moderate`, `git diff --check`, and a local preview smoke test at `http://127.0.0.1:5173/`.
- Finished the April 24 post-Round-8 regression fix pass: fixed `for value in queue:` without an active `with TaskGroup` so standalone `TaskGroup()` producers are registered with queues they receive as arguments and queue iteration waits for those producers instead of exiting immediately; fixed direct-backend `with` cleanup registrations so mutated resources are refreshed before callee-propagated traps unwind; added focused CLI regressions for both bugs; and reverified with formatting, compiler check, compiler fixtures, the full compiler lib suite, focused queue/cleanup CLI tests, and the full serialized aura CLI suite.
- Finished the April 24 Round 8 review fix pass: validated and fixed native direct-backend `with` cleanup for callee-propagated runtime traps and recursion-limit traps, zero-producer `Queue[T]` iteration shutdown, `process.Completed.stdout_bytes()` / `stderr_bytes()` short-form `Some` / `None` match inference, non-empty `{...}` Set literals with compiler and LSP fallback support, and the maintained set examples/tutorial text; added focused CLI/compiler/LSP regressions; and reverified with compiler fixtures, the compiler lib/integration suite, the serialized aura CLI suite, LSP tests/checks, `cargo fmt --all --check`, `git diff --check`, and Clippy correctness.
- Finished the April 23 Round 7 review fix pass: validated and fixed the remaining reported defects around direct-backend `with` cleanup on runtime traps, clean-return queue iteration wakeups, annotated empty Set literals, direct recursion diagnostics, streamed `aura run` stdout before external termination, and raw `process.Completed.stdout_bytes()` / `stderr_bytes()` access; added focused CLI/compiler/LSP regressions; updated maintained process examples/tutorials/README text and LSP fallback metadata; and reverified with compiler fixtures, the compiler lib suite, the serialized aura CLI suite, LSP tests, `cargo check`, `git diff --check`, and Clippy correctness.
- Finished the April 23 audit hardening fix pass: fixed the reported `self` non-copy move hole, runtime-error `with` cleanup skipping, duplicate supervisor child leak, stdin editor-analysis lockfile writes, stale LSP completion test, under-validated `main` return types, MIR typed-empty collection lowering, and hyphenated package-name mismatch; added focused regressions for each defect; hardened bounded runtime reads and package git command execution with timed, drained output collection; refreshed affected examples, tutorials, LSP fallback metadata, VS Code syntax coverage, and package lock state; and reverified with focused compiler/CLI/runtime tests, `cargo test -p aura -- --nocapture`, Node LSP/extension tests and checks, `git diff --check`, and `cargo clippy -p aurora-compiler -p aura -- -D clippy::correctness`.
- Finished the April 22 latest-review follow-up pass 3: added failing-first CLI and runtime regressions for queue iteration hanging when a sibling task panics without closing the queue; taught queue-iteration receives in both MIR and direct runtimes to wake on unobserved sibling task failure through the active `TaskGroup`; added group-failure wake-flag tracking plus a missed-wake fix in the lightweight scheduler wait-registration path; tightened task-group cleanup probing so fresh child spawns settle before blocked cleanup waits are cancelled; aligned MIR/native no-timeout `Queue.get_or*` and `Task.result_or*` helpers with the documented immediate fallback semantics without scheduler-yield side effects; fixed direct-runtime fallback-value handling to clone defaults instead of consuming them; manually revalidated the new sibling-panic repro on both `aura run` and a direct-built binary; reverified the compiler fixture suite with `cargo test -p aurora-compiler --test fixtures -- --nocapture`; and reverified the focused runtime regression with `cargo test -p aurora-compiler queue_iteration_wait_wakes_for_unobserved_task_group_failure -- --nocapture`.
- Finished the April 22 latest-review follow-up pass 2: added failing-first coverage for the remaining match-expression move-tracking false positive, the queue-iteration cancellation hang, and swap out-of-bounds message parity; fixed match-expression value-scrutinee first use without reintroducing move-state leaks; removed the abandoned scope-wide task-group cancellation rewrite; added a targeted queue-iteration receive path that threads the active `TaskGroup` cancellation into `for value in queue:` for both MIR and direct backends; aligned the direct-runtime swap diagnostic with MIR; narrowed the concurrency/resource/current-surface tutorials back to the exact supported cancellation behavior; and reverified the tree with `cargo fmt --all`, the targeted CLI regressions, `cargo test -p aurora-compiler --lib -- --nocapture`, `cargo test -p aurora-compiler --test fixtures -- --nocapture`, and `cargo test -p aura --test cli -- --nocapture`.
- Finished the April 22 latest-review fix pass: validated the new review cluster around native bare-`None` coercions, wait-site inconsistencies, checker false positives, and `fs.write_bytes([], ...)` parity; fixed direct-backend coercion for bare `None` across collection literals/member calls and nested `Option[...]` class fields; aligned MIR/native no-timeout `Queue` and `Task` helper semantics with the documented immediate non-blocking behavior; made cancelled `sleep(...)` wake so tasks can observe `cancelled()`, made `wait_any([])` return `TimedOut` immediately, accepted empty byte vectors in `fs.write_bytes(...)`, removed the move-type collection-literal checker false positive, refreshed the maintained concurrency examples/tutorials, aligned the queue fairness CLI regression with the new `Task.result_or(..., timeout=...)` contract, and reverified the tree with `cargo fmt --all`, `cargo test -p aurora-compiler --lib -- --nocapture`, `cargo test -p aurora-compiler --test fixtures -- --nocapture`, and `cargo test -p aura --test cli -- --nocapture`.
- Finished the April 21-22 live eleven-defect fix pass: closed the still-live `/tmp/aurora_review` issues around unannotated `Queue.get_or_none()` / `Task.result_or_none()` match inference, `aura run` partial-stdout loss on runtime errors, `TaskGroup` scope shutdown semantics, cooperative cancellation in CPU-bound lightweight tasks, surfaced task failures via `TaskResult.Error(...)`, literal-`match` and `with` move-state leaks, the self-receiver bound-call false positive, and `Vec.insert(...)` / `Vec.swap(...)` out-of-bounds silent no-ops; added and updated failing-first compiler/runtime/CLI regressions plus maintained examples/tutorial text for the changed structured-concurrency behavior; repaired the broad compiler test harnesses that were still overflowing default test-thread stacks; and reverified the final tree with `cargo fmt --all`, `cargo test -p aurora-compiler --test fixtures -- --nocapture`, `cargo test -p aurora-compiler --lib -- --nocapture`, and `cargo test -p aura --test cli -- --nocapture`.
- Finished the April 21 twelve-finding fix audit: rechecked the specific review-finding list covering MIR fs read caps, compiler-backed LSP invalidation, UNC file URI parsing, architecture-doc concurrency syntax, stale `match mut` bindings, builtin module enum identity, malformed HTTP listener recovery, Unix/non-Unix TLS listener backlog handling, and the non-Unix TLS wait policy; confirmed the current tree already contains those fixes; and reverified them through targeted Rust and Node regression tests plus direct source inspection, so no additional production-code changes were required for that finding set.
- Finished the April 21 Claude review validation pass: replayed the external harness repros under `/tmp/aurora_review` against the current `target/debug/aura`, confirmed that the headline correctness bugs around unannotated `Option` matches from `Queue.get_or_none`, buffered stdout loss on `aura run` runtime errors, missing `TaskGroup` join-at-scope-exit behavior, non-firing `cancelled()` in CPU-bound loops, unrecoverable task runtime failures, literal-`match` and `with` move-tracking leaks, `run`/direct-backend divergence caused by the ownership hole, the self-receiver bound-call false positive, and silent `Vec.insert`/`Vec.swap` OOB no-ops still reproduce, and recorded that the task-failure claim is slightly overstated because the Aurora program terminates with a surfaced diagnostic rather than a host-process panic. 
- Finished the April 21 follow-up fix pass 3: added failing-first regressions for builtin module enum identity across `aura run` and direct-built binaries plus the non-Unix TLS listener wait policy; preserved qualified builtin enum names through sema canonicalization, MIR constructor lowering, and MIR match-pattern lowering so `io.Error.*` / `process.Error.*` round-trip consistently through construction, printing, equality, and `match`; replaced the non-Unix TLS listener fixed-sleep wait path with a readiness wait backed by `mio` plus a shared timeout-policy helper that blocks until real listener progress when the handshake queue is empty; and reverified the tree with `cargo fmt --all`, `cargo test -p aurora-compiler --lib -- --nocapture`, `cargo test -p aurora-compiler --test fixtures -- --nocapture`, and `cargo test -p aura --test cli -- --nocapture`.
- Finished the April 21 post-follow-up review: re-read the latest checker/runtime diffs after the follow-up fix pass, replayed targeted repros for builtin module enum constructors and their interaction with matching/equality, inspected the non-Unix TLS accept helper added by the shared backlog refactor, and recorded the remaining constructor-identity and non-Unix polling issues.
- Finished the April 21 follow-up fix pass 2: added failing-first regressions for stale `match mut` binding use after `mut ` helper calls and module-qualified builtin `io.Error` constructors; invalidated overlapping `match mut` bindings from actual `mut ` call sites without reintroducing dead-branch writeback fallout; unified builtin module-type canonicalization so `io.Error.NotFound` type-checks as `io.Error`; reworked `TlsListenerValue::accept()` onto the shared pending-handshake queue so the non-Unix branch no longer keeps the old inline one-peer-at-a-time handshake path; and reverified the tree with `cargo fmt --all`, `cargo test -p aurora-compiler --lib -- --nocapture`, `cargo test -p aurora-compiler --test fixtures -- --nocapture`, and `cargo test -p aura --test cli -- --nocapture`.
- Finished the April 21 follow-up review: re-read the current post-fix checker/runtime tree, replayed targeted repros against `match mut` mutation-through-call semantics and module-qualified builtin enum constructors, confirmed broad compiler and CLI suites still pass, and recorded the remaining uncovered issues around stale bindings after `mut ` calls, module-qualified enum type identity, and the still-inline non-Unix TLS accept path.
- Finished the April 21 review-finding fix pass: added failing-first compiler/runtime/CLI regressions for the remaining stale `match mut`, TLS accept backlog, and malformed HTTP listener defects; fixed checker-side stale pattern-binding invalidation without regressing dead branches; reworked the TLS listener accept loop so queued stalled peers no longer linearly delay the next valid client while preserving in-runtime scheduler progress; made malformed HTTP requests return `400 Bad Request` and continue the listener loop; updated the supporting compiler test helpers and fixture expectations; and reverified the final tree with `cargo fmt --all`, `cargo test -p aurora-compiler --lib -- --nocapture`, `cargo test -p aurora-compiler --test fixtures -- --nocapture`, `cargo test -p aura --test cli -- --nocapture`, plus direct targeted example/runtime regressions.
- Finished the April 21 post-fix review of the fifth-pass change set: re-read the landed compiler/runtime diffs, replayed targeted adversarial repros against the current tree, and recorded the remaining semantic and listener-path issues that still survive after the fifth-pass fixes.
- Finished the April 21 fifth-review fix pass: validated the fifth-pass external review, added failing-first regressions, fixed the broad `match mut` dead-branch writeback regression, corrected direct-backend bare `None` enum emission, made TLS and HTTP listeners continue past per-connection handshake/request failures, raised the maintained `read_all` ceiling to `64 MiB`, added `431` handling, enabled `Self` in trait/impl parameter positions, restored user-class precedence over builtin variant names, added `io.Error.Cancelled` plus explicit `io.Error.Closed`/`Cancelled` runtime mapping, hardened websocket transport fallback errors, updated the maintained traits and I/O tutorials plus examples, and reverified the final tree with `cargo fmt --all`, `cargo test -p aurora-compiler --lib -- --nocapture`, `cargo test -p aurora-compiler --test fixtures -- --nocapture`, `cargo test -p aura --test cli -- --nocapture`, and `./target/debug/aura run examples/traits/self_parameters.au`.

- Finished the April 20 fourth-review fix pass: closed the fourth-pass externally reviewed defects across recursive indirect enum construction, nested generic trait-bound direct-backend dispatch, `match mut` writeback and nested-aliasing holes, managed `with` resource field moves, MIR/runtime filesystem read-cap parity, supertrait syntax and inherited bounds, `Option.Some(...)` inference, expression-form `match` positions, unreachable enum-arm detection, nested missing-pattern diagnostics, TLS handshake deadline handling, oversized HTTP request `413` responses, websocket error-kind preservation, compiler-backed LSP cache invalidation, and UNC `file://` URI handling; added failing-first compiler, runtime, CLI, and LSP regressions for those paths; aligned the maintained architecture docs, tutorials, and examples with the fixed surface; and reverified the final tree with `cargo fmt --all`, `cargo test -p aurora-compiler --lib -- --nocapture`, `cargo test -p aurora-compiler --test fixtures -- --nocapture`, `cargo test -p aura --test cli -- --nocapture`, `npm test`, `npm run check`, and direct `aura run` smoke runs for the new examples.

- Finished the April 20 third-review fix pass: closed the externally reviewed third-pass correctness and soundness defects around `match mut` rebinding, nested borrow-vs-move aliasing through sibling expression ordering, live Unix-socket listener hijacking, inferred generic-class field arithmetic in MIR, native trait-specialization order dependence, nested-pattern exhaustiveness over the same outer variant, annotation-directed `Option.None` resolution, direct-backend filesystem read caps, TLS server handshake completion and timeout handling, stricter HTTP header validation, and supervisor restart-loop throttling; added failing-first checker/runtime/CLI regressions for the new ownership, match, process, TLS, HTTP, direct-backend, and inference cases; updated the maintained I/O tutorial for the restart-backoff and TLS handshake behavior; and reverified the final tree with `cargo test -p aurora-compiler`, `cargo test -p aura`, `npm run test:lsp`, `npm run check:extension`, and `cargo clippy -p aurora-compiler -p aura -- -D clippy::correctness`.

- Finished the April 20 second-review fix pass: closed the second-pass externally reviewed correctness and soundness defects around inferred enum `match` fallthrough, nested consume-plus-borrow aliasing, reassignment during borrowed iteration, `net.unix_listen(...)` regular-file clobbering, generic-class field arithmetic in MIR, namespace-qualified enum variants, imported-module syntax diagnostic attribution, `match mut` writeback, duplicate nested match-arm discrimination, direct-backend multi-payload enum support, finite-only float parsing, builtin shadowing rejection, and the lightweight-task stack/runtime regressions that were crashing websocket and Unix/TLS examples; restored the maintained 256-frame recursion contract; updated the affected runtime/sema/analysis/MIR/native tests and maintained diagnostics; and reverified the final tree with `cargo test -p aurora-compiler`, `cargo test -p aura`, `npm run test:lsp`, `npm run check:extension`, and `cargo clippy -p aurora-compiler -p aura -- -D clippy::correctness`.
- Finished the April 20 review-hardening fix pass: closed the externally reviewed ownership, concurrency, parser/runtime, HTTP/process, and I/O/networking defects end to end; added failing-first regressions for consume-plus-borrow call arguments, borrowed-vector iteration mutation, explicit non-copy vector indexing, overlapping trait impl ambiguity, large left-associative expression chains, blocked `TaskGroup` cleanup, sleep cancellation propagation, queue fairness, HTTP header injection, large TCP/HTTP payload handling, `read_all` caps, filesystem directory error precision, websocket runtime stability, recursion-depth diagnostics, and the updated compiler-bridge editor surface; fixed the checker, parser, MIR runtime, direct runtime, native backend, lexer, test harness stack sizing, websocket handshake path, and docs/examples to match the hardened semantics; and reverified the finished tree with `cargo fmt --all`, targeted compiler/CLI regressions, `cargo test -p aurora-compiler`, `cargo test -p aura`, `npm run test:lsp`, `npm run check:extension`, and `cargo clippy -p aurora-compiler -p aura -- -D clippy::correctness`.
- Finished the April 19 process-supervisor pass: added the maintained `process.supervisor()` surface with `process.Supervisor`, `process.RestartPolicy`, `process.SupervisorEvent`, and `process.SupervisorWait`; implemented named supervised child processes with restart policy, restart backoff, max-restart limits, and group-aware shutdown across the shared runtime, MIR runtime, direct runtime, and direct backend; added compiler typing plus direct regressions for supervised restart and stop behavior; updated CLI direct-backend product coverage, fallback LSP metadata/completions/return-type inference, the maintained supervisor example, the I/O and current-surface tutorials, the root and CLI READMEs, and the examples index; and reverified the final tree with `cargo fmt --all`, `cargo test -p aurora-compiler --test process`, `cargo test -p aura direct_backend_build_supports_process_module_surface -- --nocapture`, `npm run test:lsp`, `cargo test -p aurora-compiler`, `cargo test -p aura`, `npm run check:extension`, and `cargo clippy -p aurora-compiler -p aura -- -D clippy::correctness`.
- Finished the April 19 process-groups pass: added maintained `group=true` support to `process.start(...)` and `process.run(...)`; implemented Unix process-group creation plus group-aware `kill()`, `terminate()`, and `close()` semantics in the shared runtime child lifecycle; made grouped child cleanup wait for the full process group to disappear before returning; added a regression that proves grouped `close()` tears down descendant processes rather than only the leader PID; threaded the new argument through MIR execution, direct native execution, direct-codegen lowering, CLI integration coverage, examples, tutorials, root/example READMEs, and fallback LSP metadata/tests; and reverified the final tree with `cargo fmt --all`, `cargo test -p aurora-compiler`, `cargo test -p aura`, `npm run test:lsp`, `npm run check:extension`, and `cargo clippy -p aurora-compiler -p aura -- -D clippy::correctness`.
- Finished the April 19 Pythonic convenience API pass: added `Queue.get_or_none(...)`, `Queue.get_or(...)`, `Task.result_or_none(...)`, `Task.result_or(...)`, `process.Child.wait_or_none(...)`, `process.Child.wait_ok(...)`, and `process.Completed.check()` across the checker, MIR runtime, direct runtime, native backend, compiler analysis/completions, and fallback LSP analysis; added failing-first compiler fixtures and process regressions for the new queue/task/process helpers; rewrote the maintained concurrency and process examples plus the concurrency/I/O tutorials to lead with the new linear helper style while keeping `QueueReceive`, `TaskResult`, and `process.Wait` documented as the lower-level surface; updated the root README and examples index to describe the new default queue/task/process style; and reverified the finished tree with `cargo fmt --all`, `cargo test -p aurora-compiler`, `cargo test -p aura`, `npm run test:lsp`, `npm run check:extension`, and `cargo clippy -p aurora-compiler -p aura -- -D clippy::correctness`.
- Finished the April 19 process-module pass: added the first maintained shell-free `process` builtin module with `process.start(...)`, `process.run(...)`, `process.inherit()`, `process.null()`, `process.pipe()`, `process.Child`, `process.Pipe`, `process.ExitStatus`, `process.Completed`, and `process.Error`; implemented timeout-aware child waiting plus explicit `terminate()` / `kill()` / `close()` behavior across the checker, MIR runtime, direct runtime, native backend, compiler-owned analysis/completions, and LSP fallback analysis; added maintained compiler, CLI, example-smoke, and LSP regression coverage for subprocess execution, stdio piping, and builtin member completions; added the runnable `examples/io/process_run.au` and `examples/io/process_pipes.au` examples; aligned the root README, CLI README, examples index, and tutorials with the new process surface and its current limits; and reverified the final tree with `cargo fmt --all`, `cargo test -p aurora-compiler`, `cargo test -p aura`, `npm run test:lsp`, `npm run check:extension`, and `cargo clippy -p aurora-compiler -p aura -- -D clippy::correctness`.
- Finished the April 19 concurrency Pythonic surface reset: removed the legacy `spawn`, `spawn detached`, `select`, `after(...)`, `queue()`, `queue[T]()`, and `tasks()` surface; kept only the structured concurrency model centered on `Queue[T]()`, `TaskGroup()`, `TaskGroup.start(...)`, `TaskGroup.start_soon(...)`, `Task.result(timeout=...)`, `wait_any(...)`, and `wait_all(...)`; renamed and rewired maintained fixtures/examples around the new queue/task semantics; updated the fallback LSP analysis metadata and payload inference to the maintained `QueueReceive`, `TaskResult`, `WaitAny`, and `WaitAll` enums; aligned tutorials, READMEs, and VS Code syntax/snippets with the new surface; and reverified the final tree with `cargo fmt --all`, `cargo test -p aurora-compiler`, `cargo test -p aura`, `npm run test:lsp`, `npm run check:extension`, and `cargo clippy -p aurora-compiler -p aura -- -D clippy::correctness`.
- Refined the April 19 ML systems roadmap so the near-term plan now centers Aurora on subprocess supervision, structured serialization, observability, and a host-side array or tensor-lite layer for NumPy-style local data processing, while scoping full tensor/device placement and distributed runtime support as explicit later phases in the same roadmap.
- Finished the April 19 ML systems roadmap pass: added `docs/ml_systems_support_plan.md` as a forward-looking plan for making Aurora a strong ML systems language without replacing Python training workflows, covering process supervision, tensor/device handle interop, zero-copy/shared-memory transport, structured serialization, observability, cross-cutting compiler/runtime implications, and staged delivery milestones; linked the roadmap from the root README; verified the new markdown links; and recorded the pass in the dated work log.
- Finished the April 19 async file I/O and bounded-queues pass: added bounded `Queue[T]` capacity support with scheduler-aware blocked send wakeups, cancellation-aware `SendError.Cancelled(value)`, and shared send-readiness handling across the MIR runtime and direct backend; routed maintained file I/O through the lightweight-task scheduler via the blocking-I/O pool so ordinary file reads and writes no longer pin a scheduler task on a blocking host thread; added maintained regressions for bounded-queue blocking and scheduler-friendly FIFO file reads; added the runnable `examples/concurrency/bounded_queue.au` example; aligned fixtures, examples, tutorials, root/CLI READMEs, compiler smoke coverage, and LSP fallback docs with `queue(capacity=...)`, `SendError.Cancelled(...)`, and scheduler-aware file I/O; and reverified the final tree with `cargo fmt --all`, `cargo test -p aurora-compiler`, `cargo test -p aura`, `npm run test:lsp`, `npm run check:extension`, and `cargo clippy -p aurora-compiler -p aura -- -D clippy::correctness`.
- Finished the April 19 lightweight-tasks runtime pass: replaced MIR and direct-runtime task spawning so Aurora tasks now run on the shared coroutine scheduler instead of one OS thread per task; added scheduler task-local cancellation propagation for the direct runtime; changed the direct native main wrapper to execute through `aurora_direct_run_root(...)`; added maintained regressions for thousands-of-tasks thread-count scaling and preserved recursion-limit diagnostics on the coroutine runtime; aligned the maintained concurrency and I/O tutorials plus example index with the scheduler-backed task model; and reverified the final tree with `cargo fmt --all`, `cargo test -p aurora-compiler`, `cargo test -p aura`, `npm run test:lsp`, `npm run check:extension`, and `cargo clippy -p aurora-compiler -p aura -- -D clippy::correctness`.
- Finished the April 19 async scheduler and HTTP runtime pass: replaced the remaining `sleep(...)`, queue-wait, and `select` polling paths with the shared runtime scheduler; routed the maintained HTTP listener/request helpers onto the same nonblocking evented runtime as the rest of networking; fixed select-cancellation semantics in both MIR and direct runtime paths so cancelled waits fall through promptly instead of waiting for timeout arms; added targeted regressions for scheduler wakeups, nonblocking HTTP resource invariants, and select cancellation; aligned the maintained tutorials/README surface with the new scheduler-backed model; and reverified the final tree with `cargo fmt --all`, `cargo test -p aurora-compiler`, `cargo test -p aura`, `npm run test:lsp`, `npm run check:extension`, and `cargo clippy -p aurora-compiler -p aura -- -D clippy::correctness`.
- Finished the April 19 architecture documentation pass: reviewed the full Aurora monorepo and added a new `architecture_docs/` documentation set covering the system architecture, AST/source model, lexer, parser, semantic analysis, MIR, MIR runtime, native backend/runtime, package system, CLI/build tooling, editor tooling, testing strategy, and an end-to-end walkthrough, including Mermaid diagrams plus standalone SVGs for the compiler pipeline, runtime layering, and tooling flow; linked the new docs from the root README; verified the markdown links in the new docs; and recorded the work in the dated work log.
- Finished the April 19 evented networking runtime pass: converted the maintained socket-backed runtime onto nonblocking descriptors plus poll-driven waits, fixed websocket accept/connect handshake resumption on nonblocking sockets, made timeout handling honor the caller’s full budget instead of a single poll slice, tightened TLS socket polling so handshake progress can wait on both read and write readiness, added direct runtime regressions for nonblocking descriptor invariants plus timeout-budget coverage, updated the maintained READMEs/tutorials to describe the new socket model accurately, and reverified the final tree with `cargo fmt --all`, `cargo test -p aurora-compiler`, `cargo test -p aura`, `npm run test:lsp`, and `npm run check:extension`.
- Finished the April 18-19 networking expansion/stabilization pass: expanded the maintained `io`/`fs`/`net` surface from the initial blocking file/TCP subset to the richer blocking runtime that now covers byte-oriented file and socket I/O, timeout-aware TCP/Unix/TLS/HTTP/WebSocket operations, UDP, Unix sockets, and TLS; filled the compiler-backed builtin-module completion gap for the new resource members; made the maintained Unix/TLS example self-contained with embedded certificate material; stabilized network example timeouts plus the WebSocket accept/handshake path under full-suite load; removed the dead networking helper/runtime warning leftovers; and reverified the final tree with `cargo fmt --all`, `cargo test -p aurora-compiler`, `cargo test -p aura`, `npm run test:lsp`, and `npm run check:extension`.
- Finished the April 18 I/O and network surface pass: added maintained builtin `io`, `fs`, and `net` modules; introduced `io.Error`, `fs.File`, `net.TcpListener`, and `net.TcpStream`; wired blocking file and TCP I/O through the checker, MIR runtime, native direct backend, public compiler entrypoints, CLI product tests, and language-server analysis/completion surface; added maintained examples plus the `19-io-and-networking` tutorial chapter; aligned root/CLI/tutorial/example documentation with the implemented builtin I/O model; and reverified the final tree with `cargo fmt --all`, `cargo test -p aurora-compiler`, `cargo test -p aura`, `npm run test:lsp`, and `npm run check:extension`.
- Finished the April 18 concurrency surface removal pass: removed the remaining compatibility-era concurrency spellings from the checker and tooling so `Channel[T]`, `channel()`, `task_group()`, `Task.join()`, `Queue.send()/recv()/clone()`, `Task.clone()`, and `TaskGroup.spawn(...)` are no longer part of the maintained surface at all; converted stale positive fixtures, examples, tutorials, and LSP fallback tests to the queue/task-only model; renamed maintained example and fixture stems from `channel*`/`channels*` to `queue*`/`queues*` everywhere except explicit negative regressions for removed aliases; and reverified the compiler, CLI, LSP, and extension checks on the final queue/task-only tree.
- Finished the April 17 concurrency ergonomics redesign: added the maintained `Queue[T]` / `queue()` / `tasks()` / `Task.result()` / `TaskGroup.start(...)` surface as compatibility-first aliases over the existing concurrency runtime, made queue/task handles cheap copy-like values, added `Queue.get(timeout=...)`, updated MIR/direct backend/tooling support for `put` / `get` / `result` / `start`, refreshed the maintained concurrency examples and tutorials to present queues and structured tasks as the primary model, updated regression fixtures and compiler/LSP coverage for the new surface, and verified the full compiler suite, CLI suite, LSP suite, and extension build checks.
- Finished the April 17 review hardening follow-up pass 4: hardened git checkouts against hostile symlinked contents and interactive credential prompts, tightened cached git revision reads with `O_NOFOLLOW`, replaced the `IntegerValue` inherent `cmp` with canonical `Ord`/`PartialOrd` implementations, capped hostile embedded MIR complexity, diagnosed MIR/deadline `Instant::checked_add` overflow instead of firing immediately, added the empty field-path direct-backend guard, documented task-group/refcount runtime invariants, removed the redundant direct-runtime acquire fence, and verified the final tree with the compiler suite, CLI suite, LSP suite, extension checks, and a clippy correctness pass.
- Finished the April 17 review hardening follow-up pass 3: marked the remaining raw-pointer FFI entrypoints as `unsafe extern "C"` with safety docs, tightened git revision validation to reject overly-short hashes, added embedded-input length caps for `aurora_native_run`, replaced the remaining `is_some_and(...).unwrap()` MIR-lowering sites, revalidated that nested pattern payload arity is already rejected recursively by maintained checker tests, and verified the result with the compiler suite, CLI suite, LSP suite, extension build checks, and a clippy correctness pass.
- Finished the April 17 review hardening follow-up pass 2: extended parser recursion guards to statements, types, patterns, and f-string interpolation parsing; kept the recursion cap at `128` intentionally because higher values hit the host stack before Aurora can diagnose; cleaned up partially-written unique temp files on failure; made runtime task/channel locks poison-tolerant; hardened git revision validation plus temp-path generation and Windows replace semantics for atomic cache/lockfile writes; added opaque refcount overflow/underflow guards; fixed float division-by-zero diagnostics to match modulo; restored thread SIGPIPE masks on broken-pipe paths without regressing clean built-binary exits; removed the remaining production `unwrap` / `expect` sites in `sema.rs` / `integer.rs`; and added regression coverage plus maintained-fixture updates for the confirmed issues.
- Finished the April 17 review hardening follow-up: added parser recursion limits and f-string nesting limits, removed the negative-literal inference panic path, validated git branch/tag selectors, made lockfile and git revision-cache writes atomic, made MIR stdout handling poison-tolerant, diagnosed float modulo by zero, switched exact integer-to-float casts to reject silent precision loss, replaced the direct runtime's Arc-based opaque retain/release with explicit atomic reference counting, tightened several checker internal-error paths, added a defensive positional/named call-binding guard, hardened malformed builtin `MapEntry` field typing, and added regression coverage across compiler, package, MIR runtime, native runtime, CLI, and editor-facing tests.

- Finished the April 16 runtime/package hardening pass: replaced direct-runtime opaque allocations with explicit retain/release support, fixed spawned-argument ownership through native thunks, hardened direct-runtime stdout handling so built binaries exit cleanly on broken pipes without global `SIGPIPE` suppression, removed unsafe borrowed UTF-8 decoding in the direct runtime, tightened MIR runtime panic/error paths, hardened git dependency resolution (`--` separation, source validation, hashed cache keys, revision markers, lockfile/version/package validation, dependency-count caps), fixed canonical import-root checks, and added regression coverage across compiler, CLI, and runtime tests.
- Removed the redundant compiler-side `run_*_via_mir` aliases left behind after interpreter removal, collapsed internal coverage onto the canonical `run_*` entrypoints plus explicit `lower_*_to_mir + run_mir(...)` where MIR-level coverage is still intentional, renamed stale CLI tests that still implied a removed `run-mir` path, and hardened git dependency checkout caching to fall back to a temp cache root when a home-directory cache is unavailable.
- Removed the tree-walk interpreter from the maintained Aurora architecture: extracted shared runtime state into `runtime_value.rs`, switched the public `run` path onto MIR, removed the `run-mir` CLI command, deleted `interpreter.rs` / `interpreter_tests.rs`, added dedicated runtime-value coverage, and aligned READMEs/tutorials/tests/work logs with the reduced two-path model (`run` via MIR, `build` via native codegen).
- Finished the April 16 major language-surface pass across compiler/runtime/tooling/docs: richer enum `match` with expression-form and nested/multi-payload patterns, float literal match cases, default trait methods, ordering traits for `<`/`<=`/`>`/`>=`, explicit borrow labels such as `` for borrowed-return lifetimes, positional class constructors, keyword enum payload arguments, bare built-in enum constructors with expected type, explicit `channel[T]()` construction, expanded `spawn`/`TaskGroup.spawn(...)` targets, and an `auto` build fallback that preserves native build coverage for richer source programs.

- Added `aura deps update` and `aura deps update <package>` so branch/tag/default-main git dependencies can be refreshed without deleting `Aurora.lock`, with direct compiler coverage, CLI product tests, and maintained README/tutorial updates for the new workflow.
- Extended the Aurora package system from local path dependencies to git-backed dependencies, with manifest support for `git`, `rev`, `tag`, and `branch`, default `main` branch fallback, lockfile-pinned git revisions, compiler/CLI/LSP regression coverage, and README/tutorial updates for the maintained package surface.
- Implemented the first Aurora package-system milestone with `Aurora.toml` manifests, manifest-rooted `src/` packages, local path dependencies, workspace roots, manifest-aware CLI/compiler entrypoints, relative `Aurora.lock` generation, maintained package examples, tutorial/README coverage, compiler/CLI regression tests, and an LSP compiler-bridge regression for package-aware analysis/completion.
- Added another direct checker/interpreter sweep covering empty-`select` validation, direct index/member assignment helper branches, runtime `main` parameter rejection, extra inferred builtin member types, invalid runtime `select` arms, additional loop-control branches, float-to-int cast overflow edges, map render/equality edges, and current-module namespace fallback resolution; verified the new focused tests and restarted a fresh full `cargo llvm-cov` summary from the updated source tree.
- Extended compiler-backed `analyze` / `complete` and the LSP from local-module behavior to fully correct cross-file definitions for imported items, including fields, methods, variants, and trait methods that resolve back to their defining source files.
- Narrowed the JS fallback so hover and go-to-definition now stay compiler-owned whenever compiler analysis succeeds, using JS only when the compiler cannot analyze the buffer.
- Extended the maintained trait surface with specialized generic trait bounds and operator traits across the checker, interpreter, MIR/runtime, direct builds, examples, tutorials, CLI coverage, and compiler/LSP regression suites.
- Raised the enforced coverage gates after new compiler and LSP regression/unit coverage: compiler to lines `67%`, functions `74%`, regions `67%`; language server to statements `89%`, branches `78%`, functions `98%`, and lines `89%`.
- Raised the enforced coverage gates again after additional fallback-helper, bridge, lexer, call-surface, and AST coverage work: compiler to lines `68%`, functions `74%`, regions `68%`; language server to statements `91%`, branches `82%`, functions `98%`, and lines `91%`.
- Raised the enforced coverage gates again after the April 14 compiler/runtime/helper sweep: compiler to lines `77%`, functions `78%`, regions `78%`; language server to statements `91%`, branches `83%`, functions `100%`, and lines `91%`.
- Expanded direct compiler coverage across `native_codegen`, `native_runtime`, `mir_runtime`, `analysis`, `sema`, `interpreter`, and runnable maintained examples, moving compiler coverage to roughly `77.47%` lines / `78.15%` functions / `78.99%` regions and keeping the LSP at `91.17%` statements / `83.69%` branches / `100%` functions / `91.17%` lines.
- Added another focused compiler helper sweep over `interpreter`, `mir_runtime`, `sema`, and `native_runtime`, moving compiler coverage to `82.45%` lines / `81.35%` functions / `83.69%` regions while the language server sits at `91.49%` statements / `84.08%` branches / `100%` functions / `91.49%` lines.
- Added another April 14 helper sweep across `analysis`, `native_codegen`, `sema`, and `interpreter`, moving compiler coverage to `83.75%` lines / `82.71%` functions / `84.95%` regions and the language server to `93.34%` statements / `86.41%` branches / `100%` functions / `93.34%` lines.
- Completed a fresh full compiler coverage run and raised the measured compiler baseline for that pass to `84.10%` lines / `82.87%` functions / `85.23%` regions while the language server moved to `94.55%` statements / `87.64%` branches / `100%` functions / `94.55%` lines.
- Added another helper sweep across `diag`, `integer`, `ast`, `call`, `lexer`, `parser`, `sema`, and `native_runtime`, then reran compiler coverage to move the compiler to `84.69%` lines / `83.37%` functions / `85.81%` regions.
- Fixed the latest `mir_runtime` helper-test imports, verified the new targeted `mir_runtime` and `sema` tests, and resumed the compiler coverage push from a green baseline.
- Added another dense validation/helper sweep in `sema`, `mir_runtime`, and `native_codegen`, then reran the full compiler coverage pass to move the compiler to `85.02%` lines / `83.54%` functions / `86.07%` regions.
- Added another helper sweep in `lib`, `lexer`, and `interpreter`, then reran the full compiler coverage pass to move the compiler to `85.11%` lines / `83.58%` functions / `86.15%` regions.
- Added another helper sweep in `interpreter`, `native_runtime`, and `native_codegen`, then reran the full compiler coverage pass to move the compiler to `85.49%` lines / `83.84%` functions / `86.54%` regions while the remaining gap stayed concentrated in `sema`, `interpreter`, `native_codegen`, and `mir_runtime`.
- Added another helper sweep in `sema`, `mir_runtime`, `interpreter`, `native_runtime`, and `native_codegen`, then reran the full compiler coverage pass to move the compiler to `85.63%` lines / `83.85%` functions / `86.65%` regions while the remaining gap stayed concentrated in `sema`, `interpreter`, `native_codegen`, and `mir_runtime`.
- Added another helper sweep in `sema`, `interpreter`, `mir_runtime`, and `native_codegen`, then reran the full compiler suite and coverage pass to move the compiler to `86.30%` lines / `84.20%` functions / `87.15%` regions while the remaining gap stayed concentrated in `sema`, `interpreter`, `native_codegen`, `mir_runtime`, and `native_runtime`.
- Added another helper sweep in `interpreter`, `sema`, `mir_runtime`, and `native_runtime`, then reran the full compiler suite and coverage pass to move the compiler to `86.44%` lines / `84.41%` functions / `87.27%` regions while the remaining gap stayed concentrated in `interpreter`, `sema`, `native_codegen`, `mir_runtime`, and `analysis`.
- Added another helper sweep in `interpreter` and `native_runtime`, then reran the full compiler suite and coverage pass to move the compiler to `86.57%` lines / `84.59%` functions / `87.39%` regions while the remaining gap stayed concentrated in `interpreter`, `sema`, `native_codegen`, `mir_runtime`, and `analysis`.
- Added another helper sweep in `analysis`, `sema`, `interpreter`, `mir_runtime`, and `native_runtime`, verified the new targeted tests plus the full `aurora-compiler` lib suite, and resumed the next full compiler coverage run from that green baseline.
- Completed that full compiler coverage run and moved the compiler to `86.78%` lines / `84.74%` functions / `87.59%` regions while the remaining gap stayed concentrated in `interpreter`, `sema`, `native_codegen`, `mir_runtime`, and `native_runtime`.
- Added another checker/runtime helper sweep in `sema`, `interpreter`, `mir_runtime`, and `native_runtime`, verified the expanded `aurora-compiler` lib suite at 213 passing tests, and reran compiler coverage to move the compiler to `86.93%` lines / `84.81%` functions / `87.71%` regions.
- Added another helper sweep in `sema`, `interpreter`, `mir_runtime`, `native_runtime`, and `native_codegen`, verified the expanded `aurora-compiler` lib suite at 214 passing tests, and reran compiler coverage to move the compiler to `87.07%` lines / `84.82%` functions / `87.79%` regions.
- Added another dense runtime/member and helper sweep in `lib`, `native_codegen`, and `mir_runtime`, including a runtime member matrix across `String` / `Vec` / `Map` / `Set` / `Channel` / `Task` / `TaskGroup`, direct thunk helper coverage, and more MIR operator/task helper coverage; reran the expanded `aurora-compiler` lib suite at 216 passing tests and moved compiler coverage to `87.54%` lines / `84.83%` functions / `88.35%` regions.
- Added another helper sweep across `sema`, `interpreter`, `mir_runtime`, `native_codegen`, and `lib`, covering builtin enum-constructor hints, literal-pattern rendering, trait-bound lookup helpers, more MIR operator/task branches, direct thunk helpers, and a dense runtime member matrix compiled through both execution paths; reran the expanded `aurora-compiler` lib suite at 220 passing tests and moved compiler coverage to `88.06%` lines / `85.22%` functions / `88.98%` regions.
- Added another helper sweep across `interpreter`, `mir_runtime`, and `native_codegen` to cover callable-default evaluation, borrowed writeback and spawnability helpers, and more direct type-parameter / opaque-fallback lowering paths; reran the expanded `aurora-compiler` lib suite at 223 passing tests and moved compiler coverage to `88.16%` lines / `85.29%` functions / `89.04%` regions.
- Added a denser runtime/codegen matrix across `lib.rs` and `native_codegen.rs` to drive borrow-mut writebacks, named `range(...)`, `select` arms, cleanup resources, and spawn/task paths through interpreter, MIR runtime, and direct backend compilation; reran the expanded `aurora-compiler` lib suite at 225 passing tests and moved compiler coverage to `88.19%` lines / `85.30%` functions / `89.05%` regions.
- Added another direct-path helper sweep across `analysis.rs`, `interpreter.rs`, `mir_runtime.rs`, `native_codegen.rs`, and `lib.rs`, then reran the full compiler suite and coverage pass to move the compiler to `88.51%` lines / `85.55%` functions / `89.28%` regions while keeping the lib suite green at 228 passing tests.
- Reworked closure-heavy coverage hot spots in `native_runtime.rs`, `native_codegen.rs`, `interpreter.rs`, and `mir_runtime.rs`, then reran the full compiler suite and coverage pass to move the compiler to `88.51%` lines / `87.70%` functions / `89.34%` regions while keeping the lib suite green at 228 passing tests.
- Added another helper/refactor sweep across `native_codegen.rs`, `native_runtime.rs`, `interpreter.rs`, `mir_runtime.rs`, `sema.rs`, `lexer.rs`, and `call.rs`, then reran the full compiler coverage pass to move the compiler to `88.64%` lines / `88.29%` functions / `89.49%` regions while keeping the lib suite green at 230 passing tests.
- Added another helper/control-flow sweep across `native_codegen.rs`, `interpreter.rs`, and `mir_runtime.rs`, then reran the full compiler coverage pass to move the compiler to `88.62%` lines / `88.57%` functions / `89.52%` regions while keeping the lib suite green at 230 passing tests.
- Added another MIR/checker/direct-backend sweep across `mir.rs`, `sema.rs`, and `native_codegen.rs`, then reran the full compiler coverage pass to move the compiler to `89.08%` lines / `89.21%` functions / `89.90%` regions while keeping the compiler suite green at 237 passing lib tests plus the fixture and module suites.
- Added another interpreter/checker/direct-backend sweep across `interpreter.rs`, `sema.rs`, and `native_codegen.rs`, then reran the full compiler coverage pass to move the compiler to `89.34%` lines / `89.34%` functions / `90.17%` regions while keeping the compiler suite green at 241 passing lib tests plus the fixture and module suites.
- Tightened another `native_codegen.rs` error-mapping batch with a macro-based refactor that preserved function coverage while shaving more uncovered backend lines, then reran the full compiler coverage pass to move the compiler to `89.38%` lines / `89.34%` functions / `90.17%` regions.
- Added another helper sweep in `interpreter.rs`, `sema.rs`, and `native_codegen.rs`, then reran the full compiler coverage pass to move the compiler to `89.45%` lines / `89.35%` functions / `90.20%` regions while the remaining drag stayed concentrated in `interpreter`, `sema`, and `native_codegen`.
- Added another checker/runtime helper sweep in `sema.rs` and `interpreter.rs`, then reran the full compiler coverage pass to move the compiler to `90.26%` lines / `89.45%` functions / `90.79%` regions while the remaining drag stayed concentrated in `interpreter`, `sema`, and `native_codegen`.
- Completed the next full compiler coverage run after that helper sweep, moving the compiler to `90.85%` lines / `89.51%` functions / `91.09%` regions while the remaining drag stayed concentrated in `interpreter`, `native_codegen`, and `sema`.
- Added another interpreter-focused helper sweep over runtime equality/casting, `for`/`select`/`eval_expr` control-flow branches, specialized collection constructors, `try`, logical operators, enum members, and index errors, then reran the full compiler coverage pass to move the compiler to `91.29%` lines / `89.63%` functions / `91.35%` regions while the remaining drag stayed concentrated in `native_codegen`, `interpreter`, and `sema`.
- Added another direct-backend constructor/thunk sweep in `native_codegen.rs`, covering receiver/writeback metadata registration for lowered methods plus float/bool/plain-class thunk parameter lowering and unit-return `main` wrappers, then reran the full compiler coverage pass to move the compiler to `91.33%` lines / `89.65%` functions / `91.40%` regions while the remaining drag stayed concentrated in `native_codegen`, `interpreter`, and `sema`.
- Added another checker/direct-backend sweep in `sema.rs` and `native_codegen.rs`, covering default-argument and recursive-type helper paths, reserved built-in `Result`/`Option` name rejection, scalar `to_string` member typing, scalar direct-type rendering, and opaque thunk error handling, then reran the full compiler coverage pass to move the compiler to `91.46%` lines / `89.84%` functions / `91.50%` regions while the remaining drag stayed concentrated in `native_codegen`, `interpreter`, and `sema`.
- Added another direct-backend/interpreter sweep in `native_codegen.rs` and `interpreter.rs`, removing unreachable collection/task clone-specialization branches from the direct backend, adding direct cleanup/task-group smoke coverage, and covering unsigned cast plus unary operator fallback interpreter paths, then reran the full compiler coverage pass to move the compiler to `91.67%` lines / `89.84%` functions / `91.65%` regions while the remaining drag stayed concentrated in `native_codegen`, `interpreter`, and `sema`.
- Added another checker/interpreter sweep in `sema.rs` and `interpreter.rs`, covering specialized enum member diagnostics, member assignment mismatch diagnostics, qualified module class/enum checker paths, imported module runtime evaluation, constructor error handling, builtin propagation on `try`, and more numeric/string runtime paths, then reran the full compiler coverage pass to move the compiler through `91.98%` lines / `90.17%` functions / `92.08%` regions and then to `92.07%` lines / `90.26%` functions / `92.21%` regions while the remaining drag stayed concentrated in `native_codegen`, `interpreter`, and `sema`.
- Flattened several `native_codegen.rs` direct-backend test scaffolds to remove unexecuted panic closures in the coverage denominator, reran the touched direct-backend tests, and then reran the full compiler coverage pass to move the compiler to `92.12%` lines / `90.59%` functions / `92.25%` regions while `native_codegen.rs` rose to `91.47%` lines / `80.36%` functions / `92.71%` regions.
- Fixed compiler-backed `Vec.insert(...)` analysis/completion metadata so the compiler and LSP bridge now report the correct `-> bool` signature instead of the stale `-> None` detail.
- Added practical parsing/formatting and collection-finisher support across the checker, interpreter, MIR/runtime, direct backend, compiler/LSP tooling, fixtures, examples, and tutorials: `parse_int32`, `parse_int64`, `parse_float64`, scalar and boolean `.to_string()`, `String.join(...)`, `Vec.insert(...)` / `clear()` / `reverse()`, `Map.items()` / `entries()` / `clear()` / `extend(...)`, builtin `MapEntry[K, V]`, and owned `Set[T]` collections with `Set{...}` literals, iteration, and the maintained set method surface.
- Added literal `match` patterns over `bool`, integer, and `String` scrutinees across the parser, checker, interpreter, MIR lowering/runtime, maintained examples, CLI smoke tests, and tutorial track, while keeping wildcard exhaustiveness requirements for open-ended literal domains.
- Added builtin owned `Vec[T]` collections across the checker, interpreter, MIR runtime, direct backend, CLI, compiler-backed tooling, fixtures, maintained examples, and tutorials, including list literals, borrow-safe indexing, indexed assignment, by-value/shared/mutable iteration, `len() -> int32`, equality, and the maintained method surface `len`, `is_empty`, `clone`, `push`, `pop`, `get`, `set`, `remove`, `swap`, `contains`, and `extend`.
- Added builtin `String` utility methods, numeric helper builtins, and owned `Map[K, V]` collections across the checker, interpreter, MIR/runtime, direct backend, compiler-backed tooling, fallback LSP analysis, fixtures, maintained examples, tutorials, and CLI smoke coverage, including map literals, indexed map reads/writes, and the maintained `Map` method surface `len`, `is_empty`, `clone`, `get`, `set`, `remove`, `contains_key`, `keys`, and `values`.
- Expanded the maintained `String` utility surface with `split`, `replace`, `to_lower`, `to_upper`, `strip_prefix`, and `strip_suffix` across the checker, interpreter, MIR runtime, direct backend, compiler/LSP tooling, fixtures, examples, tutorials, and CLI smoke coverage.
- Fixed postfix parsing so indexed expressions can chain members and calls correctly again, and locked in compiler/LSP coverage for indexed expressions inside f-string interpolations such as `f"{counts["key"]}"`.
- Fixed `for value in mut vec:` so it now requires a mutable `Vec[T]` place during checking, instead of silently mutating immutable bindings.
- Fixed interpreter and MIR `Vec[T]` equality for mixed-construction vectors, so empty-annotated-plus-push vectors now compare by element contents just like literal-built vectors.
- Fixed the remaining Vec follow-up gaps by requiring mutable places for `mut ` vector iteration and teaching MIR/direct build inference that `Vec[T]()` constructor locals still carry `Vec[T]` types into later `for` lowering.
- Fixed overlapping borrowed call arguments so free functions and method receivers can no longer alias the same place across `mut ` / `mut ` or `` / `mut ` combinations.
- Fixed direct/default native builds for `float64` returns from enum `match` arms that destructure payloads, keeping build parity with `run` and `run-mir`.
- Removed the duplicate prefix spelling for ordinary borrowed parameters so free-function borrows are now written only as `name: Type` / `name: mut Type`, with parser regression coverage and aligned examples/tutorials.
- Fixed direct `check` / `analyze` package-root inference for nested package modules, so opening files like `examples/modules/pkg/user.au` no longer resolves imports through a duplicated path segment.
- Added normal `aura help` / `aura --help` / `aura version` / `aura --version` success paths and documented them in the maintained CLI/tutorial surface.
- Replaced machine-local absolute repo paths in the maintained READMEs/tutorials with portable relative links or `$(pwd)`-style command examples, and refreshed `examples/README.md` to include `examples/modules/trait_impl_imports.au`.
- Documented the current first-user limitations that are still real surface constraints, including no `String(...)` constructor, no bare `Ok(...)` / `Err(...)` constructors, required `Channel[T]` context for `channel()`, and named-function-only `spawn` targets.
- Fixed bare `None` parity across `run`, `run-mir`, and native builds, recovered compiler-backed analysis/completions for buffers with multiple dangling member accesses, and restored source-aware arithmetic runtime diagnostics for built binaries.
- Fixed f-string lexing/parsing so interpolations can contain inner string literals and nested braces, with maintained compiler fixture coverage for both checking and execution.
- Added maintained regression coverage for `Option.None` inference, namespace-qualified imports inside imported module bodies, and closed-channel `select` timers.
- Fixed field-level move tracking for owned member reads so Aurora now rejects reusing a moved field while still allowing access to untouched fields and explicit field reinitialization.
- Fixed `select` with `after(...)` over closed-and-empty channels so timer arms no longer starve behind immediate `recv()` closure results, and added maintained runtime regression coverage for that path.
- Fixed specialized generic trait impl dispatch across interpreter, MIR runtime, and direct native builds, and added maintained examples plus CLI coverage for specialized dispatch and trait-associated methods.
- Fixed direct-backend multi-implementor trait dispatch so bounded generic calls like `animal.describe()` now build natively across multiple concrete receiver types, with maintained example and CLI coverage.
- Reserved built-in type names such as `Task`, `Channel`, and `Result` for the language/runtime surface so user-defined classes, enums, and traits now fail early with a clear diagnostic instead of later type-arity confusion.
- Fixed module-crossing trait impl resolution across checking, interpreter/MIR execution, direct builds, compiler-backed completions, and the LSP bridge.
- Added generic trait declarations plus generic impl headers across the parser, checker, interpreter, MIR/runtime, direct builds, fixtures, examples, tutorials, and CLI smoke coverage.
- Fixed module-qualified `spawn` targets so `check`, `run`, `run-mir`, and `build` now report a user diagnostic instead of letting MIR lowering panic.
- Fixed compiler-backed definitions for namespace-imported symbols and enum variants used in `match` patterns, with matching LSP bridge coverage.
- Added module-qualified type annotations to the maintained module surface and updated the examples/tutorials to use them directly.
- Extended compiler-backed dangling-member recovery so `aura analyze` / `aura complete` still recover symbols and completions when `counter.` is the final buffer line.
- Fixed direct-backend native builds for recursive match payloads and `Task.join()` values that carry plain classes, including spawned functions that return plain-class values.
- Hardened compiler, MIR/runtime, and direct-backend parity around external regression cases, including stdin-backed local-module execution, generic dispatch/composition, borrowed field projections, large negative literals, float rendering, and maintained-example native builds.
- Added a Rust workspace root with `aurora-compiler` and `aura`.
- Added the first compiler modules: diagnostics, AST, lexer, parser, semantic checker, and evaluator.
- Added the first milestone sample program at `examples/point.au`.
- Added `examples/README.md` with instructions for running, checking, and inspecting example programs.
- Added `crates/aura/README.md` with release-build and direct binary usage instructions.
- Added in-repo work tracking under `work/`.
- Verified `cargo test` passes.
- Verified `cargo run -p aura -- run examples/point.au` prints `5.0`.
- Added support for `def name(...):` as shorthand for `-> None`.
- Added support for running top-level script statements without an explicit `main`.
- Renamed primitive language types to explicit spellings like `int32`, `uint64`, and `float64`.
- Renamed the line-printing builtin from `println` to `print`.
- Verified `examples/basic_addition.au` and `examples/top_level_addition.au` both run and print `16`.
- Added `tools/vscode-aurora` as an in-repo VS Code extension package.
- Added `tools/aurora-language-server` as an in-repo LSP package.
- Added a root npm workspace manifest for repo-managed tools.
- Verified the VS Code extension analysis/tests with `npm run check:extension` and `npm run test:extension`.
- Switched the VS Code package from local editor analysis to an LSP client.
- Added a bundled `dist/` build for the VS Code extension so VSIX packaging stays self-contained inside the monorepo.
- Verified `npm run package:extension` produces `tools/vscode-aurora/aurora-language.vsix`.
- Regenerated `docs/aurora_language_proposal.html` from the updated proposal Markdown.
- Added parser, semantic checker, and interpreter support for `if`, `elif`, `else`, `while`, `break`, `continue`, strings, booleans, comparison operators, and compound assignment.
- Added `examples/control_flow.au` and verified the control-flow bootstrap path.
- Improved CLI diagnostics so parser/type/runtime errors render with source context and a caret.
- Staged compiler MIR lowering with explicit basic blocks and a new `aura mir <file.au>` command.
- Added LSP hover, go-to-definition, and document diagnostics on top of the current Aurora-aware analysis layer.
- Added categorized examples covering most of the currently implemented language surface.
- Added a `tutorials/` directory with Markdown chapters for the implemented subset and documented the maintenance rule that examples and tutorials must evolve with the language.
- Fixed LSP false positives for top-level script bindings and added member resolution for parenthesized receiver expressions such as `(dx * dx + dy * dy).sqrt()`.
- Added a repo-level `AGENTS.md` and `docs/testing_strategy.md` to define the test-first workflow.
- Added fixture-based compiler tests for parse/check/run/diagnostic behavior under `crates/aurora-compiler/tests/fixtures/`.
- Added `crates/aurora-compiler/README.md` documenting compiler test layers and fixture categories.
- Added `npm run coverage:lsp` as the repeatable language-server coverage command and documented it in the repo.
- Added `npm run coverage:compiler` and measured the first Rust compiler-library coverage baseline with `cargo-llvm-cov`.
- Added parser, checker, interpreter, MIR, examples, and LSP support for non-generic enums with unit and single-payload variants plus exhaustive statement-form `match`.
- Added parser, checker, interpreter, MIR, examples, tutorials, and LSP support for `for` loops over `range(...)`.
- Added parser, checker, interpreter, examples, tutorials, and LSP support for user-defined instance methods with `self` plus associated methods.
- Added built-in generic `Result[T, E]` and `Option[T]` support across the checker, interpreter, examples, tutorials, and LSP analysis.
- Added fuller mutating receiver semantics with member-target assignment, `mut self`, mutating methods, and regression fixtures.
- Added `try expr` over built-in `Result[T, E]` with checker/runtime support, examples, tutorials, and diagnostics.
- Added `with` scoped cleanup using `close(mut self)` resources, plus examples, tutorials, and runtime cleanup on early return.
- Added bootstrap concurrency with `Channel[T]`, `channel()`, `spawn`, `Task[T]`, `send`, `recv`, `close`, and `join()`, plus examples, fixtures, and LSP support.
- Added bootstrap structured concurrency with `task_group()`, `with task_group() as group:`, `group.spawn(...)`, `group.cancel()`, cooperative `cancelled()`, `select`, and duration literals for `after(...)`, plus examples, fixtures, tutorials, MIR support, and LSP coverage.
- Added explicit detached tasks with `spawn detached`, proposal-level `Channel.send() -> Result[None, SendError[T]]`, and broader `select` send/recv/timer arm support across the compiler, runtime, examples, fixtures, tutorials, syntax highlighting, and LSP.
- Fixed LSP false diagnostics for `after(...)` select timers and duration literals like `5ms` in concurrency examples.
- Added machine-readable compiler analysis output plus `aura analyze` and `aura ast-json`.
- Switched the language server to prefer compiler-owned diagnostics, symbols, hover, and go-to-definition via `aura analyze`, with local JS analysis kept as fallback and for completions.
- Added machine-readable compiler completions via `aura complete`.
- Switched the language server to prefer compiler-owned completions, leaving the JS analysis layer as fallback for incomplete or currently-invalid buffers.
- Expanded the tutorial track so it covers the full currently implemented bootstrap language surface, not just the features already represented by the example walkthroughs.
- Fixed VS Code indentation so pressing Enter after Aurora block headers keeps the expected block indent instead of jumping back to column 0.
- Added an Aurora-specific VS Code Enter handler so indentation now deterministically follows Aurora block structure instead of relying only on editor heuristics.
- Added named arguments for ordinary functions, instance methods, associated methods, and spawned function targets, aligning callable syntax more closely with class construction.
- Added a shared compiler-side call binding layer for user-defined callables and builtins.
- Added named arguments for supported builtins, including `print(value=...)`, `range(stop=...)`, `range(start=..., stop=...)`, `after(duration=...)`, and `Channel.send(value=...)`.
- Added compiler and LSP regression coverage plus categorized examples and tutorial updates for builtin named arguments.
- Added integer-literal range enforcement for fixed-width integer annotations and default `int32` literals.
- Added support for `String.clone()` in the checker/runtime and removed unsupported `String.as_str()` from the documented current surface and completions.
- Improved the diagnostic for builtin method references like `ch.send` so they report a missing call instead of a misleading generic-type error.
- Clarified current limitations and `aura complete` semantics in the README and tutorial track so the documented bootstrap surface matches the implementation more closely.
- Made `aura complete --trigger .` tolerate the common incomplete-editor state where the current buffer contains a dangling member access like `counter.`.
- Made `aura analyze` recover symbols and occurrences for the common dangling-dot editor state while still surfacing the parse diagnostic.
- Added CLI product tests for broken-pipe stdout handling in `ast` and `mir`, and fixed those commands to exit cleanly when piped into consumers like `head`.
- Added `aura build -o <output>` as a bootstrap standalone-binary path by generating and compiling a Rust launcher linked against `aurora-compiler`.
- Added a MIR runtime for the current simpler subset plus `aura run-mir` for exercising that execution path directly.
- Expanded `aura run-mir` so it now covers the current implemented Aurora surface natively through MIR, including concurrency, `try`, and `with`.
- Switched `aura build` from embedding source execution to embedding checked MIR and running it directly through `run_mir(...)`.
- Added backend regression coverage for native MIR execution through both `run-mir` and built binaries.
- Added native MIR support for `try expr`, removing `try` from the backend fallback surface.
- Added native MIR support for `with` cleanup, removing `with` from the backend fallback surface.
- Added boolean operators `and`, `or`, and `not` across the parser, checker, interpreter, MIR lowering, and MIR runtime.
- Added unary minus support across the parser, checker, interpreter, MIR lowering, and MIR runtime.
- Added checker-level use-after-move diagnostics for straight-line moves through function arguments, value receivers, constructors, enum payloads, and channel sends.
- Added clean Aurora diagnostics for division by zero and integer overflow in both the interpreter and MIR runtime.
- Added runtime enforcement for annotated fixed-width integer bindings and assignments instead of silently widening values.
- Unified `main` parameter validation so both execution paths reject parameterized `main` functions during checking.
- Added contextual `float32` literal support so floating-point literals can be used in typed `float32` bindings, parameters, returns, and class fields.
- Added explicit numeric casts with `expr as Type` across the parser, checker, interpreter, MIR runtime, compiler analysis, fixtures, and maintained examples.
- Added user-defined generic `class`, `enum`, and `def` declarations with generic inference across the checker, runtimes, fixtures, examples, tutorials, and LSP fallback analysis.
- Added first-pass traits with `trait`, `impl Trait for Type`, bounded generic functions, trait method checking, interpreter/MIR trait dispatch, compiler-backed trait symbols/completions, and maintained examples/tutorial coverage.
- Added default parameter values on ordinary functions and class methods, including checker/runtime/MIR support, call-site omission handling, and proposal-aligned restrictions on ordering and parameter references.
- Promoted multiple trait bounds with `T: A + B` from an untracked capability to a maintained surface with fixtures, examples, and tutorial coverage.
- Fixed the compiler-backed LSP bridge to prefer the current source-tree compiler via `cargo run` inside the Aurora repo, avoiding stale `target/debug/aura` behavior during local development and tests.
- Added `pass` as a maintained no-op statement for intentionally empty blocks.
- Added the `sleep(duration)` builtin across checking, runtime, MIR, examples, tutorials, and editor tooling.
- Added local file module support with `import`, `from ... import ...`, and `public` module boundaries across checking, interpreter execution, MIR execution, CLI run/build, examples, tutorials, and compiler tests.
- Extended compiler-backed `aura analyze` / `aura complete` and the LSP bridge so stdin/file analysis now resolves local module imports for diagnostics, hover, and completions.
- Added CI-style repo gates plus enforced baseline coverage thresholds for the compiler and language server.
- Fixed generic method inference for method calls on generic class instances inside generic functions.
- Fixed user-defined generic enum unit variants so they retain instantiated type arguments.
- Fixed specialized generic trait impl dispatch for concrete generic instances such as `impl Trait for Box[String]`.
- Raised integer and duration literal parsing to `i128`, including minute duration literals with `m`.
- Added wildcard `case _:` support in statement-form `match`.
- Added trait bounds on generic class and enum type parameters.
- Added empty marker traits with `pass`.
- Rejected direct recursive class fields without `indirect` and added proposal-aligned `indirect` recursive fields to the maintained compiler surface.
- Fixed direct-expression narrow integer overflow checking so runtime arithmetic respects annotated widths even when values flow straight into calls.
- Fixed whole-number float rendering so values like `5.0` and `9.0` preserve their `.0` suffix in output.
- Added ordinary free-function `` and `mut ` parameters across the parser, checker, interpreter, MIR runtime, fixtures, examples, tutorials, and LSP fallback analysis.
- Fixed namespace-imported classes and enums so `import a.b` now supports `a.b.Type(...)`, `a.b.Enum.Variant`, and qualified `match` arms in both the interpreter and MIR execution paths.
- Finished the remaining numeric-runtime gap for true full-range `uint128` execution across the checker, interpreter, MIR runtime, direct backend, fixtures, CLI coverage, and maintained examples/tutorials.
- Clarified in the maintained tutorials/examples that `range(...)` is still limited to the current signed index space in the bootstrap compiler, without freezing that limitation into the proposal.
- Brought several proposal-defined syntax/features into the maintained compiler surface: `copy class`, `indirect Node?`, `str` parameters, `match `, unqualified match variants, `for` iteration over `Channel[T]`, contextual `copy` keyword handling, f-strings, and explicit generic constructor specialization like `Box[int32](...)`.
- Added maintained examples, fixture coverage, tutorial updates, and LSP fallback coverage for those proposal-alignment features.
- Replaced `aura build`'s generated Rust launcher with a native MIR artifact build path that embeds serialized MIR in a native launcher and links it against a compiled Aurora runtime library.
- Added product coverage for stdin-backed native builds with local modules and for binaries that still run after the original source file is removed.
- Added a true direct native backend for a supported scalar/control-flow MIR subset and exposed it through `aura build --backend direct`.
- Switched `aura build` to a three-way backend matrix with `--backend auto|direct|mir-runtime`, where `auto` now tries direct native codegen first and falls back when needed.
- Added compiler-side direct-backend coverage so the enforced Rust coverage gate remains green after introducing native codegen modules.
- Expanded the direct native backend to support floats, plain classes, field access, associated methods, and immutable instance methods, including clean broken-pipe handling for direct-built binaries.
- Expanded the direct native backend to cover the full currently implemented Aurora language surface, including mutable borrows, `range`/`for`, traits, generics, resource cleanup, and concurrency/task-group/select examples.
- Verified direct backend parity against every runnable maintained example by building with `--backend direct` and comparing output to `aura run`.
- Removed `--backend mir-runtime` from the CLI and docs now that the maintained Aurora surface has full native direct coverage.
- Fixed direct-backend parity bugs for float comparisons, float modulo, normal-scope `with` cleanup, scalar return values through `with`, boolean printing, narrow integer overflow checks, and trait method dispatch on builtin types.
- Fixed interpreter `float32` display so round-tripped `float32` values render without leaking binary noise like `3.140000104904175`.
- Fixed generic trait dispatch contamination in the tree-walk interpreter so repeated trait-bounded generic calls no longer reuse the first concrete type across later calls.
- Fixed `mir --stdin` so it now resolves local module imports using the provided path, matching `run-mir --stdin`.
- Fixed explicit built-in enum constructor specialization such as `Result[int32, String].Ok(...)` across checking, interpreter execution, MIR lowering/runtime, examples, and tutorials.
- Fixed imported functions that return module-local classes so the caller can use the returned value's fields and methods without importing the class separately.
- Fixed f-string interpolation diagnostics so inner expression errors point at the interpolation site instead of the start of the enclosing function.
- Rejected mutual recursive class fields without `indirect` and replaced raw recursion stack overflows with a friendly runtime call-depth diagnostic.

## Blocked

- None currently.
