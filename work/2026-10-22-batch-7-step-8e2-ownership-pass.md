# Batch 7 Step 8e, PR 2 of 4: Ownership Pass and Checker

## Goal

State every retain and release in MIR, and check what a module states,
with no change to what either backend emits. This is PR 2 of four for
step 8e of the
[Batch 7 design checkpoint](../architecture_docs/22-batch-7-design-checkpoint.md).
PR 1 added the vocabulary. PR 3 switches the direct emitter to the MIR's
decisions, and PR 4 moves lending into MIR.

## Work Completed

- `mir::ownership::insert_ownership` states ownership in an implicit
  module. `mir::lower` runs it after the union plans, and the native
  boundary runs it on an implicit module. It does nothing to an explicit
  module. `aura mir` and the native build path lower with it.
  - It tracks each root local whose value needs drop. Copy builtins,
    `MirClass.copy`, `MirEnum.copy`, and union plans decide that.
  - Backward liveness finds where each value dies. A loan, a closure that
    borrows a capture, and a `VariantPayload` result keep their source
    alive.
  - A consuming `Place` operand gets a `Retain` right before its
    instruction. A value is released right after its last use.
  - A value that dies on an edge is released at the start of the
    successor when the successor has one way in, or when the edge leaves a
    `ForRange`, whose exit the interpreter matches by label. Otherwise the
    pass splits the edge with a new `%edgeN` block.
  - A store to a root that holds a value releases it first. When the right
    side reads the root, the value goes through a new temporary.
  - `Return` releases what is left, except `mut` parameters and the root
    it returns from. A `try` lists what holds a value there, except `mut`
    parameters.
  - Releases at one point come out in name order.
  - Nothing goes between `IndexInRange` and the access it covers, or
    between a call and the returned loan that follows it.
  - A block that ends in `Unreachable` or `AssertFail` releases nothing.
    The abort reclaims every value, and an initializer may not mention a
    half-built `self` there.
  - A match chain whose arms cover every variant of an enum has no
    fall-through edge in the pass's graph. Lowering emits that edge, but
    no value takes it, and paths through it joined moved values with live
    ones.
  - A module the pass cannot read stays implicit, and the validator
    reports what is wrong with it.
- `mir_ownership_check.rs` holds `validate_ownership`. Native emission
  runs it next to `validate_loan_flow` through `mir::validate_at_boundary`
  on every module. The interpreter runs it on explicit modules only.
  - Each tracked root is Empty, Owned, or Maybe. Joining Owned with Empty
    gives Maybe. A path that never stored the root adds nothing, because
    definite assignment is the type checker's rule.
  - It refuses a double release, a use after a release or a move, a store
    over a held value, a release on only some paths, a release or a move
    under a loan or a cleanup, a wrong `try` list, a retained task
    argument, an unmatched or leftover retain, and a return that leaves a
    value held or a `mut` parameter empty.
  - It has the loan validator's budgets: roots times blocks, and block
    visits.
- The interpreter reports a checker error after `invalid MIR ownership:`.
  Native emission reports it as it is.
- The native boundary strips what the pass added before it emits, with
  `strip_ownership`. An implicit module is checked through a copy that
  the pass fills in, and the original is emitted.
- The interpreter gains nothing from the pass, so `aura run` and the
  other interpreter paths lower with `mir::lower_implicit` and skip both
  the pass and the checker. The interpreter still treats `Retain` and
  `Release` as no-ops. Native builds check every fixture, so the gate
  stays.
- The loan validator lets a `try` release a root a loan still holds,
  because the error edge leaves the frame and ends every loan in it. A
  `Retain` may follow a returned loan.
- Tests that forge MIR now forge implicit MIR, which the native boundary
  fills in. One forgery read a local it never stored and now stores it first.

## Verification

- `cargo fmt`, the CI clippy command, and `scripts/check-hygiene.sh` pass.
- The checker passes on every run-pass and run-fail fixture on both
  backends, and on the MIR tests.
- Every run-pass fixture emits a byte-identical object at base and head
  (627 of 627).
- Tests pass with `package_io_helpers_cover_local_error_paths` skipped:
  aura-compiler 2619 (under coverage), aura-runtime 739, aura 500.
- Backend parity passes, and so does
  `direct_objects_are_identical_across_repeated_builds`.
- Debug builds without debug info, medians, base then head, interleaved:
  - `aura run` of `print("hi")`, 30 runs: 43.3 ms and 40.5 ms. Callgrind
    counts 79.93 and 79.91 million instructions, so `aura run` does the
    same work as before;
  - `aura run` of `json_dynamic_values.au`, the largest run-pass fixture,
    15 runs: 840.7 ms and 811.3 ms;
  - `aura mir` of it: 383.4 ms and 395.6 ms;
  - `aura check` of it: 100.9 ms and 99.4 ms.
- A first version ran the pass and the checker on the interpreter path
  too, which cost 11 ms at startup for the 111 functions even
  `print("hi")` lowers. The interpreter gains nothing from them, so it now
  skips both.
- `scripts/test_bench_compile_time.py` passes. The full
  `bench-compile-time.py` run needs release builds of base and head and
  was not run.
- Coverage meets the floors: lines 96.56%, functions 97.37% (10013 of
  10283), regions 95.48%. Every function in `mir_ownership.rs` and
  `mir_ownership_check.rs` runs. Three new tests cover old validator
  paths: a tuple projection past the address space, an unknown argument
  to an indirect call with a union parameter, and the emitter's own arity
  check for an unresolved internal slice.

## Follow-Up

- PR 3: the emitter lowers `Retain` and `Release` by layout, with pair
  cancellation. It should emit a trap on the fall-through edge of an
  exhaustive match, which the pass treats as unreachable.
- PR 3: compare each fixture's `retains` plus `clones` against the base
  counters from PR 1.
