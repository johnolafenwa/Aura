# Batch 6: Phase 7, final self-audit, and release preparation

- Date: 2026-07-30
- Status: active
- Entry commit: `8131ebe`
- Stop boundary: final report; do not push commits or tags and do not publish
  archives

## Goal

Close the four Batch 6 entry findings, implement Python-shaped
comprehensions and owned slices, add the first contiguous numeric array
surface with native kernels, then conduct a fresh-eyes product and claims
audit. Prepare and locally verify the 0.2.0 technical-preview release without
publishing it.

## Accepted entry rulings

- Batch 5 is accepted.
- ADR-0037 is Accepted.
- ADR-0038 is Accepted as a design; implementation targets Aurora 0.3 and is
  not authorized in the 0.2 cycle. Its ten ratification questions are all
  answered yes as recommended.
- Compiler coverage floors remain frozen at 96.18% lines, 96.97% functions,
  and 94.62% regions until the one-time final downward-truncated re-ratchet.

## Current stage

B6.0 is active in strict-order entry closure:

- B6.0-b is implemented with dedicated `AU2008`. Named function values,
  capture-free closures, and capturing closures uniformly reject `==` and
  `!=` before backend selection, with exact semantic, fixture, registry, and
  MIR/direct diagnostic-parity tests. The first complete gate found two stale
  FFI closure assertions that still expected generic opaque-handle equality;
  both now explicitly pin `AU2008` precedence while direct opaque-handle
  comparisons retain `AU2003`.
- B6.0-c is implemented. ADR-0037 is recorded as Accepted; ADR-0038 has the
  exact accepted-design-only status and all ten yes answers. The retry and Vec
  callback pages now describe repeatable capturing closures, and the closure
  page pins the enclosing bare-parameter restriction and Copy workaround.
- B6.0-d is implemented through a shared owned-process-group guard. Every
  scalable-runtime workload and the legacy direct-loop helper launches in a
  new session, verifies `SIGTERM` then `SIGKILL` cleanup, reaps the leader and
  streams, and turns silent cleanup failure into a failed benchmark. The
  54-test Python harness covers success, validation failure, timeout,
  interruption, TERM-resistant descendants, and ineffective kill detection.
- B6.0-a is complete. The baseline Mac rebooted at
  `2026-07-30 23:02:25 +0100`; a clean detached schema-4 run at
  `18654158d22b2227149369e7911af04aafcbeecb` measured whole-process V6
  medians of `36.691666 ms` (`int32`) and `14.837417 ms` (`int64`), within
  `1.99%` / `1.12%` of and slightly faster than the accepted reactor-era pair.
  The slower dirty pair therefore does not reproduce after reboot and is
  attributed to dirty/load/thermal host state rather than a HEAD regression.
  All maintained runtime gates pass; only the already-withdrawn 100,000-task
  RSS claim remains red. The contractual raw report SHA-256 is
  `134efcc894742ed73b16e07f1e31845c83d19930d5894b4dc39f01533a9be2fd`,
  and no benchmark workload survived the process-group cleanup.
- The first exact clean full-CI replay at `5a6a77e` passed every behavior gate
  before the frozen compiler coverage ratchet: 54 benchmark tests, 320 CLI
  tests, 1,386 compiler tests and every integration target, 6 retry tests,
  4 FFI acceptance tests, 2 closure acceptance tests, the 665.73-second
  forced-backend parity matrix, 97 LSP tests, and 15 extension tests. All
  instrumented compiler tests then passed, but the report reached
  80,456/83,656 lines (96.174811%), 5,345/5,513 functions (96.952657%), and
  117,328/123,996 regions (94.622407%): the frozen line and function floors
  missed while regions passed. The retained log is
  `/private/tmp/aurora-b60-ci-5a6a77e.log` with SHA-256
  `2afb2b7b99de4a21a729569f2de8d4f0d91722d7be588a9fe16b56a75b7af236`.
  Per the standing rule this is a coverage-only closure, not an escalation.
- Coverage inspection found that the new callable-equality precedence made
  the `opaque_handle_in_type_inner` closure-capture recursion unreachable:
  callable comparison now rejects before structural inspection, closures
  cannot be cloned, and closure environments cannot enter collection
  cloning. That branch is restructured to an explicit non-structural
  `Closure` case instead of adding a synthetic test. The behavior-focused
  closure also pins both malformed lambda-parameter diagnostics: a capability
  marker without a name, and a trailing comma without a following name.
  Focused parser and FFI equality tests pass.
