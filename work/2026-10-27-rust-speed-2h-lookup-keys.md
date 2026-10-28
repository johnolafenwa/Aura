# Rust Speed Step 2h Parts 7 To 9: Lookup Keys, Callbacks, And Union Strings

## Goal

Step 2h of Phase 2 of the Rust-speed plan moves ownership off boxes. Its
gate asks that retains per program drop at least 90%. After parts 1 to 6
(`2026-10-27-rust-speed-2h-cleanup-tests.md` and the notes it follows),
87,876 retains were left across the 30 cross-language programs, and 14
programs were still short of 90%. Most of what was left came from three
conventions.

## The Old Conventions

- Element loan keys. Lowering copies the selector of a dictionary
  element loan (`match table.lookup(key)`, `table[key]` as a view) into
  a temporary, so that rebinding the place it read cannot change the
  loan's key (ADR-0061, 2026-09-21 section, A2). The copy took a
  reference to the key, and native code transferred the key into the
  loan and released it when the loan ended.
- Scalars found by `get`. A `get` on a `dict[str, int64]` found the
  value's box, and reading its payload copied the box first.
- Named callbacks. `rows.sort(key=record_key)`, `map`, and `filter`
  called the callback through the function-value contract, which takes
  each argument as an owned reference, and made the named function
  reachable as a value, so no caller could lend it its arguments.
- Union payload reads. `case str as digits` on a `str | None` borrows the
  member's payload with a loan, but each use of `digits` read the loan
  into a temporary that took a reference to the string.

## Changes

- Part 7, keys (`mir_ownership.rs`, `select_with_unchanged_places`): a
  temporary copied from a root place just before the one element loan
  it selects for, and read nowhere else, gives way to the place when
  nothing rebinds, moves, or changes the place from the copy until the
  loan ends. A shared parameter's method calls do not count as changes;
  for any other place a method call on it does. The element loan now
  borrows its selector (operand role and `borrowed_roots`), so the
  ownership pass keeps the key live and unchanged until the loan ends.
  Native code (`begin_element_loan`) passes a key read from a tracked
  place to the loan without a reference and releases nothing at the end.
- Part 7, scalars (`compile_scalar_variant_payload`): the payload of a
  lookup slot that is not lent and whose target is a scalar is read out
  of the found box, which the slot keeps until its release.
- Part 8, callbacks (`mir.rs`, `vec_callback_target`): a callback that
  names a plain function with one shared parameter, no type parameters,
  and no returned view is called with `CallTarget::Name`. Anything else
  (lambdas, function values in locals, generic functions) is still
  called as a value.
- Part 9, union payloads (`native_codegen_elements.rs`,
  `union_payload_loans`): a `ReadLoan` of a loan begun once on
  `root.__union_payload_N` joins the lent payload reads of part 4, with
  the union local as its scrutinee. Inline unions (`DirectType::Union`)
  hold their member's box in their payload words, so they count as
  scrutinees. `read_lent_union_payload` keeps the box with the lent flag
  unless the read made a value of its own.

## Measurements

`AURA_RUNTIME_STATS=1` retains and callgrind instructions, the 30
cross-language programs at the check size, Linux container, release
builds. Before is part 6.

| Program | Retains before | Retains after | Instructions before | Instructions after |
| --- | ---: | ---: | ---: | ---: |
| `json_roundtrip` | 28,086 | 0 | 566.6M | 568.0M |
| `interpreter` | 23,005 | 8,245 | 83.2M | 82.9M |
| `sql_engine` | 13,422 | 5,946 | 1,375.5M | 1,377.5M |
| `docdb` | 6,104 | 40 | 64.6M | 64.6M |
| `csv_aggregate` | 5,548 | 0 | 65.1M | 65.0M |
| `word_freq` | 2,935 | 1,000 | 53.0M | 53.0M |
| `lru_cache` | 2,447 | 0 | 42.2M | 42.2M |
| `pubsub_broker` | 1,210 | 514 | 84.3M | 84.5M |
| `bank_ledger` | 1,156 | 0 | 13.2M | 13.2M |
| `template_render` | 1,080 | 600 | 31.5M | 31.6M |
| `sort_records` | 1,000 | 0 | 41.3M | 41.3M |
| `lzw_compress` | 660 | 0 | 4.2M | 4.2M |
| `kv_server` | 563 | 4 | 54.6M | 54.8M |
| `http_api` | 414 | 104 | 28.9M | 29.0M |
| All 30 programs | 87,876 | 16,507 | | |

Part 7 alone left 19,984; parts 8 and 9 took `sort_records`,
`csv_aggregate`, and `kv_server` to nothing or almost nothing.

Instruction counts hardly move: retains were inline increments already.
In `json_roundtrip` the generated code runs 0.86M fewer instructions,
and `malloc` and the runtime's release path run about 2.1M more, because
boxes are now freed at different times. No program changes by more than
0.4% apart from `file_io` (+1.3%), which varies run to run with its file
system. Every program still frees every box it allocates.

## Gate

Retains per program against 2g (1,062,951 in all, 16,507 now, 98.4%
fewer): 28 of the 30 programs drop 90% or more, and 20 retain nothing.

- `pubsub_broker`: 4,695 to 514, 89.1%. Almost all of the rest is
  `match self.topics.get(name)`: `get` copies the subscriber list, and
  the copy shares the list's buffer with one reference. The lent lookup
  analysis of step 10g only lends from a local dictionary to a payload
  read, not from a field to a loop over the list.
- `task_pool`: 46 to 26. The program has almost no retains to begin
  with.

## Verification

- Ownership tests: an element loan borrows its key where it lies, for a
  shared parameter, a mutable dictionary's key, and a local bound once;
  a key bound again after the loan ends is still selected in place, and
  one bound again while the loan holds the element stays copied.
- Analysis test: reads of a union member's payload loan that only feed
  `parse_int64` and `len` are lent from the union.
- New fixtures `run-pass/lookup_keys_lent.au` (keys from parameters,
  locals moved after the loan, a key rebound inside the arm, nested and
  mutable lookups, scalar `get` of every number width and `bool`) and
  `run-pass/union_payload_reads_lent.au`. Native builds retain 0 and 4
  times and free every box.
- `representation_stats`: lookups with keys from parameters and
  loop variables, and scalar `get` results, take no reference.
- Runtime, compiler, and CLI tests, backend parity,
  `check:cross-language`, `check:operation-probes`, `cargo fmt --check`,
  hygiene, and clippy.
