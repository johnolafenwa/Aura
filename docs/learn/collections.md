# Working With Collections

A program that manipulates a handful of values usually needs one of three shapes: an ordered sequence, a keyed lookup, or a membership test. Aurora ships these as `Vec[T]`, `Map[K, V]`, and `Set[T]`. All three are owned move types: passing one by value transfers ownership, and helpers that only need to inspect a collection take it by `borrow`.

This chapter uses each of them to solve a small problem, then shows the patterns they share.

## `Vec[T]`: Ordered Data

A vector stores values in insertion order and indexes them with `int32`.

```python
mut names = ["Ada", "Grace"]
names.push("Katherine")
print(names.len())
```

Index-safe access goes through `get`, which returns `Option[T]`:

```python
match names.get(1):
    case Some(name):
        print(name)
    case None:
        print("missing")
```

Negative indexes count from the end across the whole Vec surface:

```python
scores = [10, 20, 30]
print(scores[-1])      # 30; direct reads work for copy elements
match names.get(-2):
    case Some(name):
        print(name)    # Grace
    case None:
        print("missing")
```

The runtime normalizes a negative `index` once as `len + index`. Direct reads
and writes, `get`, `set`, `remove`, both `swap` indexes, and `insert` all use
that rule. `get` returns `None` when the normalized index is still invalid;
the other operations report an out-of-bounds runtime error.

Empty vectors need a type, either through an annotation or by calling the constructor:

```python
mut names: Vec[String] = []
mut scores = Vec[int32]()
```

Mutation uses explicit methods. The safe ones return `Option`:

```python
mut values = [10, 20, 30]

match values.pop():
    case Some(value):
        print(value)
    case None:
        print("empty")
```

`set`, `remove`, `insert`, and `swap` treat an out-of-range index as a bug. They raise a runtime error with file, line, and caret rather than returning a sentinel:

```python
values.set(0, 5)        # ok: replaces index 0
values.insert(1, 15)    # ok: inserts before index 1
values.insert(-1, 25)   # ok: inserts before the final element
values.swap(0, 2)       # ok: swaps two indices
# values.swap(0, 99)    # runtime error: index out of bounds
```

The short practical rule: use `get` when absence is normal; use the mutating methods when an invalid index is a program bug you want to catch.

`insert` accepts normalized indexes from `0` through `len`: `insert(len,
value)` appends. Aurora deliberately does not copy Python's clamping behavior
for an extremely negative insertion index. If one normalization still leaves
the index below zero, Aurora reports the error instead of silently inserting
at the start.

## Borrowed Iteration

`for value in names` iterates by value. For a move-type vector, that **consumes** the vector:

```python
names = ["Ada", "Grace"]

for name in names:
    print(name)

# names is no longer available
```

Use `borrow` when the collection should remain available after the loop:

```python
names = ["Ada", "Grace"]

for name in borrow names:
    print(name)

print(names.len())
```

When the loop body needs to mutate the elements, borrow mutably:

```python
mut values = [1, 2, 3]

for value in borrow mut values:
    value += 10

for value in borrow values:
    print(value)
```

`borrow mut` iteration requires a mutable binding.

## `Map[K, V]`: Lookup Tables

A map associates keys of type `K` with values of type `V`:

```python
mut counts = {"queued": 2, "done": 1}
counts.set("failed", 0)

match counts.get("queued"):
    case Some(count):
        print(count)
    case None:
        print("no queue")
```

Empty maps need a type:

```python
counts: Map[String, int32] = {}
```

`Map.set` returns the previous value when there was one, so a program can distinguish insertion from replacement:

```python
match counts.set("queued", 3):
    case Some(old):
        print(f"replaced {old}")
    case None:
        print("inserted")
```

Iterate keys and values together with `items()` or `entries()`:

```python
for entry in counts.entries():
    print(entry.key + "=" + entry.value.to_string())
```

`MapEntry[K, V]` is a small owned record with `.key` and `.value` fields.

## `Set[T]`: Membership

A set stores unique values. Because `{...}` is also a map literal, give the compiler an expected set type when you write a set literal:

```python
mut seen: Set[int32] = {1, 2, 2, 3}
seen.insert(5)
print(seen.contains(2))
```

Duplicates in the literal collapse to one entry; the set above has four values.

Empty sets also need an expected type:

```python
seen: Set[int32] = {}
```

`insert` and `remove` return `bool`, which is useful when the program cares whether anything changed:

```python
mut users = Set[String]()

if users.insert("ada"):
    print("first time")

if not users.insert("ada"):
    print("already present")
```

Sets support read-only iteration through `borrow`:

```python
for value in borrow users:
    print(value)
```

`for value in borrow mut set:` is not supported; modify sets with `insert` and `remove` instead.

## Ownership Details That Matter

Most of the friction people run into with Aurora collections is about keys and clones. Two common patterns will feel awkward the first time and obvious the second.

### Cloning keys at lookup

`Map.get`, `Map.set`, `Map.remove`, and `Map.contains_key` all take their key by value. If a program needs the same key for two calls in a row, it clones before the first and moves into the second:

```python
def bump(counts: borrow mut Map[String, int32], key: String):
    match counts.get(key.clone()):
        case Some(count):
            counts.set(key, count + 1)
        case None:
            counts.set(key, 1)
```

The clone is deliberate. You can see the program keeping two copies of the string, which is almost always what you want to be able to see.

### Borrow to inspect, move to transfer

Helpers that need to read a collection take it by `borrow`. Helpers that take ownership — transferring elements out, or consuming the whole thing — use by-value. The compiler enforces this at the boundary, so a program ends up with clear divisions between "inspection" and "transfer."

## A Larger Example: Unique Count

Put the pieces together. The helper below walks a vector of strings and reports how many unique values appear:

```python
def unique_count(values: borrow Vec[String]) -> int32:
    mut seen = Set[String]()

    for value in borrow values:
        seen.insert(value.clone())

    return seen.len()
```

`values` is borrowed, so the caller still owns it after the call. `value` inside the loop is borrowed from the vector's elements. Because the set needs an owned `String`, the clone appears right where ownership changes hands.

## Another Example: Count Words

A small word counter updates a caller-owned map:

```python
def count_words(counts: borrow mut Map[String, int32], line: borrow String):
    words = line.split(" ")

    for word in words:
        if word.len() == 0:
            continue

        match counts.get(word.clone()):
            case Some(count):
                counts.set(word, count + 1)
            case None:
                counts.set(word, 1)
```

`line.split(" ")` returns a `Vec[String]` of owned words. Each `word` is therefore owned. The `clone` on the key exists for the same reason as in `bump`: the lookup consumes the key, so the subsequent `set` needs the original copy.

## Reference

See [Collections](/manual/collections) in the Manual for the exact signature and return contract of every method.

The next chapter is the centrepiece of the book: Aurora's ownership model explained through the programs that benefit from it.
