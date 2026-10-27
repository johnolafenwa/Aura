# Rust Speed Step 2e Parts 3 And 4: Nested Reads, New Lists, Layout Descriptors

## Goal

Parts 3 and 4 of step 2e of the Rust-speed plan (parts 1 and 2 are in
`2026-10-27-rust-speed-2e-class-layouts.md` and
`2026-10-27-rust-speed-2e-element-loans.md`). After part 2, the largest
costs left in `order_book`, `regex_match`, and `bank_ledger` were element
reads of lists held in fields, which still copied the element, and making
new lists: each `[]` or `list[int64]()` was a box, two type tag calls of
about 490 instructions each, and a runtime call for the first append.
Part 4 came from the native-binaries size check: since part 1, every
program with a class linked the layout builder.

## Part 3: Nested Reads And New Lists

- `native_codegen_elements.rs` lends an element of a list that a method
  reads out of a field through `Lend`/`EndLend`, as in
  `self.asks[level][head]`. The element loan used to end at the field's
  `EndLend`. It now lasts until an instruction changes or frees the
  field's place (`keeps_place`), or uses a loan whose borrowed place may
  overlap it (`ends_element_loan`). Loans are tracked by the place they
  borrow, resolved through chains of loans, so a mutable loan of
  `self.bids` elsewhere in the method no longer stops a read of
  `self.asks` from lending. A field that a closure or a task captures is
  never lent from. The borrow checker already rejects a local alias of
  `self.x[...]` across a change to `self`, so these rules are a backstop.
- `aura_direct_vec_empty_tagged(cell, type)` builds a typed empty list in
  one call and tags it, in the storage form of its element type
  (`empty_elements_for`), so the first `append` of an `int64` to a new
  `list[int64]` runs inline. The tag that followed an empty literal's
  assignment is dropped, since the list already carries it.
- `retag_changes` looks at the value first, so a list, set, dict, or tuple
  that already has the tag's element types, and an enum variant that
  already carries the tag's interned enum name, skips the retag.
- A list, dict, or set appended with one reference keeps its box in the
  list (`holds_element_box_value`), as typed instances do since part 2, so
  `self.bids[p].append(x)` loans hit on the first pass instead of calling
  `aura_direct_vec_lend` (about 3,200 instructions) after misses.
  `aura_direct_vec_push_in_place` returns nothing, so an append no longer
  builds and frees a unit box.

## Part 4: Layout Descriptors Carry Offsets

With parts 1 to 3, the native-binaries check failed: the `json` example
grew from 1.55 MB to 1.74 MB and the reference agent from 2.10 MB to
2.17 MB, past their 10% budgets. Most of the growth was `aura_core::layout`'s builder
(cycle detection, generic substitution, the descriptor decoder) and the
hash maps, B-trees, and sorts it instantiates. Registering a class layout
at startup decoded the class and enum definitions and ran the builder.

A descriptor now carries the layouts the compiler computed: for each
layout reached from the type, its type, size, alignment, and kind with
field and payload offsets, numbered so each comes after the layouts it
holds in place. The runtime reads them back, runs `LayoutTable::validate`
(which now also checks that a tag has aligned room), and refuses a
descriptor that holds a later layout in place, so no value holds itself
without a heap pointer between. The design note's descriptor section says
why this keeps the safety the definitions form gave.

## Verification

- Emitter tests: an element of a lent field stays lent past the field's
  `EndLend` across reads, immutable lends, and writes to other fields, and
  is promoted at a write to `self` or the field, a move of a field, a
  release of `self`, a mutable lend of the field, a use of a loan of the
  field, and a task that captures `self`.
- Runtime tests: typed empty lists in every element form and both errors,
  retags skipped for values that already have the shape, appended
  collections keeping their boxes, and malformed layout descriptors.
- `aura-core` tests: descriptors round-trip with the layouts in order, and
  each malformed form is refused.
- The fixture-wide layout test now checks that every type's descriptor
  reads back as the plan's layout.
- New fixture: `run-pass/nested_list_field_reads_lend.au`.
- Backend parity passes.

## Measurements

Callgrind instruction counts, Linux container, release builds,
cross-language suite at the check size. Before is part 2, after adds
part 3. Part 4 changes only what startup registers.

| Program | Before | After | Change | After vs Rust |
| --- | ---: | ---: | ---: | ---: |
| `order_book` | 16.2M | 11.8M | -27.3% | 15.4x |
| `regex_match` | 355.1M | 294.6M | -17.0% | 12.1x |
| `file_io` | 27.4M | 22.9M | -16.2% | 10.5x |
| `lzw_compress` | 5.2M | 4.7M | -10.1% | 5.6x |
| `kv_server` | 82.6M | 75.4M | -8.7% | 9.0x |
| `astar_pathfinding` | 16.2M | 14.8M | -8.6% | 13.5x |
| `markdown_render` | 10.4M | 9.5M | -8.9% | 10.0x |
| `text_diff` | 10.2M | 9.4M | -8.0% | 7.4x |
| `nbody` | 15.7M | 14.6M | -7.2% | 17.8x |
| `word_freq` | 58.3M | 54.8M | -6.0% | 11.2x |
| `spreadsheet` | 31.1M | 29.4M | -5.7% | 17.1x |

The largest increase is `binary_trees` (+0.6%). `http_api` showed +3.2%
in the suite run; three more runs of each build gave 27.4M to 29.7M before
and 27.9M to 28.0M after. `task_pool` gave 44.0M to 45.3M before and
43.1M to 44.0M after.

Native binary sizes on Linux, before and after part 4:
reference agent 2,173,336 to 2,085,616 bytes, `json` 1,735,736 to
1,652,120.

## Gates

The step's gates are not met, and the rest belongs to later steps:

- `field_write` is 28 instructions per write against a gate of about 5
  (part 2 explains the list checks LLVM keeps in the loop).
- `order_book` is 15.4x Rust, `regex_match` 12.1x, and `bank_ledger` 5.4x,
  against 3x. From profiles of the three:
  - call frames and the sink records for `mut` parameters cost about 200
    instructions per call, and entering and leaving a frame about 380, in
    `regex_match`'s `add_state` and `order_book`'s matching loop. Phase 4a
    makes calls cheap.
  - type tags still cost about 100 instructions per call where a value
    crosses into the runtime, mostly comparing and cloning `Type` values
    by their names. Step 2j takes conversions off hot paths.
  - dropping the nested lists of 2,000 orders at exit is about 15% of
    `order_book`. Step 2h frees typed values without boxes.
  - `bank_ledger` spends about a quarter of its instructions building and
    matching `Result` values, which steps 2f and 2g lay out.

## Follow-Ups

- A `float32` in a class field or an enum payload still renders widened on
  both backends. Step 2f fixes both, with one record of `float32` slots
  that both backends read.
