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

## Phase 5.7 pinned-worker multicore

The accepted Phase 5.6 report above is the before-stage baseline. Phase 5.7
replaces the single scheduler with one pinned scheduler/reactor per OS worker,
while preserving the structural Transfer boundary, non-migrating coroutine
stacks, event-driven idle waits, and the existing memory, timer, idle,
starvation, and native-loop controls. The runner also adds the mandatory
four-worker scaling workload: seven alternating one-task/four-task pairs,
fixed checksums, minimum signal duration, process-CPU corroboration, qualified
physical-core count, and MAD rejection.

The clean-tree after-stage command was:

```bash
cargo build --release --locked -p aura --target-dir target
python3 scripts/bench-scalable-runtime.py \
  --label phase57-after-pinned-worker-multicore \
  --aura target/release/aura \
  --repeats 3 \
  --timer-repeats 3 \
  --v6-repeats 5 \
  --multicore-repeats 7 \
  --idle-seconds 30 \
  --json /private/tmp/aurora-phase57-after-pinned-worker-multicore.json
```

The report records clean implementation commit
`6fb5efbb6b5c677eb5b9f3980a73ec88980d989c`, no dirty files, empty competing
process inventories, `contractual: true`, and no non-contractual reasons. The
host is the Mac14,9 Apple M2 Pro with 10 physical/logical cores and 16 GiB
RAM, running Darwin 25.5.0. The freshly qualified locked release `aura`
SHA-256 is
`9e81f90221d41899e017a3a6fbafd8dfaccdbb74a4884c4246aa448610aa0591`.
Raw report:
`/private/tmp/aurora-phase57-after-pinned-worker-multicore.json`; SHA-256:
`6d47c90d3dd9eb85421245c92aa3d12b01cb58ddf9ac0819b0e210c14123531d`.

| Workload | Repetitions | Contractual post-multicore result | Gate |
| --- | ---: | --- | --- |
| 10,000 sleepers | 3 | 206,503,936 bytes worst whole-process peak RSS; 197,885,952 bytes worst incremental peak RSS | PASS, whole-process peak at most 512 MiB |
| 100,000 sleepers plus 1,000 timers | 3 | 1,989,033,984 bytes worst whole-process peak RSS; 1,981,513,728 bytes worst incremental peak RSS; 5 ms worst arm span; 3 ms worst p99 | RSS FAIL against 1.5 GiB; timer gates PASS under the recorded escape hatch |
| 1,000 timers | 3 | 7 ms worst arm span; 3 ms worst p99 | PASS |
| 10 idle tasks | 3 | 0.0005296816042224426% worst CPU | PASS, less than 2% |
| 10 ms sleeper beside hot loop | 3 | 14 ms worst result | PASS, at most 50 ms |
| Four-worker CPU scaling | 7 paired repetitions | paired median ratio 1.077123x; ratio of medians 1.056700x; four-task median 0.593762 s versus one-task median 0.561902 s; 393.61% median four-task process CPU; all seven pairs pass | PASS, valid ratio at most 1.6 with at least 150% CPU |
| V6 int32 loop | 5 plus warmup | median 33.709750 ms; MAD 0.168875 ms; p95 46.408792 ms; best 33.540875 ms | recorded stage evidence |
| V6 int64 loop | 5 plus warmup | median 13.073333 ms; MAD 0.166834 ms; p95 15.675208 ms; best 10.962958 ms | recorded stage evidence |

The multicore sample is valid: the host reports 10 physical cores, both lanes
use `AURORA_WORKERS=4`, one-task relative MAD is 0.009574, four-task relative
MAD is 0.029027, and no paired sample exceeds the 1.6 ratio. Every workload
completed naturally with its exact protocol marker, zero status, empty
standard error, and no sampling error.

The runner exits nonzero solely because the already-accepted
massive-concurrency RSS gate remains unavailable. The mandatory multicore
gate and every maintained control pass, so the Phase 5.4 escape hatch permits
the stage to proceed without a massive-concurrency claim. This report is the
current Phase 5.7 benchmark evidence; the Phase 5.6 values above remain the
historical pre-multicore comparison.

## Phase 5.8 typed heterogeneous select