- B6.0 is complete at `49ae8bb`. The exact clean full-CI replay passes the
  54-test benchmark harness, 320 CLI tests, 1,386 compiler tests and all
  integration targets, 6 retry tests, 4 FFI acceptance tests, 2 closure
  acceptance tests, the 659.88-second forced MIR/direct matrix, 97 LSP tests,
  15 extension tests, both coverage gates, all 683 historical migration
  manifests, the executable Manual inventory, docs build, npm and Rust
  audits, warning-denied Clippy, and hygiene. Compiler coverage is
  80,466/83,652 lines (96.191364%), 5,345/5,512 functions (96.970247%), and
  117,334/123,989 regions (94.632588%), above the frozen floors. LSP coverage
  remains 100% at 937/937 lines, 49/49 functions, and 251/251 branches. The
  exact log is `/private/tmp/aurora-b60-ci-49ae8bb-full-access.log`, SHA-256
  `130a78d5f09982b58918b2454254b41edb194187829d69878fbdf9e714e5da36`.
  No synthetic coverage test or exclusion was added; one unreachable
  structural closure branch was replaced by its explicit 0.2 non-structural
  case.

Phase 7.1 comprehension implementation is complete across feature commit
`c7170b5`, completion/coverage closure `e8c7af1`, and deterministic ADR-0035
coverage stabilization `5609d74`:

- The parser and AST accept eager list, set, and map comprehensions with one
  or more progressive `for` clauses, left-to-right filters, recursive tuple
  targets, and multiline layout. Capability modifiers, generator
  expressions, mixed literal/comprehension forms, malformed clauses, and
  trailing commas are rejected with maintained diagnostics.
- Static semantics infer or contextually check `Vec[T]`, `Set[T]`, and
  `Map[K, V]`, require every filter to be exactly `bool`, introduce targets
  progressively under the ordinary no-shadowing rule, and prevent target
  leakage. Range, Vec, Set, Queue, enumerate, and zip clauses reuse bare-loop
  typing and ownership; Vec/Set traversal stays shared and frozen while Queue
  receive produces an owned item. Output insertion is an owned storage
  boundary with no implicit clone. Owner-qualified `ComprehensionInfo`
  metadata records the result type, every clause binding type, and the Queue
  receive-owned distinction for lowering and analysis.
- MIR allocates one fresh typed collection, lowers clauses as nested existing
  bare loops, branches filters in execution order, and emits ordinary Vec
  append, Set insert, or key-before-value Map update operations. Later equal
  map keys replace earlier values and duplicate set values collapse. Runtime
  traps and `try` propagation use the existing partial-result cleanup path.
  The direct backend shares this MIR contract, and maintained runtime fixtures
  pin both-backend output parity.
- Compiler analysis, the language server, and the bundled extension provide
  the checked collection result type, progressively scoped completion and
  hover, exact tuple-target definitions, output member completion, and
  post-expression non-leakage.
- ADR-0039 records the binding Aurora 0.2 contract. The normative Manual,
  Learn and tutorial tracks, root/example/editor indexes, maintained
  `examples/collections/comprehensions.au`, and source-hash reference
  inventory are synchronized.

An integration regression exposed one imported-module metadata defect. MIR
lowering already selects the defining module namespace, but namespace export
did not include the new comprehension map. Public imported functions could
therefore reach lowering without their checked result and clause-binding
types. Namespace export now carries owner-qualified comprehension records and
qualifies nominal result and binding types; the
`comprehension_imported_metadata` fixture proves that imported public helpers
run as `2\n6` on both MIR and direct.

The final independent audit found another lowering-metadata defect before the
feature commit. Comprehensions in accepted function-parameter defaults and
class-field defaults passed semantic checking but panicked when either backend
lowered them. Test-first semantic, run-pass, and dual-backend regressions now
pin both positions. Field-default metadata is retained and merged into the
checked program; hidden default-function lowerers carry the exact lexical
owner; generated lambdas inherit that metadata context; and field defaults
select the defining module's top-level metadata.

Focused verification completed so far:

- 14/14 comprehension-focused compiler unit tests
- 60/60 parser tests, parse fixtures, and the Python-comprehension acceptance
  hint
