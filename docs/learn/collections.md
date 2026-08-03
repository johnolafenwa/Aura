# Working With Collections

Aura uses `list[T]`, `dict[K, V]`, and `set[T]`. Each collection has one exact
static element shape, deterministic ownership, and explicit absence.

## Lists

A list preserves order and allows duplicates:

```aura
mut names: list[str] = ["Ada", "Grace"]
names.append("Katherine")
print(names)
```

An empty list needs an annotation or constructor:

```aura
mut names: list[str] = []
mut scores = list[int32]()
```

Positions use `int64`. Negative positions count from the end:

```aura
mut values = [10, 20, 30]
print(values[-1])

match values.get(-2):
    case Option.Some(value):
        print(value)
    case Option.None:
        print("missing")
```

Use `get` when an invalid position is ordinary input. It returns `Option[T]`
and requires clone-safe `T`. Direct indexing, `pop`, `set`, and `swap` trap on
invalid positions.

The core mutations have Python-shaped names and typed ownership:

```aura
mut values = [10, 20, 30]
values.insert(-1, 25)
values.append(40)
old = values.set(0, 5)
last = values.pop()
values.remove(20)
```

`insert` clamps its position to the range from zero through the current
length. `pop()` removes the final element and returns it. `remove(value)`
removes the first equal value. An absent value traps with `AU4008`; test
membership first when absence is expected.

`index(value)` returns the first equal position, and `count(value)` counts all
equal elements:

```aura
values = [3, 1, 3, 2]
print(values.index(3))
print(values.count(3))
```

List and string slices return fresh owned values:

```aura
values = [10, 20, 30, 40]
middle = values[1:3]
suffix = values[-2:]
copy = values[:]

text = "A🎉Z"
celebration = text[1:2]
```

Slice positions count elements for lists and Unicode scalar values for `str`.
Bounds are half-open. Invalid or reversed bounds trap with `AU4003`.

### Eager Algorithms

`map` and `filter` return fresh owned lists. Sorting is stable and mutates the
receiver:

```aura
def doubled(value: int32) -> int32:
    return value * 2

def is_even(value: int32) -> bool:
    return value % 2 == 0

def descending(value: int32) -> int32:
    return -value

def main():
    values = [3, 1, 2, 4]
    mapped = values.map(doubled)
    filtered = values.filter(is_even)

    mut ascending = values.copy()
    ascending.sort()

    mut reverse_order = values.copy()
    reverse_order.sort(key=descending)

    mut descending_natural = values.copy()
    descending_natural.sort(reverse=true)
```

A key function runs once per element before the list changes. Equal keys keep
their input order. `copy()` requires clone-safe elements and returns storage
independent from the source.

Use capacity control for workloads that know their size:

```aura
mut values = list[int32].with_capacity(1_000)
values.reserve(500)
```

Capacity calls do not change list contents. Negative requests trap with
`AU4003`; allocation failures trap with `AU4005`.

## Dictionaries

A dictionary preserves key insertion order:

```aura
mut counts: dict[str, int32] = {"ready": 2}
counts["done"] = 1
counts["ready"] = 3
```

Use `in` for membership and `get` for typed optional lookup:

```aura
if "ready" in counts:
    print(counts["ready"])

match counts.get("missing"):
    case Option.Some(value):
        print(value)
    case Option.None:
        print("not found")
```

`get` has no default argument. It returns a cloned value and therefore
requires clone-safe `V`. `remove(key)` transfers the value when present and
returns `None` when absent.

`keys()`, `values()`, and `items()` return eager owned lists in insertion
order. Items are tuples:

```aura
for key, value in counts.items():
    print(key + ": " + value.to_string())
```

`copy()` duplicates the dictionary into independent owned storage.
`update(other)` transfers entries from another
dictionary. An existing key keeps its insertion position; a new key is added
at the end.

## Sets

A set stores one value per equality class. A non-empty set literal needs a set
context, and an empty set uses its constructor:

```aura
mut seen: set[int32] = {1, 2, 2, 3}
mut names = set[str]()
```

Membership uses `in` and `not in`. Mutation uses `add`, `remove`, and
`discard`:

```aura
seen.add(5)

if 2 in seen:
    seen.remove(2)

seen.discard(99)
```

`remove` traps with `AU4008` when the value is absent. `discard` is silent.
Both return `None`. `copy`, `clear`, `reserve`, and `with_capacity` follow the
same ownership and capacity rules as the other collection types.

Sets render with braces when non-empty and as `set()` when empty. Program logic
must not depend on set iteration order.

## Comprehensions

Comprehensions eagerly create fresh owned collections:

```aura
values = [1, 2, 3, 4]
squares = [value * value for value in values]
even = {value for value in values if value % 2 == 0}
labels = {value: str(value) for value in values}
```

Nested clauses run in outer-major order and filters run from left to right.
Collection sources are shared and frozen during a comprehension. A non-Copy
value reached through shared iteration needs an explicit `.clone()` before it
can enter the new collection.

## Choosing A Collection

Use `list[T]` when order or duplicates matter. Use `dict[K, V]` for keyed
lookup and updates. Use `set[T]` for uniqueness and membership.

The normative method signatures, failure codes, evaluation order, and backend
contract are in [Collections](/manual/collections).