The accepted Phase 5.7 report above is the before-stage baseline. Phase 5.8
adds typed heterogeneous Queue/Task/deadline selection through one composite
reactor registration. The implementation does not change integer lowering,
loop safepoints, worker dispatch, or the CPU-scaling workload's executed path.
The complete suite was nevertheless repeated to pin memory, timer, idle,
starvation, multicore, and native-loop behavior.

The contractual run used a detached clean worktree at the implementation
commit so unrelated user files in the main worktree could not enter benchmark
provenance:

```bash
cargo build --release --locked -p aura --target-dir target
python3 scripts/bench-scalable-runtime.py \
  --label phase58-after-typed-select \
  --aura target/release/aura \
  --repeats 3 \
  --timer-repeats 3 \
  --v6-repeats 5 \
  --multicore-repeats 7 \
  --idle-seconds 30 \
  --json /private/tmp/aurora-phase58-after-typed-select.json
```

The report records clean implementation commit
`3e15b8a50010b51b8ffd832f5036d7aac8882299`, no dirty files, empty competing
process inventories, `contractual: true`, and no non-contractual reasons. The
host is the same Mac14,9 Apple M2 Pro with 10 physical/logical cores and
16 GiB RAM, running Darwin 25.5.0. The freshly qualified locked release
`aura` SHA-256 is
`c760cb374267c9475a45c348c44c0817e37473029b04e867abb2785d5c264ce7`.
Raw report: `/private/tmp/aurora-phase58-after-typed-select.json`; SHA-256:
`f72889aa83b8a222517808ef39df91d62a175109bc8806c3628602884a8c9ea2`.

| Workload | Repetitions | Contractual post-select result | Gate |
| --- | ---: | --- | --- |
| 10,000 sleepers | 3 | 206,585,856 bytes worst whole-process peak RSS; 198,475,776 bytes worst incremental peak RSS | PASS, whole-process peak at most 512 MiB |
| 100,000 sleepers plus 1,000 timers | 3 | 1,720,057,856 bytes worst whole-process peak RSS; 1,711,783,936 bytes worst incremental peak RSS; 4 ms worst arm span; 2 ms worst p99 | RSS FAIL against 1.5 GiB; timer gates PASS under the recorded escape hatch |
| 1,000 timers | 3 | 6 ms worst arm span; 1 ms worst p99 | PASS |
| 10 idle tasks | 3 | 0.0006353396996670736% worst CPU | PASS, less than 2% |
| 10 ms sleeper beside hot loop | 3 | 18 ms worst result | PASS, at most 50 ms |
| Four-worker CPU scaling | 7 paired repetitions | paired median ratio 1.020775x; ratio of medians 1.021596x; four-task median 0.796642 s versus one-task median 0.779801 s; 398.54% median four-task process CPU; all seven pairs pass | PASS, valid ratio at most 1.6 with at least 150% CPU |
| V6 int32 loop | 5 plus warmup | median 49.064750 ms; MAD 0.461292 ms; p95 50.737000 ms; best 48.522000 ms | recorded stage evidence |
| V6 int64 loop | 5 plus warmup | median 18.423417 ms; MAD 0.105500 ms; p95 18.823875 ms; best 18.317917 ms | recorded stage evidence |

Every workload completed naturally with its exact protocol marker, zero
status, empty standard error, and no sampling error. The mandatory multicore
sample is valid on 10 physical cores, has no failed pair, and is well inside
both the 1.6 ratio and 150% CPU requirements. The runner exits nonzero solely
because the accepted massive-concurrency RSS gate remains unavailable. That
workload improved by about 13.5% versus Phase 5.7 but still exceeds 1.5 GiB,
so the massive-concurrency marketing restriction remains unchanged.

The contractual absolute CPU times were 34-46% slower than the earlier
Phase 5.7 observation even though benchmark source hashes were identical and a
code audit found no new logic on either timed hot path. A same-session,
21-repetition control built both clean commits independently: Phase 5.7 versus
Phase 5.8 best times were both 48.4 ms for int32 and 14.9 versus 15.4 ms for
int64. The common contractual slowdown is therefore recorded as host
core-placement, QoS, or thermal variance rather than a Phase 5.8 regression.
This evidence establishes that typed selection did not materially regress the
maintained runtime gates; it does not make a new absolute performance claim.

## Phase 5.9 configurable blocking-I/O pool

