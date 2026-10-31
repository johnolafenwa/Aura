# Beat Python On The Last Slow Benchmarks

Started 2026-10-30, after 0.10.0-preview and the file I/O hand-off.

In 0.10.0, four cross-language programs were still slower than Python:
`docdb` (3.2x), `json_roundtrip` (1.9x), `template_render` (1.9x) and
`csv_aggregate` (1.3x). `regex_match` was the one program more than 8x
slower than Rust.

## Where The Time Went

`docdb` and `json_roundtrip` spent 35-45% of their instructions on
dictionary lookups. A read-only `match d.lookup(key)` tested for the key,
probed the dictionary again inside the `Found` arm, then walked from the
dictionary to the value to copy it, and released the copy at the end.

## Steps

- 1a: one probe per read-only lookup. When every use of the found value
  is a read or a call argument, the native code calls the existing
  "copy or null" helper once, branches on the result, and hands the copy
  to its single reader (or retains it for each of several readers). The
  rule covers dictionaries in locals and in class fields, and `str` and
  `int64` keys. Fixture: `lookup_entry_copies.au`.
  Release builds, best of five on Linux: `docdb` 1.88 -> 1.49 s,
  `json_roundtrip` 0.88 -> 0.69 s; `template_render` and `csv_aggregate`
  unchanged.
- 1b: after 1a, `docdb`'s field readers spent most of their time in
  `json.as_int` / `json.as_bool`: an argument buffer, a boxed `T | None`
  result, then a take and an unbox. `aura_direct_json_as_scalar` writes
  the payload bits to a stack slot and returns whether the variant
  matched, and the emitter builds the union's words from that. `docdb`
  instructions at size 3000: 452M -> 426M (about 150M of each is the
  blocking-pool poll that callgrind counts). Wall time is within noise.
  Fixture: `json_scalar_accessors.au`.
- Both merged in PR #199.
- 2a: splitting `json_roundtrip` showed parse 0.30 s (Python 0.10),
  walk 0.30 s (Python 0.16), dumps 0.05 s (Python 0.09). Parsing went
  through serde with `arbitrary_precision`, so every number arrived as a
  map with an allocated lexeme, every key as a `String` copied again into
  a `str`, after a separate pass over the source for the depth limit.
  `FastJsonParser` in `json_codec.rs` reads valid JSON in one pass and
  gives up on anything else, and `parse_with_serde` then produces the
  ranked error. Numbers use the same `classify_number`, objects the same
  `object_insert`. A differential test compares the two on a corpus of
  valid and invalid sources. Parse-only 0.30 -> 0.24 s, `json_roundtrip`
  0.67 -> 0.59 s. What remains in parsing is building the values (a
  payload `Vec` per node, two `Type`s per object, dict inserts) and
  dropping them.
- Next: lend dictionary values and JSON array elements in `walk`.
- 2: lend JSON array elements in loops.
- 3: string work in `template_render` and `csv_aggregate`.
- 4: `regex_match`.
