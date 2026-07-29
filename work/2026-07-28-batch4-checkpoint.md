# Batch 4 checkpoint: Phase 5 scalable runtime

- Date: 2026-07-28
- Status: complete; exact-clean settled-tree CI passed at `77c999d`
- Entry checkpoint: `1c249ab`
- Stop boundary: Batch 4 is closed; Phase 6 / Batch 5 has not started

## Outcome

Batch 4 has closed the four B4.0 findings and implemented Phase 5 in the
ratified order: persistent reactor, public `yield_now()`, loop safepoints,
guarded lightweight stacks, scheduler soundness, structural Transfer and
static single-consumer results, pinned-worker multicore, typed heterogeneous
select, the configurable blocking-I/O pool, and native structured diagnostic
frames.

The implemented semantic matrix preserves MIR/direct behavior parity and keeps
the language's resource and ownership claims explicit. Phase 5.10's committed
implementation at `181204b` has passed exact-clean full CI, its final
instrumented measurement, and every contractual benchmark gate except the
100,000-sleeper RSS ceiling. That ceiling is below the host's proved physical
one-page-per-child floor, so Part 3's ratified escape hatch applies and the
associated massive-concurrency claim has been removed from maintained
documentation. The one-time compiler-coverage re-ratchet is 96.13% lines,
96.90% functions, and 94.46% regions. Exact-clean full CI passed on the
checkpoint-documentation commit `77c999d`.

## B4.0 disposition

| Ticket | Disposition | Evidence |
| --- | --- | --- |
| B4.0-a cross-process cache/runtime contention | Closed at `665d540` | Cross-process runtime-identity and content-key locks give concurrent cold direct runs one builder plus verified consumers; the five cache tests and a deterministic four-process regression pass under default parallelism. |
| B4.0-b silent rebuild/wait operations | Closed at `665d540` | Human stderr flushes exact rebuild/wait notices before the long operation. JSON mode preserves one structured document and carries buffered progress through automatic MIR fallback. |
| B4.0-c capability diagnostic polish | Closed at `4f0461e` | AU3001/AU3002/AU3003/AU3005 use capability-aware current syntax. The provisional AU3006 clone-safety gap-fill is recorded for review. |
| B4.0-d checkpoint count precision | Closed at `5cb4476` | Gate-count claims state the debug/single-threaded conditions that explain the reconciled suite counts. |

B4.0's exact settled CI passed the full forced-backend matrix, 79 LSP tests at
100% coverage, 13 extension tests, both coverage gates, reference integrity,
documentation, audits, warning-denied Clippy, and hygiene. Its final compiler
coverage was 64,670/67,265 lines (96.142124%), 4,202/4,336 functions
(96.909594%), and 94,996/100,674 regions (94.360014%). Ordinary Rust-test
serialization was removed; only instrumentation, parity, stress, and
sanitizer constraints that require deterministic isolation remain.

## Phase 5 stage ledger

