# Batch 7a Step 8: Copy-On-Write `dict` And `set`

## Goal

Stop deep-copying a `dict` or `set` each time a program reads one out of a
list, binds one in a `for` loop, or copies a list of them. Section G of the
[Batch 7a design checkpoint](../architecture_docs/20-batch-7a-design-checkpoint.md)
records the design.

## Work Completed

- `IndexedEntries` keeps its entries, and its index once it has one, behind
  an `Arc`. A clone shares them. Every write goes through `Arc::make_mut`,
  so the first write through a shared handle copies the storage and value
  semantics do not change. `clear` on shared storage drops its handle
  instead of copying.
- `try_clone_array_containing_value` shares a `dict` or `set` whose key and
  value types cannot hold an `Array`, at the top level and inside lists,
  tuples, instances, and variants. Unknown and generic types keep the
  element-by-element copy, because an `Array` buffer must be copied.
- `IndexedEntries` is now 16 bytes, smaller than a `Vec`.

## Verification

- `clones_share_storage_until_a_write` covers insert, overwrite, removal,
  `pop`, `value_at_mut`, `clear`, `into_vec`, and `try_reserve` on shared
  storage, below and above the index threshold.
- `array_free_dicts_and_sets_clone_by_sharing` checks that array-free
  collections share and that `Unknown`, generic, `Array`, and class value
  types are still copied.
- `cargo test -p aura-runtime --lib`: 691 passed.
- `cargo test -p aura --test backend_parity --test native_codegen_acceptance
  --test representation_stats`, `cargo test -p aura-compiler --test fixtures`,
  and `cargo test -p aura-compiler --lib`. The only failure is
  `package_io_helpers_cover_local_error_paths`, which fails because the
  container runs tests as root.
- Callgrind: `record_field_reads` 2.64 G to 1.54 G instructions, AuraDB
  `upd.au` 834 M to 407 M.
- AuraDB, same container: `update` by id 8,095 µs to 660 µs per operation,
  `delete` by id 31,630 µs to 10,980 µs, and `open` with 5,000 documents
  683 ms to 284 ms. The raw report is
  `work/2026-10-18-batch-7a-measurements/auradb-copy-on-write.txt`.

## Follow-Up

- `list` copies still copy every element. `VecValue.elements` is a public
  `Vec` with about 300 direct uses, so sharing it is a separate change.
- Type tags on `json.Value` and `Lookup` enums are now 20% of
  `record_field_reads`.
- Lock removal still needs the retagging design change.
