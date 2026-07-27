# Collections

Aurora provides three generic owned collection types:

- `Vec[T]` for ordered sequences
- `Map[K, V]` for key/value lookup
- `Set[T]` for uniqueness and membership tests

Collections are move types. Assigning one to another binding transfers ownership. A bare non-copy parameter or loop borrows it by default; use `own` to transfer it deliberately, `` to make shared access explicit, and `mut ` to mutate caller-owned storage.

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

`Set{a, b, c}` is the explicit set-literal form. In particular, `Set{}`
parses unambiguously as a set, although an empty explicit set still needs an
expected `Set[T]` type because it has no element from which to infer `T`.

Empty collection literals always need an expected type.

## Iteration

Vectors support default shared, explicit consuming, explicit shared, and mutable-borrow iteration:

```python
for value in values:
    print(value)

for value in own values:
    consume(value)

for value in values:
    print(value)

for value in mut values:
    value += 1
```

Sets support default shared, `own`, and explicit shared iteration. Maps expose
iteration through returned collection values; prefer `items()` or `entries()`
when you want key/value pairs:

```python
for entry in counts.items():
    print(entry.key + "=" + entry.value.to_string())
```

`for value in mut set:` is not supported. Mutate sets with `insert` and `remove`.

Bare or explicit shared Vec/Set iteration freezes the selected collection for
the loop. `own` iteration instead moves the collection once into a loop-private
source. Reinitializing the consumed source binding in the body cannot switch or
truncate the active iteration. Accepted ADR-0017 records this one-time
source-selection rule without changing ADR-0006's loop ownership modes.

## Vec[T]

`Vec[T]` stores values in insertion order and indexes them with `int32`.

Every Vec indexing surface follows one rule: a negative index `i` is
normalized once as `len + i`. This applies to direct `[]` reads and writes,
`get`, `set`, `remove`, both indexes passed to `swap`, and `insert`. After
normalization, each operation keeps its normal bounds contract.

| API | Signature | Contract |
| --- | --- | --- |
| constructor | `Vec[T]()` | Creates an empty vector. |
| literal | `[a, b, c]` | Creates a vector whose element type is inferred from the elements or expected type. |
| `len` | `len() -> int64` | Returns the current number of elements. |
| `is_empty` | `is_empty() -> bool` | Returns `true` when `len() == 0`. |
| `clone` | `clone() -> Vec[T]` | Returns a new owned vector with cloned element values; requires clone-safe `T`. |
| `push` | `push(value: own T) -> None` | Moves `value` to the end of the vector. |
| `pop` | `pop() -> Option[T]` | Removes and returns the final element, or `None` when empty. |
| `get` | `get(index: int32) -> Option[T]` | Returns a cloned element after normalization, or `None` when the normalized index is out of bounds; requires clone-safe `T`. |
| `set` | `set(index: int32, value: own T) -> Option[T]` | Replaces the normalized index and returns the previous element. Out-of-bounds indices raise a runtime error. |
| `remove` | `remove(index: int32) -> Option[T]` | Removes and returns the normalized index. Out-of-bounds indices raise a runtime error. |
| `swap` | `swap(first: int32, second: int32) -> bool` | Normalizes both indexes, swaps the elements, and returns `true`. Out-of-bounds indices raise a runtime error. |
| `contains` | `contains(value: T) -> bool` | Returns `true` when an equal value is present. |
| `extend` | `extend(other: own Vec[T]) -> None` | Moves every element from `other` to the end of the receiver. |
| `insert` | `insert(index: int32, value: own T) -> bool` | Normalizes `index`, inserts `value` before it, and returns `true`. The valid normalized range is `0..=len`. |
| `clear` | `clear() -> None` | Removes all elements. |
| `reverse` | `reverse() -> None` | Reverses the vector in place. |

`get` is the safe lookup primitive when absence is a normal condition and `T`
is clone-safe. Use `remove` when the stored value must be transferred instead.
Direct indexed reads and compound indexed assignment require copy `T`; simple
indexed assignment may store any `T`. For a clone-safe non-copy
read-modify-write, use an explicit `get`; use `remove` when ownership must be
transferred, including for a value that contains `random.Rng`.

```python
match values.get(index):
    case Option.Some(value):
        print(value)
    case Option.None:
        print("missing")
```

Negative indexes count from the end:

