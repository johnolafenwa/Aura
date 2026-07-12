# Collections

Aurora provides three generic owned collection types:

- `Vec[T]` for ordered sequences
- `Map[K, V]` for key/value lookup
- `Set[T]` for uniqueness and membership tests

Collections are move types. Assigning a collection to another binding or passing it by value transfers ownership. Use `borrow` to inspect a collection without consuming it, and `borrow mut` to mutate a caller-owned collection.

## Literals And Constructors

Vector literals:

```python
values = [1, 2, 3]
empty: Vec[int32] = []
other = Vec[int32]()
```

Map literals:

```python
counts = {"ready": 1, "done": 2}
empty: Map[String, int32] = {}
other = Map[String, int32]()
```

Set literals require an expected set type because the same braces are used for maps:

```python
seen: Set[int32] = {1, 2, 3}
empty: Set[int32] = {}
other = Set[int32]()
```

Empty collection literals always need an expected type.

## Iteration

Vectors support by-value, shared-borrow, and mutable-borrow iteration:

```python
for value in values:
    print(value)

for value in borrow values:
    print(value)

for value in borrow mut values:
    value += 1
```

Maps and sets support by-value and shared-borrow iteration through their public surfaces. For maps, prefer `items()` or `entries()` when you want key/value pairs:

```python
for entry in counts.items():
    print(entry.key + "=" + entry.value.to_string())
```

`for value in borrow mut set:` is not supported. Mutate sets with `insert` and `remove`.

## Vec[T]

`Vec[T]` stores values in insertion order and indexes them with `int32`.

| API | Signature | Contract |
| --- | --- | --- |
| constructor | `Vec[T]()` | Creates an empty vector. |
| literal | `[a, b, c]` | Creates a vector whose element type is inferred from the elements or expected type. |
| `len` | `len() -> int32` | Returns the current number of elements. |
| `is_empty` | `is_empty() -> bool` | Returns `true` when `len() == 0`. |
| `clone` | `clone() -> Vec[T]` | Returns a new owned vector with cloned element values. |
| `push` | `push(value: T) -> None` | Moves `value` to the end of the vector. |
| `pop` | `pop() -> Option[T]` | Removes and returns the final element, or `None` when empty. |
| `get` | `get(index: int32) -> Option[T]` | Returns a cloned element, or `None` when `index` is out of bounds. |
| `set` | `set(index: int32, value: T) -> Option[T]` | Replaces the element at `index` and returns the previous element. Out-of-bounds indices raise a runtime error. |
| `remove` | `remove(index: int32) -> Option[T]` | Removes and returns the element at `index`. Out-of-bounds indices raise a runtime error. |
| `swap` | `swap(first: int32, second: int32) -> bool` | Swaps two elements and returns `true`. Out-of-bounds indices raise a runtime error. |
| `contains` | `contains(value: T) -> bool` | Returns `true` when an equal value is present. |
| `extend` | `extend(other: Vec[T]) -> None` | Moves every element from `other` to the end of the receiver. |
| `insert` | `insert(index: int32, value: T) -> bool` | Inserts `value` before `index` and returns `true`. Out-of-bounds indices raise a runtime error. |
| `clear` | `clear() -> None` | Removes all elements. |
| `reverse` | `reverse() -> None` | Reverses the vector in place. |

`get` is the safe lookup primitive. Use it when absence is a normal condition.

```python
match values.get(index):
    case Option.Some(value):
        print(value)
    case Option.None:
        print("missing")
```

`set`, `remove`, `swap`, and `insert` treat invalid indexes as runtime errors because they usually indicate a broken invariant. Use `get` before mutating when an out-of-range index is normal program data.

## Map[K, V]

`Map[K, V]` stores keys and values. Key equality uses Aurora equality for `K`.

| API | Signature | Contract |
| --- | --- | --- |
| constructor | `Map[K, V]()` | Creates an empty map. |
| literal | `{key: value}` | Creates a map from literal pairs. |
| `len` | `len() -> int32` | Returns the number of entries. |
| `is_empty` | `is_empty() -> bool` | Returns `true` when there are no entries. |
| `clone` | `clone() -> Map[K, V]` | Returns a new owned map with cloned keys and values. |
| `get` | `get(key: K) -> Option[V]` | Looks up `key` and returns a cloned value when present. |
| `set` | `set(key: K, value: V) -> Option[V]` | Inserts or replaces `key`, returning the previous value when present. |
| `remove` | `remove(key: K) -> Option[V]` | Removes `key`, returning the previous value when present. |
| `contains_key` | `contains_key(key: K) -> bool` | Returns `true` when `key` exists. |
| `keys` | `keys() -> Vec[K]` | Returns the current keys as cloned owned values. |
| `values` | `values() -> Vec[V]` | Returns the current values as cloned owned values. |
| `items` | `items() -> Vec[MapEntry[K, V]]` | Returns key/value entries in insertion order. |
| `entries` | `entries() -> Vec[MapEntry[K, V]]` | Same contract as `items()`. |
| `clear` | `clear() -> None` | Removes all entries. |
| `extend` | `extend(other: Map[K, V]) -> None` | Moves entries from `other`; matching keys are replaced. |

`MapEntry[K, V]` is the entry type returned by `items()` and `entries()`:

| Field | Type |
| --- | --- |
| `key` | `K` |
| `value` | `V` |

Because `get`, `set`, `remove`, and `contains_key` receive owned keys, clone the key when the caller needs to use it again:

```python
def bump(counts: borrow mut Map[String, int32], key: String):
    match counts.get(key.clone()):
        case Option.Some(count):
            counts.set(key, count + 1)
        case Option.None:
            counts.set(key, 1)
```

## Set[T]

`Set[T]` stores unique values.

| API | Signature | Contract |
| --- | --- | --- |
| constructor | `Set[T]()` | Creates an empty set. |
| literal | `{a, b, c}` with expected `Set[T]` | Creates a set. Duplicate values collapse to one entry. |
| `len` | `len() -> int32` | Returns the number of unique values. |
| `is_empty` | `is_empty() -> bool` | Returns `true` when there are no values. |
| `clone` | `clone() -> Set[T]` | Returns a new owned set with cloned values. |
| `contains` | `contains(value: T) -> bool` | Returns `true` when an equal value is present. |
| `insert` | `insert(value: T) -> bool` | Inserts `value`; returns `true` only when it was not already present. |
| `remove` | `remove(value: T) -> bool` | Removes `value`; returns `true` only when it was present. |

Set literals need a contextual type:

```python
ids: Set[int32] = {1, 2, 2, 3}
```

Use `Set[T]()` when there is no good annotation site:

```python
mut ids = Set[int32]()
ids.insert(42)
```

## Equality And Cloning

Collection equality compares contents. Vectors compare element order. Maps and sets compare their stored entries by key/value equality.

Methods that return values from a collection return owned values. For move element types, that means the returned value is cloned and the collection keeps its element. Methods that remove values, such as `pop` and `remove`, transfer the stored value out of the collection.

## Choosing The Right Collection

Use `Vec[T]` when order or duplicates matter.

Use `Map[K, V]` when you need to find or update a value by key.

Use `Set[T]` when the question is "have I seen this before?"
