# Performance

Aura records exact workloads, pinned source commits, raw observations, and
content hashes. These measurements guide engineering; they are not language
semantic guarantees or ratified numeric targets.

## Current Measurements

The earlier 7 September 2026 post-reboot session covers foundations items 1–4
and pinned Rust baselines. Item 7 adds a later quiet-host Array comparison,
labeled separately below. The release-performance and Array reports are
**contractual** under their runner rules. The standalone integer/Rust and
per-call profiling comparisons are **diagnostic-grade**, with their limits
disclosed below.

The host is Mac14,9 / Apple M2 Pro (10 cores, 16 GiB), arm64 macOS 26.5.2
(build 25F84). Boot identity is `1788744561`, 7 September 2026 at 02:29:21 BST.
CPython is Xcode 3.9.6 at
`/Applications/Xcode.app/Contents/Developer/usr/bin/python3`, with NumPy 2.0.2.
Rust is 1.95.0 (`59807616e`, LLVM 22.1.2); concurrent references pin tokio 1.53.1.

The compiler bases are after merge `50531b45797ae765ec8d885165c13042617ab567`
and before tag `v0.3.3-preview`, `d3cc6b96104dd597687a98e9624f800a0cb3cf1e`.
The original task/TCP inputs pass int64 range values to int32 task parameters
and fail checking at both bases. The release measurements use clean detached
commits with the same two explicitly authorized `as int32` corrections:

| Release measurement | Corrected source commit |
| --- | --- |
| After | `4e1e48c81bae6c62a54af763a4046901820038ec` |
| Before | `ddeaddf74301322fc96d8c09742ea12faa1dd8c3` |

Each corrected commit differs from its base by those two casts only. Compiler
and runner sources are unchanged. Arrays and standalone integer results were
collected earlier in the same boot session at the original bases; their
unaltered inputs and exact identities are recorded separately. The evidence
includes the initial failures, approval record, identical diffs, and a Git
bundle containing both corrected commits.

### Control-Plane Workloads

The harness validates exact READY/GO/DONE records and checksums, times GO to
DONE, rotates 11 pairs, and excludes one warmup per lane. Both before and after
reports have clean detached sources, three empty quiet-host inventories, and
successful input/hash rechecks. No override or Rust exclusion was used. Ratios
are ratios of medians; lower is faster.

| Exact protocol workload | Aura median | CPython median | Rust median | Aura / CPython | Aura / Rust |
| --- | ---: | ---: | ---: | ---: | ---: |
| Naive recursive fib(30) | 89.359292 ms | 158.321541 ms | 3.012292 ms | 0.564417 | 29.664884 |
| Create and join 10,000 tasks | 98.692417 ms | 51.786208 ms | 3.395167 ms | 1.905766 | 29.068501 |
| 20-client delayed loopback TCP fan-out | 104.268375 ms | 108.793000 ms | 104.597250 ms | 0.958411 | 0.996856 |
| 16-cycle retrying HTTP worker | 428.814667 ms | 522.391625 ms | 472.607250 ms | 0.820868 | 0.907338 |

TCP uses 20 pre-bound loopback listeners with 100 ms handlers. Aura rejects
transfer of an accepted `TcpStream` into a handler task (`AU3008`), so a single
listener would serialize handler work. Tasks include creation and joining of
all 10,000 tasks after GO. Retry runs use the same local fixture, status/delay
schedule, 112 requests and checksum 18112 in all three languages.

### Integer Loops

The **contractual release V6** comparison includes process startup. Python has
one arbitrary-precision integer lane, paired with each Aura fixed width.
Startup-adjusted estimates subtract the same-repetition startup observation;
nonpositive adjustments are retained in raw evidence and excluded only from
the estimate. All 11 whole-process observations remain in each median.

| Exact 10,000,000-iteration comparison | Aura whole process | CPython whole process | Aura startup-adjusted | CPython startup-adjusted | Valid adjusted pairs |
| --- | ---: | ---: | ---: | ---: | ---: |
| Aura `int32` / CPython integer | 28.954291 ms | 328.618208 ms | 24.342791 ms | 298.047876 ms | 11/11 |
| Aura `int64` / CPython integer | 12.772500 ms | 328.618208 ms | 6.126791 ms | 294.897875 ms | 9/11 |