The accepted Phase 5.8 report above is the before-stage baseline. Phase 5.9
adds exact process-wide blocking-worker configuration, an optional bound on
accepted pending jobs, FIFO scheduler-aware admission, and deterministic
timeout/cancellation behavior on both sides of the acceptance boundary. The
task scheduler, coroutine stack size, safepoint policy, and timed CPU workload
are unchanged. The full maintained benchmark was nevertheless repeated to
pin the complete runtime contract.

The contractual run used a detached clean worktree at the implementation
commit:

```bash
cargo build --release --locked -p aura --target-dir target
python3 scripts/bench-scalable-runtime.py \
  --label phase59-after-configurable-blocking-pool \
  --aura target/release/aura \
  --repeats 3 \
  --timer-repeats 3 \
  --v6-repeats 5 \
  --multicore-repeats 7 \
  --idle-seconds 30 \
  --json /private/tmp/aurora-phase59-after-configurable-blocking-pool.json
```

The report records clean implementation commit
`d92131399bacf63b7ae43ac12745aed183038883`, no dirty files, empty competing
process inventories, `contractual: true`, and no non-contractual reasons. The
host is the same Mac14,9 Apple M2 Pro with 10 physical/logical cores and
16 GiB RAM, running Darwin 25.5.0. The freshly qualified locked release
`aura` SHA-256 is
`b49246ee9cc4af82cee945d027605bb9ad47742f225d652456a37c898b608a0c`.
Raw report:
`/private/tmp/aurora-phase59-after-configurable-blocking-pool.json`; SHA-256:
`d9947ddc4c65c7ff7f592585d85530f92f10045b73fa66f25dfd5a1b2dabf21a`.

| Workload | Repetitions | Contractual post-pool result | Gate |
| --- | ---: | --- | --- |
| 10,000 sleepers | 3 | 206,962,688 bytes worst whole-process peak RSS; 198,492,160 bytes worst incremental peak RSS | PASS, whole-process peak at most 512 MiB |
| 100,000 sleepers plus 1,000 timers | 3 | 1,457,848,320 bytes worst whole-process peak RSS; 1,449,525,248 bytes worst incremental peak RSS; 4 ms worst arm span; 3 ms worst p99 | PASS, whole-process peak at most 1.5 GiB and stable timers |
| 1,000 timers | 3 | arm spans 6, 6, 7 ms; p99 1, 3, 1 ms | PASS |
| 10 idle tasks | 3 | 0.001074327945503786% worst CPU | PASS, less than 2% |
| 10 ms sleeper beside hot loop | 3 | 18 ms worst result | PASS, at most 50 ms |
| Four-worker CPU scaling | 7 paired repetitions | paired median ratio 1.020214x; ratio of medians 1.020275x; four-task median 0.796815 s versus one-task median 0.780980 s; 398.49% median four-task process CPU; all seven pairs pass | PASS, valid ratio at most 1.6 with at least 150% CPU |
| V6 int32 loop | 5 plus warmup | median 48.824791 ms; MAD 0.191166 ms; p95 50.831458 ms; best 48.633625 ms | recorded stage evidence |
| V6 int64 loop | 5 plus warmup | median 16.324583 ms; MAD 0.275041 ms; p95 19.039625 ms; best 16.049542 ms | recorded stage evidence |

Every workload completed naturally with its exact protocol marker, zero
status, empty standard error, and no sampling error. All contractual gates
pass. The mandatory multicore sample is valid on 10 physical cores and is
well inside both the 1.6 ratio and 150% CPU requirements.

This is also the first contractual Batch 4 observation in which the
100,000-sleeper plus 1,000-timer workload passes the 1.5 GiB RSS gate together
with stable timers. The earlier escape-hatch records remain valid historical
measurements, but the maintained Mac14,9 baseline can now make the bounded
100,000-sleeper claim. It is a measured baseline, not a portable guarantee for
every operating system, allocator, workload, or host.

## Phase 5.10 native structured diagnostic frames

The accepted Phase 5.9 report above remains the before-stage baseline. Phase
5.10 replaces the direct backend's string-normalized runtime errors with
structured native frame metadata and preserves task ancestry across nested
runtime scopes. This changes task-local runtime state, so the 100,000-sleeper
RSS gate is a required acceptance condition rather than an escape-hatch
observation.

