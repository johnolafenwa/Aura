# Rust Speed Step 2f: Enum Payloads In Place

## Goal

Step 2f of Phase 2 of the Rust-speed plan John approved on 2026-10-26:
keep an enum variant as a tag and its payloads in the enum's native
layout, as a Rust enum does, so a `match` tests a number and reads its
payloads where they lie. The design for all of Phase 2 is in
`architecture_docs/24-typed-layouts-design-note.md`. The step plan is in
the project files, `rust-speed-plan/phase-2/step-2f-plan.md`.

The gates: `binary_trees` at most 2x Rust and `interpreter` at most 3x
Rust. Also folded in: a `float32` in a class field or an enum payload
rendered with its `float64` digits on both backends (a line in
`current-limits.md`).

Before this step a variant was a box holding a vector of payload values.
A `match` binding cloned its payload into a new box, and a payload that
was an enum cloned its whole subtree, so `binary_trees`'s `check` copied
every subtree once for each level above it. Each `case` was a runtime
call that compared variant names as strings, and building a variant
looked its names up and tagged its type by name.

## Changes

- `EnumVariantValue.payloads` is now `VariantPayloads`: `Values` (what
  `aura run`, JSON decoding, and variants the runtime builds keep) or
  `Typed` (`native_enum_layouts.rs`). A typed variant keeps one heap block
  in the layout the compiler computed for its enum
  (`LayoutBuilder::boxed_enum_layout`). Scalars and plain records sit
  inline, and every other payload, a recursive one included, is an owned
  `ElementBox` the block holds, null while the payload is moved out. A
  variant with no payloads has no block. `VariantPayloads` reads (`get`
  returns a `Cow`), iterates, compares, takes, and converts both forms,
  so printing, equality, hashing, JSON, copies, and errors see the same
  payloads either way.
- Generated code registers one `EnumLayout` per program-defined enum it
  builds, under access codes from `FIRST_ENUM_ACCESS` (`0x6000_0000`). The
  descriptor carries each variant's payload offsets, its name, and the
  enum's runtime type name, so building a variant needs no name lookup
  and no separate tag call. The compiler's own enums (`Result`, `Lookup`)
  and the library's (`json.Value`) keep the `Values` form.
- A typed variant's box shows the enum's access code, the variant's index,
  and the block's address in its list view while it is confined to its
  thread. A shared variant shows none, so generated code reads it through
  the runtime.
- `native_codegen_enums.rs` emits:
  - a variant built with one call (`aura_direct_enum_new_typed`) that takes
    the layout cell and the variant index, then stores each payload;
  - a `case` as one access code check and one compare of the variant
    index, with the old runtime call as the slow path;
  - a scalar payload binding as a load at its offset, and any other
    binding as a new reference to the payload's box, so a `match` on a
    recursive enum shares the subtree instead of copying it.
- `native_codegen_shapes.rs` finds the boxes that already carry the tag of
  their static type: locals that only plain assignments write, parameters
  of functions that only named calls reach, and results of functions that
  return only through `return`. Coercing such a box to its own type skips
  the tag call. A `float32` list keeps its tag, since tagging narrows it.
- `float32` slots: the compiler builds one record of the `float32` class
  fields and enum payload positions from the MIR (`mir::float32_slots`).
  Native code registers it once from the main wrapper
  (`aura_direct_register_float32_slots`), and the interpreter keeps it per
  run. Rendering a class instance or a variant asks it which numbers to
  spell with `float32` digits, so both backends print `P(v=0.1)`. A field
  or payload whose type is a type parameter still prints widened on both,
  since neither keeps the type argument.
- Copies and drops: a variant whose payloads need no drop copies and frees
  its block as bytes. A variant whose payload boxes all hold leaves copies
  without a walk. Any other typed variant is copied one payload box at a
  time with an explicit frame, so a deep recursive enum copies without
  recursion.
- Size: class and enum layouts share one table of field converters
  (`FIELD_OPS`) that only registration names. Programs register plain,
  class, and enum layouts through three tables (`aura_plain_layouts`,
  `aura_class_layouts`, `aura_enum_layouts`), so a program links only the
  kinds it uses. Class and enum layouts share one registry, which scans a
  vector.

## Problems Found

- Copying a deep typed recursive enum overflowed the stack. The first
  version copied a payload box by cloning its value, which recursed once
  per level. The copy now walks payload boxes with explicit frames
  (`CloneFrame::TypedVariant`); `deep_typed_variants_copy_without_recursion`
  copies a 100,000-level chain.
