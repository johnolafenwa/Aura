# Phase 5 scalable-runtime benchmarks

Date: 2026-07-27.

## Purpose

This note records the dedicated before/after measurements for every Phase 5
runtime stage. Measurements use standalone direct-backend binaries so compiler,
cache, and link work remain outside the measured interval. Raw machine-readable
results are written outside `target/` and retained separately from disposable
build artifacts.

The Batch 4 calibration host is:

- model: Mac14,9
- CPU: Apple M2 Pro
- logical CPUs: 10
- memory: 16 GiB
- operating system: macOS 26.5.2 (25F84)

The host runner records the exact commit and dirty paths, Aura release binary,
commands, platform details, repetitions, per-run measurements, validation
results, and aggregates. Contractual runs require a quiet machine and reject
other Aurora-repository `cargo`, `rustc`, or `aura` processes unless the
operator explicitly marks the result non-contractual.

## Workloads

- `10k_sleepers.au`: 10,000 tasks parked at once; measure peak RSS.
- `1000_timers.au`: 1,000 independent 10 ms sleeps; measure p50, p95, p99, and
  maximum overshoot, and reject runs whose start-time span does not demonstrate
  an overlapping load.
- `idle_10_tasks.au`: five queue waits and five timer waits; measure process CPU
  only inside the flushed 30-second ready window.
- V6 `int32` and `int64` ten-million-iteration loops: alternate widths after
  warmup and record every observation plus median, MAD, p95, and best.

The maintained workload protocol is documented in
`benchmarks/scalable_runtime/README.md`. Any unexpected output, missing ready or
done marker, wrong V6 result, duplicate timer index, nonzero exit, timeout, or
invalid timer overlap invalidates that sample.

## Gates

- 10,000 sleeping tasks: at most 512 MiB peak RSS.
- 1,000 10 ms timers: at most 5 ms p99 overshoot.
- 10 blocked tasks: less than 2% CPU during the stable idle window.
- Safepoints: no more than approximately 2% median native `int64` loop overhead
  relative to the preceding accepted stage.
- Pinned-worker multicore: four CPU-bound tasks complete within 1.6 times the
  wall time of one task on four cores. Failure of this gate is stop-worthy.
- “Massive concurrency” remains unavailable unless 100,000 sleepers fit within
  1.5 GiB with stable timers.

## Before-reactor baseline

Runtime source checkpoint: `665d540`.

Benchmark harness checkpoint: `850e906`.

The first contractual quiet-machine run used:

```bash
cargo build --release -p aura
npm run bench:scalable-runtime -- \
  --label before-reactor \
  --aura target/release/aura \
  --repeats 3 \
  --timer-repeats 5 \
  --v6-repeats 7 \
  --idle-seconds 30 \
  --json /tmp/aurora-phase51-before.json
```

The report records an empty process inventory both before workload builds and
immediately before timing, a clean repository at `850e906`, and
`contractual: true`. The measured release `aura 0.1.0` binary is 12,052,208
bytes with SHA-256
`5fedd7bb82f5a2f60ffb1e40cf066460f85ac7b8cfaf0080662a8acc7f85c625`.
The raw report is `/tmp/aurora-phase51-before.json`, SHA-256
`6bbe066ebcf49ac4a2a67f05578eb841e0e9a83490e52077ad671cac4d787bf8`.

| Workload | Repetitions | Before-reactor result | Gate |
| --- | ---: | --- | --- |
| 10,000 sleepers | 3 | peak RSS 189.641 MiB worst; individual peaks 189.641, 189.453, 189.391 MiB | PASS, at most 512 MiB |
| 1,000 timers | 5 | all runs invalid for overlap; arm spans 15, 14, 14, 14, 13 ms; raw per-run p99 overshoot 10, 9, 8, 9, 8 ms | FAIL, no valid-overlap p99 and arm span exceeds 10 ms |
| 10 idle tasks | 3 | CPU 0.018459%, 0.018552%, 0.018886%; worst 0.018886% | PASS, less than 2% |
| V6 int32 loop | 7 plus warmup | median 32.734250 ms; MAD 0.093291 ms; p95 45.655833 ms; best 32.629417 ms | baseline evidence |
| V6 int64 loop | 7 plus warmup | median 10.248625 ms; MAD 0.103334 ms; p95 12.658583 ms; best 10.145166 ms | baseline evidence |

