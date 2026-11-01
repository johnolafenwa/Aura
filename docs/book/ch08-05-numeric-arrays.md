# Numeric arrays

Our gradebook is really a table: one row per student, one column per
assignment, a number in every cell. We've been storing it as a dictionary of
lists, which is flexible, and that flexibility costs something. Each list is
its own allocation, each row can have a different length, and every
calculation is a loop we write by hand. When the data is a rectangular table
of numbers and the work is arithmetic, Aura has a type built for it:
`Array[T]`.

If you've used NumPy, `Array` will feel like a small, strict version of it.
It holds numbers of one type in a single contiguous block of memory, laid out
row by row, with a *shape* that says how many rows and columns (or more
dimensions) there are. You can add two arrays, multiply one by a number, or
take the mean of all of them without writing a loop, and those operations run
as tight loops over the block.

## Making an array

*Filename: src/main.au*

```aura
def main():
    scores = Array[float64].from_list(
        [72.0, 88.0, 95.0,
         95.0, 91.0, 84.0,
         64.0, 58.0, 77.0],
        [3, 3],
    )
    print(scores)
    print(scores.shape())
    print(scores.len())
    print(scores[1, 2])
    print(scores[-1, 0])
    print(scores.get([0, 1]))
    print(scores.get([5, 1]))
```

<span class="caption">Listing 8-19: Building a 3-by-3 array and reading from it</span>

```console
$ aura run src/main.au
Array[float64](shape=[3, 3], values=[72.0, 88.0, 95.0, 95.0, 91.0, 84.0, 64.0, 58.0, 77.0])
[3, 3]
9
84.0
64.0
88.0
None
```

`Array[float64].from_list(values, shape)` copies the numbers from a list and
arranges them into the shape you give, filling the first row, then the
second, and so on. The line breaks in the list are only for us; to Aura it's
nine numbers. `shape()` gives the shape back as a `list[int64]`, and `len()`
is the total number of elements, here 3 times 3.

The type in brackets is the array's *dtype*, its element type, and there are
exactly four choices: `int32`, `int64`, `float32`, and `float64`. Anything
else is refused:

```aura check-fail:AU2002
def main():
    a = Array[str].zeros([2])
```

```console
$ aura check src/main.au
error[AU2002]: Array dtype must be one of `int32`, `int64`, `float32`, or `float64`, found `str`
 --> src/main.au:2:9
  |
2 |     a = Array[str].zeros([2])
  |         ^
```

Two other constructors make arrays without a list:
`Array[int64].zeros([2, 3])` fills a shape with zeros, and
`Array[float64].full([2, 2], 0.5)` fills it with a value you choose.

The shape is a runtime value, not part of the type. A 3-by-3 array and a
1-by-100 array are both `Array[float64]`, so the compiler can't check that a
shape matches its data. That check happens when the array is built:

```aura
def main():
    a = Array[float64].from_list([1.0, 2.0, 3.0], [2, 2])
```

```console
$ aura run src/main.au
error[AU4007]: array shape [2, 2] requires 4 values, but storage contains 3
 --> src/main.au
  = note: Aura call chain (innermost first): main at 1:1
```

`AU4007` is the trap code for array shape problems, and you'll see it again.

### Reading one element

An array index has one coordinate per dimension, separated by commas:
`scores[1, 2]` is row 1, column 2, counting from zero. Negative coordinates
count from the end of their own axis, as with lists. The number of
coordinates has to match the number of dimensions, and since the shape is
only known at run time, so is this check:

```aura
def main():
    a = Array[float64].zeros([2, 2])
    print(a[1])
```

```console
$ aura run src/main.au
error[AU4007]: array rank 2 requires 2 coordinates, found 1
 --> src/main.au:3:13
  |
3 |     print(a[1])
  |             ^
  = note: Aura call chain (innermost first): main at 1:1
```

A coordinate that's out of range traps with `AU4003`. For coordinates that
might be bad, `get` takes them as a list and returns `T | None`, so
`scores.get([5, 1])` is `None` instead of a trap. An array element is always
a number, never `None`, so a plain optional is enough here; there's no need
for the `Lookup` type that lists and dictionaries use.

## Changing an array

Changing an array follows the same rule as changing anything else: you need a
mutable binding.

*Filename: src/main.au*

```aura
def main():
    mut grid = Array[int64].zeros([2, 3])
    grid[0, 1] = 7
    old = grid.set([1, 2], 3)
    print(grid)
    print(old)
    mut bonus = Array[float64].full([2, 2], 0.5)
    bonus.fill(1.5)
    print(bonus)
```

```console
$ aura run src/main.au
Array[int64](shape=[2, 3], values=[0, 7, 0, 0, 0, 3])
0
Array[float64](shape=[2, 2], values=[1.5, 1.5, 1.5, 1.5])
```

