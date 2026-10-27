# Rust Speed Step 2h Part 3: Collection Writes Change The Collection In Place

## Goal

Step 2h of Phase 2 of the Rust-speed plan moves ownership off boxes, and
its gate asks retains and releases per program to drop at least 90%.
After part 2 (`2026-10-27-rust-speed-2h-mutable-arguments.md`), every
retain left in `astar_pathfinding` came from `heap.swap` in its heap
helpers, and most of those in `graph_paths`, `order_book`, and
`word_freq` came from collection writes too.

## Cause

A runtime collection write on a receiver with a place (`xs.swap(i, j)`,
`xs.append(item)`, `counts[key] = value`, `seen.add(item)`, and others)
ended with `store_receiver_back`, which stored the receiver into its
place. The runtime helpers change the receiver's box in place, so the
place already held the changed value, and the store only retained the
box and released the place's reference to the same box. A few writes
used `store_receiver_back_in_place`, which skipped the store for a whole
local; the rest did not.

## Change

`store_receiver_back` now skips the store for every write when the place
is a whole local holding a box, is not a view, overlaps no registered
cleanup, and the load did not copy the receiver into a temporary (a
helper that changed a copy still stores it). A `mut` parameter still
publishes the value to its caller's write-through sink. The separate
`store_receiver_back_in_place` is gone.

## Verification

- New fixture `run-pass/collection_writes_in_place.au`: list `swap`,
  `sort`, `reverse`, `extend`, `remove`, `insert`, `clear`, and `append`
  on a local, through a `mut` parameter, and on a `str` list; `dict` and
  `set` writes through `mut` parameters with a copy taken before a
  change; a `mut self` method writing its fields; and `Array` `fill`,
  `set`, and an index assignment. Its native build makes no retains
  where part 2 made 43.
- `representation_stats` test: a `mut` list argument written in the
  callee and a `dict` written in a loop make no retains.
- Compiler tests, backend parity, the CLI tests, `check:cross-language`,
  `check:operation-probes`, `cargo fmt --check`, hygiene, and clippy.

## Measurements

Callgrind instruction counts and `AURA_RUNTIME_STATS=1` retains, the 30
cross-language programs at the check size, Linux container, release
builds. Before is part 2.

| Program | Retains before | Retains after | Instructions before | Instructions after |
| --- | ---: | ---: | ---: | ---: |
| `astar_pathfinding` | 7,442 | 0 | 14.9M | 13.3M |
| `graph_paths` | 13,450 | 0 | 79.1M | 76.0M |
| `order_book` | 2,000 | 0 | 11.1M | 11.0M |
| `mini_browser` | 15,153 | 30 | 583.2M | 581.4M |
| `markdown_render` | 1,077 | 0 | 9.1M | 9.0M |
| `word_freq` | 12,437 | 2,935 | 53.8M | 53.3M |
| `sort_records` | 4,547 | 1,000 | 45.0M | 44.6M |
| `sql_engine` | 34,867 | 26,017 | 1,413.2M | 1,412.9M |

Retains across all 30 programs: 1,062,951 before step 2h, 400,936 after
part 2, 326,693 after this part. 15 programs now retain at least 90%
less than before step 2h. No program runs more than 0.6% more
instructions (`task_pool`, whose tasks vary run to run).

## What Is Left For The Gate

- `interpreter` (212,087 retains), `binary_trees`, `docdb`: each enum
  payload a pattern binds is read into a temporary and copied into the
  binding, which takes a second reference. Part 4 reads it straight into
  the binding.
- `json_roundtrip`, `sql_engine`, `csv_aggregate`: `str`, `dict`, and
  `list` values copied out of collections.
- Unit results of runtime helpers, the scalar math builtins, and the
  leak, failure, and cancellation tests.
