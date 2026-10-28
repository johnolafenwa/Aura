# Rust Speed Step 2j: Conversions Only At The Edges

## Goal

Step 2j of Phase 2 of the Rust-speed plan keeps values in their typed
layouts on every hot path: a value is converted to or from a runtime
value only at host calls, FFI, and `aura run`. Its gate is "No conversion
on a hot path of the 30 programs." The step plan
(`rust-speed-plan/phase-2/step-2j-plan.md` in the project files) starts
from a census of every converter run in the 30 cross-language programs
at the check size.

## Census

A scratch build counted every value built from, or written into, a
typed layout's bytes, with the runtime call site. Few programs converted
at all:

| Program | Conversions | Where |
| --- | ---: | --- |
| `sql_engine` | 35,530 | `condition.value`, an enum field kept in a `Condition` block, built as a runtime value on every read |
| `sql_engine` | 10,311 | `match own parser.statement(...)` moving payloads out of typed variants, and `Value` enums stored into class blocks |
| `graph_paths` | 1,388 | `heap.pop()` of a list of `Entry` records |
| `interpreter` | 239 | enum fields of class blocks |
| `mini_browser` | 180 | class fields of class blocks |

The same census found two costs of the same kind that are not
conversions but were larger:

- `aura_direct_vec_push_scalar`, the runtime path of an append to a list
  of scalars (a new empty list, a full list, a list field), built a
  runtime value for every scalar and pushed it through the generic path:
  15.8% of `regex_match`'s instructions.
- Type tags. Coercing a box to its static type calls
  `aura_direct_tag_value_type_cell`, which compares types. It was 31.8%
  of `nbody`, 22.6% of `regex_match`, 17.5% of `interpreter`, and 16.9%
  of `astar_pathfinding`, mostly on boxes that already carried the tag.

## Changes

- Part 1, counting (`native_typed_layout.rs`, `runtime_value.rs`): the
  `AURA_RUNTIME_STATS` report has a `conversions` counter, bumped at
  every entry point that builds a value from a class, tuple, record,
  enum, or union's bytes in place or writes one into them. It counts
  only when the report is requested.
- Part 2, scalar pushes (`push_scalar_bits`): the runtime writes a
  scalar's bits straight into a list of its form, and only a list of
  another form takes the value path.
- Part 3, tags (`native_codegen_shapes.rs`): a local stays shaped (its
  box carries its type's tag) when it is passed for write-back, when a
  mutable loan borrows it, and when it is a mutable parameter of a
  function only named calls reach, since every store into it tags what
  it stores. An argument moved out of a shaped local is shaped, and a
  write-back of a parameter's own type from a function whose parameters
  only plain assignments write (`ProgramShapes::writes_back_tagged`)
  carries its tag, so storing it back tags nothing.
- Part 4, enum and union fields (`LayoutBuilder::boxed_class_layout`,
  `field_lies_inline`): a class that is not Copy keeps an enum or union
  field in a box of its own in its block instead of in the field's bytes.
  A read copies or lends the box as generated code holds an enum value,
  and a write puts the box in the slot, with no conversion either way.
  Copy classes and class fields stay inline, where generated code reads
  their scalars in place. The cell registers the layout's own descriptor
  (`encode_layouts`), which differs from the type's.
- Part 5, payloads and pops: `aura_direct_variant_take_payload` moves a
  typed variant's payload box out as it is (`TypedPayloads::take_box`,
  `owned_element_box`) instead of taking the value out and boxing it
  again; a box something else still holds is copied. `pop()` of a list
  of records (`emit_plain_list_pop`) lowers the list view's length and
  reads the record where it lay, and any other list takes
  `aura_direct_vec_pop_plain`, which moves the record's words.

## Measurements

Callgrind instructions, the 30 cross-language programs at the check
size, Linux container, release builds. Before is step 2i (#173).

| Program | Before | After |
| --- | ---: | ---: |
| `nbody` | 7.3M | 4.9M |
| `astar_pathfinding` | 9.0M | 7.4M |
| `graph_paths` | 68.8M | 58.8M |
| `interpreter` | 83.6M | 73.6M |
| `regex_match` | 286.9M | 254.2M |
| `markdown_render` | 9.0M | 8.3M |
| `order_book` | 10.1M | 9.5M |
| `spreadsheet` | 18.6M | 17.6M |
| `sql_engine` | 1,377.7M | 1,307.5M |
| `bank_ledger` | 13.3M | 12.8M |

No other program changes by more than 3%. `task_pool` varies by a few
percent run to run with its threads; three direct runs average 1.4%
more. By part: scalar pushes take `regex_match` 6.7% and `spreadsheet`
5.5% lower, the tags `nbody` 32.8%, `astar_pathfinding` 17.2%, and
`interpreter` 11.7%, and enum fields, payload moves, and record pops
`graph_paths` 9.7% and `sql_engine` 3.5%.

## Step 2j gate

"No conversion on a hot path of the 30 programs." With the counter, 29
of the 30 programs convert nothing. `mini_browser` converts 20 times per
run: each `Page` stores its `Layout` class field inline, once per page.

## Verification

- New fixture `run-pass/class_fields_in_boxes.au`: enum, union, and class
  fields of a class read, written, matched, and copied with their list;
  payloads moved out of typed variants with `match own`; records popped
  off a list. `cleanup_leaks.rs` checks the native run frees every box.
- Layout test: a class in a box keeps scalars and strings where they
  were and its enum field in a handle. Codegen tests: a class's enum,
  union, and class fields lay out as boxes, boxes, and inline; a record
  pop calls the plain pop and never unboxes; write-backs keep their
  parameters' tags.
- Runtime tests: scalar pushes store bits in a list of their form; a
  typed payload's box moves out as it is and a shared one is copied; a
  record pops off a typed list, a list of values, and traps on an empty
  one; the stats line reports `conversions`.
- `representation_stats.rs`: a program of enum fields, record lists, and
  record pops converts only when it prints a list of records.
- Runtime, compiler, and CLI tests, backend parity, `check:cross-language`,
  `check:operation-probes`, `cargo fmt --check`, hygiene, and clippy.