The timer failure is the intended pre-reactor finding, not a harness failure.
All five complete raw sample sets are retained. Because none armed within the
10 ms overlap bound, the runner correctly refuses to manufacture a contractual
p99 from them; the 8–10 ms figures above remain diagnostic raw observations.
The sleeper and idle workloads completed naturally with exact `DONE` lines,
zero exit status, empty standard error, and no sampling errors.

The required before-reactor evidence is now complete. Phase 5.1 may begin with
failing reactor lifecycle tests; no runtime implementation changed before this
measurement.

## Phase 5.1 reactor

### Measurement correction and like-for-like replay

The first clean-tree after-reactor run at `7420bc2` correctly failed the timer
gate: all five runs had 17-18 ms arm spans and 15-16 ms p99 overshoot. Profiling
then found that the timer worker formatted its complete `SAMPLE` string after
recording its own end timestamp. Aurora tasks are cooperative, so that
post-observation formatting delayed the next ready worker and was charged to
the next timer's overshoot. This was an observer effect in the workload, not
timer latency.

The maintained protocol now keeps worker observations primitive. Workers send
their start and overshoot `int64` values; the parent aggregates start extrema,
joins the task group, and only then formats output. `READY` carries the raw
minimum and maximum start values, and each `SAMPLE` carries a unique
observation index plus the raw overshoot. The runner still requires exactly
1,000 independent 10 ms timers, a start span at most 10 ms, every raw sample,
and the worst valid-run p99 at most 5 ms.

For a comparable before value, the corrected workload was replayed with the
clean pre-reactor `850e906` Aura binary (the runtime remains the B4.0
`665d540` implementation). The active source checkout stayed clean at
`1de9cf7`; the report records the detached Aura binary separately:

- Aura SHA-256:
  `b306814b2f91ea0bf3548bc27b8cc53d7fa04e101994d146f6d2ed2a6d6e6cb7`
- raw report: `/tmp/aurora-phase51-before-corrected.json`
- report SHA-256:
  `fa3cce0adff08c9eee7cf3dca720a62f6adfac174a1c0cff6dc15a59aa23b7bf`
- contractual quiet-machine result: true, with no non-contractual reasons

All five corrected pre-reactor timer runs had an 18 ms arm span, so the runner
rejected them as non-overlapping. Their retained raw p99 values were 11, 12,
11, 11, and 11 ms. The replay also passed sleeper memory at 198,606,848 bytes
worst and idle CPU at 0.025282% worst. Its later V6 observations are retained
in the raw report, but the original pre-stage V6 measurements above remain the
accepted stage baseline because the timer-only protocol correction does not
alter those workloads.

### Accepted after-reactor result

The accepted implementation is the clean commit family `7420bc2` plus the
timer-latency closure `1de9cf7`. The closure coalesces reactor Waker syscalls,
uses keyed source subscriptions and transition-triggered Queue wake broadcasts,
bounds nonblocking poll cadence, avoids initializing the dormant host
scheduler, removes duplicate fired-wait cleanup, and adds unboxed direct
monotonic-clock and void-return sleep ABIs. The corrected timer protocol
removes reporting work from the observed callback path without changing the
load or threshold.

The contractual run used:

```bash
cargo build --release -p aura
npm run bench:scalable-runtime -- \
  --label after-reactor-final \
  --aura target/release/aura \
  --repeats 3 \
  --timer-repeats 5 \
  --v6-repeats 7 \
  --idle-seconds 30 \
  --json /tmp/aurora-phase51-after-final.json
```

The report records commit
`1de9cf72de85b0d6f0b0ef530a41ca5d74724e98`, no dirty files, empty competing
process inventories, `contractual: true`, and no non-contractual reasons. Raw
report SHA-256:
`81318b6ce566e7715223e807c3092c7953fa43035c7184fe9ab5b9d29b502951`.

