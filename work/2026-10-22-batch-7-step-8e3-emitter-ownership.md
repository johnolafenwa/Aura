# Batch 7 Step 8e, PR 3 of 4: The Emitter Follows MIR Ownership

## Goal

Make the direct emitter carry out the retains and releases that MIR
states, instead of deciding them itself. This is PR 3 of four for step 8e
of the
[Batch 7 design checkpoint](../architecture_docs/22-batch-7-design-checkpoint.md).
PR 1 added the vocabulary and PR 2 the pass and the checker. PR 4 moves
lending into MIR.

The gate is behaviour, not byte identity. This PR is part 3a of a split:
the emitter follows each `Release`, move, `try` list, and return that MIR
states. A `Retain` is paid where its consumer hands the value on, so the
emitter's transfer helpers stay. Part 3b deletes them once the operand-role
table agrees with the consumers that only borrow, such as a call
dispatched on a boxed generic receiver.

## Work Completed

- `cancel_retain_release_pairs` runs last in the ownership pass, on roots
  only. It turns `Retain(x)`, one consumer of `Place(x)`, and a later
  `Release(x)` into one consumer of `MovePlace(x)`. A retain right before
  `Return(Place(x))` cancels against the release the return makes.
  - A `Borrow` or `BorrowMut` parameter, a root read from a loan, a root
    another value borrows, an element-loan selector, and a repeatable
    closure's capture never move.
  - The consumer must touch the root once, by consuming it whole. Nothing
    between it and the release may touch, retain, or list the root.
  - A moved `try` operand leaves the `try` list.
  - Each rewritten function must still pass the loan validator and the
    ownership checker. Otherwise the pass keeps pairs one at a time and
    drops each pair that fails. 19 functions across the fixtures take this
    path.
- The native boundary no longer strips the pass output. The emitter:
  - lowers `Release` by layout and zeroes the root's words;
  - pays for a `Retain` at the consumer that hands the value on;
  - adds no release when it stores to a tracked root;
  - releases the root a `Return` returns from, and the `try` error list,
    after the pending cleanups;
  - moves `mut` parameters back through the writeback;
  - traps on the fall-through edge of a match chain that covers every
    variant, where MIR releases nothing.
- Deleted `release_all_opaque_roots`, `release_root_if_opaque`, and
  `export_return_value`. The release in `store_root` stays only for roots
  MIR does not track.
- MIR does not track Copy values, but this backend boxes some of them:
  narrow integers, `Duration`, Copy enums, and tuples of Copy values. The
  emitter still releases those roots at a store over them and at each
  return.
- `OwnedTemporaries` became `LayoutScratch`, an instruction-local set.
  Debug builds check that it is empty at each instruction and block
  boundary.
- Lending, borrowed-element fusion, proven-access adjacency, and the tag
  elision in `no_call_since` are unchanged.
- Debug builds of the direct runtime stop a task that releases a value
  its ledger does not hold, so every debug run checks for over-releases.
  One runtime test released a value its scope never owned and now
  releases it outside the scope.
- The direct runtime sets up standard output at start. Its 1 KiB buffer
  was the first large request after a program freed a big structure, and
  the allocator merged every freed chunk first.

## Verification

- Every run-pass fixture passes on the direct backend in a debug build,
  663 of 663, with the ledger and the over-release check on.
- Counters per fixture, base `af09f0d` against head:
  - retains 9085 to 8474, releases 71669 to 71063, clones 3636 to 3637;
  - retains plus clones are lower in 149 fixtures, equal in 513, and
    higher in 1.
  - `task_group_owned_capture` gains a clone in one run. Base shows the
    same clone in 1 of 40 parallel runs, so it depends on scheduling.
  - `union_borrow_generic_and_callables` trades 6 retains for 3 counted
    clones. A moved inline union boxed for a `T | None` parameter goes
    through `union_inject`, which counts its copy. Base used
    `union_inject_copy`, which makes the same copy without counting it.
  - `task_group_cancel` and `yield_now_fairness` releases vary run to run
    on both sides.
- Tests pass with `package_io_helpers_cover_local_error_paths` skipped:
  aura-compiler 2617, aura-runtime 740, aura 501.
  `loop_backedge_safepoints_prevent_socket_readiness_starvation` failed
  once on its interpreter half and passed 2 of 3 reruns. The interpreter
  path does not change here.
- Backend parity passes, and so does
  `direct_objects_are_identical_across_repeated_builds`.
- `cargo fmt`, the CI clippy command, and `scripts/check-hygiene.sh` pass.
- `aura run` of `print("hi")`, debug builds: 93.59 and 93.61 million
  instructions, medians 42.8 ms and 41.5 ms over 50 runs.
- Release builds, base then head, interleaved medians:
  - `string_list_scan` 17.8 and 18.3 ms, then 18 and 17 ms;
  - `record_field_reads` 113.7 and 122.5 ms. Callgrind counts 695.9 and
    694.6 million instructions, and cache simulation finds the same
    misses. Adding one unrelated function moves base by 4%;
  - `int_list_sum` 12.6 and 12.9 ms, `dict_build_get` 130.1 and 132.9 ms,
    `set_insert_contains` 27 and 26 ms.
- Binary sizes of the five benchmark programs are equal, except
  `record_field_reads`, which is 4 KiB smaller.
- `native_codegen.rs` grew from 14897 to 14991 lines, and
  `native_codegen_callables.rs` stays at 1448.
- Coverage meets the floors: lines 96.57%, functions 97.38%, regions
  95.49%.
- The sanitizer script needs a nightly toolchain, which this container
  lacks, so it did not run.

## Follow-Up

- Part 3b: make the operand-role table match the consumers that borrow,
  then delete the transfer decisions on MIR operands.
- Find why `record_field_reads` runs slower at equal instruction counts.
  Simulated branch misses in `aura_direct_tag_value_type` rise from 130
  to 319 thousand as releases now interleave with the loop's calls.
- PR 4: move lending into MIR.
