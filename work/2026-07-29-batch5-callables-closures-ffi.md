# Batch 5: defect closure, callables, closures, and FFI

- Date: 2026-07-29
- Status: active
- Entry commit: `4c9d9a2`
- Stop boundary: checkpoint after Phase 6.5; do not begin Phase 7 or release work

## Goal

Close every mandatory Batch 4 follow-up before expanding Aurora's semantic
surface. Then implement Phase 6 in the ratified sequence: capture-free
function values, callable-powered collection and retry APIs, value-capturing
expression lambdas, manifest-gated FFI v0, and a proposal-only loan/view ADR.

## Entry decisions

- Batch 4 is accepted, including its 100,000-task RSS escape-hatch
  disposition.
- ADR-0032, ADR-0033, and ADR-0036 are Accepted.
- ADR-0034 is Accepted after B5.0-b closed its nested generic select payload
  matrix and passed independent audit.
- ADR-0035 is Accepted after B5.0-c serialized only the two mutually
  contending watchdogs and passed repeated default-parallel runs.
- The compiler floors remain frozen at 96.13% lines, 96.90% functions, and
  94.46% regions until the one-time checkpoint re-ratchet.

## B5.0 disposition

B5.0 is complete and committed as seven isolated changes:

- `e25387d` replaces elapsed-time TaskGroup join cancellation with a
  reachability search over live task waits, queue directions, dynamically
  acquired queue handles, and nested join dependencies. The artificial-load
  regression runs 12 CPU burners around four producers, four consumers, and a
  capacity-64 queue; all 4,000 values arrive on MIR and direct. Separate
  regressions pin a true deadlock, the joining-parent exclusion, dynamic queue
  handoff, and a cross-join cycle. A post-gate correction keeps queue-iteration
  producer lifetime separate from general queue-handle reachability; all six
  queue-iteration CLI regressions pass.
- `22a9073` preserves concrete nested `SelectOutcome` payload types through MIR
  pattern lowering. Arithmetic, comparison, f-string interpolation, typed
  rebinding, and reassignment pass on MIR and direct; independent audit also
  covered task, deadline, non-copy, and nested generic payloads.
- `e93e789` narrowly serializes only the two mutually contending blocking-pool
  watchdogs and adds a pinned-nightly Linux ThreadSanitizer workflow. The local
  normal-mode sanitizer selection runs all 214 runtime-value scheduler tests
  plus the multicore and TaskGroup join CLI families; actual TSan execution is
  intentionally Linux-only.
- `8dc509b` advances the scalable-runtime report to schema 4 and adds paired
  startup/loop workloads. The accepted alternate disposition records the clean
  whole-process baseline and treats the available dirty split only as evidence
  that the tool can separate fixed and loop work, not as proof of causality.
- `616ac71` is the required single B5.0-f commit: human and JSON `aura build`
  wait reporting, the MIR multicore contention limit, and the final two
  descriptive shared-access labels. No least-loaded admission change landed
  because the available measurement did not establish a sound scheduling
  policy.
- `90fe059` keeps the zero-producer Queue-iteration watchdog deterministic
  under an oversubscribed host by running that inherently single-worker case
  with one worker. It preserves the 15-second deadlock guard and leaves the
  default-parallel runtime gate intact.
- `14f2b8b` makes the human and JSON build-wait regressions lock both the
  normal and coverage-specific native-runtime build locations. The two tests
  now exercise the intended reporter path in both ordinary and instrumented
  profiles.

ADR-0032, ADR-0033, ADR-0034, ADR-0035, and ADR-0036 are Accepted. ADR-0034's
nested-payload completion matrix and ADR-0035's default-parallel watchdog
condition are now satisfied.

## B5.0 verification

- `RUST_MIN_STACK=33554432 cargo test`: green, including 308 CLI tests and
  1,157 compiler-library tests.
- Queue-iteration CLI regression family: 6/6; the role-separation unit,
  reachability units, and both originally failing direct cases pass.
- Blocking-pool watchdogs: three prior default-parallel repetitions plus the
  combined workspace gate pass.
