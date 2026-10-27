# Rust Speed Step 2h Part 1: Shared Arguments Stay With The Caller

## Goal

Step 2h of Phase 2 of the Rust-speed plan John approved on 2026-10-26
moves ownership off boxes. Its gate: leak, failure, and cancellation
cleanup tests pass, and retains and releases per program drop at least
90%. The step plan is in the project files,
`rust-speed-plan/phase-2/step-2h-plan.md`. This is its first part.

A census of every retain and release the 30 cross-language programs make
at the check size (a scratch runtime that recorded each value's type and
whether the release left other references) found one cause well ahead of
the others: the ownership pass treated every call as handing the callee
a reference to each argument (`call_roles`: "The callee owns every
parameter, including its receiver"). A call that passed a local it used
again retained it, and the callee released it on return, even when the
parameter only read it. That was most of the retains in `regex_match`,
`interpreter`, `raytracer`, `nbody`, `lru_cache`, and `sql_engine`.

## Changes

- `ownership::borrowing_functions` finds the functions only named calls
  and statically dispatched method calls reach. A function value or task
  entry, a closure body, a cleanup or `From` callee, a method a union arm
  or a runtime type picks, a module constant's initializer, a default
  argument helper, `main`, and a function that returns a view keep the
  owning convention.
- The ownership pass records those functions in `MirModule.borrowing`.
  Native emission drops builtin wrappers the program never reaches, and a
  dropped wrapper may have been the only other way into a function, so
  later stages read the record instead of recomputing it. The ownership
  checker refuses a recorded function that another call could reach.
- In a recorded function, a shared parameter or shared `self` whose type
  needs a drop is kept: the caller passes the value it holds, the callee
  never releases it, any use that consumes it retains it first, and it
  must still be there at every return. The checker enforces each of
  these. A shared parameter of a type the MIR copies freely, such as a
  `Queue` handle or a `Duration`, keeps the owning convention: the native
  backend boxes such values and releases them itself.
- Call sites borrow the kept arguments (`callee_borrowed_operands`). The
  native backend passes them without a transfer, and a temporary it
  built for the call is released after it. A function's thunk, which
  function values and task entry use, releases the kept arguments after
  the call, so those callers keep the owning convention.
- The boundaries resolve method calls before the ownership pass, and
  validate them before the ownership checker, which trusts the callees
  they name.

## Problems Found

- `http_api` released a `Queue` handle twice: the compiler copies a
  `Queue` freely and the native backend releases the boxed handles of
  every such local when a function returns, including a kept parameter.
  Such parameters now keep the owning convention.
- A bound method returning a view (`def pick(self, source: Pair) -> view
  str from source`) crashed: the view's loan rules assume the callee owns
  its argument. Functions that return a view keep the owning convention.
- `json_codec_derived.au` failed the checker after emission dropped the
  `json.encode` wrapper whose dispatch had been the other way into
  `Money.encode`. Recording the functions in the module fixed it.

## Verification

- Ownership pass and checker tests for kept parameters: a release or a
  move of one is refused, and so is a recorded function that a function
  value also reaches. Lending tests cover a lent field passed to a
  borrowing callee and to an owning one.
- New fixture `run-pass/shared_parameters_borrowed.au`: the same list as
  two shared arguments, a shared argument copied before a change, a
  recursive enum walked through shared parameters, methods taking `self`
  and another instance shared, a temporary passed to a shared parameter,
  a union parameter, a function called directly and through a function
  value, a `Queue` and a `Duration` passed twice, and a function
  returning a view into its argument.
- Compiler tests, backend parity, the CLI tests, `check:cross-language`,
  `check:operation-probes`, `cargo fmt --check`, hygiene, and clippy.

## Measurements

Callgrind instruction counts, Linux container, release builds, the
30-program cross-language suite at the check size. Before is step 2g
(#165). Retains are counted by `AURA_RUNTIME_STATS=1`.

| Program | Retains before | Retains after | Instructions before | Instructions after |
| --- | ---: | ---: | ---: | ---: |
| `sql_engine` | 156,514 | 71,207 | 1,705.4M | 1,413.8M |
| `lru_cache` | 23,005 | 18,005 | 51.4M | 42.8M |
| `spreadsheet` | 7,916 | 4,021 | 25.1M | 22.4M |
| `json_roundtrip` | 55,702 | 49,660 | 644.2M | 576.5M |
| `template_render` | 7,120 | 4,620 | 34.1M | 32.5M |
| `interpreter` | 339,335 | 226,756 | 101.5M | 98.0M |
| `mini_browser` | 83,879 | 38,941 | 595.6M | 584.0M |
| `regex_match` | 249,932 | 166,416 | 297.6M | 290.9M |
| `raytracer` | 12,612 | 1,850 | 37.0M | 36.5M |

Retains across all 30 programs fall from 1,062,951 to 696,205 (34%).
The suite showed `http_api` 13.5%, `kv_server` 5.7%, and `file_io` 1.5%
higher, but direct callgrind runs of both builds give 30.22M and 29.92M,
57.99M and 57.99M, and 17.23M and 17.24M: their tasks, sockets, and files
vary between runs. No other program grows by more than 0.3%.

## What Is Left For The Gate

- `mut` arguments: the caller retains the value, and the writeback
  releases the old reference when the callee hands the value back. Moving
  the value into the call and writing it back removes both; it is most
  of what remains in `regex_match` (`mut self` and `mut list` arguments
  to `add_state`), `graph_paths`, `astar_pathfinding`, `order_book`, and
  `word_freq`.
- Unit results of runtime helpers and the scalar math builtins still
  allocate boxes; enum payload bindings still retain their payload.
- The leak, failure, and cancellation tests.
