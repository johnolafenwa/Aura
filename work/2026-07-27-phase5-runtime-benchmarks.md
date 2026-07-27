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
