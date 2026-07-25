# Direct integer loop baseline

This is the V6 workload: a ten-million iteration counter loop, built with the
direct native backend, at `int32` and `int64` width. Both widths are kept here
so the relationship between them stays visible instead of being summarized
away.

Run it with:

```bash
npm run bench:direct-integer-loops
```

The reported figure is the best of several runs, which is the right statistic
for a CPU-bound loop: the minimum is the run least disturbed by unrelated
system activity.

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
