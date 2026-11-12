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
error[AU2002]: Array dtype must be one of `int32`, `int64`, `float32`, `float64`, or `bool`, found `str`
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

An array can't grow or shrink, so there's no `append`. You can look at the
same elements under another shape, though, and write to whole rows and
columns at once. Both come up later in this section.

A loop over the elements is a fine way to fill or read an array when no
whole-array method says what you mean:

*Filename: src/main.au*

```aura
def main():
    mut table = Array[int64].zeros([3, 4])
    for row in range(3):
        for column in range(4):
            table[row, column] = (row + 1) * (column + 1)
    mut diagonal = 0
    for i in range(3):
        diagonal += table[i, i]
    print(table)
    print(diagonal)
```

```console
$ aura run src/main.au
Array[int64](shape=[3, 4], values=[1, 2, 3, 4, 2, 4, 6, 8, 3, 6, 9, 12])
14
```

In a native build, a read or a write like `table[row, column]` goes straight
to the element after a range check on each coordinate. A loop that touches
the array only through its elements runs about as fast as the same loop over
a list.

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
*not* matrix multiplication; that's the `matmul` method you'll meet later in
this section. `+`, `-`, `*`, and,
for floating-point arrays, `/` all work this way, with either two arrays or
an array and a single number. `map` applies a function to every element and
returns a new array of the results. `sum`, `min`, and `max` reduce the whole
array to one number of its dtype, and the builtin `sum(scores)` does the same
as `scores.sum()`. `mean` always returns a `float64`, even for an integer
array, so you never lose the fraction.

Every one of these produces a new array and leaves `scores` alone. Arrays are
owned values, so assigning one to another name moves it, and `.clone()` makes
an independent copy when you need two.

### Shapes stretch, types don't

Two arrays in one operation don't need the same shape, as long as the
shapes *broadcast*. Line the two shapes up at their last dimension. Each
pair of dimensions has to be equal, or one of them has to be one. A shape
with fewer dimensions counts as having ones in front. The smaller operand
is then repeated along every dimension where it has one:

```aura
def main():
    scores = Array[float64].from_list([72.0, 88.0, 95.0, 64.0, 80.0, 91.0], [2, 3])
    curve = Array[float64].from_list([5.0, 0.0, -2.0], [3])
    print(scores + curve)
    print(scores - scores.mean(0))
```

```console
$ aura run src/main.au
Array[float64](shape=[2, 3], values=[77.0, 88.0, 93.0, 69.0, 80.0, 89.0])
Array[float64](shape=[2, 3], values=[4.0, 4.0, 2.0, -4.0, -4.0, -2.0])
```

`curve` has three elements and `scores` has rows of three, so the curve is
added to each row. `scores.mean(0)` is the mean of each column, which
you'll meet properly later in this section, and subtracting it centers
every column on zero. If you know NumPy, these are its rules.

Shapes that don't line up are still an error:

```aura
def main():
    a = Array[float64].zeros([2, 2])
    b = Array[float64].zeros([2, 3])
    print(a + b)
```

```console
$ aura run src/main.au
error[AU4007]: array operands require the same dtype and shapes that broadcast, found [2, 2] float64 and [2, 3] float64
 --> src/main.au:4:11
  |
4 |     print(a + b)
  |           ^
  = note: Aura call chain (innermost first): main at 1:1
```

What Aura won't do is change a type for you. NumPy promotes an integer
array next to a float array, and that hides a conversion that can lose
precision. In Aura both arrays need the same dtype, which the compiler can
check before the program runs,
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

## Rows, columns, and blocks

An array index takes one piece per dimension, and each piece can be a
coordinate or a slice. A coordinate picks one position. A slice, written
with a colon as for lists, keeps a range, and a bare `:` keeps everything
along that dimension:

*Filename: src/main.au*

```aura
def main():
    scores = Array[float64].from_list(
        [72.0, 88.0, 95.0,
         95.0, 91.0, 84.0,
         64.0, 58.0, 77.0],
        [3, 3],
    )
    print(scores[1, :])
    print(scores[:, 0])
    print(scores[::2, 1:])
    print(scores[::-1, 0])
    print(scores[0:2].shape())
    print(scores[0:1, :].shape())
```

```console
$ aura run src/main.au
Array[float64](shape=[3], values=[95.0, 91.0, 84.0])
Array[float64](shape=[3], values=[72.0, 95.0, 64.0])
Array[float64](shape=[2, 2], values=[88.0, 95.0, 58.0, 77.0])
Array[float64](shape=[3], values=[64.0, 95.0, 72.0])
[2, 3]
[1, 3]
```

