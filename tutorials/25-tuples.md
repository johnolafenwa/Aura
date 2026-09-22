# Tuples

A tuple bundles a fixed number of values, which may have different types. Use
one when a function has two or three natural results and a class would add
more ceremony than meaning.

## Values And Types

A comma inside parentheses makes a tuple:

```aura check-pass
pair = ("Aura", 7)
only = (true,)
```

`(value)` is still grouping, so a one-element tuple needs its comma:
`(value,)`. Aura has no empty tuple. A tuple with two or more elements does
not take a trailing comma.

Tuple types look like tuple values:

```aura check-pass
def version() -> (str, int64):
    return ("Aura", 7)
```

The order and number of element types matter. `(str, int64)` and
`(int64, str)` are different types.

## Unpacking A Return Value

Use a comma-separated target to name each result:

```aura fragment
name, number = version()
print(name)
print(number)
```

A tuple value expression needs parentheses, but a top-level assignment target
does not. Write `name, number = pair`. A naked tuple expression without
parentheses is not allowed. Nested targets use parentheses:

```aura check-pass
label, (x, y) = ("point", (3, 4))
```

The right side is evaluated once. Its full recursive shape must match the
target.

## Copy And Move Behavior

A tuple is a copy value only when every element is a copy value:

```aura check-pass
point = (3, 4)
x, y = point
print(point[0]) # point is still usable
```

A tuple that holds a `str`, a `list`, or another move value is itself a move
value. Unpacking it moves the whole source and gives you owned leaf bindings:

```aura check-pass
def main():
    record = ("Aura", 7)
    name, number = record
    print(name)
    print(number)
    # print(record) would be a use-after-move error
```

Aura reports any reuse of the original tuple. It does not expose partial moves
of single positions.

## Structural Equality

`==` and `!=` compare tuples element by element, recursively. Both operands
must have exactly the same tuple type, and every element must support
equality:

```aura check-pass
baseline = ("Aura", (7, true))
same = ("Aura", (7, true))
changed = ("Aura", (8, true))

assert baseline == same
assert baseline != changed
assert same != changed
```

Tuple equality reads both operands and keeps them. That holds for non-copy
tuples too. These tuples contain a `str`, yet each binding stays usable in a
later comparison.

Tuple ordering is a separate matter. The checker rejects `<`, `<=`, `>`, and
`>=` on tuple operands. Compare the elements you care about explicitly
instead.

## Constant Indexes

You can index a tuple for a small read-only case:

```aura check-pass
point = (3, 4)
print(point[1])
```

The index must be a non-negative integer literal, must be in bounds, and must
select a copy element. The checker rejects a variable index or a non-copy
element. To take ownership of a non-copy element, unpack the tuple.

## Unpacking In Loops

A `for` target can unpack tuple items recursively:

```aura check-pass
for label, count in [("ready", 2), ("done", 3)]:
    print(f"{label}:{count}")
```

What each leaf gets depends on the loop form:

| Loop form | Result |
| --- | --- |
| Bare collection iteration | Keeps the collection. Non-copy tuple leaves get shared access. |
| `own` collection iteration | Gives owned leaves. |
| Bare Queue iteration | Receives each tuple already owned. |
| `mut` iteration with a tuple target | Not supported. |

`mut` iteration is not supported because Aura does not rebuild a changed
tuple and write it back into the collection.

## Tuple Patterns

Tuple patterns use the same fixed shape:

```aura check-pass
match ((1, 2), true):
    case ((left, right), flag):
        print(left + right)
        print(flag)
```

`match own` consumes a non-copy tuple as one whole value. A bare `match` keeps
it and gives shared access to non-copy leaves. `match mut` with a tuple
pattern is not supported.

## What Tuples Are Not

Tuples are not small vectors. Apart from structural `==` and `!=`, tuples have
none of these:

- ordering
- iteration
- methods
- named elements
- rest or star unpacking
- slicing
- dynamic indexing
- implicit conversion to `list`

Run the maintained example:

```bash
cargo run -p aura -- run examples/basics/tuples.au
```

It prints:

```text
Aura
7
20
ready:2
done:3
3
true
```

The normative [Tuples Manual page](../docs/manual/tuples.md) has the complete
contract, including diagnostics and backend parity.
