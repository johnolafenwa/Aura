# Collections

Aura provides three generic owned collection types:

- `list[T]` for ordered sequences
- `dict[K, V]` for key/value lookup
- `set[T]` for uniqueness and membership

`str` is the immutable owned UTF-8 text type. Collections are move types. A
bare parameter or loop grants shared access, `mut` grants exclusive mutable
access, and `own` transfers the value.

## Literals And Constructors

List literals and constructors are homogeneous:

```aura
values = [1, 2, 3]
empty: list[int32] = []
other = list[int32]()
```

Dictionary literals evaluate entries from left to right, with each key before
its value. An equal key updates the value at the key's first insertion
position.

```aura
counts = {"ready": 1, "done": 2}
empty: dict[str, int32] = {}
other = dict[str, int32]()
```

Set literals use braces when a `set[T]` context is available. `{}` is a
dictionary literal, so an empty set uses its typed constructor.

```aura
seen: set[int32] = {1, 2, 3}
empty = set[int32]()
```

Every literal is homogeneous in each type position. Mixed list or set element
types, mixed dictionary key types, and mixed dictionary value types are
rejected with `AU2002`. Contextual integer-literal typing applies, but Aura
does not convert an already typed value or infer a union for a collection
literal.

## Iteration

Lists support shared, consuming, and mutable place iteration. Sets support
shared and consuming iteration. Mutable set iteration is unavailable.

```aura
for value in values:
    print(value)

for value in own values:
    consume(value)

for value in mut values:
    value += 1
```

Dictionaries expose eager owned snapshots. `items()` returns key/value tuples
in insertion order:

```aura
for key, value in counts.items():
    print(key + "=" + value.to_string())
```

Bare list and set iteration freezes the selected collection for the loop.
`own` selects and consumes the source once. Comprehensions use the bare shared
form and eagerly build fresh owned collections:

    squares = [value * value for value in values]
    even = {value for value in values if value % 2 == 0}
    labels = {value: str(value) for value in values}

Nested clauses execute in outer-major order. Filters run from left to right.
A dictionary comprehension evaluates its key before its value. Output storage
owns each produced element, key, and value.

## list[T]

`list[T]` preserves element order. `len(values)` and `values.len()` return
`int64`.

| Method | Signature | Contract |
| --- | --- | --- |
| `append` | `append(value: own T) -> None` | Transfers `value` to the end. |
| `pop` | `pop(index: int64 = -1) -> T` | Removes and transfers the normalized position. |
| `remove` | `remove(value: T) -> None` | Removes the first equal element. |
| `index` | `index(value: T) -> int64` | Returns the first position containing an equal element. |
| `count` | `count(value: T) -> int64` | Counts equal elements. |
| `insert` | `insert(index: int64, value: own T) -> None` | Transfers `value` before the clamped position. |
| `extend` | `extend(other: own list[T]) -> None` | Transfers all elements from `other` in order. |
| `clear` | `clear() -> None` | Removes all elements. |
| `reverse` | `reverse() -> None` | Reverses the list in place. |
| `sort` | `sort(reverse: bool = false) -> None` | Stably sorts an orderable list in place. |
| `sort` | `sort[K](key: def(T) -> K, reverse: bool = false) -> None` | Stably sorts by keys computed once per element. |
| `copy` | `copy() -> list[T]` | Returns independent owned storage; requires clone-safe `T`. |
| `get` | `get(index: int64) -> Option[T]` | Returns a cloned element or `None`; requires clone-safe `T`. |
| `set` | `set(index: int64, value: own T) -> T` | Replaces a position and transfers out its old element. |
| `swap` | `swap(first: int64, second: int64) -> None` | Swaps two positions. |
| `reserve` | `reserve(additional: int64) -> None` | Ensures room for `len() + additional` elements. |
| `with_capacity` | `list[T].with_capacity(minimum: int64) -> list[T]` | Creates an empty list with at least the requested capacity. |

`map` and `filter` remain eager source-retaining operations. `map` owns each
callback result. `filter` clones accepted elements and therefore requires a
clone-safe element type.

### Positions, Indexing, And Slicing