| Workload | Repetitions | Accepted after-reactor result | Gate |
| --- | ---: | --- | --- |
| 10,000 sleepers | 3 | peak RSS 204,128,256 bytes worst; individual peaks 203,816,960, 203,898,880, 204,128,256 bytes | PASS, at most 512 MiB |
| 1,000 timers | 5 | arm spans 5, 5, 5, 5, 4 ms; per-run p99 4, 4, 3, 4, 4 ms; worst p99 4 ms | PASS, arm span and p99 both at most 5/10 ms |
| 10 idle tasks | 3 | CPU 0.000012315%, 0.000003830%, 0.000012063%; worst 0.000012315% | PASS, less than 2% |
| V6 int32 loop | 7 plus warmup | median 53.286542 ms; MAD 0.544208 ms; p95 54.133958 ms; best 51.869125 ms | recorded reactor-stage evidence |
| V6 int64 loop | 7 plus warmup | median 20.858292 ms; MAD 0.518751 ms; p95 21.769000 ms; best 19.968583 ms | recorded reactor-stage evidence |

Every sleeper, timer, and idle process completed naturally with its exact
`DONE` marker, zero status, empty standard error, and no sampling error. No
Phase 5 benchmark escape hatch is needed for the reactor stage.

## Phase 5.2 `yield_now()`

The accepted Phase 5.1 reactor result above is the before-stage measurement.
The Phase 5.2 implementation does not change any existing benchmark workload:
none of the sleeper, timer, idle, or V6 sources call `yield_now()`. The
after-stage run nevertheless re-executes the complete protocol so unintended
scheduler or code-generation regressions remain visible.

The contractual run used:

```bash
cargo build --release -p aura
npm run bench:scalable-runtime -- \
  --label after-yield-now \
  --aura target/release/aura \
  --repeats 3 \
  --timer-repeats 5 \
  --v6-repeats 7 \
  --idle-seconds 30 \
  --json /tmp/aurora-phase52-after-yield-now.json
```

The report records commit
`d22ae10c5d7096bbc978812c25d0bc44d0bedc6f`, no dirty files, empty competing
process inventories, `contractual: true`, and no non-contractual reasons. Raw
report SHA-256:
`1db729bde174f92c6b8da5752f33a01735c8a0d471de831e73cd30ec4dfff9aa`.

| Workload | Repetitions | Accepted after-`yield_now` result | Gate |
| --- | ---: | --- | --- |
| 10,000 sleepers | 3 | peak RSS 205,799,424 bytes worst; individual peaks 205,799,424, 204,046,336, 203,997,184 bytes | PASS, at most 512 MiB |
| 1,000 timers | 5 | arm spans 7, 6, 9, 4, 5 ms; per-run p99 4, 3, 4, 5, 2 ms; worst p99 5 ms | PASS, arm span and p99 both at most 10/5 ms |
| 10 idle tasks | 3 | CPU 0.000020959%, 0.000012223%, 0%; worst 0.000020959% | PASS, less than 2% |
| V6 int32 loop | 7 plus warmup | median 53.354167 ms; MAD 0.649124 ms; p95 55.018833 ms; best 51.944750 ms | recorded stage evidence |
| V6 int64 loop | 7 plus warmup | median 21.916833 ms; MAD 0.539417 ms; p95 22.654208 ms; best 20.744291 ms | recorded stage evidence |

All four contractual gates pass. Every process completed naturally with the
expected protocol marker, zero status, empty standard error, and no sampling
error. No Phase 5 benchmark escape hatch is needed for the `yield_now` stage.
The subsequent exact full `npm run ci` gate is green: 275 CLI tests, 971
compiler library tests, forced MIR/direct parity, 80 LSP tests, 13 extension
tests, compiler and LSP coverage, reference integrity, documentation, audits,
warning-denied Clippy, and hygiene.

## Phase 5.3 automatic loop safepoints

The accepted Phase 5.2 report above is the before-stage baseline. Its native
int64 median is 21.916833 ms, so the exact two-percent acceptance ceiling is
22.35516966 ms.

The new starvation workload arms a 10 ms sleeper before entering a 200 ms hot
loop with no explicit scheduler operation. The preserved Phase 5.2 direct
binary reports:

```text
SAMPLE starvation 10 200
DONE starvation
```

This is the intended red proof: before compiler safepoints, the sleeper cannot
resume until the loop finishes. The Phase 5.3 runner executes the probe three
times and gates the worst observed completion at 50 ms. It also repeats the V6
native loops 21 times because their accepted baseline variation is close to the
permitted two-percent change.

