# Sync Module

The `sync` module holds the values several tasks can change: a lock around
a value, atomic integers and booleans, and a shared map. Everything else
in Aura has one owner, and module constants never change, so these four
types are how tasks share state.

```aura
import sync
```

Every type here is a Copy handle, as a `Queue[T]` is. Copying a handle
copies a reference to the same shared state, not the state. A handle is
`Transfer`, so it can be a task argument, a `Queue` item, a field of a
class, or a module constant.

## Public API

| API | Signature | Result |
| --- | --- | --- |
| `sync.Mutex[T]` | `Mutex[T](value: own T)` | A lock that guards `value`. `T` must be `Transfer`. |
| `Mutex.lock` | `lock(self) -> sync.Locked[T]` | The `with` item that holds the lock for a block. |
| `Mutex.replace` | `replace(self, value: own T) -> T` | Stores `value` and returns the value it replaces, as one step. |
| `Mutex.set` | `set(self, value: own T) -> None` | Stores `value` and drops the value it replaces. |
| `Mutex.is_poisoned` | `is_poisoned(self) -> bool` | Whether a task failed while it held the lock. |
| `sync.AtomicInt` | `AtomicInt(value: int64)` | An `int64` changed in single atomic steps. |
| `AtomicInt.load` | `load(self) -> int64` | The value now. |
| `AtomicInt.store` | `store(self, value: int64) -> None` | Replaces the value. |
| `AtomicInt.add` | `add(self, delta: int64) -> int64` | Adds `delta` and returns the new value. |
| `AtomicInt.swap` | `swap(self, value: int64) -> int64` | Stores `value` and returns the value it replaces. |
| `AtomicInt.compare_and_set` | `compare_and_set(self, expected: int64, new: int64) -> bool` | Stores `new` if the value is `expected`, and says whether it did. |
| `sync.AtomicBool` | `AtomicBool(value: bool)` | A `bool` changed in single atomic steps. |
| `AtomicBool.load` | `load(self) -> bool` | The value now. |
| `AtomicBool.store` | `store(self, value: bool) -> None` | Replaces the value. |
| `AtomicBool.swap` | `swap(self, value: bool) -> bool` | Stores `value` and returns the value it replaces. |
| `AtomicBool.compare_and_set` | `compare_and_set(self, expected: bool, new: bool) -> bool` | Stores `new` if the value is `expected`, and says whether it did. |
| `sync.Map[K, V]` | `Map[K, V]()` | An empty shared map. `K` and `V` must be `Transfer`. |
| `Map.get` | `get(self, key: K) -> Lookup[V]` | `Lookup.Found` with a copy of the value, or `Lookup.Missing`. |
| `Map.set` | `set(self, key: own K, value: own V) -> None` | Stores `value` for `key`. |
| `Map.remove` | `remove(self, key: K) -> Lookup[V]` | Removes the entry and returns its value, or `Lookup.Missing`. |
| `Map.contains` | `contains(self, key: K) -> bool` | Whether `key` has an entry. |
| `Map.len` | `len(self) -> int64` | The number of entries. |
| `Map.clear` | `clear(self) -> None` | Removes every entry. |
| `Map.snapshot` | `snapshot(self) -> dict[K, V]` | A copy of the whole map at one instant. |

## Example

<<< ../../examples/concurrency/shared_state.au{aura}

It prints:

```text
400
400
81
10
11400
```

## Mutex

`sync.Mutex[T](value)` guards one value of any `Transfer` type. The only
way to reach the value is a `with` block:

```aura fragment
hits = sync.Mutex[dict[str, int64]](dict[str, int64]())

with table = hits.lock():
    table["home"] = 1
```

The block waits until no other task holds the lock. Inside it, the item
name is a mutable view of the guarded value: the body reads it, assigns to
it, and calls its methods as it would with a `mut` local. The view cannot
be stored, returned, or passed to a task, so no reference to the value
outlives the block. Every way out of the block releases the lock: the end
of the body, `return`, `break`, `continue`, and `try`.

A value computed inside the block leaves it as an owned value:

