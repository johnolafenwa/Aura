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
Lowering keeps its cell in a hidden local until block exit. For the
standard `mutex.lock()` call it reads the mutex's cell directly. A custom
factory or other manager expression runs once before its cell is read.
Entering the block takes the guarded value into a second local. The user binding
borrows that local mutably. Normal control flow, including early exits,
ends the borrow and moves the value back before leaving the block.

The scheduler identifies the current holder by task. The first acquired
cell installs an exit callback on that task. Task retirement can then
poison live cells and wake waiters. Programs that never acquire a cell
can link without the mutex wake code. The task keeps its held cells in
acquisition order. This same list checks release ownership and
recursive locking. If a task fails, it poisons its active cells
and signals their wait queues. An inner recovery scope can record its
starting epoch. A trap affects only locks acquired since the mark, even
when a caller releases an earlier lock in the meantime.
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
boolean lock payloads cross the runtime boundary as words and stay as
words in the native cell. A generic or interpreter caller can also take
these values through the usual boxed interface. For other
payloads the mutex can transfer a unique native box to the next holder.
A box with another owner first supplies an independent value.

A shared map lookup locks the cell, copies the matching entry, and then
unlocks it. A native map can stay in its existing box for this read.
Building the returned `Lookup` after the copy shortens the critical section.

## Consequences

The body can use ordinary value and borrowing operations. A trap discards
the interrupted value and subsequent acquisitions report a poisoned lock.
The runtime allocates its wake queue only if spinning does not acquire the
cell. Programs that take several locks must choose a consistent lock order.
