# Rust Speed Step 2a: Layout Plan and Value Converters

## Goal

Step 2a of Phase 2 of the Rust-speed plan John approved on 2026-10-26:
give every concrete type a native layout, and convert values between a
layout and the boxed `Value` the runtime reads. Generated code does not
change. The design for all of Phase 2 is in
`architecture_docs/24-typed-layouts-design-note.md`.

## Changes

- `aura-core` `layout` computes layouts: scalars at their width
  (`Duration` is sixteen bytes), classes and tuples as structs, enums and
  unions as a tag and overlapping variant payloads, runtime objects and
  erased types as one pointer. A class or enum inside a cycle of
  instantiated classes and enums holds the other members of its cycle
  behind a pointer to a heap block. A type that reaches more than 512
  instantiations, or whose type arguments nest more than 32 deep, stays a
  pointer. Both rules depend only on the program, never on the order types
  are laid out in.
- Layout descriptors carry a type and every class and enum definition its
  layout reaches, so the runtime lays a type out again from the descriptor
  alone. `substitute_type_in_context` moved from `sema` to `aura-core`
  for this, unchanged.
- `mir_layout.rs` builds a module's `LayoutPlan`: the layout of every type
  its parameters, locals, results, fields, payloads, and constants use.
- `native_typed_layout.rs` writes, reads, takes, drops, and clones a value
  in a layout. A write checks the whole value first. A handle in a layout
  is a box no ledger links, tagged with its static type. A value read back
  names its integer kinds, tuple element types, union type, and generic
  type arguments, as boxes generated code makes do.
- The runtime's tag application moved into `retag_value`, and box
  allocation without ledger registration into `untracked_boxed_value`, so
  the converters share them.

## Verification

- Layout tests in `aura-core` cover each rule, recursion found from either
  end, the budget in both walk orders, table validation, and descriptor
  round trips.
- A compiler test lowers all 672 run-pass fixtures, builds each plan,
  validates it, and checks that each of the 34,025 distinct types decodes
  from its descriptor and lays out the same way again.
- Runtime converter tests cover every scalar's bytes, struct offsets, tags,
  unions, recursive heap blocks, generic type arguments, refused shapes,
  and that drops and takes free what the layout owns.
- Objects unchanged: every run-pass fixture's object is byte-identical to
  main's.
