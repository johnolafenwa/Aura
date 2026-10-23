# Batch 7 Step 8g Measurements

Raw reports behind the Step 8 Results section of the
[Batch 7 design checkpoint](../../architecture_docs/22-batch-7-design-checkpoint.md).
They compare `840f91ee`, the `v0.6.0-preview` release that starts phase 2,
with `ff35ee70`, the end of step 8f. Both ran on one shared four-core Linux
x86-64 container with an Intel Xeon at 2.80 GHz, Rust 1.95.0, and Python
3.13. The CPU has no SHA extensions. The step 1 reports came from another
host, so their build times do not compare with these.

Each Aura build is a release build of `aura` and `aura-runtime`. The base
build used its own worktree and Cargo target directory. Every Aura build
and run used its own `AURA_CACHE_DIR`.

| File | Command |
| --- | --- |
| `compile-time-base.json` | `python3 scripts/bench-compile-time.py --repetitions 5 --aura BASE/aura --runtime BASE/libaura_runtime.a --raw-json ...` |
| `compile-time-head.json` | `python3 scripts/bench-compile-time.py --repetitions 5 --raw-json ...` |
| `phases.json` | `python3 phases.py BASE/aura BASE/libaura_runtime.a target/release/aura target/release/libaura_runtime.a WORK phases.json 11` |
| `runtime.json` | `python3 runtime_bench.py BASE/aura target/release/aura RUST_RELEASE_DIR WORK runtime.json 31` |
| `callgrind-phases.txt` | `valgrind --tool=callgrind aura build --backend direct ...`, then `callgrind_annotate --inclusive=yes` and `callgrind_phases.py` |
| `emitter_inventory.py` | Prints the line count of each emitter group |

- `phases.py` alternates base and head builds. It times `aura check`, a
  cold `aura build`, and the final `cc` link through a wrapper. The rest of
  the build is lowering, the MIR passes, the runtime archive hash, and
  emission.
- `runtime_bench.py` alternates base, head, and the Rust reference. The
  collection programs time the whole process. `fib30` and `tasks_10000`
  time GO to DONE.
- The callgrind profiles used separate builds of both commits with symbols
  kept, LTO off, and 16 codegen units, so each phase has its own frame.
  Their counts split the work. They are not the release binaries' counts.
