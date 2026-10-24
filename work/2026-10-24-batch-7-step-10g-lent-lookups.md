# Batch 7 Step 10g: Lent Lookups

## Goal

Bring `record_field_reads` to 4 times Rust or less in instructions on
LLVM, about 144 million. A lookup whose found value is only read should
borrow the stored value instead of copying it into a new box. The proof
must fall back to the copy whenever it fails, and no error may change.

## Profile

Callgrind on the LLVM build of `bc354d91`, which is 10f. The program
builds 5,000 dicts of four `json.Value` fields, then reads the `age` of
each dict 20 times. The run took 227.1 million instructions, against
Rust's 36.0 million.

- Building the dicts took about 100.6 million. `aura_direct_enum_variant`
  cost about 1,000 instructions a call. `aura_direct_map_set_index_in_place`
  cost 1,029, part of it a boxed unit result that the caller freed at
  once. The dict was tagged again after every set, at about 217
  instructions. `aura_direct_arg_buffer_new` cost 270, mostly `calloc`.
- The read loop took 126.5 million, about 1,265 instructions for each of
  the 100,000 lookups:

| Step | Instructions |
| --- | ---: |
| Find the key and copy the found value into a new box | 626 |
| Release that box after the match | 258 |
| Test the variant | 151 |
| Read the `int64` payload | 65 |
| Tag the payload type | 65 |
| Generated code | about 100 |

The copy and its release are 70% of the loop. The variant test spent
most of its time comparing names byte by byte.

## Design

### Why not `Lend`

MIR's `Lend` and `EndLend` move a place into a temporary for the length
of one call, and the ownership checker sees the loan end at that call. A
lookup result is a new value. Its uses cross blocks and terminators, and
native code reads a view through `ElementPathLoad`, which copies. Using
`Lend` would need a new MIR form for a loan into a value that is not a
place, and the checker would need to follow it across blocks. So the
proof sits beside the existing lookup slot analysis in
`native_codegen_lookups.rs`. It is stricter than the checker would be,
and it needs no language change.

### The proof

`lent_lookup_places` starts from the lookup slots of 10d. A slot is lent
when all of these hold:

- The slot is assigned once and is not a parameter. It holds a handle
  the MIR tracks.
- The `get` reads a local collection, not a field path.
- The found value and every local it is copied into are each assigned
  once and hold a handle. Each is only retained, released, matched, or
  unpacked into an `int32`, `int64`, `uint64`, or `bool` local.
- A backward liveness pass covers the found value and its copies. While
  any of them is live, every step is one of those uses, a scalar `Use`,
  `Unary`, or `Binary` of scalar locals and constants, `EndLoan`, or
  `IndexInRange`. Every terminator is a `Goto`, a `Branch`, or a match on
  one of them.
- None of them is live where the function starts.

A call, a write to any collection, and a release of the dict all fail the
proof. The MIR releases a dict right after its last use, so a lookup
that is the dict's last use is not lent either. The slot then keeps the
copying `_payload` form of 10d.

### The word

A lent slot, and each local that holds its found value, keeps one of
three words:

- null for `Lookup.Missing`,
- an owned box, with the low bit clear, or
- the stored value's address with the low bit set.

The `_lent` forms of the `get` helpers fill the slot. They lend only when
the dict box is confined to this thread, and only a value whose copy
cannot fail: a leaf, or an enum variant whose payloads are all leaves.
Any other value, and any value in a shared dict, comes back as an owned
copy, the way the `_payload` forms copy it. So every error, including
the type errors of the list and dict forms, is the same as before.

Generated retain and release skip a word with the low bit set. A move of
the word nulls its source, as for an owned box.
`aura_direct_lent_variant_matches` and `aura_direct_lent_variant_payload_*`
read either form.

### Other runtime work

- **Short names.** Variant tests, enum name prefixes, and `str` key
  checks compare up to 16 bytes with two overlapping word loads. The
  separator check in a nominal name scans a word at a time.
- **Set results.** `aura_direct_map_set_index_in_place` returns null,
  which releases as nothing, instead of a boxed unit.
- **Argument buffers.** `aura_direct_arg_buffer_new` takes plain memory
  and writes the zeros itself instead of calling `calloc`.
