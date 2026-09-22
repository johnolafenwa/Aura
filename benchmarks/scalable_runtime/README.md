# Scalable runtime workloads

These programs are the source workloads for the scheduler gates listed under
[Gate interpretation](#gate-interpretation). Build them as standalone
direct-native binaries before you measure, so compiler and cache activity
stays outside the measured interval:

```bash
cargo build -p aura --release
mkdir -p target/scalable-runtime-benchmarks

./target/release/aura build --backend direct \
  -o target/scalable-runtime-benchmarks/10k-sleepers \
  benchmarks/scalable_runtime/10k_sleepers.au
./target/release/aura build --backend direct \
  -o target/scalable-runtime-benchmarks/100k-sleepers-1000-timers \
  benchmarks/scalable_runtime/100k_sleepers_1000_timers.au
./target/release/aura build --backend direct \
  -o target/scalable-runtime-benchmarks/1000-timers \
  benchmarks/scalable_runtime/1000_timers.au
./target/release/aura build --backend direct \
  -o target/scalable-runtime-benchmarks/idle-10-tasks \
  benchmarks/scalable_runtime/idle_10_tasks.au
./target/release/aura build --backend direct \
  -o target/scalable-runtime-benchmarks/sleeper-vs-hot-loop \
  benchmarks/scalable_runtime/sleeper_vs_hot_loop.au
./target/release/aura build --backend direct \
  -o target/scalable-runtime-benchmarks/cpu-scaling \
  benchmarks/scalable_runtime/cpu_scaling.au
```

Run the resulting binaries directly. Do not benchmark through `cargo run` or
`aura run`. The host runner defaults to the workloads' advertised 30-second
stable window. Shorter windows are useful only for harness smoke checks, and
the resulting JSON records them.

## Output protocol

Standard output is machine-readable, line-oriented ASCII. Each workload
flushes a `READY` line before the stable measurement interval begins. Any
nonzero exit, missing line, duplicate timer index, or unexpected
standard-output line makes the sample invalid.

The host runner waits for the process to finish on its own. It requires the
exact `DONE` line, a zero exit status, and empty standard error. The runner
never terminates nominal sleepers or idle runs.

Even so, every measured binary starts as the leader of a fresh POSIX process
group that the runner owns. On success, protocol failure, timeout, or
interrupt, the runner:

1. checks the whole group
2. sends `SIGTERM` to any remaining members
3. escalates to `SIGKILL`
4. verifies that the group disappeared
5. reaps the leader

A failed or silently ineffective cleanup invalidates the benchmark. This also
covers a descendant that survives after its original benchmark leader exits.

### `10k-sleepers`

The program first emits and flushes a pre-spawn observation point:

```text
BASELINE sleepers 10000
```

The host records resident set size (RSS) at that point and starts its process
monitor. The program then starts 10,000 child tasks. Each child reports that
it reached its `sleep(1m)` path. Cooperative scheduling carries each child to
that sleep boundary before another task runs. Only after the parent receives
all 10,000 reports does it emit:

```text
READY sleepers 10000
```

The process then holds that parked population for 30 seconds, cancels the
group, waits for structured cleanup, and emits:

```text
DONE sleepers 10000
```

The report publishes two distinct measurements:

- **Whole-process peak RSS** is what the contractual 512 MiB gate uses.
- **Incremental peak RSS** is the larger of the largest monitored RSS and the
  synchronous `READY` observation, minus the pre-spawn `BASELINE` RSS. This
  secondary value removes the fixed cost of the executable and runtime when
  deriving a per-task memory estimate.

### `100k-sleepers-1000-timers`

This is the massive-concurrency qualification workload. It uses the same
pre-spawn baseline:

```text
BASELINE massive 100000 1000 10
```

After 100,000 sleepers are parked, the program arms 1,000 independent 10 ms
timers. The readiness record includes their monotonic start bounds:

```text
READY massive 100000 1000 10 <min_start_ms> <max_start_ms>
```

The parent keeps primitive overshoot observations until all timers finish. It
then cancels and joins the sleepers, emits exactly 1,000 indexed records, and
emits the completion line:

```text
SAMPLE massive_timer <observation_index> <overshoot_ms>
DONE massive 100000 1000
```

The massive-concurrency gate is joint evidence. All three must hold:

- whole-process peak RSS is at most 1.5 GiB
- every timer arm span is valid
- the worst valid-run p99 overshoot is at most 5 ms

The report keeps incremental RSS for per-task analysis, but it does not
replace the absolute Batch 4 gate. A memory-only pass fails the joint gate.

### `1000-timers`

The program first parks every worker on a release queue. The parent releases
all workers. It then waits until each worker has sent its monotonic start
through the primitive `armed` queue and entered its independent 10 ms sleep
path. The parent computes the minimum and maximum start observations and
emits:

```text
READY timers 1000 10 <min_start_ms> <max_start_ms>
```

Each worker records its overshoot right after it wakes and sends only that
primitive integer to the parent. The task group then joins every worker before
the parent formats exactly 1,000 records:

```text
SAMPLE timer <observation_index> <overshoot_ms>
```

`observation_index` is the parent's unique sequence in `0..999`.
`overshoot_ms` is `max(0, end_ms - start_ms - 10)`. The `READY` bounds let the
runner report the exact worker-start span and verify that the timer intervals
overlapped. The raw overshoots form the p99 sample set.

Keeping worker-side observations primitive is part of the measurement
contract. Aura tasks are cooperative. Worker-side string interpolation after
one timer's timestamp would delay the next ready worker, and the formatting
overhead would be wrongly charged to that timer's overshoot. Formatting only
after the task group has joined removes that observer effect. The final line
is:

```text
DONE timers 1000
```

### `idle-10-tasks`

The program parks five tasks on an empty queue and five tasks in `sleep(1m)`,
then emits:

```text
READY idle 10 30000
```

The final field is the 30,000 ms stable measurement window. Measure process
CPU only inside that window. A short unmeasured guard follows the window so
the host's final reading cannot race task cancellation. The parent then
cancels and joins every child before emitting:

```text
DONE idle 10
```

### `sleeper-vs-hot-loop`

The program arms one 10 ms sleep, then starts a sibling task whose only work
is a 200 ms loop over the monotonic clock. The loop has no explicit scheduler
operation. Any progress during it must come from loop-backedge safepoints
that the compiler inserts. The sleeper records the elapsed time from just
before it arms the sleep until it runs again:

```text
SAMPLE starvation 10 <elapsed_ms>
DONE starvation
```

The runner requires both lines exactly. It rejects standard error, nonzero
exit, negative elapsed time, extra output, and timeout. The starvation gate
uses the worst elapsed time across repetitions.

The runner forces `AURA_WORKERS=1` for this workload. That way the measurement
still proves cooperative safepoint progress on one worker, even though
multicore is the default. Other non-multicore workloads explicitly remove any
ambient `AURA_WORKERS` override, so they measure the production default.

### `cpu-scaling`

The program is built once and invoked with either `1` or `4` as its sole
program argument. Both timed shapes run with `AURA_WORKERS=4`. Changing the
worker count between the two legs invalidates the comparison.

Each child first reports to a prepared queue and then parks on a release
queue. Only after every child is parked does the parent emit and flush:

```text
READY multicore <tasks> 80000000 48271 2147483647
```

The host validates that line and starts the wall clock just before it writes
`GO multicore`. Each released child applies the Park-Miller recurrence
`state = state * 48271 % 2147483647` for exactly 80,000,000 iterations,
starting from `task_index + 1`. The parent sums the final states and emits:

```text
DONE multicore <tasks> <checksum>
```

The host derives the checksum independently with modular exponentiation. It
stops the wall interval at the complete `DONE` line, then writes
`ACK multicore`. After the acknowledgment, the child must exit zero with empty
standard error and no trailing standard output. The host samples every timed
process for process CPU while its PID is alive.

Any of these invalidates the run: a protocol mismatch, checksum mismatch,
timeout, sampling failure, premature exit, output noise, or failure to reap
after `ACK`.

One excluded warmup of each shape comes before an odd number of paired
repetitions. The default is seven pairs and the minimum is five. Pair order
alternates `1,4`, then `4,1`, so drift is not assigned systematically to one
shape.

The primary gate is the median of the raw paired `T4 / T1` ratios, inclusive
at 1.6. The report also keeps every duration and order, the ratio of medians,
median/MAD/p95/best summaries, and the indexes of the pairs that pass and fail
individually. MAD is the median absolute deviation.

Multicore evidence is invalid before gate evaluation when any of these holds:

- the host has fewer than four qualified cores
- the one-task median is below 250 ms
- either shape has `MAD / median > 15%`
- the four-task median process CPU is below 150% of wall time

All of these boundaries are inclusive: exactly four cores, a 250 ms signal,
15% relative MAD, 150% CPU corroboration, and a 1.6 paired-median ratio all
satisfy their bounds.

## Gate interpretation

Use a quiet machine, and record the hardware and operating-system version with
the results. The runner checks for repository `cargo`, `rustc`, and `aura`
processes before it builds the workloads, and again just before timing.

A contractual report is one that counts as gate evidence. These reports are
non-contractual:

- A run with `--allow-competing-processes`. The flag keeps exploratory
  evidence, but it marks the report non-contractual, records the reason, and
  cannot produce an `all_gates_passed` result.
- A report from a dirty checkout, or from hardware other than the calibrated
  Mac14,9 baseline.

The runner accepts only this checkout's `target/release/aura`. It runs
`cargo build --release --locked -p aura --target-dir target` before compiling
the workloads. The measured compiler and runtime are therefore a fresh Cargo
product of the recorded checkout, not an unqualified binary copied from
another revision.

The Batch 4 gates are:

- 10,000 sleepers at no more than 512 MiB whole-process peak RSS.
- 100,000 sleepers plus 1,000 timers at no more than 1.5 GiB whole-process
  peak RSS, with valid overlap and p99 timer overshoot no more than 5 ms.
- p99 timer overshoot at no more than 5 ms under the 1,000-timer load.
- less than 2% process CPU during the idle workload's stable window.
- a 10 ms sleeper beside the hot loop completes within 50 ms.
- four synchronized CPU-bound tasks complete within 1.6 times the wall time
  of one task when both shapes use four workers.

Timer millisecond readings use the language's public monotonic clock on
purpose, not a hidden host hook. Report the `READY` maximum-minus-minimum arm
span alongside p99. A run whose intervals did not substantially overlap does
not demonstrate the 1,000-timer gate.

The p99 gate uses the worst p99 among valid-overlap repetitions. The report
lists invalid-overlap repetitions explicitly, and they fail the separate
arm-span gate. The combined sample summary is informational only.

On Linux the runner samples `/proc`. On macOS it uses `proc_pid_rusage` from
`libproc` for resident bytes and nanosecond process CPU time. Background
monitors never fall back to spawning `ps`, so process sampling cannot inject a
high-frequency subprocess workload into timer measurements.

The JSON report records the monitoring cadence. Every monitor is joined before
the workload result is accepted. A monitor error, or a run with no process
samples, invalidates the benchmark instead of producing partial evidence.
