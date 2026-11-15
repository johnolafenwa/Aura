# Operations and broadcasting

A model is arithmetic on tensors, and a surprisingly short list of
operations covers all of it. We'll meet them here by doing useful things to
the table of houses: finding each column's average, rescaling the columns so
they can be compared, and turning each row into a guessed price. Every one
of these comes back in the chapters that follow.

Each listing in this section starts with the same function that builds the
table, so that every listing is a whole program.

## Arithmetic on every element

The operators `+`, `-`, `*`, and `/` work on two tensors of the same shape,
one pair of elements at a time, and on a tensor and a plain number:

*Filename: src/main.au*

```aura
from tensor import Device, Tensor

def houses(device: Device) -> Tensor[float32]:
    # One row per house: size in square meters, bedrooms, age in years.
    table = Array[float32].from_list(
        [50.0, 1.0, 30.0,
         80.0, 2.0, 12.0,
         120.0, 3.0, 5.0,
         65.0, 2.0, 40.0,
         150.0, 4.0, 2.0,
         95.0, 3.0, 20.0],
        [6, 3],
    )
    return Tensor[float32].from_array(table, device)

def main():
    device = Device.cpu()
    x = houses(device)
    ages = x.slice(1, 2, 3)
    print(ages.shape())
    print(ages.to_array())
    built = 2026.0 - ages
    print(built.to_array())
    decades = ages / 10.0
    print((decades * decades).to_array())
    print(ages.to_array())
```

<span class="caption">Listing 21-5: Arithmetic on a column</span>

```console
$ aura run src/main.au
[6, 1]
Array[float32](shape=[6, 1], values=[30.0, 12.0, 5.0, 40.0, 2.0, 20.0])
Array[float32](shape=[6, 1], values=[1996.0, 2014.0, 2021.0, 1986.0, 2024.0, 2006.0])
Array[float32](shape=[6, 1], values=[9.0, 1.44, 0.25, 16.0, 0.040000003, 4.0])
Array[float32](shape=[6, 1], values=[30.0, 12.0, 5.0, 40.0, 2.0, 20.0])
```

`x.slice(1, 2, 3)` takes a piece of the table. The first argument is the
axis to cut along, counted from zero, so `1` means columns, and the other two
are where to start and where to stop, with the stop left out as in a Python
slice. Columns 2 up to 3 is the age column alone, and the result keeps both
axes: its shape is `[6, 1]`, six rows of one column.

A number may stand on either side of an operator. It is used as the tensor's
own dtype, so `ages / 10.0` is still a `Tensor[float32]`. An integer tensor
takes whole numbers only. `count * 2.5` on a `Tensor[int64]` stops the
program, because the fraction would be lost without anyone asking for that.

The last line prints `ages` again, and it has not changed. No operation
changes a tensor. Each one gives a new tensor and leaves its operands alone,
which is different from PyTorch, where `x.add_(1)` and `x[0] = 5` rewrite a
tensor in place. Aura gives that up for two reasons. One is that a value
that never changes can be handed to any function, or any task, without the
ownership questions of [Chapter 4](/book/ch04-00-understanding-ownership):
`clone()` on a tensor copies a handle and no numbers. The other is gradients.
The next section's machinery works by remembering which tensors a result was
computed from, and that memory would be wrong the moment one of them was
overwritten. PyTorch detects that case while the program runs and raises an
error. In Aura it can't be written.

Besides the operators there is a member for each of the usual functions:
`sqrt()`, `exp()`, `log()`, `abs()`, `sin()`, `cos()`, `tanh()`, and a few
that are specific to machine learning and that we'll introduce when we need
them. The [Manual](/manual/tensors) lists them all.

## Reducing along an axis

A *reduction* boils many numbers down to fewer. `sum()`, `mean()`, `min()`,
and `max()` with no arguments reduce a whole tensor to one element. Given an
axis, they reduce along it:

```aura
from tensor import Device, Tensor

def houses(device: Device) -> Tensor[float32]:
    # One row per house: size in square meters, bedrooms, age in years.
    table = Array[float32].from_list(
        [50.0, 1.0, 30.0,
         80.0, 2.0, 12.0,
         120.0, 3.0, 5.0,
         65.0, 2.0, 40.0,
         150.0, 4.0, 2.0,
         95.0, 3.0, 20.0],
        [6, 3],
    )
    return Tensor[float32].from_array(table, device)

def main():
    device = Device.cpu()
    x = houses(device)
    print(x.max(0).to_array())
    print(x.mean(0).to_array())
    print(x.mean(0).shape())
    print(x.mean(0, true).shape())
    print(x.argmax(0).to_array())
```

<span class="caption">Listing 21-6: The largest and the average of each column</span>

```console
$ aura run src/main.au
Array[float32](shape=[3], values=[150.0, 4.0, 40.0])
Array[float32](shape=[3], values=[93.333336, 2.5, 18.166666])
[3]
[1, 3]
Array[int64](shape=[3], values=[4, 4, 3])
```

The argument is called `dim`, as in PyTorch, and the way to remember what it
does is that it names the axis that disappears. Our table has shape `[6, 3]`.
Reducing along axis 0 collapses the six rows, leaving three numbers, one per
column: the largest house is 150 square meters, and the average age is a
little over 18 years. Reducing along axis 1 would collapse the columns and
leave six numbers, one per house, which for this table would add square
meters to years and mean nothing.

The second argument, `keepdim`, keeps the collapsed axis with length one, so
the result has shape `[1, 3]` in place of `[3]`. That looks like a detail. It
is about to matter.

`argmax` is a reduction that answers a different question: not what the
largest element is, but where. Along axis 0 it says the largest size and the
most bedrooms are in row 4, and the greatest age is in row 3. The result is a
`Tensor[int64]`, because positions are whole numbers. This is how a model
that scores several possible answers tells us which one it picked.

## Broadcasting

The three columns of our table are on very different scales. Sizes run from
50 to 150 and bedrooms from 1 to 4. A model that learns from these numbers
has a much easier time if each column is first moved so its average is zero
and rescaled so its typical distance from zero is one. This is called
*standardizing*, and nearly every training program begins with it.

To center the size column we subtract the average size from every row, the
average bedroom count from every row of the next column, and so on. We have
the three averages as a tensor of shape `[3]`, and the table has shape
`[6, 3]`. Here is what happens when we subtract one from the other:

```aura
from tensor import Device, Tensor

def houses(device: Device) -> Tensor[float32]:
    # One row per house: size in square meters, bedrooms, age in years.
    table = Array[float32].from_list(
        [50.0, 1.0, 30.0,
         80.0, 2.0, 12.0,
         120.0, 3.0, 5.0,
         65.0, 2.0, 40.0,
         150.0, 4.0, 2.0,
         95.0, 3.0, 20.0],
        [6, 3],
    )
    return Tensor[float32].from_array(table, device)

def main():
    device = Device.cpu()
    x = houses(device)
    mean = x.mean(0)
    centered = x - mean
    print(centered.shape())
    spread = (centered * centered).mean(0).sqrt()
    print(spread.to_array())
    scaled = centered / spread
    print(scaled.slice(0, 0, 2).to_array())
    print(scaled.mean(0).to_array())
```

<span class="caption">Listing 21-7: Standardizing the columns</span>

```console
$ aura run src/main.au
[6, 3]
Array[float32](shape=[3], values=[33.623734, 0.95742714, 13.495884])
Array[float32](shape=[2, 3], values=[-1.2887722, -1.5666988, 0.8768106, -0.39654538, -0.52223295, -0.4569294])
Array[float32](shape=[3], values=[-6.953875e-8, 0.0, 5.9604645e-8])
```

It worked, and the result has the table's shape. Aura *broadcast* the three
averages across the six rows, as if we had written them out six times. The
rule is the one `Array` uses, and the one NumPy and PyTorch use. Line the two
shapes up at their right-hand ends. Two shapes can be combined when, in every
position, the lengths are equal or one of them is one, and a shape that runs
out on the left is treated as having ones there:

```text
table      6  3
averages      3      treated as  1  3
result     6  3
```

An axis of length one is stretched to match the other side. So `[6, 3]` and
`[3]` broadcast, and so do `[6, 3]` and `[6, 1]`: a column of six numbers
would be repeated across the three columns. A plain number is the limiting
case, a shape that stretches along every axis.

