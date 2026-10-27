# Rust Speed Step 2h Part 2: Mutable Arguments Move Into The Call

## Goal

Step 2h of Phase 2 of the Rust-speed plan moves ownership off boxes. Its
gate: leak, failure, and cancellation cleanup tests pass, and retains and
releases per program drop at least 90%. Part 1
(`2026-10-27-rust-speed-2h-shared-arguments.md`) kept shared arguments
with the caller. This part handles `mut` arguments, which the part 1
census named as most of what was left in `regex_match`, `graph_paths`,
`astar_pathfinding`, `order_book`, and `word_freq`.

## The Old Convention

A `mut` parameter hands its value back through the call's write-back: a
named call returns it as an extra result, and the caller stores it into
the place it passed. The ownership pass treated the argument like any
other consumed operand, so a caller that used the place again retained it
before the call; the callee owned that reference and returned it, and the
store of the write-back released the caller's old reference to the same
box. Each such call paid a retain and a release for nothing.

## Changes

- `ownership::moved_writebacks` finds the call operands that move: a
  whole root the function tracks, passed to a `mut` parameter whose
  write-back is that same root, or as the `mut self` receiver of a
  statically dispatched method that writes back to it. Only named calls
  and statically dispatched method calls qualify: a call through a
  function value, a union arm or a runtime type's method, and runtime
  helpers keep the old convention. A root with a registered cleanup stays,
  because a trap in the callee runs the cleanup on the root.
- `OwnershipFacts::accesses` reports such an operand as a move instead of
  a consume, so the ownership pass inserts no retain, the root is empty
  until the write-back fills it again, and the ownership checker refuses
  a retain or release of it around the call.
- The native backend reads the same operands from `ExplicitLocals.moved`
  (by operand address in the module it emits) and takes the root's value
  instead of reading it, as it already did for the temporary of a mutable
  lend. The write-back then stores into an empty root and releases
  nothing.

## Verification

- An ownership pass test: a `mut` parameter passed on, a local passed to
  a named call and to a `mut self` method take no retain, and a call
  through a function value still does. A checker test refuses a retain
  or a release of the moved root before the call.
- New fixture `run-pass/mutable_arguments_moved.au`: a list pushed to a
  thousand times through a `mut` parameter, two `mut` arguments in one
  call, a recursive function passing its `mut` parameter on, `mut self`
  methods calling each other with another `mut` argument, a record, a
  union and an enum passed as `mut`, a copy taken before a change, and a
  call through a function value. Its native build makes 14 retains where
  the part 1 compiler made 1,050.
- Compiler tests, backend parity, the CLI tests, `check:cross-language`,
  `check:operation-probes`, `cargo fmt --check`, hygiene, and clippy.

## Measurements

Retains counted with `AURA_RUNTIME_STATS=1` at the check size of the 30
cross-language programs, Linux container, release builds. "2g" is before
step 2h, "Part 1" is #166.

| Program | 2g | Part 1 | Part 2 |
| --- | ---: | ---: | ---: |
| `regex_match` | 249,932 | 166,416 | 221 |
| `interpreter` | 339,335 | 226,756 | 212,295 |
| `sql_engine` | 156,514 | 71,207 | 34,867 |
| `mini_browser` | 83,879 | 38,941 | 15,153 |
| `lru_cache` | 23,005 | 18,005 | 2,452 |
| `graph_paths` | 25,226 | 25,226 | 13,450 |
| `astar_pathfinding` | 11,570 | 11,546 | 7,442 |
| `order_book` | 4,746 | 4,746 | 2,000 |
| `nbody` | 7,011 | 6,003 | 0 |
| `raytracer` | 12,612 | 1,850 | 0 |
| All 30 programs | 1,062,951 | 696,205 | 400,936 |

A retain is an inline count update, so callgrind instruction counts move
less than the retains: against part 1, `regex_match` runs 1.9% fewer
instructions (290.9M to 285.4M), `nbody` 1.3%, `lru_cache` 1.1%, and
no program runs more than 0.1% more.

## What Is Left For The Gate

- Runtime collection writes on a local or `mut` parameter receiver
  (`list.swap`, `list.remove`, `extend`, `clear`, and others) store the
  receiver back after the helper changed the box in place, which retains
  it and releases the old reference to the same box. That is every retain
  left in `astar_pathfinding` (`heap.swap` in `heap_pop` and
  `heap_push`).
- `interpreter`: `Expr` and `Op` payload bindings and `str` arguments.
- `json_roundtrip`, `sql_engine`, `docdb`: `str`, `dict`, and `list`
  values copied out of collections.
- Unit results of runtime helpers and the scalar math builtins, and the
  leak, failure, and cancellation tests.
