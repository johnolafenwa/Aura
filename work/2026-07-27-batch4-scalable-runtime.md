# Batch 4: Phase 5 scalable runtime

Date: 2026-07-27. Entry commit: `1c249ab`.

## Goal and boundary

Close B4.0's four independent-verification findings, then implement Phase 5
in its ratified strict order:

1. persistent reactor registrations, timer heap, and direct wakeups
2. public `yield_now()`
3. compiler-inserted loop-backedge safepoints
4. coroutine stack reduction after deep protocol work moves to service threads
5. removal of aliased mutable scheduler access
6. structural task-boundary Transfer rules and single-consumer task results
7. pinned-worker multicore execution
8. typed heterogeneous select
9. configurable blocking pool
10. native Aurora/task-ancestry frames and structured diagnostic frames

Stop at the Batch 4 checkpoint. Do not begin Phase 6.

The provisional-decision protocol, reference-freeze discipline, and
behavior-focused coverage rules remain in force. Compiler coverage floors are
frozen for this batch at 96.13% lines, 96.89% functions, and 94.35% regions.
They will be re-ratcheted once, using truncated values, at the checkpoint.

## Benchmark host

- Model: Mac14,9
- CPU: Apple M2 Pro
- Logical CPUs reported by the host: 10
- Memory: 16 GiB
- Operating system: macOS 26.5.2 (25F84)

Contractual benchmark runs must use a dedicated quiet-machine protocol and
record command, build profile, repetitions, validation, peak RSS, CPU, and
latency distributions. The Mac14,9 baseline is calibration evidence rather
than a portable language guarantee until the checkpoint accepts it.

## Entry verification

- Batch 3 is accepted at `1c249ab`.
- The worktree was clean at Batch 4 entry.
- The frozen floors in `scripts/coverage-compiler.sh` are exactly
  96.13/96.89/94.35.
- `target/` was 7.8 GiB with 192 GiB free at entry, below the cleanup
  threshold.

## Current work

B4.0 implementation and its repository gates are complete:

- B4.0-a uses a short cross-process runtime-identity lock plus per-content-key
  writer locks. N concurrent cold runs of one program now produce one build
  and N-1 verified consumers; optimistic established hits do not wait for the
  key writer. The installed immutable-runtime path remains usable with caching
  disabled or unavailable.
- B4.0-b flushes `aura: waiting for a concurrent build...` before a human-mode
  process blocks and `aura: rebuilding native runtime...` before rebuild work.
  Each notice is deduplicated per invocation. JSON mode provisionally buffers
  those exact strings so stderr remains one JSON document: success uses the
  `progress` array and failure uses diagnostic `notes`; an `auto` fallback
  also preserves its direct-to-MIR transition and reason.
- B4.0-c capability-aware AU3001/AU3002/AU3003/AU3005 guidance is committed at
  `4f0461e`. The same clone-safety helper also corrects AU3006's directly
  non-cloneable `random.Rng` wording as a provisional diagnostic gap-fill.
- B4.0-d gate-condition suite-count precision is committed at `5cb4476`.

The Phase 5.1 reactor implementation, contractual after-stage benchmark,
frozen coverage gate, and exact full CI are complete:

- `RuntimeReactor` owns one persistent `mio::Poll`, a durable command inbox
  and `Waker`, epoch-keyed readiness, a versioned and compacting deadline heap,
  and aggregate persistent registrations for shared descriptors.
- Queue receive/send, task completion, cancellation/group signals, and the
  blocking pool's completion Queue publish direct keyed wakeups. Registration
  follows check-subscribe-recheck; resolution and scheduler teardown remove
  every losing subscription.
- The scheduler blocks until an event or deadline when idle. It also admits
  reactor events nonblocking before every ready-task turn so a task that
  explicitly yields forever cannot starve timers, queues, or descriptors.
- Reactor registration and cleanup failures become runtime diagnostics instead
  of false readiness. Descriptor bookkeeping is retired transactionally even
  when a closed descriptor makes deregistration or interest narrowing fail.

The dedicated benchmark harness is committed at `850e906`, and the contractual
before-reactor baseline is recorded in
`work/2026-07-27-phase5-runtime-benchmarks.md`.

## Verification

- The five `native_run_cache_*` tests pass under default test parallelism.
- The complete CLI integration suite passes under default parallelism:
  274 passed, 0 failed.
- The complete default-parallel Rust workspace gate passes, including 931
  compiler unit tests and every integration, fixture, package, scheduler-model,
  and documentation test.
- The deterministic contention regression holds the exact content-key lock,
  starts four processes, observes the flushed wait line while all four are
  blocked, then proves exactly one rebuild, four successful program results,
  one published entry, and a subsequent verified hit with `CC` and `CARGO`
  deliberately unavailable.
- Focused behavior tests also pin unrelated and same-key warm hits while a key
  lock is held, one-document JSON failure, buffered JSON wait progress,
  automatic-fallback reporting, and an uncached installed direct run.
  The timed warm-hit regression uses the installed immutable-runtime fixture,
  so parallel Cargo activity cannot change the runtime archive identity while
  the test deliberately holds the exact cache-key lock. Production runtime
  identity remains strict and content-derived.
- Broad serialization was removed from `npm run test:rust`. The instrumented
  compiler-coverage wrapper retains single-threaded libtest execution because
  the default-parallel probe passed every behavior test but undercounted
  function coverage at 96.86%. The serialized run restored the stable result
  to 4201/4336 functions (96.886531%) while retaining the 15 known LLVM
  mismatched-profile warnings. Dedicated forced-backend parity,
  scheduler-stress, and sanitizer runners also retain their narrow ordering
  constraints.
