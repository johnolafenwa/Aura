# Batch 7a Step 2: Interned Runtime Type Tags

## Goal

Make `aura_direct_tag_value_type` cheap in the direct backend without changing
observable behavior. Callgrind showed it at 45-58% of all instructions in
list, string, and record loops.

## Work Completed

- Added `RuntimeTypeTag` in `crates/aura-runtime/src/native_runtime.rs`. It
  holds the descriptor as `Arc<str>`, the parsed `Type`, and the integer kind.
  Each descriptor is decoded and parsed once.
- Tags are interned in two layers. A per-thread, 64-slot direct-mapped table
  is keyed by the descriptor's address and length and checks the bytes on a
  hit. A process-wide `Mutex<HashMap>` keyed by the bytes backs it. The shared
  registry clears at 4096 entries; stored values keep their own `Arc`s.
- `OpaqueValue.runtime_type_name` is `RwLock<Option<Arc<str>>>`. Retagging
  with the same descriptor skips the store.
- Applying a tag compares before cloning element, key, value, tuple, and
  function types. Instance tag fields are only rewritten when they differ.
- Enum variant names are rebuilt only when the embedded descriptor differs,
  with a pre-sized string instead of `format!`.
- The MIR interpreter is unchanged.

## Verification

- New test `direct_runtime_type_tags_are_interned_and_retagging_is_idempotent`
  in `native_runtime_tests.rs`.
- `cargo test -p aura-runtime`: pass.
- `cargo test -p aura-compiler --lib`: pass except the known root-only
  `package_io_helpers_cover_local_error_paths`.
- `cargo test -p aura --test cli --test representation_stats --test
  backend_parity --test native_codegen_acceptance`: pass.
- `cargo fmt --all --check` and `npm run check:clippy` command: pass.

Release timings, direct backend, before and after:

| Benchmark | Before | After |
| --- | --- | --- |
| int_list_sum | 1.97-2.15 s | 1.04-1.06 s |
| string_list_scan | 1.52-1.62 s | 0.87-0.88 s |
| record_field_reads | 0.80-0.82 s | 0.61-0.64 s |
| set_insert_contains | 24.7 s | 25.4-26.1 s |
| dict_build_get | 18.8 s | 18.2-19.0 s |

Callgrind instruction totals:

| Benchmark | Before | After |
| --- | --- | --- |
| int_list_sum | 23.79 G | 11.50 G |
| string_list_scan | 17.71 G | 9.52 G |
| record_field_reads | 8.21 G | 5.24 G |

`aura_direct_tag_value_type` inclusive cost on int_list_sum fell from 13.88 G
to 1.86 G instructions.

## Follow-Up

- Set and dict benchmarks are dominated by other costs; tagging was not their
  bottleneck.
- Iterator `Option` variants are still allocated fresh and renamed on every
  tag. Avoiding the enum name allocation needs a different variant name
  representation.