- The first full suite run had `sql_engine` at +10.7% and `bank_ledger` at
  +4.7% against step 2e: every copy and drop of a typed variant decoded
  each payload through the layout table. The byte-copy path for variants
  that hold no boxes and a flat-copy check that reads payload boxes
  without decoding the block took both back (`sql_engine` -0.2%,
  `bank_ledger` -3.0%).
- The native-binaries check: the reference agent first came out 8 KB over
  its Linux budget (2,114,784 against 2,106,500). The new code was enum
  layout registration, `float32` slot lookups in rendering, typed variant
  copies, and the boxed iterator behind `VariantPayloads::iter`. Splitting
  registration by kind, the shared registry, an index-based payload
  iterator, and `float32` slots kept in sorted vectors brought it to
  2,102,232.
- In the 30-program suite, `kv_server` showed +5.2% against the 2e run.
  Direct callgrind runs of both builds give 84.5M and 85.1M (+0.7%); the
  2e suite run (80.9M) was low.
- Pre-existing bugs, recorded in the project files
  (`rust-speed-plan/found-bugs/`) and not fixed here: the checker
  overflows its stack on a recursive enum whose recursive variant comes
  first; dropping a recursive enum 1,000,000 levels deep overflows the
  stack on `main`; `aura run` walks recursive enums in quadratic time; and
  AU3005's help suggests `items[index].clone()` for an enum element,
  which AU2999 then rejects because enums have no `clone`.

## Verification

- Runtime tests (`native_enum_layouts_tests.rs`): descriptor validation
  and every refusal, building, reading, taking, and writing payloads in
  both forms, moved-out payloads, copies that share and that copy boxes,
  byte copies, deep copies without recursion, conversions to values,
  unsharing boxes before another thread reaches a variant, and
  registration through the enum table only.
- Emitter tests: a program with classes names only the class layout
  table, and one with enums only the enum table.
- `aura-core` tests for boxed enum layouts and their descriptors.
- New fixtures: `run-pass/typed_enum_payloads.au` (a recursive enum built,
  matched, and moved out of with `match own`; variants with scalar, string,
  and list payloads in lists, class fields, sets, and a task) and
  `run-pass/float32_fields_and_payloads.au`.
- Backend parity, `check:cross-language`, the operation probes, the
  runtime and compiler tests, `cargo fmt --check`, hygiene, and clippy
  pass.

## Measurements

Callgrind instruction counts, Linux container, release builds,
cross-language suite at the check size. Before is step 2e.

| Program | Before | After | Change | After vs Rust |
| --- | ---: | ---: | ---: | ---: |
| `binary_trees` | 15.1M | 4.7M | -68.8% | 3.2x |
| `interpreter` | 231.8M | 101.2M | -56.3% | 12.6x |
| `spreadsheet` | 29.3M | 26.3M | -10.1% | 15.4x |
| `lzw_compress` | 4.6M | 4.3M | -8.3% | 5.2x |
| `order_book` | 11.6M | 11.2M | -4.2% | 14.6x |
| `markdown_render` | 9.4M | 9.1M | -3.1% | 9.7x |
| `bank_ledger` | 18.9M | 18.4M | -3.0% | 5.2x |

The largest increase is `pubsub_broker` (+2.2%), apart from `kv_server`
(see above). The `enum_match` operation probe (build a list of shapes,
then `area(shapes[i])` for each) costs 1,099 instructions per operation
instead of 1,930.

Native binary sizes on Linux: `hello` 1,034,280 bytes, reference agent
2,102,232, `json` 1,639,648 (limits 1,070,317, 2,106,500, and 1,657,101).

## Gates

The gates are not met: `binary_trees` is 3.2x Rust against 2x, and
`interpreter` 12.6x against 3x. From profiles of the two:

- `binary_trees` no longer copies subtrees. Per node it still allocates a
  variant box and a payload block, registers the box with the task's
  ledger (8% of the run), and drops both (about 10%), and each call enters
  and leaves a frame. Step 2h keeps values off boxes and cuts retains and
  releases; Phase 4a makes calls cheap.
- `interpreter` spends about a quarter of its instructions writing and
  probing element paths of its dict and list state, 9% tagging values
  where they cross into the runtime (step 2j), 8% in dict lookups, and
  about 14% entering and leaving frames (Phase 4a).
- The `enum_match` probe is 116x Rust because passing `shapes[i]` to a
  function still copies the element. The plan's part 2 lent such elements
  to the call; this step shares payload boxes in `match` instead, and
  element loans for call arguments are left for later.

## Follow-Ups

- Lend a list element passed as a call argument instead of copying it
  (`area(shapes[i])`).
- `Result` and the other compiler-defined enums keep the `Values` form; a
  `Result` in a typed layout is part of step 2g.
