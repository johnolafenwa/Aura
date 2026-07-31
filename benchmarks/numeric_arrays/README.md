# Numeric-array release evidence

This benchmark measures two exact one-million-element `float64` workloads:

- fresh owned elementwise addition of two contiguous arrays
- reduction of one existing contiguous array with `sum()`

It compares Aurora's direct native backend with NumPy under the same explicit
single-thread environment. The comparison is release evidence, not a CI
performance gate and not a general claim of NumPy performance or API
compatibility.

The add lane allocates and releases a fresh result on every measured
operation. Both implementations prepare their two inputs before the clock
starts. The sum lane reuses one prepared input. Every process performs one
unmeasured kernel warmup, emits `READY`, waits for the host's exact `GO` line,
then reports a checksum in `DONE`. The host owns and verifies the whole
process group.

Run the benchmark only from a clean detached checkout on the maintained,
post-reboot Mac14,9 baseline:

```bash
python3 scripts/bench-numeric-arrays.py \
  --label phase73-post-reboot \
  --aura target/release/aura \
  --python /Applications/Xcode.app/Contents/Developer/usr/bin/python3 \
  --pairs 11 \
  --raw-json /private/tmp/aurora-phase73-arrays-post-reboot-raw.json \
  --summary-json /private/tmp/aurora-phase73-arrays-post-reboot-summary.json
```

The raw schema records all warmups and paired observations, exact commands,
source and binary hashes, repository/host/boot identity, dependency identity,
quiet-process inventories before build, before timing, and after timing,
parameters, checksums, and derived statistics.
Input provenance includes SHA-256 identities for the benchmark runner, NumPy
reference, and `scripts/benchmark_process.py`, which owns process-group launch,
timeout, and cleanup behavior.
The smaller summary repeats the release-relevant provenance and links back to
the raw report by SHA-256.

The four-lane order reverses every repetition. Each lane uses
`AURORA_WORKERS=1` plus the common BLAS/OpenMP single-thread environment.
There are 512 add operations and 1,024 reductions per timed observation.
Reported values include raw samples, median, median absolute deviation, p95,
best, the paired Aurora/NumPy ratios, their median, and the ratio of medians.

No threshold compares Aurora with NumPy. A report is contractual only when
the checkout is clean and detached, the host is Mac14,9, every
protocol/checksum validates, the competing-process override is absent, and
all three host inventories are quiet. An inventory rejects an Aurora-checkout
`cargo`, `rustc`, or `aura` process at any CPU level. It also rejects any other
process that remains at or above 50% CPU in two snapshots 0.25 seconds apart,
so a canonical CPU burner such as `yes` is recorded even outside the checkout.
The runner PID, its descendants, its direct parent, and the short-lived
`ps`/`lsof` inventory helpers are excluded from classification.

Measured results belong here only after the controlled post-reboot run. State
them as:

> On the post-reboot Mac14,9 host, at commit `<sha>`, the direct native Aurora
> kernel measured a median `<x>` ms per one-million-element `float64` add and
> `<y>` ms per sum across 11 paired repetitions. NumPy `<version>` measured
> `<a>` and `<b>` ms under the recorded single-thread environment. These are
> measurements of these exact workloads on this host, not a portable
> performance guarantee, general NumPy comparison, or claim of NumPy API
> compatibility.
