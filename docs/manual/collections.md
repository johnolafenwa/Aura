# Collections

This page covers Aura's three generic owned collection types:

- `list[T]` for ordered sequences
- `dict[K, V]` for key/value lookup
- `set[T]` for uniqueness and membership

`str` is the immutable owned UTF-8 text type. Collections are move types. A
bare parameter or loop grants shared access. `mut` grants exclusive mutable
access, and `own` transfers the value.

## Literals And Constructors

List literals and constructors are homogeneous:

```aura
values = [1, 2, 3]
empty: list[int32] = []
other = list[int32]()
```

A dictionary literal evaluates its entries from left to right, each key before
its value. A repeated equal key updates the value in the slot where the key
was first inserted.

```aura
counts = {"ready": 1, "done": 2}
empty: dict[str, int32] = {}
other = dict[str, int32]()
```

A set literal uses braces when the context expects a `set[T]`. `{}` is a
dictionary literal, so an empty set needs its typed constructor.

```aura
seen: set[int32] = {1, 2, 3}
empty = set[int32]()
```

Every literal is homogeneous in each type position. The checker rejects mixed
list or set element types, mixed dictionary key types, and mixed dictionary
value types with `AU2002`. Integer literals take their type from context.
Aura does not convert a value that already has a type, and it does not infer
a union for a collection literal.

### Equality Requirement

An operation that compares stored values needs equality for the relevant
type. These operations compare values:

- list `remove`, `index`, and `count`
- `in` and `not in`
- set insertion
- every dictionary-key operation

Callables, `random.Rng`, opaque FFI handles, and values that contain any of
them have no equality. The compiler rejects the operation with `AU2008` and
names the unmet requirement.

## Iteration

Lists support shared, consuming, and mutable place iteration. Sets support
shared and consuming iteration, but not mutable iteration.

```aura fragment
for value in values:
    print(value)

for value in own values:
    consume(value)

for value in mut values:
    value += 1
```

A bare list or set loop freezes the collection for the duration of the loop.
`own` selects the source once and consumes it.

Dictionaries are not iterated directly. They expose eager owned snapshots.
`items()` returns key/value tuples in insertion order:

```aura fragment
for key, value in counts.items():
    print(key + "=" + value.to_string())
```

Comprehensions use the bare shared form and eagerly build fresh owned
collections:

    squares = [value * value for value in values]
    even = {value for value in values if value % 2 == 0}
    labels = {value: str(value) for value in values}

Nested clauses run in outer-major order, and filters run from left to right. A
dictionary comprehension evaluates its key before its value. The output owns
each element, key, and value it produces.

## list[T]

`list[T]` keeps elements in order. `len(values)` and `values.len()` return
`int64`.

`view name = values[index]` and `view mut name = values[index]` bind one
element in place. You can read or assign a field of a class element through
the index, as in `values[index].field`, without copying the element. See
[Statements](/manual/statements).

| Method | Signature | Contract |
| --- | --- | --- |
| `append` | `append(value: own T) -> None` | Moves `value` to the end. |
| `pop` | `pop(index: int64 = -1) -> T` | Removes the element at the normalized position and moves it out. |
| `remove` | `remove(value: T) -> None` | Removes the first equal element. |
| `index` | `index(value: T) -> int64` | Returns the position of the first equal element. |
| `count` | `count(value: T) -> int64` | Counts equal elements. |
| `insert` | `insert(index: int64, value: own T) -> None` | Moves `value` in before the clamped position. |
| `extend` | `extend(other: own list[T]) -> None` | Moves every element of `other` to the end, in order. |
| `clear` | `clear() -> None` | Removes all elements. |
| `reverse` | `reverse() -> None` | Reverses the list in place. |
| `sort` | `sort(reverse: bool = false) -> None` | Stably sorts an orderable list in place. |
| `sort` | `sort[K](key: def(T) -> K, reverse: bool = false) -> None` | Stably sorts by keys computed once per element. |
| `copy` | `copy() -> list[T]` | Returns independent owned storage. Requires clone-safe `T`. |
| `get` | `get(index: int64) -> Lookup[T]` | Returns `Lookup.Found(value)` with a cloned element, or `Lookup.Missing`. Requires clone-safe `T`. |
| `set` | `set(index: int64, value: own T) -> T` | Replaces the element at a position and moves the old element out. |
| `swap` | `swap(first: int64, second: int64) -> None` | Swaps two positions. |
| `reserve` | `reserve(additional: int64) -> None` | Ensures room for `len() + additional` elements. |
| `with_capacity` | `list[T].with_capacity(minimum: int64) -> list[T]` | Creates an empty list with at least the requested capacity. |