- comprehension check-pass and check-fail fixtures
- imported nominal-metadata MIR regression plus `2\n6` MIR/direct fixture
- main comprehension, full runtime-matrix, `try`-propagation, and partial-trap
  fixtures with identical MIR/direct behavior across the three focused CLI
  parity tests
- 91/91 compiler-analysis tests
- 99/99 language-server tests at 100% statements, branches, functions, and
  lines
- 17/17 extension tests
- reference integrity: 36 pages, 258 executable blocks, 9 reference tests,
  59 integrity tests, and all 683 migration manifests
- documentation build and Rust formatting
- the latest complete instrumented compiler-library replay at 1,416/1,416 and
  the process integration suite at 5/5
- the new default-expression semantic regression, run-pass fixture, and
  MIR/direct CLI regression after the audit fix

The first exact clean full-CI replay at `c7170b5` passed all behavior,
forced-backend parity, LSP, extension, reference, documentation, audit,
warning-denied Clippy, and hygiene stages, then stopped only at the frozen
compiler-coverage ratchet. Its report was 96.01% lines, 96.90% functions, and
94.40% regions. The log is
`/private/tmp/aurora-comprehension-ci-c7170b5.log`, SHA-256
`f4e8bb8fe140277ce5a9362389fe28fe71a6fb29dd334d29156548b457c4036d`.

The first behavior-focused closure reached 81,690/84,978 lines (96.13%),
5,405/5,577 functions (96.92%), and 119,105/125,951 regions (94.56%). The
second reached 81,734/84,979 lines (96.18%), 5,412/5,581 functions (96.97%),
and 119,176/125,954 regions (94.61865%). The printed region value rounded to
94.62%, but the exact fraction was still just below the frozen 94.62% floor.
Every instrumented suite was green, including 1,413/1,413 compiler-library
tests.

Coverage review found three real completion defects. The current target was
hidden immediately after a comprehension `if`; raw matching could mistake
`if` in a comment for that keyword and compared byte columns with UTF-16 LSP
positions; and a multiline final statement ended the enclosing function at
its first line, dropping local and comprehension scope on continuation lines.
The fixes lex the exact clause-to-filter interval, translate source byte
columns to UTF-16, and compute recursive statement extents from contained
expressions and bodies. Regressions pin comments, non-BMP Unicode, f-string
comprehensions, final multiline assignments, returns, assertions, calls, and
nested blocks. Compiler analysis is green at 91/91 and LSP remains 99/99 at
100% coverage.

No synthetic coverage test or exclusion was added. The settled full-access
replay after these fixes passed every test at 81,766/85,015 lines
(96.178321%), 5,410/5,579 functions (96.970783%), and 119,241/126,026
regions (94.616190%). Only 2 covered lines and 5 covered regions remained.
Its log is
`/private/tmp/aurora-comprehension-coverage-settled-full-access.log`, SHA-256
`087286b9d38f3da7b1d72e616fb401e1849882ea3f02065243631480fb857fd0`.

A final observable regression pins function-local completion inside a
multiline indexed assignment used as the function's last statement. This
covers the indexed-assignment extent path without production changes. The
only other uncovered line added by the completion repair is a defensive
no-filter-token fallback. A checked comprehension filter exists only after
the parser consumes `if`, so reaching that fallback would require a synthetic
AST/source mismatch. It is deliberately unforced and is the justified-
invariant list for this coverage closure.

The definitive full-access compiler-coverage replay is green at
81,768/85,015 lines (96.180674%), 5,410/5,579 functions (96.970783%), and
119,248/126,026 regions (94.621745%), above all three frozen floors. It passed
324/324 CLI tests in 729.57 seconds, 1,416/1,416 compiler-library tests in
379.82 seconds, and every integration target. The exact log is
`/private/tmp/aurora-comprehension-coverage-final-full-access.log`, SHA-256
`bd4ac540e1b20e52925c885d4b23611cd9de2c56661538810b0a85379198d77e`.

The coverage/completion closure is committed at `e8c7af1`. Its exact clean
full-CI replay passed the 54-test benchmark harness, 324 CLI tests, 1,416
compiler-library tests and all integration targets, the 697.85-second forced
MIR/direct matrix, 99 LSP tests, and 17 extension tests. The only red stage
was the compiler-coverage ratchet. Scheduling selected the submitter-side
cleanup of a blocking-I/O admission deadline instead of the worker-side
cleanup in `runtime_value.rs`, leaving one otherwise reachable line
unexecuted. The clean report was 81,767/85,015 lines (96.179498%),
5,410/5,579 functions (96.970783%), and 119,247/126,026 regions
(94.620951%). The retained log is
`/private/tmp/aurora-comprehension-ci-e8c7af1.log`, SHA-256
`5f049f2aa5def066c698fbd1122061cb4c598d55609e29236777dd4b3273583e`.