### Rejected initial representation

The first clean contractual run measured implementation commit
`29ff7f606e3c0320c590947291a8f041db9e15cb`. Its native frame representation
eagerly owned frame strings and vectors in every task-local runtime-state
value. The qualified release Aura SHA-256 was
`014f29cc51db393f27968a5722a4a2060a2f6e67c4b2cdfc643c8c4c88dd9b24`.
Raw report:
`/private/tmp/aurora-phase510-after-native-frames.json`; SHA-256:
`012feee3e8a840c1c59c3c503572188b7bc4a8cc72b1ac3f30b5bb53f49f4528`.
The report records no dirty files, empty competing-process inventories,
`contractual: true`, and no non-contractual reasons.

| Workload | Repetitions | Rejected initial-frame result | Gate |
| --- | ---: | --- | --- |
| 10,000 sleepers | 3 | 221,282,304 bytes worst whole-process peak RSS; 212,680,704 bytes worst incremental peak RSS | PASS, whole-process peak at most 512 MiB |
| 100,000 sleepers plus 1,000 timers | 3 | 2,040,184,832 bytes worst whole-process peak RSS; 2,031,927,296 bytes worst incremental peak RSS; 4 ms worst arm span; 2 ms worst p99 | **RSS FAIL** against 1.5 GiB; embedded timer gates PASS |
| 1,000 timers | 3 | arm spans 4, 4, 14 ms; valid-run p99 up to 7 ms, with the 14 ms-arm-span run excluded from the contractual p99 | **FAIL**, overlap and p99 controls |
| 10 idle tasks | 3 | 0.0013306468811813544% worst CPU | PASS, less than 2% |
| 10 ms sleeper beside hot loop | 3 | 14 ms worst result | PASS, at most 50 ms |
| Four-worker CPU scaling | 7 paired repetitions | paired median ratio 1.059896x; ratio of medians 1.061839x; 397.74% median four-task process CPU; all seven pairs pass | PASS |
| V6 int32 loop | 5 plus warmup | median 34.675208 ms; MAD 1.216250 ms; p95 48.866375 ms; best 33.193333 ms | recorded rejected-run control |
| V6 int64 loop | 5 plus warmup | median 12.875583 ms; MAD 1.014416 ms; p95 16.032750 ms; best 11.861167 ms | recorded rejected-run control |

The run is retained as diagnostic evidence only. It cannot qualify Phase 5.10:
the massive-concurrency peak is about 410 MiB over the 1.5 GiB limit, and the
standalone timer controls also failed.

### Rejected compact representation

Commit `1e1263d` compacted inactive and active frame metadata rather than
retaining eagerly owned strings and vectors. Commit `e171420` added the
observable ABI regression for null required frame metadata and closed the
compiler line-coverage floor without a synthetic line-execution test.

The next clean contractual run measured
`e1714205d13bc9511c8d99f6d0f7c9782548298f`. Its qualified release Aura
SHA-256 was
`a7e644bd05ebf8744a15cd2e00885b7e2803f3899211cab67e626ffd51912935`.
Raw report:
`/private/tmp/aurora-phase510-after-native-frames-compact.json`; SHA-256:
`64868869cec520b438594214d8b62e0691cf921b8d062a1337fd0be82280ca60`.
The report records no dirty files, empty competing-process inventories,
`contractual: true`, and no non-contractual reasons.

| Workload | Repetitions | Rejected compact-frame result | Gate |
| --- | ---: | --- | --- |
| 10,000 sleepers | 3 | peaks 210,255,872, 210,714,624, and 211,189,760 bytes; 203,030,528 bytes worst incremental peak RSS | PASS, whole-process peak at most 512 MiB |
| 100,000 sleepers plus 1,000 timers | 3 | peaks 1,095,598,080, 1,629,388,800, and 1,831,649,280 bytes; 1,823,522,816 bytes worst incremental peak RSS; 3 ms worst arm span; 3 ms worst p99 | **RSS FAIL** against 1.5 GiB; embedded timer gates PASS |
| 1,000 timers | 3 | arm spans 5, 6, 5 ms; 1 ms p99 in every run | PASS |
| 10 idle tasks | 3 | 0.0009397762979994653% worst CPU | PASS, less than 2% |
| 10 ms sleeper beside hot loop | 3 | 19 ms worst result | PASS, at most 50 ms |
| Four-worker CPU scaling | 7 paired repetitions | paired median ratio 1.011633x; ratio of medians 1.011328x; 395.58% median four-task process CPU; all seven pairs pass | PASS |
| V6 int32 loop | 5 plus warmup | median 50.570916 ms; MAD 0.120293 ms; p95 51.054375 ms; best 49.949125 ms | recorded rejected-run control |
| V6 int64 loop | 5 plus warmup | median 18.645500 ms; MAD 0.878334 ms; p95 20.151375 ms; best 17.293750 ms | recorded rejected-run control |