Indexed assignment writes one element. `set` does the same and hands back
the value it replaced, and `fill` overwrites every element. Without `mut`,
the compiler stops at the first write:

```aura check-fail:AU3003
def main():
    a = Array[float64].zeros([2, 2])
    a[0, 0] = 1.0
```

```console
$ aura check src/main.au
error[AU3003]: cannot assign through immutable place `a[..]`
 --> src/main.au:3:5
  |
3 |     a[0, 0] = 1.0
  |     ^
```

An array can't grow or shrink. Its shape is fixed when it's made, and there's
no `append`, `reshape`, or `transpose`. To get a different shape, build a new
array.

## Arithmetic on whole arrays

Here's where arrays earn their place:

*Filename: src/main.au*

```aura
def to_percent(score: float64) -> float64:
    return score / 100.0

def main():
    scores = Array[float64].from_list([72.0, 88.0, 95.0, 64.0], [2, 2])
    weights = Array[float64].from_list([0.5, 0.5, 0.25, 0.75], [2, 2])
    print(scores + 5.0)
    print(scores * weights)
    print(scores.map(to_percent))
    print(scores.sum())
    print(scores.min())
    print(scores.max())
    print(scores.mean())
    print(sum(scores))
```

<span class="caption">Listing 8-20: Elementwise arithmetic, `map`, and reductions</span>

```console
$ aura run src/main.au
Array[float64](shape=[2, 2], values=[77.0, 93.0, 100.0, 69.0])
Array[float64](shape=[2, 2], values=[36.0, 44.0, 23.75, 48.0])
Array[float64](shape=[2, 2], values=[0.72, 0.88, 0.95, 0.64])
319.0
64.0
95.0
79.75
319.0
```

`scores + 5.0` adds five to every element and returns a new array.
`scores * weights` multiplies the two arrays element by element, which is
*not* matrix multiplication; Aura arrays don't have that. `+`, `-`, `*`, and,
for floating-point arrays, `/` all work this way, with either two arrays or
an array and a single number. `map` applies a function to every element and
returns a new array of the results. `sum`, `min`, and `max` reduce the whole
array to one number of its dtype, and the builtin `sum(scores)` does the same
as `scores.sum()`. `mean` always returns a `float64`, even for an integer
array, so you never lose the fraction.

Every one of these produces a new array and leaves `scores` alone. Arrays are
owned values, so assigning one to another name moves it, and `.clone()` makes
an independent copy when you need two.

### The rules are strict on purpose

NumPy will happily combine arrays of different shapes by *broadcasting*, and
arrays of different types by promoting one of them. Those rules are
convenient and also a common source of results that are subtly wrong: a
column added to a row silently becomes a table. Aura has neither. Two arrays
in one operation must have exactly the same shape:

```aura
def main():
    a = Array[float64].zeros([2, 2])
    b = Array[float64].zeros([2, 3])
    print(a + b)
```

```console
$ aura run src/main.au
error[AU4007]: array operands require the same shape and dtype, found [2, 2] float64 and [2, 3] float64
 --> src/main.au:4:11
  |
4 |     print(a + b)
  |           ^
  = note: Aura call chain (innermost first): main at 1:1
```

and the same dtype, which the compiler can check before the program runs,
because the dtype is part of the type:

```aura check-fail:AU2002
def main():
    a = Array[float64].zeros([2, 2])
    b = Array[float32].zeros([2, 2])
    print(a + b)
```

```console
$ aura check src/main.au
error[AU2002]: Array arithmetic requires matching dtypes, found `Array[float64]` and `Array[float32]`
 --> src/main.au:4:11
  |
4 |     print(a + b)
  |           ^
```

A single number has to have exactly the array's dtype, too. An `int64`
binding doesn't become a `float64` because it's next to a float array:

```aura check-fail:AU2002
def main():
    a = Array[float64].zeros([2, 2])
    bonus: int64 = 5
    print(a + bonus)
```

```console
$ aura check src/main.au
error[AU2002]: Array arithmetic requires scalar dtype `float64`, found `int64`
 --> src/main.au:4:11
  |
4 |     print(a + bonus)
  |           ^
```

Write the number as a float literal, `5.0`, or convert the binding with
`bonus.to_float()`. Be careful with integer literals in particular. At the
time of writing, `a + 5` on a `float64` array gets past `aura check` and
then traps when it runs:

```aura
def main():
    a = Array[float64].zeros([2, 2])
    print(a + 5)
```

```console
$ aura run src/main.au
error[AU4007]: array float64 storage requires float64 scalar at flat index 0, found 5
 --> src/main.au:3:11
  |
3 |     print(a + 5)
  |           ^
  = note: Aura call chain (innermost first): main at 1:1
```

That's a compiler bug, not a rule; the checker ought to reject it with
`AU2002`, as it does for the `int64` binding. Until it's fixed, write
`5.0`.

