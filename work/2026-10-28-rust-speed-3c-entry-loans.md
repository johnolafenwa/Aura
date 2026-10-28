# Rust Speed Step 3c, Part 1: Entry Loans and Calls Without Sink Handoffs

## Goal

Step 3c of the Rust-speed plan covers dicts and sets. Its gate is
`word_freq` and `lru_cache` at most 2x Rust. Profiles of both after step 3a
showed two costs this part removes:

- In `lru_cache`, about a quarter of all instructions went to
  `aura_direct_set_next_mutable_sinks` and `aura_direct_exit_call`. Every
  call that passes a `mut` argument, such as `self.unlink(slot)` inside
  `get(mut self, ...)`, stored its write-through sinks in a buffer and
  handed them to the runtime. A pending handoff makes the runtime push the
  callee's frame, so the call lost the frame push generated code does in
  place, and paid an allocation for the sink list. Almost every sink is 0:
  a sink only exists for a place inside a `with` block, or for a parameter
  whose own caller passed one.
- In `word_freq`, `bump` updates a count through
  `match mut counts.lookup(key)`, which walked the element path through
  the runtime for each read and write.

## What Changed

- A call with `mut` arguments ORs its sinks together and calls the
  runtime only when the result is not 0. The release of the sinks after
  the call is under the same test. A callee entered without a handoff sees
  no sinks, as before.
- An `int64` dict value loaned for update (an `int64` or `str` key, no
  projection) is found once, by `aura_direct_map_value_slot` or
  `aura_direct_map_value_slot_int64`, which return the address of the
  value inside the dict. Reads and writes go to that address. A dict shared
  with another thread, or a missing key, gives null and the loan takes the
  runtime walk as before, which reports the key. The probe on that path
  boxes a scalar key, and the box is released in the same block.

## Measurements

Callgrind instructions at the check size, the 30 cross-language programs,
before (step 3a) and after. Every program prints the same output.

| Program | Before | After | Change |
| --- | ---: | ---: | ---: |
| `raytracer` | 21.7M | 6.6M | -69.7% |
| `hashing` | 12.4M | 3.9M | -68.6% |
| `interpreter` | 66.4M | 33.8M | -49.2% |
| `astar_pathfinding` | 7.4M | 4.1M | -44.8% |
| `graph_paths` | 55.1M | 34.2M | -37.9% |
| `lru_cache` | 38.6M | 24.7M | -35.9% |
| `order_book` | 9.5M | 6.9M | -27.1% |
| `regex_match` | 253.9M | 188.8M | -25.6% |
| `csv_aggregate` | 53.2M | 40.9M | -23.1% |
| `markdown_render` | 8.0M | 6.3M | -21.9% |
| `nbody` | 4.9M | 3.8M | -21.5% |
| `image_filters` | 1.2M | 1.0M | -20.1% |
| `word_freq` | 42.3M | 35.4M | -16.3% |
| `bank_ledger` | 12.9M | 11.0M | -15.0% |
| `sql_engine` | 779.5M | 671.3M | -13.9% |
| `spreadsheet` | 15.2M | 13.7M | -10.0% |

`docdb`, `mini_browser`, `json_roundtrip`, `text_diff`, `sort_records`,
`template_render`, `lzw_compress`, and `pubsub_broker` drop 2% to 6%; the
rest change by less than 1%. `kv_server` varies by about 5% from run to
run under callgrind (socket polling), so its single A/B reading is noise.

The gate is not met yet. At 50,000 operations, by instructions:

| Program | Step 3a | Now | Rust | Now / Rust |
| --- | ---: | ---: | ---: | ---: |
| `lru_cache` | 409.7M | 263.7M | 67.3M | 3.9x |
| `word_freq` | 1,003.1M | 613.1M | 139.1M | 4.4x |

What remains in `word_freq`: releasing string boxes (15%), `str + str`
through the generic binary operator, which copies both operands (11%),
`bump` looking the key up twice (`contains_key` then the entry), list
element copies through the element path, and `format_begin` decoding its
literal. In `lru_cache`: type tags on fields taken out of `self` and on
`self` after each call (9%), SipHash of string keys, and boxes for
`f"user:{k}"`. Later parts of 3c and steps 3b and 3d take these on.