Compaction restored the 10,000-sleeper result and every non-massive gate, but
the massive result varied from below the limit to about 211 MiB above it. The
stage therefore remained rejected; one favourable repetition was not treated
as acceptance.

### Boxed/prebuilt task-state correction and final contractual run

Commit `c3278c4` boxes each task's direct runtime state and prebuilds the
pristine state on the spawning task before the child coroutine begins. This
removes the large state value and its construction from the child coroutine's
scope while preserving allocation identity when the state is installed.
Commit `181204b` adds the observable nested-task ancestry restoration
regression, covering the normal completion path through the corrected scope
without adding a synthetic coverage-only test.

The final run measured clean commit
`181204b02ca419d3f8cad683e8a0015499a4363b`. The runner found no dirty files
and no competing Aurora `cargo`, `rustc`, or `aura` processes before either
the build or timing phase. It records `contractual: true`, an empty
`noncontractual_reasons` list, and a fresh successful locked release build.
The qualified `aura 0.1.0` binary is 12,910,176 bytes with SHA-256
`50503389792f7f86efb8f021f983a3917855bad82e4fbc90b99414695331142a`.

The measured host was Mac14,9 with an Apple M2 Pro, 10 physical and 10 logical
cores, 16 GiB of memory, and Darwin 25.5.0
(`Darwin Kernel Version 25.5.0: Tue Jun 9 22:28:24 PDT 2026;
root:xnu-12377.121.10~1/RELEASE_ARM64_T6020`). The exact command was:

```bash
cargo build --release --locked -p aura --target-dir target
npm run bench:scalable-runtime -- \
  --label phase510-after-native-frames-state-prebuilt \
  --aura target/release/aura \
  --repeats 3 \
  --timer-repeats 3 \
  --v6-repeats 5 \
  --multicore-repeats 7 \
  --idle-seconds 30 \
  --json /private/tmp/aurora-phase510-after-native-frames-state-prebuilt.json
```

Raw report:
`/private/tmp/aurora-phase510-after-native-frames-state-prebuilt.json`;
SHA-256:
`8ba448a06a8efb505af723ed00b8248fc1aa44ed270b46df5c15d74ecb9bd986`.
Run log:
`/private/tmp/aurora-phase510-after-native-frames-state-prebuilt.log`;
SHA-256:
`4c8d5cf7149b0b224847ae8c9e3ba49b5b26e80d8e5e2d20559457e91e3a683b`.
The concise extracted evidence is
`/private/tmp/aurora-phase510-final-summary.json`; SHA-256:
`edd1026137e2c800e7d63499c4104c38aa536673d87136732564e57530c6f304`.

