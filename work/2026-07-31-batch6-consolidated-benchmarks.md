# Batch 6 Consolidated Post-Reboot Benchmarks

## Goal

Replace the scattered performance history with one release-facing record for
Aurora 0.2.0. These figures are measurements of exact workloads on one host.
They are not portable language-performance claims and are not release gates.

## Provenance

The primary Aurora/CPython suite ran from clean detached commit
`18c45ac63a02887328b434c06ce3ba08d046cea3` on a Mac14,9 with an Apple M2 Pro,
10 physical/logical cores, and 16 GiB of memory. The recorded boot identity is
`Thu Jul 30 23:02:25 2026`. The comparator is Xcode CPython 3.9.6; it is not
free-threaded CPython 3.13 or later.

The runner:

- built `aura` fresh with `cargo build --release --locked -p aura`;
- built every measured Aurora input as a direct-native executable before
  timing;
- used one excluded warmup and exactly 11 rotating pairs per lane;
- required exact output, checksums, and `READY`/`GO`/`DONE` records;
- cleared known Aurora and CPython runtime-affecting overrides;
- found no competing sustained-CPU process before build, before timing, or
  after timing;
- reverified a clean detached tree and identical inputs after timing; and
- emitted `contractual: true` with no non-contractual reasons.

Evidence:

- raw report:
  `/private/tmp/aurora-b6-release-performance-raw.json`
- raw SHA-256:
  `06cc1223630b1063c8a6806bf590449d6121a3be8d33e8dc1b0ffd17cee93ccb`
- summary:
  `/private/tmp/aurora-b6-release-performance-summary.json`
- summary SHA-256:
  `4490e0d169d9a031ae57f04ade772d22169189f71a949356234f529d40e56236`
- measured release `aura` SHA-256:
  `5d95d54345bb268aa7eeaef070142bcbca410ee8f82383126d6a0390df2b087e`

## Consolidated results

Times are medians. `A/P` means Aurora divided by CPython; `A/N` means Aurora
divided by NumPy. A ratio below 1 means the Aurora interval was shorter for
that exact workload; a ratio above 1 means it was longer.

| Workload and measured interval | Aurora | Comparator | Ratio |
| --- | ---: | ---: | ---: |
| naive recursive `fib(30)`, `GO` to `DONE` | 93.875250 ms | CPython 158.491666 ms | A/P 0.592304 |
| create, join, verify, and clean 10,000 tasks | 101.743042 ms | CPython `asyncio` 51.950667 ms | A/P 1.958455 |
| 20-client TCP fan-out, `GO` to `DONE` | 104.505375 ms | CPython `asyncio` 108.605459 ms | A/P 0.962248 |
| 16-cycle retrying HTTP worker, `GO` to `DONE` | 429.291292 ms | CPython `asyncio` 520.447791 ms | A/P 0.824850 |
| 10M-increment V6 `int32`, whole process | 36.620333 ms | CPython integer 321.096625 ms | A/P 0.114048 |
| 10M-increment V6 `int64`, whole process | 13.724042 ms | CPython integer 321.096625 ms | A/P 0.042741 |
| 1M-element `float64` Array add | 1.142461 ms | NumPy 2.0.2 0.251602 ms | A/N 4.540751 |
| 1M-element existing-Array `float64` sum | 1.150392 ms | NumPy 2.0.2 0.174065 ms | A/N 6.608975 |

The protocol rows measure only after both implementations have completed
setup and emitted their exact `READY` record. Process creation and interpreter
startup remain visible in the separately retained whole-process observations.

The TCP comparison uses 20 pre-bound ephemeral listeners in both
implementations, 20 clients, and a 100 ms handler delay. Aurora 0.2 does not
permit transferring an accepted `TcpStream` to a handler task (`AU3008`), so a
single-listener Aurora shape would serialize handler work and would not test
fan-out.

