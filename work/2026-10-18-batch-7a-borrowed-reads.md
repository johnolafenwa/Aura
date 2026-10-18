# Batch 7a Step 3: Borrowed Reads

## Goal

Stop the direct runtime from cloning operands that it only reads, as
section C of the
[Batch 7a checkpoint](../architecture_docs/20-batch-7a-design-checkpoint.md)
plans.

## Work Completed

- `aura_direct_binary_value_at` compares both operands in place through
  `compare_value_refs`. The same value on both sides is read once.
- `aura_direct_variant_payload` clones only the requested payload instead
  of the whole variant.
- List, `dict`, and `set` inserts infer the element type only while it is
  still unknown, instead of on every insert.
- Shared `Arc<Type>` element types are dropped from the plan. After step 2,
  cloning type trees costs about 1% of the AuraDB `update` profile.

## Verification

- New test `direct_comparisons_borrow_both_operands`. It fails on the
  step 2 runtime and passes here.
- `cargo test -p aura-runtime`, `cargo fmt --all --check`, and
  `npm run check:clippy` pass.

Release timings, direct backend, after step 2 and after step 3:

| Benchmark | Step 2 | Step 3 |
| --- | ---: | ---: |
| `int_list_sum` | 1.05 s | 0.88 s |
| `string_list_scan` | 0.88 s | 0.77 s |
| `record_field_reads` | 0.62 s | 0.46 s |

## Follow-Up

- Step 4 removes the per-element `Lookup` value from list loops and makes
  owned list loops linear.