| Workload | Repetitions | Final boxed/prebuilt result | Gate |
| --- | ---: | --- | --- |
| 10,000 sleepers | 3 | whole-process peaks 207,798,272, 206,946,304, and 206,831,616 bytes; incremental peaks 198,574,080, 198,688,768, and 198,787,072 bytes; 207,798,272 bytes worst whole-process peak | PASS, at most 512 MiB |
| 100,000 sleepers plus 1,000 timers | 3 | whole-process peaks 1,170,735,104, 1,921,531,904, and 2,001,305,600 bytes; incremental peaks 1,162,985,472, 1,913,192,448, and 1,993,097,216 bytes; ready RSS 1,009,532,928, 1,921,531,904, and 2,001,305,600 bytes; every run had 3 ms arm span and timer p50/p95/p99 of 1/2/2 ms; maxima 2, 3, and 3 ms | **RSS FAIL** against 1.5 GiB; embedded arm-span and p99 gates PASS |
| 1,000 timers | 3 | arm spans 6, 4, and 4 ms; every run had p50/p95/p99 of 0/1/1 ms; maxima 2, 1, and 1 ms; worst p99 1 ms | PASS, arm span at most 10 ms and p99 at most 5 ms |
| 10 idle tasks | 3 | CPU 0.001315454859327715%, 0.0016754605941909423%, and 0.0007211218436201194%; worst 0.0016754605941909423% | PASS, less than 2% CPU |
| 10 ms sleeper beside hot loop | 3 | 14, 13, and 13 ms; worst 14 ms | PASS, at most 50 ms |
| Four-worker CPU scaling | 7 paired repetitions | one-task/four-task seconds, ratio, and four-task CPU by pair: 0.558479125/0.581772959, 1.041709409x, 396.734801%; 0.558883334/0.579024375, 1.036038006x, 396.889826%; 0.564198959/0.582300833, 1.032084203x, 396.514917%; 0.557809875/0.586279833, 1.051038820x, 394.382397%; 0.563612250/0.579693541, 1.028532543x, 396.900108%; 0.556853833/0.582595542, 1.046227048x, 396.035886%; 0.559773083/0.581980708, 1.039672549x, 396.912018%; all seven pairs pass | PASS, valid ratio at most 1.6 with at least 150% CPU |
| V6 int32 loop | 5 plus warmup | warmup 593.378958 ms; samples 48.803417, 37.830541, 36.904000, 36.724625, and 37.436334 ms; median 37.436334 ms; MAD 0.532334 ms; p95 48.803417 ms; best 36.724625 ms | recorded stage evidence |
| V6 int64 loop | 5 plus warmup | warmup 559.237375 ms; samples 16.746292, 15.677542, 15.005584, 14.803625, and 14.965667 ms; median 15.005584 ms; MAD 0.201959 ms; p95 16.746292 ms; best 14.803625 ms | recorded stage evidence |

The multicore aggregates are: one-task best/median/MAD/p95
0.556853833/0.558883334/0.001073458/0.564198959 seconds; four-task
best/median/MAD/p95
0.579024375/0.581980708/0.000614834/0.586279833 seconds; paired-ratio
best/median/MAD/p95 1.028532543/1.039672549/0.006554499/1.051038820.
The ratio of medians is 1.041327720x, median four-task process CPU is
396.734801%, and the one-task and four-task relative MAD values are
0.001920721 and 0.001056451, both below the 0.15 bound.

Every workload completed naturally with its required protocol output, zero
status, and no sampling error. The 10,000-sleeper, standalone-timer, idle,
starvation, multicore, and embedded massive-timer gates are green. The sole
red result is the whole-process RSS of the massive-concurrency workload:
2,001,305,600 bytes observed versus the 1,610,612,736-byte ceiling.

The result also establishes that the 1.5 GiB requirement is below the
platform's stack-page scale for this workload. The 101,000 simultaneous tasks
(100,000 sleepers and 1,000 timer tasks) at one 16 KiB page each imply
1,654,784,000 bytes before runtime objects, scheduler state, allocator
overhead, or the rest of the process. That page-scale figure alone is
44,171,264 bytes above the 1,610,612,736-byte gate. The very low first
repetition and the earlier Phase 5.9 peak of 1,457,848,320 bytes are therefore
treated as macOS residency/compression outliers, not evidence that Aurora can
reliably hold the workload below the stated whole-process ceiling.

The documented Phase 5 benchmark escape hatch is invoked after the eager,
compact, and boxed/prebuilt representations all preserved behavior and all
non-RSS gates while the clean repeated measurement still could not satisfy
the physically mismatched RSS ceiling. Phase 5.10 may proceed on the boxed and
prebuilt implementation, but Aurora withdraws the Phase 5.9 bounded
100,000-sleeper claim. Massive concurrency remains unavailable as a supported
claim on this runtime. The accepted maintained claim is 10,000 suspended tasks
within 512 MiB, with stable timers, idle behavior, starvation latency, and
four-worker scaling as measured above.

## B5.0-d: V6 startup-versus-loop reconciliation

Batch 2 recorded whole-process medians of `32.734250 ms` for the ten-million
iteration `int32` loop and `10.248625 ms` for `int64`. The clean final
Phase-5.10 run above recorded `37.436334 ms` and `15.005584 ms`, regressions of
`14.36%` and `46.42%`. They therefore miss the attempted restoration target of
at most 10% above Batch 2.

