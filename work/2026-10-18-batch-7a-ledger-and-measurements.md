# Batch 7a Step 5: Ownership Ledger And Final Measurements

## Goal

Cut the cost of the per-task owned-value ledger, and record the Batch 7a
results in the Performance chapter.

## Work Completed

- The ledger is a `HashMap` keyed by value address with a multiplicative
  hasher, instead of a `BTreeMap`. A leaked-value report sorts its entries,
  so its output stays deterministic.
- Cloning a scalar or a string skips the iterative clone frames.
- `docs/manual/performance.md` has a "Collections And Value Reads"
  section with the micro-benchmarks and AuraDB. The raw reports are in
  `work/2026-10-18-batch-7a-measurements/`.
- The roadmap row, checkpoint, and task board record what is done and what
  remains.

## Verification

- New tests `owned_value_ledger_hashes_spread_aligned_addresses` and
  `leaf_values_clone_without_frames`.
- `cargo test -p aura-runtime`, `cargo fmt --all --check`,
  `npm run check:clippy`, and `npm run check:doc-examples`.
- Callgrind on `int_list_sum`: 2.75 G instructions before this step and
  2.42 G after.

## Follow-Up

- Unboxed scalars and strings need a change to the direct backend's value
  ABI, where every value is a pointer to a boxed `OpaqueValue`.
- Removing the per-value lock needs a guarantee that no shared value is
  retagged, or type tags fixed when a value is built.
- AuraDB `update` and `delete` copy every document. Copy-on-write values
  would address them.