- **Tags.** A repeated type tag is dropped across `FormatBegin`, string
  literals, and boxed `int64` and `bool` values, which run no user code.
- **Literal names.** The per-thread table of names that
  `aura_direct_enum_variant` decodes from literals was direct-mapped. In
  the new build, two of the program's literals landed in one slot and
  evicted each other on every call, 10,000 times in the run. Each literal
  may now sit in its own slot or the one beside it.

## Measurements

Callgrind instructions for one whole run, startup included, from release
builds. Before is `bc354d91`.

| Code generator | Program | Before | After | Change |
| --- | --- | ---: | ---: | ---: |
| Cranelift | `fib(30)` | 210,611,440 | 210,615,386 | 0.0% |
| Cranelift | `int_list_sum` | 47,218,352 | 47,219,120 | 0.0% |
| Cranelift | `string_list_scan` | 58,009,633 | 58,008,194 | 0.0% |
| Cranelift | `dict_build_get` | 377,132,961 | 371,742,577 | -1.4% |
| Cranelift | `set_insert_contains` | 64,968,383 | 64,937,418 | 0.0% |
| Cranelift | `record_field_reads` | 245,978,063 | 147,049,020 | -40.2% |
| LLVM | `fib(30)` | 115,029,200 | 115,028,925 | 0.0% |
| LLVM | `int_list_sum` | 16,026,446 | 16,017,473 | -0.1% |
| LLVM | `string_list_scan` | 32,939,417 | 32,940,365 | 0.0% |
| LLVM | `dict_build_get` | 349,923,142 | 344,782,039 | -1.5% |
| LLVM | `set_insert_contains` | 59,562,853 | 59,583,231 | 0.0% |
| LLVM | `record_field_reads` | 227,085,094 | 135,093,316 | -40.5% |

- `record_field_reads` runs at 3.75 times Rust's 36.0 million on LLVM.
  The gate is 4 times. Lending alone took it to 152.9 million. The
  runtime work above brought it the rest of the way.
- No other program grows by more than 0.03%. `dict_build_get` gains from
  the set result and the short name tests.
- Every program prints the same output as before.

The main costs in `record_field_reads` on LLVM, inclusive, in millions:

| Function | Before | After |
| --- | ---: | ---: |
| Lookup into the list's dict | 62.6 | 20.8 |
| Release of the last owned value | 40.9 | 13.0 |
| Type cell tags | 22.8 | 15.1 |
| `map[key] = value` | 20.6 | 17.8 |
| `aura_direct_enum_variant` | 18.9 | 19.1 |
| Variant tests | 15.1 | 11.6 |
| `int64` payload reads | 6.5 | 6.0 |
| Argument buffers | 6.1 | 3.6 |

- A lookup now costs about 415 instructions in all: 208 for the lookup,
  116 for the variant test, and 60 for the payload, plus generated code.

### Sizes

Stripped executables from `scripts/check-native-binaries.py`, in bytes.
Before is the 10f build. The gate passes on both code generators.

| Subject | Cranelift before | Cranelift after | LLVM before | LLVM after |
| --- | ---: | ---: | ---: | ---: |
| Hello world | 982,472 | 982,472 | 982,472 | 982,472 |
| Reference agent | 1,878,824 | 1,878,824 | 1,894,808 | 1,894,808 |
| JSON | 1,469,320 | 1,473,416 | 1,477,432 | 1,477,432 |
| TLS | 3,283,048 | 3,283,048 | 3,299,176 | 3,299,176 |
| WebSocket | 3,608,760 | 3,608,760 | 3,608,584 | 3,608,584 |

- Only the Cranelift JSON program grows, by one 4,096-byte page. The new
  helpers add a little text, and the section's padding takes it up in
  the others.

### Startup

`aura run --backend direct examples/basics/hello_world.au` with a warm
cache, median of 15 runs, in milliseconds. Each cell is two sets of
runs. The second set ran the after build first. The second table counts
the built executable's instructions under callgrind, median of 5 runs.

