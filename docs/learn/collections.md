# Working With Collections

A program that manipulates a handful of values usually needs one of three
shapes: an ordered sequence, a keyed lookup, or a membership test. Aurora ships
these as `Vec[T]`, `Map[K, V]`, and `Set[T]`. All three are owned move types. A
bare parameter or loop shares them; write `own` when ownership should transfer.

This chapter uses each of them to solve a small problem, then shows the patterns they share.

## `Vec[T]`: Ordered Data

A vector stores values in insertion order and indexes them with `int32`.
Its `len()` method returns an `int64` count:

```python
mut names = ["Ada", "Grace"]
names.push("Katherine")
count: int64 = names.len()
assert len(names) == count
print(count)
```

Index-safe access goes through `get`, which returns `Option[T]`:

```python
match names.get(1):
    case Some(name):
        print(name)
    case None:
        print("missing")
```

`get` creates a cloned owned element, so the element type must be clone-safe.
For a value containing `random.Rng`, use `remove` to transfer ownership instead.

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

The short practical rule: use `get` when absence is normal and the element is
clone-safe; use `remove` to transfer a non-cloneable element; use the other
mutating methods when an invalid index is a program bug you want to catch.

`insert` accepts normalized indexes from `0` through `len`: converting the
length at the `int32` index boundary makes an append explicit:

```python
values.insert(values.len() as int32, 40)
```

The cast is checked, so a length outside the `int32` range fails rather than
wrapping. Aurora deliberately does not copy Python's clamping behavior for an
extremely negative insertion index. If one normalization still leaves the
index below zero, Aurora reports the error instead of silently inserting at
the start.

The same boundary applies to length-driven `range(...)` loops:

```python
for index in range(values.len() as int32):
    print(values[index])
```

## Eager Vec Algorithms

Function values let a vector apply named callbacks without introducing a lazy
iterator type:

```python
def doubled(value: int32) -> int32:
    return value * 2

def is_even(value: int32) -> bool:
    return value % 2 == 0

values: Vec[int32] = [3, 1, 2, 4]
mapped = values.map(doubled)       # [6, 2, 4, 8]
filtered = values.filter(is_even)  # [2, 4]
```

Both methods read the vector through shared access and return a fresh owned
result, so `values` remains available. `filter` clones each accepted element;
its element type must therefore be clone-safe.

Sorting is stable and mutates the receiver:

```python
def descending(value: int32) -> int32:
    return -value

mut ascending: Vec[int32] = [3, 1, 2]
ascending.sort()

mut reverse_order: Vec[int32] = [3, 1, 2]
reverse_order.sort_by(descending)
```

`sort_by` calls its key function once for each element from left to right
before it reorders anything. If a key call fails at runtime, the vector remains
unchanged. The callback parameter must be bare/shared. A callback declared
with `mut` or `own` is intentionally a different contract.

See
[`examples/collections/vec_algorithms.au`](../../examples/collections/vec_algorithms.au)
for stable key ordering and the complete output.

## Lengths Are `int64`

All five maintained length members return `int64`:

- `String.len()` counts Unicode scalar values.
- `String.byte_len()` counts UTF-8 bytes.
- `Vec[T].len()`, `Map[K, V].len()`, and `Set[T].len()` count entries.

The free builtin delegates to the member, so `len(value) == value.len()` for
`String`, `Vec`, `Map`, and `Set`. Unicode text makes the distinction between
the two String counts visible:

```python
text = "A🎉"
scalar_count: int64 = text.len()       # 2
byte_count: int64 = text.byte_len()    # 5
assert len(text) == scalar_count
```

## Collection Iteration

`for value in names` uses the shared default, so the vector remains available:

```python
names = ["Ada", "Grace"]

for name in names:
    print(name)

print(names.len())
```

Write `own` to consume it and receive owned elements:

```python
names = ["Ada", "Grace"]

for name in own names:
    print(name)
# names is no longer available
```

`for name in names:` is the bare shared spelling.