```python
mut values = [10, 20, 30]
print(values[-1])             # 30
print(values.get(-2))         # Option.Some(20)
values[-1] = 31               # writes the final element
values.insert(-1, 25)         # [10, 20, 25, 31]
end_index = values.len() as int32
values.insert(end_index, 40)  # appends
```

Vec lengths are `int64`, while Vec index arguments remain exactly `int32`.
Passing a computed length to an index API therefore requires an explicit
checked `as int32` cast, as in the append example above. Integer literals at
index sites continue to adopt the expected `int32` type directly.

`set`, `remove`, `swap`, and `insert` treat invalid indexes as runtime errors because they usually indicate a broken invariant. Use `get` before mutating when an out-of-range index is normal program data.

Aurora deliberately differs from Python for insertion indexes. Python clamps
an extremely negative `list.insert` index to the start; Aurora does not clamp
an index that remains out of range after normalization. For example,
`values.insert(-999, value)` raises a runtime error instead of silently placing
`value` at the wrong position. `get(-999)` follows its existing optional
contract and returns `None`.

## Map[K, V]

`Map[K, V]` stores keys and values. Key equality uses Aurora equality for `K`.

| API | Signature | Contract |
| --- | --- | --- |
| constructor | `Map[K, V]()` | Creates an empty map. |
| literal | `{key: value}` | Creates a map in source order; a later equal key replaces the earlier value while retaining the key's first insertion slot. |
| indexed read | `map[key] -> V` for copy `V` | Returns the stored copy value; a missing key traps with `AU4003`. Non-copy `V` is rejected. |
| indexed assignment | `map[key] = value` for any `V` | Owns the key and value, consuming either when non-copy, then inserts or replaces without an absence trap. |
| compound indexed assignment | `map[key] op= rhs` for copy `V` | Copies the stored value, applies `op` with `rhs`, and stores the result. A missing key traps with `AU4003`; non-copy `V` is rejected. |
| `len` | `len() -> int64` | Returns the number of entries. |
| `is_empty` | `is_empty() -> bool` | Returns `true` when there are no entries. |
| `clone` | `clone() -> Map[K, V]` | Returns a new owned map with cloned keys and values; requires clone-safe `K` and `V`. |
| `get` | `get(key: K) -> Option[V]` | Looks up `key` and returns a cloned value when present; requires clone-safe `V`. |
| `set` | `set(key: own K, value: own V) -> Option[V]` | Inserts or replaces `key`, returning the previous value when present. |
| `remove` | `remove(key: K) -> Option[V]` | Removes `key`, returning the previous value when present. |
| `contains_key` | `contains_key(key: K) -> bool` | Returns `true` when `key` exists. |
| `keys` | `keys() -> Vec[K]` | Returns cloned owned keys in insertion order; requires clone-safe `K`. |
| `values` | `values() -> Vec[V]` | Returns cloned owned values in their keys' insertion order; requires clone-safe `V`. |
| `items` | `items() -> Vec[MapEntry[K, V]]` | Returns cloned key/value entries in insertion order; requires clone-safe `K` and `V`. |
| `entries` | `entries() -> Vec[MapEntry[K, V]]` | Same clone-safety and ordering contract as `items()`. |
| `clear` | `clear() -> None` | Removes all entries. |
| `extend` | `extend(other: own Map[K, V]) -> None` | Moves entries from `other`; matching keys are replaced. |

`MapEntry[K, V]` is the entry type returned by `items()` and `entries()`:

| Field | Type |
| --- | --- |
| `key` | `K` |
| `value` | `V` |

`get`, `remove`, and `contains_key` retain their keys: copy keys are copied and
non-copy keys are shared-borrowed. `set` owns and stores its key and value. No
clone is needed for lookup followed by insertion:

```python
def bump(counts: mut Map[String, int32], key: own String):
    match counts.get(key):
        case Option.Some(count):
            counts.set(key, count + 1)
        case Option.None:
            counts.set(key, 1)
```

Map literal entries evaluate from left to right, each key before its value. If
a later key equals an earlier key, the later value replaces the earlier value
without moving that key's first insertion slot. The later key and value are
still evaluated and pass through their owned literal positions.

