# Batch 7 Step 9c Measurements

Raw reports behind
[the step 9c note](../2026-10-23-batch-7-step-9c-llvm-tuning.md). They ran
on one shared four-core Linux x86-64 container with Rust 1.95.0, Python
3.13, and LLVM 22.1.2.

One release `aura` built with the `llvm` feature measured every setting. It
carried the patch in `pipeline.diff`, which reads the pass pipeline from
`AURA_LLVM_PIPELINE_MEASURE`, so the LLVM levels differ only in the
pipeline. The committed code fixes the pipeline at `default<O3>`. The
Cranelift lane sets `AURA_NATIVE_CODEGEN=cranelift`; the feature does not
change what Cranelift emits. Builds used an installed layout, `bin/aura`
and `lib/aura`, staged by `scripts/bench-compile-time.py`. Every Aura build
had its own `AURA_CACHE_DIR`.

| File | Command |
| --- | --- |
| `runtime.json` | `python3 runtime_bench.py PREFIX/bin/aura RUST_RELEASE_DIR WORK runtime.json 51` |
| `callgrind.json` | `python3 callgrind_counts.py RUST_RELEASE_DIR WORK callgrind.json` |
| `compile.json` | `python3 compile_bench.py PREFIX/bin/aura WORK compile.json 11` |
| `compile-time-*.json` | `python3 scripts/bench-compile-time.py --allow-dirty --repetitions 5 --aura target-llvm/release/aura --runtime target-llvm/release/libaura_runtime.a --raw-json ...`, once per setting |
| `native-binaries-*.json` | `python3 scripts/check-native-binaries.py --aura PREFIX/bin/aura`, once per setting |

- `runtime_bench.py` builds the six phase 2 targets with each setting and
  runs them with the Rust reference, 51 rounds in a rotating order.
  `fib30` times GO to DONE. The rest time the whole process. It records
  each executable's SHA-256, so identical builds show.
- `callgrind_counts.py` counts the instructions of one run of each of those
  executables, startup included.
- `compile_bench.py` times cold `aura build` runs of hello world, the
  reference agent, and the `json_codec_derived` fixture, 11 rounds in a
  rotating order. It then counts the `aura` process's instructions with
  callgrind. The count leaves out the `cc` link, which runs in a child.
- The Rust references are release builds of `benchmarks/rust_baselines`
  from 2026-10-20.
