# Batch 7 Step 10d: Lookups And Variants

## Goal

Implement part 10d of the step 10 plan. Profile `record_field_reads` and
`dict_build_get` on LLVM first, then cut the lookup, variant, tag, and
formatting work that the profiles show. The gates are `dict_build_get` at
or under 4 times Rust, `record_field_reads` at least 2 times better than
after 10b, and byte-identical error messages.

## Profile

Callgrind on the LLVM build of `1ff9ac4e`, which is 10b. Per lookup in
`record_field_reads`, before any change:

- `doc.get("age")` built a `Lookup.Found` around a copy of the found
  value, tagged it, and boxed it.
- The match compared the `Lookup` and `Found` names, then copied the
  payload out into a second box.
- `aura_direct_unbox_int64` copied the whole value before it read the
  integer.
- Each copy of a `json.Value` copied its two names, because
  `EnumVariantValue` held them as `String`.
- `aura_direct_tag_value_type` hashed and compared its descriptor on
  every call, then took the value's lock even when the tag changed nothing
  but the name.

`dict_build_get` spent about 35% of its run in `aura_direct_format_begin`
and `aura_direct_format_push_i64` for `f"key-{j}"`. The second call grew
the buffer the first had sized for the literal alone.

## Design

### Lookup slots

- `native_codegen_lookups.rs` finds the slots. A slot is a compiler
  temporary that only `dict` `get` assigns, of type `Lookup[...]`, that
  is not a parameter. It is otherwise only matched on `Lookup.Found`,
  `Lookup.Missing`, or a wildcard, unpacked at index 0, and released.
- A slot may be unpacked in one place at most. That place must not be
  reachable again without passing a new `get`, because two copies of one
  payload would share one box.
- A slot holds the found value's own box, or null for `Lookup.Missing`.
  The `get` helpers gain `_payload` forms that return it:
  `aura_direct_map_get_payload`, `aura_direct_map_get_str_payload`, and
  the two borrowed list element forms.
- A `get` that compiles through a path that builds a `Lookup` converts it
  with `aura_direct_lookup_payload`.
- The match on a slot compares the word with zero. The unpack retains the
  box, or takes it for a move. No tag is emitted for a slot.

### Scalar payloads

- A payload assigned to an `int32`, `int64`, `uint64`, or `bool` local is
  read in place by `aura_direct_variant_payload_i64`, `_int64`, `_u64`,
  and `_bool`. Each one finds the payload with the same code as
  `aura_direct_variant_payload`, then unboxes it with the same code as the
  matching unbox, so its errors are the same text.
- The unbox exports read the value through a borrow instead of copying it.

### Names

- `RuntimeName` replaces the two `String` fields of `EnumVariantValue`.
  It holds a `&'static str` or an `Arc<str>`, so a copy of a value shares
  its names. It compares, hashes, orders, and prints as its text.
- `aura_direct_enum_variant` looks each name literal up in a per-thread
  table of 64 entries, keyed by the literal's address and length. A hit
  still compares the bytes. A loop that builds variants decodes and
  allocates each name once.

### Tags and matches

- Each module defines one writable word per type descriptor, named
  `aura_tag_cell_N`. Generated code passes it to the new
  `aura_direct_tag_value_type_cell`. The first call decodes the descriptor
  and stores the tag there, and later calls read it back. The cell keeps
  its own reference to the tag for the life of the process.
- A tag whose descriptor is a plain nominal type, such as `json.Value`,
  changes nothing in a value but its name, except in a channel or a task.
  The runtime now records the name and returns without the value's write
  path.
- `aura_direct_variant_matches` compares the enum name with the expected
  name and checks the byte after it for the separator, instead of
  searching the whole name first.

### f-strings

- An f-string whose first part after the leading literal is an `int64`
  starts with `aura_direct_format_begin_i64`. It writes the literal and
  the digits into one buffer, sized for the later parts as well.

## Measurements

Callgrind instructions for one whole run, startup included, from release
builds. Before is `1ff9ac4e`. Each later column adds one change:

- Slots: lookup slots.
- Scalars: scalar payloads read in place, and unboxing without a copy.
- Names: `RuntimeName`.
- After: name literals, tag cells, the nominal tag path, the match check,
  and the fused f-string start.

| Code generator | Program | Before | Slots | Scalars | Names | After | Change |
| --- | --- | ---: | ---: | ---: | ---: | ---: | ---: |
| Cranelift | `fib(30)` | 210,612,946 | 210,609,926 | 210,616,612 | 210,615,071 | 210,611,394 | 0.0% |
| Cranelift | `int_list_sum` | 73,113,381 | 73,113,925 | 73,116,513 | 73,113,229 | 73,117,476 | 0.0% |
| Cranelift | `string_list_scan` | 145,125,500 | 145,122,514 | 145,123,862 | 145,127,393 | 142,866,642 | -1.6% |
| Cranelift | `dict_build_get` | 785,848,027 | 489,627,508 | 474,624,033 | 474,617,026 | 424,982,491 | -45.9% |
| Cranelift | `set_insert_contains` | 141,886,611 | 141,903,475 | 141,913,052 | 141,900,905 | 140,470,471 | -1.0% |
| Cranelift | `record_field_reads` | 588,291,729 | 367,062,273 | 325,058,346 | 297,346,711 | 252,171,337 | -57.1% |
| LLVM | `fib(30)` | 115,028,508 | 115,030,654 | 115,028,610 | 115,032,615 | 115,022,229 | 0.0% |
| LLVM | `int_list_sum` | 42,213,539 | 42,218,447 | 42,219,012 | 42,214,695 | 42,217,222 | 0.0% |
| LLVM | `string_list_scan` | 130,919,380 | 130,916,678 | 130,917,760 | 130,912,965 | 128,685,097 | -1.7% |
| LLVM | `dict_build_get` | 764,796,663 | 462,390,569 | 447,378,912 | 447,393,176 | 397,756,448 | -48.0% |
| LLVM | `set_insert_contains` | 136,521,985 | 136,530,964 | 136,513,207 | 136,536,808 | 135,077,314 | -1.1% |
| LLVM | `record_field_reads` | 570,593,510 | 345,837,785 | 306,136,830 | 278,433,150 | 233,275,433 | -59.1% |

