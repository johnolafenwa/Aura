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
- `d[k]` passed to a borrowing parameter now takes a shadow box too, and
  `json.dumps` / `json.parse` read and build `json.Value` variants by
  their index in the layout (docdb 801M, json_roundtrip 708M).
- Keys loops: `for key in d.keys(): match d.lookup(key)` over a dict that
  nothing changes no longer copies the keys or each key and does not
  hash. Native code keeps the slot of the entry it is on
  (`aura_direct_map_next_slot`) and the fused test lends that slot's
  value (`aura_direct_map_shadow_at`). The analysis is
  `native_codegen_key_loops.rs`. A micro walk of 8000 keys per round went
  from about 1800 to 700 instructions per key. json_roundtrip 10 rounds:
  708M -> 546M; wall about 620 ms against Python's 420.
- Found while profiling: a typed variant box's layout tag is registered
  under the plain name (`json.Value`) and generated code tags with the
  descriptor (`aura-type-v2:n10:json.Value0;`), so the two never compare
  equal and each fresh element box is tagged once more (about 130
  instructions each). Parse is about 1150 instructions per value and
  dropping the tree about 1000.
- The native binary size baselines were re-recorded: the json subject
  grew about 11% on Linux with this work (1661336 -> 1838368 bytes).
- Matched lookups: `match d.lookup(k): case Found(v): match v: case
  json.Value.String(text): return text.clone()` reads a shadow box when
  the read is only matched and its payloads are only cloned on a straight
  path (`native_codegen_shadow_reads.rs`). `field_str` went from about
  2100 to 1200 instructions a call; docdb 10k 803M -> 768M.
- `xs.append(x)` of a scalar through an element or entry loan is one call,
  `aura_direct_element_path_push_scalar`, instead of a take, a push, and a
  store of the list (docdb 768M -> 729M).
- The allocation counters of `AURA_RUNTIME_STATS` now count only when the
  report is requested, and a release of a box's last reference frees it
  without a compare-exchange. Neither moved wall time on this machine.
- Wall time is still about 1.8x Python on docdb (975 ms vs 540 at 50k)
  although Aura now runs fewer instructions (729M vs 814M at 10k, Python's
  startup taken out). Aura peaks at 210 MB against Python's 120 MB and
  takes twice the page faults: every box is 192 bytes (`Value` is 96, the
  largest variants being `EnumVariantValue` 80 with two 24-byte
  `RuntimeName`s, `VecValue` 72, `IntegerValue` 48), and a dict entry is
  two `Value`s. Per phase at 50k (Aura / Python ms): insert 216 / 81,
  reads 73 / 63, count_where 43 / 17, update 26 / 1, aggregate 45 / 7,
  dumps 161 / 181, reload 206 / 155, and about 120 ms of freeing at the
  end against Python's 20.
- `match mut d.lookup(k)` on a dict value lowers to a copy of the entry
  (`aura_direct_element_path_load` clones the whole JSON document), a
  mutation of the copy, and a store of it back, so docdb's update loop
  costs about 25k instructions an update.

## Lean values, steps 1 and 2 (2026-11-08)

The design is in the lean-values checkpoint John approved on 2026-11-08
(steps 1 to 6, one PR each).

- Step 1: the enum and variant names a typed layout keeps are static
  `RuntimeName`s, so copying one touches no count. Locked operations on
  docdb fell from 2.36M to 1.64M per 10k docs; wall time did not move.
- Step 2: a variant whose payloads are scalars of 8 bytes or less keeps
  them in its `TypedPayloads` handle instead of a heap block, and a box
  that holds one points its view at the handle (`follow_inline_block`).
  That removes an allocation and a free per JSON number or bool, and cut
  docdb's memory by about 5%.
- Found by sampling json_roundtrip in real time (an `ITIMER_PROF`
  sampler, which counts what callgrind does not: cache misses and
  faults): 14.5% of samples sat on one load in the allocator's
  `refill`, which walked every block of a shared batch to count it. The
  blocks are cold, so each step was a cache miss. A batch now keeps its
  length in its second block.

Wall time on the cloud box, median of 5 (json_roundtrip 30 rounds, docdb
50k docs):

| Program | Steps 1-2 | + batch length | Python |
| --- | --- | --- | --- |
| json_roundtrip | 367 ms | 306 ms | 224 ms |
| docdb | 831 ms, 194 MB | 768 ms, 194 MB | 488 ms, 115 MB |

## Lean values, step 3: a 64-byte `Value` (2026-11-08)

- `RuntimeName` is one word. A static name keeps its address in the low
  48 bits, its length in the next 15, and sets the top bit; a shared name
  is the address of an `Arc<String>`. `EnumVariantValue` went from 80 to
  48 bytes without dropping its names.
- `list` and `set` keep their element type as a `SharedType` (one
  `Arc<Type>`, with the common types shared once), like `dict` already
  did: `VecValue` 72 -> 32 bytes, `SetValue` 64 -> 24.
- An instance's class name is a `RuntimeName` (`InstanceValue` 56 -> 40),
  and an HTTP response keeps its parts behind one pointer (80 -> 8).
