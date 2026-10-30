# Rust Speed Step 5d: Enums

Date: 2026-10-30

## Problem

`binary_trees` ran in about 8.7x Rust's time at depth 14. At depth 12 it
took 718M instructions for about 674,000 tree nodes, roughly 1,065 per
node, against Rust's single allocation and free:

- every node, `Tree.Leaf` included, was a new 208-byte box from the
  allocator plus, for `Tree.Node`, a separate 16-byte payload block from
  `calloc`, and a ledger entry;
- `check(tree)` tagged its argument on every call, because a new box
  recorded no shape for the 5a fast path to compare;
- freeing a tree walked every box through the general drop path, and the
  per-thread box cache (256 boxes) emptied long before a tree of
  thousands of nodes was rebuilt.

## Changes

- **Shared payload-free variants.** `aura_direct_enum_new_typed` builds a
  variant with no payloads once per layout and variant, freezes it, and
  keeps its typed view (it has no payload box generated code could reach
  through the view). Every later build returns that box. Frozen boxes
  take no counts and no ledger entry. They are freed when the program
  ends, with the module constants, so the leak checks still see every box
  freed.
- **Fields and payloads hold frozen boxes as they are.**
  `adopt_field_box` keeps a frozen box instead of copying it, and
  `owned_element_box` hands it out as it is. A frozen box counts as
  shared, so a change through the field still copies first.
- **Tags at construction.** A typed variant's box records its layout's
  registered type tag as its shape (`EnumLayout::shape_tag`), so the 5a
  fast path skips tagging it.
- **Block pool.** Payload and field blocks of up to 128 bytes come from a
  per-thread cache of freed blocks in 16-byte size classes, up to 1,024 a
  class (`native_block_pool.rs`). The box cache keeps 1,024 boxes instead
  of 256.

The plan described this step as inline enum values of up to four words
and thin refcounted blocks for recursive nodes. That representation would
change how every runtime helper receives an enum value. The changes above
remove the same allocations for `binary_trees` without changing the value
representation, so the native cache format stays at v10.

## Measurements

`binary_trees` at depth 12 (callgrind): 718.2M instructions on 5c, 375.9M
with this step (-47.7%). Wall time at depth 14 on the Linux container
(three runs): Rust 0.072 s, 5c 0.63 s (8.7x), this step 0.27-0.29 s
(about 3.8x). The gate (at most 4x) is met.

Check size (callgrind), 5c release prefix against this step's:

| Program | 5c | 5d | Change |
| --- | ---: | ---: | ---: |
| binary_trees | 4,268,578 | 2,853,822 | -33.14% |
| mini_browser | 361,120,045 | 349,894,284 | -3.11% |
| sql_engine | 451,251,404 | 442,072,323 | -2.03% |
| spreadsheet | 11,073,740 | 11,001,486 | -0.65% |
| order_book | 6,546,927 | 6,511,239 | -0.55% |

Every other program measured changes by less than 0.2%, and every output
matches.

The second gate, `interpreter` at least 20% fewer instructions, is
missed (+0.03%). Its enums are already in their typed layouts; its cost
is variable lookups in `dict[str, ...]` environments (`map_index`, value
and text compares, about 28%).