The clean-tree after-stage command is:

```bash
cargo build --release -p aura
npm run bench:scalable-runtime -- \
  --label after-safepoints \
  --aura target/release/aura \
  --repeats 3 \
  --timer-repeats 5 \
  --v6-repeats 21 \
  --idle-seconds 30 \
  --json /tmp/aurora-phase53-after-safepoints.json
```

The accepted report records clean commit
`a339c61503a358842acb2601e42f5d195b25a749`, no dirty files, empty competing
process inventories, `contractual: true`, and no non-contractual reasons. Raw
report path: `/tmp/aurora-phase53-after-safepoints.json`. SHA-256:
`a25589f602e7c30a1e9be1fce75d468ea7e704676ec299cab98b01c542d2428e`.

| Workload | Repetitions | Accepted after-safepoint result | Gate |
| --- | ---: | --- | --- |
| 10,000 sleepers | 3 | peak RSS 204,193,792 bytes worst; individual peaks 204,062,720, 204,013,568, 204,193,792 bytes | PASS, at most 512 MiB |
| 1,000 timers | 5 | arm span 4 ms in every run; p99 2 ms in every run | PASS, arm span and p99 both at most 10/5 ms |
| 10 idle tasks | 3 | CPU 0.000010609%, 0.000011116%, 0.000011333%; worst 0.000011333% | PASS, less than 2% |
| 10 ms sleeper beside hot loop | 3 | 14, 18, 18 ms; worst 18 ms | PASS, at most 50 ms |
| V6 int32 loop | 21 plus warmup | median 47.882667 ms; MAD 0.075792 ms; p95 48.405584 ms; best 47.518792 ms | recorded stage evidence |
| V6 int64 loop | 21 plus warmup | median 16.793333 ms; MAD 0.788209 ms; p95 17.985083 ms; best 15.824000 ms | PASS, 23.377% faster than the accepted Phase 5.2 median and below the 22.355170 ms ceiling |

Every process completed naturally with the exact protocol marker, zero status,
empty standard error, and no sampling error. All five contractual gates pass.
No Phase 5 benchmark escape hatch is needed for the safepoint stage. The
subsequent exact full `npm run ci` gate is green: 277 CLI tests, 979 compiler
library tests, the full forced-backend parity matrix, 80 LSP tests, 13
extension tests, compiler and LSP coverage, reference integrity, docs, audits,
warning-denied Clippy, and hygiene.

## Phase 5.4 coroutine stack diet

The stack-diet runner extends the accepted harness without weakening the
10,000-sleeper control. It records both whole-process peak RSS and the
increment above a pre-start baseline, and adds a 100,000-sleeper workload with
1,000 concurrently armed 10 ms timers. That massive-concurrency gate requires
no more than 1.5 GiB whole-process peak RSS, no more than a 10 ms timer arm
span, and no more than 5 ms p99 timer overshoot. A failing pre-change memory
gate is expected evidence for this stage; the benchmark escape hatch is not
being used to claim acceptance.

The contractual pre-change report was captured from a clean detached worktree
at commit `5af134a2b1be9b54771e43f36ac355c68882c002`. The runner qualified a
fresh locked release build whose `aura` SHA-256 is
`9385fdbe3d05d493f3f7acc7c76c6c545e50aa2abce3da9a8cec473351fc5484`.
The report records no dirty files, no competing processes, and
`contractual: true`.

```bash
/Applications/Xcode.app/Contents/Developer/usr/bin/python3 \
  scripts/bench-scalable-runtime.py \
  --label phase54-before-stack-diet \
  --aura target/release/aura \
  --json /tmp/aurora-phase54-before.json
```

Raw report: `/tmp/aurora-phase54-before.json`. SHA-256:
`405f3acb61126aed87ee6bebdb0d2abb3e98feef9f3992f6f0d42e32bffdfb2f`.

