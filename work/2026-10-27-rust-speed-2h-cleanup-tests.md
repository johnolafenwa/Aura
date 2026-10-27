# Rust Speed Step 2h Part 6: Leak, Failure, And Cancellation Cleanup

## Goal

The step 2h gate asks that leak, failure, and cancellation cleanup tests
pass. The step plan (`rust-speed-plan/phase-2/step-2h-plan.md` in the
project files) asks for tests that cover typed blocks (records in lists,
instance fields, typed variants, typed `Result`) and check that the
allocator's live count returns to where it started.

## Counting What Is Left

Typed layouts keep every heap block inside a box: a list of plain records
is one block its list box owns, and a class kept in its layout keeps each
handle field in an element box its instance box owns. So a native program
that frees every box it allocated frees every block too.

`AURA_RUNTIME_STATS=1` already reported `opaque_boxes`, the boxes a
native program allocated. The line now also reports `freed_boxes`, which
`free_direct_value` counts when the report is requested. The direct
runtime printed the line before it freed the module constants, so even a
program that freed everything else reported its constants as left. The
report now comes after them: a program that frees everything reports as
many `freed_boxes` as `opaque_boxes`.

## Tests

`crates/aura/tests/cleanup_leaks.rs` runs each program on both backends,
checks that their output and exit codes agree, and checks that the native
run freed every box it allocated. The programs hold a list of copy
records, a class with a list field and a `dict[str, list[Point]]` field,
a recursive enum, and `Result` values across frames and tasks.

- `a_finished_program_frees_every_box`: a recursive descent that builds
  values in every frame and returns normally.
- `a_trap_deep_in_calls_frees_every_frame_s_boxes`: the same descent
  stops at an out-of-range list index five frames down (AU4003). Every
  frame's values are freed.
- `failed_and_cancelled_tasks_free_every_box`: a task group runs a task
  that fails an `assert` while it holds values, one waiting on a queue
  and one sleeping, then cancels the group. The waiting task returns from
  its `Cancelled` arm and the sleeping one finishes. Every task's values
  are freed.
- `a_trap_while_tasks_hold_values_frees_every_box`: `main` stops at an
  out-of-range index while two tasks hold values. The runtime cancels and
  joins them, and their values are freed.

## Wider Checks

- Each of the 30 cross-language programs at the check size ends with no
  box left (1,523,899 boxes allocated and freed).
- Each of the 705 `run-pass` fixtures built with a release compiler ends
  with no box left (85,916 boxes).

## Verification

- New `cleanup_leaks` tests; the `representation_stats` CLI test, the
  runtime stats test, and `scripts/test_bench_representation.py` read
  `freed_boxes`.
- Runtime, compiler, and CLI tests, backend parity, `cargo fmt --check`,
  hygiene, and clippy.