`scores[1, :]` is row 1 and `scores[:, 0]` is column 0. `scores[::2, 1:]`
takes every second row and drops the first column, and a negative step
walks backwards, so `scores[::-1, 0]` is the first column from the bottom
up. Dimensions you don't mention are kept whole, which is why `scores[0:2]`
is two full rows.

Notice the last two lines. A coordinate removes its dimension and a slice
keeps it, even a slice one element long. `scores[0, :]` has shape `[3]`,
and `scores[0:1, :]` has shape `[1, 3]`.

Slices follow the list rules you already know: negative bounds count from
the end, and a bound past the end is an `AU4003` trap, never a quietly
shorter result.

That's what our gradebook needs. One student is a row, and one assignment is
a column:

*Filename: src/main.au*

```aura
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
        row = scores[i, :]
        print(f"{name:<6} mean {row.mean():.1f}, best {row.max():.0f}")

    assignments = scores.shape()[1]
    for column in range(assignments):
        print(f"assignment {column + 1}: {scores[:, column].mean():.1f}")

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

Neither loop touches an element by hand. The last two lines subtract the
class mean from every score in one step and report how far the lowest and
highest scores sit from it.

### A slice doesn't copy, and a write doesn't leak

Taking `scores[i, :]` doesn't copy the row. The new array reads the same
block of memory as `scores`, so a slice of a huge table costs the same as a
slice of a small one.

If you've used NumPy, that might worry you. There, a slice is a second name
for the same memory, and writing through one name changes what you see
through the other. Aura doesn't allow that. An array is a value, like a
list, and nothing you do to one array changes another:

*Filename: src/main.au*

```aura
def main():
    mut scores = Array[float64].from_list(
        [72.0, 88.0, 95.0,
         95.0, 91.0, 84.0,
         64.0, 58.0, 77.0],
        [3, 3],
    )
    before = scores[2, :]
    scores[:, 1] += 5.0
    scores[2, :] = 70
    scores[0, 1:] = scores[1, 1:]
    print(scores)
    print(before)
```

<span class="caption">Listing 8-22: Writing to rows and columns</span>

```console
$ aura run src/main.au
Array[float64](shape=[3, 3], values=[72.0, 96.0, 84.0, 95.0, 96.0, 84.0, 70.0, 70.0, 70.0])
Array[float64](shape=[3], values=[64.0, 58.0, 77.0])
```

A slice on the left of `=` writes to every element it selects. The value
can be one number, which fills the selection, or an array of the right
shape. `scores[:, 1] += 5.0` gives everyone five points on the second
assignment, `scores[2, :] = 70` overwrites a whole row, and the last
assignment copies part of one row over another.

`before` still holds the old third row. Aura got there by sharing memory
until the first write: when `scores[:, 1] += 5.0` ran, `before` was still
reading the same block, so `scores` copied its elements once and wrote to
its own copy. After that, `scores` is alone in its block and the later
writes happen in place.

So the rule to remember is small. Slices are free, and a write to an array
that something else is still reading costs one copy. If you keep a slice
around while you update the table in a loop, call `.clone()` on the slice.
That gives it a small block of its own and leaves the table free to change
in place.

### Another shape for the same numbers

`transpose` swaps rows and columns, and `reshape` lays the same elements
out under a new shape. Like slices, they share memory with the original:

*Filename: src/main.au*

```aura
def main():
    scores = Array[float64].from_list(
        [72.0, 88.0, 95.0,
         95.0, 91.0, 84.0],
        [2, 3],
    )
    by_assignment = scores.transpose()
    print(by_assignment.shape())
    print(by_assignment[0, :])
    print(scores.reshape([3, 2]))
    print(scores.reshape([-1, 2]).shape())
    print(by_assignment.flatten())
```

```console
$ aura run src/main.au
[3, 2]
Array[float64](shape=[2], values=[72.0, 95.0])
Array[float64](shape=[3, 2], values=[72.0, 88.0, 95.0, 95.0, 91.0, 84.0])
[3, 2]
Array[float64](shape=[6], values=[72.0, 95.0, 88.0, 91.0, 95.0, 84.0])
```

`reshape` reads the elements row by row and deals them out again, so the
new shape has to hold the same number of elements. Write `-1` for one
dimension and Aura works it out from the others. `flatten` is the reshape
to a single dimension. For arrays with more than two dimensions,
`permute([2, 0, 1])` puts the axes in any order you name.

### Products and Faster Sums

Two arrays of the same shape have a *dot product*, the sum of their
matching elements multiplied together, and two tables whose inner sizes
agree have a *matrix product*. Both are methods:

*Filename: src/main.au*

```aura
def main():
    scores = Array[float64].from_list(
        [72.0, 88.0, 95.0,
         95.0, 91.0, 84.0],
        [2, 3],
    )
    weights = Array[float64].from_list([0.2, 0.3, 0.5], [3])
    final = scores.matmul(weights)
    print(final)
    print(weights.dot(weights))
