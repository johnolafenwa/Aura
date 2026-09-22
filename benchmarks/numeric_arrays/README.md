# Numeric-array release evidence

This benchmark measures two exact `float64` workloads over one million
elements:

- fresh owned elementwise addition of two contiguous arrays
- reduction of one existing contiguous array with `sum()`

It compares Aura's direct native backend with NumPy and plain Rust under the
same explicit single-thread environment, and it records release evidence.

The add lane allocates and releases a fresh result on every measured
operation. The sum lane reuses one prepared input. Every implementation
prepares its two inputs before the clock starts. Each process then:

1. performs one unmeasured kernel warmup
2. emits `READY` and waits for the host's exact `GO` line
3. reports a checksum in `DONE`

The host owns and verifies the whole process group.

Run the benchmark from a clean detached checkout on the Mac14,9 baseline.
Release-session measurements use a host after a fresh reboot. The explicitly
authorized item 7 session below uses a quiet host and records that
distinction.

```bash
python3 scripts/bench-numeric-arrays.py \
  --label phase73-post-reboot \
  --aura target/release/aura \
  --python /Applications/Xcode.app/Contents/Developer/usr/bin/python3 \
  --pairs 11 \
  --raw-json /private/tmp/aura-phase73-arrays-post-reboot-raw.json \
  --summary-json /private/tmp/aura-phase73-arrays-post-reboot-summary.json
```

The raw report records:

- all warmups and paired observations
- exact commands
- source and binary hashes
- repository, host, and boot identity
- dependency identity
- quiet-process inventories before build, before timing, and after timing
- parameters, checksums, and derived statistics

Input provenance includes SHA-256 identities for the benchmark runner, the
NumPy reference, and `scripts/benchmark_process.py`. That script owns
process-group launch, timeout, and cleanup. The smaller summary repeats the
release-relevant provenance and links back to the raw report by SHA-256.

The six-lane order reverses every repetition. Each lane uses `AURA_WORKERS=1`
plus the common BLAS/OpenMP single-thread environment. Each timed observation
covers 512 add operations and 1,024 reductions. Reported values include raw
samples, median, median absolute deviation, p95, best, paired Aura/NumPy and
Aura/Rust ratios, their medians, and ratios of medians.

No speed threshold is enforced against either reference implementation. A
report is contractual only when all of these hold:

- the checkout is clean and detached
- the host is Mac14,9
- every protocol and checksum check validates
- the competing-process override is absent
- all three host inventories are quiet

An inventory rejects an Aura-checkout `cargo`, `rustc`, or `aura` process at
any CPU level. It also rejects any other process that stays at or above 50% CPU
in two snapshots taken 0.25 seconds apart. So a canonical CPU burner such as
`yes` is recorded even outside the checkout. Classification excludes the runner
PID, its descendants, its direct parent, and the short-lived `ps`/`lsof`
inventory helpers.

## 7 September 2026 foundations measurements

- **After:** clean detached revision `50531b45797ae765ec8d885165c13042617ab567`.
- **Before:** `v0.3.3-preview` (`d3cc6b96104dd597687a98e9624f800a0cb3cf1e`).

Both ran on Mac14,9 / Apple M2 Pro / 16 GiB after boot at 02:29:21 BST,
7 September 2026, with Xcode CPython 3.9.6 / NumPy 2.0.2. Both reports are
contractual: 11 pairs, excluded warmups, three empty inventories, and no
override. The after run uses schema 2 and includes Rust 1.95.0. The unchanged
before runner uses schema 1.

| Workload (one million `float64` elements) | Aura median | NumPy median | Rust median | Aura / NumPy | Aura / Rust |
| --- | ---: | ---: | ---: | ---: | ---: |
| Fresh owned addition | 1.205427 ms | 0.247190 ms | 0.244576 ms | 4.876520 | 4.928642 |
| Existing-array sum | 1.148677 ms | 0.168981 ms | 0.858175 ms | 6.797687 | 1.338511 |