Direct `map[key]` is available only when `V` is copyable. Its lookup key uses
the same retained mode: copy `K` is copied and non-copy `K` is shared-borrowed.
The read returns the stored copy value; an absent key is a runtime `AU4003`
trap. For clone-safe non-copy values, `get(key)` provides an explicit cloned
optional read; `remove(key)` transfers any stored value out and is required for
values containing `random.Rng`. Simple indexed assignment is
the storing counterpart and accepts every `V`: its key and value are owned
positions matching `set(key: own K, value: own V)`, so either is consumed when
non-copy. The key evaluates and is captured before the right-hand value is
evaluated; value-side effects cannot change the key already selected for
storage. Assignment inserts an absent key or replaces an equal key's value and
does not trap merely because the key was absent. `set`, indexed assignment,
literal duplicate replacement, and `extend` preserve an equal existing key's
insertion slot.

Compound Map indexed assignment is narrower. `map[key] op= rhs` is permitted
only for copy `V`, because it must first read the stored value and later write
the operator result. A missing key traps with `AU4003` at that read. For
non-copy `V`, Aurora neither inserts an implicit clone nor destructively removes
the stored value before an operation that may fail.
Use `get(key)` when `V` is clone-safe, or `remove(key)` to take ownership and
perform an explicit simple assignment.

## Set[T]

`Set[T]` stores unique values.

| API | Signature | Contract |
| --- | --- | --- |
| constructor | `Set[T]()` | Creates an empty set. |
| literal | `{a, b, c}` with expected `Set[T]` | Creates a set. Duplicate values collapse to one entry. |
| `len` | `len() -> int64` | Returns the number of unique values. |
| `is_empty` | `is_empty() -> bool` | Returns `true` when there are no values. |
| `clone` | `clone() -> Set[T]` | Returns a new owned set with cloned values; requires clone-safe `T`. |
| `contains` | `contains(value: T) -> bool` | Returns `true` when an equal value is present. |
| `insert` | `insert(value: own T) -> bool` | Inserts `value`; returns `true` only when it was not already present. |
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

Collection methods return owned values. Non-removing reads such as `get`,
`keys`, `values`, `items`, and `entries` explicitly clone move values so the
collection retains its contents. `pop` and `remove` transfer the removed stored
value, and `set` transfers the displaced previous value while storing its new
owned replacement.

## Choosing The Right Collection

Use `Vec[T]` when order or duplicates matter.

Use `Map[K, V]` when you need to find or update a value by key.

Use `Set[T]` when the question is "have I seen this before?"

## Grammar

The normative productions for list, map, set, and empty collection literals;
generic collection types; indexing; indexed assignment; method calls; and
loop ownership modifiers are in [Grammar](/manual/grammar). The first colon in
a nonempty brace literal selects map syntax. `{}` is grammatically a map but
may type as an empty `Set[T]` under an expected set type. `Set{...}` is the
explicit set-literal form, and `Set[T]()` is the typed constructor form.

## Typing Rules

Every collection is homogeneous under one exact invariant specialization.
Literal elements, keys, and values must agree after contextual literal typing,
and empty literals require an expected collection type. Vec indexes have exact
type `int32`; Map indexes and lookup keys have exact type `K`. Mutating and
retaining APIs use the owned positions shown in the tables above, and every
mutating method or indexed assignment requires a mutable collection place.
Bare or explicit shared Vec/Set iteration borrows the collection, `own`
consumes it, and `mut ` is supported only for a mutable Vec place. A
direct Map read requires copy `V`; a cloned non-copy read uses `get` only when
`V` is clone-safe, while `remove` transfers any `V`. Simple
indexed assignment accepts any Vec element or Map value type and owns the
assigned value; a Map also owns its key. Compound indexed assignment requires
copy `T` for Vec and copy `V` for Map.

Clone-producing APIs infer obligations when their relevant element, key, or
value types are unresolved. The obligation is checked after specialization and
propagates through generic callers; a produced type containing `random.Rng` is
rejected with `AU3007`.

## Runtime Semantics

Vec preserves element order. Literal elements evaluate in source order; a map
evaluates each key before its value and entries in source order. Set collapses
equal duplicates and compares by contents. Map and Set equality ignores
insertion order, while Vec equality includes order. Map `items()` and
`entries()` explicitly return insertion order; `keys()` and `values()` return
the corresponding insertion-ordered projections. Every Vec index is
normalized once with `len + i` when negative and then checked under that
operation's documented range. Vec `get` returns `None` for absence; the
mutating Vec index methods trap on an invalid normalized index. A later equal
Map-literal key replaces the earlier value and preserves the first slot. A
missing direct Map index traps with `AU4003`; simple indexed assignment inserts
or replaces for any `V`; compound indexed assignment requires a copy Vec
element or Map value and traps with `AU4003` when a Map key is absent.