- `dict_build_get` runs at 2.2 times Rust's 179.0 million on LLVM. The
  gate is 4 times.
- `record_field_reads` runs 2.45 times fewer instructions than after 10b
  on LLVM. The gate is 2 times, or 285 million.
- Lookup slots alone remove 40% of `dict_build_get` and 39% of
  `record_field_reads`. Building, tagging, matching, and copying the
  `Lookup` cost more than the lookup itself.
- `RuntimeName` alone made each variant built from literals slower. Each
  name became an `Arc<str>` built from a fresh `String`, about 400
  instructions per name. The literal table removes most of that:
  `aura_direct_enum_variant` drops from 40.6 million to 18.7 million
  instructions over its 20,000 calls.
- After 10d, a lookup in `record_field_reads` still copies the found
  `json.Value` into a new box, about 410 instructions, and frees it at the
  end of the arm, about 330. The `dict` stores values inline, so the copy
  stays until a lookup can borrow its result.
- The tag call still costs about 130 instructions. Most of it is the
  runtime boundary and the value's access check.

### Sizes

Stripped executables from `scripts/check-native-binaries.py`, in bytes.
The gate passes before and after on both code generators.

| Subject | Cranelift before | Cranelift after | LLVM before | LLVM after |
| --- | ---: | ---: | ---: | ---: |
| Hello world | 982,488 | 982,488 | 982,488 | 982,488 |
| Reference agent | 1,874,992 | 1,882,824 | 1,891,328 | 1,898,808 |
| JSON | 1,468,904 | 1,473,320 | 1,477,096 | 1,473,240 |
| TLS | 3,271,520 | 3,282,960 | 3,287,840 | 3,299,088 |
| WebSocket | 3,601,416 | 3,608,664 | 3,601,352 | 3,608,488 |

- The new runtime exports add up to 11 KB.

### Startup

`aura run --backend direct examples/basics/hello_world.au` with a warm
cache, median of 15 runs, in milliseconds. Each cell is two separate
sets of runs.

| Code generator | Before | After |
| --- | ---: | ---: |
| Cranelift | 25.7, 24.8 | 23.8, 23.6 |
| LLVM | 23.5, 23.5 | 23.2, 22.3 |

- The differences are within this container's noise.

## Verification

- `cargo fmt --check`, `npm run check:clippy`, `npm run check:clippy:llvm`,
  and `scripts/check-hygiene.sh` pass.
- The whole workspace passes on Cranelift, except
  `package_io_helpers_cover_local_error_paths`, which fails only because
  the container runs as root.
- With the `llvm` feature and `AURA_NATIVE_CODEGEN=llvm`, every compiler
  and CLI test target passes, with the same exceptions as 10b: the eleven
  emitter tests that pin Cranelift's object shape, the root-only test,
  and `native_run_cache_keys_on_the_code_generator`.
- `npm run test:llvm` passes, apart from the root-only test.
- `npm run test:backend-parity` passes in 970 s, and
  `npm run test:backend-parity:llvm` passes in 983 s.
- Error messages are unchanged. Every existing test that pins a runtime
  message passes. New runtime tests check that each payload form of a
  `get` fails with the same text as its `Lookup` form, and that each
  scalar payload read fails with the same text as the boxed payload and
  its unbox.
- New runtime tests cover the payload `get` forms against the `Lookup`
  forms for found and missing keys, `aura_direct_lookup_payload` and its
  errors, the scalar payload reads, shared enum names and a reused name
  buffer, the match check against the separator search, tag cells, the
  nominal tag path, the fused f-string start, and `RuntimeName`.
- New compiler tests check that a lookup match references the payload
  helper and the tag cell, and none of `aura_direct_map_get_str`,
  `aura_direct_variant_matches`, `aura_direct_variant_payload`, or
  `aura_direct_tag_value_type`. Others check the scalar payload reads,
  the fused f-string start, and that the slot analysis rejects two unpack
  sites, an unpack in a loop, any other read, any other assignment, a
  parameter, a match on another variant, and a later payload index.
- Coverage is 96.57% of lines, 97.47% of functions, and 95.52% of
  regions, above the floors. `native_codegen_lookups.rs` covers every
  line. The run merged profiles per binary, as in 10b.

## Follow-Up

- A lookup whose found value is only matched and read as a scalar could
  borrow the value in the `dict` instead of copying it into a box.
- The tag call could skip the runtime boundary for a fresh box that no
  other code has seen.
