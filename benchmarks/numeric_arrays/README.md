# Numeric-array release evidence

This benchmark measures two exact one-million-element `float64` workloads:

- fresh owned elementwise addition of two contiguous arrays
- reduction of one existing contiguous array with `sum()`

It compares Aura's direct native backend with NumPy and plain Rust under the
same explicit single-thread environment and records release evidence.

The add lane allocates and releases a fresh result on every measured
operation. All implementations prepare their two inputs before the clock
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
  --raw-json /private/tmp/aura-phase73-arrays-post-reboot-raw.json \
  --summary-json /private/tmp/aura-phase73-arrays-post-reboot-summary.json
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

The six-lane order reverses every repetition. Each lane uses
`AURA_WORKERS=1` plus the common BLAS/OpenMP single-thread environment.
There are 512 add operations and 1,024 reductions per timed observation.
Reported values include raw samples, median, median absolute deviation, p95,
best, paired Aura/NumPy and Aura/Rust ratios, their medians, and ratios of medians.

No speed threshold is enforced against either reference implementation. A report is contractual only when
the checkout is clean and detached, the host is Mac14,9, every
protocol/checksum validates, the competing-process override is absent, and
all three host inventories are quiet. An inventory rejects an Aura-checkout
`cargo`, `rustc`, or `aura` process at any CPU level. It also rejects any other
process that remains at or above 50% CPU in two snapshots 0.25 seconds apart,
so a canonical CPU burner such as `yes` is recorded even outside the checkout.
The runner PID, its descendants, its direct parent, and the short-lived
`ps`/`lsof` inventory helpers are excluded from classification.

## 7 September 2026 foundations measurements

The after clean detached revision is `50531b45797ae765ec8d885165c13042617ab567`;
the before revision is `v0.3.3-preview` (`d3cc6b96104dd597687a98e9624f800a0cb3cf1e`).
Both ran on Mac14,9 / Apple M2 Pro / 16 GiB after boot at 02:29:21 BST,
7 September 2026, with Xcode CPython 3.9.6 / NumPy 2.0.2. Both reports are
contractual: 11 pairs, excluded warmups, three empty inventories and no override.
The after schema-2 run includes Rust 1.95.0; the unchanged before runner is schema 1.

| Workload (one million `float64` elements) | Aura median | NumPy median | Rust median | Aura / NumPy | Aura / Rust |
| --- | ---: | ---: | ---: | ---: | ---: |
| Fresh owned addition | 1.205427 ms | 0.247190 ms | 0.244576 ms | 4.876520 | 4.928642 |
| Existing-array sum | 1.148677 ms | 0.168981 ms | 0.858175 ms | 6.797687 | 1.338511 |

Ratios are ratios of medians. Before/after Aura changes are +7.1740% for addition
and -0.0994% for sum; NumPy drift is -0.0208% / -0.0394%. No repeat was required.
This comparison includes profile/link tuning as well as Cranelift `speed`.

[After raw](https://github.com/johnolafenwa/Aura/blob/main/work/2026-09-07-foundations-measurements/aura-foundations-after-arrays-raw.json):
`58128b5331777b8a868aae952bbc21b02145a54331f233d02efbb69b4becd22f`.
[Before raw](https://github.com/johnolafenwa/Aura/blob/main/work/2026-09-07-foundations-measurements/aura-foundations-before-arrays-raw.json):
`6f1f1c3b2d3fa288785204d54da2ec507a25a8c20e234584d72f8afe51c950fd`.
[SHA256SUMS](https://github.com/johnolafenwa/Aura/blob/main/work/2026-09-07-foundations-measurements/SHA256SUMS) also covers both summaries and the control calculation.
Manifest SHA-256: `911dc4a7901357b33679ad260923c56d0c8216440b200a0eb12e426ea60cac67`.

Reproduce after building the pinned release compiler, from the after checkout:

```bash
python3 scripts/bench-numeric-arrays.py --label foundations-after-rust --aura target/release/aura --python /Applications/Xcode.app/Contents/Developer/usr/bin/python3 --pairs 11 --raw-json /tmp/aura-foundations-after-arrays-raw.json --summary-json /tmp/aura-foundations-after-arrays-summary.json
```

Use the before checkout's own runner with label `foundations-before` and
before-specific JSON paths for the control. The Rust lane builds with
`--release --locked`, fat LTO, and one codegen unit. See the
[Rust baseline contract](../rust_baselines/README.md) for exact allocation,
arithmetic, scheduling, and protocol equivalence.
