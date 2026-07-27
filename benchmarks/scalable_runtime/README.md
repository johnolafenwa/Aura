# Scalable runtime workloads

These programs are the source workloads for the Phase 5 scheduler gates. Build
them as standalone direct-native binaries before taking measurements so
compiler and cache activity is outside the measured interval:

```bash
cargo build -p aura --release
mkdir -p target/scalable-runtime-benchmarks

./target/release/aura build --backend direct \
  -o target/scalable-runtime-benchmarks/10k-sleepers \
  benchmarks/scalable_runtime/10k_sleepers.au
./target/release/aura build --backend direct \
  -o target/scalable-runtime-benchmarks/1000-timers \
  benchmarks/scalable_runtime/1000_timers.au
./target/release/aura build --backend direct \
  -o target/scalable-runtime-benchmarks/idle-10-tasks \
  benchmarks/scalable_runtime/idle_10_tasks.au
```

Run the resulting binaries directly. Do not benchmark through `cargo run` or
`aura run`. The maintained host runner defaults to the workloads' advertised
30-second stable window; shorter windows are useful only for harness smoke
checks and are recorded in the resulting JSON.

## Output protocol

Standard output is machine-readable, line-oriented ASCII. A `READY` line is
flushed before the stable measurement interval begins. Any nonzero exit,
missing line, duplicate timer index, or unexpected standard-output line makes
the sample invalid. The host runner waits for natural process completion and
requires the exact `DONE` line, zero exit status, and empty standard error;
nominal sleepers and idle runs are never terminated by the runner.

`10k-sleepers` starts 10,000 child tasks. Each child reports that it reached
its `sleep(1m)` path, and cooperative scheduling carries it through to that
sleep boundary before another task runs. Only after all 10,000 reports have
been received does the parent emit:

```text
READY sleepers 10000
```

The process then holds that parked population for 30 seconds, cancels the
group, waits for structured cleanup, and emits:

```text
DONE sleepers 10000
```

Measure peak RSS after `READY` and before `DONE`.

`1000-timers` first parks every worker on a release queue. The parent releases
all workers, then waits until each has recorded its own monotonic start and
entered its independent 10 ms sleep path. It emits:

```text
READY timers 1000 10
```

This is followed by exactly 1,000 records in unspecified completion order:

```text
SAMPLE timer <index> <start_ms> <overshoot_ms>
```

`index` is unique in `0..999`. `start_ms` is the worker's
`sys.monotonic_time_ms()` value immediately before the sleep.
`overshoot_ms` is `max(0, end_ms - start_ms - 10)`. The start values allow the
runner to report the arm span and verify that the timer intervals overlapped;
the overshoot values provide the p99 sample set. The final line is:

```text
DONE timers 1000
```

`idle-10-tasks` parks five tasks on an empty queue and five tasks in
`sleep(1m)`, then emits:

```text
READY idle 10 30000
```

The final field is the 30,000 ms stable measurement window. Measure process CPU
only inside that window. A short unmeasured guard follows the window so the
host's final reading cannot race task cancellation. The parent then cancels
and joins every child before emitting:

```text
DONE idle 10
```

## Gate interpretation

Use a quiet machine and record the hardware and operating-system version with
the results. The runner checks for repository `cargo`, `rustc`, and `aura`
processes both before building workloads and again immediately before timing.
`--allow-competing-processes` preserves exploratory evidence but marks the
report non-contractual, records the reason, and cannot produce an
`all_gates_passed` result. The Batch 4 gates are:

- 10,000 sleepers at no more than 512 MiB peak RSS.
- p99 timer overshoot at no more than 5 ms under the 1,000-timer load.
- less than 2% process CPU during the idle workload's stable window.

Timer millisecond readings are intentionally the language's public monotonic
clock rather than a hidden host hook. Report the timer arm span alongside p99;
a run whose intervals did not substantially overlap does not demonstrate the
1,000-timer gate. The p99 gate uses the worst p99 among valid-overlap
repetitions; invalid-overlap repetitions are reported explicitly and fail the
separate arm-span gate. The combined sample summary is informational only.

On Linux the runner samples `/proc`. On macOS it uses `proc_pid_rusage` from
`libproc` for resident bytes and nanosecond process CPU time. Background
monitors never fall back to spawning `ps`, so process sampling cannot inject a
high-frequency subprocess workload into timer measurements. The monitoring
cadence is recorded in the JSON report, and every monitor is joined before the
workload result is accepted.
