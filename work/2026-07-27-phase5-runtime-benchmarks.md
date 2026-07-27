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

Pending the first quiet-machine run of:

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

No Phase 5.1 runtime implementation change may precede this baseline.
