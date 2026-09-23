# Working With Collections

Aura has three collection types: `list[T]`, `dict[K, V]`, and `set[T]`. Each
one holds a single static element type. Ownership of the elements is
deterministic, and lookups report absence explicitly.

## Lists

A list keeps its elements in order and allows duplicates:

```aura
mut names: list[str] = ["Ada", "Grace"]
names.append("Katherine")
print(names)
```

An empty list needs a type annotation or a constructor:

```aura
mut names: list[str] = []
mut scores = list[int32]()
```

Positions are `int64`. A negative position counts from the end:

```aura
mut values = [10, 20, 30]
print(values[-1])

match values.get(-2):
    case Lookup.Found(value):
        print(value)
    case Lookup.Missing:
        print("missing")
```

Use `get` when an invalid position is ordinary input. It returns `Lookup[T]`:

- A valid position gives `Lookup.Found(value)`, where `value` is a clone of
  the element. For this reason `get` requires a clone-safe `T`.
- An invalid position gives `Lookup.Missing`.

This keeps an element that is itself `None` distinct from a missing one.
Direct indexing, `pop`, `set`, and `swap` trap on an invalid position.

When the element is large, or not clone-safe, match on `lookup` instead. The
`Found` arm gets a view of the element in place, so nothing is copied:

```aura
def main():
    mut teams = [["ada"], ["grace", "alan"]]
    match mut teams.lookup(1):
        case Lookup.Found(team):
            team.append("barbara")
        case Lookup.Missing:
            print("no team")
    print(teams[1].len())
```

`lookup` only works as the subject of a `match` statement. A bare `match`
gives a shared view and `match mut` gives a mutable one. The view ends with
its arm, and the list cannot change shape while it is live.

The core mutations have Python-shaped names and typed ownership:

```aura
def main():
    mut values = [10, 20, 30]
    values.insert(-1, 25)
    values.append(40)
    old = values.set(0, 5)
    last = values.pop()
    values.remove(20)
```

- `insert` clamps its position to the range from zero through the current
  length.
- `pop()` removes the last element and returns it.
- `remove(value)` removes the first equal value. If the value is absent, it
  traps with `AU4008`. Test membership first when absence is expected.

`index(value)` returns the position of the first equal element.
`count(value)` counts every equal element:

```aura
values = [3, 1, 3, 2]
print(values.index(3))
print(values.count(3))
```

Slicing a list or a string returns a fresh owned value:

```aura
values = [10, 20, 30, 40]
middle = values[1:3]
suffix = values[-2:]
copy = values[:]

text = "A🎉Z"
celebration = text[1:2]
```

A list slice counts elements. A `str` slice counts Unicode scalar values.
Bounds are half-open. Invalid or reversed bounds trap with `AU4003`.

### Eager Algorithms

`map` and `filter` return fresh owned lists. `sort` is stable and changes the
list it is called on:

```aura
def doubled(value: int32) -> int32:
    return value * 2

def is_even(value: int32) -> bool:
    return value % 2 == 0

def descending(value: int32) -> int32:
    return -value

def main():
    values: list[int32] = [3, 1, 2, 4]
    mapped = values.map(doubled)
    filtered = values.filter(is_even)

    mut ascending = values.copy()
    ascending.sort()

    mut reverse_order = values.copy()
    reverse_order.sort(key=descending)

    mut descending_natural = values.copy()
    descending_natural.sort(reverse=true)
```

A key function runs once per element before the list changes. Elements with
equal keys keep their input order. `copy()` requires clone-safe elements and
returns storage that is independent of the source.

When a workload knows its size, control the capacity:

```aura
mut values = list[int32].with_capacity(1_000)
values.reserve(500)
```

Capacity calls do not change the list's contents. A negative request traps
with `AU4003`. An allocation failure traps with `AU4005`.

## Dictionaries

A dictionary keeps its keys in insertion order:

```aura
mut counts: dict[str, int32] = {"ready": 2}
counts["done"] = 1
counts["ready"] = 3
```

Use `in` to test membership. Use `get` for a lookup that reports absence:

```aura fragment
if "ready" in counts:
    print(counts["ready"])

match counts.get("missing"):
    case Lookup.Found(value):
        print(value)
    case Lookup.Missing:
        print("not found")
```

`get` takes no default argument. It returns `Lookup.Found(value)` with a
cloned value, or `Lookup.Missing` when the key is absent. For this reason it
requires a clone-safe `V`.

`remove(key)` moves the value out into `Lookup.Found(value)` when the key is
present. It returns `Lookup.Missing` when the key is absent.

`lookup(key)` views the value in place instead of cloning it. Like list
`lookup`, it is only valid as a `match` subject:

```aura fragment
match mut groups.lookup("admins"):
    case Lookup.Found(members):
        members.append("ada")
    case Lookup.Missing:
        groups["admins"] = ["ada"]
```

`keys()`, `values()`, and `items()` return eager owned lists in insertion
order. Each item is a tuple:

```aura fragment
for key, value in counts.items():
    print(key + ": " + value.to_string())
```

`copy()` duplicates the dictionary into independent owned storage.
`update(other)` moves the entries of another dictionary into this one. An
existing key keeps its position. A new key goes at the end.

## Sets

A set stores one value per equality class. A non-empty set literal needs a
set context, such as a `set[...]` annotation. An empty set uses its
constructor:

```aura
mut seen: set[int32] = {1, 2, 2, 3}
mut names = set[str]()
```

Test membership with `in` and `not in`. Change the set with `add`, `remove`,
and `discard`:

```aura fragment
seen.add(5)

if 2 in seen:
    seen.remove(2)

seen.discard(99)
```

`remove` traps with `AU4008` when the value is absent. `discard` does nothing
when the value is absent. Both return `None`.

`copy`, `clear`, `reserve`, and `with_capacity` follow the same ownership and
capacity rules as the other collection types.

A non-empty set prints with braces. An empty set prints as `set()`. Program
logic must not depend on the order in which a set iterates.

## Comprehensions

A comprehension builds a fresh owned collection right away:

```aura
values = [1, 2, 3, 4]
squares = [value * value for value in values]
even = {value for value in values if value % 2 == 0}
labels = {value: str(value) for value in values}
```

- Nested `for` clauses run in outer-major order.
- Filters run from left to right.
- The source collections are shared and frozen while the comprehension runs.
- A value that is not a copy type, reached through shared iteration, needs an
  explicit `.clone()` before it can go into the new collection.

## Choosing A Collection

| Need | Use |
| --- | --- |
| Order or duplicates matter | `list[T]` |
| Keyed lookup and updates | `dict[K, V]` |
| Uniqueness and membership | `set[T]` |

[Collections](/manual/collections) in the Manual gives the exact method
signatures, failure codes, evaluation order, and backend contract.