Independent audit confirmed no production bug. A slot-release/deadline race
has two correct mutex-linearized outcomes: the worker can expire the oldest
waiter while filling the released slot, or the submitter can remove itself
after its deadline wake. Both preserve the ADR-0035 acceptance point and
prevent pre-acceptance execution.

A deterministic behavior-focused unit regression now pins the full
worker-side contract: an expired FIFO head becomes `TimedOut` and never
executes; its live successor becomes `Accepted` into the same released slot;
both completion signals deliver once and close; the live job executes; and no
queued job, admission waiter, or capacity leaks. The focused test, formatting,
diff check, and warning-denied production-library Clippy pass.

The exact clean full-CI replay at `5609d74` is green end to end:

- 54/54 benchmark-harness tests
- 324/324 CLI tests in 733.47 seconds
- 1,417/1,417 compiler-library tests in 377.84 seconds plus every integration
  target
- the full forced MIR/direct fixture matrix in 683.02 seconds
- 99/99 LSP tests and 17/17 extension tests
- compiler coverage of 81,768/85,015 lines (96.180673999%),
  5,410/5,579 functions (96.970783294%), and 119,248/126,026 regions
  (94.621744719%), above the frozen `96.18/96.97/94.62` floors
- LSP coverage of 937/937 lines, 49/49 functions, and 251/251 branches
- reference integrity over 36 pages, 258 fenced blocks, 125 verified blocks,
  9 reference tests, 59 integrity tests, and all 683 migration manifests
- docs build, zero npm vulnerabilities, the allowed `rustls-pemfile` RustSec
  warning, warning-denied Clippy, and hygiene

The retained log is
`/private/tmp/aurora-comprehension-ci-5609d74.log`, SHA-256
`878710a6d88a79e9a0ae0993edbb4f8a2fe9dc4e551fb7f78d1db23255bc56c1`.
The detached proof worktree is clean after the run, its target is 17 GiB, the
main target is 20 GiB, and the host has 126 GiB free. No synthetic coverage
test or exclusion was added. The only justified invariant remains the
unreachable comprehension no-filter-token fallback described above.

Phase 7.1 is signed off.

## Phase 7.2 owned Vec and String slices

Implementation, independent audit, and the exact clean full-CI and frozen-
floor coverage replay are complete. Phase 7.2 is signed off at `1903aae`.

- The AST has a distinct `Slice` expression with optional endpoints and the
  exact colon span. The parser accepts `value[start:end]`, `value[:end]`,
  `value[start:]`, and `value[:]`; keeps indexing and specialization
  distinct; rejects mixed tuple/slice delimiters; and gives the ratified
  `AU2005` guidance for steps and slice assignment.
- Static semantics require exact `int32` written endpoints and accept only
  `Vec[T]` and `String`. The base is retained across start and end evaluation.
  Vec results preserve `T` and infer clone-safety obligations through generic
  specializations; `random.Rng`, opaque handles, capturing closure
  environments, and non-repeatable Task observation rights remain protected.
- MIR lowers the base, start, and end once from left to right, passes explicit
  endpoint-presence flags plus colon coordinates, and returns a fresh owned
  value. The MIR runtime borrows a place receiver so it clones only selected
  Vec elements rather than cloning the whole source first.
- Direct code generation uses the same six-operand private ABI through
  `aurora_direct_vec_slice` and `aurora_direct_string_slice`. Both backends use
  the shared normalize-once, no-clamp bounds helper. String indices are
  Unicode scalar positions and String slicing is O(n).
- ADR-0040 is Accepted. ADR-0004 and ADR-0038, the normative Manual, Learn and
  tutorials, README surfaces, editor guidance, CHANGELOG, and the maintained
  `examples/collections/slices.au` are synchronized. The executable
  Collections Manual block now runs Vec and Unicode String slices and has a
  reviewed source hash and exact output.

Independent audit found and closed three concrete defects:

1. completion after `make_values()[1:].` recovered only `()[1:]`, and `]`
   inside an endpoint string corrupted raw bracket matching; the receiver
   scanner is now delimiter-stack and string aware
