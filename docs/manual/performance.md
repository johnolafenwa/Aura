# Performance

Aura records exact workloads, pinned source commits, raw observations, and
content hashes. These measurements guide engineering; they are not language
semantic guarantees or ratified numeric targets.

## Current Measurements

The 7 September 2026 post-reboot session is **partially complete**. Numeric
Arrays have contractual before/after and Rust results. Standalone integer
comparisons are diagnostic-grade. The unchanged release-performance suite
fails compilation before timing at both source revisions.

The host is Mac14,9 / Apple M2 Pro (10 cores, 16 GiB), arm64 macOS 26.5.2
(build 25F84). Boot identity is `1788744561`, 7 September 2026 at 02:29:21 BST.
The after compiler is the clean detached merge commit
`50531b45797ae765ec8d885165c13042617ab567`; the before compiler is
`v0.3.3-preview`, `d3cc6b96104dd597687a98e9624f800a0cb3cf1e`.
Both compilers and the Rust references were built before timing. Each harness
also completes its own workload/reference builds before its measured samples.
CPython is Xcode 3.9.6 at
`/Applications/Xcode.app/Contents/Developer/usr/bin/python3`, with NumPy 2.0.2.
Rust is 1.95.0 (`59807616e`, LLVM 22.1.2); concurrent references pin tokio 1.53.1.

### Control-Plane Workloads

No new Aura, CPython, or Rust control-plane medians are available. Both frozen
checkouts reject `tasks_10000.au:40` and `tcp_fanout.au:62` with `AU2002`:
int64 range values are passed to int32 task parameters. The suite builds all
inputs before timing, so fib30, tasks, TCP, retry, and the release V6 lanes
have no observations from this session. This is an Aura input compilation
failure, not a failed Rust checksum or protocol exclusion.