| Stage | Commit family | Gate disposition |
| --- | --- | --- |
| 5.1 persistent reactor | `850e906`, `7420bc2`, `1de9cf7`, `df104fa` | Persistent `mio` registration, timer heap, keyed wake subscriptions, and direct Queue/task/pool wakeups. All stage gates and exact CI pass. |
| 5.2 `yield_now()` | `d22ae10`, `57e3816` | Public documented builtin on both backends; all stage gates and exact CI pass. |
| 5.3 loop safepoints | `a339c61`, `af03d15`, `f8fcf8` | One compiler-inserted cooperative latch per loop, with native fuel checks and sequential elision. Starvation and V6 gates pass. |
| 5.4 stack diet | `5af134a`, `0dddb43`, `f72fd2f` | Guarded 512 KiB default, explicit 256 KiB–64 MiB override, and bounded protocol/JSON services. The stage-local 100,000-task gate uses the ratified RSS escape hatch. |
| 5.5 scheduler soundness | `ea92897`, `015db33` | Aliased scheduler access removed; spawn is prepared and queued before the scheduler admits it. All soundness, cleanup, and stage gates pass. |
| 5.6 structural Transfer | `7dcdd70`, `8d7f984` | Compiler-derived Transfer and static single-consumer task-result claims land before multicore; ADR-0033 and both-backend matrix pass. |
| 5.7 pinned-worker multicore | `6fb5efb`, `f601fc7` | Tasks are assigned before coroutine construction and never migrate; synchronized cross-worker handles and wake paths pass all seven paired multicore trials. |
| 5.8 typed select | `ec3fd61`, `3e15b8a`, `dcb7667` | ADR-0034 builtin-style heterogeneous wait, atomic one-winner selection, deterministic arbitration, and loser cleanup pass both backends. |
| 5.9 configurable blocking pool | `cc450c9`, `d921313`, `7df4df2` | ADR-0035 environment/configuration, FIFO admission, acceptance boundary, and outage saturation matrix pass; every contractual gate is green. |
| 5.10 native frames | `ad6bef6`, `29ff7f6`, `1e1263d`, `e171420`, `c3278c4`, `181204b` | ADR-0036 typed call/task frames, exact MIR/direct parity, and private native JSON transport are implemented. `e171420` pins observable null-metadata rejection; `c3278c4` prebuilds boxed child task state outside the coroutine and narrows scope installation; `181204b` pins nested ancestry restoration. Exact-clean CI and final coverage are green. Every contractual benchmark gate passes except massive RSS; Part 3's explicit performance escape hatch applies because the host's physical floor already exceeds the target before metadata. |

## Contractual benchmark evidence

All accepted reports were captured on the dedicated Mac14,9 host with an
Apple M2 Pro, 10 logical/physical CPUs, 16 GiB RAM, macOS 26.5.2 (Darwin
25.5.0), clean repository provenance, and empty Aurora build/runtime process
inventories. Exact commands, binary identities, individual samples, and raw
report hashes are preserved in
`work/2026-07-27-phase5-runtime-benchmarks.md`.

| Stage | Accepted commit/report | 10,000-task RSS | 100,000 tasks + 1,000 timers | Timer arm / p99 | Idle CPU | Starvation | Multicore |
| --- | --- | ---: | ---: | --- | ---: | ---: | --- |
| Before reactor | `850e906`; `6bbe066e…` | 189.641 MiB | not yet gated | 13–15 ms / raw 8–10 ms, fail | 0.018886% | not yet gated | not yet gated |
| 5.1 | `1de9cf7`; `81318b6c…` | 204,128,256 B | not yet gated | 4–5 ms / 3–4 ms | 0.000012315% | not yet gated | not yet gated |
| 5.2 | `d22ae10`; `1db729bd…` | 205,799,424 B | not yet gated | 4–9 ms / 2–5 ms | 0.000020959% | not yet gated | not yet gated |
| 5.3 | `a339c61`; `a25589f6…` | 204,193,792 B | not yet gated | 4 ms / 2 ms | 0.000011333% | 18 ms | not yet gated |
| 5.4 | `0dddb43`; `5245595a…` | 205,389,824 B | 1,978,384,384 B, escape hatch | 3 ms / 3 ms | pass | pass | not yet gated |
| 5.5 | `ea92897`; `d0f3f96a…` | 206,815,232 B | 1,962,000,384 B, escape hatch | 5 ms / 2 ms | 0.000018876% | 13 ms | not yet gated |
| 5.6 | `7dcdd70`; `209baaf5…` | 205,209,600 B | 1,855,143,936 B, escape hatch | 5 ms / 2 ms | 0.000013550% | 14 ms | not yet gated |
| 5.7 | `6fb5efb`; `6d47c90d…` | 206,503,936 B | 1,989,033,984 B, escape hatch | 7 ms / 3 ms | 0.000529682% | 14 ms | 1.077123x paired median; 393.61% CPU |
| 5.8 | `3e15b8a`; `f72889aa…` | 206,585,856 B | 1,720,057,856 B, escape hatch | 6 ms / 1 ms | 0.000635340% | 18 ms | 1.020775x; 398.54% CPU |
| 5.9 | `d921313`; `d9947ddc…` | 206,962,688 B | 1,457,848,320 B, pass | 4 ms / 3 ms | 0.001074300% | 18 ms | 1.020214x; 398.49% CPU |
| 5.10 | `181204b`; `8ba448a0…` | 207,798,272 B | 2,001,305,600 B, escape hatch | 6 ms / 1 ms | 0.001675461% | 14 ms | 1.039673x; 396.73% CPU |

