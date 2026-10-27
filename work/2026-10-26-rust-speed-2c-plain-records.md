# Rust Speed Step 2c: Plain Records In Their Layouts

## Goal

Step 2c of Phase 2 of the Rust-speed plan John approved on 2026-10-26:
keep `copy class` values in their typed layouts, so reading one from a
list or a field is loads instead of copies into new boxes. The design for
all of Phase 2 is in `architecture_docs/24-typed-layouts-design-note.md`,
section "Plain Records".

## Changes

- A plain record is a class whose fields are all scalars the emitter keeps
  as one word (`int32`, `int64`, `uint64`, `float32`, `float64`, `bool`,
  `None`) or other plain records. Its layout holds no handle and no heap
  block.
- The runtime registers plain layouts (`register_plain_layout`) under an
  access code of at least `FIRST_PLAIN_ACCESS`. A list whose elements all
  fit a registered layout keeps them in `ListElements::Typed`, one element
  per stride of 8-byte words, and leaves the form for `Value`s when an
  element does not fit. The list view carries the layout's access code.
- Rendering, hashing, equality, copies, and shuffles of a typed list match
  the same list of `Value`s.
- `native_codegen_plain.rs` gives each record type the emitter uses a cell
  and an access code, and the main wrapper registers every layout before
  the first call (`aura_direct_register_plain_layouts`).
- List indexing, `append`, and indexed assignment of records load and
  store the fields at their layout offsets when the list keeps the records
  in place and is confined to the thread. Assignment also needs storage
  no other list shares, and `append` needs room. Every other case calls
  `aura_direct_vec_read_plain`, `_push_plain`, or `_set_plain` with the
  record's bytes in a stack slot. The slot holds a whole stride with its
  padding zeroed, so a list that keeps that layout copies the words as they
  are, and any other list builds the value from them.
- Boxing a record (`aura_direct_box_plain`), reading one out of a box
  (`aura_direct_unbox_plain`), and reading a record field of a boxed
  instance (`aura_direct_instance_read_plain`) each take one call. The
  field read borrows the field instead of cloning it.
- The inline `len` accepts typed lists.
- A record with a union field keeps the boxed paths. Tuples, records in
  classes that own handles, and records in enum payloads move inline with
  their owners in steps 2e and 2f. The benchmarks' tuples hold `str`.
- The typed form made LLVM stop inlining several hot runtime helpers, which
  cost 3% to 10% on programs that hold no records. The typed form's list
  view and appended-element adoption now sit in their own out-of-line
  functions, so the scalar forms' paths stay as small as before. So does
  the typed form's element read. A runtime write refreshes the list view
  itself when it returns, and the drop guard only when it unwinds, because
  LLVM stopped inlining the guard, and the type tag path is inlined into
  both its callers now that 2c adds a second one. A layout's
  stride is a `NonZeroUsize`, the typed storage is boxed so a list stays
  one pointer and a tag, and an empty list of values keeps its storage when
  the first value arrives.
- `pop()` with no index on a list of scalars runs in native code: when the
  list is confined and has room, generated code reads the last element and
  lowers the length, and the next runtime access drops the popped elements.
  Any other list calls `aura_direct_vec_pop_scalar`, which returns the
  element's bits with no box. This took back the extra 3.5% `bytecode_vm`
  showed after the typed form, and more (see below).
- The list iterator implements `nth`, so `iter().skip(n)` steps over
  elements without building them. Step 1e (PR #146) makes the same change;
  the rebase keeps one copy.

## Verification

- Runtime tests: typed lists in every edit sequence of the list model test,
  forms, hashing and rendering parity, shuffle and capacity, and every
  record helper with its error paths.
- Emitter tests: record lists call the record helpers and no per-element
  box helper; a record with a union field keeps the boxed paths.
- New fixtures: `run-pass/copy_class_lists_in_place.au` and
  `run-fail/copy_class_list_index_out_of_bounds.au`.

## Measurements

Callgrind instruction counts, Linux container, release builds, cross-language
suite at the check size. Before is `main` at `addebcca` (steps 2b and 1b),
after adds this step. Step 1c (PR #154) is not in either build.

| Program | Before | After | Change | After vs Rust |
| --- | ---: | ---: | ---: | ---: |
| `bytecode_vm` | 48.9M | 6.9M | -85.9% | 2.6x |
| `graph_paths` | 561.5M | 80.4M | -85.7% | 29.5x |
| `raytracer` | 464.1M | 94.0M | -79.7% | 83.8x |
| `http_api` | 32.3M | 29.0M | -10.1% | 4.5x |
| `binary_trees` | 16.8M | 15.2M | -9.7% | 10.3x |
| `astar_pathfinding` | 17.4M | 15.9M | -8.6% | 14.4x |
| `sql_engine` | 5,034.2M | 4,939.4M | -1.9% | 104.3x |

The other programs stay within 3%. The largest increases are
`lzw_compress` +2.2%, `text_diff` +1.8%, and `regex_match` +1.4%.
`task_pool` runs tasks on several threads, so its count moves with
scheduling: six runs of each build ranged from 41.1M to 44.8M before and
41.8M to 46.2M after, a mean increase of about 2%.

Operation probes (PR #152), instructions per operation at a count of
100,000:

| Probe | Before | After |
| --- | ---: | ---: |
| `class_field_read` (`ps[i].x` on a `list[Point]`) | 4,855.7 | 23.0 |
| `list_push` (a `float64` and a `Point`) | 4,579.6 | 30.4 |
| `field_write` (a class holding `str`) | 5,396.8 | 5,441.8 |

Gates: `raytracer` and `astar_pathfinding` at most 3x Rust are not met. Most
of what `raytracer` still runs is the `Tracer` instance's fields
(`self.rays += 1` takes and puts back a field of a boxed instance) and
runtime call frames for the recursive `trace`. Classes with handles become
native structs in step 2e. `astar_pathfinding` holds only `int64` lists,
so this step changes it little beyond the inline `pop()`.