The retrying-worker comparison performs 16 deterministic cycles, 112 loopback
HTTP requests, 288 ms of specified retry delay, and verifies checksum `18112`.
The task comparison creates all 10,000 tasks after `GO`, joins them, and
verifies checksum `49,995,000`.

## V6 startup estimates

Whole-process V6 timing is the primary evidence. Paired subtraction of each
runtime's same-repetition startup control is retained as a secondary estimate:

| Lane | Aurora estimate | CPython estimate | Valid pairs |
| --- | ---: | ---: | ---: |
| `int32` loop | 31.037083 ms | 295.458959 ms | 11/11 |
| `int64` loop | 7.737813 ms | 296.966042 ms for the aligned comparison | 10/11 |

One `int64` repetition produced a nonpositive startup subtraction and was
retained as invalid rather than being used in the estimate. This is why the
aligned CPython median in the 10-pair `int64` comparison differs from the
11-pair CPython estimate (`295.458959 ms`). Startup subtraction is sensitive
to process-launch noise and must not replace the whole-process result.

## Numeric Array provenance

The Array rows come from the separately qualified Phase 7.3 run at clean
detached commit `0511adf61931953df096dc1b6721a543d856be25`. It used the same
post-reboot Mac14,9 host, one worker/thread, 11 pairs, NumPy 2.0.2 backed by
Apple Accelerate, and repeated operations within each observation.

- raw:
  `/private/tmp/aurora-phase73-arrays-post-reboot-raw.json`
- raw SHA-256:
  `f51b979977519b5cbca9be4119a77bb3aff1d1a2874e1cdd4269f315bc1f9e7d`
- summary:
  `/private/tmp/aurora-phase73-arrays-post-reboot-summary.json`
- summary SHA-256:
  `f6fc84c1f0fadfb4b93a5f07befb5a33cbaa6926d54ef88a795e103106b410ab`
- measured `aura` SHA-256:
  `a717e19d2f634087ae51c601632b428ed8cc5c98ed6745039d7f036b189ca035`

Release disassembly showed scalar float kernels, so Aurora 0.2 makes no
float-SIMD claim. The measured first implementation is approximately 4.54
times the NumPy add interval and 6.61 times the NumPy sum interval for these
exact inputs.

## Runtime continuity evidence

The post-reboot schema-4 runtime run at clean commit
`18654158d22b2227149369e7911af04aafcbeecb` remains the accepted baseline
continuity record. Its V6 whole-process medians were 36.691666 ms for `int32`
and 14.837417 ms for `int64`, reproducing the earlier reactor-era baseline.
The slower pre-reboot observation was host contamination, not a demonstrated
HEAD regression.

That run also recorded the 10,000-sleeper workload at:

- 216,023,040 bytes maximum whole-process peak RSS; and
- 207,863,808 bytes maximum incremental peak RSS.

The broader massive-concurrency RSS gate remained red at 2,073,526,272 bytes
whole-process peak against its former 1.5 GiB limit. That claim was withdrawn;
this document does not generalize the 10,000-sleeper result to arbitrary task
counts.

The schema-4 evidence is
`/private/tmp/aurora-b60-post-reboot-schema4.json`, SHA-256
`134efcc894742ed73b16e07f1e31845c83d19930d5894b4dc39f01533a9be2fd`.

## Interpretation

Aurora's current strengths in this sample are native scalar execution and the
integration of scoped tasks, typed network failure, and retry control in one
compiled model. Its first Array implementation is materially behind NumPy,
and its 10,000 short-task workload takes about 1.96 times the CPython
`asyncio` interval. These results support scoped product positioning, not a
blanket claim that Aurora is faster, lighter, safer, or more scalable than the
comparison systems.

## Follow-up

Use these figures, with their workload and host qualifiers, in the Batch 6
claims audit and positioning page. Rerun the same versioned harness for later
releases rather than comparing ad hoc measurements from different hosts or
boot states.