The original Phase 5.10 report at `29ff7f6` is retained as rejected evidence:
`/private/tmp/aurora-phase510-after-native-frames.json`, SHA-256
`012feee3e8a840c1c59c3c503572188b7bc4a8cc72b1ac3f30b5bb53f49f4528`.
Its 2,040,184,832-byte massive peak was a real regression, not an accepted
escape hatch. The `1e1263d` correction retains immutable compact metadata,
materializes owned strings only on the first trap, shares ancestry, tears down
deep ancestry iteratively, and reduces native call-entry stack use from 528
to 208 bytes.

The second report, from the exact-clean `e171420` tree, passes every
contractual benchmark gate except massive RSS. Its
three peaks are 1,095,598,080, 1,629,388,800, and 1,831,649,280 bytes against
the 1,610,612,736-byte limit. It is retained at
`/private/tmp/aurora-phase510-after-native-frames-compact.json`, SHA-256
`64868869cec520b438594214d8b62e0691cf921b8d062a1337fd0be82280ca60`.
The large run-to-run spread exposed a remaining 1,312-byte task-scope setup
frame that dirties child coroutine pages before suspension.

The third correction at `c3278c4` moves each task's runtime state behind a
stable `Box`, reducing the worker-local map value from the full state to one
pointer. Spawn now constructs the pristine boxed state on the spawning task
before the child entry exists; the child moves only the prepared box into an
`#[inline(never)]` scope installer before user code begins. The narrow
`PreparedDirectTaskRuntimeState` transfer wrapper is `Send` only because its
cleanup stack and owned-value registry are empty at construction, it contains
no live raw-pointer registrations, and it is consumed exactly once on the
selected pinned worker before user code can populate those collections. The
live `DirectTaskRuntimeState` itself was not made `Send`. Allocation-identity
and pointer-size regression coverage pins the indirection, while ancestry,
isolation, task-key reuse, forced-exit, cancellation, and cleanup tests pin the
state lifecycle. Focused verification is green. The exact-clean CI and the
full corrected benchmark report at this commit are green except for the
massive-RSS result described below.

The final contractual report was captured from clean commit
`181204b02ca419d3f8cad683e8a0015499a4363b` with three workload repetitions,
three timer repetitions, five V6 repetitions, seven paired multicore
repetitions, and a 30-second idle sample. The freshly qualified locked release
`aura` SHA-256 is
`50503389792f7f86efb8f021f983a3917855bad82e4fbc90b99414695331142a`.
Raw report:
`/private/tmp/aurora-phase510-after-native-frames-state-prebuilt.json`;
SHA-256:
`8ba448a06a8efb505af723ed00b8248fc1aa44ed270b46df5c15d74ecb9bd986`.
The distilled evidence in
`/private/tmp/aurora-phase510-final-summary.json` has SHA-256
`edd1026137e2c800e7d63499c4104c38aa536673d87136732564e57530c6f304`.

The three 10,000-sleeper peaks were 207,798,272, 206,946,304, and
206,831,616 bytes, all below 512 MiB. Standalone timer runs had a 6 ms maximum
arm span and 1 ms p99 overshoot; idle CPU was 0.001675461%; starvation was
14 ms. All seven multicore pairs passed, with a 1.039672549x paired median,
1.041327720x ratio of medians, and 396.734801% median four-task process CPU.
The three massive-workload peaks were 1,170,735,104, 1,921,531,904, and
2,001,305,600 bytes; their timer behavior still passed at a 3 ms maximum arm
span and 2 ms p99.