- The behavior-focused closure tests pin the exact AU2999 enum/literal match
  diagnostic and canonical editor inference for a specialized user generic
  class. The latter raised covered functions from 4201 to 4202 without a
  synthetic execution-only test. Final exact compiler coverage is
  64670/67265 lines (96.142124%), 4202/4336 functions (96.909594%), and
  94996/100674 regions (94.360014%), clearing the frozen
  96.13/96.89/94.35 floors.
- The exact final-tree `npm run ci` gate is green: format; the default-parallel
  Rust workspace; the 529.82-second forced MIR/direct fixture matrix; all 79
  language-server tests with 100% coverage; all 13 extension tests; compiler
  coverage; reference integrity and the retired-syntax sweep; docs build; npm
  and Rust audits; Clippy with warnings denied; and hygiene. The Rust audit
  retains the existing allowed `rustls-pemfile` unmaintained warning.
- This checkpoint change lands B4.0-a/b and its behavior-focused coverage
  closure as one isolated commit family.
- Phase 5.1 focused verification is green: 970 compiler library tests, all 22
  reactor primitive tests, five adversarial scheduler-model tests, the exact
  mixed-wakeup run-pass fixture and MIR/direct parity probe, and the expanded
  fairness/cancellation/mixed stress runner. Product Clippy with warnings
  denied, reference integrity (34 pages, 246 fences, 118 verified blocks), and
  the docs build pass. The 3-run focused scheduler stress also passed before
  the final audit hardening, and a final 1-run sweep passed afterward.
- Behavior-focused regressions cover lost-wake registration interleavings,
  stale epochs, deduplication and one-winner cleanup, persistent fd interest,
  terminal/error readiness, stale timer-heap compaction, transactional
  close-before-cancel cleanup, direct Queue/task/cancellation wakeups, exact
  cancellation/source/timeout/fd precedence, teardown cleanup, and reactor
  admission while another task continuously yields. The obsolete scan/pollfd
  and manually injected dormant waiter tests were removed.
- Maintained documentation now describes the reactor without claiming later
  safepoints, smaller stacks, multicore, Transfer, typed select, configurable
  pool sizing, or native frames. All executable fence bytes and ordering are
  unchanged.
- The first clean implementation benchmark at `7420bc2` exposed remaining
  timer-path overhead and a worker-side formatting observer effect. The
  performance closure `1de9cf7` coalesces reactor wakes, makes source
  subscription and Queue-transition work bounded, removes duplicate fired-wait
  cleanup, adds scalar direct clock/sleep ABIs, and moves benchmark formatting
  after primitive observation and task-group join.
- The corrected workload was replayed against the pre-reactor binary: all five
  runs had 18 ms arm spans and raw p99 overshoot of 11-12 ms. The contractual
  clean-tree after result passes every gate: 204,128,256-byte worst sleeper
  RSS; 4-5 ms arm spans and 3-4 ms timer p99; and 0.000012315% worst idle CPU.
  Both raw report hashes and full V6 observations are recorded in
  `work/2026-07-27-phase5-runtime-benchmarks.md`.
- Frozen compiler coverage passes at 65,732/68,369 lines
  (96.142989%), 4,333/4,472 functions (96.891771%), and
  97,052/102,827 regions (94.383771%), above the unchanged
  96.13/96.89/94.35 floors. The closure contains only observable reactor,
  scheduler, native-ABI, HTTP-diagnostic, and malformed-MIR diagnostic tests.
  No synthetic test or coverage exclusion was added. The timer-sequence
  exhaustion, descriptor-token exhaustion, poisoned-lock recovery, and
  monotonic-clock `i64` overflow remain justified defensive branches: inducing
  them would require corrupting private counters, poisoning an internal lock,
  or waiting beyond a practical process lifetime. Three duplicated
  scheduler-poll error closures were replaced by one tested diagnostic helper
  instead of manufacturing unreachable failures.
- Exact `npm run ci` is green on the Phase 5.1 closure tree: 275 CLI tests,
  970 compiler library tests, the serialized forced-backend fixture matrix, 79
  LSP tests, 13 extension tests, compiler and LSP coverage, reference
  execution, all 683 migration manifests, docs build, npm and cargo audits,
  warning-denied Clippy, and hygiene. The first two attempts exposed test
  watchdogs that were too short under default-parallel cold native compilation:
  isolated and CPU-saturated replays proved the programs and cache protocols
  completed correctly, so the affected bounded watchdogs were raised while
  preserving their ability to catch genuine lock waits and scheduler hangs.
  Every formerly timing-sensitive test then passed both the normal and
  instrumented CLI suites. Cargo audit retains only the repository's allowed
  `rustls-pemfile` unmaintained warning.
- Phase 5.2 exposes `yield_now() -> None` as a zero-argument builtin backed by
  the scheduler's existing ready-tail requeue operation. MIR and direct calls
  share that implementation; direct codegen uses a void runtime ABI rather
  than allocating a boxed Unit value. The call keeps the task runnable, does
  not wait for an event or deadline, does not inspect cancellation, and does
  not promise that a different task or any particular runnable task executes
  next.
