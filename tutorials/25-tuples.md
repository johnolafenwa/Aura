# Tuples

Tuples bundle a fixed number of values that may have different types. They are
useful when a function has two or three natural results but defining a class
would add more ceremony than meaning.

## Values And Types

A comma inside parentheses makes a tuple:

```aurora
pair = ("Aurora", 7)
only = (true,)
```

`(value)` still means grouping. The comma in `(value,)` is therefore required
for a singleton. Aurora has no empty tuple, and a tuple with two or more
elements does not take a trailing comma.

Tuple types mirror tuple values:

```aurora
def version() -> (String, int64):
    return ("Aurora", 7)
```

The order and number of element types matter. `(String, int64)` and
`(int64, String)` are different types.

## Unpacking A Return Value

Use a comma-separated target to give each result a name:

```aurora
name, number = version()
print(name)
print(number)
```

Tuple value expressions require parentheses, but the top-level assignment
target does not: write `name, number = pair`, not a naked tuple expression.
Nested targets use parentheses:

```aurora
label, (x, y) = ("point", (3, 4))
```

The right side is evaluated once, and its complete recursive shape must match
the target.

## Copy And Move Behavior

A tuple is a copy value only when every element is a copy value:

```aurora
point = (3, 4)
x, y = point
print(point[0]) # point is still usable
```

A tuple containing `String`, `Vec`, or another move value is itself a move
value. Unpacking it moves the whole source and gives owned leaf bindings:

```aurora
record = ("Aurora", 7)
name, number = record
print(name)
# print(record) would be a use-after-move error
```

Aurora deliberately reports reuse of the original tuple instead of exposing
independent positional partial moves.

## Constant Indexes

Indexing is available for the small read-only case:

```aurora
point = (3, 4)
print(point[1])
```

The index must be a non-negative integer literal, must be in bounds, and must
select a copy element. A variable index or a non-copy element is rejected.
Unpack the tuple when you need ownership of a non-copy element.

## Unpacking In Loops

A `for` target may recursively unpack tuple items:

```aurora
for label, count in [("ready", 2), ("done", 3)]:
    print(f"{label}:{count}")
```

Bare collection iteration keeps the collection and gives non-copy tuple leaves
shared access. `own` collection iteration gives owned leaves. Bare Queue
iteration receives each tuple already owned. `borrow mut` iteration with a
tuple target is not supported because the minimal tuple surface does not
reconstruct and write a changed tuple back into the collection.

## Tuple Patterns

Tuple patterns use the same fixed shape:

```aurora
match ((1, 2), true):
    case ((left, right), flag):
        print(left + right)
        print(flag)
```

A normal by-value match consumes a non-copy tuple as one whole value.
`match borrow` keeps it and gives shared access to non-copy leaves.
`match borrow mut` with a tuple pattern is not supported.

## What Tuples Are Not

Tuples are not small vectors. The initial surface has no tuple iteration,
methods, equality, ordering, named elements, rest/star unpacking, slicing,
dynamic indexing, or implicit conversion to `Vec`.

Run the maintained example:

```bash
cargo run -p aura -- run examples/basics/tuples.au
```

It prints:

```text
Aurora
7
20
ready:2
done:3
3
true
```

For the complete contract, including diagnostics and backend parity, see the
normative [Tuples Manual page](../docs/manual/tuples.md).