Massive RSS is the sole raw benchmark failure. Part 3's ratified escape hatch
applies after the compact-metadata and prebuilt-boxed-state corrections:
Darwin's 16 KiB page size means one resident page for each of the workload's
101,000 stackful children requires at least 1,654,784,000 bytes, already above
the 1,610,612,736-byte (1.5 GiB) gate before task metadata, scheduler state,
reactor state, allocator overhead, or the parent process is counted. The
measured best is recorded, but a stable result below that physical floor is
not achievable with the Phase 5 stackful architecture. The
massive-concurrency marketing claim is therefore withdrawn from maintained
README/manual/tutorial content. A future stackless or safe
copy-and-decommit architecture may re-open the claim; Phase 5 does not make
it.

## Scheduler architecture boundary for Batch 5 callables

- The runtime has N pinned OS workers. Each worker owns its scheduler,
  reactor, ready queue, wait table, and admitted coroutines.
- Worker selection happens before coroutine construction. A corosensei stack
  is never `Send`, migrated, or stolen; only a fully prepared task entry may
  cross a synchronized worker inbox.
- Spawn is `prepare -> register -> publish`: all fallible stack/state work,
  TaskGroup membership, Queue producer registration, Transfer validation, and
  result-claim setup finish before publication.
- The coordinator owns globally unique task IDs, durable worker inboxes and
  wakers, root/fatal shutdown, partial-start cleanup, and final drains.
- Queue, Task, TaskGroup, cancellation/failure/completion signaling,
  non-repeatable result claims, and native opaque-handle state are synchronized.
  Task identity, capability state, and cleanup remain task-owned.
- Typed select uses one composite check-subscribe-recheck registration,
  cancellation-first then lowest-index arbitration, an atomic one-winner
  commit, and idempotent loser cleanup.
- Direct diagnostic and call-frame state is worker-local and task-keyed by the
  global task ID. Cleanup and forced reset happen once on the pinned owner.
- A future callable may cross workers only as an owned, prepared Transfer
  entry. It may not contain a coroutine stack, capability view, non-Transfer
  capture, TaskGroup, host resource, `random.Rng`, borrowed/in-loan capture, or
  worker-local runtime/diagnostic state. Capture specialization and return
  Transfer checks must precede publication. `FnOnce` invocation and
  non-repeatable results require the existing atomic one-winner discipline.

## Provisional decisions for review

These remain explicitly provisional until checkpoint review; this report does
not silently ratify them.

| Decision | Recommendation | Checkpoint note |
| --- | --- | --- |
| ADR-0032 guarded lightweight task stacks | Accept as written | The 512 KiB default, override range, guard behavior, bounded services, diagnostics, both backends, cleanup, and stage evidence match the decision. Phase 5.9 produced a raw passing sample, but the final repeated evidence proved that sample compression-dependent; the checkpoint invokes the escape hatch and withdraws the massive-concurrency claim. |
| ADR-0033 structural Transfer and task results | Accept as written, with Batch 5 callable follow-up | The current static named-callable surface is complete. Callable values must derive Transfer through owned captures and must not launder non-Transfer state. |
| ADR-0034 typed heterogeneous select | Accept as written | Type shape, evaluation order, cancellation/lowest-index arbitration, atomic claim, loser cleanup, both backends, and evidence match. Statement sugar remains deferred. |
| ADR-0035 configurable blocking-I/O pool | Accept as written | Configuration, initialization, FIFO admission, acceptance semantics, saturation matrix, both backends, and clean benchmark match. Accepted host work remains non-preemptive, as documented. |
| ADR-0036 native structured runtime frames | Accept as written | The semantic and transport matrix, exact MIR/direct parity, clean CI, and all mandatory performance gates are complete. The sole raw failure is the 100,000-sleeper RSS ceiling, dispositioned through Part 3's explicit escape hatch with the associated marketing claim removed. Exact-clean settled-tree CI passed at `77c999d`. |