- The Phase 5.2 behavior matrix pins AU2004 for arguments, AU2007 for builtin
  redefinition, compiler/LSP completion and hover, Unit inference, MIR
  dispatch, malformed-MIR argument rejection, the direct void ABI, and a
  bounded fairness fixture whose success requires an already-runnable sibling
  to make progress. Focused Rust fixtures, MIR and direct example execution,
  80 LSP tests at 100% coverage, 13 extension tests, reference integrity, and
  the docs build are green. The complete serialized forced-backend fixture
  matrix also passes in 761.72 seconds with no MIR/direct mismatch.
- The clean `d22ae10` after-stage benchmark is contractual with empty competing
  process inventories and passes every existing gate: 205,799,424-byte worst
  sleeper RSS; 4-9 ms timer arm spans and 2-5 ms p99; and 0.000020959% worst
  idle CPU. The full V6 comparison, raw report path, and SHA-256 are recorded
  in `work/2026-07-27-phase5-runtime-benchmarks.md`.
- Frozen compiler coverage passes without a closure pass at 65,767/68,407
  lines (96.140746%), 4,335/4,474 functions (96.893160%), and
  97,103/102,880 regions (94.384720%), above the unchanged
  96.13/96.89/94.35 floors. Every added test pins public diagnostics,
  scheduler progress, tooling metadata, MIR behavior, or native ABI behavior;
  no synthetic test or coverage exclusion was added.
- Exact full `npm run ci` is green on the committed Phase 5.2 implementation:
  275 CLI tests, 971 compiler library tests, the complete forced MIR/direct
  parity matrix, 80 LSP tests, 13 extension tests, both coverage gates,
  executable reference integrity, the docs build, audits, warning-denied
  Clippy, and hygiene. Cargo audit retains only the repository's allowed
  `rustls-pemfile` unmaintained warning.
- Phase 5.3 adds explicit MIR safepoint instructions at the latch of every
  `while` and `for` shape, including `enumerate`, `zip`, Queue iteration, and
  mutable Vec iteration. Normal loop tails and `continue` traverse the latch;
  `break` and `return` bypass it. Mutable Vec writeback and index advancement
  remain before the latch. MIR yields every eight traversed latches. Native
  code uses a per-function unboxed fuel counter, checks every backedge, and
  calls the existing void `aurora_direct_yield_now` ABI every 4,096 latches.
  Native modules with no possible sibling task retain the MIR marker but
  statically elide the runtime check.
- Behavioral regressions prove that a 200 ms hot loop no longer blocks a
  queued sibling, a 10 ms timer, or an armed loopback socket on either backend.
  The dedicated benchmark records the pre-safepoint failure as
  `SAMPLE starvation 10 200`; the current direct and MIR probes complete the
  same 10 ms sleeper before the hot loop ends. Structural tests cover all loop
  forms, nested latches, `continue`/`break`, mutable Vec ordering, malformed
  MIR, and the direct void-ABI and sequential-elision shapes.
- Phase 5.3 focused gates are green: 277 CLI tests, 979 compiler library tests,
  the complete forced MIR/direct parity matrix, 25 benchmark-runner tests,
  reference integrity, documentation, formatting, and diff hygiene. Frozen
  compiler coverage passes without a closure pass at 65,842/68,478 lines
  (96.150589%), 4,337/4,476 functions (96.894549%), and 97,258/103,032 regions
  (94.395916%). Every new test pins observable progress, diagnostic, MIR
  placement, writeback ordering, parity, or native ABI behavior; no synthetic
  test, exclusion, or unreachable-branch fixture was added.
- A contained pre-existing defect found while auditing loop exits remains a
  follow-up: error propagation through `try` inside mutable Vec iteration can
  bypass loop writeback on both backends. Explicit `return`, `break`, and
  `continue` write back correctly. This is outside the safepoint behavior and
  is recorded rather than silently absorbed into Phase 5.3.
- The clean `a339c61` after-stage benchmark is contractual and passes every
  gate: 204,193,792-byte worst 10,000-sleeper RSS; 4 ms timer arm spans and
  2 ms p99; 0.000011333% worst idle CPU; and 14-18 ms starvation results
  against the 50 ms limit. The 21-sample native int64 median is 16.793333 ms,
  23.377% faster than the accepted Phase 5.2 baseline and safely below its
  22.355170 ms two-percent ceiling. Exact report provenance and SHA-256 are in
  `work/2026-07-27-phase5-runtime-benchmarks.md`.
- Exact full `npm run ci` is green on the committed Phase 5.3 tree: 277 CLI
  tests, 979 compiler library tests, the complete forced MIR/direct parity
  matrix in 547.42 seconds, 80 LSP tests, 13 extension tests, compiler and LSP
  coverage, executable reference integrity, all 683 migration manifests, docs,
  npm and cargo audits, warning-denied Clippy, and hygiene. Cargo audit retains
  only the repository's allowed `rustls-pemfile` unmaintained warning.
- Phase 5.4 stack-diet investigation found that the 1 MiB default was a
  containment measure for deep host-library frames, not an Aurora-frame
  requirement. DNS/connect work and WebSocket handshakes are already
  offloaded, but HTTP URL/build/parsing, rustls handshake/data paths, and
  WebSocket framing still execute on coroutine stacks. The implementation
  target is a dedicated bounded protocol-step service: each bounded
  nonblocking host-library step temporarily owns its protocol state and is
  awaited to completion, while descriptor readiness, deadlines, and
  cancellation remain reactor-owned. Whole-operation blocking-pool wrappers
  are rejected because they can abandon owned protocol state, starve unrelated
  filesystem/resolver jobs, or deadlock through nested connect work.
