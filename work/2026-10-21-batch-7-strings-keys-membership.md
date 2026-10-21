# Batch 7: Strings, Keys, And Membership Without Boxes

## Goal

Remove the per-operation boxes that kept `string_list_scan`,
`dict_build_get`, `set_insert_contains`, and `record_field_reads` above
their phase 1 targets, before the step 7 release. This is the "one more
runtime step" option recommended to John on 2026-10-20. It sits between
step 6a and step 7 of the
[Batch 7 design checkpoint](../architecture_docs/22-batch-7-design-checkpoint.md).

## Work Completed

- F-strings build in one `str` box. `aura_direct_format_begin` starts it
  with the leading literal, and `aura_direct_format_push_str`, `_push_i64`,
  `_push_value`, and `_push_formatted` append in place with the same
  `AU4005` limit as concatenation. MIR no longer renders a number or `bool`
  part into its own temporary string: the sequence point already copies the
  scalar, and rendering it runs no user code.
- `dict[str, V].get("literal")`, on a local map or on a lent list element,
  calls `aura_direct_map_get_str` or `aura_direct_vec_element_map_get_str`.
  `IndexedEntries::position_str` hashes the text as `hash_value` hashes a
  `str` value.
- `x in s` and `s.contains(x)` with an `int64` `x` call
  `aura_direct_set_contains_int64`, which builds the needle on the stack.
- `hash_value` hashes a scalar or `str` key without allocating its walk
  stack.
- A `for` loop that lends each list element emits `IndexInRange` before the
  loan, so native code begins the loan without `aura_direct_vec_check_index`.
- Native code skips `aura_direct_tag_value_type` when the same value got the
  same tag earlier in the block and only retains or releases ran since.
- `aura_direct_variant_matches` compares byte slices instead of decoding
  both names. A tagged enum name is split with `memchr`.
- Copying an enum variant whose payloads are all leaves, such as
  `Lookup.Found(3)` or `json.Value.Int(3)`, skips the JSON tree walk and the
  copy frames.
- Comparing a list element or a value with a `str` literal returns a flag
  and builds no `bool` value.
- A new run-pass fixture, `fstring_part_kinds.au`, covers every part kind,
  `int64` extremes, other integer widths, a custom `Display`, nested
  f-strings, and evaluation order with a mutating call.

## Verification

- Runtime tests compare the builder with `aura_direct_stringify_value` and
  `aura_direct_format_value`, check the `AU4005` and `AU4001` errors, and
  compare the `str`-key and `int64` lookups with the boxed lookups on small
  and hashed collections.
- Callgrind counts whole-process instructions against main at `54e7a0f`.
  Every program prints the same output.

| Program | Before | After |
| --- | ---: | ---: |
| `int_list_sum` | 73,209,658 | 73,212,011 |
| `string_list_scan` | 370,041,432 | 150,970,171 |
| `record_field_reads` | 1,173,189,605 | 692,557,847 |
| `dict_build_get` | 3,080,859,389 | 939,551,832 |
| `set_insert_contains` | 351,223,140 | 148,691,758 |

Medians of 11 alternating whole-process runs on the shared Linux container,
with the Rust reference measured in the same runs:

| Program | Rust | Before | After | After / Rust | Phase 1 target |
| --- | ---: | ---: | ---: | ---: | ---: |
| `int_list_sum` | 2.9 ms | 11.6 ms | 10.8 ms | 3.7x | 5x |
| `string_list_scan` | 2.7 ms | 35.7 ms | 14.1 ms | 5.3x | 10x |
| `record_field_reads` | 7.7 ms | 127.3 ms | 84.4 ms | 11.0x | 8x |
| `dict_build_get` | 20.3 ms | 322.4 ms | 89.3 ms | 4.4x | 8x |
| `set_insert_contains` | 5.3 ms | 39.6 ms | 18.0 ms | 3.4x | 5x |

## Follow-Up

- `record_field_reads` misses its 8x target. Each iteration still copies
  the found `json.Value` into a `Lookup`, copies the payload out again,
  copies both enum names as `String`s, and registers every box in the
  task's ownership ledger.
- Step 7 re-measures every program on the macOS baseline host.
