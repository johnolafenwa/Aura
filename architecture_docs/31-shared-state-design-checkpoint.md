# Shared Mutable State Design Checkpoint

Status: **Proceeding on the defaults below (2026-11-16).** The choices
marked "owner's call" stay open to change until release. The decisions
are recorded as ADR-0102 and ADR-0103.

Tasks in Aura 0.16 share nothing. Module storage is immutable, every task
argument is moved or copied into the child, and the only way two tasks
reach the same data is a `Queue`. That keeps data races out of the
language, and it also keeps out the ordinary shapes of a server: a cache
every request reads, a connection pool, a counter of requests served.
This checkpoint settles how tasks share a value they can all change.

## Starting Point

- A `Queue[T]` handle is a Copy value that is also `Transfer`. Copies of
  it on different workers reach one synchronized runtime object. A module
  constant may hold one, and a `copy class` may have one as a field.
- A `with` item whose manager declares
  `__enter__(mut self) -> view mut T from self` binds the item name to a
  mutable view. The view cannot be stored or returned, and it ends before
  the exit runs.
- A `with` block over a user manager costs about 8,500 instructions in a
  native build, because the manager is boxed and its exit is registered
  with the runtime for the trap path. A Queue `get` then `put` costs about
  260 ns. Neither is cheap enough to be a lock.
- A trap does not unwind generated frames. The runtime runs registered
  cleanup thunks and resets the task's stack.
- A task that has started never moves to another worker. Several tasks
  interleave on one worker at scheduling points.

## A. The Shape Of Sharing

- **A1. Shared handles in a `sync` module (default).** `sync.Mutex[T]`,
  `sync.AtomicInt`, `sync.AtomicBool`, and `sync.Map[K, V]` are Copy
  handles, as `Queue[T]` is. A copy of a handle reaches the same state. A
  handle is `Transfer`, so it can be a task argument, a Queue item, a
  field, or a module constant. Nothing else about ownership changes: an
  ordinary value still has one owner, and module storage stays immutable.
- A2. `mut` module storage. Rejected. Two tasks writing one module
  variable is a data race unless every access takes a lock the reader
  cannot see, and a hidden lock per access is slow and still races at the
  level of the program's logic.
- A3. Reference-counted shared objects with interior mutability, as
  Python has. Rejected for the same reason: it removes the single-owner
  rule the checker and the verifier rest on.

The names live in a module, not the prelude, so no existing program's
`Mutex` class changes meaning.

## B. The Lock

```aura
import sync

def main() -> int32:
    hits = sync.Mutex[dict[str, int64]](dict[str, int64]())
    with table = hits.lock():
        match table.get("home"):
            case Lookup.Found(count):
                table["home"] = count + 1
            case Lookup.Missing:
                table["home"] = 1
    return 0
```

- **B1. `with name = mutex.lock():` (default).** The item name is a
  mutable view of the value for the body. The view rules already keep it
  from being stored, returned, or sent to a task, so no reference to the
  value outlives the lock. Every way out of the body releases the lock.
- B2. A closure, `mutex.update(function)`. Rejected for now. A lambda is
  one expression and a capturing closure cannot be stored, so real
  critical sections would not fit.
- B3. Explicit `lock()` and `unlock()` calls. Rejected. A missed unlock
  on one path is the classic bug, and the checker could not tie the
  borrowed value to the lock.

**B4. The value moves out and back (default).** Taking the lock moves the
value out of the cell into a hidden local of the task. The body's view is
an ordinary view of that local, and leaving the body moves the value
back. Generated code therefore works on the value exactly as on any
local, with no new kind of reference into runtime memory.

**B5. A failed holder poisons the lock (default, owner's call).** When a
task ends by a trap while it holds a lock, the value it held is lost with
its stack. The runtime marks the lock poisoned and wakes the waiters, and
every later `lock()` traps with `AU4001` naming the cause. This is Rust's
rule. The alternative, putting back a value that a trap interrupted
halfway through a change, would hide the failure from the next reader.
`is_poisoned()` reports the state.

**B6. Waiting.** A task that finds the lock held spins briefly, then
parks through the scheduler, so it never blocks its worker thread. A
holder may sleep or do I/O while it holds the lock. Waiting for a lock is
not a cancellation point: `lock()` returns only with the lock held. The
lock is not fair and not reentrant. A task that locks a mutex it already
holds traps with `AU4001` instead of waiting forever.

`replace(value) -> T` and `set(value)` swap the value in one step under
the lock. `replace` returns the previous value.

## C. Atomics

`sync.AtomicInt` holds an `int64`: `load`, `store`, `add` (returns the
new value), `swap`, and `compare_and_set`. `sync.AtomicBool` holds a
`bool`: `load`, `store`, `swap`, `compare_and_set`. Each call is one
atomic machine operation with sequentially consistent ordering.

- **C1. `add` traps on overflow (default).** Aura integer arithmetic
  traps with `AU4002`, and an atomic counter that wrapped silently would
  be the one integer in the language that does. The addition is done
  first and checked after, so the fast path is a single instruction. The
  stored value has wrapped by the time the trap is raised.

## D. The Shared Map

`sync.Map[K, V]` is a dictionary several tasks can use at once: `get`,
`set`, `remove`, `contains`, `len`, `clear`, and `snapshot`. Each call locks only for
its own duration and returns owned values, so no view crosses a lock.
A read-modify-write of one key that must be atomic uses a
`Mutex[dict[K, V]]` instead.

## E. What Is Not In The First Version

- A reader-writer lock. Readers in parallel need shared views into
  storage the cell keeps, which B4 does not give. It can be added on the
  same handle model.
- A condition variable. A `Queue` already wakes a waiting task.
- Lock timeouts and `try_lock`.

## F. Implementation Outline

- Runtime: one `Value::Sync` variant holding a cell (lock word, holder,
  stored value, a wait queue built on the existing scheduler wait) or an
  atomic word. Each task tracks the cells it holds so its completion
  can poison them and wake their waiters.
- Compiler: the `sync` builtin module declares two Copy host handle
  types and their host functions. `Mutex`, `Locked`, `AtomicInt`,
  `AtomicBool`, and `Map` are Aura source in `std/sync.au`.
  `classify_manager` recognizes `sync.Locked[T]`, and `with` lowering
  emits the take, a view of a hidden local, and the put on each exit.
  No cleanup is registered, so the lock costs two runtime calls.
- Native code calls the cell and atomic entry points directly with
  machine-word arguments.

## G. Verification

`aura verify` treats the value a lock yields, and the result of an
atomic read, as unknown values of their type. Another task may have
changed them, so nothing proved before the lock holds after it. Contracts
on the body are checked against that unknown value.

## H. Performance Bar

Within about three times Rust on an uncontended lock, a contended
counter, an atomic counter, and a shared cache, and faster than Python on
each. `benchmarks/shared_state` holds the programs.