| Workload | Repetitions | Contractual pre-change result | Gate |
| --- | ---: | --- | --- |
| 10,000 sleepers | 3 | 204,193,792 bytes worst whole-process peak RSS; 196,935,680 bytes worst incremental peak RSS | PASS, whole-process peak at most 512 MiB |
| 100,000 sleepers plus 1,000 timers | 3 | 1,980,628,992 bytes worst whole-process peak RSS; 1,972,830,208 bytes worst incremental peak RSS; 4 ms worst arm span; 5 ms worst p99 | FAIL on the 1.5 GiB whole-process RSS gate; timer gates pass |
| 1,000 timers | 3 | 5 ms worst arm span; 3 ms worst p99 | PASS |
| 10 idle tasks | 3 | 0.000019655072722165167% worst CPU | PASS, less than 2% |
| 10 ms sleeper beside hot loop | 3 | 12 ms in every run | PASS, at most 50 ms |
| V6 int64 loop | 5 plus warmup | median 14.373750 ms; MAD 0.260250 ms; p95 15.700000 ms; best 13.963209 ms | recorded stage evidence |

Every workload process completed naturally with its expected marker, zero
status, empty standard error, and no sampling error. `all_gates_passed` is
false solely because the pre-change massive-concurrency memory result exceeds
the new limit.

The implementation now under verification uses guarded 512 KiB default
coroutine stacks. Explicit
`TaskGroup.start_with_stack`/`start_soon_with_stack` requests accept guarded
capacities from 256 KiB through 64 MiB; 256 KiB is an opt-in floor for measured
shallow tasks, not the default. Deep HTTP/rustls/WebSocket steps use a
dedicated bounded two-worker service with 2 MiB worker stacks. Dynamic
`json.parse` uses a separate two-worker, two-in-flight service with 2 MiB
stacks, bounded admission before source copying, and iterative
conversion/write/render/clone paths for supported-depth values. The legacy
`json.is_valid` and `json.parse_string_map` helpers remain bounded caller-side
compatibility operations rather than codec-service jobs. Focused 512 KiB
protocol, both-backend override, and supported-depth dynamic JSON checks have
been reported green.

The stack-selection evidence has two different scopes. The complete compiled
Aurora HTTP example terminated with `SIGBUS` when the experimental global
default was 256 KiB and completed at 512 KiB; that full workload includes the
MIR/direct language-execution frames. An isolated Rust runtime round trip now
completes with its direct protocol-calling children forced to 256 KiB. The
isolated result proves deep host protocol frames stay on 2 MiB service-worker
stacks, but it does not establish 256 KiB as safe for a complete compiled
Aurora task.

The implementation has passed exact full `npm run ci`: 280 CLI tests, 1,007
compiler library tests, the complete forced MIR/direct parity matrix in
543.05 seconds, 81 LSP tests, 13 extension tests, both coverage gates,
reference/migration/docs, audits, warning-denied Clippy, and hygiene. Frozen
compiler coverage is 67,159/69,851 lines (96.146082%), 4,446/4,587 functions
(96.926095%), and 99,186/105,100 regions (94.372978%); LSP coverage remains
100%. No synthetic coverage test or exclusion was added.

The contractual post-change report was captured from clean commit
`0dddb43ff83d96d9b1f847e62afb9aa0edf5fb92` on the same Mac14,9 M2 Pro,
with the same runner, workload hashes, parameters, and quiet-process checks as
the pre-change report. The fresh locked release `aura` SHA-256 is
`972e29088fc34d12cd0373e21d3d7a4f33bd4e3dd635f13eaeb51bb44bc306f0`.
Raw report: `/private/tmp/aurora-phase54-after.json`; SHA-256:
`5245595a6675dba0cc1e39383dda505e50d7333cb59fbc3afea4c648fcca0ab4`.

| Workload | Repetitions | Contractual post-change result | Gate |
| --- | ---: | --- | --- |
| 10,000 sleepers | 3 | 205,389,824 bytes worst whole-process peak RSS; 197,836,800 bytes worst incremental peak RSS; amortized incremental upper bound 19,784 bytes (19.32 KiB) per requested sleeper | PASS, whole-process peak at most 512 MiB |
| 100,000 sleepers plus 1,000 timers | 3 | 1,571,995,648 bytes best and 1,978,384,384 bytes worst whole-process peak RSS; 1,970,782,208 bytes worst incremental peak RSS; 3 ms worst arm span; 3 ms worst p99 | RSS FAIL against 1.5 GiB; timer gates PASS; escape hatch recorded |
| 1,000 timers | 3 | 3 ms worst arm span; 3 ms worst p99 | PASS |
| 10 idle tasks | 3 | 0.000013142653912887135% worst CPU | PASS, less than 2% |
| 10 ms sleeper beside hot loop | 3 | 14 ms worst result | PASS, at most 50 ms |
| V6 int64 loop | 5 plus warmup | median 11.884417 ms; MAD 0.974750 ms; p95 12.859167 ms; best 10.044125 ms | recorded stage evidence |

