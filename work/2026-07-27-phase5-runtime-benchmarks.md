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
No Phase 5 benchmark escape hatch is needed for the safepoint stage.