- `corosensei::DefaultStack` already provides the required inaccessible guard
  page. On the contractual Mac14,9 host the page size is 16 KiB. The provisional
  override surface is collision-free
  `TaskGroup.start_with_stack(bytes, target, args...)` and
  `start_soon_with_stack(...)`; this avoids stealing a named argument from the
  child function.
- The contractual pre-change report comes from a clean detached worktree at
  baseline commit `5af134a2b1be9b54771e43f36ac355c68882c002`, using a fresh
  release build. Raw report: `/tmp/aurora-phase54-before.json`; SHA-256:
  `405f3acb61126aed87ee6bebdb0d2abb3e98feef9f3992f6f0d42e32bffdfb2f`.
  The 10,000-sleeper control passes with 204,193,792-byte worst
  whole-process peak RSS and 196,935,680-byte worst incremental peak RSS. The
  new 100,000-sleeper plus 1,000-timer gate is the required red proof:
  1,980,628,992-byte worst whole-process peak RSS and 1,972,830,208-byte
  incremental peak RSS exceed the 1.5 GiB ceiling. Its 4 ms arm span and 5 ms
  p99 still pass. The ordinary timer gate passes at 5 ms arm span and 3 ms
  p99; idle CPU peaks at 0.000019655072722165167%; starvation peaks at 12 ms;
  and the five-run native int64 median is 14.373750 ms.
- The implemented default is guarded 512 KiB, not the investigation's initial
  256 KiB target. Explicit overrides remain guarded and deterministic from
  256 KiB through 64 MiB; 256 KiB is reserved for measured shallow tasks and
  is never implied by an ordinary task start. Both task-start surfaces carry
  the same behavior through MIR and direct lowering.
- The 512 KiB selection came from the complete compiled Aurora HTTP example:
  making 256 KiB the global default left both the language-execution frames
  and the protocol path on that capacity and terminated with `SIGBUS`;
  512 KiB completed. The distinct isolated Rust runtime round trip forces only
  its direct protocol-calling children to 256 KiB and now completes. That
  narrower result proves the deep protocol frames moved to service-worker
  stacks, but excludes MIR/direct language-execution frames and is not
  evidence for a 256 KiB global default.
- Deep HTTP, rustls, and WebSocket host-library frames now execute through a
  dedicated bounded protocol-step service with two 2 MiB-stack workers and a
  bounded queue. It does not replace reactor readiness: descriptor waits,
  deadlines, cancellation, and protocol-state transitions remain owned by the
  scheduler protocol. Generic filesystem/resolver blocking work remains on
  the separate generic pool.
- Dynamic `json.parse` uses another dedicated service rather than consuming
  the protocol or generic pool. It has two 2 MiB-stack workers and exactly two
  in-flight operations. Admission precedes the fallible source copy, non-task
  callers wait on a condition variable, lightweight-task callers park on
  availability, and an RAII reservation restores capacity after completion or
  failure. Parsing on the service is paired with iterative runtime conversion,
  JSON writing, rendering, and canonical `json.Value` cloning so the supported
  depth does not reintroduce recursive host frames on a 512 KiB coroutine
  stack. Cancellation of an admitted synchronous JSON parse remains deferred
  until the operation publishes its result, preserving the existing
  synchronous call contract. The legacy `json.is_valid` and
  `json.parse_string_map` helpers retain their bounded caller-side paths and
  never enter this service; `json.stringify_map` remains caller-side too.
- Exact full `npm run ci` is green on the complete Phase 5.4 implementation:
  280 CLI tests, 1,007 compiler library tests, the complete forced MIR/direct
  parity matrix in 543.05 seconds, 81 LSP tests, 13 extension tests, both
  coverage gates, executable reference integrity, all 683 migration
  manifests, docs, npm and cargo audits, warning-denied Clippy, and hygiene.
  Cargo audit retains only the repository's allowed `rustls-pemfile`
  unmaintained warning.
- Frozen compiler coverage passes at 67,159/69,851 lines (96.146082%),
  4,446/4,587 functions (96.926095%), and 99,186/105,100 regions
  (94.372978%), above the frozen 96.13/96.89/94.35 floors. LSP coverage is
  100% across statements, branches, functions, and lines. Every closure test
  pins observable behavior, a stable diagnostic, or backend parity; no
  synthetic line-execution test or exclusion was added.
- The clean contractual post-change report at commit `0dddb43` is
  `/private/tmp/aurora-phase54-after.json` (SHA-256
  `5245595a6675dba0cc1e39383dda505e50d7333cb59fbc3afea4c648fcca0ab4`).
  Its fresh release `aura` SHA-256 is
  `972e29088fc34d12cd0373e21d3d7a4f33bd4e3dd635f13eaeb51bb44bc306f0`.
  The 10,000-sleeper gate passes at 205,389,824 bytes worst whole-process RSS
  and 197,836,800 bytes worst incremental RSS, an amortized upper bound of
  19,784 bytes (19.32 KiB) per requested sleeper. Independent timers pass at
  3 ms arm span and 3 ms p99, idle CPU at
  0.000013142653912887135%, starvation at 14 ms, and the V6 int64 median is
  11.884417 ms.
- The 100,000-sleeper plus 1,000-timer workload passes its 3 ms timer gates but
  reaches 1,978,384,384 bytes worst whole-process RSS and 1,970,782,208 bytes
  worst incremental RSS. Phase 5's explicit benchmark escape hatch applies:
  on this 16 KiB-page host, one resident page for each of the 101,000
  stackful children alone is 1,654,784,000 bytes, already above 1.5 GiB before
  metadata. Lowering the demand-paged virtual reservation cannot make the
  ceiling robust. The massive-concurrency claim remains out of maintained
  product documentation; a later stackless or safe stack-copy/decommit
  architecture is required to revisit it.
