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
  functions, so the scalar forms' paths stay as small as before. A layout's
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
suite at the check size. Before is steps 2b, 1b, and 1c together; after adds
this step.

| Program | Before | After | Change | After vs Rust |
| --- | ---: | ---: | ---: | ---: |
| `bytecode_vm` | 49.6M | 6.9M | -86.1% | 2.6x |
| `graph_paths` | 563.5M | 81.0M | -85.6% | 29.7x |
| `raytracer` | 457.2M | 78.9M | -82.7% | 70.4x |
| `astar_pathfinding` | 17.6M | 16.4M | -6.6% | 14.9x |
| `sql_engine` | 2,927.1M | 2,809.7M | -4.0% | 59.3x |

The other 25 programs stay within 3%. The largest increases are
`markdown_render` +2.2%, `nbody` +2.0%, and `lzw_compress` +1.9%.

Measured on step 2b alone, before the rebase onto Phase 1, `mini_browser`
dropped 36.3%. Step 1c makes the same reads cheap on its own, so on the
rebased base it stays within 1%.

Operation probes (PR #152), instructions per operation at a count of 10,000,
measured on step 2b alone:

| Probe | Before | After |
| --- | ---: | ---: |
| `class_field_read` (`ps[i].x` on a `list[Point]`) | 4,925.7 | 23.3 |
| `list_push` (a `float64` and a `Point`) | 5,308.9 | 1,034.7 |
| `field_write` (a class holding `str`) | 5,405.0 | 5,437.1 |

Gates: `raytracer` and `astar_pathfinding` at most 3x Rust are not met. Most
of what `raytracer` still runs is the `Tracer` instance's fields
(`self.rays += 1` takes and puts back a field of a boxed instance) and
runtime call frames for the recursive `trace`. Classes with handles become
native structs in step 2e. `astar_pathfinding` holds only `int64` lists,
so this step changes it little beyond the inline `pop()`.