| Code generator | Before | After |
| --- | ---: | ---: |
| Cranelift | 26.1, 25.6, 27.5, 24.7 | 24.4, 27.0, 24.3, 26.6 |
| LLVM | 25.5, 24.5, 26.1, 27.8 | 28.6, 33.1, 24.9, 25.8 |

| Code generator | Before | After |
| --- | ---: | ---: |
| Cranelift | 539,512 | 537,516 |
| LLVM | 538,893 | 536,286 |

- The first set put the after LLVM build at 28.6 and 33.1 ms. The second
  set, which ran it first, put it at 24.9 and 25.8 ms, so the gap is
  noise. The instruction counts of the two builds overlap from run to
  run. Hello world makes no lookup.

## Verification

- `cargo fmt --check`, `npm run check:clippy`, `npm run check:clippy:llvm`,
  and `scripts/check-hygiene.sh` pass. `aura_direct_arg_buffer_new`
  allows `clippy::slow_vector_initialization`, because the form clippy
  suggests is the `calloc` call this step removes.
- The whole workspace passes on Cranelift, except
  `package_io_helpers_cover_local_error_paths`, which fails only because
  the container runs as root.
  `loop_backedge_safepoints_prevent_socket_readiness_starvation` failed
  once in the suite run and passed when run again, as in 10e and 10f.
- With the `llvm` feature and `AURA_NATIVE_CODEGEN=llvm`, every compiler
  and CLI test target passes, with the same exceptions as 10f: the eleven
  emitter tests that pin Cranelift's object shape, the root-only test,
  and `native_run_cache_keys_on_the_code_generator`.
- `npm run test:llvm` passes, apart from the root-only test, and the
  repeated-build test passes.
- `npm run test:backend-parity` passes in 1067 s, and
  `npm run test:backend-parity:llvm` passes in 1056 s.
- The coverage run passes its floors, with 96.55% of lines, 97.46% of
  functions, and 95.49% of regions. One CLI test failed in the first run
  because a build directory was removed under it. It passed when run
  again with the same instrumentation, and the report includes it.
- Error messages are unchanged. The `_lent` helpers raise the same texts
  as the `_payload` helpers, and a value whose copy could fail is copied
  as before.
- New runtime tests:
  - `lookups_lend_values_whose_copy_cannot_fail` checks that a leaf and a
    variant of leaves are lent, that lending copies nothing, that a
    variant holding a list is copied, and that `Lookup.Missing` is null.
  - `lent_lookups_raise_the_errors_of_the_copying_lookups` checks that
    the list forms of the `_lent` and `_payload` lookups raise the same
    text for a value that is not a list, an index out of range, and an
    element that is not a dict.
  - `lookups_copy_from_a_map_another_thread_may_reach` checks that a
    shared dict always copies.
  - `variant_tests_compare_short_names_by_words` checks the word
    compares on names of every short length, including tagged names.
  - `direct_enum_variants_share_the_names_of_their_literals` now also
    checks that two literals in one slot each keep their name. It fails
    with the old direct-mapped table.
- New compiler tests:
  - `lookups_only_read_lend_their_found_value` checks that a list of
    dicts and an `int64`-keyed dict reference the `_lent` lookups and
    readers and not the `_payload` lookups.
  - `lookups_copy_their_found_value_when_the_dict_can_change` checks
    that a set inside the match, a call that takes the value, and an
    owned match never reference a `_lent` lookup.
  - Both check presence and absence, not relocation counts.
- The new run-pass fixture `lent_lookups.au` reads `int64` and `bool`
  payloads from a list of dicts and from an `int64`-keyed dict. It
  covers `Lookup.Missing`, a value that holds a list, a set between the
  lookup and its use, and a value handed to a function. Its output
  matches the 10f build and the MIR interpreter, and it passes in parity
  on both code generators.

## Follow-Up

- A lookup that is the dict's last use is copied, because the MIR
  releases the dict right after it. Moving that release past the found
  value's last read would let those lookups lend too.
- A found value handed to a function is copied. Lending it would need the
  callee to promise it neither keeps nor changes the value, which is
  what MIR's `Lend` does for places.
- A `str` payload, or a value with a list or dict inside, is still
  copied. A lent `str` would need the view rules of 10c.
- Building the dicts now dominates the program: variant construction,
  the set, and the type tag after each set.
