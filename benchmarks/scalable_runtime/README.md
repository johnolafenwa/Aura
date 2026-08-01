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

Every measured binary is nevertheless launched as the leader of a fresh,
runner-owned POSIX process group. On success, protocol failure, timeout, or
interrupt, the runner checks that whole group, sends `SIGTERM` to any remaining
members, escalates to `SIGKILL`, verifies that the group disappeared, and
reaps the leader. A failed or silently ineffective cleanup invalidates the
benchmark. This also covers a descendant that survives after its original
benchmark leader exits.

`10k-sleepers` first emits and flushes a pre-spawn observation point:

```text
BASELINE sleepers 10000
```

The host records RSS at that point and starts its process monitor. The program
then starts 10,000 child tasks. Each child reports that it reached
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

The maintained report publishes two distinct measurements. The contractual
512 MiB gate uses whole-process peak RSS exactly. Incremental peak RSS is the
largest monitored RSS (or the synchronous `READY` observation, whichever is
larger) minus the pre-spawn `BASELINE` RSS; that secondary value removes the
executable and runtime's fixed cost when deriving a per-task memory estimate.

`100k-sleepers-1000-timers` is the massive-concurrency qualification workload.
It uses the same pre-spawn baseline:

```text
BASELINE massive 100000 1000 10
```

After 100,000 sleepers are parked, 1,000 independent 10 ms timers are armed.
The readiness record includes their monotonic start bounds:

```text
READY massive 100000 1000 10 <min_start_ms> <max_start_ms>
```

The parent retains primitive overshoot observations until all timers finish,
cancels and joins the sleepers, then emits exactly 1,000 indexed records and
the completion line:

```text
SAMPLE massive_timer <observation_index> <overshoot_ms>
DONE massive 100000 1000
```

The massive-concurrency gate is joint evidence: whole-process peak RSS must be
at most 1.5 GiB, every timer arm span must be valid, and the worst valid-run
p99 overshoot must be at most 5 ms. Incremental RSS remains in the report for
per-task analysis, but does not replace the absolute Batch 4 gate. A
memory-only pass does not qualify the claim.

`1000-timers` first parks every worker on a release queue. The parent releases
all workers, then waits until each has sent its monotonic start through the
primitive `armed` queue and entered its independent 10 ms sleep path. The
parent computes the minimum and maximum start observations and emits:

```text
READY timers 1000 10 <min_start_ms> <max_start_ms>
```

Each worker records its overshoot immediately after waking and sends only that
primitive integer to the parent. The task group then joins every worker before
the parent formats exactly 1,000 records:

```text
SAMPLE timer <observation_index> <overshoot_ms>
```

`observation_index` is the parent's unique sequence in `0..999`;
`overshoot_ms` is `max(0, end_ms - start_ms - 10)`. The `READY` bounds let the
runner report the exact worker-start span and verify that the timer intervals
overlapped. The raw overshoots provide the p99 sample set.

Keeping worker-side observations primitive is part of the measurement
contract. Aura tasks are cooperative: worker-side string interpolation after
one timer's timestamp would delay the next ready worker and incorrectly charge
formatting overhead to that timer's overshoot. Formatting only after the task
group has joined removes that observer effect. The final line is:

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

`sleeper-vs-hot-loop` arms one 10 ms sleep, then starts a sibling task whose
only work is a 200 ms loop over the monotonic clock. The loop contains no
explicit scheduler operation, so progress during it must come from
compiler-inserted loop-backedge safepoints. The sleeper records elapsed time
from immediately before it arms the sleep until it runs again:

```text
SAMPLE starvation 10 <elapsed_ms>
DONE starvation
```

The runner requires both lines exactly, rejects standard error, nonzero exit,
negative elapsed time, extra output, and timeout, and uses the worst elapsed
time across repetitions for the starvation gate. The runner forces
`AURA_WORKERS=1` for this workload so the measurement continues to prove
cooperative safepoint progress on one worker after multicore becomes the
default. Other non-multicore workloads explicitly remove any ambient
`AURA_WORKERS` override and therefore measure the production default.