Every workload process completed naturally with its expected marker, zero
status, empty standard error, and no sampling error. All controls pass. The
massive-concurrency marketing claim remains unavailable under the explicit
escape hatch: this 16 KiB-page host needs at least 1,654,784,000 bytes for one
resident page across the workload's 101,000 stackful children, already above
1.5 GiB before task metadata. The 1 MiB-to-512 KiB change halves virtual
reservation but cannot remove that physical-page floor; the occasional lower
RSS run reflects macOS reclaim/compression variability and is not a stable
contract. Beating the ceiling requires a later stackless or safely
copy-and-decommit architecture, not a smaller Phase 5.4 reservation.

## Phase 5.5 scheduler soundness

The accepted Phase 5.4 post-stack-diet report is the before-stage baseline.
It records clean commit `0dddb43ff83d96d9b1f847e62afb9aa0edf5fb92`;
the later `f72fd2f` commit changes documentation only. Phase 5.5 is a
soundness refactor rather than a performance feature: it replaces aliased
scheduler mutation with owned prepared-task admission, makes teardown terminal
for every exposed task handle, and contains generated direct-task state without
Rust-unwinding through Cranelift frames. The complete workload suite was still
repeated so a performance regression could not hide behind that intent.

The clean-tree after-stage command was:

```bash
cargo build --release --locked -p aura --target-dir target
/Applications/Xcode.app/Contents/Developer/usr/bin/python3 \
  scripts/bench-scalable-runtime.py \
  --label phase55-after-scheduler-soundness \
  --aura target/release/aura \
  --repeats 3 \
  --timer-repeats 3 \
  --v6-repeats 5 \
  --idle-seconds 30 \
  --json /tmp/aurora-phase55-after-scheduler-soundness.json
```

The report records clean implementation commit
`ea928975d867a51771553602aa1eba51cd0ebd37`, no dirty files, empty competing
process inventories, `contractual: true`, and no non-contractual reasons. The
host remains the Mac14,9 Apple M2 Pro with 10 physical/logical cores and 16 GiB
RAM. All workload source hashes and runner parameters match the Phase 5.4
baseline. The freshly qualified locked release `aura` SHA-256 is
`1bf073ab90b26dadbf3c0bfeb18bce086b30728655c9e7c19965214d932c8def`.
Raw report: `/private/tmp/aurora-phase55-after-scheduler-soundness.json`;
SHA-256:
`d0f3f96a02a2280cac728b6da80f9a2e35c6f893ab22b591c4f73fe627749f89`.

| Workload | Repetitions | Contractual post-soundness result | Gate |
| --- | ---: | --- | --- |
| 10,000 sleepers | 3 | 206,815,232 bytes worst whole-process peak RSS; 199,458,816 bytes worst incremental peak RSS | PASS, whole-process peak at most 512 MiB |
| 100,000 sleepers plus 1,000 timers | 3 | 1,465,106,432 bytes best and 1,962,000,384 bytes worst whole-process peak RSS; 1,954,578,432 bytes worst incremental peak RSS; 4 ms worst arm span; 4 ms worst p99 | RSS FAIL against 1.5 GiB; timer gates PASS under the recorded escape hatch |
| 1,000 timers | 3 | arm spans 5, 3, 3 ms; p99 2 ms in every run | PASS |
| 10 idle tasks | 3 | 0.00001887614530740043% worst CPU | PASS, less than 2% |
| 10 ms sleeper beside hot loop | 3 | 13 ms worst result | PASS, at most 50 ms |
| V6 int32 loop | 5 plus warmup | median 32.511584 ms; MAD 0.089501 ms; p95 46.254458 ms; best 32.422083 ms | recorded stage evidence |
| V6 int64 loop | 5 plus warmup | median 9.970917 ms; MAD 0.045501 ms; p95 11.638750 ms; best 9.925416 ms | recorded stage evidence |