Direct indexing, `get`, `set`, `swap`, and `pop` normalize a negative position
once as `len() + index`. The result must be in `0..len()`. Invalid direct
positions and invalid `pop`, `set`, or `swap` positions trap with `AU4003`.
`get` returns `None` for an invalid position.

```aura
match values.get(index):
    case Option.Some(value):
        print(value)
    case Option.None:
        print("missing")
```

`pop()` selects the final element. It traps on an empty list. `remove(value)`
and `index(value)` search from the start and trap with `AU4008` when the value
is absent. `count(value)` returns zero when the value is absent.

```aura
def main():
    mut values = [10, 20, 30]
    print(values[-1])
    print(values.get(-2))
```

`insert` applies Python clamping. A negative input first adds the current
length. A result below zero becomes zero, and a result above the length becomes
the length. The value is inserted before that effective position.

List positions and written slice endpoints use the `int64` index domain.
Values of type `int8`, `int16`, `int32`, `uint8`, `uint16`, and `uint32` widen
losslessly at these positions. This position rule does not convert ordinary
assignments or function arguments.

One-colon slices return fresh owned lists. Endpoints are half-open, may be
omitted, and normalize negative values once. Both effective endpoints must be
in `0..=len()`, and the start must not exceed the end. Invalid or reversed
bounds trap with `AU4003`. List slices copy Copy elements and clone clone-safe
non-Copy elements.

`str` slicing uses the same position rules and counts Unicode scalar values.
It returns a fresh valid UTF-8 `str`. Integer indexing of `str` is unavailable.

### Stable Sorting

The canonical calls are:

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
Equal elements or keys retain their relative input order in both directions.
The key function runs exactly once per element from first to last, and all keys
are stored before the list mutates. Argument, key, ordering, or allocation
failure before mutation leaves the receiver unchanged.

## dict[K, V]

Dictionaries preserve insertion order for iteration and snapshots. Indexing,
assignment, and membership are the primary lookup and storage forms:

    value = table[key]
    table[key] = value
    present = key in table

| Method | Signature | Contract |
| --- | --- | --- |
| `get` | `get(key: K) -> Option[V]` | Returns a cloned value or `None`; requires clone-safe `V`. |
| `remove` | `remove(key: K) -> Option[V]` | Removes the entry and transfers its value, or returns `None`. |
| `keys` | `keys() -> list[K]` | Returns cloned keys in insertion order. |
| `values` | `values() -> list[V]` | Returns cloned values in insertion order. |
| `items` | `items() -> list[(K, V)]` | Returns cloned key/value tuples in insertion order. |
| `copy` | `copy() -> dict[K, V]` | Returns independent owned storage. |
| `update` | `update(other: own dict[K, V]) -> None` | Transfers entries from `other` in insertion order. |
| `clear` | `clear() -> None` | Removes all entries. |
| `reserve` | `reserve(additional: int64) -> None` | Ensures room for `len() + additional` entries. |
| `with_capacity` | `dict[K, V].with_capacity(minimum: int64) -> dict[K, V]` | Creates an empty dictionary with at least the requested capacity. |

`keys()` and `copy()` require clone-safe `K`; `values()` requires clone-safe
`V`; `items()` requires both. These methods return eager snapshots, not live
views. `get` accepts no default argument. Absence is represented by
`Option[V]`.

```aura
def bump(counts: mut dict[str, int32], key: own str):
    match counts.get(key):
        case Option.Some(count):
            counts[key] = count + 1
        case Option.None:
            counts[key] = 1
```

An indexed read follows the collection ownership rule for `V` and traps with
`AU4003` when the key is absent. Indexed assignment transfers its key and value
as needed. It inserts an absent key and updates an equal key without changing
the key's insertion position. `update` applies the same position rule.

## set[T]

Sets store one value per equality class. Membership uses `in` and `not in`.

