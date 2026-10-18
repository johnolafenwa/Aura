# Batch 7a Step 9: Copy-On-Write Lists

## Goal

Stop copying every element when a program copies a `list`, reads one out of
another collection, or binds one in a loop. Section H of the
[Batch 7a design checkpoint](../architecture_docs/20-batch-7a-design-checkpoint.md)
records the design. John's 2026-10-18 decision card offered lists first,
lock removal first, or closing Batch 7a. Work followed the recommended
option, lists first, and John confirmed it on the card.

## Work Completed

- `SharedVec<T>` in `collection_index.rs` holds a `Vec` behind an `Arc`.
  It derefs to `Vec` for reads, and `DerefMut` calls `Arc::make_mut`, so
  existing writes compile unchanged and copy shared storage first.
- `VecValue.elements` is a `SharedVec<Value>`. Construction sites in the
  runtime, the MIR interpreter, and their tests add `.into()`.
- `try_clone_array_containing_value` shares a `list` whose element type
  cannot hold an `Array`. The iterative drop skips shared lists.
- The Collections chapter states that a copy shares storage until a write.

## Verification

- `shared_vecs_copy_on_the_first_write` covers a write to a clone,
  `iter_mut`, `into_vec`, collection, and NaN equality.
- `array_free_dicts_and_sets_clone_by_sharing` covers a list of documents
  and a list with an unknown element type.
- `cargo test -p aura-runtime --lib`: 692 passed.
- `cargo test -p aura --test backend_parity --test native_codegen_acceptance
  --test representation_stats`, `cargo test -p aura-compiler --test fixtures`,
  `cargo test -p aura-compiler --lib`, and `npm run check:clippy`.
- AuraDB in the same container: `update` by id 660 µs to 25 µs per
  operation. The raw report is
  `work/2026-10-18-batch-7a-measurements/auradb-shared-lists.txt`.

## Follow-Up

- AuraDB `delete` by id spends its time in AuraDB's own
  `rebuild_positions`, which recomputes every document's key per delete.
- Lock removal still needs the retagging design change.