2. conformance claimed executable Manual slice evidence before the executable
   block actually exercised slicing; the block now does
3. generic `singleton[T]` construction could form a
   `Vec[consuming closure]`, after which slicing shared the single-use closure
   environment; structural clone-safety now rejects that specialization with
   `AU3007`

Focused verification:

- 19/19 slice-focused compiler-library tests
- 2/2 dedicated forced-MIR/direct CLI parity tests, including exact `AU4003`
  diagnostics and frames
- 9/9 fixture gates, including the new valid `-len`, negative-end-underflow,
  String integer-index, generic-specialization, and retained-base-consumption
  fixtures
- 94/94 compiler-analysis tests
- 100/100 language-server tests
- 18/18 bundled-extension tests
- warning-denied compiler/CLI Clippy, Rust formatting, and scoped diff hygiene
- reference integrity over 36 pages, 258 fenced blocks, 125 verified blocks,
  9 reference tests, 59 integrity tests, and all 683 migration manifests
- documentation build

Build hygiene removed coverage-only output and the obsolete
`native-runtime-uninstrumented` tree, reducing `target/` from 24 GiB to 17
GiB before the focused rebuild. It is currently 20 GiB with 125 GiB free. No
synthetic coverage test or exclusion was added. There is no current blocker.

### Exact clean sign-off

The exact detached full-CI replay at `1903aae` is green:

- 54/54 benchmark-harness tests
- 326/326 CLI tests in 429.08 seconds
- 1,436/1,436 compiler-library tests in 183.10 seconds plus every integration
  target
- complete forced MIR/direct fixture parity in 725.84 seconds
- 100/100 language-server tests and 18/18 bundled-extension tests
- compiler coverage of 82,477/85,734 lines (96.201040427%),
  5,448/5,617 functions (96.991276482%), and 120,411/127,229 regions
  (94.641158855%), above the frozen `96.18/96.97/94.62` floors
- 100% LSP statements, branches, functions, and lines
- reference integrity over 36 pages, 258 fenced blocks, 125 verified blocks,
  9 reference tests, 59 integrity tests, and all 683 migration manifests
- docs build, zero npm vulnerabilities, the allowed `rustls-pemfile` RustSec
  warning, warning-denied Clippy, and hygiene

The retained log is `/private/tmp/aurora-slice-ci-1903aae.log`, SHA-256
`a3088d808902694863e7109be4b518d8f3f1d114d9fc4a435570d4ecdef770a0`.
The detached proof worktree is clean. No coverage-only closure, synthetic
test, or exclusion was required.

Phase 7.3 contiguous arrays, scalar and array integer wrapping/saturating
operations, native kernels, and measured post-reboot NumPy comparisons is now
active. There is no current blocker.

## Authorized sequence

1. Close B6.0-a through B6.0-d and commit the gated entry result.
2. Implement and gate list, set, and map comprehensions.
3. Implement and gate owned Vec and scalar-indexed String slicing.
4. Implement and gate contiguous arrays, scalar and array integer
   wrapping/saturating operations, native kernels, and measured post-reboot
   NumPy comparisons.
5. Run the independent 30-program fresh-eyes corpus on MIR and direct.
6. Produce the consolidated post-reboot benchmark story and claims audit.
7. Align positioning, README, changelog, reference version, and release
   guidance.
8. Build and install-test supported archives, create only the local
   `v0.2.0-preview` tag, run the final gates and coverage re-ratchet, and write
   the final report.

## Standing verification

Each language or tooling behavior change begins with a failing regression.
Every Phase 7 stage lands its compiler, backend, LSP, examples, tutorials, and
normative reference surface together. Coverage-closing tests must pin
observable behavior, diagnostics, or parity; no synthetic execution-only
tests or unjustified exclusions are permitted. The B6.0 coverage closure adds
no synthetic test; one unreachable structural closure branch was replaced by
an explicit non-structural case.

The unrelated modified `personal/file_ops.au` and untracked
`architecture_docs/decisions/0022-implicit-shared-capability-syntax.md` remain
outside Batch 6.

## Follow-up

Record each B6.0 disposition, per-stage commit and gate, the post-reboot
hardware provenance, all measured results, the fresh-eyes corpus, every
autonomous/provisional decision and ADR, release archive hashes, installed
verification, exact user publish commands, and the first three Aurora 0.3
priorities in the final report.
