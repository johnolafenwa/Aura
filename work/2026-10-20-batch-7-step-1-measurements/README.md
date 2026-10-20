# Batch 7 Step 1 Measurements

Raw reports behind section E of the
[Batch 7 design checkpoint](../../architecture_docs/22-batch-7-design-checkpoint.md).
They ran on a shared four-core Linux x86-64 container at commits `7798aec`
and `e272e1c`, which change no compiler or runtime source after `aa52bfc`,
with Rust 1.95.0 and Python 3.13. The host was not quiet, so every number is
diagnostic-grade.

| File | Command |
| --- | --- |
| `runtime-values.json` | `python3 scripts/bench-runtime-values.py --repetitions 5 --raw-json ...` |
| `compile-time.json` | `python3 scripts/bench-compile-time.py --repetitions 5 --raw-json ...` |
| `protocol.json` | `python3 work/2026-10-20-batch-7-step-1-measurements/protocol_bench.py RUST_RELEASE_DIR ...` |

`scripts/bench-release-performance.py` produces contractual evidence only on
the macOS baseline host, so `protocol_bench.py` takes 11 diagnostic GO to DONE
medians of the same sources here.