```aura fragment
mut count = 0
with table = hits.lock():
    count = table.len()
```

`replace` and `set` change the whole value in one step without a block.

### Waiting

A task that finds the lock held waits without holding its worker thread,
so other tasks on that worker keep running. A holder may sleep, do I/O, or
wait on a `Queue` inside the block. Tasks that wait for one lock are not
served in arrival order.

Waiting for a lock is not a cancellation point. `lock()` returns only
with the lock held, and a cancelled task sees its cancellation at the next
operation that reports one.

### Locking Twice

A mutex is not reentrant. A task that enters a `with` block for a mutex it
already holds traps with `AU4001` instead of waiting for itself forever.
`replace` and `set` take the lock too, so they trap inside a block that
holds the same mutex.

Two tasks that take two locks in opposite orders can each wait for the
other. Take locks in one order everywhere. The runtime reports the group
as deadlocked when every task in it waits and none can run.

### Poisoning

A task that traps while it holds a lock leaves the guarded value half
changed, and the value is lost with the task. The mutex is then poisoned:
every later `lock()`, `replace`, and `set` traps with `AU4001`, and tasks
already waiting for the lock trap the same way. `is_poisoned()` reports
the state without trapping.

### Generators

A generator that yields inside a lock block keeps the lock until the
consumer asks for the next item or closes the generator. Prefer to copy
what the generator needs out of the block first.

## Atomics

`sync.AtomicInt` and `sync.AtomicBool` change in single steps that no
other task can observe halfway, with no lock. Use them for counters,
flags, and identifiers:

```aura fragment
REQUESTS = sync.AtomicInt(0)

def serve() -> None:
    _ = REQUESTS.add(1)
```

`add` returns the new value. An addition that overflows `int64` traps with
`AU4002`, as `+` does. The stored value has wrapped by the time the trap
is raised.

`compare_and_set(expected, new)` is the step other lock-free updates are
built from: it stores `new` only if the value is still `expected`.

Every operation is sequentially consistent: all tasks agree on one order
of all atomic steps.

## Map

`sync.Map[K, V]` is a dictionary that tasks use at once. Each call locks
the map for its own duration and hands back owned values, so each call is
one atomic step and no reference to an entry leaves the map. `get` and
`snapshot` copy the values they return.

Two calls are two steps. Another task may change the map between a `get`
and the `set` that follows it:

```aura fragment
match cache.get(key):
    case Lookup.Found(value):
        return value
    case Lookup.Missing:
        value = compute(key)
        cache.set(key, value)   # another task may have set it meanwhile
        return value
```

That is the right shape for a cache, where computing a value twice is
harmless. A change that must read and write as one step uses a
`sync.Mutex[dict[K, V]]` and one `with` block.

## Module Constants

A module constant may hold a handle. The constant itself never changes,
and the state it reaches does:

```aura fragment
CACHE = sync.Map[str, str]()
NEXT_ID = sync.AtomicInt(1)
```

Every task in the program reaches the same map and the same counter
through these names.

## Typing Rules

- `sync.Mutex[T]` and `sync.Map[K, V]` require `Transfer` type arguments,
  because the guarded values cross between tasks.
- Every handle type is Copy and `Transfer`, whatever its type arguments.
- The item of `with name = mutex.lock():` has type `view mut T`.
- `sync.Locked[T]` is useful only as a `with` item. The item form
  `with mutex.lock() as name:` is accepted too.

## Verification

`aura verify` treats the value a lock block sees, and the result of every
atomic and map operation, as an unknown value of its type. Another task
may have changed it, so a fact proved about the value in one block does
not hold in the next.

## Backend Support

`aura run` and `aura build` both support every operation on every
platform the runtime supports. A freestanding build has no tasks and no
`sync` module.

## Limits

- There is no reader-writer lock, no condition variable, no `try_lock`,
  and no lock timeout. A `Queue` wakes a waiting task.
- A lock cannot be handed to another task while it is held.
- `sync.Map` has no call that updates one entry in place. Use a
  `sync.Mutex[dict[K, V]]` for that.