The maintained runner now builds
`benchmarks/direct_integer_loops/startup.au`, a silent empty program using the
same direct runtime entry, and rotates it with the two V6 binaries on every
repetition. The JSON retains each whole-process observation and adds paired
startup and `whole process - startup` summaries at
`benchmarks.v6.startup_vs_loop`. Python unit tests pin silent startup output,
pair cardinality, and the split calculation.

This changes the report contract to schema version 4. Version 3 used `width`
for the two V6 run kinds; version 4 uses `workload` and adds the startup run
kind and split summary.

A 21-repetition run through the new `run_v6_benchmark` path on the Mac14,9
host measured:

| Component | `int32` | `int64` |
| --- | ---: | ---: |
| Whole-process median | 49.391916 ms | 18.875542 ms |
| Paired loop-estimate median | 41.746208 ms | 11.123916 ms |
| Startup median shared by the pair | 7.679583 ms | 7.679583 ms |

All 21 paired deltas were nonnegative. The cyclic orders were
startup/int32/int64, int32/int64/startup, and int64/startup/int32, repeated
seven times. A separate run of the same binaries with `AURORA_WORKERS=1`
measured a `7.851334 ms` startup median. This gives no evidence that selecting
one worker reduces the measured startup component. These runs were made while
Batch-5 work left the repository dirty and are diagnostic rather than
contractual; their purpose is to prove the split and distinguish a measured
fixed entry component from loop work, not to establish the complete cause of
the regression or replace the clean Phase-5.10 timing record. In particular,
the dirty run's `41.746208 ms` `int32` loop estimate does not reproduce the
Batch-2 whole-process median, so it cannot support a per-iteration or full
causal conclusion.

The diagnostic artifacts were built by release `aura` SHA-256
`758a49f5c5be3b12bb8666d190a7a1e0006ffbcd0f0ecb450314e29a831d6fe4`
against runtime archive
`ff4a3255c699ea38c9d35f48faef523500806219e64ec652af5ad2607892c9f1`.
The startup, int32, and int64 binary SHA-256 values were respectively
`940864dd612937949eb19bcd6faa94cf3a8b454d58ebbb24019e8515e4d45e4d`,
`8c8319053efa5bbdfaf7aeaf35f5f6784492a33cf17681ce9ae0acebb2058f35`,
and
`118bc0b3878ac05af06d05b3386671dd2672912efbf892e472c64b0a3c783cbb`.

The measurements establish a roughly `7.68 ms` fixed startup component, but
they do not identify the complete cause of the historical gap. The
`AURORA_WORKERS=1` result gives no evidence that lazy initialization of
additional workers would reduce that component. Bypassing the direct root
scheduler would remove the boundary responsible for task cleanup, traps,
cancellation, and cooperative scheduling, and the evidence does not identify
a safe initialization change.

No runtime initialization change is made. Under the alternate disposition
permitted by B5.0-d, the clean Phase-5.10 whole-process pair is explicitly
accepted as the reactor-era baseline: `37.436334 ms` for `int32` and
`15.005584 ms` for `int64`. That acceptance records the maintained baseline;
it is not a claim that startup explains the entire regression. Future V6
reports must retain both those whole-process values and the
startup-versus-loop split so a fixed process-entry change remains visible
alongside per-loop estimates.

## B5.0-f: small runtime follow-through

The CLI direct-build path now passes the existing native-runtime lock wait
callback through `aura build`, not only native `run`. A Unix integration test
holds an isolated real runtime lock and proves that human output flushes
`aura: waiting for a concurrent build...` before blocking. It deliberately
uses an unavailable Cargo executable after releasing the lock, so the test
pins the wait behavior without rebuilding the runtime or contending with other
CLI tests. A second real-lock regression pins JSON failure behavior: standard
error remains exactly one JSON document, and its diagnostic notes contain the
exact wait notice exactly once.

The runtime limits now record the measured MIR multicore contention:
four tasks take about `2.1x` the one-task wall time because interpreter work
and synchronization inflate per-task cost. MIR remains the checked
development path; direct native execution remains the performance path.
Secondary diagnostic labels generated at the two retained-access sites now say
`shared access ... begins here`, removing the last descriptive use of the
retired `shared borrow` spelling from that surface while preserving the
diagnostic code and primary message.

