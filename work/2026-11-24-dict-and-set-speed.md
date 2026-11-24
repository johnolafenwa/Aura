# Dict and set speed

Part of the work toward Rust's speed that John asked for on 2026-11-24.
This note covers the dict and set half: hashing a key, finding its entry,
removing an entry, and looping over entries.

## What was measured first

Nine operation probes were added (`benchmarks/operation_probes`, plan step
`dict`). On `main` at `6642a7c` (0.17.0), in instructions per operation:

- Hashing and probing were already close to Rust. An `int64` lookup spent
  about 160 instructions in the hash and the table, and Rust's whole
  operation is about 190.
- The cost was around the lookup. `dict_remove` took 2,030 instructions
  against Rust's 204, because every remove built a `Lookup` value around
  the removed value, boxed the key, and probed twice.
- `for key, value in d.items()` took 4,046 instructions per entry against
  Rust's 16.5. It built a list of every entry as a tuple, copied each
  tuple out of the list, and copied each element out of the tuple.
- A `str` key read from a list was copied into a box before each lookup.

## What changed

- Hashing: `str` and `int64` keys are hashed in one call with no hasher
  state kept between writes. The result is the same SipHash-1-3 value, so
  the protection against chosen keys stays. A test compares the one-call
  hashes with the streaming hasher.
- Finding: a lookup matches on the storage of the entries once. An
  `int64` insert probes once and reuses the hash for the new entry.
- Removes: a remove whose result is matched, unpacked or dropped builds no
  `Lookup`. Native code keeps the removed value or null, as it already did
  for `get`. A remove whose result is dropped does not box the value at
  all. `int64` keys are passed as a word.
- Loops: a loop over `d.items()`, `d.keys()` or `d.values()` makes no list
  when the keys and values are numbers, `bool`s or `str`s. Native code
  takes a snapshot of the dict, which is a second handle to the same
  entries, and walks it by entry. A dict copies its entries on the first
  write through a handle that shares them, so the loop still sees the
  entries the dict held when the loop began, whatever the body does to
  the dict. The fixture `dict_entry_loops_walk_a_snapshot.au` writes to
  and removes from the dict inside each kind of loop.

The loop recognizer is `native_codegen_entry_loops.rs`. It keeps the list
when anything else uses it, or when a key or value could share storage
with the dict (a list, a class, a tuple).

## Results

Instructions per operation, release `aura`, Linux x86-64:

| operation | Rust | before | after |
| --- | ---: | ---: | ---: |
| `dict_remove` | 202.6 | 2,030.0 | 524.3 |
| `dict_insert_int` | 455.4 | 686.0 | 513.5 |
| `dict_count_str` | 254.9 | 1,976.0 | 1,829.8 |
| `dict_contains_str` | 233.8 | 1,050.0 | 985.1 |
| `set_contains_str` | 232.3 | 1,087.0 | 995.5 |
| `set_contains_int` | 193.9 | 284.0 | 255.0 |
| `dict_items_loop` | 16.5 | 4,046.0 | 252.2 |

Loops over other entry shapes, instructions per entry, before and after:
`int64` keys 215 to 159, `int64` values 216 to 160, `str` keys with
`int64` values as items 2,666 to 678, `str` keys 852 to 585, `str` to
`str` items 2,746 to 1,103.

Whole programs moved less, because the lookups are a small share of each.
Instructions against Rust, before and after: `lru_cache` 3.3x to 3.0x,
`lzw_compress` 7.1x to 6.9x, `word_freq` 3.1x to 3.0x, `csv_aggregate`
3.1x to 3.0x. `sql_engine` (4.8x), `mini_browser` (6.2x), `interpreter`
(2.9x) and `template_render` (2.6x) did not move. Python ran `lru_cache`
in 205 ms, Rust in 18 ms and Aura in 55 ms (64 ms before); for
`lzw_compress` the figures are 323 ms, 31 ms and 170 ms (188 ms before).

## Verification

Every run-pass fixture with expected output (914) was built with the
native backend and printed what the interpreter prints. The remove fixture
ran clean under valgrind.

## What is left

- A `str` key read from a list, a view or a field is copied before the
  lookup. This is most of what `dict_get_str`, `dict_count_str`,
  `dict_contains_str` and `set_contains_str` still cost over Rust.
- A scalar value read from a dict comes back boxed (`dict_get_int`, 4.2x).
  The thread on boxes owns that.
- Entry loops call the runtime twice per entry, and copy each `str` key.
- Entry loops over values that are lists, classes or tuples still make
  the list.
