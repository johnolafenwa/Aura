# Direct integer loop baseline

This is the V6 workload: a ten-million iteration counter loop, built with the
direct native backend, at `int32` and `int64` width. Both widths are kept here
so the relationship between them stays visible instead of being summarized
away. `startup.au` is a silent, empty direct program with the same runtime
entry path. The scalable-runtime runner pairs it with both loops so fixed
process startup is not mistaken for loop cost.

Run it with:

```bash
npm run bench:direct-integer-loops
```

The reported figure is the best of several runs, which is the right statistic
for a CPU-bound loop: the minimum is the run least disturbed by unrelated
system activity.

The contractual scalable-runtime runner also reports a startup split:

```bash
npm run bench:scalable-runtime -- \
  --label v6-startup-split \
  --aura target/release/aura \
  --json /tmp/aurora-v6-startup-split.json
```

It rotates the startup, `int32`, and `int64` process order on each repetition
and publishes both whole-process summaries and paired
`whole process - startup` loop estimates under
`benchmarks.v6.startup_vs_loop`.

This is report schema version 4. Schema version 3 identified each V6 run with
a `width` field and contained only the two integer loops. Version 4 identifies
each run with `workload` and adds the `startup` workload and split summary.

## Recorded baseline

Development workstation, Apple silicon, debug `aura` driving a release-quality
Cranelift build, seven repeats.

| Width | Before the V6 fix | After the V6 fix |
| --- | --- | --- |
| `int32` | 0.0697s | 0.0327s |
| `int64` | 0.0115s | 0.0111s |
| `int32` / `int64` | 6.05x | 2.95x |

`int64` is unchanged, as expected: the fix touches only the narrow-width range
check. See `work/2026-07-25-v6-direct-int32-loops.md` for the diagnosis and for
what still separates the two widths.

## Reactor-era baseline

The accepted post-Phase-5 whole-process baseline on the clean Mac14,9 host is
`37.436334 ms` for `int32` and `15.005584 ms` for `int64`, using the median of
five measured repetitions after warmup. These are respectively `14.36%` and
`46.42%` above the Batch-2 medians of `32.734250 ms` and `10.248625 ms`, so
they do not satisfy the attempted “within 10%” restoration target.

The split measures a fixed runtime-entry component separately from the loop.
One 21-repetition run measured a `7.679583 ms` startup median,
whole-process medians of `49.391916 ms` / `18.875542 ms`, and paired
loop-estimate medians of `41.746208 ms` / `11.123916 ms` for
`int32` / `int64`. That run occurred in a dirty checkout during concurrent
Batch-5 work. It proves that the maintained split works, but it does not
establish the complete cause of the regression or replace the clean baseline:
in particular, its `41.746208 ms` `int32` loop estimate does not reproduce the
Batch-2 whole-process median.

A separate `AURORA_WORKERS=1` diagnostic measured a `7.851334 ms` startup
median. This gives no evidence that selecting one worker reduces the measured
startup component; it does not by itself prove which initialization work
causes the historical gap. The direct root scheduler remains the boundary
that owns task cleanup, traps, cancellation, and cooperative scheduling, so it
is not bypassed for scalar programs without evidence for a safe replacement.

Under the alternate disposition permitted by B5.0-d, the clean Phase-5.10
`37.436334 ms` / `15.005584 ms` whole-process pair is accepted as the
reactor-era baseline. This is a baseline decision, not a claim that startup
explains the entire regression. The startup split remains in the maintained
runner so future loop work can compare loop estimates separately from process
entry cost.
