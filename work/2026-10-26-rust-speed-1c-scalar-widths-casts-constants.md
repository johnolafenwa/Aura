# Rust-speed plan step 1c: every scalar width, inline casts, constant module constants

Plan: https://claude.ai/artifact/Jx47RTWBdr1tNdCcNcaLwC (Phase 1, step 1c).

## What changed

- The runtime keeps a list of `int8`, `int16`, `int32`, `uint8`, `uint16`,
  `uint32`, or `uint64` values unboxed, at the element's own width, next to
  the existing `int64`, `float64`, and `bool` forms. A `list[float32]` uses
  the `float64` form, since a `float32` widens to `float64` exactly. Each
  form has its own list access code (6-12), so native code reads, writes,
  and appends elements of every width in place, with sign or zero
  extension on reads and a range check before a narrow store.
- `bytes` helpers (`str.from_bytes`, file and socket writes, `to_bytes`)
  read and build `list[uint8]` storage as a byte slice, without a box per
  byte.
- Native code emits `as` casts between scalar types inline: integer to
  integer with one range check, integer to float with an exactness check
  (24 bits for `float32`, 53 for `float64`) on a cold path, and float to
  integer with a range check that also catches NaN and infinities. A
  failing cast calls the runtime only to build the same error message the
  interpreter gives.
- Integer `&`, `|`, `^`, `~`, `<<`, and `>>` on scalars run inline for
  every width. A shift whose count is in range, and a left shift that
  loses no bits, runs inline; any other shift calls the runtime, which
  reports the error.
- MIR can compare two different integer kinds: a literal in a
  `while ... and` condition reaches the comparison as an `int64`. Now that
  narrow list elements are scalars, native code compares the exact values
  inline (a `uint64` against a signed value goes through the runtime).
  Fixture `integer_comparisons_across_kinds.au`.
- MIR folds a module constant whose value is a scalar literal into the
  places that read it, so native code no longer calls
  `aura_direct_module_constant` for it.
- Runtime list storage got cheaper in the paths the new forms touch:
  `Iter::nth` skips in O(1), and an empty `Values` list keeps its capacity
  when a boxed value is appended.

## Gate

No `aura_direct_cast_value_at` call remains in hashing, markdown_render,
lzw_compress, or spreadsheet. `aura_direct_module_constant` calls drop from
70,041 / 296 / 0 / 17,809 to 3 / 4 / 0 / 10, all made once at startup for
non-scalar constants.

## Measurements (callgrind, Linux container, check size, vs step 1b)

| Program | 1b (M instr) | 1c (M instr) | Change | 1c vs Rust |
|---|---:|---:|---:|---:|
| astar_pathfinding | 26.0 | 26.2 | +0.8% | 23.9x |
| bank_ledger | 45.2 | 43.4 | -4.0% | 12.4x |
| binary_trees | 15.2 | 15.2 | +0.2% | 10.3x |
| bytecode_vm | 50.7 | 51.7 | +1.9% | 19.5x |
| csv_aggregate | 102.4 | 103.5 | +1.0% | 12.8x |
| docdb | 83.0 | 83.1 | +0.2% | 15.0x |
| file_io | 69.8 | 30.6 | -56.2% | 14.1x |
| graph_paths | 570.3 | 573.5 | +0.6% | 210.6x |
| hashing | 385.4 | 47.2 | -87.8% | 30.4x |
| http_api | 32.6 | 32.5 | -0.5% | 5.1x |
| image_filters | 24.9 | 23.9 | -3.8% | 44.1x |
| interpreter | 371.8 | 367.2 | -1.3% | 46.0x |
| json_roundtrip | 642.0 | 642.1 | +0.0% | 6.5x |
| kv_server | 84.3 | 86.0 | +2.0% | 10.3x |
| lru_cache | 449.9 | 428.6 | -4.7% | 51.5x |
| lzw_compress | 16.6 | 7.5 | -54.8% | 9.1x |
| markdown_render | 81.0 | 15.1 | -81.4% | 15.9x |
| mini_browser | 2,689.3 | 1,403.2 | -47.8% | 43.3x |
| nbody | 35.4 | 35.5 | +0.3% | 43.6x |
| order_book | 127.9 | 118.3 | -7.6% | 154.3x |
| pubsub_broker | 150.4 | 151.3 | +0.6% | 14.3x |
| raytracer | 470.1 | 461.4 | -1.8% | 410.9x |
| regex_match | 2,174.9 | 1,942.8 | -10.7% | 80.0x |
| sort_records | 575.5 | 542.0 | -5.8% | 130.3x |
| spreadsheet | 98.4 | 55.1 | -44.0% | 32.2x |
| sql_engine | 4,757.6 | 2,971.7 | -37.5% | 62.8x |
| task_pool | 66.5 | 66.5 | +0.1% | 12.1x |
| template_render | 42.1 | 39.6 | -5.7% | 10.0x |
| text_diff | 13.0 | 11.3 | -13.4% | 8.9x |
| word_freq | 75.9 | 76.4 | +0.6% | 15.6x |

No program grows by more than 3%. bytecode_vm's +1.9% comes from the list
view refresh after each runtime list write: with eleven list forms the
helper that computes the view no longer inlined on its own, so it is now
forced inline and its form maps to the access code without a table, which
brought bytecode_vm down from +3.2%. kv_server's +2.0% is mostly run-to-run
noise from its threads: a second pair of runs gives 85.4M and 85.8M
(+0.4%, the same view refresh). mini_browser's drop comes from
`Iter::nth`: its slices skip to the start through the list iterator, which
used to step one element at a time (step 1e makes slices O(len) for good).