```

<span class="caption">Listing 8-23: A weighted grade with `matmul`</span>

```console
$ aura run src/main.au
Array[float64](shape=[2], values=[88.3, 88.3])
0.38
```

`scores` has shape `[2, 3]` and `weights` has shape `[3]`, so the product
has one number per student. `matmul` also multiplies `[n, k]` by `[k, m]`
into `[n, m]`. Shapes that don't line up stop the program with `AU4007`.

An array with more than two dimensions is a stack of matrices. `matmul`
uses the last two dimensions as the matrix and multiplies once per
matrix in the stack, so a `[10, 2, 3]` array times a `[3, 4]` array is a
`[10, 2, 4]` array. You don't need a loop over the stack.

`sum` adds from left to right, which is the order you'd add by hand and the
slowest order for a CPU. When an array is large and the last digit doesn't
matter, `fast_sum` and `fast_mean` add in a fixed order that runs several
times faster. The order is the same on every machine, so the answer is
still reproducible. It just isn't always the same last digit as `sum`.

Arrays have a few more limits worth knowing before you plan a program around
them. They live in ordinary memory on the CPU, they have no equality, so `a
== b` is an `AU2003` error, and you can't loop over one with `for`. The
[Manual](/manual/numeric-arrays) lists exactly what's there, along with
measured performance against NumPy and Rust.

### Several Floats at Once

Sometimes the kernel you need isn't one of the Array methods, and you write
a loop over a list of floats. Most such loops are fast already. A running
float sum is the exception: Aura adds in the order you wrote, one element
at a time, because any other order can change the last digit.

The `simd` module lets you pick a faster order yourself. Its *lane values*
hold a few floats side by side, and one `+` or `*` works on all of them.
`F64x4` holds four `float64` lanes:

*Filename: src/main.au*

```aura
from simd import F64x4

def dot(xs: list[float64], ys: list[float64]) -> float64:
    mut acc = F64x4.splat(0.0)
    for i in range(0, len(xs) - 3, 4):
        acc = acc + F64x4.load(xs, i) * F64x4.load(ys, i)
    mut total = acc.sum()
    for i in range(len(xs) - len(xs) % 4, len(xs)):
        total += xs[i] * ys[i]
    return total

def main():
    xs = [1.0, 2.0, 3.0, 4.0, 5.0, 6.0]
    ys = [0.5, 0.25, 2.0, 1.0, 3.0, 0.5]
    print(dot(xs, ys))
```

<span class="caption">Listing 8-24: A dot product four elements at a time</span>

```console
$ aura run src/main.au
29.0
```

`splat` fills every lane with one number, and `load(xs, i)` reads the four
elements starting at `i`. The first loop steps by four and stops while
four elements are still left, so `acc` ends up holding four separate
running sums. `acc.sum()` adds them together, and the second loop picks up
the one to three elements at the end.

In a native build each lane value lives in the CPU's vector registers, and
this loop runs about three times as fast as the one-at-a-time version. The
answer is the same on every machine and in `aura run`, because every
operation is defined lane by lane. It can differ in the last digit from a
plain left-to-right sum, since the additions happen in a different order.

There are also `F64x2`, `F32x4`, and `F32x8`, with `store` to write lanes
back into a list and `min`, `max`, `abs`, and `sqrt` for each lane. When
the numbers live in a one-dimensional array, `load_array` and `store_array`
do the same job: `F64x4.load_array(xs, i)` reads four elements of an
`Array[float64]`. The [Manual](/manual/simd) has the full list.

### One Number per Row

`sum`, `min`, `max`, and `mean` reduce a whole array to one number. Give
any of them an axis and it reduces along that axis only, and returns an
array with that axis removed:

*Filename: src/main.au*

```aura
def main():
    scores = Array[float64].from_list([72.0, 88.0, 95.0, 64.0, 80.0, 91.0], [2, 3])
    print(scores.sum(0))
    print(scores.mean(1))
    print(scores.max(1))
    print(scores / scores.sum(1).reshape([2, 1]))