- Phase 5.4 is complete through `f72fd2f`. The implementation, exact full CI,
  frozen coverage result, contractual after-stage benchmark, scope-qualified
  512 KiB default, and massive-concurrency escape-hatch evidence are all
  recorded above. Coverage floors remain frozen at 96.13/96.89/94.35.
- Phase 5.5 scheduler soundness is implemented and verified in `ea92897`.
  The previous nested-spawn path reconstructed a second live mutable scheduler
  reference from a raw `*mut LightweightTaskScheduler`; that aliased
  `&mut *scheduler` pattern is replaced by an owned FIFO request broker
  whose only mutable scheduler consumer is the scheduler driver. FIFO is an
  internal admission invariant, not a language scheduling-order guarantee.
- A nested start now prepares its guarded stack and task state synchronously
  before publishing an owned request. Preparation failure returns the existing
  error immediately and publishes no request. The scheduler drains prepared
  requests after each task resume, after forced cleanup or unwind, and
  repeatedly during teardown, preserving nested start followed by immediate
  wait without exposing scheduler mutation to the running task.
- Unbounded-wait state is published atomically on `TaskState` only after wait
  registration succeeds and is cleared when that registration is removed.
  Group cleanup therefore no longer reads scheduler internals through a shared
  raw alias. Task context is owned and cloned around callbacks and suspension;
  no `RefCell` borrow is retained across a coroutine yield.
- Scheduler teardown disarms waits, drains pending starts, retires admitted and
  prepared tasks, transitions exposed task handles to `Cancelled`, and
  notifies completion, group, and reactor observers. Pure Rust/MIR coroutine
  frames are force-unwound. Generated direct frames are never Rust-unwound:
  started child tasks and direct roots reset their coroutine stack and then
  release scheduler-owned argument storage, claim flags, retained opaque
  values, and task-local direct-runtime state exactly once. Unstarted direct
  tasks drop their entry closure normally and release external state exactly
  once. Cleanup runs with the task context installed, so requests produced by
  host-state cleanup are admitted and retired before teardown finishes.
- This forced-abandonment path is host/runtime-state containment, not a new
  Aurora cleanup mechanism. It does not execute arbitrary Aurora cleanup code
  in an abandoned generated task, and programs must not rely on it as an
  alternative to maintained control-flow, cancellation, or runtime-failure
  cleanup.
- Current Phase 5.5 focused verification is green: the compiler library suite
  passes 1,017 tests; scheduler regressions cover synchronous preparation
  failure, nested admission and immediate wait, wait-state publication,
  teardown cancellation/wakeup, broker release, pure-Rust unwind, and cleanup
  that publishes another request; native-runtime regressions cover direct
  child/root forced exit, queued and suspended teardown, retained-reference
  release, and normal completion without double release. The targeted
  `scheduler_nested_spawns.au` CLI regression passes on MIR and forced-direct
  backends, and the hygiene gate rejects both a raw scheduler pointer and
  reconstructed `&mut *scheduler`.
- The exact Phase 5.5 full CI is green on the settled implementation tree:
  281 CLI tests, 1,017 compiler-library tests, the 547.91-second forced
  MIR/direct fixture matrix, 81 language-server tests, 13 extension tests,
  reference integrity (34 pages, 247 fences, 118 verified blocks, 59 migration
  tests, and 683 migrated manifests), documentation build, both audits,
  warning-denied Clippy, and hygiene. `cargo audit` reports only the accepted
  `rustls-pemfile 2.2.0` unmaintained warning.
- Frozen compiler coverage passes without a synthetic line-execution test or
  justified exclusion: 67,266/69,957 lines (96.153351%), 4,454/4,596
  functions (96.910357%), and 99,304/105,216 regions (94.381083%), above the
  frozen 96.13/96.89/94.35 floors. LSP coverage remains 100%: 895/895
  statements and lines, 246/246 branches, and 49/49 functions.
- The clean contractual after-stage benchmark was captured from `ea92897` on
  the Mac14,9 baseline. All gates pass except the already-recorded
  massive-concurrency RSS escape hatch: 10,000 sleepers peak at 206,815,232
  bytes, standalone timer p99 is 2 ms, worst timer arm span is 5 ms, idle CPU
  is 0.000018876%, and starvation latency is 13 ms. The 100,000-sleeper plus
  1,000-timer workload peaks at 1,962,000,384 bytes with 4 ms arm span and
  4 ms p99. The contractual report, full before/after table, provenance, and
  SHA-256 are recorded in `work/2026-07-27-phase5-runtime-benchmarks.md`.
- Phase 5.5 is complete. Its measured outcome supports a soundness change
  without material performance regression; it does not add a performance or
  massive-concurrency marketing claim.

## Phase 5.6 implementation gate

Phase 5.6 structural Transfer and static single-consumer task-result
enforcement is implemented and has passed its complete repository gate:

- The checker derives Transfer from fully resolved specialized types rather
  than a user trait. Copy values, `String`, and recursively transferable
  collections, tuples, classes, and enums pass. Shared or mutable capability
  views, `random.Rng`, `TaskGroup`, and live filesystem, process, network,
  HTTP, WebSocket, TLS, and similar host resources fail. Owned snapshot data
  remains classified by its stored fields.