When the loop body needs to mutate the elements, borrow mutably:

```python
mut values = [1, 2, 3]

for value in mut values:
    value += 10

for value in values:
    print(value)
```

`mut` iteration requires a mutable binding.

## Comprehensions: Transform And Filter

Use a comprehension when a transformation is easier to read as one eager
expression:

```python
scores = [1, 2, 3, 4]
squares = [score * score for score in scores]
even_squares = [score * score for score in scores if score % 2 == 0]
```

The result is a fresh owned `Vec`, not a lazy iterator or view. The source
remains available because every clause uses bare shared iteration. Set and map
results use braces:

```python
remainders = {score % 3 for score in scores}
labels = {score: f"score-{score}" for score in scores}
```

Multiple clauses are nested in outer-major order:

```python
pairs = [
    left * 10 + right
    for left in scores if left < 3
    for right in scores if right < 3
]
# [11, 12, 21, 22]
```

Read this as nested bare `for` loops. The first source is selected once,
filters run left to right, and each inner source is selected for every
surviving outer item. The output is written first but runs only after its
clauses and filters. In a map comprehension, the key runs before the value.

Comprehension targets exist only inside the expression. Output storage owns
its values, so a non-copy item borrowed from a Vec needs an explicit clone:

```python
names = ["Ada", "Grace"]
copied_names = [name.clone() for name in names]
```

There is no `mut` or `own` clause form. Queue clauses keep the ordinary Queue
exception: each receive item arrives owned even though the syntax is bare.
Parenthesized generator expressions remain unavailable; choose an eager
comprehension or an explicit loop when you need control over incremental work.

See
[`examples/collections/comprehensions.au`](../../examples/collections/comprehensions.au)
for list, set, map, filter, and nested-clause examples.

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
These projection methods clone both fields, so both types must be clone-safe.

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

Sets support bare shared iteration:

```python
for value in users:
    print(value)
```

`for value in mut set:` is not supported; modify sets with `insert` and `remove` instead.

## Ownership Details That Matter

Most of the friction people run into with Aurora collections is about keys and clones. Two common patterns will feel awkward the first time and obvious the second.

### Borrowing keys at lookup

`Map.get`, `Map.remove`, and `Map.contains_key` borrow their key. `Map.set`
owns the key and value because it stores them. A lookup followed by insertion
therefore needs no clone:

```python
def bump(counts: mut Map[String, int32], key: own String):
    match counts.get(key):
        case Some(count):
            counts.set(key, count + 1)
        case None:
            counts.set(key, 1)
```

The lookup only lends `key`; the later `set` transfers it into the map.

### Borrow to inspect, move to transfer

Helpers that need to read a collection use the bare shared form. Helpers that
take ownership use `own`. The compiler
enforces this boundary, so inspection and transfer remain visible.

## A Larger Example: Unique Count

Put the pieces together. The helper below walks a vector of strings and reports how many unique values appear:

```python
def unique_count(values: Vec[String]) -> int64:
    mut seen = Set[String]()

    for value in values:
        seen.insert(value.clone())

    return seen.len()
```

`values` is borrowed, so the caller still owns it after the call. `value` inside the loop is borrowed from the vector's elements. Because the set needs an owned `String`, the clone appears right where ownership changes hands.

## Another Example: Count Words

A small word counter updates a caller-owned map:

```python
def count_words(counts: mut Map[String, int32], line: String):
    words = line.split(" ")

    for word in own words:
        if word.len() == 0:
            continue

        match counts.get(word):
            case Some(count):
                counts.set(word, count + 1)
            case None:
                counts.set(word, 1)
```

`line.split(" ")` returns a `Vec[String]` of owned words. The `own` loop
consumes that temporary vector and gives each iteration an owned `word`.
Lookup borrows the word; `set` then stores it.

## Reference

See [Collections](/manual/collections) in the Manual for the exact signature and return contract of every method.

The next chapter is the centrepiece of the book: Aurora's ownership model explained through the programs that benefit from it.