Every process completed naturally with its exact protocol marker, zero status,
empty standard error, and no sampling error. The contractual runner exits
nonzero solely because the already-accepted massive-concurrency RSS gate
remains unavailable; every other gate passes. Relative to the Phase 5.4
baseline, the 10,000-sleeper worst peak changes by less than 0.7%, the
massive-workload worst peak improves by about 0.8%, standalone timer p99
improves from 3 ms to 2 ms, and the other controls remain comfortably inside
their limits. These are evidence of no material regression, not new
performance claims. The one-resident-page stackful-coroutine floor and the
resulting restriction on massive-concurrency marketing are unchanged.

## Phase 5.6 structural Transfer

The accepted Phase 5.5 report above is the before-stage baseline. Phase 5.6
adds compiler-derived structural Transfer checks, static task-result
observation rights, and an atomic runtime claim for non-repeatable task
results. It intentionally does not add parallel execution; this benchmark
repeats every maintained runtime control to detect an unintended scheduler,
memory, timer, or native-code regression before pinned-worker multicore.

The clean-tree after-stage command was:

```bash
cargo build --release --locked -p aura --target-dir target
/Applications/Xcode.app/Contents/Developer/usr/bin/python3 \
  scripts/bench-scalable-runtime.py \
  --label phase56-after-transfer \
  --aura target/release/aura \
  --repeats 3 \
  --timer-repeats 3 \
  --v6-repeats 5 \
  --idle-seconds 30 \
  --json /tmp/aurora-phase56-after-transfer.json
```

The report records clean implementation commit
`7dcdd70aa54bdae01a61d83ce867a2020fec4909`, no dirty files, empty competing
process inventories, `contractual: true`, and no non-contractual reasons. The
host remains the Mac14,9 Apple M2 Pro with 10 physical/logical cores and
16 GiB RAM. The freshly qualified locked release `aura` SHA-256 is
`b50fc66fda17e39d97af3409dfa1d6bb1a40a5ab87e3bdf05df1dffd479fa716`.
Raw report: `/private/tmp/aurora-phase56-after-transfer.json`; SHA-256:
`209baaf5264fe469db9f88c2c7aa235fce2d2505e3d233eb0baad69fbe060bb7`.

| Workload | Repetitions | Contractual post-Transfer result | Gate |
| --- | ---: | --- | --- |
| 10,000 sleepers | 3 | 205,209,600 bytes worst whole-process peak RSS; 197,869,568 bytes worst incremental peak RSS | PASS, whole-process peak at most 512 MiB |
| 100,000 sleepers plus 1,000 timers | 3 | 1,438,973,952 bytes best and 1,855,143,936 bytes worst whole-process peak RSS; 1,847,754,752 bytes worst incremental peak RSS; 4 ms worst arm span; 4 ms worst p99 | RSS FAIL against 1.5 GiB; timer gates PASS under the recorded escape hatch |
| 1,000 timers | 3 | arm spans 5, 3, 3 ms; p99 2 ms in every run | PASS |
| 10 idle tasks | 3 | 0.00001354949145045805% worst CPU | PASS, less than 2% |
| 10 ms sleeper beside hot loop | 3 | 14 ms worst result | PASS, at most 50 ms |
| V6 int32 loop | 5 plus warmup | median 34.838167 ms; MAD 0.542583 ms; p95 47.171209 ms; best 34.295584 ms | recorded stage evidence |
| V6 int64 loop | 5 plus warmup | median 14.039166 ms; MAD 0.734291 ms; p95 15.000125 ms; best 12.093083 ms | recorded stage evidence |

Every process completed naturally with its exact protocol marker, zero status,
empty standard error, and no sampling error. The contractual runner exits
nonzero solely because the already-accepted massive-concurrency RSS gate
remains unavailable; every other gate passes. The 10,000-sleeper peak is
slightly below Phase 5.5, the massive-workload worst peak is about 5.4% lower,
standalone timer p99 remains 2 ms, and idle/starvation controls remain far
inside their limits. These measurements establish no material runtime
regression from the Transfer boundary. They do not claim multicore execution
or restore the massive-concurrency marketing claim.