| Method | Signature | Contract |
| --- | --- | --- |
| `add` | `add(value: own T) -> None` | Transfers a value into the set. |
| `remove` | `remove(value: T) -> None` | Removes an equal value; absence traps with `AU4008`. |
| `discard` | `discard(value: T) -> None` | Removes an equal value when present. |
| `copy` | `copy() -> set[T]` | Returns independent owned storage; requires clone-safe `T`. |
| `clear` | `clear() -> None` | Removes all values. |
| `reserve` | `reserve(additional: int64) -> None` | Ensures room for `len() + additional` values. |
| `with_capacity` | `set[T].with_capacity(minimum: int64) -> set[T]` | Creates an empty set with at least the requested capacity. |

`add`, `remove`, `discard`, and membership require equality for `T`. Mutating
methods return `None`. Callers use membership to distinguish presence.

```aura
def main():
    mut ids = set[int32]()
    ids.add(42)
    ids.discard(7)
```

A non-empty set renders as `{first, second}` in its defined iteration order.
An empty set renders as `set()`.

## Equality, Copying, And Capacity

Lists compare elements in order. Dictionaries compare equal key/value mappings.
Sets compare equal membership. Equality consumes neither operand.

`copy` creates independent owned storage under the stated clone-safety
requirements. Removing methods transfer stored values. Shared lookup and
search operations retain the collection and their arguments.

`reserve(additional)` guarantees capacity of at least `len() + additional`.
`with_capacity(minimum)` creates an empty collection with capacity of at least
`minimum`. These operations do not change contents or order. A negative value
traps with `AU4003`. Overflow, maintained-limit violations, and allocation
failure trap with `AU4005`; a failed reserve leaves the receiver unchanged.

This executable example covers collection literals, eager algorithms, stable
sorting, set deduplication, and comprehension order:

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

The normative productions for literals, comprehensions, constructors,
indexing, slicing, indexed assignment, method calls, and loop ownership modes
are in [Grammar](/manual/grammar). The first colon in a non-empty brace literal
selects dictionary syntax. `{}` is a dictionary literal.

## Typing Rules

Collection specializations are invariant and homogeneous. Empty literals need
an expected type. Mutating methods and indexed assignment require a mutable
collection place. Direct dictionary reads follow the value ownership contract;
`get` provides an optional cloned read for clone-safe `V`, and `remove`
transfers any stored `V`.

List callbacks use exact shared function types. `map` requires
`def(T) -> U`, `filter` requires `def(T) -> bool`, and keyed sorting requires
`def(T) -> K` with `K: Ord`. The callback must be repeatable.

## Runtime Semantics

All collection expressions evaluate once from left to right. Lists and
dictionaries preserve their specified order. Sets collapse equal duplicates.
Comprehensions are eager and execute as nested loops. A trap cleans up any
partially created collection.

## Ownership And Evaluation Order

Collection storage positions own non-Copy elements, keys, and values. Shared
lookups and searches retain their inputs. `append`, `insert`, `extend`, `add`,
`update`, indexed assignment, and comprehension output transfer owned values.
Slices and `copy` produce independent storage. No collection operation inserts
a hidden clone.

## Diagnostics

`AU2001` reports unknown collection types and members. `AU2002` reports type,
arity, homogeneity, and callback mismatches. `AU3001` reports use after move;
`AU3002` reports conflicting access; `AU3003` reports mutation through an
immutable place; `AU3005`, `AU3006`, `AU3007`, and `AU3009` report ownership or
clone-safety violations. `AU4003` reports invalid positions and missing direct
dictionary keys. `AU4005` reports allocation and capacity failures. `AU4008`
reports a missing value for list `remove`/`index` and set `remove`.

## Backend Support

The MIR and direct backends implement the same collection types, methods,
ordering, ownership, evaluation, rendering, and diagnostic behavior. Compiler
analysis and the language server consume the same builtin signatures.

## Limits And Implementation-Defined Behavior

Mutable set iteration and direct dictionary iteration are unavailable. Set
algebra and relations are outside this surface. Arbitrary user-defined
iterables, generator expressions, slice steps, slice assignment, views, and
`str` integer indexing are unavailable. Set order is not an API contract.
Allocation is limited by available host resources and maintained runtime caps.

## Status

The collection contract in this chapter is Accepted by ADR-0044 and is the
canonical Aura 0.3 surface.
