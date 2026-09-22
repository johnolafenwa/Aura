# Direct integer loop baseline

This benchmark is the V6 workload: a counter loop of ten million iterations,
built with the direct native backend at `int32` and `int64` width. It keeps
both widths so the relationship between them stays visible instead of being
summarized away.

`startup.au` is a silent, empty direct program with the same runtime entry
path. The scalable-runtime runner pairs it with both loops so fixed process
startup is not mistaken for loop cost.

Run the benchmark with:

```bash
npm run bench:direct-integer-loops -- \
  --aura target/release/aura --repeats 11 \
  --raw-json /tmp/aura-rust-integer-loops.json
```

Run it from a clean checkout with a freshly built compiler. The runner does
one excluded warmup. It then reports raw samples, medians, minima, and paired
Aura/Rust ratios. Lane order and width order alternate across repetitions.

This helper and the scalable-runtime runner both launch each measured binary
in its own process group. They verify that the whole group is reaped on
success, error, timeout, or interrupt.

The contractual scalable-runtime runner also reports a startup split. See
[Gate interpretation](../scalable_runtime/README.md#gate-interpretation) for
what makes a report contractual.

```bash
npm run bench:scalable-runtime -- \
  --label v6-startup-split \
  --aura target/release/aura \
  --json /tmp/aura-v6-startup-split.json
```

The split rotates the process order of startup, `int32`, and `int64` on each
repetition. It publishes whole-process summaries and paired
`whole process - startup` loop estimates under
`benchmarks.v6.startup_vs_loop`.

The scalable-runtime report uses schema version 4. Version 4 identifies each
run with a `workload` field and adds the `startup` workload and the split
summary. Schema version 3 identified each V6 run with a `width` field and
contained only the two integer loops.

## Recorded baseline

Development workstation, Apple silicon, debug `aura` driving a
release-quality Cranelift build, seven repeats.

| Width | Before the V6 fix | After the V6 fix |
| --- | --- | --- |
| `int32` | 0.0697s | 0.0327s |
| `int64` | 0.0115s | 0.0111s |
| `int32` / `int64` | 6.05x | 2.95x |

The `int64` time is unchanged, as expected, because the fix touches only the
range check for narrow widths. See `work/2026-07-25-v6-direct-int32-loops.md`
for the diagnosis and for what still separates the two widths.

## Reactor-era baseline

The accepted reactor-era baseline is the clean Phase 5.10 whole-process pair:
`37.436334 ms` for `int32` and `15.005584 ms` for `int64`. It was measured
after Phase 5 on the clean Mac14,9 host. Each value is the median of five
measured repetitions after warmup.

| Measurement | Startup median | `int32` whole process | `int64` whole process | `int32` loop estimate | `int64` loop estimate |
| --- | ---: | ---: | ---: | ---: | ---: |
| Batch 2 medians | | `32.734250 ms` | `10.248625 ms` | | |
| Accepted Phase 5.10 baseline, clean, five repetitions | | `37.436334 ms` | `15.005584 ms` | | |
| Startup split, dirty checkout, 21 repetitions | `7.679583 ms` | `49.391916 ms` | `18.875542 ms` | `41.746208 ms` | `11.123916 ms` |
| `AURA_WORKERS=1` diagnostic | `7.851334 ms` | | | | |
| Batch 6 replay after reboot, clean, five repetitions | `6.574667 ms` | `36.691666 ms` | `14.837417 ms` | `30.292500 ms` | `8.255709 ms` |

The accepted baseline is `14.36%` above the Batch 2 `int32` median and
`46.42%` above the Batch 2 `int64` median. It does not meet the attempted
"within 10%" restoration target.

The startup split measures a fixed runtime-entry component separately from the
loop. The 21-repetition split ran in a dirty checkout during concurrent
Batch 5 work. It proves that the split works. It does not establish the
complete cause of the regression, and it does not replace the clean baseline.
In particular, its `41.746208 ms` `int32` loop estimate does not reproduce the
Batch 2 whole-process median.

The `AURA_WORKERS=1` diagnostic gives no evidence that one worker reduces the
startup component. On its own, it does not prove which initialization work
causes the historical gap. The direct root scheduler owns task cleanup, traps,
cancellation, and cooperative scheduling. It is not bypassed for scalar
programs without evidence for a safe replacement.

Under the alternate disposition that B5.0-d permits, the clean Phase 5.10 pair
is accepted as the reactor-era baseline. The evidence does not isolate startup
as the complete cause of the regression. The runner keeps the startup split so
future loop work can compare loop estimates separately from process entry
cost.

Batch 6 repeated the schema 4 measurement after a real reboot of the baseline
host. That clean, contractual replay is within `1.99%` for `int32` and `1.12%`
for `int64` of the accepted Phase 5.10 pair, and slightly faster. It did not
reproduce the dirty `49.391916 ms` / `18.875542 ms` diagnostic. The accepted
reactor-era pair therefore remains the baseline. The cold-boot result and full
provenance are in `work/2026-07-27-phase5-runtime-benchmarks.md`.

## Replay after the unified int64 index domain

This clean, contractual schema 4 replay ran at
`face52e3900f775a3284df56a2519622d8381d60`. It used a fresh locked release
build from `2026-08-02T20:13:03.022427+00:00` on the same Mac14,9 Apple M2 Pro
/ 16 GiB host. It kept the established protocol: one warmup, five rotating
repetitions, exact-output validation, and paired startup subtraction.

| Lane | Whole-process median | Paired loop-estimate median |
| --- | ---: | ---: |
| `int32` | 36.222917 ms | 29.305958 ms |
| `int64` | 14.673875 ms | 7.744333 ms |
| startup | 6.570375 ms | |

All five adjusted pairs were valid. The whole-process medians are 1.28% and
1.10% lower than the Batch 6 observations after reboot. The width ratio is
2.469x, against 2.473x.

The unified `int64` index domain from S2 therefore adds no V6 regression. It
does not change the outstanding narrow-arithmetic representation work.

The raw report is `/tmp/aura-s1-post-s2-v6-face52e.json`, with SHA-256
`491d1268398c46b0c55393d7542d63a93804034ba6e8b128be67565f93fcdf64`.

## Rust comparison lane (0.3.4 foundations)

The runner builds pinned Rust 1.95.0 references from `benchmarks/rust_baselines/`
with `--release --locked`, fat link-time optimization (LTO), and one codegen
unit. The report uses schema 2. It records source, lockfile, and binary SHA-256
identities and paired Aura/Rust samples. Protocol smoke checks are not
performance evidence.

The standalone comparison from 7 September 2026 records diagnostic-grade
whole-process medians:

| Width | Aura median | Rust median | Paired median ratio |
| --- | ---: | ---: | ---: |
| int32 | 24.325417 ms | 16.429334 ms | 1.478258 |
| int64 | 9.778250 ms | 16.337417 ms | 0.594855 |

It uses 11 alternating pairs, excluded warmups, and exact checksums. The
external inventories before and after the run were quiet. The runner lacks the
release suite's three-phase inventory and full hash rechecks.
[Raw evidence](https://github.com/johnolafenwa/Aura/blob/main/work/2026-09-07-foundations-measurements/aura-foundations-after-integer-loops.json)
has SHA-256
`8747324a496b9280eb10bf54013b5dd06d358d3a4acf11772501b6b7dbf60dfb`.
The before-tag output contains only rounded minima, so it has no comparable
medians.

The release V6 comparison uses identically corrected task and TCP sources. Its
contractual whole-process medians are:

| Width | Before | After |
| --- | ---: | ---: |
| int32 | 36.445833 ms | 28.954291 ms |
| int64 | 15.148792 ms | 12.772500 ms |

Keep these separate from the standalone Aura/Rust ratios, because the two
families come from different runners. The
[Performance chapter](../../docs/manual/performance.md) records both families,
startup-adjusted estimates, controls, and all hashes.

```bash
python3 scripts/bench-direct-integer-loops.py --aura target/release/aura --repeats 11 --raw-json /tmp/aura-foundations-after-integer-loops.json
```

See [the Rust baseline contract](../rust_baselines/README.md) for exact
workload, allocation, arithmetic, scheduling, and protocol equivalence. The
integer-loop lane keeps its whole-process checksum protocol. The other lanes
use READY/GO/DONE.
