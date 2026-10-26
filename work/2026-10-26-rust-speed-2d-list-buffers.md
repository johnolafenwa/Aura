# Rust Speed Step 2d: List Buffers And Lent Element Reads

## Goal

Step 2d of Phase 2 of the Rust-speed plan John approved on 2026-10-26:
make `list[T]` a typed buffer for every `T`, emit the common operations
inline, and let element reads lend the element instead of copying it. The
design for all of Phase 2 is in
`architecture_docs/24-typed-layouts-design-note.md`. The step plan is in
the project files, `rust-speed-plan/phase-2/step-2d-plan.md`.

The gate, `heap[i]` in `graph_paths` in at most 10 instructions, was
already met by 2c, which reads a plain record in place in about 9.
Scalars (1b, 1c) and plain records (2c) already sit inline in lists. Every
other element was a `Value` in a `Vec`, and each native read cloned it
into a new 192-byte box. Before this step, that copy and the box's release
took 46% of `interpreter`, 37% of `mini_browser`, 36% of `sort_records`,
and 30% of `sql_engine`.

## Changes

### Part 1: `float32` lists

- `ListElements::Float32` keeps a `list[float32]` in 4-byte elements
  (access code 13). Tagging a list `list[float32]`, or a scalar write of a
  `float32`, moves it to the form. Generated code reads, writes, and
  appends in place, widening and narrowing at the load and store.
- A `list[float32]` prints its elements with `float32` digits, and
  `aura run` rounds a float literal written where a `float32` is
  expected, such as `singles.append(0.1)`, as native code does. This fixes
  `found-bugs/float32-list-literal-append-parity.au`. The plan owner thread
  fixed every other `float32` rendering in PR #156. A `float32` in a class
  field or an enum payload is left for 2e and 2f.

### Part 2: lent element reads

- `ListElements::Boxes` (access code 14, `native_element_boxes.rs`) keeps
  each element in a box of its own (`ElementBox`). The list owns one
  reference to each box and no task ledger links it. Cloning an
  `ElementBox` copies the value into a new box, so two lists never share
  one, and storage two lists share stays copy-on-write as in every form.
- `ElementBox::value_mut` copies the element into a new box first when
  another reference holds the box, so a reference generated code kept
  keeps the value it read. Every mutable accessor of the form goes
  through it. `ElementMut` refreshes the box's list view when it drops.
- `aura_direct_vec_lend(list, index, line, column)` returns the element's
  box with the low bit set when the list is confined, and an owned copy
  otherwise (as `aura_direct_vec_index` does). A `Values` list moves to
  `Boxes` only after native code has copied out as many elements as an
  eighth of its length (`StrView.copied_reads`), so a list read a few
  times keeps the compact form.
- Every `ListElements` method has a `Boxes` arm, and readers that walk
  elements use `value_refs()` and `value_at()`, which cover both forms.
  Sorting, hashing, rendering, JSON conversion, and the element path
  helpers (`direct_element_walk_mut`, now recursive) handle boxes.
- `native_codegen_elements.rs` finds the lent element reads of each
  function: a local assigned once from `list.Index` or
  `list.IndexUnchecked` on a root list, whose element is a handle other
  than `str`, and that is otherwise only read. It may be read, have its
  parts read or consumed (a copy or a reference to the part), be the
  receiver of an op that only reads its receiver (a `str` method, or a
  read of a list, dict, or set), or be matched. A loan, a closure, a task,
  a move, an assignment, or a consume of the whole local copies instead.
  A list that a closure or a task captures, or a mutable loan reaches, is
  never lent from.
- Before each instruction that names the list and does more than read it,
  while a lent local is still to be read, the local takes its own
  reference ("promotion"), so `Release items` after the last index, or a
  write to the list, cannot free or change what the local reads.
- The emitter keeps a flag per lent local. The fast path checks the
  access code, the bounds (unless the loop proved them), and loads the
  box pointer. The slow path calls `aura_direct_vec_lend` and reads the
  flag from the low bit. A release skips a lent local, and a promotion
  retains it and clears the flag.
- `str` elements keep the copying read, which their list view already
  makes cheap. Named bindings of list elements are `AU3005` errors (use
  `view`), so part 2 covers temporaries: `for` loop elements and
  `items[i].field`. `view` and `match` on `items[i]` take element loans,
  which part 3 lends.

## Verification

- Runtime tests: the list model's edit sequence covers lends, writes
  through a lent element, and a snapshot that stays unchanged after an
  edit; box identity, copy-on-write sharing, sort, and conversion of a
  shared list; `ElementBox` copies an element another reference holds
  before a change.
- Emitter tests: a `for` loop lends each element with no promotion; a
  released list gives its live element a reference first; keeping,
  writing, moving, loaning, or writing back through the local copies; a
  list a mutable loan reaches is never lent from, and reading parts keeps
  the read lent.
- New fixtures: `run-pass/float32_list_elements.au` and
  `run-pass/lent_list_elements.au` (loops, a temporary list, a `mut`
  parameter, insert, pop, reverse, sort, copy, clear, a 200-element list
  past the conversion threshold, nested lists, enums, `json.Value` lists,
  and a task).
- Backend parity (every runtime fixture on both backends) passes.

## Measurements

Callgrind instruction counts, Linux container, release builds,
cross-language suite at the check size. Before is step 2c, after adds
both parts of this step. The suite holds no `float32` list, so the gains
come from part 2.

| Program | Before | After | Change | After vs Rust |
| --- | ---: | ---: | ---: | ---: |
| `interpreter` | 368.8M | 236.9M | -35.8% | 29.5x |
| `mini_browser` | 1,392.1M | 1,150.0M | -17.4% | 35.4x |
| `sort_records` | 75.2M | 66.9M | -11.0% | 16.1x |
| `sql_engine` | 2,809.7M | 2,596.4M | -7.6% | 54.8x |
| `nbody` | 16.2M | 15.9M | -2.4% | 19.4x |
| `text_diff` | 10.6M | 10.4M | -2.0% | 8.2x |

No program regresses. The largest increase is `graph_paths`, at 0.6%.

## Follow-Ups

- Part 3: lend `view` and `match` on `items[i]` (element loans) and
  element path reads, write and append boxes inline, and keep `str`
  views in boxes.
- A lent local passed to a function is still copied. A promotion would
  cost one retain instead.
