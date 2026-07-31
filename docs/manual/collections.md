# Collections

Aurora provides three generic owned collection types:

- `Vec[T]` for ordered sequences
- `Map[K, V]` for key/value lookup
- `Set[T]` for uniqueness and membership tests

Collections are move types. Assigning one to another binding transfers
ownership. A bare parameter or loop grants shared access; use `own` to transfer
ownership deliberately and `mut` to mutate caller-owned storage.

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

Vectors support bare shared, consuming `own`, and mutable `mut` iteration:

```python
for value in values:
    print(value)

for value in own values:
    consume(value)

for value in mut values:
    value += 1
```

Sets support bare shared and `own` iteration. Maps expose
iteration through returned collection values; prefer `items()` or `entries()`
when you want key/value pairs:

```python
for entry in counts.items():
    print(entry.key + "=" + entry.value.to_string())
```

`for value in mut set:` is not supported. Mutate sets with `insert` and `remove`.

Bare Vec/Set iteration freezes the selected collection for
the loop. `own` iteration instead moves the collection once into a loop-private
source. Reinitializing the consumed source binding in the body cannot switch or
truncate the active iteration. Accepted ADR-0017 records this one-time
source-selection rule without changing ADR-0006's loop ownership modes.

## Comprehensions

Comprehensions eagerly build fresh owned collections:

    squares = [value * value for value in values]
    even = {value for value in values if value % 2 == 0}
    labels = {value: str(value) for value in values}

List comprehensions produce `Vec[T]`, set comprehensions produce `Set[T]`, and
map comprehensions produce `Map[K, V]`. At least one `for` clause is required.
Filters and later clauses may follow:

    pairs = [
        left * 10 + right
        for left in values if left < 3
        for right in values if right < 3
    ]

Evaluation is outer-major. The first iterable is evaluated once. For each
outer item, filters run left to right; each later iterable is evaluated once
for every surviving combination of earlier targets. At the innermost
surviving combination, the element expression runs and its result moves or
copies into the result. Although the output expression is written first, it
does not run until all clauses and filters that guard it have succeeded. A map
evaluates and captures its key before its value. Equal set elements deduplicate
and later equal map keys replace values under the ordinary collection rules.

Every comprehension clause uses the same bare iteration contract as a
statement loop. Vec and Set sources are shared and frozen, Range produces copy
`int32` values, and `enumerate(...)` and `zip(...)` keep their compiler-known
bare-loop behavior. Queue keeps its receive carve-out: the handle is copied
for the active clause and each received item arrives owned. There is no
`mut` or `own` comprehension-clause spelling.

Result storage is owned. A copy element copies into it; an owned non-Copy value
moves. A non-Copy element reached through a shared Vec or Set source must use
an explicit `.clone()` when clone-safe:

    names_copy = [name.clone() for name in names]

