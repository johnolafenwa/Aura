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
- 2a merged in PR #200.
- 2b: `lent_element_reads` rejected a lent list element as soon as an
  instruction other than a read named it, so a borrowed call argument
  always got a copy (`aura_direct_vec_index`: clone, box, release).
  It now takes the callee's borrowed operands, as `lent_payload_reads`
  already did, and the existing promotion still copies the element
  before any instruction that could change the list. Element loop micro
  (400k elements to a borrowed `size`): 648M -> 446M instructions.
  `json_roundtrip` gains little (0.58 -> 0.56 s), because `walk` binds
  `items` as a fresh payload copy of a shared list each call, which the
  runtime does not convert into boxes; lending payloads of borrowed
  runtime-enum parameters in place is the next lever there.
  Fixture: `lend_elements_to_borrowed_calls.au`.
- 2: lend JSON array elements in loops.
- 2b merged in PR #201.
- 3a: `template_render` spent 4.4% in `count`'s `str::matches`
  (TwoWaySearcher) and about 10% in `replace`'s `memmem` finder setup
  and search, all for one-byte patterns. Both now use `memchr` for a
  one-byte pattern. `template_render` 1.06 -> 0.91 s (Python 0.57).
  What remains: each chained `replace` makes a box even when nothing
  matched, box churn in `out.append`, and `join`.
- 3b: `csv_aggregate` (callgrind, 40k rows): `split(",")` ran
  TwoWaySearcher (5.8%), and each field passed to `to_int`/`add_to`
  was copied into a new box (360k copies). `split` now scans one-byte
  separators with `memchr`, its parts skip the ASCII check, and
  f-string parts pass their known ASCII flag: 943M -> 880M instructions.
  Tried and dropped: lending element loans (`to_int(fields[3])`) to
  borrowed calls. A fresh `list[str]` must first move its elements into
  boxes, which costs as much as the copies (net +8M instructions).
  What remains: freeing each line's lists (14%), and f-string build cost
  (about 225 instructions per part).
- 4: `regex_match` (0.59 s; Python 1.84 s, Rust 0.084 s). Callgrind:
  21% went to type tags on `following: list[int64] = []`, made for each
  byte. `aura_direct_vec_empty_tagged` did not record the shape tag
  (step 5a), and each scalar push cleared it, so the tag on the next
  assignment or call reached `set_explicit_runtime_type_tag` and compared
  `Type`s. Now the new list records the tag, and a push that stores the
  scalar unboxed in a list of that kind keeps it. 1,434M -> 1,164M
  instructions, 0.59 -> 0.53 s (6.2x Rust). What remains: cloning the
  element `Type` (String allocation) for each new list, about 6%.

## 2026-11-08: docdb and json_roundtrip (PR from `claude/project-thread-yj00ct`)

Goal: beat Python wall time on `docdb` (Python 574 ms) and
`json_roundtrip` (Python 434 ms). Not met yet; this PR is progress.
Callgrind instruction counts (docdb 10000 rows, json_roundtrip 10):

| Step | docdb | json_roundtrip |
| --- | --- | --- |
| Start | 1376M | 946M |
| Typed `json.Value` layout, lent field reads | 1102M | 911M |
| Dict literal entries, element path walks on bytes | 964M | 887M |
| Map insert in place, faster JSON runtime | 947M | 879M |
| Shadow loans of dict values to borrowing calls | 896M | 776M |
| Frozen borrowed str literals, own SipHash-1-3, next-entry lookup hint, cached payload tags | 830M | 752M |

- `json.Value` uses the typed enum layout. Generated code mutates its
  ElementBoxes in place without refcount checks, so two holders must
  never share a payload box. Sharing them on clone broke
  `module_constants_read_across_tasks` and was reverted.
- Shadow loans: when every read of a dict entry loan is a borrowed
  argument of a user call, the entry's value is aliased into a box whose
  count carries `SHADOW_REFERENCE`, so it always looks shared and never
  frees the value. `aura_direct_end_shadow` frees the box, or makes a
  real copy if a callee kept it. Only typed variants in a confined,
  unshared dict get a shadow.
- Owned host builtin arguments now copy a box that another reference
  holds, rather than emptying it under the other holder.
- Wall time on the cloud box after these (bench sizes): docdb about
  1030 ms against Python's 500, json_roundtrip about 680 ms against 410.
- Dict keys still hash with keyed SipHash-1-3, now the runtime's own copy
  (checked against the reference vectors) with keys taken from
  `RandomState` once per process, so a one-word key compresses directly.
- A `str` literal passed to a borrowing parameter is one frozen box per
  literal, kept in a data word, like a module constant.
- Tried and dropped: lending `list[str]` elements (`keys[i]`) in
  `lent_element_reads`; it saved under 1% (892M / 776M).
- Callgrind counts include the scheduler spin around file calls (about
  331M of docdb), so docdb counts overstate the work.
- What remains: json_roundtrip walk over `keys()` (str copies, releases,
  type tags), parse 254M, dumps 153M, tree drop 141M; docdb `field_str`
  (its loan copy outlives the loan), parse, dumps, insert.