Ratios are ratios of medians. From before to after, Aura time changed by
+7.1740% for addition and -0.0994% for sum. NumPy drift is -0.0208% for
addition and -0.0394% for sum. No repeat was required. This comparison
includes profile and link tuning as well as Cranelift `speed`.

- [After raw](https://github.com/johnolafenwa/Aura/blob/main/work/2026-09-07-foundations-measurements/aura-foundations-after-arrays-raw.json):
  `58128b5331777b8a868aae952bbc21b02145a54331f233d02efbb69b4becd22f`
- [Before raw](https://github.com/johnolafenwa/Aura/blob/main/work/2026-09-07-foundations-measurements/aura-foundations-before-arrays-raw.json):
  `6f1f1c3b2d3fa288785204d54da2ec507a25a8c20e234584d72f8afe51c950fd`
- [SHA256SUMS](https://github.com/johnolafenwa/Aura/blob/main/work/2026-09-07-foundations-measurements/SHA256SUMS)
  also covers both summaries and the control calculation. Manifest SHA-256:
  `911dc4a7901357b33679ad260923c56d0c8216440b200a0eb12e426ea60cac67`

To reproduce, build the pinned release compiler and run this from the after
checkout:

```bash
python3 scripts/bench-numeric-arrays.py --label foundations-after-rust --aura target/release/aura --python /Applications/Xcode.app/Contents/Developer/usr/bin/python3 --pairs 11 --raw-json /tmp/aura-foundations-after-arrays-raw.json --summary-json /tmp/aura-foundations-after-arrays-summary.json
```

For the control, use the before checkout's own runner with the label
`foundations-before` and before-specific JSON paths. The Rust lane builds with
`--release --locked`, fat link-time optimization (LTO), and one codegen unit.
See the [Rust baseline contract](../rust_baselines/README.md) for exact
allocation, arithmetic, scheduling, and protocol equivalence.

## Pre-Batch-1 item 7 kernel investigation

The shared `runtime_value.rs` slice kernels for float32 and float64 hoist
operation selection and fill a preallocated result through exact-length
iterators. The release arm64 instructions include `fadd.4s`, `fadd.2d`,
`fsub.4s`, `fsub.2d`, `fmul.4s`, `fmul.2d`, `fdiv.4s`, and `fdiv.2d`. These
vector instructions replace the earlier scalar implementation.

- Scalar division uses four outlined `Vec::extend_trusted` specializations.
- The other scalar broadcasts are visible in `ArrayValue::scalar_binary`.
- Scalar tails keep the same operations.

The earlier default profile selects separate scalar add, sub, and mul loops.
In the tuned profile, the old kernel instead executes all three scalar
operations and two `fcsel` instructions per element. Both profiles keep an
out-of-line `array_float64_binary` call. So this evidence identifies loop
selection, not a lost kernel-inlining boundary, and no inlining attribute is
warranted. The new explicit outer match removes that optimizer dependency.
These controls differ in both LTO and codegen units, so they do not isolate LTO
as the sole historical cause.

An eleven-observation diagnostic control reproduced a 7.43% regression:
1.128668 ms default versus 1.212512 ms tuned. The vectorized kernel measured
0.246133 ms, a 79.70% reduction from tuned. These controls preserve symbols and
ran on a quiet host, so they are diagnostic. The clean-detached NumPy/Rust
publication is separate.

Before the kernel edits, two paths recorded identical results for 1,008
fixed-seed cases: MIR operator and method execution, and the direct runtime
ABI. MIR is Aura's mid-level intermediate representation, and the ABI is the
application binary interface. The frozen hashes include every element's exact
bits and shape, or the reduction bits, or the diagnostic code and message.
Debug and optimized release checks pass after the rewrite. The cases cover:

- all four element types
- lengths 0/1/7/8/9/1,000,001
- integer modes
- scalar broadcasts
- payload-bearing NaNs, both infinities, and signed zero
- first traps at indices 0/7/8

Reductions keep their sequential deterministic order. Fallible allocation keeps
precedence over divisor validation.

The [investigation evidence](https://github.com/johnolafenwa/Aura/tree/main/work/2026-09-08-pre-batch-1-items-6-7)
contains the profile commands, binary hashes, full compressed disassemblies,
selected kernel instructions, raw diagnostic timings, and the identity of the
corpus before the change. No existing fixture was changed.

## Item 7 clean-detached publication

The item 7 measurements carry this label: "quiet host, not post-reboot;
contractual re-measure scheduled with the 0.3.4 release session."

Both runs are contractual under the runner's qualification rules:

- clean detached sources
- 11 rotating pairs and excluded warmups
- exact protocol and checksum validation
- three empty host inventories
- successful input and hash rechecks
- no override

The host is Mac14,9 / Apple M2 Pro / 16 GiB, with Xcode CPython 3.9.6, NumPy
2.0.2, and Rust 1.95.0. Measurements are single-threaded and per operation.
Ratios are ratios of medians.

- **Before:** merge base `a368dce7e7b335c1c0cb800ba5240501a21f02bc`.
- **After:** implementation head `d9fc79921ba9fed1116634ec9994bcb599aa9f90`.

Each clean checkout used its own unchanged runner in the same measurement
session, after the full local gates. Later publication edits do not change
compiler or runtime sources.

| Workload (one million `float64` elements) | Aura before | Aura after | Change in time | Before Aura / Rust | After Aura / Rust |
| --- | ---: | ---: | ---: | ---: | ---: |
| Fresh owned addition | 1.244818 ms | 0.249473 ms | -79.9591% | 5.039359 | 1.006524 |
| Existing-array sum | 1.149271 ms | 1.149844 ms | +0.0498% | 1.338990 | 1.339087 |

| Control | Before median | After median | Drift |
| --- | ---: | ---: | ---: |
| NumPy add | 0.253398 ms | 0.247745 ms | -2.2310% |
| Rust add | 0.247019 ms | 0.247856 ms | +0.3387% |
| NumPy sum | 0.173397 ms | 0.172073 ms | -0.7639% |
| Rust sum | 0.858312 ms | 0.858677 ms | +0.0425% |

Addition meets the task's 1.5x Rust target at 1.006524x, with 79.9591% less
Aura time. Its after Aura/NumPy ratio is 1.006974. The sum ratio against NumPy
is 6.682308. That gap comes chiefly from the deterministic reduction-order
policy. Under the same left-to-right order, Aura stays within 1.34x of Rust.

Addition allocates a fresh owned result, and sum reuses its input. These exact
workloads do not establish a general performance ranking for the language or
the Array API.

[Before raw](https://github.com/johnolafenwa/Aura/blob/main/work/2026-09-08-pre-batch-1-items-6-7/arrays-before-raw.json),
[after raw](https://github.com/johnolafenwa/Aura/blob/main/work/2026-09-08-pre-batch-1-items-6-7/arrays-after-raw.json),
[comparison and controls](https://github.com/johnolafenwa/Aura/blob/main/work/2026-09-08-pre-batch-1-items-6-7/array-publication-comparison.json),
and [SHA256SUMS](https://github.com/johnolafenwa/Aura/blob/main/work/2026-09-08-pre-batch-1-items-6-7/SHA256SUMS)
keep all observations and source and binary identities.

To reproduce, build each pinned release compiler, then run each checkout's own
runner:

```bash
python3 scripts/bench-numeric-arrays.py --label items-6-7-after --aura target/release/aura --python /Applications/Xcode.app/Contents/Developer/usr/bin/python3 --pairs 11 --raw-json /tmp/arrays-after-raw.json --summary-json /tmp/arrays-after-summary.json
```

In the before checkout, use `items-6-7-before` and distinct output paths.
Finish both compiler builds before either measurement. Keep all other build
and test work stopped until both runs finish. Never enable the
competing-process override.
