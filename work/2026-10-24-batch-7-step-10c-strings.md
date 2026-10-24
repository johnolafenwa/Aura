# Batch 7 Step 10c: Strings

## Goal

Implement part 10c of the step 10 plan. Give a `list[str]` a view of
where its elements lie, and compare an element with a literal inline: a
length check, then the bytes. Keep a lean helper for the case without a
literal. The gate is `string_list_scan` at or under 4 times Rust in
instructions on LLVM, with UTF-8 and Unicode behavior unchanged.

## Profile

Callgrind on the LLVM build of `1b3bd984`, which is 10d.
`string_list_scan` compares one million list elements with `"user-7"`:

- `aura_direct_compare_str_literal_at` took about 95 instructions an
  element, 74% of the run. Nearly all of it was the runtime boundary, the
  value access, and the index check. The length differs for 90% of the
  elements, so the bytes were rarely compared.
- The loop itself took about 16 instructions an element.

In the other programs, the `str` work was UTF-8 validation of literal
text. `from_utf8` took about 70 instructions a call for 4-byte literals:
15 million instructions in `dict_build_get`, from the `"key-"` prefix of
`f"key-{j}"`, and 8 million in `record_field_reads`, from the `"age"` key.
`aura_direct_format_begin_i64` also reserved room for the digits and then
checked the reservation again through `append_string_checked`.

## Design

### The `str` view

- `DirectListAccess::Str` is a new list form, code 5. The array codes move
  to 6 through 9, and `LIST_FORMS` grows to 5, so a `Str` list still
  reads its length inline.
- Its view points at one `DirectStrViewEntry` per element, the address
  and length of the element's UTF-8 bytes. `DIRECT_STR_VIEW_ENTRY_BYTES`
  and the two field offsets are public constants, and the list layout
  fingerprint covers them and the new code.
- `VecValue` gains a `StrViewCache`, one word that owns the entries once
  a list builds them. `Value` keeps its 112 bytes and the box its 192,
  because a `dict` is the largest value. A copy of a list starts without
  a view, and the view is not part of the value: equality and debug
  output ignore it.
- `aura_direct_compare_str_literal_at` counts a miss on the list after it
  compares. Once the misses since the last build reach an eighth of the
  length, the runtime records every element and switches the view to
  `Str`. A list that holds anything but `str` keeps `Values`.
- Each build is one pass over the list, and the misses before it pay for
  it. A list that changes between reads costs a constant factor more at
  most, never a pass per read.
- A write refreshes the view to `Values` as before, so generated code
  never reads a stale entry. A shared value's view stays `None`, and the
  build checks that the value is confined.

### Generated code

- `==` and `!=` between a lent `list[str]` element and a literal check the
  access code, then the index against the view's length, then the
  element's length against the literal's.
- When the lengths match, `aura_direct_bytes_equal` compares the bytes. It
  cannot fail, so it skips the runtime boundary. An empty literal needs no
  call.
- Any other list, or an index out of range, calls
  `aura_direct_compare_str_literal_at` for `==`. It reports the same
  diagnostics as before.

### The lean helper and literals

- `aura_direct_compare_values_at` compares an element with a `str` value
  through the literal path's byte comparison instead of the general value
  comparison. Other operands take the general comparison as before.
- `literal_str` checks whether literal bytes are ASCII before it runs the
  full UTF-8 check. Enum names, f-string literals, `dict` keys, and
  `decode_bytes` share it. Invalid bytes fail with the same message.
- `aura_direct_format_begin_i64` appends the digits directly when they fit
  the string limit, since the buffer already has room for them.

## Measurements

Callgrind instructions for one whole run, startup included, from release
builds. Before is `1b3bd984`.

| Code generator | Program | Before | After | Change |
| --- | --- | ---: | ---: | ---: |
| Cranelift | `fib(30)` | 210,617,695 | 210,616,106 | 0.0% |
| Cranelift | `int_list_sum` | 73,118,521 | 73,112,038 | 0.0% |
| Cranelift | `string_list_scan` | 142,858,806 | 57,807,819 | -59.5% |
| Cranelift | `dict_build_get` | 424,972,122 | 403,287,563 | -5.1% |
| Cranelift | `set_insert_contains` | 140,456,141 | 140,469,111 | 0.0% |
| Cranelift | `record_field_reads` | 252,168,531 | 245,590,358 | -2.6% |
| LLVM | `fib(30)` | 115,025,019 | 115,030,190 | 0.0% |
| LLVM | `int_list_sum` | 42,214,595 | 42,215,750 | 0.0% |
| LLVM | `string_list_scan` | 128,675,338 | 32,736,019 | -74.6% |
| LLVM | `dict_build_get` | 397,754,287 | 376,068,275 | -5.5% |
| LLVM | `set_insert_contains` | 135,063,608 | 135,092,104 | 0.0% |
| LLVM | `record_field_reads` | 233,272,043 | 226,694,034 | -2.8% |