`map` and `filter` are eager and leave the source list in place. `map` owns
each callback result. `filter` clones the accepted elements, so it needs a
clone-safe element type.

### Positions, Indexing, And Slicing

Direct indexing, `get`, `set`, `swap`, and `pop` normalize a negative position
once as `len() + index`. The result must be in `0..len()`.

- An invalid direct index, or an invalid `pop`, `set`, or `swap` position,
  traps with `AU4003`.
- `get` returns `Lookup.Missing` for an invalid position. A present element
  whose value is `None` stays distinct from an absent one.

```aura fragment
match values.get(index):
    case Lookup.Found(value):
        print(value)
    case Lookup.Missing:
        print("missing")
```

```aura
def main():
    mut values = [10, 20, 30]
    print(values[-1])
    print(values.get(-2))
```

`pop()` removes the final element and traps on an empty list. `remove(value)`
and `index(value)` search from the start and trap with `AU4008` when the value
is absent. `count(value)` returns zero when the value is absent.

`insert` clamps the way Python does:

1. A negative index first has the current length added to it.
2. A result below zero becomes zero.
3. A result above the length becomes the length.

The value goes in before that effective position.

List positions and written slice endpoints are in the `int64` index domain.
`int8`, `int16`, `int32`, `uint8`, `uint16`, and `uint32` values widen
losslessly in these positions. This rule applies only to positions. It does
not convert ordinary assignments or function arguments.

A one-colon slice returns a fresh owned list:

- Endpoints are half-open and may be omitted.
- A negative endpoint is normalized once.
- Both effective endpoints must be in `0..=len()`, and the start must not
  exceed the end. Invalid or reversed bounds trap with `AU4003`.
- The slice copies Copy elements and clones clone-safe non-Copy elements.

`str` slicing uses the same position rules, counts Unicode scalar values, and
returns a fresh valid UTF-8 `str`. You cannot index a `str` with an integer.

### Stable Sorting

These are the four forms of `sort`:

```aura
def make_key(value: int64) -> int64:
    return -value

def main():
    mut values = [3, 1, 2]
    values.sort()
    values.sort(reverse=true)
    values.sort(key=make_key)
    values.sort(key=make_key, reverse=true)
```

Natural sorting requires `T: Ord`. Key sorting requires an orderable key type.
Equal elements or keys keep their input order in both directions.

The key function runs exactly once per element, from first to last. All keys
are stored before the list changes. If an argument, key, ordering, or
allocation fails before that point, the list is unchanged.

## dict[K, V]

Dictionaries keep insertion order for iteration and snapshots. Indexing,
assignment, and membership are the main ways to look up and store entries:

    value = table[key]
    table[key] = value
    present = key in table

