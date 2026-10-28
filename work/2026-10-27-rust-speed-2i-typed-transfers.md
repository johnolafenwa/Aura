# Rust Speed Step 2i: Typed Values Across Tasks And Queues

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

## Part 3: Moved Task Arguments Move

`start_direct_task_call` called `share_direct_value` on every task
argument: the box left its task's ledger, got a lock, and lost its list
view, so every later access on either side took the lock, even when the
caller had moved the value into the task and never saw it again.

- `compile_start_task` stores an argument the caller moved in, or a box
  made for the call, with `aura_direct_arg_buffer_store_owned`, so its
  one reference passes to the buffer. Any other argument is stored with
  a new reference, as before.
- `direct_task_arg_hands_over` (`native_runtime.rs`) is true when the
  buffer's reference is the box's only one and no ledger links it. Such
  an argument is not shared.
- `claim_direct_task_args` treats an argument without a lock as handed
  over: in debug builds it records the child's thread as the box's
  thread, and it links the box into the child's ledger, so generated
  code retains and releases it in place. Every other argument is counted
  by address, as before.

Probe (2,000 tasks, each summing an `Order` with 50 `Point`s or a list of
100 numbers moved into it): 489.2M instructions before, 99.8M after. In
the part 2 probe, a call of `sum_order` drops from about 15,400
instructions to 3,600. No program of the 30 changes by more than 0.4%.

## Part 4: Module Constants Are Frozen

A native read of a module constant called `aura_direct_module_constant`,
which hashed the constant's name, looked it up in a registry behind a
mutex, and retained the value; the read site then tagged the value with
its type again. A constant read from more than one task was shared, so
every access took its lock and missed the inline list reads.

- `freeze_direct_value` (`native_runtime.rs`) runs once, after the
  initializer built the constant and gave it its type tag. It unlinks the
  box from every ledger, points its lock at the static
  `FROZEN_VALUE_LOCK`, and sets its reference count to `1 << 30`.
  `retain_ref_count` and `release_ref_count` leave a count that high as
  it is, so retains and releases need no extra check. Releasing through
  a ledger, sharing, claiming, retaining through
  `aura_direct_retain_value`, and tagging pass a frozen box by; a read
  takes no lock, and a write is the runtime error "a module constant
  cannot change". The checker already rejects such writes, so it is a
  backstop. With the frozen count as the limit, the old count overflow
  error can no longer happen.
- Every check of "is this value shared" stays true for a frozen value,
  so code that changes or keeps a constant copies it, as it did for a
  shared value. The boxes a frozen value holds have no room for inline
  appends, so nothing below it needs freezing too.
- The list view is kept for lists of scalars and strings, so loops over a
  constant use the inline reads. Lists of boxes and of class or enum
  layouts drop it and take the runtime path.
- `aura_direct_module_constant` takes a per-constant word, a type tag
  cell, and the constant's type. A read loads the word, and only the
  first read, from the main thunk, runs the registry. The read site no
  longer tags the value or coerces it when the types match.
- Frozen constants are freed directly when the program ends.

Probes (release builds, callgrind):

| Probe | Before | After |
| --- | ---: | ---: |
| list and `dict` constants read in loops, main task and two workers | 47.8M | 14.2M |
| 13 kinds of constants read in 8 tasks, copied, changed, compared | 17.4M | 10.4M |

The 30 programs run 0.1% to 1.1% more instructions (median 0.5%). A
release through a ledger checks for a frozen value, which saves a ledger
lookup for a constant but costs every other release 4 instructions, and
tagging checks too, 2 instructions a call. Most of the rest is
`memcmp` in type comparisons taking longer paths with the heap laid out
differently (nbody: +32K of its +55K). The checks keep a frozen box out
of every ledger, which the per-task ledgers need, so they stay.

## Part 2 Measurements

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
- Runtime tests: only a box that the argument buffer alone holds is
  handed over, and a started task finds such an argument unshared and
  linked in its ledger while an argument the starter still holds is
  shared.
- New fixture `run-pass/module_constants_read_across_tasks.au` reads
  13 kinds of constants from 8 tasks, copies and changes them, and
  compares them; `cleanup_leaks.rs` checks the native run frees every
  box. A codegen test checks that a constant read tags nothing, and the
  runtime cache test checks the word, the fixed reference count through
  retain and release, and that a reset empties the word.
- Runtime, compiler, and CLI tests, backend parity, `cargo fmt --check`,
  hygiene, and clippy.