The [failure logs and checked, unapplied two-cast proposal](https://github.com/johnolafenwa/Aura/blob/main/work/2026-09-07-foundations-measurements)
retain the blocker. Correcting both source snapshots requires an exception to
the session's explicit source freeze. This page reports only observations
collected in the current session.

### Integer Loops

The maintained standalone runner records 11 alternating Aura/Rust whole-process
pairs per width after excluded warmups and checks exact `10000000` output.
These are **diagnostic-grade** comparisons: external pre/post inventories were
quiet, but this runner does not implement the release suite's three-phase host
inventory and full input/binary hash rechecks. No competing-process override was
used. Startup is included; no startup-adjusted estimate is available here.

| Checked loop (10,000,000 increments) | Aura median | Rust median | Aura / Rust | Paired median ratio |
| --- | ---: | ---: | ---: | ---: |
| `int32` | 24.325417 ms | 16.429334 ms | 1.480609 | 1.478258 |
| `int64` | 9.778250 ms | 16.337417 ms | 0.598519 | 0.594855 |

The Rust loop uses an optimizer barrier on each counter update, as disclosed in
the [Rust workload contract](https://github.com/johnolafenwa/Aura/blob/main/benchmarks/rust_baselines/README.md).
The table does not establish a general compiler speed ranking. The before
checkout's own standalone runner reports only rounded minima (33.3 ms int32,
11.4 ms int64), without raw samples or an excluded warmup. Those minima cannot
supply the requested before/after median comparison. CPython and before V6
medians remain blocked with the release suite.

### Numeric Arrays

Both Array runs are **contractual**: 11 rotating pairs, excluded warmups,
exact READY/GO/DONE checksums, clean detached sources, three empty quiet-host
inventories per run, and successful input/hash rechecks. No override was used.
Measurements are single-threaded and reported per operation. Ratios are ratios
of medians; lower is faster.

| Workload (one million `float64` elements) | Aura median | NumPy median | Rust median | Aura / NumPy | Aura / Rust |
| --- | ---: | ---: | ---: | ---: | ---: |
| Fresh owned addition | 1.205427 ms | 0.247190 ms | 0.244576 ms | 4.876520 | 4.928642 |
| Existing-array sum | 1.148677 ms | 0.168981 ms | 0.858175 ms | 6.797687 | 1.338511 |

The paired median Aura/Rust ratios are 4.942400 for add and 1.338601 for sum.
Addition allocates a fresh owned result on every operation; sum reduces an
existing array left to right. The [Numeric Arrays](/manual/numeric-arrays)
chapter records API and reduction-order boundaries.

## Optimization Level

These are same-session before/after medians for the unchanged Array workloads.
Positive change means slower. The comparison includes Cranelift `speed`, Cargo
release-profile tuning, and user link/strip changes; it does not isolate the
optimization flag's causal effect.

| Workload | Aura before | Aura after | Aura change | NumPy before | NumPy after | Control drift |
| --- | ---: | ---: | ---: | ---: | ---: | ---: |
| Array add | 1.124738 ms | 1.205427 ms | +7.1740% | 0.247242 ms | 0.247190 ms | -0.0208% |
| Array sum | 1.149820 ms | 1.148677 ms | -0.0994% | 0.169047 ms | 0.168981 ms | -0.0394% |

Array addition became 7.1740% slower and reduction changed by -0.0994%; this
session does not demonstrate an Array speedup from the foundations bundle.
NumPy drift is below 0.04% in both controls, below the 5% repetition threshold,
so no Array repetition was required. CPython drift and control-plane/integer
median deltas cannot be computed because the frozen release inputs do not
compile. Items 1 and 3 therefore remain partially measured.

## Current Performance Gaps

The after Array addition and reduction cost 4.876520 and 6.797687 times their
NumPy comparisons, or 4.928642 and 1.338511 times these exact Rust references.
Integer width and optimizer-barrier differences matter to the diagnostic
comparison. Task/runtime conclusions require a repaired, qualified release
suite before current task/runtime ratios can be stated.

## Performance Direction

Performance work will address task creation and scheduling, native loop and
call-boundary optimization, allocation/copying, and numeric kernels while
preserving ownership, checked arithmetic, deterministic reductions, and
MIR/direct parity. The Array kernels execute Rust runtime code, so replacing
the native emitter alone does not establish a solution to these gaps.

## Evidence And Reproduction

The [session evidence directory](https://github.com/johnolafenwa/Aura/blob/main/work/2026-09-07-foundations-measurements) retains unchanged raw and summary
JSON, build/failure logs, eleven ten-minute host retries, pre/post inventories,
input checks, the unapplied correction, and the Cargo-reference check. The host
became quiet at retry 11; there were no timed observations during the refusals.
All evidence files are covered by [SHA256SUMS](https://github.com/johnolafenwa/Aura/blob/main/work/2026-09-07-foundations-measurements/SHA256SUMS).

| Evidence | SHA-256 |
| --- | --- |
| [aura-foundations-after-arrays-raw.json](https://github.com/johnolafenwa/Aura/blob/main/work/2026-09-07-foundations-measurements/aura-foundations-after-arrays-raw.json) | `58128b5331777b8a868aae952bbc21b02145a54331f233d02efbb69b4becd22f` |
| [aura-foundations-before-arrays-raw.json](https://github.com/johnolafenwa/Aura/blob/main/work/2026-09-07-foundations-measurements/aura-foundations-before-arrays-raw.json) | `6f1f1c3b2d3fa288785204d54da2ec507a25a8c20e234584d72f8afe51c950fd` |
| [aura-foundations-after-integer-loops.json](https://github.com/johnolafenwa/Aura/blob/main/work/2026-09-07-foundations-measurements/aura-foundations-after-integer-loops.json) | `8747324a496b9280eb10bf54013b5dd06d358d3a4acf11772501b6b7dbf60dfb` |
| [SHA256SUMS](https://github.com/johnolafenwa/Aura/blob/main/work/2026-09-07-foundations-measurements/SHA256SUMS) | `db6f659c3466c16a8001639e6aca33ef6d56423ca5d2eab62e4d137b42c7f999` |

The Array raw reports embed exact commands, interpreter identities, compiler,
source and binary hashes, boot/host identity, observations, dispersion, and
qualification checks. Their summaries link the raw bytes by SHA-256.

Build the clean detached after revision first:

```bash
cargo +1.95.0 build --release --locked -p aura
cargo +1.95.0 build --manifest-path benchmarks/rust_baselines/Cargo.toml --release --locked --bins
python3 scripts/bench-direct-integer-loops.py --aura target/release/aura --repeats 11 --raw-json /tmp/aura-foundations-after-integer-loops.json
python3 scripts/bench-numeric-arrays.py --label foundations-after-rust --aura target/release/aura --python /Applications/Xcode.app/Contents/Developer/usr/bin/python3 --pairs 11 --raw-json /tmp/aura-foundations-after-arrays-raw.json --summary-json /tmp/aura-foundations-after-arrays-summary.json
```

The exact release command attempted at the frozen after revision is below. It
currently fails with the source errors described above, before measuring:

```bash
npm run bench:release-performance -- --label foundations-after-rust --aura target/release/aura --python /Applications/Xcode.app/Contents/Developer/usr/bin/python3 --pairs 11 --raw-json /tmp/aura-foundations-after-release-raw.json --summary-json /tmp/aura-foundations-after-release-summary.json
```

For the before tag, build its release compiler and use its unchanged runners.
The standalone integer command is `python3 scripts/bench-direct-integer-loops.py
--repeats 11` (no raw-JSON option). For Arrays, use the same interpreter and 11
pairs, label `foundations-before`, and before-specific output paths. Retain
separate evidence; never substitute the after runner into the before checkout.

## Executable Size

The pre-Batch-1 implementation reduces executable sizes while retaining panic
unwinding and Aura's embedded diagnostic metadata. Counts below are exact bytes
from separate clean detached builds; the table counts each executable, while
installed toolchains also ship the separate runtime archive.

| Executable | Before: v0.3.3-preview, Cargo default | After: Cargo default | After: tuned release | Reduction from before |
| --- | ---: | ---: | ---: | ---: |
| `aura` compiler | 15,424,032 | 15,866,008 | 10,897,392 | 29.35% |
| Native hello world | 23,646,792 | 1,702,088 | 1,586,968 | 93.29% |
| Retrying-worker stand-in | 23,715,816 | 4,049,240 | 3,666,600 | 84.54% |

The tuned profile uses level 3, fat LTO, one codegen unit, no debug data, and
stripped compiler symbols. The after-default control explicitly sets the documented release
values through `CARGO_PROFILE_RELEASE_*` overrides in a separate target
directory; it retains the new Cranelift flag and user-executable link/strip steps.
Both after columns use macOS unused-section removal and debug/local-symbol
stripping of user binaries. The runtime archive retains its linkable symbols.

Provenance: Mac14,9 / Apple M2 Pro, arm64, macOS 26.5.2; Rust 1.95.0
(`59807616e`, LLVM 22.1.2); Apple clang 21.0.0 (`clang-2100.1.1.101`). The before
ref is `v0.3.3-preview` (`d3cc6b96104dd597687a98e9624f800a0cb3cf1e`). The after
comparison uses the delivered pre-Batch-1 implementation. Exact measured commits,
commands, profile overrides, source and executable SHA-256 hashes, and standalone
output are retained in the [versioned raw evidence](https://github.com/johnolafenwa/Aura/blob/main/work/2026-09-07-pre-batch-1-executable-sizes.json).

The hello input is `examples/basics/hello_world.au`, a single print. Because the
before ref predates that file, the tool stages identical source bytes in its
build area. `examples/agents/retrying_network_worker.au` is the reference-agent
stand-in until pre-Batch-1 item 6 lands. Both subjects produce identical output
across all three builds with Cargo unavailable during standalone execution.
Each detached checkout and target directory is removed after hashing.

Reproduce from the desired after checkout:

```bash
python3 scripts/bench-binary-size.py \
  --before-ref v0.3.3-preview \
  --after-ref "$(git rev-parse HEAD)" \
  --output /tmp/aura-executable-sizes.json
```

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

The [reference check](https://github.com/johnolafenwa/Aura/blob/main/work/2026-09-07-foundations-measurements/cargo-default-strip-check.json) records Cargo's
exact version, reference URL and source hash, relevant lines, and this decision.
