# ADR-0102: Tasks share state through handles in the `sync` module

- Status: Accepted
- Date: 2026-11-16
- Builds on: ADR-0043, ADR-0057, ADR-0059
- Design checkpoint: `architecture_docs/31-shared-state-design-checkpoint.md`

## Context

Tasks shared nothing. Module storage is immutable, every task argument is
moved or copied into the child, and a `Queue` was the only value two tasks
could both reach. That keeps data races out of the language. It also kept
out a cache every request reads, a connection pool, and a counter of
requests served, which a server needs.

## Decision

### Handles, not shared ownership

The builtin `sync` module adds four types:

```aura fragment
sync.Mutex[T](value: own T)        # T: Transfer
sync.AtomicInt(value: int64)
sync.AtomicBool(value: bool)
sync.Map[K, V]()                   # K: Transfer, V: Transfer
```

Each is a Copy handle, as a `Queue[T]` is. A copy reaches the same shared
state. A handle is `Transfer`, so it can be a task argument, a `Queue`
item, a field, or a module constant. Nothing else about ownership changes:
an ordinary value still has one owner, and module storage stays immutable.
A module constant that holds a handle never changes; the state the handle
reaches does.

Two other shapes were rejected:

- `mut` module storage. Two tasks writing one module variable race unless
  every access takes a lock the reader cannot see, and a hidden lock per
  access is slow and still races at the level of the program's logic.
- Shared objects with interior mutability by default, as Python has. That
  removes the single-owner rule the checker and the verifier rest on.

The names are in a module, not the prelude, so a program's own `Mutex`
class keeps its meaning.

### The lock is a `with` item

```aura fragment
with table = hits.lock():
    table["home"] = 1
```

`Mutex.lock()` returns a `sync.Locked[T]`, which is a `with` item of a new
shape: the item name is a mutable view (`view mut T`) of the guarded value
for the body. The view rules already keep it from being stored, returned,
or sent to a task, so no reference to the value outlives the lock, and
every way out of the body releases it.

- A closure, `mutex.update(function)`, was rejected for now. A lambda is
  one expression and a capturing closure cannot be stored, so real
  critical sections would not fit.
- Explicit `lock()` and `unlock()` calls were rejected. A missed unlock on
  one path is the classic bug, and the checker could not tie the borrowed
  value to the lock.

`replace(value) -> T` and `set(value)` swap the whole value in one step.
There is no `get()` that clones, because a method of a generic class
cannot clone a `T` yet.

### The value moves out and back

Taking the lock moves the value out of the lock's cell into a hidden
local of the task. The body's view is an ordinary view of that local, and
leaving the body moves the value back. Generated code works on the value
exactly as on any local, and the language gains no new kind of reference
into runtime memory.

### A failed holder poisons the lock

When a task ends while it holds a lock, the value it held is gone with
its stack. The runtime marks the lock poisoned and wakes the waiters, and
every later `lock()`, `replace`, and `set` traps with `AU4001`.
`is_poisoned()` reports the state. This is Rust's rule. Putting back a
value that a trap interrupted halfway through a change would hide the
failure from the next reader.

A recovery boundary records the lock depth on entry and poisons cells
acquired in the discarded frames. Outer holders can still release their
locks. [ADR-0103](0103-lock-lowering-and-native-storage.md) records the
lowering and native storage rules.

### Waiting

A task that finds the lock held checks it a few dozen times and then
waits through the scheduler, so it never blocks its worker thread and a
holder may sleep or do I/O. The lock is not fair. Waiting for a lock is
not a cancellation point: `lock()` returns only with the lock held, so a
`with` body never starts without it. The lock is not reentrant. A task
that locks a mutex it already holds traps with `AU4001` instead of
waiting for itself.

### Atomics

`AtomicInt` has `load`, `store`, `add` (which returns the new value),
`swap`, and `compare_and_set`. `AtomicBool` has the same without `add`.
Each call is one machine operation with sequentially consistent ordering.

`add` traps with `AU4002` on overflow, because an atomic counter that
wrapped silently would be the one integer in the language that does. The
addition is done first and checked after, so the usual path is one atomic
instruction. The stored value has wrapped by the time the trap is raised.

### The shared map

`sync.Map[K, V]` has `get`, `set`, `remove`, `contains`, `len`, `clear`,
and `snapshot`. Each call locks only for its own duration and returns
owned values, so no view crosses a lock. It is one lock around a `dict`.
A change that must read and write as one step uses a
`Mutex[dict[K, V]]`.

### A generic copy class is Copy by what it stores

A `copy class` was Copy only when every type argument was Copy. A handle
class stores a handle and never a `T`, so `Mutex[dict[str, int64]]` must
be Copy. The rule is now the one the MIR already used: a copy class is
Copy when every field is Copy with the type arguments in place.

## Consequences

- Servers can keep a cache, a pool, and counters in memory. The
  `examples/concurrency/shared_state.au` example shows all three.
- A program can deadlock by taking two locks in opposite orders. The
  group deadlock report covers it when every task in the group waits.
- A lock held across a `yield` in a generator stays held until the
  generator resumes or closes.
- `aura verify` treats the value a lock yields and the result of an
  atomic or map call as unknown values of their type.

## Not decided here

- A reader-writer lock. Readers in parallel need shared views into
  storage the cell keeps, which moving the value out does not give.
- A condition variable, `try_lock`, and lock timeouts.
- A `sync.Map` call that updates one entry in place, which needs a
  function argument that can capture.