- `Queue[T]` and `Task[T]` handles are transferable independently of their
  payload because the handle names synchronized state. Queue construction and
  `put`/`try_put` separately require a transferable payload. The four
  TaskGroup start surfaces check every captured argument and the specialized
  task result before scheduling. Nested failures use `AU3008` and name the
  boundary plus the field, element, or payload path to the non-Transfer leaf.
- Result repeatability is static. `Task[T]` is copyable only when `T` is copy
  data, a Queue handle, or a recursively repeatable Task handle. `result`,
  `result_or_none`, and `result_or` consume a non-repeatable Task binding on
  every outcome; `wait_any` and `wait_all` consume the complete task vector.
  `AU3009` rejects clone, collection access, or aggregate/container copying
  that would duplicate the unique observation right. Later binding use and
  shared-access consumption retain the existing moved-value and borrow
  diagnostics.
- MIR start instructions and direct-native task entry metadata carry the
  result-repeatability bit into task creation. MIR and direct observation
  paths both use an atomic one-winner runtime claim as defense in depth, so a
  backend defect or foreign handle cannot clone or return the same
  non-repeatable stored result twice. TaskGroup cleanup does not claim a
  source-level result.
- Provisional ADR-0033 records the structural rules and completion matrix,
  with corresponding amendments to ADR-0008 and ADR-0020. Semantic/runtime
  architecture documents, the language manual, diagnostic catalog, editor
  behavior, and API descriptions have been updated. The fixture matrix covers
  transferable aggregates and recursive data, generic specialization, Queue
  payloads, capability and host-resource leaves, repeated observations,
  alias/container/branch/loop escape attempts, both multi-wait helpers, and a
  MIR/direct runtime matrix.

Final review additionally closed two material gaps:

- `Range` is explicitly Transfer without becoming Copy. Behavioral coverage
  sends a Range through `Queue[Range]`, passes and returns one across a task
  boundary, and observes the non-repeatable result on both backends.
- Maintained prose no longer calls Queue/Task internals synchronized before
  Phase 5.7. It states the implemented transferable/copyable handle-identity
  contract and makes thread-safe cross-worker internals a Phase 5.7
  requirement. A same-named user `Transfer` trait remains an ordinary user
  trait and does not confer the compiler-derived property.

Focused and final evidence is green:

- 9/9 compiler fixture-harness tests
- 19/19 call-metadata tests
- 85/85 language-server tests with 100% coverage: 895/895 statements and
  lines, 246/246 branches, and 49/49 functions
- 13/13 MIR `task_group_` tests plus four focused specialization and move tests
- imported-class, associated-method, and native-object tests
- CLI structural-Transfer MIR/direct parity
- the single-observer JSON MIR/direct cleanup regression
- direct TCP and Unix ownership smoke tests
- executable reference integrity and the documentation build

The exact final-tree `npm run ci` gate is green: formatting; 282 CLI tests;
1,056 compiler-library tests; the serialized forced MIR/direct fixture matrix
in 565.37 seconds; 85 language-server tests and compiler-bridge coverage at
100%; 13 extension tests; compiler coverage; executable reference integrity
(34 pages, 247 fences, 118 verified blocks, 59 migration tests, and 683
migrated manifests); docs build; npm and Rust audits; warning-denied Clippy;
and hygiene. Cargo audit retains only the accepted `rustls-pemfile 2.2.0`
unmaintained warning.

Frozen compiler coverage passes at 68,580/71,330 lines (96.144680%),
4,525/4,670 functions (96.895075%), and 101,189/107,171 regions
(94.418266%), above the unchanged 96.13/96.89/94.35 floors. All new tests pin
observable semantics, diagnostics, runtime containment, editor behavior, or
backend parity. No synthetic coverage test or exclusion was added.
Unreachable defensive paths were restructured into checked-MIR and
validated-type invariants rather than exercised artificially: redundant direct
Task-target reconstruction was removed, and validated Transfer nominal/arity
fallbacks are assertions.

Phase 5.7 pinned-worker multicore remains pending. Phase 5.6 establishes the
boundary contract while execution remains single-worker and makes no parallel
execution claim.

The contractual clean-tree after-stage benchmark is recorded from
`7dcdd70aa54bdae01a61d83ce867a2020fec4909`. Ten thousand sleepers, standalone
timers, idle CPU, starvation latency, and V6 controls all pass. The
100,000-sleeper workload remains the sole red gate under the accepted Phase
5.4 escape hatch, while its timer controls pass. The report is
`/private/tmp/aurora-phase56-after-transfer.json` with SHA-256
`209baaf5264fe469db9f88c2c7aa235fce2d2505e3d233eb0baad69fbe060bb7`;
full provenance and measurements are in
`work/2026-07-27-phase5-runtime-benchmarks.md`.

## Phase 5.7 pinned-worker multicore

The runtime now owns N OS worker threads, defaulting to
`std::thread::available_parallelism()` and accepting a strict positive
`AURORA_WORKERS` override. Each worker owns one scheduler, reactor, ready
queue, wait table, and every coroutine assigned to it. Prepared task entries
are `Send`, but admitted coroutines are not: round-robin inbox publication
chooses the permanent worker before coroutine construction, so stackful
coroutines never migrate.