## Ownership And Evaluation Order

Collection literals, storing methods, and simple Map indexed assignment own
every stored non-copy element, key, and value. In simple Map indexed assignment
the key is fully evaluated and captured, then consumed when non-copy, before the
assigned value is evaluated or consumed. Methods documented as cloned reads,
including `get`, create an explicit new owned structural value only after their
clone-safety obligations are satisfied; `pop` and
`remove` transfer stored values out. A direct Map read returns only a copy
`V`, so it never hides a clone. Bare/borrow iteration freezes the iterated
place, own iteration moves once into a loop-private source, and mutable Vec
iteration writes through its exclusive borrow. Reinitializing an own-iteration
source binding does not change the active loop. Non-copy Vec/Map elements are
rejected as direct compound targets rather than implicitly cloned or moved.
Collection and argument expressions retain the language-wide left-to-right
order.

## Diagnostics

`AU1101` reports malformed literal, index, method-call, or loop syntax.
`AU2001` reports unknown collection types and members. `AU2002` covers literal
element/key/value mismatch, missing empty-literal context, generic arity,
index/key type mismatch, and method argument type mismatch. `AU2004` reports
invalid constructor or method argument binding. `AU2005` supplies the focused
Python-migration guidance for `len(...)`, `.append(...)`, `in`, and
comprehensions. `AU2999` covers unsupported collection methods, non-indexable
values, and remaining static collection rejections. `AU3001` reports use after moving a
stored value, indexed-assignment key, or consuming collection. `AU3002`
reports mutation/move while a collection or element is borrowed and invalid
mutable iteration. `AU3003` reports mutation through an immutable collection
place. `AU3005` rejects a non-copy direct Vec/Map indexed read, and `AU3006`
rejects a non-copy Vec/Map indexed compound assignment. `AU3007` rejects a
clone-producing collection operation whose result contains or may contain
non-cloneable `random.Rng` state. `AU4003` (`bounds or lookup violation`) reports an out-of-range Vec
index or a missing direct Map key.
Optional absence from `get` and Boolean absence from Set/Map membership or
removal are typed values, not diagnostics.

## Backend Support

Vec, Map, and Set literals, equality, cloning, iteration, indexing, mutation,
and the complete method tables above are implemented for MIR execution and
direct native generation. Both backends use the same static collection types
and are parity-tested for duplicate-key replacement, key-before-value effects,
indexed reads and writes, missing-key traps, and maintained observable
behavior. Compiler analysis and the LSP consume those same builtin signatures.

## Limits And Implementation-Defined Behavior

Mutable Set iteration and direct Map iteration are unavailable; iterate the
owned Vec snapshots returned by Map methods. Arbitrary user-defined iterables,
comprehensions, and trailing commas are unavailable; collection literals may
span physical lines through their own delimiters. The slice surface is
reserved for Phase 7 and is not accepted in
Aurora 0.1. Set iteration and rendering currently follow an insertion-oriented
representation, but Set order is not a promised API contract; algorithms MUST
NOT depend on it. Allocation success is limited by available host resources.
Map duplicate-key position, read ownership, simple-assignment ownership/order,
and missing-key behavior are language rules under ADR-0014. Copy-only Vec/Map
indexed compound assignment is also a language rule under ADR-0014. Neither is
implementation-defined permission for backend divergence.

## Status

Vec, Map, and Set typing, literals, constructors, equality, cloning, Vec/Set
iteration modes, negative Vec indexing, and the documented method surfaces are
implemented for the post-Phase 1.5 surface. The duplicate-key, direct-read,
simple-assignment, and compound-assignment Map rules are implemented under
`architecture_docs/decisions/0014-map-literals-and-indexing.md`, whose status is
**Accepted**. They are pinned by
`crates/aurora-compiler/tests/fixtures/run-pass/map_literal_duplicate_keys.au`,
`crates/aurora-compiler/tests/fixtures/check-fail/map_index_non_copy_requires_explicit_clone.au`,
`crates/aurora-compiler/tests/fixtures/check-fail/map_index_assignment_consumes_noncopy_key.au`,
`crates/aurora-compiler/tests/fixtures/check-fail/map_compound_assignment_noncopy_value_rejected.au`,
and
`crates/aurora-compiler/tests/fixtures/run-fail/map_index_missing_key.au`.
Mutable Set iteration, direct Map iteration, general iterable protocols,
and comprehensions are unavailable. Collection slicing is reserved for the
Phase 7 slice work.