- Benchmark-runner suite: 49/49.
- Reference integrity: 9 Python checks, 59 migration checks, all 683 maintained
  manifest files, and 116 verified Aurora manual blocks pass.
- `npm run docs:build`, `npx --yes github-actionlint`,
  `cargo fmt --all -- --check`, and warning-denied Clippy pass.
- Human and JSON build-wait regressions pass after the final reporter lifetime
  cleanup, including focused self-contained `cargo llvm-cov` runs.
- The exact isolated `npm run ci` gate passes at `14f2b8b`: the 308-test CLI
  suite, 1,157-test compiler suite, forced MIR/direct parity matrix, 90 LSP
  tests, 13 extension tests, both coverage gates, reference integrity, docs,
  npm and Rust audits, warning-denied Clippy, and hygiene are green.
- Exact compiler coverage is 71,457/74,328 lines (96.137391%), 4,800/4,953
  functions (96.910963%), and 104,919/111,056 regions (94.473959%), above the
  frozen 96.13/96.90/94.46 floors. No synthetic coverage test or exclusion
  was added.

B5.0 is closed.

## Phase 6.1 implementation

Capture-free function values are implemented across the maintained product
surface:

- Function types use declaration-shaped `def(...) -> R` syntax. Bare
  parameters are shared, `mut` parameters preserve mutable writeback, and
  `own` parameters preserve transfer. Function types nest in other function
  types, tuples, generics, fields, and collection element types.
- Named local and imported functions, explicitly or contextually specialized
  generic functions, and supported builtin-module functions are first-class
  Copy and Transfer values. They can be assigned, passed, returned, stored in
  fields and collections, selected at runtime, called indirectly, and used as
  `TaskGroup.start` or `start_soon` targets.
- Concrete inferred function values preserve parameter names and dynamic
  defaults. Written structural annotations and mutable storage boundaries
  intentionally erase names and default availability while retaining the
  structural parameter types and capabilities. Control-flow and generic
  evidence retain only the callable contract shared by every possible value.
- Method and associated-method values remain explicitly outside Phase 6.1 and
  receive teaching diagnostics.
- MIR execution and direct native execution share source-order argument
  evaluation, declaration-slot binding, generic specialization, hidden
  default suppliers, mutable writeback, owned consumption, Task handoff,
  selected-target frames, and runtime type identity. The direct ABI uses
  synchronous stack argument buffers and guarded heap buffers only across Task
  handoff.
- The semantic-interface schema is version 3. Compiler analysis, LSP
  completion/hover, fixtures, examples, tutorials, and the normative Manual
  all expose the same function-value surface.

The first exact compiler-coverage replay passed every behavioral test but
missed only the function floor. The gap was closed without execution-only
tests: one public Aurora filesystem regression pins invalid UTF-8 and sorted
directory entries; a native Set boundary pin covers `i64::MAX`; duplicated or
unreachable closure boundaries in callable lookup, MIR inference, native
argument binding, reactor bookkeeping, codecs, and supported-target index
conversion were replaced with equivalent explicit control flow.

The final exact compiler-coverage replay is green: 317/317 CLI tests,
1,221/1,221 compiler-library tests, all integration targets, fixtures, and the
coverage-only public callable ABI regression pass. Coverage is
74,018/76,980 lines (96.15%), 4,975/5,127 functions (97.04%), and
108,683/114,967 regions (94.53%), above the frozen 96.13/96.90/94.46 floors.
No synthetic coverage test or exclusion was added.

The full repository gate then passed formatting, the 49-test scalable-runtime
harness, 317 CLI tests, 1,221 compiler-library tests, every remaining Rust
target, the forced MIR/direct parity matrix, 91 LSP tests, 13 extension tests,
both exact coverage gates, reference integrity, the documentation build, and
both dependency audits. Its first warning-denied Clippy pass found two local
representation/style issues: callable signatures made the MIR operand enum
larger than the lint budget, and a default-marker construction used
`bool::then` unnecessarily. Callable signatures are now boxed inside MIR
operands without changing their serialized wire shape, the eager construction
uses `then_some`, and warning-denied Clippy plus callable, serialized-MIR, and
capture-free runtime regressions are green.