Two int64 adjustments are nonpositive in each before/after run. The table's
CPython adjusted medians use the same valid pairs as their Aura row. After
startup medians are Aura 5.610000 ms and CPython 23.895667 ms. The raw report
also records dispersion; startup subtraction is an estimate, not a pure kernel
measurement.

The **diagnostic standalone** runner separately compares Aura and Rust with
11 alternating pairs, excluded warmups and exact `10000000` output. It was run
at original after merge `50531b45797ae765ec8d885165c13042617ab567`. External
pre/post inventories were quiet, but this runner lacks the release suite's
three-phase host inventory and full input/binary hash rechecks. Its medians
must not be mixed with the release V6 observations above when forming ratios.

| Standalone checked loop | Aura median | Rust median | Aura / Rust | Paired median ratio |
| --- | ---: | ---: | ---: | ---: |
| `int32` | 24.325417 ms | 16.429334 ms | 1.480609 | 1.478258 |
| `int64` | 9.778250 ms | 16.337417 ms | 0.598519 | 0.594855 |

The Rust loop places an optimizer barrier on each counter update; that cost
and process startup are part of this exact reference. These results do not
establish a general compiler-speed ranking. The before standalone runner emits
only rounded minima; before/after integer medians therefore come from the
qualified release V6 runner, not those minima.

### Numeric Arrays

The item 7 measurements are labeled **quiet host, not post-reboot; contractual re-measure scheduled with the 0.3.4 release session.**
Both runs are contractual under the runner's qualification rules: clean detached
sources, 11 rotating pairs, excluded warmups, exact protocol/checksum validation,
three empty host inventories and successful input/hash rechecks. No override
was used. The host is Mac14,9 / Apple M2 Pro / 16 GiB, with Xcode CPython 3.9.6,
NumPy 2.0.2 and Rust 1.95.0. Measurements are single-threaded, per operation;
ratios are ratios of medians.

Before is merge base `a368dce7e7b335c1c0cb800ba5240501a21f02bc`;
after is implementation head `d9fc79921ba9fed1116634ec9994bcb599aa9f90`.
Each clean checkout used its own unchanged runner in the same measurement
session, after the full local gates. Later publication edits do not change
compiler/runtime sources.

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
Aura time; its after Aura/NumPy ratio is 1.006974. The sum ratio is 6.682308
against NumPy. That gap is chiefly the deterministic reduction-order policy:
Aura remains within 1.34x of Rust under the same left-to-right order.
Addition allocates a fresh owned result; sum reuses its input. These exact
workloads do not establish a general language or Array-API performance ranking.

