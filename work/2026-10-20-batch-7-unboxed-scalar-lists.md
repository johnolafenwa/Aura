# Batch 7: Unboxed Scalar Lists

## Goal

Store `int64`, `float64`, and `bool` lists without a 112-byte `Value` per
element, as John chose on 2026-10-20. Step 5 left `int_list_sum` bound by
memory: 100,000 boxed elements take 11 MB and do not fit in the processor
cache. This step sits between step 6 and step 7 of the
[Batch 7 design checkpoint](../architecture_docs/22-batch-7-design-checkpoint.md).

## Work Completed

- `crates/aura-runtime/src/list_elements.rs` adds `ListElements`, with the
  forms `Values`, `Int64`, `Float64`, and `Bool`. Each form sits in a
  `SharedVec`, so copy-on-write sharing works as before.
- The form follows the elements. A value is stored as an `int64` scalar
  only when its runtime kind is `int64` and rebuilding it from the bits
  gives the same representation. Any other value moves the list to
  `Values`, and moving between forms loses nothing. An empty list takes the
  form of its first element, and `clear` resets it to `Values`.
- Reads return `Cow<Value>`. Whole-element edits such as `set`, `insert`,
  `swap`, and `reverse` keep the scalar form. An edit through `get_mut` or
  `values_mut` moves the list to `Values`.
- Printing, hashing, equality, cloning, JSON conversion, and spans read
  scalar forms directly.
- `OpaqueValue` keeps its 160-byte box. `ref_count` is an `AtomicU32`, and
  the freed word holds a `DirectListAccess` code that names the list form or
  the `Array` element type. The startup layout fingerprint covers the
  offsets, the codes, and the strides.
- Generated code checks the access code before an inline read. It reads an
  8-byte `int64` or `float64` element, or a 1-byte `bool`, with no box. A
  shared list stores the code for no inline access, so it reads through the
  runtime.
- The MIR interpreter's `place_ref` returns `Cow<Value>`. `place_borrow`
  reports an error when a caller needs a reference to a scalar element.
- A new run-pass fixture, `scalar_list_edits.au`, edits `int64`, `float64`,
  `bool`, nested, and mixed lists with every list method. The interpreter
  and the native backend print the same output.

## Verification

- `list_elements_tests.rs` compares 60 seeds of random edits against a
  plain `Vec<Value>`, and covers each form change, sharing, equality,
  iteration, and the out-of-range panics.
- Native runtime tests read the list view of each form and check that a
  shared list is never read in place.
- Runtime, compiler, and CLI tests pass, apart from
  `package_io_helpers_cover_local_error_paths`, which fails only because the
  container runs as root. The backend parity matrix passes. Clippy and
  `cargo fmt` are clean.
- Callgrind counts whole-process instructions against main at `aea2e17`.
  The first version slowed `string_list_scan` by 4.5%. A `Values` fast path
  in the element accessor and an inlined scalar hash brought each program
  back within 1%.

| Program | Before | After |
| --- | ---: | ---: |
| `int_list_sum` | 78,657,685 | 73,209,689 |
| `string_list_scan` | 366,708,558 | 370,123,743 |
| `record_field_reads` | 1,164,169,187 | 1,169,197,249 |
| `dict_build_get` | 3,070,686,107 | 3,083,658,347 |
| `set_insert_contains` | 351,464,280 | 351,217,106 |

Medians of 11 alternating whole-process runs on the shared Linux container:

| Program | Before | After |
| --- | ---: | ---: |
| `int_list_sum` | 21.0 ms | 9.5 ms |
| `string_list_scan` | 32.5 ms | 31.6 ms |
| `record_field_reads` | 122.0 ms | 122.5 ms |
| `dict_build_get` | 326.3 ms | 316.5 ms |
| `set_insert_contains` | 40.8 ms | 41.7 ms |

`int_list_sum` is now about 4.1 times the 2.3 ms Rust reference, inside the
phase 1 target of 5x.

The native binaries gate failed on Linux. The JSON example grew from
1,493,080 to 1,506,456 bytes, 0.9%, and went over its 1,501,456-byte limit. The
limit was 10% over a baseline recorded before the JSON codecs and the
Batch 7 runtime work, which had already used 9.4% of it. The gate's
baselines and the size table in `docs/manual/performance.md` now record this
change's release sizes on both platforms. The reference agent grew 1.4% on
Linux and 0.2% on macOS.

## Follow-Up

- Integer values of other kinds, such as `int32`, stay boxed.
- A list of `str` values stays boxed. `string_list_scan` is about 12 times
  the Rust reference, against a phase 1 target of 10x.
- Step 7 re-measures every program on the macOS baseline host.
