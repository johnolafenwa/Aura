# Rust Speed Step 2e Part 1: Class Fields In Place

## Goal

Step 2e of Phase 2 of the Rust-speed plan John approved on 2026-10-26:
keep a class instance's fields in the class's native layout, as a Rust
struct does, so a field read or write is a load or a store. The design for
all of Phase 2 is in `architecture_docs/24-typed-layouts-design-note.md`.
The step plan is in the project files, `rust-speed-plan/phase-2/step-2e-plan.md`.
This note covers part 1, typed instance storage. Part 2 (classes inline in
lists and enum payloads) and part 3 (lent handle-field reads) follow.

The gate: a class field read or write in at most 5 instructions, and
`order_book`, `regex_match`, and `bank_ledger` at most 3x Rust. Before this
step an instance was a map from field names to values in a 192-byte box.
Every `self.count` read cloned the field into a new box, every write boxed
the value and walked the path by name, and every `self.items.append(x)`
took the list out of the map into a new box and put it back after. Those
three runtime calls took 72% of `order_book`, 69% of `regex_match`, and 50%
of `bank_ledger`.

## Changes

- `InstanceValue.fields` is now `InstanceFields`: `Map` (the map, which
  `aura run`, JSON decoding, and other instances the runtime builds keep)
  or `Typed` (`native_class_layouts.rs`). A typed instance keeps one heap
  block in its class's 2a layout. Scalars, plain records, tuples of them,
  and fields that hold other non-recursive classes sit inline. Each `str`,
  collection, enum, union, or runtime object field is an owned
  `ElementBox` the block holds, null while the field is moved out.
  `InstanceFields` reads (`get` returns a `Cow`), iterates in name order,
  writes, takes, and walks paths for both forms, so printing, equality,
  hashing, JSON, copies, and errors see the same fields either way. A cold
  path that needs `&mut Value` of an inline field turns the instance into
  the map form.
- Generated code registers one `ClassLayout` per class it builds, under
  access codes from `FIRST_CLASS_ACCESS` (`0x4000_0000`), so they never
  meet the plain record codes. Generic classes and classes whose fields
  reach the class itself keep the map form.
- A typed instance's box carries the class's access code and the block's
  address in its list view. A shared instance reports no access code, as
  a shared list does, so generated code reads it through the runtime.
- `native_codegen_classes.rs` finds field places whose root is a local of
  a class type with a layout (not a `view` local) and emits:
  - scalar and plain-record reads and writes at any static path through
    inline fields: one access code check, then a load or a store at the
    offset, with the runtime call as the slow path;
  - reads of a handle field: a copy of the field box's value
    (`aura_direct_instance_copy_field`) instead of the path walk;
  - a `Lend` of a handle field to one runtime operation: the field's box
    itself, with no take and no new box. `EndLend` checks the slot still
    holds the box with a count of 1 and otherwise calls the runtime to put
    the value back;
  - a class literal: one call that builds the block and adopts each owned
    field box without copying it.
- A field box never lets its list view run ahead of its list: generated
  code appends to or pops from a list in a field box only through the
  runtime, and typed field reads use the held value, which adopts no
  appended elements.

## Problems Found While Building It

- Copying a typed instance nested in another value walked it as a map and
  panicked. The clone walk now copies a typed instance whole.
- `compile_member_call` drops the receiver place of a lent temporary
  before it reaches the runtime operation, so the check that a receiver is
  a field box missed `books[0].add(7)` style calls. An inline push then
  ran the field box's list view ahead of the list and the appends were
  lost when the list was read back through the element. The check is now a
  flag set for the call.
- Field reads through a `view` local (`view book = books[i]`) took the
  in-place path although the local is a loan, not an instance box, and
  `mini_browser` crashed. The class paths now skip `view` locals.

## Verification

- Runtime tests (`native_class_layouts_tests.rs`): registration under one
  code, which classes keep fields in place, layout cells, typed reads
  matching map reads, writes in place, the box view while confined and
  after sharing, a generated-code-shaped build and write sequence, the
  runtime field helpers on typed instances, and the fall back to the map.
- New fixtures: `run-pass/typed_class_fields_in_place.au` (every field
  kind, nested classes, enums, unions, field writes through every path
  shape, lends to runtime operations and to user functions that reassign
  the parameter, method calls on list elements read back through the list,
  and field reads through `view` locals) and
  `run-pass/typed_class_fields_across_tasks.au` (instances moved into
  tasks and copied there, and tasks that trap while a field is lent).
- Backend parity and the 30-program suite pass.

## Measurements

Callgrind instruction counts, Linux container, release builds,
cross-language suite at the check size. Before is step 2d, after adds this
part.

| Program | Before | After | Change | After vs Rust |
| --- | ---: | ---: | ---: | ---: |
| `order_book` | 114.9M | 19.3M | -83.2% | 25.3x |
| `regex_match` | 1,940.6M | 366.6M | -81.1% | 15.1x |
| `bank_ledger` | 41.7M | 19.3M | -53.8% | 5.5x |
| `raytracer` | 78.1M | 38.4M | -50.8% | |
| `spreadsheet` | 54.4M | 33.4M | -38.6% | |
| `lru_cache` | 426.1M | 276.0M | -35.2% | |
| `sort_records` | 67.1M | 46.9M | -30.1% | |
| `mini_browser` | 1,153.8M | 819.5M | -29.0% | |
| `sql_engine` | 2,616.8M | 1,936.2M | -26.0% | |

The largest increases are `task_pool` (+2.6%), `astar` (+2.3%), and
`graph_paths` (+2.2%), all inside the 3% rule.

Operation probes (instructions per operation, before and after):
`class_field_read` 22.0 and 22.0 (the probe reads a field of a list
element, which part 2 lays out), `list_push` 30.4 and 30.3, and
`field_write` 5,501.8 and 6,770.0. The `field_write` probe writes
`counters[i].count` through the runtime element path, which now walks a
typed instance in each list element. Part 2 keeps classes inline in lists
and writes that field in place.

## Gates Not Met

- The class field gate holds for fields of a local instance (one check, a
  load or a store), but not yet for fields of list elements, which part 2
  covers.
- `order_book` is 25.3x Rust, `regex_match` 15.1x, and `bank_ledger` 5.5x,
  against a 3x target. What is left in them is mostly list element access
  to class elements (part 2), copies of handle fields read into locals
  (part 3), and `str` and dict operations.

## Follow-Ups

- Part 2: classes inline in lists and enum payloads, with per-element drop
  and clone, so `ps[i].x` reads and `counters[i].count += 1` writes in
  place. Element path walks (`direct_element_walk_mut`) handle typed
  instances already; the other `map_mut()` callers turn a typed instance
  into a map, which is correct but slow.
- Part 3: a temporary that only reads a handle field borrows its box.
- A `float32` in a class field still renders widened on both backends;
  the current-limits line stays until both render `float32` digits.