- `Value` is 64 bytes (from 96), a box 160 (from 192), and a dict entry
  128 (from 192).

| Program | Before | After | Python |
| --- | --- | --- | --- |
| json_roundtrip, 30 rounds | 340 ms, 16 MB | 318 ms, 12 MB | 228 ms, 14 MB |
| docdb, 50k docs | 798 ms, 194 MB | 730 ms, 151 MB | 452 ms, 115 MB |

## Lean values, step 4: a smaller box header (2026-11-09)

- A task's ledger keeps its linked boxes in a vector, and each box holds
  its index there (4 bytes) instead of two list links (16). A box leaves
  by swapping with the last.
- A box's type name is a one-word `RuntimeName`. Type tags live in the
  registry for good, so their names are static text, and tagging a box
  copies a word instead of taking an `Arc` count.
- A box is 144 bytes (from 160). Getting to 128 needs a 4-byte cut more,
  which means generated code reading a narrower field; left for later.

| Program | Before | After | Python |
| --- | --- | --- | --- |
| json_roundtrip, 30 rounds | 309 ms, 12 MB | 268 ms, 12 MB | 228 ms, 14 MB |
| docdb, 50k docs | 739 ms, 151 MB | 689 ms, 146 MB | 452 ms, 115 MB |
- A full allocator list hands over its newer half without walking it:
  each list marks the block that took it past half, and the batch runs
  from the head down to that mark. `share_half` was 2.5% of
  json_roundtrip's samples. docdb 50k: 670 -> 644 ms; json_roundtrip
  272 -> 267 ms.

## Compact JSON storage, step 1: parsed documents in a tape (2026-11-09)

Design: https://claude.ai/artifact/DdT7b3GDk6GRWAN9JpWteo (approved by
John 2026-11-09).

- `json.parse` keeps a document as one immutable tape
  (`crates/aura-runtime/src/json_tape.rs`): 16-byte nodes in document
  order, a container's node holding its count and the index past it, and
  one text buffer (the source, then any unescaped string). A
  `json.Value` in a tape is `VariantPayloads::Json(JsonNode)`: the tape,
  shared by `Arc`, and a node index. Dropping or copying a document is
  one count change.
- Reading a payload gives a fresh value one level deep whose children are
  tape values again, and a write (`values_mut`) turns that one level into
  values first, so a change copies only the level it writes.
- `json.dumps` writes a tape value (alone, or inside a tree) by scanning
  its nodes, keys sorted as before.
- A source the tape does not take (a parse error, nesting or value
  limits, or an object that repeats a key) takes the tree parse it took
  before, which reports what it reported. Only a program whose
  `json.Value` has its layout parses into tapes.
- No codegen change: generated code reads a tape value through the
  runtime, as it reads a variant kept as values.

Wall time on the cloud box, median of 5 (json_roundtrip 30 rounds,
docdb 50k docs):

| Program | Before | After | Python |
| --- | --- | --- | --- |
| json_roundtrip | 261 ms, 12 MB | 144 ms, 12 MB | 200 ms, 16 MB |
| docdb | 687 ms, 146 MB | 677 ms, 163 MB | 433 ms, 117 MB |

json_roundtrip now beats Python. docdb gains little: every field read of
a reloaded document builds that document's dict again, which is what
step 3 (lookups in the tape) removes, and its inserted documents are
still trees until step 4. Its peak grows by about 17 MB during the
reload: 50k tapes alone take 27 MB against 97 MB as trees, but their
blocks fall in other size classes than the boxes freed before them.

## Compact JSON storage, step 2: generated code matches tape nodes (2026-11-09)

- A box that holds a tape value gets the list-view access code
  `JSON_TAPE_ACCESS`, and its view word is the node's address.
- When generated code tests a `json.Value` variant, it reads the node's
  tag byte; when it reads an `Int`, `Float` or `Bool` payload, it reads the
  node's scalar word. A box in either other form takes the path it took
  before, and so does every other payload read.
- In the runtime, typed scalar reads (`as_scalar`) also read the node.

json_roundtrip, 10 rounds: 589M -> 513M instructions (callgrind). Wall
time on the cloud box, 50 rounds, median of 11: 228 -> 208 ms (Python
377 ms on the same run).

## Compact JSON storage, step 3: dict reads in the tape (2026-11-09)

- A dict made from a parsed object keeps its entries unbuilt
  (`IndexedEntries` storage `Lazy`: the object's node and a build
  function). Its length comes from the node, and it counts as shared, so
  any write builds its own entries first.
- A lookup with a `str` key (get, get_str and their payload, lent and
  shadow forms) reads the key in the tape and builds only the value it
  finds. Any other read builds the entries once and keeps them.

docdb, 10k docs: 801M -> 695M instructions (callgrind). Wall time on the
cloud box, 50k docs, median of 11: 667 ms (after step 2: 694 ms) ->
589 ms. Python took 433 ms on an earlier run on this box. json_roundtrip
does not change (218 ms, within noise).