Two smaller provisional B4.0 gap-fills also remain before the reviewer:
buffering native rebuild/wait notices in JSON mode, and extending AU3006 with
clone-safe wording.

## Coverage and one-time re-ratchet

The one-time Batch 4 compiler re-ratchet is complete at 96.13% lines, 96.90%
functions, and 94.46% regions. The exact `181204b` clean-tree instrumented
measurement is 71,153/74,016 lines (96.131917%), 4,757/4,909 functions
(96.903646%), and 104,478/110,598 regions (94.466446%). The retained report is
`/private/tmp/aurora-phase510-state-prebuilt-coverage-closure.json`, SHA-256
`e66a3db6c7f94f5cd2ea966c19717538b646ea6c001d9fa873b03bf44f219ddd`.
LSP coverage remains 100%: 897/897 lines/statements, 245/245 branches, and
49/49 functions.

The exact-clean implementation CI at `181204b` passed 1,150 compiler library
tests, 300 CLI tests, the complete forced MIR/direct parity matrix, 90 LSP
tests, 13 extension tests, compiler and LSP coverage gates, reference
integrity, documentation build, npm and cargo audits, warning-denied Clippy,
and hygiene. Its log is
`/private/tmp/aurora-phase510-state-prebuilt-coverage-closure-ci.log`,
SHA-256
`0776403c16bd356cb46d42b1e3dcc19c0c09a0ebf29be0e0cf2e405f6fa6c910`.
Exact-clean settled-tree CI at `77c999d` reran the complete gate after the
checkpoint documentation and floor update were committed. It passed the
45-test benchmark harness, 300 CLI tests, 1,150 compiler library tests, the
complete forced MIR/direct parity matrix in 685.76 seconds, 90 LSP tests,
13 extension tests, compiler and 100% LSP coverage, reference and stale-syntax
integrity, the documentation build, npm and cargo audits, warning-denied
Clippy, formatting, and hygiene.

No synthetic line-execution test or coverage exclusion was added during
Phase 5.10. Defensive branches were either tested through observable
diagnostics and parity behavior or structurally removed when checked MIR made
them unreachable. The coverage closure used observable null-metadata
rejection, and the settled ancestry addition pins restoration behavior rather
than line execution. The iterative ancestry teardown and compact/prebuilt
task-state changes are implementation restructures, not coverage exclusions.
There is no justified-exclusion list.

## Work moving to Batches 5 and 6

Batch 5 / Phase 6 should begin with a contained B5.0 closure for two compiler
defects already recorded during Phase 5:

1. `try` propagation can bypass mutable-Vec writeback on both backends.
2. The direct backend rejects `int32 != int32` when an operand is a function
   result, while MIR accepts it.

Callable design must absorb ADR-0013/ADR-0022 capture rules and ADR-0033
Transfer. A safe order is capture-free function values, then move-only
`FnOnce`, then borrowed/in-loan captures only after the new place,
lifetime, and escape proposal. The retired `borrow[label]` syntax must not
return. Unresolved generic Transfer obligations remain conservatively rejected
unless Batch 5 deliberately designs deferred callable obligations.

Slices and comprehension breadth remain in Batch 6 / Phase 7 because slices
depend on the settled place/view model and comprehensions depend on callable
and lambda foundations. FFI callbacks remain after callable ownership and ABI
are complete. Persistent kernel `mio::Waker` failure recovery remains a Batch
6 hardening item unless a second control primitive is deliberately designed.
Select statement sugar is later breadth, not a Batch 5 prerequisite.

## Stop

The Batch 4 implementation, semantic, benchmark-disposition, and coverage
work is complete at this checkpoint. Exact-clean settled-tree CI passed at
`77c999d`. Phase 6 / Batch 5 has not started, and no Phase 6 implementation or
design work is authorized as part of Batch 4 closeout.