Integer arrays don't support `/` at all, for the same reason integers don't:
it's unclear whether you want floor division or a fraction.

```aura check-fail:AU2003
def main():
    a = Array[int64].zeros([2, 2])
    print(a / 2)
```

```console
$ aura check src/main.au
error[AU2003]: integer Array `/` is not supported
 --> src/main.au:3:11
  |
3 |     print(a / 2)
  |           ^
  = help: map the values to `float32` or `float64` before using true division
```

Integer arrays keep Aura's checked arithmetic: an `int64` array sum that
overflows traps rather than wrapping. If you want wrapping or saturating
arithmetic, integer arrays have the same `wrapping_add`, `saturating_mul`,
and related methods as integer scalars.

Last, a reduction needs something to reduce. `sum` of an empty array is
zero, but there's no mean of nothing:

```aura
def main():
    a = Array[float64].zeros([2, 0])
    print(a.len())
    print(a.sum())
    print(a.mean())
```

```console
$ aura run src/main.au
0
0.0
error[AU4007]: cannot compute array mean over an empty array
 --> src/main.au
  = note: Aura call chain (innermost first): main at 1:1
```

`min` and `max` behave the same way. A dimension of zero is allowed, so a
2-by-0 array exists; it has no elements.

## Slicing rows

A slice of an array selects whole rows: `scores[1:3]` is a new array with
rows 1 and 2 and every column. It works only on the first axis, and it's a
copy, not a view, so changing the slice doesn't change the original. There's
no step, either; `a[::2]` is refused with `AU2003` and a suggestion to
convert to a list first. That's enough for our gradebook, which needs one
student's row at a time:

*Filename: src/main.au*

```aura
def column_mean(table: Array[float64], column: int64) -> float64:
    rows = table.shape()[0]
    mut total = 0.0
    for row in range(rows):
        total += table[row, column]
    return total / rows.to_float()

def main():
    names = ["ada", "grace", "alan", "kathy"]
    scores = Array[float64].from_list(
        [72.0, 88.0, 95.0,
         95.0, 91.0, 84.0,
         64.0, 58.0, 77.0,
         81.0, 79.0, 90.0],
        [4, 3],
    )
    for i, name in enumerate(names):
        row = scores[i:i + 1]
        print(f"{name:<6} mean {row.mean():.1f}, best {row.max():.0f}")

    assignments = scores.shape()[1]
    for column in range(assignments):
        print(f"assignment {column + 1}: {column_mean(scores, column):.1f}")

    overall = scores.mean()
    spread = scores - overall
    print(f"class mean {overall:.2f}, from {spread.min():.2f} to {spread.max():+.2f}")
```

<span class="caption">Listing 8-21: The gradebook as an array</span>

```console
$ aura run src/main.au
ada    mean 85.0, best 95
grace  mean 90.0, best 95
alan   mean 66.3, best 77
kathy  mean 83.3, best 90
assignment 1: 78.0
assignment 2: 79.0
assignment 3: 86.5
class mean 81.17, from -23.17 to +13.83
```

The per-student numbers come from row slices, so they need no loop of our
own. The per-assignment averages are columns, and since slices only cut
rows, `column_mean` walks down a column with a loop. That's the honest cost
of a deliberately small array type: you get exact, predictable arithmetic,
and for column operations you sometimes write the loop. The last two lines
subtract the class mean from every score in one step and report how far the
lowest and highest scores sit from it.

Arrays have a few more limits worth knowing before you plan a program around
them. They live in ordinary memory on the CPU. They have no equality, so `a
== b` is an `AU2003` error, no views, and no reshaping. The
[Manual](/manual/numeric-arrays) lists exactly what's there, along with
measured performance against NumPy and Rust.

## Summary

You now know the collections that most Aura programs are built from. Lists
keep values in order and give you indexing, slicing, sorting, and in-place
changes through `mut`. Strings are owned UTF-8 text that counts characters as
Unicode scalars, slices without ever splitting one, and offers methods for
the splitting, searching, and formatting that text processing needs.
Dictionaries map keys to values in insertion order, sets answer membership
questions, and both make you be explicit about absence with `Lookup` or
`in`. Comprehensions and builtins such as `sum`, `max`, and `sorted` turn
most of the loops you'd write over these into one expression, and `Array[T]`
handles rectangular tables of numbers with whole-array arithmetic.

Running through all of it is ownership. A collection owns its elements, so
storing a value moves it in, reading one gives you shared access or a copy,
and the compiler tells you when a string or a list would end up with two
owners. You've also met a lot of traps along the way: indexes past the end,
missing keys, `remove` of a value that isn't there, the `max` of an empty
list. In the [next chapter](/book/ch09-00-error-handling) we'll look at
errors properly: when a trap is the right response, and how to use `Result`
to report a problem to the caller and let it decide what to do.
