# ADR-0017: Iteration source selection

- Status: Provisional — Batch 1 checkpoint review
- Date: 2026-07-14
- Reference gap: timing and identity of a selected loop source

## Context

ADR-0006 defines the ownership mode of place iteration and the receive-based
Queue carve-out, but it does not say whether a loop re-evaluates or re-reads its
iterable place after the body begins. The direct backend selected its source
once, while MIR kept a place operand that could observe a body-local rebinding.
The two backends could therefore visit different collections or receive from
different queues for the same checked program.

This is an evaluation-order gap, not a change to ADR-0006's ownership modes.
No syntax, yielded-value ownership, or mutation permission changes.

## Provisional decision

- A `for` statement evaluates and selects its iterable once, before its first
  iteration.
- `for value in own values:` moves a `Vec` or `Set` into a loop-private source.
  Reinitializing the consumed source binding in the body does not retarget or
  truncate that active iteration.
- Bare Queue iteration copies the Queue handle once at loop entry. The source
  binding is not frozen and may be rebound in the body, but the active receive
  loop continues through the selected handle.
- These rules supplement ADR-0006; its parameter defaults, loop ownership
  modes, Queue modifier rejection, and task-capture decisions are unchanged.

This contained gap-fill follows P1 (no source retargeting hidden behind a
seemingly single loop expression), P2 (one result on MIR and direct), P4 (the
selection is an ordinary visible evaluation point with no hidden deep clone),
and P6 (one source-selection rule for all `for` statements). Queue's copied
handle preserves its existing cheap shared-runtime identity.

## Completion tests

- `crates/aurora-compiler/tests/fixtures/run-pass/own_iteration_captures_collection.au`
  pins one-time Vec and Set selection after the consumed source binding is
  reinitialized.
- `crates/aurora-compiler/tests/fixtures/run-pass/queue_iteration_captures_handle.au`
  pins one-time Queue-handle selection while allowing the source binding to be
  rebound.
- Both fixtures are exercised by the forced MIR/direct parity matrix.
