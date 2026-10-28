# Rust Speed Step 2k: Dead Boxed Paths And The Phase 2 Measurement

## Goal

Step 2k closes Phase 2 of the Rust-speed plan: delete the boxed paths that
typed layouts replaced in the emitter and runtime, and re-measure. Its gate
is "Every compute program within its final target."

## What Was Dead

A search of the runtime helper table for helpers no emitter code or
runtime op names, and of the runtime for public functions that only tests
call, found five:

- `aura_direct_vec_pop_in_place`, `aura_direct_vec_remove_in_place`, and
  `aura_direct_set_remove_in_place`: the boxed `pop`, `remove`, and set
  `remove` that returned a `Lookup` box. The shared collection operation
  (`aura_direct_collection_operation`) replaced them, and typed record
  lists pop in place (2j). Their runtime tests now call the collection
  operation, and the tests of their own error messages are gone.
- `TypedPayloads::unshare_boxes`, which copied shared payload boxes
  before a variant crossed to another thread; nothing calls it since
  typed values cross tasks by a move (2i).
- `list_elements::int64_bits`, left from before unboxed scalar lists.

Every other boxed path still has callers. Generic classes, classes that
hold themselves, and values that reach host calls, FFI, and printing keep
runtime values, and `aura run` keeps them throughout.

## Measurements

Wall time, the 30 cross-language programs at the bench size, three
repetitions, a Linux x86-64 container with 4 vCPUs, release builds. End of
phase 1 is `202046db` (the 0.8.0 measurement); end of phase 2 is step 2j
(#174). The helpers deleted here are never called, so the programs' code
is the same.

| Measure | End of phase 1 | End of phase 2 |
| --- | ---: | ---: |
| All 30 programs, wall time | 147 s | 48.5 s |
| Median Aura / Rust, wall time | 16x | 8.2x |
| Median Aura / Python 3.13, wall time | 2.7x | 0.71x |
| Programs faster than Python | 9 | 19 |
| All 30 programs, instructions at the check size | 9,323M | 3,442M |

| Program | End of phase 1 | End of phase 2 | Aura / Rust | Target |
| --- | ---: | ---: | ---: | ---: |
| `astar_pathfinding` | 875 ms | 436 ms | 4.9x | 1.5x |
| `binary_trees` | 2,715 ms | 689 ms | 11.2x | 1.5x |
| `bytecode_vm` | 1,057 ms | 90 ms | 1.7x | 1.5x |
| `graph_paths` | 37,183 ms | 3,053 ms | 10.7x | 1.5x |
| `hashing` | 67 ms | 68 ms | 1.5x | 1.5x |
| `image_filters` | 4,657 ms | 129 ms | 2.0x | 1.5x |
| `lzw_compress` | 376 ms | 379 ms | 8.4x | 1.5x |
| `nbody` | 422 ms | 113 ms | 6.1x | 1.5x |
| `order_book` | 626 ms | 52 ms | 7.3x | 1.5x |
| `raytracer` | 13,479 ms | 478 ms | 14.4x | 1.5x |
| `task_pool` | 2,812 ms | 2,319 ms | 7.9x | 1.5x |
| `text_diff` | 223 ms | 218 ms | 2.4x | 1.5x |
| `bank_ledger` | 892 ms | 297 ms | 6.1x | 2x |
| `csv_aggregate` | 1,398 ms | 1,607 ms | 10.8x | 2x |
| `docdb` | 3,360 ms | 3,046 ms | 7.1x | 2x |
| `interpreter` | 29,407 ms | 4,212 ms | 11.2x | 2x |
| `json_roundtrip` | 1,823 ms | 2,292 ms | 15.2x | 2x |
| `lru_cache` | 844 ms | 241 ms | 9.0x | 2x |
| `markdown_render` | 655 ms | 603 ms | 9.1x | 2x |
| `mini_browser` | 4,694 ms | 2,662 ms | 11.3x | 2x |
| `regex_match` | 9,323 ms | 1,363 ms | 14.8x | 2x |
| `sort_records` | 1,407 ms | 733 ms | 7.3x | 2x |
| `spreadsheet` | 1,583 ms | 698 ms | 18.6x | 2x |
| `sql_engine` | 7,497 ms | 4,431 ms | 97.9x | 2x |
| `template_render` | 4,251 ms | 3,761 ms | 11.0x | 2x |
| `word_freq` | 1,823 ms | 1,772 ms | 9.3x | 2x |
| `file_io` | 2,799 ms | 2,892 ms | 3.0x | 1.25x |
| `http_api` | 1,179 ms | 1,046 ms | 3.0x | 1.25x |
| `kv_server` | 3,280 ms | 3,276 ms | 1.6x | 1.25x |
| `pubsub_broker` | 6,464 ms | 5,576 ms | 2.5x | 1.25x |

Wall times on the container vary by up to a quarter from run to run:
`json_roundtrip` took 2,292 ms in this run and 1,708 ms and 1,768 ms in
two runs of five repetitions each on the 2i and 2j builds, and
`csv_aggregate` 1,607 ms against 1,447 ms and 1,410 ms. Both run fewer
instructions than at the end of phase 1 (641M to 570M, 66M to 64M at the
check size), so neither slowed down.

## Step 2k gate

"Every compute program within its final target." Missed. Of the 12
programs with a 1.5x target only `hashing` (1.5x) meets it; `bytecode_vm`
(1.7x) and `image_filters` (2.0x) are close. None of the 14 programs with
a 2x target and none of the 4 with a 1.25x target meets it; `kv_server`
(1.6x) is closest.

Where the remaining time goes, from the step profiles:

- Calls: each function call still pushes and pops a frame record, and
  boxed arguments and results are retained and released (`raytracer`,
  `binary_trees`, `interpreter`).
- Nested lists stay boxed, so reading an element of a list of lists walks
  an element path through the runtime (`graph_paths`, `spreadsheet`,
  `astar_pathfinding`).
- Strings, dicts, and JSON (`sql_engine`, `regex_match`, `json_roundtrip`,
  `template_render`, `csv_aggregate`, `docdb`), which Phase 3 covers.
- The queue hand-off for `task_pool` and the I/O programs, which Phase 4
  covers.

## Verification

- Runtime, compiler library, and helper table tests; `cargo check` of the
  coverage-only FFI tests with `--cfg coverage`; `cargo fmt --check`,
  hygiene, and clippy.
