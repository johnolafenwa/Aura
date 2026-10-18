# 2026-10-17 Batch 7a Step 1: Hash-Indexed dict And set

## Goal

Start roadmap Batch 7a. Record where native programs spend their time, plan
the value-representation work, and replace the linear `dict` and `set`
storage with a hash index that keeps insertion order.

## Work Completed

- **Design checkpoint.** `architecture_docs/20-batch-7a-design-checkpoint.md`
  records baselines, a callgrind profile, and a six-step plan. Type tagging
  costs about half of every loop's instructions, which makes it the second
  step.
- **Benchmarks.** `benchmarks/runtime_values/` holds five programs and a
  Python reference. `scripts/bench-runtime-values.py` runs them, and
  `scripts/test_bench_runtime_values.py` tests the script.
- **Hash index.** `crates/aura-runtime/src/collection_index.rs` adds
  `IndexedEntries`: a `Vec` in insertion order with a `hashbrown::HashTable`
  of `(hash, position)` above eight entries. `MapValue.entries` and
  `SetValue.elements` use it on both backends.
- **Hash agreement.** `hash_value` agrees with `Value` equality: unions hash
  as their payload, integers by numeric value, and `-0.0` as `0.0`. Sets,
  dictionaries, and resources hash by kind. The walk is iterative and visits
  at most 64 nodes. `IntegerValue` gains a `Hash` impl.
- **Borrowed keys.** The direct runtime's dictionary and set entries read
  their key or member argument in place instead of cloning it.
- **Equality.** Dictionary and set equality look each entry up through the
  index instead of a nested scan. Union equality compares payloads without
  cloning them.

## Verification

- `cargo test -p aura-runtime`: nine new `collection_index` unit tests.
- New run fixture `run-pass/hashed_dict_and_set_order.au` covers order,
  removal, reinsertion, equality in another order, and set operations above
  the index threshold. Both backends print the same output.
- Micro-benchmarks, release build, Linux x86-64 container:

  | Program | Before | After | Python 3.13 |
  | --- | ---: | ---: | ---: |
  | `dict_build_get` | 18.47 s | 0.71 s | 0.026 s |
  | `set_insert_contains` | 24.69 s | 0.062 s | 0.007 s |

- AuraDB rows did not move. Their time goes to document clones and type
  tagging, which later steps address.

## Follow-Up

- Steps 2 to 6 of the checkpoint.
- Removal stays linear to keep order. A tombstone scheme could make it
  constant time if a workload needs it.