The final repository hygiene command reaches only the unrelated, user-owned
`personal/file_ops.au` whitespace diff, which remains deliberately untouched
and outside this commit. `git diff --check` over the complete Phase 6.1 change
set is green, and every remaining artifact, tracked-executable, scheduler
pointer, and historical-commit hygiene rule passes independently.

## Phase 6.2 maintained surface

The callable-powered standard-library reference, examples, and teaching
surface now specify:

- stable mutable `Vec.sort()` and `Vec.sort_by(key)`, including one
  left-to-right key evaluation per element before mutation and unchanged
  receiver state when a key call traps
- eager shared `Vec.map(f)` and clone-producing `Vec.filter(f)`, fresh owned
  results, source retention, and exact bare/shared callback capabilities
- `control.retry[T, E]` over a capture-free
  `def() -> Result[T, E]` worker, with an immediate first attempt, every-Err
  retry policy, doubling `Duration` delays, zero-delay sleep elision, exact
  final-error return, no post-final sleep/multiply, and trap/overflow/task-
  cancellation propagation

The normative Collections, Control-Plane, API Index, Static Semantics,
Execution Model, Diagnostics, Current Limits, Status, and Conformance pages
are aligned. Two new executable Manual blocks are source-hash pinned.
`examples/collections/vec_algorithms.au` and
`examples/agents/retry_with_backoff.au` are indexed from the repository,
compiler-package, and examples READMEs; the Learn and tutorial tracks teach the
same eager ownership and failure contracts.

Both maintained examples execute with their documented output. The complete
`npm run check:reference` gate is green across 34 Manual pages, 118 verified
Aurora blocks, all 59 capability-migration tests, and the maintained-source
retired-syntax check. `npm run docs:build`, the reference-inventory unit tests,
and scoped whitespace checks are also green.

Phase 6.2 is now fully integrated and gated. The implementation uses ordinary
MIR callable targets for Vec callbacks and the retry worker. A generated Aurora
retry state machine also backs specialized `control.retry[T, E]` function
values, so direct calls and indirect calls have identical semantics. During the
full gate, two existing unboxed-int64 native regressions exposed that the
checker-wide builtin registry was injecting an unused empty retry declaration
into programs that did not import `control`. The fix both limits that generated
function to programs which can name it and gives imported retry function values
the real state-machine body. Focused regressions pin unrelated native-object
isolation and specialized retry function values on both backends.

The full Phase 6.2 gate passed 49 scalable-runtime benchmark checks, 318 CLI
tests, 6 retry integration tests, 1,231 compiler tests, the forced-backend
parity matrix, 92 LSP tests, 13 extension tests, reference integrity, docs
build, both dependency audits, and Clippy with warnings denied. LSP coverage
remains 100% for statements, branches, functions, and lines. Compiler coverage
is 75,389/78,414 lines (96.14%), 5,019/5,176 functions (96.97%), and
110,433/116,803 regions (94.55%), above the frozen 96.13/96.90/94.46 Phase 6
floors. Every added coverage test pins observable behavior; no synthetic
coverage test or exclusion was added.

The global hygiene command reaches only the excluded user-owned
`personal/file_ops.au`, where it reports pre-existing trailing whitespace.
That file and the untracked user-owned ADR-0022 draft remain outside this
change. The complete maintained Phase 6.2 change set passes `git diff --check`
and every other hygiene rule.

## Phase 6.3 documentation and editor surface

Provisional ADR-0037 and the normative Closure Manual page now define
expression-only lambdas, contextual parameter types, zero-parameter result
inference, by-value Copy/move capture, repeatable versus consuming calls,
read-only environments, and structural Transfer. The maintained limit is
explicit: arbitrary written `def` storage describes capture-free code
pointers, while compiler-known callback and task-start sites preserve closure
environment and call-kind metadata. Conditional and `match` expressions also
reject merging distinct capturing closure values because Phase 6.3 has no
closure-union type; the AU2002 diagnostic directs callers to invoke inside
each branch or use capture-free lambdas or named functions.

