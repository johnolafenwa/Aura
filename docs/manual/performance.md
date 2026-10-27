# Performance

This page records Aura's performance measurements. Each one has an exact
workload, pinned source commits, raw observations, and content hashes. The
measurements guide engineering. They are not language semantic guarantees.
The speed targets for native code are ratios to matching Rust programs. The
[design record](https://github.com/johnolafenwa/Aura/blob/main/architecture_docs/22-batch-7-design-checkpoint.md#targets)
lists them.

## Current Measurements

The numbers come from two sessions:

- **Foundations session.** It ran on 7 September 2026, after a reboot. It
  measured the foundations changes and pinned Rust baselines.
- **Array session.** A later quiet-host Array comparison, labeled separately
  below.

Each report has a grade. A contractual report passes its runner's
qualification rules. A diagnostic-grade report has limits, which this page
states next to its numbers.

| Report | Grade |
| --- | --- |
| Release performance | **Contractual** under its runner rules |
| Numeric Arrays | **Contractual** under its runner rules |
| Standalone integer loops against Rust | **Diagnostic-grade** |
| Per-call profiling | **Diagnostic-grade** |
| Collections and value reads | **Diagnostic-grade** |
| Cross-language programs | **Diagnostic-grade** |

The foundations host:

| Item | Value |
| --- | --- |
| Machine | Mac14,9 / Apple M2 Pro (10 cores, 16 GiB) |
| OS | arm64 macOS 26.5.2 (build 25F84) |
| Boot identity | `1788744561`, 7 September 2026 at 02:29:21 BST |
| CPython | Xcode 3.9.6 at `/Applications/Xcode.app/Contents/Developer/usr/bin/python3`, with NumPy 2.0.2 |
| Rust | 1.95.0 (`59807616e`, LLVM 22.1.2) |
| Concurrent references | tokio 1.53.1 |

### Source Commits

The compiler bases are:

- **After:** merge `50531b45797ae765ec8d885165c13042617ab567`.
- **Before:** tag `v0.3.3-preview`, `d3cc6b96104dd597687a98e9624f800a0cb3cf1e`.

The original task and TCP inputs pass int64 range values to int32 task
parameters, so they fail checking at both bases. The release measurements use
clean detached commits with the same two explicitly authorized `as int32`
corrections:

| Release measurement | Corrected source commit |
| --- | --- |
| After | `4e1e48c81bae6c62a54af763a4046901820038ec` |
| Before | `ddeaddf74301322fc96d8c09742ea12faa1dd8c3` |

Each corrected commit differs from its base by those two casts only. Compiler
and runner sources are unchanged. The Array and standalone integer results
were collected earlier in the same boot session, at the original bases. Their
unaltered inputs and exact identities are recorded separately. The evidence
includes the initial failures, the approval record, identical diffs, and a Git
bundle that contains both corrected commits.

### Control-Plane Workloads

The harness validates exact READY/GO/DONE records and checksums and times GO
to DONE. It rotates 11 pairs and excludes one warmup per lane. The before and
after reports both have clean detached sources, three empty quiet-host
inventories, and successful input and hash rechecks. No override or Rust
exclusion was used. Ratios are ratios of medians. Lower is faster.

| Exact protocol workload | Aura median | CPython median | Rust median | Aura / CPython | Aura / Rust |
| --- | ---: | ---: | ---: | ---: | ---: |
| Naive recursive fib(30) | 89.359292 ms | 158.321541 ms | 3.012292 ms | 0.564417 | 29.664884 |
| Create and join 10,000 tasks | 98.692417 ms | 51.786208 ms | 3.395167 ms | 1.905766 | 29.068501 |
| 20-client delayed loopback TCP fan-out | 104.268375 ms | 108.793000 ms | 104.597250 ms | 0.958411 | 0.996856 |
| 16-cycle retrying HTTP worker | 428.814667 ms | 522.391625 ms | 472.607250 ms | 0.820868 | 0.907338 |

Workload details:

- **TCP** uses 20 pre-bound loopback listeners with 100 ms handlers. Aura
  rejects moving an accepted `TcpStream` into a handler task with `AU3008`. A
  single listener would therefore serialize handler work.
- **Tasks** includes creating and joining all 10,000 tasks after GO.
- **Retry** runs use the same local fixture, status and delay schedule, 112
  requests, and checksum 18112 in all three languages.

### Integer Loops

The **contractual release V6** comparison includes process startup. Python has
one arbitrary-precision integer lane, paired with each Aura fixed width.

A startup-adjusted estimate subtracts the startup observation from the same
repetition. Nonpositive adjustments stay in the raw evidence and are excluded
only from the estimate. All 11 whole-process observations stay in each median.

| Exact 10,000,000-iteration comparison | Aura whole process | CPython whole process | Aura startup-adjusted | CPython startup-adjusted | Valid adjusted pairs |
| --- | ---: | ---: | ---: | ---: | ---: |
| Aura `int32` / CPython integer | 28.954291 ms | 328.618208 ms | 24.342791 ms | 298.047876 ms | 11/11 |
| Aura `int64` / CPython integer | 12.772500 ms | 328.618208 ms | 6.126791 ms | 294.897875 ms | 9/11 |

Two int64 adjustments are nonpositive in each before and after run. Each
CPython adjusted median uses the same valid pairs as its Aura row. The after
startup medians are Aura 5.610000 ms and CPython 23.895667 ms. The raw report
also records dispersion. Startup subtraction is an estimate, not a pure kernel
measurement.

The **diagnostic standalone** runner compares Aura and Rust separately. It uses
11 alternating pairs, excluded warmups, and exact `10000000` output. It ran at
the original after merge `50531b45797ae765ec8d885165c13042617ab567`. External
inventories before and after were quiet. This runner lacks the release suite's
three-phase host inventory and full input and binary hash rechecks. Do not mix
its medians with the release V6 observations above when forming ratios.

| Standalone checked loop | Aura median | Rust median | Aura / Rust | Paired median ratio |
| --- | ---: | ---: | ---: | ---: |
| `int32` | 24.325417 ms | 16.429334 ms | 1.480609 | 1.478258 |
| `int64` | 9.778250 ms | 16.337417 ms | 0.598519 | 0.594855 |

The Rust loop places an optimizer barrier on each counter update. That cost
and process startup are part of this exact reference. These results do not
establish a general compiler-speed ranking. The before standalone runner emits
only rounded minima. The before and after integer medians therefore come from
the qualified release V6 runner, not from those minima.

### Numeric Arrays

These measurements carry the label "quiet host, not post-reboot; contractual
re-measure scheduled with the 0.3.4 release session."

Both runs are contractual under the runner's qualification rules:

- clean detached sources
- 11 rotating pairs and excluded warmups
- exact protocol and checksum validation
- three empty host inventories
- successful input and hash rechecks

No override was used. The host is Mac14,9 / Apple M2 Pro / 16 GiB, with Xcode
CPython 3.9.6, NumPy 2.0.2, and Rust 1.95.0. Measurements are single-threaded
and per operation. Ratios are ratios of medians.

Before is merge base `a368dce7e7b335c1c0cb800ba5240501a21f02bc`. After is
implementation head `d9fc79921ba9fed1116634ec9994bcb599aa9f90`. Each clean
checkout used its own unchanged runner in the same measurement session, after
the full local gates. Later publication edits do not change compiler or
runtime sources.

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

Addition meets the work's 1.5x Rust target at 1.006524x, with 79.9591% less
Aura time. Its after Aura/NumPy ratio is 1.006974.

The sum ratio against NumPy is 6.682308. The deterministic reduction-order
policy causes most of that gap. Under the same left-to-right order, Aura stays
within 1.34x of Rust.

Addition allocates a fresh owned result. Sum reuses its input. These exact
workloads do not establish a general language or Array-API performance ranking.

[Before raw](https://github.com/johnolafenwa/Aura/blob/main/work/2026-09-08-pre-batch-1-items-6-7/arrays-before-raw.json),
[after raw](https://github.com/johnolafenwa/Aura/blob/main/work/2026-09-08-pre-batch-1-items-6-7/arrays-after-raw.json),
[comparison and controls](https://github.com/johnolafenwa/Aura/blob/main/work/2026-09-08-pre-batch-1-items-6-7/array-publication-comparison.json),
and [SHA256SUMS](https://github.com/johnolafenwa/Aura/blob/main/work/2026-09-08-pre-batch-1-items-6-7/SHA256SUMS)
keep all observations and source and binary identities.

### Collections And Value Reads

These programs measure `dict` and `set` operations, list loops, and field
reads in native builds. They ran on a shared four-core Linux x86-64 container
with Python 3.13 on the same machine. The runner reports the median of five
runs of each whole process. The host was not quiet, so the numbers are
diagnostic-grade.

| Program | Before | After | Python 3.13 |
| --- | ---: | ---: | ---: |
| `dict_build_get`: 20,000 `str` keys, 200,000 `get` calls | 18.47 s | 0.31 s | 0.032 s |
| `set_insert_contains`: 20,000 inserts, 200,000 `in` tests | 24.69 s | 0.038 s | 0.008 s |
| `int_list_sum`: `for` over 100,000 `int64` values, 20 times | 1.96 s | 0.048 s | 0.048 s |
| `string_list_scan`: 10,000 `str` compared, 100 times | 1.60 s | 0.11 s | 0.018 s |
| `record_field_reads`: `doc.get("age")` on 5,000 dicts, 20 times | 0.80 s | 0.15 s | 0.007 s |

Before is `5f68c34`. After is the Batch 7a runtime changes. The programs live
in `benchmarks/runtime_values/`, and `scripts/bench-runtime-values.py` runs
them.

The same programs against Rust 1.95 ran later in the same kind of container,
at `aa52bfc`. Both columns time the whole process. The Rust programs use std
collections and checked arithmetic.

| Program | Aura | Rust | Aura / Rust |
| --- | ---: | ---: | ---: |
| `dict_build_get` | 321.6 ms | 21.3 ms | 15.1x |
| `set_insert_contains` | 42.1 ms | 6.1 ms | 6.9x |
| `int_list_sum` | 43.7 ms | 2.3 ms | 18.9x |
| `string_list_scan` | 120.3 ms | 2.7 ms | 44.8x |
| `record_field_reads` | 145.0 ms | 9.3 ms | 15.6x |

Process startup is about 2.0 ms for Aura and 1.2 ms for Rust, so the two list
rows understate the gap inside the loop.

The [AuraDB](https://github.com/johnolafenwa/Aura/blob/main/work/2026-10-16-auradb-benchmark-fixes.md)
document database benchmark ran in the same container:

| AuraDB operation | Before | After | Python 3.13 |
| --- | ---: | ---: | ---: |
| `insert_many`, 5,000 documents in memory | 72.6 µs/op | 9.4 µs/op | 0.67 µs/op |
| `get` by id, 5,000 documents | 45 µs/op | 5 µs/op | 5.8 µs/op |
| `find` by field scan, 5,000 documents | 188 ms | 54 ms | 13.9 ms |
| `open` and load, 5,000 documents | 1,252 ms | 201 ms | 14.2 ms |
| `update` by id, 2,000 documents | 7,895 µs/op | 20 µs/op | 6.0 µs/op |
| `delete` by id, 2,000 documents | 57,990 µs/op | 6,250 µs/op | 381 µs/op |

A `list`, `dict`, or `set` copy shares its storage until one side writes,
so AuraDB's copy of its document list on each `update` costs constant time.
A value that only one task thread can reach is read and written without a
lock.
AuraDB's `delete` recomputes every document's position key on each call,
and that loop dominates its row.

The AuraDB timer has 1 ms resolution, so the `get` row is coarse.

The [raw reports](https://github.com/johnolafenwa/Aura/blob/main/work/2026-10-18-batch-7a-measurements)
keep every observation.

### Build Times

`scripts/bench-compile-time.py` times `aura build` with the direct backend
from an installed runtime. These medians come from the same container at
`aa52bfc`:

| Program | `aura build` | `aura check` | Final link |
| --- | ---: | ---: | ---: |
| `examples/basics/hello_world.au` | 0.32 s | 0.004 s | 0.23 s |
| `examples/agents/tool_runner` | 0.62 s | 0.057 s | 0.21 s |
| `examples/io/unix_tls_roundtrip.au` | 0.40 s | 0.009 s | 0.21 s |

Checking takes under a tenth of each build. Native code generation and the
host C compiler's link take the rest. `aura build` writes a new executable
every time and does not read the native cache. `aura run` reuses a cached
executable when the program is unchanged.

These figures predate LLVM. [Phase 2 Release Check](#phase-2-release-check)
has the Aura 0.7.0 build times.

## Optimization Level

This comparison is from the foundations session, when Cranelift generated
Aura's native code. It predates the Array kernel rewrite measured above.

The same-session before and after comparison includes three changes: Cranelift
`speed`, Cargo release-profile tuning, and user link and strip changes. It
does not isolate the causal effect of the optimization flag. Protocol rows use
GO-to-DONE medians. Integer rows use whole-process medians. Array rows use
per-operation medians. A positive change means slower.

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

The foundations changes reduce the fib and integer medians by about 16–21%.
The task, network, and retry comparisons move much less. Array addition is
7.1740% slower, and Array reduction is essentially unchanged.

The same-session controls:

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

Every control drifts by less than 5%, including CPython startup. No repeated
before and after pair was needed. The measurements and dispersion from this
M2 Pro host feed later performance decisions.

## Current Performance Gaps

| Workload | Aura compared with its reference |
| --- | --- |
| fib | 29.664884 times the exact Rust comparison on the foundations host; in 0.6.0, about 5.8 times on Linux and 14.5 times on macOS arm64; in 0.7.0, 2.92 times in instructions on Linux |
| Task creation | 29.068501 times the exact Rust comparison |
| TCP | near parity |
| Retry | favors Aura |
| Array addition | 1.006974 times NumPy, 1.006524 times the same-session Rust kernel |
| Array reduction | 6.682308 times NumPy, 1.339087 times the same-session Rust kernel |
| `record_field_reads` | 3.75 times Rust in instructions on Linux in 0.7.0 |
| `string_list_scan` | 2.51 times Rust in instructions on Linux in 0.7.0 |
| `dict_build_get` | 1.93 times Rust in instructions on Linux in 0.7.0 |
| `set_insert_contains` | 1.89 times Rust in instructions on Linux in 0.7.0 |
| `int_list_sum` | 1.21 times Rust in instructions on Linux in 0.7.0 |
| AuraDB `update` by id | about 3 times Python 3.13 |
| 30 cross-language programs | median 16 times Rust and 2.7 times Python 3.13 by wall time on Linux in 0.8.0 |
| `task_pool` cross-language program | 11 times Rust; 2.8 s in 0.8.0 against 1.5 s in 0.7.1 |

These gaps are specific to these workloads. Scheduling, runtime
implementation, allocation, and optimizer-barrier choices affect how to read
them.

### Phase 1 Release Check

Aura 0.6.0 ran against the Rust references on two hosts: the shared Linux
x86-64 container and a macOS arm64 Mac14,9. Each figure is the median of 11
alternating runs. `fib(30)` times GO to DONE, and the other programs time
the whole process. The Rust references build with Rust 1.95.0.

| Program | Linux, Aura / Rust | macOS, Aura | macOS, Rust | macOS, Aura / Rust | Phase 1 target |
| --- | ---: | ---: | ---: | ---: | ---: |
| `fib(30)` | 5.8x | 42.65 ms | 2.95 ms | 14.5x | 8x |
| `int_list_sum` | 3.7x | 8.09 ms | 3.98 ms | 2.0x | 5x |
| `string_list_scan` | 5.3x | 13.05 ms | 3.65 ms | 3.6x | 10x |
| `dict_build_get` | 4.4x | 84.08 ms | 15.26 ms | 5.5x | 8x |
| `set_insert_contains` | 3.4x | 13.98 ms | 5.07 ms | 2.8x | 5x |
| `record_field_reads` | 11.0x | 67.02 ms | 5.01 ms | 13.4x | 8x |

Four programs meet their phase 1 targets on both hosts.
`record_field_reads` misses on both. `fib(30)` meets its target on Linux and
misses on macOS arm64. Phase 2 of Batch 7 owns both gaps.

### Phase 2 Release Check

Aura 0.7.0 generates native code with LLVM 22. It ran against the Rust
references on the shared Linux x86-64 container. The macOS arm64 baseline
host was not re-timed for this release, because it had no LLVM 22.

Wall times on the container are noisy, so the Linux columns count callgrind
instructions for one whole run, startup included. `fib(30)` reads its GO
line first. The Aura counts come from the release `aura` of Batch 7 step
11d, which `main` holds at `4b3a6713`. The Rust counts come from the step
9c run, built with Rust 1.95.0.

| Program | Linux, Aura instructions | Linux, Rust instructions | Linux, Aura / Rust | Phase 2 target |
| --- | ---: | ---: | ---: | ---: |
| `fib(30)` | 115,025,097 | 39,454,411 | 2.92x | 3x |
| `int_list_sum` | 16,015,568 | 13,247,624 | 1.21x | 2x |
| `string_list_scan` | 32,942,620 | 13,135,652 | 2.51x | 4x |
| `dict_build_get` | 344,784,267 | 179,017,701 | 1.93x | 4x |
| `set_insert_contains` | 59,570,448 | 31,558,085 | 1.89x | 3x |
| `record_field_reads` | 135,096,748 | 36,000,215 | 3.75x | 4x |

All six programs meet their phase 2 targets on Linux by instruction count.
The targets are ratios of time, so an instruction ratio does not stand in
for them exactly. In Aura 0.6.0, `fib(30)` and `record_field_reads` missed
their phase 1 targets on macOS arm64.

`scripts/bench-compile-time.py --repetitions 10` timed cold builds on the
same container. Medians, in seconds:

| Program | Cold build | Range | Phase 2 target |
| --- | ---: | ---: | ---: |
| Hello world | 0.404 | 0.357 to 0.764 | 0.600 |
| Reference agent | 0.960 | 0.895 to 1.060 | 1.500 |
| `unix_tls_roundtrip` | 0.927 | 0.818 to 1.459 | |

Both build targets are met. The reference agent splits into 8 codegen units
that LLVM optimizes and writes on parallel threads. Hello world stays one
unit. The
[step 11d note](https://github.com/johnolafenwa/Aura/blob/main/work/2026-10-24-batch-7-step-11d-parallel-codegen.md)
has the method and the raw counts.

### Cross-Language Programs

Aura 0.8.0 is the first release of the plan to bring Aura to Rust speed on
the 30
[cross-language programs](https://github.com/johnolafenwa/Aura/blob/main/benchmarks/cross_language/README.md),
complete programs written in Aura, Python, and Rust that do the same work.
Its first phase removed the costs that made some of them quadratic or boxed
every value. On a shared Linux x86-64 container with 4 vCPUs, at `abd8fef`:

| Measure | Aura 0.7.1 | Aura 0.8.0 |
| --- | ---: | ---: |
| All 30 programs, wall time | 227 s | 147 s |
| Median Aura / Rust, wall time | 35x | 16x |
| Median Aura / Python 3.13, wall time | 3.9x | 2.7x |
| Programs faster than Python | 4 | 9 |
| All 30 programs, instructions at the check size | 14,793M | 9,323M |

Twenty-nine programs got faster. `task_pool` went from 1.5 to 2.8 seconds
while it runs 39% fewer instructions: its workers finish each job so
quickly that they find their queue empty three times as often, and each
wait sleeps and wakes a thread. The largest remaining cost is copying a
class instance or an enum payload into a new box when a program reads it
from a list, a field, or a `match`. The benchmark README has the table for
each program, and the
[operation probes](https://github.com/johnolafenwa/Aura/blob/main/benchmarks/operation_probes/README.md)
give the cost of single operations next to Rust.

### Per-Call Attribution

Eleven symbol-preserving xctrace recordings on the foundations host attribute
recovered Fibonacci samples as follows. They measure `aa52bfc` and earlier,
before calls used static frame descriptors.

| Category | Share of recovered samples |
| --- | ---: |
| Call-frame push/pop and metadata validation | 49.29% |
| Other runtime work | 37.38% |
| Scalar argument and result moves | 6.28% |

The
[boundary note](https://github.com/johnolafenwa/Aura/blob/main/architecture_docs/15-backend-boundary.md#per-call-cost-attribution)
has all eight categories and the sample exclusions.

### Call Frame Cost

Each generated function has one static frame descriptor. It holds the
function name, source path, and position. The runtime validates the
descriptor's text on the function's first call. Each call then records one
pointer and the current task's state comes from a cached pointer. A call
that does not hand a view back to its caller stores nothing else.

Callgrind counts the instructions of the whole `fib(30)` process:

| Build | Instructions | Per recursive call |
| --- | ---: | ---: |
| `aa52bfc` | 1,917,708,454 | about 1,150 |
| Static frame descriptors | 568,735,408 | about 340 |

On the shared Linux container, 15 alternating GO to DONE runs give a
`fib(30)` median of 129.3 ms at `aa52bfc` and 34.7 ms with descriptors. The
Rust reference takes about 4 ms. Creating and joining 10,000 tasks is
unchanged at about 446 ms.

### Loop Element Reads

A shared `for` loop over a local list reads each element in place when the
element is a `str`, `list`, `dict`, or `set` and the body uses it only in
these ways:

- as a side of `==` or `!=`
- as the receiver of `len()`
- as the receiver of `get(key)`, for a `dict` element

Any other use, such as passing the element to a function, storing it, or
capturing it in a lambda, makes the loop copy each element as before.

```aura
def count_matches(names: list[str], wanted: str) -> int64:
    mut hits = 0
    for name in names:
        if name == wanted:
            hits += 1
    return hits


def main():
    print(count_matches(["ada", "bo", "ada"], "ada"))
```

On the shared Linux container, 11 alternating whole-process runs give these
medians. Before is `aa52bfc`.

| Program | Before | After |
| --- | ---: | ---: |
| `string_list_scan` | 115.0 ms | 32.8 ms |
| `record_field_reads` | 149.9 ms | 122.9 ms |
| `dict_build_get` | 298.2 ms | 283.4 ms |
| `set_insert_contains` | 33.6 ms | 33.0 ms |
| `int_list_sum` | 44.7 ms | 45.6 ms |

`record_field_reads` still spends most of its time looking up the key and
matching the `json.Value` it finds.

### List Reads

Native code reads a list's length and its `int64`, `float64`, and `bool`
elements inline. Each read checks the index once against the length. A
list that another task can reach, an index out of range, or an integer
too large for `int64` goes through the runtime, which reports the same
error as before. Appending one of those scalars to a list stores it
without a box.

Callgrind counts the instructions of each whole process. Before is the
loop element reads above.

| Program | Before | After |
| --- | ---: | ---: |
| `int_list_sum` | 444,958,513 | 106,060,271 |
| `string_list_scan` | 363,824,369 | 365,899,833 |
| `record_field_reads` | 1,137,789,339 | 1,154,420,211 |
| `dict_build_get` | 2,998,211,951 | 3,049,793,892 |
| `set_insert_contains` | 347,380,497 | 349,860,346 |

`int_list_sum` went from a median of 47.3 ms to 23.9 ms over 11
alternating runs, about 10 times the Rust reference. The other programs
run up to 1.7% more instructions. Each boxed value keeps its list's
element address and length, and every write to a box updates them.

A shared `for` loop over a list or a span reads each element with no
bounds check and advances its index with no overflow check. The loop
already compares the index with the length, and the list cannot change
length while the loop borrows it. `int_list_sum` runs 78,654,607
instructions instead of 106,060,271.

An `Array` with one dimension, indexed by one `int64`, is read inline the
same way, with one bounds check. A loop that reads each of 100,000
`float64` elements by index, 20 times, runs 63,478,022 instructions
instead of 3,445,483,320, and takes about 10.8 ms instead of 367 ms. An
`Array` with more dimensions reads through the runtime.

A list whose elements all have one fixed-width scalar type stores them
unboxed. [Unboxed Scalar Lists](#unboxed-scalar-lists) gives the details.

### Integer Array Kernels

The `int32` and `int64` Array operators, their `wrapping_` and
`saturating_` forms, and `sum()` run on machine integers in blocks of 256
elements. Checked mode computes a whole block, then scans a block that
overflowed for its first failing element. So `AU4002` names the same flat
index as an element-by-element loop. `sum()` adds a block at once when its
largest value cannot push the running sum out of range. Otherwise it adds
that block element by element and reports the first prefix that overflows.

Each program below repeats one operation 100 times on one-million-element
arrays. Each time is the fastest whole-process run on Linux x86-64, of three
runs before and five after. Times include building the inputs, about 45 ms.

| Operation | Before | After |
| --- | ---: | ---: |
| `int64` `left + right` | 3,724 ms | 138 ms |
| `int32` `sum()` | 3,974 ms | 47 ms |
| `int64` `sum()` | 4,080 ms | 100 ms |
| `int32` `wrapping_mul` | 2,935 ms | 58 ms |
| `int32` `max()`, unchanged | 68 ms | 52 ms |

### Unboxed Scalar Lists

A list whose elements all have one fixed-width scalar type stores the
scalars themselves. The types are `int8`, `int16`, `int32`, `int64`,
`uint8`, `uint16`, `uint32`, `uint64`, `float32`, `float64`, and `bool`.
Each integer element takes its own width, from 1 to 8 bytes. Each
`float32` element takes 4 bytes, each `float64` 8, and each `bool` 1. A
boxed element takes 112 bytes. Any
other list stores boxed values, and so does a list that mixes two integer
types, such as `int32` and `int64` values in an `int32 | int64` list.

The storage follows the elements. Adding a value of another type, such as
a `str` to an `int64 | str` list, moves the list to boxed storage. An empty
list takes the form of its first element. A program cannot tell which form
a list uses: printing, equality, hashing, copies, and errors behave the
same. A `list[float32]` prints its elements with `float32` digits, as
`print` prints a `float32` value, so `[0.1]` rather than
`[0.10000000149011612]`.

Native code reads the length and the elements of an unboxed list in place,
and appends, writes, and pops elements in place. `pop()` with no index
returns the last element without a box. A list that another task can reach
goes through the runtime.

On the shared Linux container, 11 alternating whole-process runs give these
medians. Before reads lists as the List Reads section describes, with boxed
elements.

| Program | Before | After |
| --- | ---: | ---: |
| `int_list_sum` | 21.0 ms | 9.5 ms |
| `string_list_scan` | 32.5 ms | 31.6 ms |
| `record_field_reads` | 122.0 ms | 122.5 ms |
| `dict_build_get` | 326.3 ms | 316.5 ms |
| `set_insert_contains` | 40.8 ms | 41.7 ms |

`int_list_sum` runs about 4.1 times the Rust reference. Callgrind counts
73,209,689 instructions instead of 78,657,685, so most of the gain comes
from the smaller list fitting in the processor cache. The other programs
run within 1% of their previous instruction counts.

### Strings, Keys, And Membership Without Boxes

Native code builds an f-string in one string. The first literal starts it,
and each later part appends in place. A number or `bool` part is copied
when the f-string evaluates it and rendered as it appends, because rendering
it runs no `Display` code. An `int64` part appends without a box.

A literal `str` key, as in `doc.get("age")` on a `dict[str, V]`, is looked
up where it lies in the program image. An `int64` needle, as in `j in seen`
on a `set[int64]`, is tested without a box. A `for` loop that lends each
element of a list checks the index against the length once, so the loan
begins without a second check. Native code also skips a type tag that
repeats the tag the same value received earlier in the block.

On the shared Linux container, 11 alternating whole-process runs give these
medians.

| Program | Before | After | Instructions before | Instructions after |
| --- | ---: | ---: | ---: | ---: |
| `int_list_sum` | 11.6 ms | 10.8 ms | 73,209,658 | 73,212,011 |
| `string_list_scan` | 35.7 ms | 14.1 ms | 370,041,432 | 150,970,171 |
| `record_field_reads` | 127.3 ms | 84.4 ms | 1,173,189,605 | 692,557,847 |
| `dict_build_get` | 322.4 ms | 89.3 ms | 3,080,859,389 | 939,551,832 |
| `set_insert_contains` | 39.6 ms | 18.0 ms | 351,223,140 | 148,691,758 |

`record_field_reads` still copies each `Lookup` payload and each enum name,
and every box passes through the task's ownership ledger.

### Plain Record Lists

A plain record is a class whose fields are all `int32`, `int64`, `uint64`,
`float32`, `float64`, or `bool` values, or other plain records. It holds no
`str`, collection, handle, or union. Native code keeps a list of plain
records as the records' fields side by side in the list's storage, the way
a Rust `Vec` of a `Copy` struct does. Reading `points[i].x` loads the field
from the list, and `points.append(p)` and `points[i] = p` store the fields
there. Moving a record into or out of a box, as passing it where any value
is accepted does, takes one runtime call, and so does reading a record
field of a class instance that holds a handle.

The storage follows the elements, as for the scalar lists above. A program
cannot tell which form a list uses: printing, equality, hashing, copies,
and errors behave the same.

Callgrind instruction counts on the shared Linux container, cross-language
suite at the check size:

| Program | Before | After |
| --- | ---: | ---: |
| `bytecode_vm` | 48.9M | 6.9M |
| `graph_paths` | 561.5M | 80.4M |
| `raytracer` | 464.1M | 94.0M |

Reading a `float64` field of a record from a `list` costs about 23
instructions instead of about 4,900.

### Lent List Elements

Reading an element of a list of class instances, enums, lists, dicts, or
other values that need a box used to copy the element into a new box.
Native code now lends the element instead when the local that receives it
is only read: the program reads its fields, calls methods that only read
it, or matches it. Each element of a `for` loop that reads its fields is
such a read, and so is `len(items[i].name)`. A read whose value is kept,
changed, or passed to a function still copies.

The list keeps each element it lends in a box of its own. A list moves to
that form once native code has copied out as many elements as an eighth of
its length, so a list that is read only a few times is never converted.
`str` elements keep the copying read, which their list view already makes
cheap.

A lent element keeps the value it had when it was read. Before the program
changes, frees, or passes on the list while it still reads the element,
the element is copied, as every read used to be. A list that a closure, a
task, or a mutable borrow can reach is never lent from, and a list that
another task can reach, or that shares its storage with a copy, is read
through the runtime first.

Callgrind instruction counts on the shared Linux container, cross-language
suite at the check size:

| Program | Before | After |
| --- | ---: | ---: |
| `interpreter` | 368.8M | 238.3M |
| `mini_browser` | 1,392.1M | 1,153.8M |
| `sort_records` | 75.2M | 67.1M |
| `sql_engine` | 2,809.7M | 2,616.8M |

The other programs change by less than 2%. The largest increase is
`order_book`, at 1.1%.

### Class Fields In Place

A class instance that native code builds keeps its fields in one block in
the class's native layout, as a Rust struct does. Scalar fields and fields
that hold other classes sit in the block, and each `str`, collection,
enum, union, or runtime object field is a pointer to a box of its own.
Reading `self.count` loads the field from the block and writing it stores
it there, about five instructions each, where the instance used to keep a
map from field names to values and every read copied the field into a new
box. An operation on a field, such as `self.items.append(x)` or
`self.seen.add(k)`, works on the field's box where it lies, and a class
literal builds the block with one runtime call.

Generic classes and classes whose fields hold the class itself, such as a
linked list node, keep the map, and so do instances `aura run` and the
runtime build, such as those `json` decoding returns. A program cannot
tell which form an instance uses: printing, equality, hashing, copies,
and errors behave the same. An instance another task can reach is read
through the runtime.

Callgrind instruction counts on the shared Linux container, cross-language
suite at the check size:

| Program | Before | After |
| --- | ---: | ---: |
| `order_book` | 114.9M | 19.3M |
| `regex_match` | 1,940.6M | 366.6M |
| `bank_ledger` | 41.7M | 19.3M |
| `raytracer` | 78.1M | 38.4M |
| `spreadsheet` | 54.4M | 33.4M |
| `lru_cache` | 426.1M | 276.0M |
| `sort_records` | 67.1M | 46.9M |
| `mini_browser` | 1,153.8M | 819.5M |
| `sql_engine` | 2,616.8M | 1,936.2M |

No other program grows by more than 2.6%.

### List Element Fields In Place

`counters[i].count += 1` changes the field where it lies. Native code
checks the list's form and the position once, then the element's class,
and reads and writes the field in the element's block. An operation on a
field of an element or on an element itself, such as
`items[i].tags.append(t)` or `rows[i].append(v)`, works on that box where
it lies. A list of scalars is changed in its storage the same way. Each
write costs about 28 instructions, where it used to walk the element path
through the runtime for about 6,800.

This works for a list in a local or in a field of a class instance kept in
its layout, with one position selected, as in `self.levels[p].append(v)`.
A nested selection, such as `grid[i][j] += 1`, and an element of a `dict`
still take the runtime walk. A class instance appended to a list keeps its
box in the list, so a list built by appends is ready from the first pass.
A list of `Value`s moves its elements into boxes of their own once loans
have missed as many times as an eighth of its length.

A list and its copy share their storage until either changes, so native
code changes an element in place only while the list's storage is its
own. After `copy()`, the next change goes through the runtime, which
copies the storage first, and the copy keeps the old elements.

Callgrind instruction counts on the shared Linux container, cross-language
suite at the check size:

| Program | Before | After |
| --- | ---: | ---: |
| `pubsub_broker` | 146.8M | 124.4M |
| `order_book` | 18.9M | 16.2M |
| `binary_trees` | 16.7M | 15.0M |
| `spreadsheet` | 33.2M | 31.1M |
| `mini_browser` | 773.1M | 739.5M |
| `sql_engine` | 1,808.4M | 1,758.2M |

No other program grows by more than 0.4%, apart from `task_pool`, whose
count moves with thread scheduling.

## Performance Direction

Performance work will address task creation and scheduling, native loop and
call-boundary optimization, allocation and copying, and numeric kernels. It
will keep ownership, checked arithmetic, deterministic reductions, and
MIR/direct parity. The Array kernels run Rust runtime code. Replacing the
native emitter alone does not solve these gaps.

## Evidence And Reproduction

The foundations
[session evidence directory](https://github.com/johnolafenwa/Aura/blob/main/work/2026-09-07-foundations-measurements)
keeps:

- unchanged raw and summary JSON
- commands
- compiler, source, and binary identities
- interpreter, toolchain, boot, and host records
- warmups, observations, and dispersion
- hash rechecks and failure logs

Eleven ten-minute host retries preceded the earlier independent runs. For the
approved continuation, the host checks were quiet immediately. No timed
observations were collected while the host check refused a run, and no
override was used.

The session
[SHA256SUMS](https://github.com/johnolafenwa/Aura/blob/main/work/2026-09-07-foundations-measurements/SHA256SUMS)
covers every evidence file. The initial partial manifest is kept as
`initial-SHA256SUMS`. Each summary links its raw report by SHA-256. Raw reports
keep their original temporary paths. The copied basenames below identify the
versioned files.

| Evidence | SHA-256 |
| --- | --- |
| [approved/aura-foundations-after-release-raw.json](https://github.com/johnolafenwa/Aura/blob/main/work/2026-09-07-foundations-measurements/approved/aura-foundations-after-release-raw.json) | `a2a31328af601c32783519d9658ab164d9b415046b03b5b7856b6e09c4c79441` |
| [approved/aura-foundations-before-release-raw.json](https://github.com/johnolafenwa/Aura/blob/main/work/2026-09-07-foundations-measurements/approved/aura-foundations-before-release-raw.json) | `16aead5c0c9fe73ff2155e66b74edf982c8ec81958ab7ae3b4b2a6ae82498ec7` |
| [aura-foundations-after-arrays-raw.json](https://github.com/johnolafenwa/Aura/blob/main/work/2026-09-07-foundations-measurements/aura-foundations-after-arrays-raw.json) | `58128b5331777b8a868aae952bbc21b02145a54331f233d02efbb69b4becd22f` |
| [aura-foundations-before-arrays-raw.json](https://github.com/johnolafenwa/Aura/blob/main/work/2026-09-07-foundations-measurements/aura-foundations-before-arrays-raw.json) | `6f1f1c3b2d3fa288785204d54da2ec507a25a8c20e234584d72f8afe51c950fd` |
| [aura-foundations-after-integer-loops.json](https://github.com/johnolafenwa/Aura/blob/main/work/2026-09-07-foundations-measurements/aura-foundations-after-integer-loops.json) | `8747324a496b9280eb10bf54013b5dd06d358d3a4acf11772501b6b7dbf60dfb` |
| [SHA256SUMS](https://github.com/johnolafenwa/Aura/blob/main/work/2026-09-07-foundations-measurements/SHA256SUMS) | `911dc4a7901357b33679ad260923c56d0c8216440b200a0eb12e426ea60cac67` |

Get the exact corrected sources from the
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
sources stay frozen during each run.

The original-base Array and standalone integer commands are:

```bash
python3 scripts/bench-direct-integer-loops.py --aura target/release/aura --repeats 11 --raw-json /tmp/aura-foundations-after-integer-loops.json
python3 scripts/bench-numeric-arrays.py --label foundations-after-rust --aura target/release/aura --python /Applications/Xcode.app/Contents/Developer/usr/bin/python3 --pairs 11 --raw-json /tmp/aura-foundations-after-arrays-raw.json --summary-json /tmp/aura-foundations-after-arrays-summary.json
```

For the before Arrays, use the original tag's own runner with the same
interpreter, 11 pairs, label `foundations-before`, and before-specific JSON
paths. The before standalone integer runner supports only `--repeats 11`. Keep
the measurement families and their source identities separate.

## Representation

These measurements count allocations with the runtime's own counters, not a
host allocator. On both backends, the runtime counts every union payload box,
closure environment, opaque runtime box, and callable overflow allocation. A
process run with `AURA_RUNTIME_STATS=1` prints the totals on standard error
when its program ends.

The same line reports how often the direct backend retains, releases, and
copies a boxed value, as `retains`, `releases`, and `clones`. The direct
runtime counts these only when the report is requested. The interpreter
reports zero for them.

`benchmarks/representation` holds the programs.
`scripts/bench-representation.py` records the counters, the direct binary's
size, and wall time, with commit provenance. The table shows the counters,
which are deterministic for these programs. They were measured at `a8aac70a`
and archived with a checksum manifest in
[work/2026-09-20-representation-measurements](https://github.com/johnolafenwa/Aura/blob/main/work/2026-09-20-representation-measurements).

| Program | Interpreter | Direct backend |
| --- | --- | --- |
| `union_scalar_local` (one million `int64 \| None` injections and tag tests) | 2,000,000 union boxes | 0 union boxes |
| `union_class_field` (one hundred thousand `Point \| None` class fields) | 100,000 union boxes | 0 union boxes |
| `union_string_local` (one hundred thousand `str \| None` injections and matches) | 100,000 union boxes | 0 union boxes |
| `callable_pack_inline` (one hundred thousand one-capture packings) | 100,000 environments | 0 environments |
| `callable_pack_overflow` (one hundred thousand four-capture packings) | 100,000 environments | 0 environments, 100,000 overflow blocks |
| `callable_move` (one packing moved one hundred thousand times) | 1 environment | 0 environments |

**Unions.** On the direct backend, every concrete union is one tagged value in
registers and locals. A runtime-object member rides along as an owned handle
word. Injections allocate no union box. The interpreter's box counts are
reference numbers for its `Value` model, not an ABI claim.

**Callables.** On the direct backend, a callable value is four words.

- Captures that fit in three words live in the value.
- A wider environment takes one checked block, counted as a callable overflow
  allocation.
- Packing, calling, and moving allocate no closure environment.
- The direct backend boxes a callable only where it leaves generated code: a
  container, a task start, a runtime helper, or a generic frame. The
  closure-environment counter reports these boxes.

Design record:
[first-class callables and binding contracts](https://github.com/johnolafenwa/Aura/blob/main/architecture_docs/decisions/0058-first-class-callables-and-binding-contracts.md)

## Executable Size

A native executable contains the Aura runtime code its program can reach.
A program that never uses TLS, WebSocket, TOML, or JSON does not carry their
libraries. The counts below are exact bytes from release builds that
include unboxed scalar lists. The size gate records them as its baselines.

| Executable | Linux x86-64 | macOS arm64 |
| --- | ---: | ---: |
| Native hello world | 973,016 | 961,496 |
| Reference agent (`tool_runner`) | 1,915,000 | 1,792,816 |
| JSON example | 1,506,456 | 1,431,448 |
| TLS example | 3,303,264 | 2,860,000 |
| WebSocket example | 3,608,752 | 3,137,448 |

On Linux x86-64, the `aura` compiler is about 66 MB stripped, because it
links LLVM statically. The runtime archive `libaura_runtime.a` is about
63 MB. User executables link the runtime archive. It contains no compiler
code, and the size gate checks that no user program links LLVM.

The JSON example is `examples/json/dynamic_values.au`. The TLS example is
`examples/io/unix_tls_roundtrip.au`, and the WebSocket example is
`examples/io/websocket_roundtrip.au`. Each links only the libraries it uses:

| Executable | Links | Does not link |
| --- | --- | --- |
| Hello world | runtime core | `serde_json`, `toml`, `http`, `httparse` |
| Reference agent, JSON example | runtime core, `serde_json` | `toml` |
| TLS example | runtime core, `rustls`, `ring` | `serde_json`, `http`, `httparse` |
| WebSocket example | runtime core, `rustls`, `ring`, `tungstenite`, `http`, `httparse`, `url` | `serde_json` |

Hello world still holds about 11 KB of `rustls`, `ring`, and `tungstenite`
cleanup code. The runtime's value type can hold a TLS or WebSocket
connection, so the code that drops one stays reachable.

The compiler profile uses level 3, fat LTO, one codegen unit, and no debug
data, and strips compiler symbols. It keeps unwinding and Aura diagnostic
metadata. User executables use unused-section removal and debug and
local-symbol stripping. Each generated function has its own object section,
so the linker drops runtime code no function calls.

Hello world and the reference agent ran with Cargo unavailable. Hello world
matched its single-print output. The agent matched every byte of its
package's pinned stdout.

[Linux size provenance](https://github.com/johnolafenwa/Aura/blob/main/work/2026-10-17-batch-5-step-6/executable-sizes-linux.json)
records the commit, commands, toolchain, linker, source identities, and
executable SHA-256 hashes. The macOS counts come from the `native-binaries`
CI job on the same tree. That job builds every subject in
`benchmarks/native_binaries/contents.json` on both platforms. It fails when
a library appears in a program that should not link it, or when hello world
or another subject grows more than 10% over its recorded size.

### Foundations Size Comparison

This comparison is from the foundations session and predates the Array kernel
rewrite. The older tag has no equivalent `tool_runner` package, so the table
claims no before-size reduction for the reference agent.

| Executable | Before: v0.3.3-preview, Cargo default | After: Cargo default | After: tuned release | Reduction from before |
| --- | ---: | ---: | ---: | ---: |
| `aura` compiler | 15,424,032 | 15,866,008 | 10,897,392 | 29.35% |
| Native hello world | 23,646,792 | 1,702,088 | 1,586,968 | 93.29% |
| Retrying-network-worker example | 23,715,816 | 4,049,240 | 3,666,600 | 84.54% |

The after-default control sets the documented release values through
`CARGO_PROFILE_RELEASE_*` in a separate target directory. It keeps Cranelift
speed and the user-executable link and strip steps. The before ref is
`v0.3.3-preview` (`d3cc6b96104dd597687a98e9624f800a0cb3cf1e`). The
[original size evidence](https://github.com/johnolafenwa/Aura/blob/main/work/2026-09-07-pre-batch-1-executable-sizes.json)
keeps the exact earlier commits, hashes, and commands.

Both size sessions use:

- Mac14,9 / Apple M2 Pro, arm64 macOS 26.5.2
- Rust 1.95.0 (`59807616e`, LLVM 22.1.2)
- Apple clang 21.0.0 (`clang-2100.1.1.101`)

### Reproducing The Size Table

Reproduce the hello world, reference agent, and compiler sizes from a clean
checkout of the desired ref:

```bash
python3 scripts/bench-binary-size.py \
  --after-ref "$(git rev-parse HEAD)" --after-only \
  --output /tmp/aura-executable-sizes.json
```

Measure every subject and check its contents against a local release build:

```bash
cargo build --release -p aura
python3 scripts/check-native-binaries.py
```

Add `--attribute` to print the symbol bytes for each library.

To collect the before, default, and tuned profile comparison, omit
`--after-only` and pass `--before-ref v0.3.3-preview`. The script records that
the older reference-agent subject is absent. It never substitutes a different
program. The hello input is identical single-print source. It is staged
outside old source trees that predate its maintained path.

### Cargo 1.95 Default-Profile Check

The
[pinned Cargo reference](https://github.com/rust-lang/cargo/blob/rust-1.95.0/src/doc/src/reference/profiles.md)
lists `strip = "none"` in the release defaults, not `debuginfo`. Under the
session's conditional decision, the size column, source JSON, and size script
are kept without remeasurement.

The column sets `strip=none` explicitly. It is not an experiment with the
setting omitted. When the option is omitted and no compiled package needs
debug information, Cargo's
[deferred strip implementation](https://github.com/rust-lang/cargo/blob/rust-1.95.0/src/cargo/core/profiles.rs)
can strip pre-existing debug information automatically. That difference limits
how far the column works as a default-profile control.

The
[reference check](https://github.com/johnolafenwa/Aura/blob/main/work/2026-09-07-foundations-measurements/approved/cargo-default-strip-recheck.json)
records Cargo's exact version, the reference URL and source hash, the relevant
lines, and this decision.
