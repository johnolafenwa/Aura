# Release-performance workloads

This directory contains the Aura and CPython inputs established for Batch 6.
The maintained runner now also builds equivalent pinned Rust inputs from
`../rust_baselines/` and records all three lanes with host and source provenance.

## Workloads

- `fib30.au` and `fib30.py` run the same naive recursive `fib(30)` algorithm
  and must produce `832040`.
- `tasks_10000.au` and `tasks_10000.py` create exactly 10,000 tasks after the
  measurement starts, join every task, and must produce the checksum
  `49995000`.
- `tcp_fanout.au` and `tcp_fanout.py` run 20 concurrent loopback clients and
  20 delayed handlers. Each client receives `pong`, producing the checksum
  `80`. Aura uses 20 pre-bound ephemeral listeners because Aura 0.2 does
  not permit transferring an accepted `TcpStream` to a handler task
  (`AU3008`); a single listener would serialize handler work rather than
  measure fan-out.
- `retrying_worker.au` and `retrying_worker.py` execute 16 identical HTTP
  retry cycles. Each cycle covers recovery after `503`, rate limiting with
  `429`, and exhausted `503` retries. A valid run serves 112 requests, has
  288 ms of specified retry delay, and produces the checksum `18112`.
- `python_int_loop.py` and `python_startup.py` are the CPython counterparts to
  the accepted Aura V6 sources in `../direct_integer_loops/`. Python has one
  arbitrary-precision integer lane, so the same Python loop is paired
  separately with Aura's `int32` and `int64` lanes.

The numeric-Array comparison is intentionally not rerun by this harness. Its
separately qualified NumPy evidence is linked in the raw and summary reports
and merged into the consolidated benchmark note.

## Measurement protocol

The fib, task, TCP, and retry inputs use exact `READY`/`GO`/`DONE` records.
The host starts timing only after receiving and validating `READY`, then sends
the exact `GO` record. For example:

```text
READY release-performance fib30 30
GO release-performance fib30
DONE release-performance fib30 832040
```

The other successful records are:

```text
READY release-performance tasks 10000
GO release-performance tasks
DONE release-performance tasks 10000 49995000

READY release-performance tcp-fanout 20 100 4
GO release-performance tcp-fanout
DONE release-performance tcp-fanout 20 80

READY release-performance retrying-worker 16 112 288
GO release-performance retrying-worker
DONE release-performance retrying-worker 112 18112
```

Any unexpected record, checksum, standard-error output, timeout, or nonzero
exit invalidates the run. V6 remains a whole-process comparison so the exact
accepted sources are reused. The runner reports raw whole-process durations
as primary evidence and also reports same-repetition startup subtraction as a
loop-only estimate. Nonpositive adjusted samples are retained as invalid
observations and excluded from the estimate rather than invalidating unrelated
measurements.

## Reproducing the qualified run

Run from a clean detached checkout on the accepted post-reboot Mac14,9 host
with no competing sustained-CPU process:

```bash
/Applications/Xcode.app/Contents/Developer/usr/bin/python3 \
  scripts/bench-release-performance.py \
  --label foundations-after-rust \
  --aura target/release/aura \
  --python /Applications/Xcode.app/Contents/Developer/usr/bin/python3 \
  --pairs 11 \
  --raw-json /tmp/aura-foundations-after-release-raw.json \
  --summary-json /tmp/aura-foundations-after-release-summary.json
```

The runner requires exactly 11 rotating pairs, performs one excluded warmup
per lane, builds a fresh locked release compiler and all Aura workload
binaries before timing, verifies CPython identity, clears known
runtime-affecting environment overrides, and rechecks the repository and input
hashes after timing. It records three quiet-host inventories, boot and hardware
identity, commands, raw observations, hashes, median, MAD, nearest-rank p95,
best, and paired Aura/CPython and Aura/Rust ratios for protocol workloads. The summary links the exact raw report
by SHA-256.

`--allow-competing-processes` exists only for explicitly non-contractual
diagnostic runs. Do not use it for release evidence.

## Rust comparison lane (0.3.4 foundations)