[Before raw](https://github.com/johnolafenwa/Aura/blob/main/work/2026-09-08-pre-batch-1-items-6-7/arrays-before-raw.json),
[after raw](https://github.com/johnolafenwa/Aura/blob/main/work/2026-09-08-pre-batch-1-items-6-7/arrays-after-raw.json),
[comparison and controls](https://github.com/johnolafenwa/Aura/blob/main/work/2026-09-08-pre-batch-1-items-6-7/array-publication-comparison.json),
and [SHA256SUMS](https://github.com/johnolafenwa/Aura/blob/main/work/2026-09-08-pre-batch-1-items-6-7/SHA256SUMS)
retain all observations and source/binary identities.

## Optimization Level

This earlier items 1–4 comparison predates the item 7 kernel rewrite.
The same-session before/after comparison includes Cranelift `speed`, Cargo
release-profile tuning and user link/strip changes. It does not isolate the
optimization flag's causal effect. Protocol rows use GO-to-DONE medians,
integer rows use whole-process medians, and Array rows use per-operation
medians. Positive change means slower.

| Exact workload | Aura before | Aura after | Change in time |
| --- | ---: | ---: | ---: |
| Naive recursive fib(30) | 110.397208 ms | 89.359292 ms | -19.0566% |
| Create and join 10,000 tasks | 100.303375 ms | 98.692417 ms | -1.6061% |
| 20-client delayed loopback TCP fan-out | 104.534666 ms | 104.268375 ms | -0.2547% |
| 16-cycle retrying HTTP worker | 427.788875 ms | 428.814667 ms | +0.2398% |
| int32 whole process | 36.445833 ms | 28.954291 ms | -20.5553% |
| int64 whole process | 15.148792 ms | 12.772500 ms | -15.6863% |
| Array add, per operation | 1.124738 ms | 1.205427 ms | +7.1740% |
| Array sum, per operation | 1.149820 ms | 1.148677 ms | -0.0994% |

The foundations bundle reduces fib and integer medians by about 16–21%, leaves
the task/network/retry comparisons much closer, and makes Array addition
7.1740% slower while Array reduction is essentially unchanged.

The same-session controls are:

| Control | Before median | After median | Drift |
| --- | ---: | ---: | ---: |
| CPython Naive recursive fib(30) | 158.086083 ms | 158.321541 ms | +0.1489% |
| CPython Create and join 10,000 tasks | 51.524875 ms | 51.786208 ms | +0.5072% |
| CPython 20-client delayed loopback TCP fan-out | 108.592667 ms | 108.793000 ms | +0.1845% |
| CPython 16-cycle retrying HTTP worker | 521.302291 ms | 522.391625 ms | +0.2090% |
| CPython int | 319.649209 ms | 328.618208 ms | +2.8059% |
| CPython startup | 22.874500 ms | 23.895667 ms | +4.4642% |
| NumPy Array add, per operation | 0.247242 ms | 0.247190 ms | -0.0208% |
| NumPy Array sum, per operation | 0.169047 ms | 0.168981 ms | -0.0394% |

Every control, including CPython startup, drifts by less than 5%; no repeated
before/after pair was required. No numeric speed target is ratified. The
measurements and dispersion from this M2 Pro host are inputs to the Batch 7
decision.

## Current Performance Gaps

Aura fib and task creation take 29.664884 and 29.068501 times their exact Rust
comparisons; TCP is near parity and the retry workload favors Aura. Array
addition/reduction now cost 1.006974/6.682308 times NumPy, or
1.006524/1.339087 times the same-session Rust kernels. These are workload-specific gaps; scheduling, runtime
implementation, allocation and optimizer-barrier choices affect interpretation.

### Per-call Attribution

Eleven symbol-preserving xctrace recordings attribute 49.29% of recovered
Fibonacci samples to call-frame push/pop and metadata validation, 37.38% to
other runtime work, and 6.28% to scalar argument/result moves. The unprofiled
estimate is 33.85854 ns per logical recursive call. A safe frame-storage
experiment improved time by 11.10%, below the required 20%, and was reverted.
No call-overhead fix ships in this item. The [boundary note](https://github.com/johnolafenwa/Aura/blob/main/architecture_docs/15-backend-boundary.md#per-call-cost-attribution)
contains all eight categories, sample exclusions and Batch 7 options.

## Performance Direction

Performance work will address task creation and scheduling, native loop and
call-boundary optimization, allocation/copying, and numeric kernels while
preserving ownership, checked arithmetic, deterministic reductions, and
MIR/direct parity. The Array kernels execute Rust runtime code, so replacing
the native emitter alone does not establish a solution to these gaps.

## Evidence And Reproduction

The earlier items 1–4 [session evidence directory](https://github.com/johnolafenwa/Aura/blob/main/work/2026-09-07-foundations-measurements) retains unchanged raw and summary
JSON, commands, compiler/source/binary identities, interpreter/toolchain and
boot/host records, warmups, observations, dispersion, hash rechecks, and failure
logs. Eleven ten-minute host retries preceded the earlier independent runs;
the approved continuation's host checks were quiet immediately. No timed
observations were collected during host refusals, and no override was used.

The earlier session [SHA256SUMS](https://github.com/johnolafenwa/Aura/blob/main/work/2026-09-07-foundations-measurements/SHA256SUMS) covers every evidence file.
The initial partial manifest is preserved as `initial-SHA256SUMS`; each summary
links its raw report by SHA-256. Temporary paths inside raw reports are retained
unchanged; the copied basenames below identify the versioned files.

| Evidence | SHA-256 |
| --- | --- |
| [approved/aura-foundations-after-release-raw.json](https://github.com/johnolafenwa/Aura/blob/main/work/2026-09-07-foundations-measurements/approved/aura-foundations-after-release-raw.json) | `a2a31328af601c32783519d9658ab164d9b415046b03b5b7856b6e09c4c79441` |
| [approved/aura-foundations-before-release-raw.json](https://github.com/johnolafenwa/Aura/blob/main/work/2026-09-07-foundations-measurements/approved/aura-foundations-before-release-raw.json) | `16aead5c0c9fe73ff2155e66b74edf982c8ec81958ab7ae3b4b2a6ae82498ec7` |
| [aura-foundations-after-arrays-raw.json](https://github.com/johnolafenwa/Aura/blob/main/work/2026-09-07-foundations-measurements/aura-foundations-after-arrays-raw.json) | `58128b5331777b8a868aae952bbc21b02145a54331f233d02efbb69b4becd22f` |
| [aura-foundations-before-arrays-raw.json](https://github.com/johnolafenwa/Aura/blob/main/work/2026-09-07-foundations-measurements/aura-foundations-before-arrays-raw.json) | `6f1f1c3b2d3fa288785204d54da2ec507a25a8c20e234584d72f8afe51c950fd` |
| [aura-foundations-after-integer-loops.json](https://github.com/johnolafenwa/Aura/blob/main/work/2026-09-07-foundations-measurements/aura-foundations-after-integer-loops.json) | `8747324a496b9280eb10bf54013b5dd06d358d3a4acf11772501b6b7dbf60dfb` |
| [SHA256SUMS](https://github.com/johnolafenwa/Aura/blob/main/work/2026-09-07-foundations-measurements/SHA256SUMS) | `911dc4a7901357b33679ad260923c56d0c8216440b200a0eb12e426ea60cac67` |

Retrieve the exact corrected sources from the
[verified Git bundle](https://github.com/johnolafenwa/Aura/blob/main/work/2026-09-07-foundations-measurements/approved/approved-measurement-commits.bundle).
The repository must already contain the two base commits listed above:

```bash
git bundle verify work/2026-09-07-foundations-measurements/approved/approved-measurement-commits.bundle
git fetch work/2026-09-07-foundations-measurements/approved/approved-measurement-commits.bundle refs/heads/codex/foundations-measured-after
git worktree add --detach /tmp/aura-foundations-after FETCH_HEAD
git fetch work/2026-09-07-foundations-measurements/approved/approved-measurement-commits.bundle refs/heads/codex/foundations-measured-before
git worktree add --detach /tmp/aura-foundations-before FETCH_HEAD
```

Build each release compiler with Rust 1.95.0 before timing. In the after
checkout, build the Rust references and run:

```bash
cargo +1.95.0 build --release --locked -p aura
cargo +1.95.0 build --manifest-path benchmarks/rust_baselines/Cargo.toml --release --locked --bins
npm run bench:release-performance -- --label foundations-after-rust --aura target/release/aura --python /Applications/Xcode.app/Contents/Developer/usr/bin/python3 --pairs 11 --raw-json /tmp/aura-foundations-after-release-raw.json --summary-json /tmp/aura-foundations-after-release-summary.json
```

In the before checkout, run its own release runner with label
`foundations-before` and before-specific output paths. Compiler and runner
sources remain frozen during each run. The original-base Array and standalone
integer commands are:

```bash
python3 scripts/bench-direct-integer-loops.py --aura target/release/aura --repeats 11 --raw-json /tmp/aura-foundations-after-integer-loops.json
python3 scripts/bench-numeric-arrays.py --label foundations-after-rust --aura target/release/aura --python /Applications/Xcode.app/Contents/Developer/usr/bin/python3 --pairs 11 --raw-json /tmp/aura-foundations-after-arrays-raw.json --summary-json /tmp/aura-foundations-after-arrays-summary.json
```

For before Arrays, use the original tag's own runner, the same interpreter and
11 pairs, label `foundations-before`, and before-specific JSON paths. For its
standalone integer runner, only `--repeats 11` is supported. Keep the distinct
measurement families and source identities separate.

## Executable Size

Current counts are exact executable bytes from a clean detached release build
at `d9fc79921ba9fed1116634ec9994bcb599aa9f90`. They include the maintained
reference agent as it was at that commit (version 0, `examples/agents/tool_runner/`,
initial source commit `fc0361bc76b8b47d300a3089b1b4bb7c2b739dfc`). The package
is now version 1 (Batch 1 phase 1: an owned-callable registry with a
factory closure and a packed bound method); its release size is re-measured
by the same bench at the phase-end head, and the table row below remains the
version-0 measurement until then.

| Executable | Tuned release bytes |
| --- | ---: |
| `aura` compiler | 10,897,408 |
| Native hello world | 1,586,968 |
| Reference agent version 0 (`tool_runner`) | 3,199,712 |

The profile uses level 3, fat LTO, one codegen unit, no debug data and stripped
compiler symbols, retaining unwinding and Aura diagnostic metadata. User
executables use unused-section removal and debug/local-symbol stripping;
the separately installed runtime archive retains its linkable symbols.
Both programs ran with Cargo unavailable. Hello world matched its single-print
output; the agent matched every byte of its package's pinned stdout.

[Size provenance](https://github.com/johnolafenwa/Aura/blob/main/work/2026-09-08-pre-batch-1-items-6-7/executable-sizes.json)
records the exact commit, commands, profile settings, source/package identities,
compiler and executable SHA-256 hashes, toolchain and standalone output.
The final PR head is rebuilt and checked against this table after publication
commits; that exact-head report is retained in the completion evidence.
The clean size checkout and target are removed after hashing.

The following items 1–4 comparison predates item 7. The older tag has
no equivalent `tool_runner` package, so no before-size reduction is claimed
for the new reference agent.

| Executable | Before: v0.3.3-preview, Cargo default | After: Cargo default | After: tuned release | Reduction from before |
| --- | ---: | ---: | ---: | ---: |
| `aura` compiler | 15,424,032 | 15,866,008 | 10,897,392 | 29.35% |
| Native hello world | 23,646,792 | 1,702,088 | 1,586,968 | 93.29% |
| Retrying-network-worker example | 23,715,816 | 4,049,240 | 3,666,600 | 84.54% |

The earlier after-default control explicitly sets the documented release
values through `CARGO_PROFILE_RELEASE_*` in a separate target directory. It
retains Cranelift speed and the user-executable link/strip steps. The before
ref is `v0.3.3-preview` (`d3cc6b96104dd597687a98e9624f800a0cb3cf1e`); exact
earlier commits, hashes and commands remain in the
[original size evidence](https://github.com/johnolafenwa/Aura/blob/main/work/2026-09-07-pre-batch-1-executable-sizes.json).
Both size sessions use Mac14,9 / Apple M2 Pro, arm64 macOS 26.5.2, Rust 1.95.0
(`59807616e`, LLVM 22.1.2), and Apple clang 21.0.0 (`clang-2100.1.1.101`).

Reproduce the current table from the desired final ref:

```bash
python3 scripts/bench-binary-size.py \
  --after-ref "$(git rev-parse HEAD)" --after-only \
  --output /tmp/aura-executable-sizes.json
```

Omit `--after-only` and pass `--before-ref v0.3.3-preview` to collect the
before/default/tuned profile comparison. The script explicitly records
that the older reference-agent subject is absent; it never substitutes
a different program. The hello input is identical single-print source, staged
outside old source trees that predate its maintained path.

### Cargo 1.95 default-profile check

The [pinned Cargo reference](https://github.com/rust-lang/cargo/blob/rust-1.95.0/src/doc/src/reference/profiles.md)
lists `strip = "none"` in the release defaults, not `debuginfo`. Per the session's
conditional decision, the size column, source JSON, and size script are retained
without remeasurement. The column explicitly sets `strip=none`; it is not an
experiment with the setting omitted. Cargo's
[deferred strip implementation](https://github.com/rust-lang/cargo/blob/rust-1.95.0/src/cargo/core/profiles.rs)
can strip pre-existing debug information automatically when the option is
omitted and no compiled package needs it. That distinction limits the existing
column's interpretation as a default-profile control.

The [reference check](https://github.com/johnolafenwa/Aura/blob/main/work/2026-09-07-foundations-measurements/approved/cargo-default-strip-recheck.json) records Cargo's
exact version, reference URL and source hash, relevant lines, and this decision.
