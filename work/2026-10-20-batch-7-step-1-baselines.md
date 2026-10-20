# Batch 7 Step 1: Rust Baselines, Build Times, And Proposed Targets

## Goal

Add the evidence the Batch 7 checkpoint asked for before numeric targets:
Rust counterparts for the five runtime value programs and a measurement of
`aura build` times. Then propose targets for John to ratify.

## Work Completed

- `benchmarks/rust_baselines` gains `dict_build_get`, `set_insert_contains`,
  `int_list_sum`, `string_list_scan`, and `record_field_reads`. They use std
  collections and checked arithmetic and print the same result as the Aura
  programs. `scripts/rust_baselines.py` checks their output.
- `scripts/bench-runtime-values.py` builds the Rust references and reports
  Aura against Rust beside Python. Its report is schema 2.
- `scripts/bench-compile-time.py` stages an installed runtime layout and
  times cold, warm, and edited builds, `aura check` alone, and the final link.
- Section E of the checkpoint records the results and the proposed targets.
  Raw reports are in `2026-10-20-batch-7-step-1-measurements/`.

## Findings

- Aura is 15x to 45x Rust on the collection programs and 32.8x on `fib(30)`.
  Task creation is 61x. TCP fan-out is at parity.
- The front end is under a tenth of a build. Cranelift code generation and
  the `cc` link take the rest, and the link alone is about 0.2 s.
- `aura build` does not reuse the native cache, so an unchanged rebuild costs
  as much as a cold one. Only `aura run` reuses cached executables.

## Verification

- `python3 -m unittest scripts/test_rust_baselines.py
  scripts/test_bench_runtime_values.py scripts/test_bench_compile_time.py`
- `python3 scripts/rust_baselines.py`: every reference passes its output
  contract.

## Follow-Up

- John ratifies or changes the targets before step 2.
- Step 7 re-measures on the macOS baseline host with the contractual runner.
- Reusing the native cache in `aura build` would make unchanged rebuilds
  nearly free. It is outside the ratified plan.
