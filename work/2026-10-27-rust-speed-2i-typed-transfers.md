# Rust Speed Step 2i Parts 1 And 2: Typed Values Across Tasks And Queues

## Goal

Step 2i of Phase 2 of the Rust-speed plan sends typed values across
tasks and queues by moving their bytes and shares module constants
read-only. Its gate is that the task tests and the multi-thread debug
assertion pass. The step plan (`rust-speed-plan/phase-2/step-2i-plan.md`
in the project files) has four parts: tests, records that cross by their
bytes, task arguments the caller moved, and read-only module constants.
Phase 4 step 4d covers the queue and task hand-off machinery itself.

## How Values Crossed

A queue keeps runtime values. `put` takes the value out of its box,
which is a move when nothing else holds the box, and `get` wraps the
value in a boxed `QueueReceive.Item`. A class instance kept in its
layout (2e), a typed list (2d), and an enum variant kept in its layout
(2f) are runtime values whose contents are already typed, so they
crossed by a move.

A `copy class` value lives in machine words, so crossing needs a box.
`aura_direct_box_plain` converted the record to an instance with a map
of fields, building a `String` key and a runtime value for every field,
and `aura_direct_unbox_plain` checked and converted every field back.
The same happened when a record was the payload of a boxed enum, and
when a list literal built its list: every element was boxed and then
pushed, and the first record pushed into an empty list was converted to
a runtime value to pick the list's form.

A probe that sends 2,000 `Point`s, 2,000 `Order`s (a class with lists of
`Point`s) and 2,000 `Shape`s (an enum with `Point` payloads) through
queues and starts 400 tasks ran 165.1M instructions. `box_plain` alone
was 14% of them, `unbox_plain` 3.5%, and boxed pushes of records into
list literals 12.5%.

## Part 1: Tests

`run-pass/typed_values_cross_tasks.au` sends plain records, tuples of
records, classes with list and dict fields, enums with record payloads,
and `Result` values through queues (`put`, `get`, iteration, `try_put`,
`poll`, `get_or`, `select`), task arguments, closure captures, and task
results, and reads back the values `try_put` and a closed queue's `put`
hand back. `cleanup_leaks.rs` runs it on both backends and checks that
the native run frees every box. The CLI tests build the runtime with
debug assertions, so the check that no unshared value reaches another
thread runs too.

Writing the probes found a compiler bug outside this step: a name bound
in two sibling blocks with different types shares one MIR local. It was
handed on for its own fix.

## Part 2: Records Cross By Their Bytes

- `box_plain_record` (`native_codegen_plain.rs`) boxes a record of a
  class with a layout as an instance kept in that layout: one
  `aura_direct_instance_new_typed` call makes the block, and each leaf is
  stored at its offset (`record_class_leaves` in
  `native_codegen_classes.rs` gives the class record and the leaves).
- `unbox_plain_record` checks the box's access code against the class's
  code and loads the leaves where they lie. Any other box, such as a
  shared one, takes the old call.
- `compile_vec_literal_for_target` pushes scalar elements with
  `aura_direct_vec_push_scalar` and records with
  `aura_direct_vec_push_plain`, with no box. It uses the runtime calls
  rather than the inline append, since the new list may become a field
  box, which never runs its list view ahead of its list.
- `ListElements::push_plain` starts an empty list in the record's layout
  with the record's words.

## Measurements

Callgrind instructions, Linux container, release builds. Before is
step 2h part 9.

| Program | Before | After |
| --- | ---: | ---: |
| probe: 6,000 records and enums through queues, 400 tasks | 165.1M | 122.3M |
| probe: 300 calls that build lists of records with literals | 3.9M | 1.3M |
| probe: 2,000 calls that build lists of numbers and strings | 8.0M | 6.3M |
| `graph_paths` | 73.2M | 68.6M |
| `raytracer` | 24.1M | 22.3M |
| `mini_browser` | 561.9M | 548.9M |
| `sql_engine` | 1,377.5M | 1,371.2M |

No other program of the 30 changes by more than 0.5%; direct runs of
`image_filters` and `lzw_compress`, which moved by that much in the
suite, show no change.

## What Is Left In The Probe

- A task that reads a record argument the caller moved into it spends
  about 14,000 instructions per call on locked field reads: every task
  argument is shared with a lock when the task starts. Part 3.
- Tagging the boxes that queues hand back (`aura_direct_tag_value_type`,
  about 7% of the probe) and the queue's task registration (about 9%)
  belong to the hand-off machinery of step 4d.

## Verification

- New codegen test: list literals of numbers, `bool`s, and records call
  the scalar and record pushes and no boxing helper, and a queued record
  is an instance that holds its bytes. The record list test now expects
  `aura_direct_instance_new_typed` instead of `aura_direct_box_plain`,
  and the `uint64` boxing test boxes its `uint64` in a `dict` literal.
- Runtime test: a record pushed into a list that keeps runtime values is
  appended as a value.
- Runtime, compiler, and CLI tests, backend parity, `cargo fmt --check`,
  hygiene, and clippy.