| Method | Signature | Contract |
| --- | --- | --- |
| `get` | `get(key: K) -> Lookup[V]` | Returns `Lookup.Found(value)` with a cloned value, or `Lookup.Missing` when the key is absent. Requires clone-safe `V`. |
| `remove` | `remove(key: K) -> Lookup[V]` | Removes the entry and moves its value into `Lookup.Found(value)`, or returns `Lookup.Missing`. |
| `keys` | `keys() -> list[K]` | Returns cloned keys in insertion order. |
| `values` | `values() -> list[V]` | Returns cloned values in insertion order. |
| `items` | `items() -> list[(K, V)]` | Returns cloned key/value tuples in insertion order. |
| `copy` | `copy() -> dict[K, V]` | Returns independent owned storage. |
| `update` | `update(other: own dict[K, V]) -> None` | Moves the entries of `other` into the dictionary in insertion order. |
| `clear` | `clear() -> None` | Removes all entries. |
| `reserve` | `reserve(additional: int64) -> None` | Ensures room for `len() + additional` entries. |
| `with_capacity` | `dict[K, V].with_capacity(minimum: int64) -> dict[K, V]` | Creates an empty dictionary with at least the requested capacity. |

Clone-safety requirements for the snapshot methods:

- `keys()` and `copy()` require clone-safe `K`.
- `values()` requires clone-safe `V`.
- `items()` requires both.

These methods return eager snapshots, not live views. For a live entry, use
`view name = table[key]` for shared access or `view mut name = table[key]` for
mutable access. See [Statements](/manual/statements).

`get` takes no default argument. `Lookup[V]` represents absence: a missing key
gives `Lookup.Missing`. A present value gives `Lookup.Found(value)`, even when
`V` is an optional `T | None` and the stored value is `None`.

```aura
def bump(counts: mut dict[str, int32], key: own str):
    match counts.get(key):
        case Lookup.Found(count):
            counts[key] = count + 1
        case Lookup.Missing:
            counts[key] = 1
```

An indexed read follows the collection ownership rule for `V`. It traps with
`AU4003` when the key is absent. Indexed assignment moves its key and value
in as needed. It inserts an absent key. For an equal key, it updates the value
and keeps the key's insertion position. `update` follows the same position
rule.

## set[T]

A set stores one value per equality class. Test membership with `in` and
`not in`.

| Method | Signature | Contract |
| --- | --- | --- |
| `add` | `add(value: own T) -> None` | Moves a value into the set. |
| `remove` | `remove(value: T) -> None` | Removes an equal value. Traps with `AU4008` when the value is absent. |
| `discard` | `discard(value: T) -> None` | Removes an equal value if one is present. |
| `copy` | `copy() -> set[T]` | Returns independent owned storage. Requires clone-safe `T`. |
| `clear` | `clear() -> None` | Removes all values. |
| `reserve` | `reserve(additional: int64) -> None` | Ensures room for `len() + additional` values. |
| `with_capacity` | `set[T].with_capacity(minimum: int64) -> set[T]` | Creates an empty set with at least the requested capacity. |

`add`, `remove`, `discard`, and membership require equality for `T`. The
mutating methods return `None`, so use membership to check whether a value is
present.

```aura
def main():
    mut ids = set[int32]()
    ids.add(42)
    ids.discard(7)
```

A non-empty set prints as `{first, second}` in its iteration order. An empty
set prints as `set()`.

## Equality, Copying, And Capacity

Equality consumes neither operand:

- Lists are equal when their elements are equal in order.
- Dictionaries are equal when they hold equal key/value mappings.
- Sets are equal when they have equal members.

`copy` creates independent owned storage, subject to the clone-safety
requirements above. Removing methods move stored values out. Shared lookup and
search operations leave the collection and their arguments in place.

`reserve(additional)` guarantees a capacity of at least `len() + additional`.
`with_capacity(minimum)` creates an empty collection with a capacity of at
least `minimum`. Neither changes contents or order.

- A negative value traps with `AU4003`.
- Overflow, a runtime-limit violation, or allocation failure traps with
  `AU4005`.
- A failed `reserve` leaves the collection unchanged.

This program exercises collection literals, eager algorithms, stable sorting,
set deduplication, and comprehension order:

```aura
def doubled(value: int32) -> int32:
    return value * 2

def is_even(value: int32) -> bool:
    return value % 2 == 0

def descending_key(value: int32) -> int32:
    return -value

def main():
    values: list[int32] = [3, 1, 2, 4]
    middle = values[1:3]
    mapped = values.map(doubled)
    filtered = values.filter(is_even)

    mut ascending = values.copy()
    ascending.sort()

    mut descending = values.copy()
    descending.sort(key=descending_key)

    squares = [value * value for value in values]
    even_squares = [value * value for value in values if value % 2 == 0]
    remainders: set[int32] = {value % 3 for value in values}
    labels = {value: value * 10 for value in values if value >= 3}
    pairs = [
        left * 10 + right
        for left in values if left < 3
        for right in values if right < 3
    ]

    assert 0 in remainders
    assert 1 in remainders
    assert 2 in remainders

    print(middle)
    print(mapped)
    print(filtered)
    print(ascending)
    print(descending)
    print(values)
    print(squares)
    print(even_squares)
    print(labels)
    print(pairs)
```

## Grammar

[Grammar](/manual/grammar) defines the productions for literals,
comprehensions, constructors, indexing, slicing, indexed assignment, method
calls, and loop ownership modes. In a non-empty brace literal, the first colon
selects dictionary syntax. `{}` is a dictionary literal.

## Typing Rules

Collection specializations are invariant and homogeneous. An empty literal
needs an expected type. Mutating methods and indexed assignment need a mutable
collection place.

A direct dictionary read follows the ownership rule for `V`. `get` gives an
optional cloned read for clone-safe `V`. `remove` moves out any stored `V`.

List callbacks must have exact shared function types:

| Operation | Callback type |
| --- | --- |
| `map` | `def(T) -> U` |
| `filter` | `def(T) -> bool` |
| keyed `sort` | `def(T) -> K` with `K: Ord` |

The callback must be repeatable. It can be a named function, a repeatable
closure, or a packed Shared `Callable[...]` or `TaskCallable[...]` value with
the matching contract. The operation borrows a packed value for the call. It
does not clone the value or erase its contract.

- A Mutable or Consuming packed value is rejected with `AU2002`.
- The operation calls the callback positionally, so a keyword-only element
  parameter is rejected with `AU2004`.

## Runtime Semantics

Every collection expression evaluates once, from left to right. Lists and
dictionaries keep the order described on this page. Sets collapse equal
duplicates. Comprehensions are eager and run as nested loops. If a trap
occurs, any partly built collection is cleaned up.

## Ownership And Evaluation Order

Collection storage owns its non-Copy elements, keys, and values. Shared
lookups and searches leave their inputs in place.

- `append`, `insert`, `extend`, `add`, `update`, indexed assignment, and
  comprehension output move owned values in.
- Slices and `copy` produce independent storage.
- No collection operation inserts a hidden clone.

## Diagnostics

| Code | Cause |
| --- | --- |
| `AU2001` | Unknown collection type or member. |
| `AU2002` | Type, arity, homogeneity, or callback mismatch. |
| `AU3001` | Use after move. |
| `AU3002` | Conflicting access. |
| `AU3003` | Mutation through an immutable place. |
| `AU3005`, `AU3006`, `AU3007`, `AU3009` | Ownership or clone-safety violation. |
| `AU4003` | Invalid position, or a missing key in a direct dictionary read. |
| `AU4005` | Allocation or capacity failure. |
| `AU4008` | Missing value for list `remove` or `index`, or for set `remove`. |

## Backend Support

The MIR and direct backends implement the same collection types, methods,
ordering, ownership, evaluation, rendering, and diagnostics. The compiler's
analysis and the language server use the same builtin signatures.

## Limits And Implementation-Defined Behavior

These are not available:

- mutable set iteration and direct dictionary iteration
- set algebra and set relations
- arbitrary user-defined iterables and generator expressions
- slice steps, slice assignment, and slice views
- integer indexing of `str`

Set order is not part of the API contract. Allocation is limited by host
resources and the runtime's size caps.

## Status

The collection contract on this page is the Aura 0.3 surface.

Design record: [ADR-0044: Canonical collection surface](https://github.com/johnolafenwa/Aura/blob/main/architecture_docs/decisions/0044-canonical-collection-surface.md).