The reference index, grammar, semantics, ownership, concurrency, diagnostics,
status, Learn/tutorial tracks, READMEs, conformance matrix, and
source-hash-pinned maintained example are synchronized. Compiler analysis and
the language server resolve lambda parameter scope, captured definitions,
closure hover types, local completions, and ownership diagnostics. The VS Code
package recognizes `lambda` and includes an expression-lambda snippet.

Focused verification passes for compiler analysis, LSP/editor regressions,
reference integrity, and the maintained example on MIR and direct backends.
Maintained parity additionally covers repeated construction of one lambda site
with distinct environments and cleanup of an uncalled consuming environment.
Adversarial MIR/direct probes pass nested Copy and non-Copy capture, shadowing,
branch-local closure moves, and capture-free storage. The branch-union
diagnostic and its exact fixture are green.

The combined semantic, ownership, backend, fixture, and instrumented suites are
green. The exact compiler-coverage replay passes 319 CLI tests, 6 retry tests,
2 closure-acceptance tests, 1,306 compiler-library tests, and every remaining
integration target. Coverage is 77,482/80,598 lines (96.133899%),
5,135/5,298 functions (96.923367%), and 113,378/119,923 regions
(94.542331%), above the frozen 96.13/96.90/94.46 floors.

The first otherwise-green replay missed only the frozen coverage ratchet. Four
rounds of focused tests closed it by pinning observable closure typing,
diagnostics, capture ordering, dynamic defaults, ownership transfer, mutable
writeback, task results, cleanup, imported identity, malformed-runtime
diagnostics, and ledger/reachability behavior. No test was added only to
execute a line, no production branch was changed for coverage, and no coverage
exclusion was added. Two defensive semantic arms remain deliberately
unforced: erasing callable contracts from a capturing `Type::Closure`, and
deriving task-observation shape directly from a capturing `Type::Closure`.
Both are unreachable from Aurora source because every storage or generic
escape that could erase the compiler-known environment is rejected first.

The first full-CI replay exposed six outer CLI watchdogs that still used a
15-second process budget. Under the complete default-parallel suite, host
oversubscription delayed otherwise-correct direct binaries past that budget;
the same Queue-iteration, join-cycle, and numeric-trap cases all passed when
focused, and the isolated cross-join case itself required 17.41 seconds. Only
those six outer watchdogs now use the repository's existing 30-second
load-tolerant margin. Their Aurora-level sleeps, outputs, diagnostics,
deadlock behavior, and worker counts are unchanged.

The corrected full repository replay is green through formatting, 49
scalable-runtime benchmark checks, all default-parallel Rust tests (319 CLI,
6 retry, 2 closure acceptance, 1,306 compiler-library tests, and every other
integration target), forced MIR/direct parity, 94 LSP tests, 14 extension
tests, compiler coverage, and 100% LSP coverage. The reference gate re-executes
the maintained Manual blocks, passes its 59 integrity tests, validates the
unchanged historical 683-file capability-migration ledger, and finds no
unallowlisted retired syntax. The docs build, npm and Rust dependency audits,
and warning-denied Clippy also pass.

The global hygiene command reaches only whitespace in the excluded,
user-owned `personal/file_ops.au`. The staged Phase 6.3 tree passes
`git diff --cached --check`; the HEAD-history, forbidden-artifact,
tracked-executable, raw-scheduler-pointer, and unsafe-scheduler-reconstruction
checks all pass independently. The user file and untracked ADR-0022 draft
remain untouched and outside the commit.

## Verification policy

Each behavior change starts with a failing regression and receives focused
MIR/direct or workflow validation before the combined B5.0 gate. Phase 6 does
not start until all B5.0 items and their conditional ADR dispositions are
green. Each Phase 6 stage then receives its own focused tests, reference
updates, coverage check, full CI gate, and commit family.

## Follow-up

At checkpoint, record B5.0 evidence, per-stage commits, callable/closure/FFI
reference inventory, suite counts, parity, coverage and the one-time
re-ratchet, provisional decisions, and any work moved into or out of Batch 6.