The runner builds pinned Rust 1.95.0 references from `benchmarks/rust_baselines/`
with `--release --locked`, fat LTO, and one codegen unit. Report schema is now 2.
It records source/lockfile and binary SHA-256 identities and paired Aura/Rust
samples. Protocol smoke checks are not performance evidence.

The 7 September 2026 runs are contractual at corrected after
`4e1e48c81bae6c62a54af763a4046901820038ec` and before
`ddeaddf74301322fc96d8c09742ea12faa1dd8c3`. Both include the identical two casts
from int64 range values to int32 task parameters; initial unchanged inputs
failed at both bases. Compiler/runner sources remain unchanged. The
[Git bundle](https://github.com/johnolafenwa/Aura/blob/main/work/2026-09-07-foundations-measurements/approved/approved-measurement-commits.bundle) preserves both
exact commits; its prerequisites are merge `50531b4` and tag `d3cc6b9`.

Both reports have 11 rotating pairs, excluded warmups, exact protocol/checksum
validation, three empty inventories, clean detached sources and input hash
rechecks. No override or Rust exclusion was used. The same Mac14,9/M2 Pro boot
and Xcode CPython 3.9.6 interpreter were used; every CPython control drifted by
less than 5%, so no repeat was required.

| Exact protocol workload | Aura median | CPython median | Rust median | Aura / CPython | Aura / Rust |
| --- | ---: | ---: | ---: | ---: | ---: |
| Naive recursive fib(30) | 89.359292 ms | 158.321541 ms | 3.012292 ms | 0.564417 | 29.664884 |
| Create and join 10,000 tasks | 98.692417 ms | 51.786208 ms | 3.395167 ms | 1.905766 | 29.068501 |
| 20-client delayed loopback TCP fan-out | 104.268375 ms | 108.793000 ms | 104.597250 ms | 0.958411 | 0.996856 |
| 16-cycle retrying HTTP worker | 428.814667 ms | 522.391625 ms | 472.607250 ms | 0.820868 | 0.907338 |

Ratios are ratios of medians. The [Performance chapter](../../docs/manual/performance.md)
records before/after effects, V6 medians, startup-adjustment exclusions, and
all controls. After raw SHA-256: `a2a31328af601c32783519d9658ab164d9b415046b03b5b7856b6e09c4c79441`.
Before raw SHA-256: `16aead5c0c9fe73ff2155e66b74edf982c8ec81958ab7ae3b4b2a6ae82498ec7`.
The [manifest](https://github.com/johnolafenwa/Aura/blob/main/work/2026-09-07-foundations-measurements/SHA256SUMS) covers both raw/summary reports and source records.
Run the command above in the corrected after checkout; run the before checkout's
own runner with label `foundations-before` and distinct output paths.

See [the Rust baseline contract](../rust_baselines/README.md) for exact workload,
allocation, arithmetic, scheduling, and protocol equivalence. The integer-loop
lane retains its whole-process checksum protocol; other lanes use READY/GO/DONE.

## Item 7 profiling and task-cost follow-up

`AURA_NATIVE_KEEP_SYMBOLS=1` keeps user-binary symbols and participates in the
native cache identity. Build the profiling compiler/runtime with
`CARGO_PROFILE_RELEASE_STRIP=none`, then build `fib30.au` with symbols kept.
The [per-call attribution](../../architecture_docs/15-backend-boundary.md#per-call-cost-attribution)
records eleven xctrace traces, all eight categories and the diagnostic estimate
of 33.85854 ns per logical call. A safe storage experiment improved 11.10%, below
the 20% adoption gate, and was reverted. No call-overhead change ships.

The [Task stack reuse proposal](../../architecture_docs/decisions/0032-guarded-lightweight-task-stacks.md#future-extension-task-stack-reuse)
scopes Batch 8 against the observed 9.869 microseconds per Aura task and 0.340
microseconds per tokio task. It does not assign the entire gap to allocation;
no scheduler code changes in item 7. The CLI now checks every Aura input under
`benchmarks/`, including the scalable-runtime inputs.