The coordinator provides globally unique task IDs, durable per-worker inboxes,
control wakeups, root-completion/fatal shutdown, partial worker-start cleanup,
and final inbox draining. Queue, Task, TaskGroup, cancellation, result claims,
and native opaque values use synchronized shared state. Each task keeps a
task-local cancellation context; only explicit TaskGroup cancellation,
failure, and completion signals are shared. Direct runtime diagnostics remain
worker-local and keyed by the globally unique task ID. Generated-task forced
cleanup runs exactly once on the task's pinned worker after admission; a
request assigned to a worker that fails to start is instead drained by the
supervisor. Normal idle workers still block on `reactor.poll(None)`; no
periodic fallback tick was accepted.

Task creation has an explicit prepare-register-publish invariant. MIR and
direct adapters register TaskGroup membership and every captured Queue
producer through a synchronous pre-submit callback after fallible stack/state
preparation and before the prepared request enters any worker inbox. A
deterministic runtime test asserts that remote entry cannot observe
registration as incomplete. This closes both structured-cleanup loss and
Queue iteration's premature no-producer conclusion for immediately completing
tasks.

The behavioral matrix includes:

- stable worker identity across `yield_now`, Queue wake, and timer wake
- simultaneous CPU progress on distinct workers
- one-winner non-repeatable Task result claims
- cross-worker completion, cancellation, and distinct failure diagnostics
- shutdown racing a nested start without a Running handle or double cleanup
- four-producer/four-consumer Queue integrity over 800 items, including no
  loss, no duplication, and producer-local FIFO without a global producer
  order
- complete per-call output lines under four-worker MIR and direct execution
- explicit single-worker preservation for tests whose contract is local
  cooperative fairness rather than global execution order

The mandatory multicore workload uses an exact READY/GO/DONE/ACK protocol,
fixed Park-Miller checksums, four explicit workers for both lanes, seven
alternating one-task/four-task pairs, minimum signal duration, CPU
corroboration, core qualification, and MAD rejection. The first calibration
measured the wall gate green but correctly invalidated CPU evidence at 9.5%.
Investigation proved the macOS runner treated `proc_pid_rusage` mach
absolute-time ticks as nanoseconds. The fixed runner applies the host
`mach_timebase_info` ratio (`125/3` here), with a regression test. The fresh
calibration is valid and passes: paired median ratio `1.061645x`, ratio of
medians `1.058870x`, four-task median `0.576568s`, one-task median
`0.544513s`, four-task median CPU `397.21%`, and relative MAD below `0.004`.

Focused verification is green: 1,072 compiler-library tests under default
parallelism; 118 native-runtime tests twice under default parallelism; the
four-worker Queue/Task, stress, cancellation/failure, and atomic-output
fixtures on MIR and direct; AU4006 diagnostics on both backends; one-worker
fairness fixtures; 45 benchmark-runner tests; formatting; warning-denied
Clippy; and diff hygiene. Frozen compiler coverage passes at 69,108/71,883
lines (96.139560%), 4,581/4,726 functions (96.931866%), and
101,829/107,849 regions (94.418122%), above the unchanged
96.13/96.89/94.35 floors. The closure tests pin worker reactor-init failure,
partial worker-start failure, cleanup-panic containment, coordinator shutdown
accounting, zero-worker AU4006, and direct Queue producer registration. No
synthetic test or coverage exclusion was added. Exact full CI, the isolated
implementation commit, and the clean-tree contractual full benchmark remain
before Phase 5.7 sign-off.

Exact full `npm run ci` is green on the final Phase 5.7 implementation tree:
45 benchmark-runner tests; 288 CLI tests and 1,072 compiler-library tests
under the default-parallel Rust workspace; the complete serialized forced
MIR/direct fixture matrix in 559.03 seconds; 85 LSP tests; 13 extension tests;
compiler coverage at the totals above; LSP coverage at 100%; reference
integrity over 34 pages, 247 fences, and 118 verified blocks; all 683 migration
manifests and the stale-syntax sweep; docs build; npm and Rust audits;
warning-denied Clippy; and hygiene. Cargo audit retains only the repository's
allowed `rustls-pemfile` unmaintained warning.

Phase 5.7 is committed at `6fb5efb`. Its clean-tree contractual benchmark is
also complete. The report records the exact implementation commit, no dirty
files, empty competing-process inventories, and `contractual: true`. The
mandatory four-worker gate passes all seven paired samples: paired median
ratio `1.077123x` against the `1.6x` ceiling, ratio of medians `1.056700x`,
four-task median `0.593762s`, one-task median `0.561902s`, and `393.61%`
median four-task process CPU. Ten thousand sleepers, standalone timers, idle
CPU, starvation, and V6 controls also pass. The 100,000-sleeper plus
1,000-timer workload reaches 1,989,033,984 bytes worst RSS and remains the
sole red gate under the accepted Phase 5.4 escape hatch; its 5 ms arm span and
3 ms p99 still pass. Raw report:
`/private/tmp/aurora-phase57-after-pinned-worker-multicore.json`, SHA-256
`6d47c90d3dd9eb85421245c92aa3d12b01cb58ddf9ac0819b0e210c14123531d`;
qualified release binary SHA-256
`9e81f90221d41899e017a3a6fbafd8dfaccdbb74a4884c4246aa448610aa0591`.

One follow-up was deliberately not absorbed. Direct compilation rejected an
`int32 != int32` comparison whose operand came from a function result while
MIR accepted it; the fixture uses equivalent positive equality. Worker-thread
spawn and reactor-init failure cleanup are now fault-injected and proven to
terminalize pending handles and run cleanup exactly once, including when
cleanup itself panics. A persistent kernel `mio::Waker` failure would require
a second registered control primitive for formal recovery; the implementation
keeps control state durable, retries terminal shutdown notification, and does
not weaken the no-periodic-tick contract.