- `string_list_scan` runs at 2.5 times Rust's 13.1 million on LLVM. The
  gate is 4 times.
- `dict_build_get` drops 5.5% and `record_field_reads` 2.8% on LLVM,
  from the ASCII check on literals and the direct digit append.
- A first version kept the view in the box. That grew `OpaqueValue` from
  192 to 208 bytes, because `Value` is 16-byte aligned, and cost
  `dict_build_get` 0.5%. The view now lives in the list value instead.
- A first version also reset the miss count on every write. That added a
  load and a branch to every write of every value, 0.7% of
  `int_list_sum`, and the count since the last build gives the same
  bound.
- In `string_list_scan`, the loop and the inline compare now take about
  12 instructions an element in generated code. The 10% of elements whose
  length matches also call `aura_direct_bytes_equal`, which uses
  `memcmp`.

### Sizes

Stripped executables from `scripts/check-native-binaries.py`, in bytes.
The gate passes before and after on both code generators.

| Subject | Cranelift before | Cranelift after | LLVM before | LLVM after |
| --- | ---: | ---: | ---: | ---: |
| Hello world | 982,488 | 982,576 | 982,488 | 982,576 |
| Reference agent | 1,882,824 | 1,874,728 | 1,898,808 | 1,890,712 |
| JSON | 1,473,320 | 1,469,320 | 1,473,240 | 1,473,336 |
| TLS | 3,282,960 | 3,278,952 | 3,299,088 | 3,299,176 |
| WebSocket | 3,608,664 | 3,604,664 | 3,608,488 | 3,608,584 |

### Startup

`aura run --backend direct examples/basics/hello_world.au` with a warm
cache, median of 15 runs, in milliseconds. Each cell is two separate
sets of runs.

| Code generator | Before | After |
| --- | ---: | ---: |
| Cranelift | 26.0, 31.0 | 29.1, 26.6 |
| LLVM | 25.3, 24.5 | 25.2, 25.4 |

- The container was noisy during these runs. A third pair of sets ranged
  from 26.1 to 32.8 ms with no consistent side. Hello world builds no
  `list[str]`, and its binary is 16 bytes smaller.

## Verification

- `cargo fmt --check`, `npm run check:clippy`, `npm run check:clippy:llvm`,
  and `scripts/check-hygiene.sh` pass.
- The whole workspace passes on Cranelift, except
  `package_io_helpers_cover_local_error_paths`, which fails only because
  the container runs as root.
- With the `llvm` feature and `AURA_NATIVE_CODEGEN=llvm`, every compiler
  and CLI test target passes, with the same exceptions as 10d: the eleven
  emitter tests that pin Cranelift's object shape, the root-only test,
  and `native_run_cache_keys_on_the_code_generator`.
- `npm run test:llvm` passes, apart from the root-only test.
- `npm run test:backend-parity` passes in 1059 s, and
  `npm run test:backend-parity:llvm` passes in 1091 s.
- UTF-8 and Unicode behavior is unchanged. Every existing UTF-8, Unicode,
  and string test passes. New tests check that non-ASCII literals decode
  as before and that invalid bytes fail with the same message.
- The new run-pass fixture `str_list_literal_equality.au` compares `str`
  elements with ASCII, empty, and non-ASCII literals and with a `str`
  value, appends between loops, and passes in parity on both code
  generators. Its loops are long enough to build the view.
- New runtime tests cover building the view after enough misses, a write
  switching it back, a list holding another value, a shared list, an
  empty list, the index diagnostic on a miss, `aura_direct_bytes_equal`,
  and an element compared with a `str` value for every comparison
  against the general comparison.
- A new compiler test checks that `==` and `!=` with a literal reference
  `aura_direct_bytes_equal` and keep the runtime compare for the slow
  path. It checks presence, not relocation counts.
- Coverage is 96.57% of lines, 97.46% of functions, and 95.51% of
  regions, above the floors.
  `loop_backedge_safepoints_prevent_socket_readiness_starvation` failed
  once under coverage, as it has before, and passed in the suite runs.
- The branch carries `86f216f9`, the arm64 Mach-O fix to the 10d f-string
  test, so it rebases onto `main` cleanly.

## Follow-Up

- A comparison with a `str` value could read the element from the view
  too, once generated code can read a `str` box's bytes.
- `dict` and `set` keys could keep the same view for lookups by literal.