The `spread` line computes each column's *standard deviation*: the root of
the average squared distance from the mean, which is the usual measure of
how far a column's values typically sit from its center. Dividing by it is
another broadcast. The last line confirms the result: the averages of the
scaled columns are zero, to within what `float32` can represent. The
`slice(0, 0, 2)` on the line before cuts along axis 0, the rows, so we print
only the first two houses.

Broadcasting is also the easiest way to be wrong without an error. Suppose
we wanted each house's numbers divided by that house's own total, and wrote
`x / x.sum(1)`. The sum along axis 1 has shape `[6]`, and `[6, 3]` against
`[6]` lines up 3 against 6:

```aura
from tensor import Device, Tensor

def main():
    device = Device.cpu()
    x = Tensor[float32].full([6, 3], 2.0, device)
    print((x / x.sum(1)).shape())
```

```console
$ aura run src/main.au
error[AU4007]: array operands require the same dtype and shapes that broadcast, found [6, 3] float32 and [6] float32
 --> src/main.au
  = note: Aura call chain (innermost first): Div for tensor.Tensor.div at 1342:5 -> main at 3:1
```

That mistake was caught, because 3 and 6 don't match. Had the table been six
by six, the same line would have run and divided each column by a row's
total. This is what `keepdim` is for. `x.sum(1, true)` has shape `[6, 1]`,
which lines up against `[6, 3]` the way we meant. When you reduce a tensor in
order to combine the result with the tensor itself, keep the axis.

## Matrix multiplication

Now for a first guess at prices. Say a house is worth 2 for each square
meter and 15 for each bedroom, loses 1 for each year of age, and starts from
a base of 50, all in thousands. For one house that is three multiplications
and a sum. For the whole table it is a *matrix product*, the operation at
the center of every model in this book:

```aura
from tensor import Device, Tensor

def houses(device: Device) -> Tensor[float32]:
    # One row per house: size in square meters, bedrooms, age in years.
    table = Array[float32].from_list(
        [50.0, 1.0, 30.0,
         80.0, 2.0, 12.0,
         120.0, 3.0, 5.0,
         65.0, 2.0, 40.0,
         150.0, 4.0, 2.0,
         95.0, 3.0, 20.0],
        [6, 3],
    )
    return Tensor[float32].from_array(table, device)

def main():
    device = Device.cpu()
    x = houses(device)
    weights = Tensor[float32].from_array(
        Array[float32].from_list([2.0, 15.0, -1.0], [3, 1]),
        device,
    )
    guess = x.matmul(weights) + 50.0
    print(guess.shape())
    print(guess.to_array())
```

<span class="caption">Listing 21-8: A price for every house in one operation</span>

```console
$ aura run src/main.au
[6, 1]
Array[float32](shape=[6, 1], values=[135.0, 228.0, 330.0, 170.0, 408.0, 265.0])
```

`x.matmul(weights)` takes each row of `x`, multiplies it element by element
with the column of `weights`, and adds up the products. The first house is
50 times 2, plus 1 times 15, plus 30 times minus 1, which is 85, and the
base of 50 brings it to 135. The shape rule is that the inner lengths must
agree and the outer ones give the result: `[6, 3]` times `[3, 1]` is
`[6, 1]`, one price per house. If `weights` had two columns, each column
would be a separate set of weights and we would get two guesses per house,
which is how a model produces a score for each of several classes.

When the inner lengths don't agree, the product has no meaning, and Aura
says so. Changing the shape of `weights` to `[1, 3]` gives:

```text
error[AU4007]: array matmul requires shapes [.., n, k] and [.., k, m], [.., n, k] and [k], or [k] and [.., k, m] of one dtype with leading dimensions that broadcast, found [6, 3] float32 and [1, 3] float32
```

The message spells out the shapes it would have accepted. The `..` means a
product also works on tensors of higher rank, treating the last two axes as
the matrix and everything before them as a batch of matrices. We'll rely on
that in [Chapter 24](/book/ch24-00-transformer-from-scratch).

Those four numbers, the three weights and the base, are the entire model.
The next chapter is about finding them from sold prices, when nobody has
told us the rule.

## Masks

Comparisons give a tensor of `bool`, called a *mask*, with `true` wherever
the comparison holds. Masks answer questions about the data, and with
`where` they choose between two tensors element by element:

```aura
import tensor
from tensor import Device, Tensor

def houses(device: Device) -> Tensor[float32]:
    # One row per house: size in square meters, bedrooms, age in years.
    table = Array[float32].from_list(
        [50.0, 1.0, 30.0,
         80.0, 2.0, 12.0,
         120.0, 3.0, 5.0,
         65.0, 2.0, 40.0,
         150.0, 4.0, 2.0,
         95.0, 3.0, 20.0],
        [6, 3],
    )
    return Tensor[float32].from_array(table, device)

def main():
    device = Device.cpu()
    x = houses(device)
    sizes = x.slice(1, 0, 1).reshape([6])
    large = sizes.greater(sizes.scalar(90.0))
    print(large.to_array())
    one = sizes.scalar(1.0)
    zero = sizes.scalar(0.0)
    print(tensor.where(large, one, zero).sum().item())
    print(tensor.where(large, sizes, zero).to_array())
    print(sizes.clamp(60.0, 120.0).to_array())
```

<span class="caption">Listing 21-9: Which houses are large, how many, and which sizes</span>

```console
$ aura run src/main.au
Array[bool](shape=[6], values=[false, false, true, false, true, true])
3.0
Array[float32](shape=[6], values=[0.0, 0.0, 120.0, 0.0, 150.0, 95.0])
Array[float32](shape=[6], values=[60.0, 80.0, 120.0, 65.0, 120.0, 95.0])
```

The comparisons are members: `less`, `less_equal`, `greater`,
`greater_equal`, `equal`, and `not_equal`. They compare two tensors, so to
compare against a number we first make a one-element tensor with
`sizes.scalar(90.0)`, which lands on the same device as `sizes` and
broadcasts like any other.

`tensor.where(mask, a, b)` takes each element from `a` where the mask is
true and from `b` where it is false. Choosing between one and zero and adding
up the result counts the true entries, and we'll compute a model's accuracy
exactly that way. `clamp(low, high)` is a shorter spelling of a common pair of
`where` calls: everything below 60 becomes 60, and everything above 120
becomes 120.

`reshape([6])` on the second line of `main` turned the `[6, 1]` column into
a flat `[6]`. Reshaping never moves or changes an element. It reads the same
numbers, in the same row-by-row order, under a different shape, so the new
shape must hold the same count. One length may be written as `-1`, and Aura
works it out: `reshape([-1])` flattens anything.

## Picking rows

The last operation we need takes rows by number. Training looks at the data
a few rows at a time, in a shuffled order, and this is how the rows are
fetched:

```aura
from tensor import Device, Tensor

def houses(device: Device) -> Tensor[float32]:
    # One row per house: size in square meters, bedrooms, age in years.
    table = Array[float32].from_list(
        [50.0, 1.0, 30.0,
         80.0, 2.0, 12.0,
         120.0, 3.0, 5.0,
         65.0, 2.0, 40.0,
         150.0, 4.0, 2.0,
         95.0, 3.0, 20.0],
        [6, 3],
    )
    return Tensor[float32].from_array(table, device)

def main():
    device = Device.cpu()
    x = houses(device)
    rows = Tensor[int64].from_array(Array[int64].from_list([4, 0, 4], [3]), device)
    picked = x.index_select(0, rows)
    print(picked.shape())
    print(picked.to_array())
```

<span class="caption">Listing 21-10: Rows 4, 0, and 4 again</span>

```console
$ aura run src/main.au
[3, 3]
Array[float32](shape=[3, 3], values=[150.0, 4.0, 2.0, 50.0, 1.0, 30.0, 150.0, 4.0, 2.0])
```

`index_select(dim, indices)` reads along an axis at the positions an integer
tensor names. The positions can come in any order and can repeat. Notice
what this does to the type: a `Tensor[int64]` of row numbers went in, and a
`Tensor[float32]` of rows came out. In Chapter 24 the row numbers will be
words, the table will hold a row of learned numbers for each word, and this
one operation will be how text gets into a model.

That is the working set: arithmetic with broadcasting, reductions along an
axis, the matrix product, masks, and reshaping, slicing, and picking. What
none of them can do yet is tell us how to make a guess better. That takes
one more idea.