## Phase 5.8 typed heterogeneous select

Provisional ADR-0034 landed before implementation at `ec3fd61`. The language
now exposes variadic positional `select(source, ...)` over Queue, Task, and
relative-Duration sources, returning `SelectOutcome[Q, T]`. Source expressions
evaluate once from left to right. Cancellation wins before ready sources;
otherwise the lowest original argument index wins. Non-repeatable Task
observation rights are consumed at call entry and abandoned when they lose,
while Queue and repeatable Task handles remain reusable.

MIR and direct execution share one scheduler primitive. It validates every
source before capturing one common deadline base, claims Task observation
rights in source order, uses a composite check-subscribe-recheck wait, and
removes all losing Queue, Task, deadline, and cancellation registrations
before returning. A panic during subscription rolls back registrations and
the reactor wait before unwinding. Atomic Queue receive commits a winner that
later cancellation cannot replace. Cross-worker notifications use the
existing direct reactor/inbox wake path; no polling tick, helper task,
coroutine migration, or backend-specific arbitration loop was added.

Both runtime adapters retain and validate typed source descriptors as defense
in depth. Malformed MIR and direct tuples trap with `AU4001` for missing or
inconsistent metadata, descriptor/value mismatches, malformed generic arity,
mixed Queue payload types, and mixed Task result types. The direct ABI remains
the internal owned-tuple `aurora_direct_select(tuple_ptr)` contract.

Focused verification is green: 40 select-named compiler tests, all nine
fixture families, four forced MIR/direct four-worker CLI parity tests, 89
language-server tests including the typed-select compiler bridge, 13 extension
tests, 119 verified reference blocks, the documentation build, formatting,
and diff hygiene. Behavioral coverage includes Queue item/closed/duplicate
and loser preservation, Task ready/error/child cancellation/repeatable reuse,
deadlines and common-base timing, cancellation and index priority, source
evaluation order, non-repeatable ownership diagnostics, registration races,
concurrent Queue/Task publication with exactly one waiter enqueue, atomic
receive races, cross-worker wakeup, late publication, committed-winner
stability, selected non-repeatable Task delivery on both backends, invalid
named-call analysis withholding, and unwind cleanup. No synthetic coverage
test or exclusion was added.

The frozen compiler-coverage gate is green at 69,985/72,794 lines
(96.141165%), 4,634/4,779 functions (96.965892%), and 103,033/109,068 regions
(94.466755%), above the unchanged 96.13/96.89/94.35 floors. The behavior-only
closure additionally pins nested Transfer derivation for both outcome payload
categories, unresolved and invalid analysis recovery, non-cloneable Task
results, inline non-repeatable Task rights, post-validation deadline overflow,
committed closed-Queue priority, direct source inference, and malformed native
ABI descriptors. No synthetic coverage test or coverage exclusion was added.
The source-count-over-`i32::MAX` guard and native-codegen fallbacks for
malformed checked MIR remain justified uncovered defensive branches. The
first requires allocating more than 2.1 billion runtime values. The latter
cover empty or named select calls, absent operand types, malformed Queue/Task
arity, inconsistent payload types, and non-source operands that semantic
checking and normal MIR lowering cannot produce. The runtime MIR adapter and
external direct ABI metadata guards are behavior-tested. No instrumentation or
source exclusion was added.

The final implementation tree passes the complete gate: 45 benchmark-runner
tests, 292 CLI product tests, 1,105 compiler library tests, the full forced
MIR/direct fixture matrix in 820.09 seconds, 89 language-server tests, 13
extension tests, compiler and 100% LSP coverage, reference integrity with 119
verified blocks and all 683 migration manifests, the docs build, npm and Rust
audits, warning-denied Clippy, and hygiene. Cargo audit retains only the
repository's allowed `rustls-pemfile` unmaintained warning.

The first final-tree attempt exposed a pre-existing test-order race: a MIR TCP
helper queried `peer_addr` after `shutdown_write`, allowing the server to close
first under default-parallel contention. Backtrace-guided stress reproduced
the failure repeatedly across 320 invocations at concurrency 16. Moving the
live address assertions before shutdown made all 320 pass, along with all 49
parallel MIR-runtime sibling tests; no runtime code or timeout changed.

The repository hygiene command is green when run against the Phase 5.8
snapshot. The main worktree also contains unrelated user-owned trailing spaces
in `personal/file_ops.au`; that file was temporarily isolated for this one
gate and restored byte-identically (SHA-256
`70c359fe35e5b7c82ecba741d54f8b7b5374fb3244de2f20b50c3832cdc3a32d`).
It is excluded from the implementation commit. The isolated implementation
commit and clean-tree contractual benchmark remain before Phase 5.8 sign-off.

## Follow-up

Phase 5.7 benchmark evidence is committed at `f601fc7`. Provisional ADR-0034
landed separately at `ec3fd61` before implementation and defines Phase 5.8's
variadic positional `select` over Queue, Task, and relative-Duration sources,
the typed `SelectOutcome[Q, T]` result, cancellation-first/lowest-index
arbitration, non-repeatable Task observation consumption, and mandatory atomic
registration plus loser cleanup. Phase 5.8's focused implementation and frozen
coverage gates and exact full CI are green. The isolated implementation commit
and its clean-tree benchmark remain.

The massive-concurrency memory claim remains unavailable under the recorded
escape hatch. Coverage floors remain frozen until the one-time Batch 4
sign-off re-ratchet.