Least-loaded admission was considered and not adopted. Inbox depth does not
measure the cost of a task already running on a pinned worker or identify
whether that worker is on a performance or efficiency core. Replacing
round-robin admission with that signal would therefore add synchronization and
change scheduling without evidence that it improves the reported asymmetric
core tail. The assignment policy stays unchanged until a representative
benchmark and a better load signal can justify a change.

## B6.0-a: cold-boot V6 baseline

The baseline Mac was rebooted after the Batch 6 entry finding. The new kernel
boot time was `2026-07-30 23:02:25 +0100`; the contractual run began about
seven minutes after boot, after the initial login load had settled. It used a
clean detached checkout at
`18654158d22b2227149369e7911af04aafcbeecb`, recorded empty competing-process
inventories both before the build and before timing, and qualified a fresh
locked release build. The measured `aura` SHA-256 is
`e1b90738563582b938d84e0882eed2afe2bec098bdc4ae2d6a786e200246d90b`.

```bash
python3 scripts/bench-scalable-runtime.py \
  --label batch6-post-reboot-b60 \
  --aura /private/tmp/aurora-b6-post-reboot/target/release/aura \
  --repeats 3 \
  --timer-repeats 3 \
  --v6-repeats 5 \
  --multicore-repeats 7 \
  --idle-seconds 30 \
  --json /private/tmp/aurora-b60-post-reboot-schema4.json
```

The schema-4 report is contractual, with no non-contractual reasons. Its
SHA-256 is
`134efcc894742ed73b16e07f1e31845c83d19930d5894b4dc39f01533a9be2fd`.
The process-group guard left no benchmark, `aura`, Cargo, Rust compiler, or
synthetic-load process running after completion.

| Workload | Repetitions | Post-reboot result | Gate |
| --- | ---: | --- | --- |
| 10,000 sleepers | 3 | 216,023,040 bytes worst whole-process peak RSS; 207,863,808 bytes incremental | PASS, at most 512 MiB |
| 1,000 timers | 3 | 5 ms worst arm span; 2 ms worst p99 overshoot | PASS |
| 10 idle tasks | 3 | 0.000821842% worst CPU | PASS, less than 2% |
| 10 ms sleeper beside hot loop | 3 | 13 ms worst | PASS, at most 50 ms |
| Four-worker scaling | 7 paired repetitions | 1.042714x paired median; every pair passed; 395.81% median four-task CPU | PASS |
| 100,000 sleepers plus 1,000 timers | 3 | 2,073,526,272 bytes worst whole-process peak RSS; 4 ms arm span; 2 ms p99 | RSS claim remains withdrawn; timer checks pass |
| V6 `int32` whole process | 5 plus warmup | median 36.691666 ms; MAD 0.431376 ms; p95 44.627083 ms; best 35.836417 ms | maintained baseline evidence |
| V6 `int64` whole process | 5 plus warmup | median 14.837417 ms; MAD 0.443750 ms; p95 16.717333 ms; best 14.393667 ms | maintained baseline evidence |
| Empty direct startup | 5 plus warmup | median 6.574667 ms; MAD 0.295250 ms; p95 7.952958 ms; best 6.225500 ms | split evidence |
| Paired `int32` loop estimate | 5 | median 30.292500 ms; MAD 0.551125 ms | split evidence |
| Paired `int64` loop estimate | 5 | median 8.255709 ms; MAD 0.436709 ms | split evidence |

The cold-boot whole-process medians are `1.99%` and `1.12%` faster than the
accepted Phase-5.10 `37.436334 ms` / `15.005584 ms` pair, while they are
`25.71%` and `21.39%` faster than the later dirty
`49.391916 ms` / `18.875542 ms` diagnostic. The accepted reactor-era pair
therefore reproduces within ordinary host variation. The slower observation
was a dirty/load/thermal-state artifact, not evidence of a HEAD regression;
no runtime change is warranted. For continuity, the maintained rounded
reactor-era baseline remains `37.436334 ms` / `15.005584 ms`, with this
post-reboot replay as its release-provenance confirmation. All subsequent
Batch 6 release-performance measurements must use the rebooted host.
