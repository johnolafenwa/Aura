# Rust speed, rounds 3 to 16: text as words, loans, and slow paths that stay out of the loop

## Goal

John's 2026-11-24 target is that Aura programs run as fast as their Rust
versions in `benchmarks/cross_language/`. Rounds 1 and 2 landed in #325
(`2026-11-24-rust-speed-round-1.md`). This note covers rounds 3 to 16,
which ran on one local branch while #325, #322, #314 and #330 were in
review, and land as one pull request.

## Starting point

After round 3 was combined (2026-11-25, rounds 1 to 3 on top of main
before #322), the median program ran 2.41 times Rust's instructions
under callgrind at the bench size, and eight programs ran above 4x:
`markdown_render` 6.6, `http_stream` 6.3, `file_io` 5.3, `lzw_compress`
5.2, `mini_browser` 4.8, `kv_server` 4.4, `pubsub_broker` 4.3 and
`order_book` 4.2. Most of the gap was text: a `str` local was a box, and
every read of a `str` field, list element or dictionary key copied one.

## Work completed

- Round 3: a list element or a field of a Copy type is lent where it lies
  instead of boxed (`enum_match` probe 965 to 136 instructions an
  operation). Loop versions come in tiers, nest, cover whole method
  bodies and carry a proof into a sibling loop. A local `str` is kept as
  words (pointer, length, owner) instead of a box, and a function that
  takes a `str` it only reads gets a second body that takes the words
  (`aura_view_<fn>`).
- Round 4: a nested field list of `str` is read in place, the reactor
  waits without copies, and the scheduler's waiting set is a map. An
  extend of a byte list from a `str`, `to_bytes()` and the text methods
  on a `str` field read the words; a list literal of `str` is built in
  one call. `markdown_render` 6.60x to 2.92x.
- Round 5: `lzw_compress` 5.23x to 3.87x and `http_stream` 6.20x to
  5.66x; `lru_cache`, `astar_pathfinding`, `spreadsheet` and
  `graph_paths` moved the median.
- Round 6: a thread keeps one spare scalar list box, a sort key is lent
  from its element, a `Queue[int64]` sends its values unboxed, a sort of
  `str` compares an 8-byte prefix first, a frame on a call cycle is
  pushed only when needed, and a literal list of `str` is frozen once.
  `regex_match` 2.19x, `sort_records` 2.07x, `spreadsheet` 1.97x.
- Round 7: a function with `mut` parameters that are not changed gets a
  body without write-back sinks, an acyclic function enters its frame
  late, and more layouts fall back without a box. `nbody` 1.96x,
  `regex_match` 1.94x. A split of LLVM units that stopped `intersect`
  from inlining in `raytracer` was fixed in the same round.
- Round 8: sinks are skipped for unchanged `mut` parameters, parsed
  numbers and split pieces stay in words, `write_all` of a byte list
  copies once, and a class of `str` and scalars lies in its list in
  place. `http_stream` 4.68x to 3.58x, `file_io` 4.48x to 2.76x,
  `regex_scan` 2.16x to 1.96x. No program was above 4x after this round.
- Round 9: a read event's union is built in words, bounded stream reads
  use a scratch buffer per thread, and a kept instance is swapped and
  cloned in place. `pubsub_broker` 2.98x to 2.42x, `http_api` 2.71x to
  2.35x, `sort_records` 1.75x to 1.50x.
- Round 10: cancel flags are shared, `Captures.get` reads its length in
  place, a `Result` slot carries `T | None` as union words, and split
  lists are typed when made. `task_pool` 2.66x to 2.23x, `http_stream`
  3.12x to 2.53x.
- Round 11: a chain of `str` `+` builds through one formatted call
  (`aura_direct_concat_str_parts`), and `resume_task` finds the task in
  place.
- Round 12: a single `str` `+` joins words, `x = x + y` appends in place,
  and a `str` literal read shares one frozen box. `kv_server` 2.73x to
  2.44x, `lzw_compress` 3.54x to 3.25x.
- Round 13: a callee that only reads scalar fields of a list element is
  handed a stand-in header instead of a copy, extends keep the spare
  storage, and class fields carry alias classes for LLVM.
  `sort_records` 1.50x to 1.28x, `lzw_compress` 3.26x to 3.03x.
- Round 14: a `str` field compare reads its words, enum arguments travel
  untagged when the callee knows the variant, and a callee that can only
  trap links into its caller's chain on the trap path. `sql_engine`
  2.70x to 2.43x, `file_io` 2.49x.
- Round 15: a function's slow paths no longer rejoin its hot loop; the
  loop's body is copied for the slow way out
  (`native_backend_llvm_slow_paths.rs`). `order_book` 3.25x to 2.64x,
  `astar_pathfinding` 7% fewer instructions.
- Round 16: the copy is made only when the join reads values again and
  is capped at 8192 instructions, a field list read only for its length
  is read in place, and recursive bodies are copied too. `order_book`
  2.45x, `file_io` 2.44x; the regressions round 15 caused in
  `text_diff`, `hashing` and `lzw_compress` are gone. Code size grew by
  0.77% at the median and at most 5%.
- A miscompile found before landing: a `keys()` or `values()` loop that
  walks a dictionary's entries and a `str` element slot both claimed the
  loop variable, so `key.clone()` made an empty `str` (595adc2). A
  membership test of a `str` list element is now excluded from element
  slots in the same way.
- A clash found in the merge with 0.18: a function that defers its frame
  record (round 7) pushes it at the first runtime call that needs it,
  which ends the block. The 0.18 drop hooks stamp an owned parameter, and
  addressed C locals copy a parameter into a cell, before the body, so
  the body was compiled into an ended block and the class drop fixtures
  failed to build natively. The body now starts in a block of its own
  after such calls.

## Measurements

Instructions under callgrind at the bench size, in millions. "Round 3"
is the combined branch of 2026-11-25 (before #322 merged); "Now" is this
branch at round 16.

| Program | Round 3 | Now | Rust | Round 3 / Rust | Now / Rust |
| --- | ---: | ---: | ---: | ---: | ---: |
| `mini_browser` | 3,498.6 | 2,424.9 | 732.4 | 4.78x | 3.31x |
| `lzw_compress` | 1,428.5 | 827.2 | 273.2 | 5.23x | 3.03x |
| `order_book` | 83.3 | 48.5 | 19.8 | 4.21x | 2.45x |
| `file_io` | 2,864.1 | 1,317.2 | 539.9 | 5.30x | 2.44x |
| `sql_engine` | 1,497.5 | 1,051.6 | 437.4 | 3.42x | 2.40x |
| `kv_server` | 1,653.5 | 898.8 | 378.9 | 4.36x | 2.37x |
| `http_stream` | 8,431.7 | 3,067.4 | 1,335.4 | 6.31x | 2.30x |
| `http_api` | 546.8 | 387.2 | 178.5 | 3.06x | 2.17x |
| `word_freq` | 3,295.0 | 2,376.8 | 1,106.1 | 2.98x | 2.15x |
| `task_pool` | 1,829.5 | 1,122.4 | 531.8 | 3.44x | 2.11x |
| `pubsub_broker` | 3,638.0 | 1,751.5 | 846.7 | 4.30x | 2.07x |
| `markdown_render` | 2,117.6 | 576.7 | 320.9 | 6.60x | 1.80x |
| `regex_match` | 3,880.1 | 2,111.7 | 1,200.7 | 3.23x | 1.76x |
| `template_render` | 6,774.8 | 4,237.1 | 3,214.2 | 2.11x | 1.32x |
| `sort_records` | 1,644.4 | 568.7 | 467.8 | 3.52x | 1.22x |

The median of the suite went from 2.41x to 1.80x Rust's instructions.
Only `lzw_compress` (3.03x) and `mini_browser` (3.31x) remain above
2.5x.

## Follow-up

- `mini_browser`: most of what is left is the runtime's held `str`:
  format boxes, `push_in_place` of a `str`, and split views. It waits on
  the text thread's stable `str` view and split-built views (#328 first).
- `lzw_compress`: the `dict[int64, int64]` lookups cost about what Rust's
  SipHash costs; John chose to keep SipHash. The rest is result boxing on
  paths the word slots do not reach.
- `sql_engine`: each condition of `matches()` costs about 85
  instructions.
- `order_book`: each operation on `self` checks its layout again.