`cpu-scaling` is built once and invoked with either `1` or `4` as its sole
program argument. Both timed shapes run with `AURA_WORKERS=4`; changing the
worker count between the legs invalidates the comparison. Each child first
reports to a prepared queue and then parks on a release queue. Only after every
child is parked does the parent emit and flush:

```text
READY multicore <tasks> 80000000 48271 2147483647
```

The host validates that line and starts the wall clock immediately before
writing `GO multicore`. Each released child applies the Park-Miller recurrence
`state = state * 48271 % 2147483647` for exactly 80,000,000 iterations,
starting from `task_index + 1`. The parent sums the final states and emits:

```text
DONE multicore <tasks> <checksum>
```

The host independently derives the checksum with modular exponentiation,
stops the wall interval at the complete `DONE` line, then writes
`ACK multicore`. The child must exit zero after the acknowledgment, with empty
standard error and no trailing standard output. Every timed process is sampled
for process CPU while its PID is alive. A protocol mismatch, checksum mismatch,
timeout, sampling failure, premature exit, output noise, or failure to reap
after `ACK` invalidates the run.

One excluded warmup of each shape precedes an odd number of paired
repetitions. The default is seven pairs and the minimum is five. Pair order
alternates `1,4`, then `4,1`, so drift is not assigned systematically to one
shape. The primary gate is the median of the raw paired `T4 / T1` ratios,
inclusive at 1.6. The report also preserves every duration and order, the
ratio of medians, median/MAD/p95/best summaries, and the indexes of
individually passing and failing pairs.

Multicore evidence is invalid, rather than failing or passing the performance
claim, when the host has fewer than four qualified cores, the one-task median
is below 250 ms, either shape has `MAD / median > 15%`, or the four-task
median process CPU is below 150% of wall time. Exactly four cores, a 250 ms
signal, 15% relative MAD, 150% CPU corroboration, and a 1.6 paired-median
ratio all satisfy their inclusive boundaries.

## Gate interpretation

Use a quiet machine and record the hardware and operating-system version with
the results. The runner checks for repository `cargo`, `rustc`, and `aura`
processes both before building workloads and again immediately before timing.
`--allow-competing-processes` preserves exploratory evidence but marks the
report non-contractual, records the reason, and cannot produce an
`all_gates_passed` result. Reports from a dirty checkout or hardware other
than the calibrated Mac14,9 baseline are also explicitly non-contractual.
The runner accepts only this checkout's `target/release/aura` and performs
`cargo build --release --locked -p aura --target-dir target` before compiling
the workloads. This makes the measured compiler/runtime input a fresh Cargo
product of the recorded checkout instead of an unqualified binary copied from
another revision.
The Batch 4 gates are:

- 10,000 sleepers at no more than 512 MiB whole-process peak RSS.
- 100,000 sleepers plus 1,000 timers at no more than 1.5 GiB whole-process
  peak RSS with valid overlap and p99 timer overshoot no more than 5 ms.
- p99 timer overshoot at no more than 5 ms under the 1,000-timer load.
- less than 2% process CPU during the idle workload's stable window.
- a 10 ms sleeper beside the hot loop completes within 50 ms.
- four synchronized CPU-bound tasks complete within 1.6 times the wall time
  of one task when both shapes use four workers.

Timer millisecond readings are intentionally the language's public monotonic
clock rather than a hidden host hook. Report the `READY` maximum-minus-minimum
arm span alongside p99; a run whose intervals did not substantially overlap
does not demonstrate the 1,000-timer gate. The p99 gate uses the worst p99
among valid-overlap repetitions; invalid-overlap repetitions are reported
explicitly and fail the separate arm-span gate. The combined sample summary is
informational only.

On Linux the runner samples `/proc`. On macOS it uses `proc_pid_rusage` from
`libproc` for resident bytes and nanosecond process CPU time. Background
monitors never fall back to spawning `ps`, so process sampling cannot inject a
high-frequency subprocess workload into timer measurements. The monitoring
cadence is recorded in the JSON report, and every monitor is joined before the
workload result is accepted. A monitor error or a run with no process samples
invalidates the benchmark instead of producing partial evidence.