```

<span class="caption">Listing 8-25: Reductions along one axis</span>

```console
$ aura run src/main.au
Array[float64](shape=[3], values=[136.0, 168.0, 186.0])
Array[float64](shape=[2], values=[85.0, 78.33333333333333])
Array[float64](shape=[2], values=[95.0, 91.0])
Array[float64](shape=[2, 3], values=[0.2823529411764706, 0.34509803921568627, 0.37254901960784315, 0.2723404255319149, 0.3404255319148936, 0.3872340425531915])
```

Axis 0 runs down the rows, so `scores.sum(0)` adds each column. Axis 1 runs
along a row, so `scores.mean(1)` is one mean per student. A negative axis
counts from the last, as a negative index does. The last line divides
every score by its row's total. The row totals have shape `[2]`, which
would line up with the columns, so `reshape([2, 1])` turns them into a
column that broadcasts across each row.

### Math, Comparisons, and Masks

`map` calls your function once per element. For the common functions there
are members that do the whole array in one call:

*Filename: src/main.au*

```aura
def main():
    scores = Array[float64].from_list([72.0, 88.0, 95.0, 64.0, 80.0, 91.0], [2, 3])
    passed = scores.greater_equal(75.0)
    print(passed)
    print(passed.count())
    print(passed.all())
    print(passed.choose(scores, 0.0))
    print(scores.minimum(90.0))
    print(scores.sqrt().floor())
    print((scores as Array[int64]).sum(0))
```

<span class="caption">Listing 8-26: A mask, a choice, and elementwise math</span>

```console
$ aura run src/main.au
Array[bool](shape=[2, 3], values=[false, true, true, false, true, true])
4
false
Array[float64](shape=[2, 3], values=[0.0, 88.0, 95.0, 0.0, 80.0, 91.0])
Array[float64](shape=[2, 3], values=[72.0, 88.0, 90.0, 64.0, 80.0, 90.0])
Array[int64](shape=[2, 3], values=[8, 9, 9, 8, 8, 9])
Array[int64](shape=[3], values=[136, 168, 186])
```

`greater_equal` compares every element with `75.0` and returns an
`Array[bool]`, called a *mask*. The other comparisons are `less`,
`less_equal`, `greater`, `equal`, and `not_equal`, and each takes one
number or an array that broadcasts. A mask answers questions with `any`,
`all`, and `count`, combines with other masks through `logical_and`,
`logical_or`, and `logical_not`, and picks values with `choose`: an element
from the first argument where the mask is `true`, and from the second
elsewhere.

`abs`, `minimum`, and `maximum` work on every numeric dtype. `sqrt`, `exp`,
`log`, `log2`, `log10`, `sin`, `cos`, `tan`, and `pow` work on `float64`
arrays and follow the functions of the `math` module, including their
errors: `log` of a negative element traps and names the element's
position. `floor`, `ceil`, and `trunc` return an `Array[int64]`, as the
`math` functions return `int64`.

`scores as Array[int64]` converts every element as `as` converts one
number. A conversion that would lose information traps, so an integer that
doesn't fit the smaller type is an error, not a wrapped value. Going the
other way, `to_float()` on an integer array always succeeds and rounds to
the nearest `float64`.

Comparisons are members and not operators for a reason. `==` between two
lists gives one `bool`, and an `==` that gave an array of them would make
`if a == b:` mean something different for arrays. So `a == b` on arrays
stays an error, and `a.equal(b).all()` says what you mean.

## Computing on a GPU

An `Array` lives in the computer's ordinary memory and the CPU does its
arithmetic. For very large arrays a graphics processor can be much
faster. The `tensor` module gives you `Tensor[T]`, which holds numbers on
a *device*: the CPU, the GPU of a Mac, or an NVIDIA GPU on Linux.

```aura
from tensor import Device, Tensor

def main():
    device = Device.default()
    prices = Array[float32].from_list([10.0, 12.5, 9.0, 14.0], [4])
    counts = Array[float32].from_list([3.0, 1.0, 4.0, 2.0], [4])
    x = Tensor[float32].from_array(prices, device)
    y = Tensor[float32].from_array(counts, device)
    print((x * y).sum().item())
```

```text
106.5
```

`Device.default()` picks the GPU when the machine has one Aura can use
and the CPU otherwise, so this program runs everywhere. You move numbers
in with `from_array` and out with `to_array` or `item`, and in between a
tensor has the operators and most of the members you just learned for
arrays, with the same shape rules.

Two things differ from an array. A tensor is never changed in place:
every operation gives a new one. And a GPU does not compute anything
until you read a result, so that it can run a whole expression in one
trip. The [Manual](/manual/tensors) explains when that matters and which
dtypes a GPU has.

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
handles rectangular tables of numbers with whole-array arithmetic,
broadcasting, and masks.

Running through all of it is ownership. A collection owns its elements, so
storing a value moves it in, reading one gives you shared access or a copy,
and the compiler tells you when a string or a list would end up with two
owners. You've also met a lot of traps along the way: indexes past the end,
missing keys, `remove` of a value that isn't there, the `max` of an empty
list. In the [next chapter](/book/ch09-00-error-handling) we'll look at
errors properly: when a trap is the right response, and how to use `Result`
to report a problem to the caller and let it decide what to do.
