# ADR-0103. Lock Lowering And Native Storage

- Status: Accepted
- Date: 2026-11-17
- Design checkpoint: [Shared mutable state](../31-shared-state-design-checkpoint.md)
- Companion decision: [Shared state handles](0102-shared-state-handles.md)

## Context

A mutex must guard scalars, records, and collections on both backends. A
task may yield while holding a lock. Another task on the same scheduler
worker must then be able to wait for that lock without blocking the worker.

## Decision

The checker recognizes the standard library's `sync.Locked[T]` manager.
Entering the block takes its value into a hidden local. The user binding
borrows that local mutably. Normal control flow, including early exits,
ends the borrow and moves the value back before leaving the block.

The scheduler identifies the current holder by task. It keeps the held
cells in acquisition order. If a task fails, it poisons its active cells
and signals their wait queues. An inner recovery scope can record its
starting depth so a trap affects only locks taken since that point.
Recursive locking by one task reports `AU4001`.

Each cell has an atomic state. An immediate acquisition takes an
unlocked cell with acquire ordering. A contended acquisition spins
briefly and marks the cell before parking on a scheduler-aware queue.
Release publishes the new contents and wakes a waiter when the cell
was marked. The wake queue retains an early signal.

Generated code carries a single-field wrapper around a synchronization
handle in one machine word. The wrapper owns a reference to the runtime
cell or atomic word. When a task receives a copied handle, it can use its
own wrapper while both tasks continue to access the same underlying state.
Atomic operations use sequentially consistent ordering. Integer and
boolean lock payloads cross the runtime boundary as words. For other
payloads the mutex can transfer a unique native box to the next holder.
A box with another owner first supplies an independent value.

## Consequences

The body can use ordinary value and borrowing operations. A trap discards
the interrupted value and subsequent acquisitions report a poisoned lock.
The runtime allocates its wake queue only if spinning does not acquire the
cell. Programs that take several locks must choose a consistent lock order.
