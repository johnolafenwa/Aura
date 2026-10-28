# Rust Speed Step 3a: Strings Without Copies

## Goal

Step 3a opens Phase 3 of the Rust-speed plan (strings, dicts, JSON). Its
gate is "a 10-character slice at offset 5,000 costs at most 300
instructions". Profiles of the string programs after Phase 2 showed where
their time went:

- `malloc` and `free` took 20% or more of every string program.
- `aura_direct_element_path_load` was the largest single cost: 26% of
  `sql_engine`, 22% of `csv_aggregate`, 17% of `word_freq`, 15% of
  `template_render`. It decoded its path into a `String` and a `Vec` of
  steps on every call, then copied the value it reached into a new box.
  In `sql_engine`, `view row = self.rows[slot]` passed to
  `matches(row, conditions)` copied the whole row, about 4,000
  instructions a read.
- The runtime's string helpers cloned their receiver before reading it.

## What Changed

- `str` values keep their bytes in `Text`, which records whether every
  byte is ASCII. On ASCII text, `len()` is the byte length and slicing
  takes byte positions, with no decoding. The flag is set when the text is
  built and kept through every append.
- A text of at most 29 bytes keeps its bytes in the value itself, in the
  32 bytes a `String` and the flag took before. Making, copying, and
  freeing a word, a CSV field, a token, or a short slice allocates
  nothing. A copy of a short text that lives on the heap is inline again.
- Element paths parse without allocating: up to eight steps live on the
  stack, and the path text is read in place.
- The string helpers read their receiver in place instead of cloning it.
- A read-only element view passed to a parameter the callee borrows hands
  over the box the list keeps the element in, as a lent local element
  already did. A read-only selection inside such a view, such as `row[i]`
  for `view row = self.rows[slot]`, selects from the parent's box. A loan
  that misses (its list keeps values rather than boxes, or the position is
  out of range) takes the runtime walk as before, and its misses move the
  list's elements into boxes of their own; enum values, dicts, and sets
  now qualify for that, as lists and instances did.
- `str.from_bytes` and the other byte codecs name `Result` and
  `bytes.Error` and their variants with static names instead of
  allocating two strings per result.
- A `str` value gets no runtime type tag: its kind names its type.
- A slice of ASCII text with both bounds in order takes a direct path.

## Measurements

Callgrind instructions at the check size, the 30 cross-language programs,
before (main at the end of Phase 2, `81e439b6`) and after. Every program
prints the same output.

| Program | Before | After | Change |
| --- | ---: | ---: | ---: |
| `sql_engine` | 1,307.5M | 779.5M | -40.4% |
| `template_render` | 31.7M | 25.6M | -19.3% |
| `word_freq` | 52.5M | 42.3M | -19.4% |
| `csv_aggregate` | 63.8M | 53.2M | -16.6% |
| `mini_browser` | 545.1M | 458.2M | -15.9% |
| `spreadsheet` | 17.6M | 15.2M | -13.7% |
| `pubsub_broker` | 84.9M | 74.6M | -12.2% |
| `sort_records` | 41.4M | 36.5M | -11.9% |
| `interpreter` | 73.6M | 66.4M | -9.7% |
| `docdb` | 43.0M | 39.1M | -9.2% |
| `lzw_compress` | 4.2M | 3.8M | -8.9% |
| `kv_server` | 54.3M | 49.6M | -8.6% |
| `json_roundtrip` | 569.6M | 523.9M | -8.0% |
| `text_diff` | 9.4M | 8.7M | -6.9% |
| `lru_cache` | 41.3M | 38.6M | -6.5% |
| `graph_paths` | 58.8M | 55.1M | -6.3% |
| `bytecode_vm` | 6.7M | 6.3M | -5.7% |

No other program changes by more than 3% (`bank_ledger` +1.0%).

Step by step on `sql_engine`: ASCII flag and allocation-free paths
1,307.5M -> 1,216.3M; views passed in place 974.1M; static byte-codec
names, no `str` tag, enum elements lendable 850.1M; nested selections
794.0M; inline short strings 779.5M.

The gate is missed. The `str_slice` probe (`text[5000:5010]` then `len()`)
costs 457 instructions against 157 in Rust (752 after the first part of
this step; 2,108 at the end of Phase 2). What remains is the box every
`str` value lives in: allocating and linking it into the task's ledger
(about 95 instructions), releasing it (about 115), and the runtime call
for `len()` (about 45). Keeping `str` values out of boxes in generated
code is the follow-up that reaches the gate.

Operation probes after the step (instructions per operation, Aura /
Rust): `str_slice` 457 / 157, `dict_get_str` 1,878 / 246 (step 3c),
`enum_match` 1,113 / 9.5.

## Notes

- `Text` derefs to `str`, not `String`; code that needs a `String` calls
  `into_string()`.
- A list's `str` view records where each element's bytes lie. For an
  inline text that is inside the list's storage, which stays put until the
  list changes, and any change switches the view off, as before.
