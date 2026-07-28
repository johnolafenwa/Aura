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
- The clean contractual post-change benchmark and its evidence commit remain
  pending. Phase 5.4 is not accepted until that report is recorded.

## Follow-up

Commit the fully gated implementation, then capture the clean contractual
post-change report and publish the measured whole-process and incremental cost
per parked task. Do not advance to scheduler soundness until that benchmark
and its evidence commit are recorded. Coverage floors remain frozen until the
one-time Batch 4 sign-off re-ratchet.