Aurora never inserts a hidden clone. A Queue-received item is already owned and
may move directly into the result. Comprehension targets are local to the
expression and do not leak afterwards. See
[Expressions](/manual/expressions#comprehensions) for clause order and
[Closures](/manual/closures) for lambdas evaluated inside clauses.

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
| `sort` | `sort() -> None` | Stably sorts an orderable vector in place through a mutable receiver. Built-in order is available for every integer type, `float32`, `float64`, and `Duration`; `String` has no built-in `Ord[String]` in Aurora 0.2. |
| `sort_by` | `sort_by[K](key: def(T) -> K) -> None` | Evaluates `key` once per element from left to right, then stably sorts in place by the orderable produced keys. |
| `map` | `map[U](f: def(T) -> U) -> Vec[U]` | Calls `f` once per element in order and returns a fresh owned vector; the source is retained. |
| `filter` | `filter(f: def(T) -> bool) -> Vec[T]` | Calls `f` once per element in order and returns a fresh owned vector of accepted cloned elements; requires clone-safe `T`. |

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

### Owned Vec And String Slices

One-colon bracket forms select a half-open contiguous range and return a fresh
owned value:

    middle = values[1:3]
    prefix = values[:2]
    suffix = values[-2:]
    all_values = values[:]
    celebration = "A🎉Z"[1:2]

The same four forms work for `Vec[T]` and `String`. An omitted start means zero
and an omitted end means the source length. A written endpoint has exactly
type `int32`; integer literals adopt that context, while an `int64` binding
must be narrowed explicitly with a checked cast.

Negative endpoints normalize once as `len + endpoint`. Each effective
endpoint must then lie in `0..=len`, and start must not exceed end. Equal
endpoints return an empty value. Any out-of-range or reversed range traps with
`AU4003`.

This is deliberately **not Python's clamping behavior**. Aurora never changes
an invalid slice endpoint to the nearest boundary. For example,
`values[-999:2]` and `values[3:1]` fail loudly instead of silently selecting a
different or empty range.

A Vec slice copies Copy elements and clones clone-safe non-Copy elements into
the new Vec. This establishes the same specialization-time clone-safety
obligation as other clone-producing operations. A value containing
`random.Rng` is rejected with `AU3007`; a non-repeatable Task observation
right is rejected with `AU3009`. The source remains usable and independent
from the returned vector.

A String slice counts Unicode scalar values, matching `String.len()`, rather
than UTF-8 bytes or grapheme clusters. It scans the source to find scalar
boundaries, so slicing is O(n), then returns a fresh valid UTF-8 String.
Integer `text[index]` remains unavailable.

The base, written start, and written end run exactly once from left to right,
with the source retained through the endpoint expressions. A slice is not an
assignable place or an ADR-0038 view. Step forms report `AU2005` with `slice
steps are unavailable; use an explicit loop to select a stride`. Slice
assignment reports `AU2005` with `slice assignment is unavailable because
slices are owned copies; mutate the source by index or build a new value`.

### Vec Algorithms And Callbacks

`sort` and `sort_by` are stable, in-place mutations. Equal elements or equal
keys retain their relative source order. `sort` uses the element's natural
ordering. `sort_by` first evaluates the shared `key` callback exactly once for
each element, from the first element to the last, and records every key before
reordering the vector. If a key call traps, the receiver has not been mutated.

Orderable values use Aurora's existing `<` relation: integers, floating-point
values under their ordinary partial-order behavior, `Duration`, and user types
with an applicable `Ord` implementation. `sort_by` requires the produced `K`,
not necessarily the stored `T`, to be orderable.

Concretely, Aurora 0.2 provides built-in ordering for every signed and unsigned
integer type (including the `int` alias), `float32`, `float64`, and `Duration`.
It does not provide a built-in `Ord[String]`, so `Vec[String].sort()` is
rejected. Keep insertion order when ordering is unnecessary; use `sort_by` to
sort Strings by an orderable application key such as length or a separate
numeric index; or define a nominal application type with an `Ord` implementation
that compares the explicit rank/key your domain requires. Stable sorting keeps
the prior relative order when two keys compare equally.

`map` and `filter` are eager. They visit the source from left to right and
return a fresh owned vector rather than a lazy iterator. Their shared receiver
leaves the source available after the call. `map` owns each callback result in
the returned `Vec[U]`. `filter` must clone each accepted source element, so its
`T` must be clone-safe. A filter over `Vec[random.Rng]`, including through a
wrapper, is rejected with `AU3007`.

Every Vec algorithm callback must be repeatable. Named function values,
capture-free lambdas, and repeatable value-capturing closures are accepted;
a consuming closure is rejected with `AU2002` because the algorithm may call
it once per visited element. Every callback position has the exact shared
capability shown by `def(T)`. A `def(mut T) -> ...` or
`def(own T) -> ...` function is not substituted for it: algorithms neither
grant element mutation nor consume source elements.

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
owned replacement. `map` owns the callback-produced result values. `filter`
clones accepted source elements into its fresh result and therefore has the
same clone-safety boundary as other clone-producing collection operations.

This executable example demonstrates eager callback and comprehension
transformation, filtering, natural ordering, keyed ordering, source retention,
set deduplication, and nested outer-major order:

```aurora
def doubled(value: int32) -> int32:
    return value * 2

def is_even(value: int32) -> bool:
    return value % 2 == 0

def descending_key(value: int32) -> int32:
    return -value

def main():
    values: Vec[int32] = [3, 1, 2, 4]
    middle = values[1:3]
    prefix = values[:2]
    suffix = values[-2:]
    all_values = values[:]
    celebration = "A🎉Z"[1:2]
    mapped = values.map(doubled)
    filtered = values.filter(is_even)

    mut ascending = values.clone()
    ascending.sort()

    mut descending = values.clone()
    descending.sort_by(descending_key)

    squares = [value * value for value in values]
    even_squares = [value * value for value in values if value % 2 == 0]
    remainders = {value % 3 for value in values}
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
    print(prefix)
    print(suffix)
    print(all_values)
    print(celebration)
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

## Choosing The Right Collection

Use `Vec[T]` when order or duplicates matter.

Use `Map[K, V]` when you need to find or update a value by key.

Use `Set[T]` when the question is "have I seen this before?"

## Grammar

The normative productions for list, map, set, and empty collection literals;
list/set/map comprehensions; generic collection types; indexing; one-colon
owned slicing; indexed assignment; method calls; and loop ownership modifiers
are in [Grammar](/manual/grammar). The first colon in
a nonempty brace literal selects map syntax. `{}` is grammatically a map but
may type as an empty `Set[T]` under an expected set type. `Set{...}` is the
explicit set-literal form, and `Set[T]()` is the typed constructor form.

## Typing Rules

Every collection is homogeneous under one exact invariant specialization.
Literal elements, keys, and values must agree after contextual literal typing,
and empty literals require an expected collection type. Vec indexes and
written Vec/String slice endpoints have exact type `int32`; Map indexes and
lookup keys have exact type `K`. Mutating and
retaining APIs use the owned positions shown in the tables above, and every
mutating method or indexed assignment requires a mutable collection place.
Bare Vec/Set iteration borrows the collection, `own` consumes it, and `mut` is
supported only for a mutable Vec place. Comprehension clauses admit only the
bare iteration form. A
direct Map read requires copy `V`; a cloned non-copy read uses `get` only when
`V` is clone-safe, while `remove` transfers any `V`. Simple
indexed assignment accepts any Vec element or Map value type and owns the
assigned value; a Map also owns its key. Compound indexed assignment requires
copy `T` for Vec and copy `V` for Map.

Clone-producing APIs and Vec slicing infer obligations when their relevant
element, key, or value types are unresolved. The obligation is checked after
specialization and propagates through generic callers; a produced type
containing `random.Rng` is rejected with `AU3007`. `filter` and Vec slicing
establish that obligation for `T`; slicing a non-repeatable Task observation
right is rejected with `AU3009`.

`sort` requires a mutable `Vec[T]` place and an orderable `T`. The built-in
orderable element types are every integer type, `float32`, `float64`, and
`Duration`; `String` has no built-in `Ord[String]` in Aurora 0.2. `sort_by`
requires a mutable `Vec[T]` place, exact callback type `def(T) -> K`, and an
orderable result type `K`. `map` requires exact callback type
`def(T) -> U`; `filter` requires exact callback type `def(T) -> bool`.
All four callback positions require a repeatable callable; consuming closures
are rejected with `AU2002`.
The bare callback parameters are logical shared capabilities even when `T` is
copy. A callback with a `mut` or `own` parameter is a different function type
and is rejected with `AU2002`.

A list/set comprehension element expression must have exactly the output
element type. A map comprehension key and value must have exactly `K` and `V`.
An expected collection type contextually types these expressions; without one,
their inferred types select the invariant result specialization. Every filter
has exactly type `bool`. Each clause source must satisfy the same iterable rule
as a statement bare loop. Targets are progressively scoped, cannot shadow
visible names or earlier targets, and do not enter the enclosing scope.

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

Vec and String slices normalize each negative endpoint once, require both
effective endpoints in `0..=len`, reject a reversed range, and copy the
half-open range into a fresh owned result. They never clamp. String positions
count Unicode scalars and require an O(n) scan of the source.

Vec algorithms visit elements from left to right. `map` and `filter` append
their eager results in visit order. Natural and keyed sorts are stable.
`sort_by` computes and stores all keys before its first receiver mutation; a
trap during key computation propagates unchanged and leaves the receiver
unchanged.

A comprehension creates one fresh output collection and executes its clauses
as nested loops. It is eager rather than resumable or lazy. Nested clauses are
outer-major, filters run left to right and short-circuit the current
combination, and map keys evaluate before values. A trap or `try` propagation
cleans up the partial result.

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
order. Algorithm callbacks receive shared element access. `map` moves each
callback result into the returned vector; `filter` retains the source and
clones each accepted element into the result.

Slicing retains its non-Copy base through written endpoint evaluation. The
base, start, and end each run at most once from left to right. Vec slicing
copies or clones elements under the inferred clone-safety and task-result
repeatability obligations; String slicing copies scalar-aligned UTF-8 bytes.
The result owns independent storage, does not consume the source, and creates
no view or assignable place.

Comprehension iterable, filter, key, value, and element expressions retain
ADR-0016 sequencing. Every reached inner source is selected once for that
surviving outer combination. Active shared collection sources stay frozen
through downstream filters, clauses, and output insertion. Output insertion is
an owned position and follows ordinary Copy, move, explicit-clone, and
loop-carried-move rules.

## Diagnostics

`AU1101` reports malformed literal, index, method-call, loop, or comprehension
syntax. A `mut`/`own` clause reports:

    comprehensions use bare iteration; remove `mut` or `own` and write `for name in values`

`AU2005` rejects a generator expression, including a parenthesized form or a
generator-shaped call argument, with:

    generator expressions are unavailable; use an eager owned list comprehension or an explicit loop

The same code reserves unsupported slice evolution with exact guidance:

    slice steps are unavailable; use an explicit loop to select a stride

    slice assignment is unavailable because slices are owned copies; mutate the source by index or build a new value

`AU2001` reports unknown collection types and members. `AU2002` covers literal
element/key/value mismatch, missing empty-literal context, generic arity,
index/key type mismatch, method argument type mismatch, and a collection
callback whose parameter capability or return type does not match the exact
shared callback contract, or an unavailable natural or key ordering. `AU2004`
reports invalid constructor or method argument binding. Other `AU2005` cases
supply focused Python-migration guidance for `len(...)`, `.append(...)`, and
`in`. `AU2999` covers unsupported collection methods, non-indexable
values, and remaining static collection rejections. `AU3001` reports use after moving a
stored value, indexed-assignment key, or consuming collection. `AU3002`
reports mutation/move while a collection or element is borrowed and invalid
mutable iteration. `AU3003` reports mutation through an immutable collection
place. `AU3005` rejects a non-copy direct Vec/Map indexed read, and `AU3006`
rejects a non-copy Vec/Map indexed compound assignment. `AU3007` rejects a
clone-producing collection operation, including `filter` or Vec slicing,
whose result contains or may contain non-cloneable `random.Rng` state.
`AU3009` rejects Vec slicing that would duplicate a non-repeatable Task result
right. `AU4003` (`bounds or lookup violation`) reports an out-of-range Vec
index, an invalid or reversed Vec/String slice, or a missing direct Map key.
Optional absence from `get` and Boolean absence from Set/Map membership or
removal are typed values, not diagnostics.

## Backend Support

Vec, Map, and Set literals, comprehensions, owned Vec/String slices, equality,
cloning, iteration, indexing, mutation, eager callback algorithms, stable
sorting, and the complete method tables
above are implemented for MIR execution and direct native generation. Both
backends use the same static collection types
and are parity-tested for duplicate-key replacement, key-before-value effects,
comprehension order and ownership, slice order/bounds/Unicode behavior,
indexed reads and writes, missing-key traps, and maintained observable
behavior. Compiler analysis and the LSP consume
those same types, binding scopes, and builtin signatures.

## Limits And Implementation-Defined Behavior

Mutable Set iteration and direct Map iteration are unavailable; iterate the
owned Vec snapshots returned by Map methods. Arbitrary user-defined iterables,
generator expressions, comprehension source modifiers, and trailing commas
are unavailable; collection literals and comprehensions may span physical
lines through their own delimiters. A Queue comprehension is eager and
continues receiving until the ordinary Queue-iteration termination condition;
use an explicit loop for early exit or bounded streaming. Slices support one
contiguous half-open range only. Steps, slice assignment, views, arbitrary
sliceable types, String integer indexing, and clamping are unavailable. Set
iteration and rendering currently follow an insertion-oriented
representation, but Set order is not a promised API contract; algorithms MUST
NOT depend on it. Allocation success is limited by available host resources.
Map duplicate-key position, read ownership, simple-assignment ownership/order,
and missing-key behavior are language rules under ADR-0014. Copy-only Vec/Map
indexed compound assignment is also a language rule under ADR-0014. Neither is
implementation-defined permission for backend divergence.

## Status

Vec, Map, and Set typing, literals, eager owned comprehensions, owned Vec and
String slices, constructors, equality, cloning, Vec/Set iteration modes,
negative Vec indexing, and the
documented method surfaces,
including callable-powered Vec algorithms, are implemented for the post-Phase
1.5 surface. The duplicate-key, direct-read,
simple-assignment, and compound-assignment Map rules are implemented under
`architecture_docs/decisions/0014-map-literals-and-indexing.md`, whose status is
**Accepted**. They are pinned by
`crates/aurora-compiler/tests/fixtures/run-pass/map_literal_duplicate_keys.au`,
`crates/aurora-compiler/tests/fixtures/check-fail/map_index_non_copy_requires_explicit_clone.au`,
`crates/aurora-compiler/tests/fixtures/check-fail/map_index_assignment_consumes_noncopy_key.au`,
`crates/aurora-compiler/tests/fixtures/check-fail/map_compound_assignment_noncopy_value_rejected.au`,
and
`crates/aurora-compiler/tests/fixtures/run-fail/map_index_missing_key.au`.
Comprehensions are Accepted under ADR-0039 and pinned by the focused
comprehension fixture family and `examples/collections/comprehensions.au`.
Owned Vec and Unicode-scalar String slices are Accepted under ADR-0040 and
pinned by the focused slice fixture family and
`examples/collections/slices.au`.
Mutable Set iteration, direct Map iteration, general iterable protocols, and
generator expressions remain unavailable.
