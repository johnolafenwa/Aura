# Numeric Arrays

`Array[T]` is Aura's owned numeric container for the CPU. Use it for local
preprocessing, postprocessing, evaluation, and batch-shaped numeric work. It
is smaller than a general tensor framework:

- Shape is runtime metadata.
- Storage is row-major and lives on the host.
- An Array is a value. Slices, transposes, and reshapes read the same
  elements without copying them, and no Array sees a write made through
  another.

The dtypes, or element types, are `int32`, `int64`, `float32`, `float64`,
and `bool`. An `Array[bool]` is a mask: the result of a comparison. It has
no arithmetic.

```aura
def main() -> int32:
    left = Array[float64].from_list([1.0, 2.0, 3.0, 4.0], [2, 2])
    right = Array[float64].full([2, 2], 0.5)
    combined = left + right
    first_row = combined[0, :]
    print(combined.shape())
    print(combined[1, 0])
    print(first_row.sum())
    print(combined.transpose()[0, :])
    print(combined.mean())
    return 0
```

## Grammar

`Array` is a global builtin generic type, not a module. It uses the ordinary
grammar for specialization, calls, member calls, indexing, and indexed
assignment. An Array index takes one component per axis, and a component is
a coordinate or a slice:

```text
Array [ dtype ]
Array [ dtype ] . constructor ( arguments )
array [ component { , component } ]
component = expression | [ expression ] : [ expression ] [ : [ expression ] ]
```

The supported `dtype` names are exactly `int32`, `int64`, `float32`,
`float64`, and `bool`. Comma-separated Array indexing is different from list indexing.
[Grammar](/manual/grammar) defines the complete syntax.

Aura has no Array literal, dtype value, or rank annotation.

## Typing Rules

### Constructors

These are all the constructors:

| Constructor | Result |
| --- | --- |
| `Array[T].zeros(shape: list[int64])` | `Array[T]` |
| `Array[T].full(shape: list[int64], value: T)` | `Array[T]` |
| `Array[T].from_list(values: list[T], shape: list[int64])` | `Array[T]` |

`T` must be one of the five dtypes. Shape is a runtime `list[int64]`, so rank
and dimensions are not part of the static type.

`from_list` requires exactly `list[T]`. It copies the scalar elements, and the
shared source list stays usable. Constructors never infer a different dtype
from a mixed numeric source.

### Members

| Member | Result and contract |
| --- | --- |
| `shape()` | `list[int64]`; owned shape snapshot |
| `len()` | `int64`; total element count |
| `clone()` | `Array[T]`; an independent copy |
| `reshape(shape: list[int64])` | `Array[T]`; the same elements under another shape |
| `flatten()` | `Array[T]`; the elements as one axis |
| `transpose()` | `Array[T]`; the axes in reverse order |
| `permute(axes: list[int64])` | `Array[T]`; the axes in the order given |
| `get(index: list[int64])` | `T \| None`; `None` for an out-of-bounds coordinate or rank mismatch |
| `set(index: list[int64], value: T)` | mutable receiver; `T \| None` holding the replaced value, or a coordinate/rank trap rather than `None` |
| `fill(value: T)` | mutable receiver; returns `None` |
| `map[U](f: def(T) -> U)` | `Array[U]`; `U` is one of the five dtypes |
| `sum()`, `min()`, `max()` | `T`; numeric dtypes |
| `mean()` | `float64` for every numeric dtype |
| `fast_sum()` | `T`; the sum in the fixed reordered order |
| `fast_mean()` | `float64`; the mean in the fixed reordered order |
| `sum(axis: int64)`, `min(axis)`, `max(axis)`, `fast_sum(axis)` | `Array[T]`; the reduction of each lane along `axis` |
| `mean(axis: int64)`, `fast_mean(axis)` | `Array[float64]` |
| `abs()` | `Array[T]`; numeric dtypes |
| `minimum(other)`, `maximum(other)` | `Array[T]`; numeric dtypes; `other` is `Array[T]` or `T` |
| `sqrt()`, `exp()`, `log()`, `log2()`, `log10()`, `sin()`, `cos()`, `tan()` | `Array[float64]`; `float64` only |
| `pow(exponent)` | `Array[float64]`; `float64` only; `exponent` is `Array[float64]` or `float64` |
| `floor()`, `ceil()`, `trunc()` | `Array[int64]`; `float64` only |
| `to_float()` | `Array[float64]`; `int32` and `int64` only |
| `less(other)`, `less_equal(other)`, `greater(other)`, `greater_equal(other)`, `equal(other)`, `not_equal(other)` | `Array[bool]`; numeric dtypes; `other` is `Array[T]` or `T` |
| `any()`, `all()` | `bool`; `Array[bool]` only |
| `count()` | `int64`; `Array[bool]` only; the number of `true` elements |
| `logical_and(other)`, `logical_or(other)` | `Array[bool]`; `Array[bool]` only; `other` is `Array[bool]` or `bool` |
| `logical_not()` | `Array[bool]`; `Array[bool]` only |
| `choose(when_true, when_false)` | `Array[U]`; `Array[bool]` only; each argument is `Array[U]` or `U` |
| `dot(other: Array[T])` | `T`; inner product of two Arrays of the same shape |
| `matmul(other: Array[T])` | fresh `Array[T]`; matrix product |

`map` requires a repeatable callable whose bare parameter type and return
type match exactly. The checker rejects a consuming closure, a `mut` or `own`
parameter, and an unsupported result dtype. `set` and `fill` require a mutable
Array place.

`dot` and `matmul` take an Array of exactly the receiver's type. They borrow
both operands and consume neither.

A member called on a dtype it does not have is `AU2003`. An operand marked
`Array[T]` or `T` follows the operator rules: an Array of the receiver's
dtype whose shape broadcasts, or one element of exactly `T`. A numeric
literal takes the dtype. `choose` takes its result dtype `U` from an Array
argument, or from its first argument when both are elements.

### Conversion

`array as Array[U]` converts between the four numeric dtypes and has type
`Array[U]`. It reads its operand and does not move it. A cast to or from
`Array[bool]`, or between an Array and a scalar, is rejected.

### Indexing And Slicing

An index of coordinates only, such as `array[i, j]`, has type `T`. It takes
exactly one `int64` coordinate per runtime axis. Assignment through it
requires a mutable Array place and a value of exactly `T`.

An index with a slice among its components, such as `array[i, :]` or
`array[::2]`, has type `Array[T]`. Assignment through it requires a mutable
Array place and a value of type `T` or `Array[T]`. Slice bounds and steps are
`int64` or a losslessly narrower integer type.

### Operators

`+`, `-`, and `*` accept either of these operand pairs and return a fresh
`Array[T]`:

- two Arrays with the same numeric dtype, whose shapes broadcast at run time
- one Array and one scalar of exactly `T`, in either order

`/` has the same forms for floating Arrays only. Integer Array `/` is rejected
with `AU2003`.

`array[selection] op= value` with `+=`, `-=`, `*=`, or `/=` takes the same
operands and updates the selected elements.

There is no mixed promotion. An `Array[int32]` and an `Array[int64]` do not
combine. `Array[bool]` has no arithmetic operators, and no Array has `==`,
`!=`, or an ordering operator: comparisons are members. A bound scalar is never
implicitly widened or narrowed for an Array operation.

### Wrapping And Saturating Arithmetic

Every scalar integer type, plus `Array[int32]` and `Array[int64]`, provides
`wrapping_add`, `wrapping_sub`, `wrapping_mul`, `saturating_add`,
`saturating_sub`, and `saturating_mul`. The Array methods accept one scalar of
the same dtype or one Array whose shape broadcasts. Ordinary arithmetic stays
checked.

## Runtime Semantics

### Shape And Layout

An Array has rank one or more and reads its elements in row-major order.
Dimensions are `int64`. They can be zero but not negative. `len()` is the
checked product of all dimensions, so a zero dimension makes the Array empty
while keeping its full shape.

`zeros`, `full`, and `from_list` lay out elements in row-major order, and
direct coordinates map to storage in the same order.

### Coordinates

A negative coordinate is normalized once against its own axis. For an invalid
coordinate:

- `get` returns `None`.
- Method `set`, direct indexed reads, and direct indexed assignment trap.

A valid `set` returns the previous scalar as its `T | None` result.

### Selections

Each component of an index applies to one axis, in order. A coordinate picks
one position and removes the axis. A slice keeps the axis. Axes after the
last component are taken whole.

```aura
def main() -> int32:
    grid = Array[int64].from_list([0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11], [3, 4])
    print(grid[1, :])
    print(grid[:, 2])
    print(grid[::2, 1:3])
    print(grid[::-1])
    print(grid[1:2, :].shape())
    return 0
```

It prints the second row, the third column, a two by two block, the rows in
reverse order, and `[1, 4]`.

A slice follows the rules of a list slice. Omitted bounds take the whole
axis, a negative bound counts from the end once, and a step may be negative.
Bounds never clamp. A bound outside the axis, a reversed range, or a zero
step is `AU4003`. More components than the Array has axes is `AU4007`.

A selection does not copy elements. See [Shared Storage](#shared-storage).

### Shape Members

| Member | Result |
| --- | --- |
| `reshape(shape)` | The same elements in row-major order under `shape`. One dimension may be `-1`, which takes the length the others leave. |
| `flatten()` | `reshape([-1])`. |
| `transpose()` | The axes in reverse order. A rank-1 Array is unchanged. |
| `permute(axes)` | The axes in the order `axes` names them. Each axis appears once, and a negative axis counts from the last. |

```aura
def main() -> int32:
    grid = Array[int64].from_list([0, 1, 2, 3, 4, 5], [2, 3])
    print(grid.transpose())
    print(grid.reshape([3, -1]))
    print(grid.transpose().flatten())
    return 0
```

A `reshape` whose shape holds a different number of elements, and a
`permute` that repeats or omits an axis, are `AU4007`.

### Slice Assignment

`array[selection] = value` writes every selected element. The value is one
element, which fills the selection, or an Array whose shape broadcasts to
the selected shape: each dimension is equal or one, aligned at the last
axis.

```aura
def main() -> int32:
    mut grid = Array[float64].zeros([3, 4])
    grid[0, :] = 1
    grid[:, 3] = Array[float64].from_list([7.0, 8.0, 9.0], [3])
    grid[1:, :2] += 0.5
    grid[2, :] = grid[0, :]
    print(grid)
    return 0
```

- A numeric literal takes the Array's dtype.
- `array[selection] op= value` reads the selection once, applies the
  operator, and writes the result back.
- The value is complete before the write starts, so it may read the Array it
  is assigned into. `values[1:] = values[:-1]` shifts the elements as they
  were.
- The selection and the value's shape are checked before anything is
  written. A value of another shape is `AU4007`.

### Shared Storage

`clone()`, a selection, `reshape`, `flatten`, `transpose`, and `permute`
return an Array that reads the same element buffer as the source. They cost
the same for any element count. Two exceptions copy elements:

- `reshape` and `flatten` of a view that is not one row-major block, such as
  a transposed matrix or a stepped slice.
- `clone()` of an Array that reads part of a larger buffer. The copy holds
  only its own elements.

A write never shows through another Array. `set`, `fill`, and assignment
write in place when the Array is the only one reading its buffer. Otherwise
the Array first copies its own elements, and the other Arrays keep the old
buffer:

```aura
def main() -> int32:
    mut grid = Array[int64].zeros([2, 2])
    row = grid[0, :]
    grid[0, 0] = 5
    print(row)
    print(grid[0, :])
    return 0
```

It prints `[0, 0]` for `row` and `[5, 0]` for the row read after the write.

That copy is the one cost to plan for. A loop that keeps a view of an Array
alive while it writes the Array copies the Array on the first write after
each view is taken. Call `clone()` on the view to keep its elements without
the buffer:

```aura
def swap_rows(grid: mut Array[float64], first: int64, second: int64) -> None:
    saved = grid[first, :].clone()
    grid[first, :] = grid[second, :]
    grid[second, :] = saved
```

### Arithmetic

- Array/Array operations broadcast. The shapes are aligned at the last
  axis. Two dimensions match when they are equal or one of them is one,
  and a missing leading dimension counts as one. The result has the larger
  dimension along each axis.
- A fault reports the row-major index of the result element it belongs
  to.
- `array[selection] op= value` gives what
  `array[selection] = array[selection] op value` gives, with the selection
  evaluated once. Nothing is written when the operation faults.
- Scalar forms apply the scalar to every element in row-major order.
- Results own fresh storage.
- Floating `/` follows the ordinary floating operator rules.
- Integer `+`, `-`, `*`, and `sum` keep checked overflow.
- Wrapping operations use fixed-width two's-complement modular arithmetic.
- Saturating operations clamp at the declared integer width.

### Reductions

`map`, reductions, `fill`, and elementwise kernels walk storage in row-major
order.

- `sum()` of an empty Array returns the dtype's zero.
- `min()`, `max()`, and `mean()` require at least one element.
- Floating reductions visit elements from left to right with deterministic
  dtype rounding, and they propagate NaN.
- `mean()` accumulates and returns a `float64` result for every source
  dtype.

`sum` and `mean` promise no reassociation and no vectorized reduction
order. A float `min` treats `-0.0` as below `0.0`, and `max` treats `0.0` as
above `-0.0`.

### Axis Reductions

A reduction with an axis reduces each lane along that axis and returns an
Array with the axis removed. A rank-1 Array gives an Array of shape `[1]`.

- A negative axis counts from the last. An axis outside the rank is
  `AU4007`.
- Each result element is the reduction of one lane: `m.sum(0)[j]` equals
  `m[:, j].sum()`, with the same bits and the same faults.
- An empty lane follows the reduction without an axis. Its `sum` is zero,
  and its `min`, `max`, or `mean` is `AU4007`.

```aura
def main() -> int32:
    grid = Array[float64].from_list([1.0, 2.0, 3.0, 4.0, 5.0, 6.0], [2, 3])
    print(grid.sum(0))
    print(grid.mean(1))
    print(grid.max(-1))
    return 0
```

### Elementwise Members

Each member gives what `map` with the scalar function gives, in one call.

- `abs()` of an integer minimum is `AU4002`.
- `minimum` and `maximum` order floats as `min` and `max` do: a pair with a
  NaN gives NaN, and `-0.0` is below `0.0`.
- `sqrt()` of a negative element is NaN, as scalar `sqrt` is.
- `exp`, `log`, `log2`, `log10`, `sin`, `cos`, `tan`, and `pow` follow the
  [`math`](/manual/math) function of the same name, with its domain and
  overflow rules.
- `floor`, `ceil`, and `trunc` give `int64` elements. An element that does
  not fit is `AU4002`.
- A fault carries the scalar function's code and message, followed by
  `at flat index N` for the first failing element in row-major order.

### Conversion Semantics

`array as Array[U]` converts each element as the scalar cast does:

- An integer must fit the target integer type.
- An integer to a float must be exactly representable.
- A float to an integer must be finite and in range, and is truncated
  toward zero.
- `float64` to `float32` rounds to nearest.

The first element that fails reports the scalar cast's diagnostic with its
flat index. A cast to the Array's own dtype shares its buffer.
`to_float()` rounds each integer to the nearest `float64` and never faults.

### Comparisons And Masks

A comparison member compares each element with the matching element of an
Array that broadcasts, or with one element, and returns `Array[bool]`. A
float comparison with a NaN is `false`, except `not_equal`, which is
`true`.

`Array[bool]` has the constructors, selections, shape members, slice
assignment, `clone`, `get`, `set`, `fill`, and `map` of every Array.

- `any()` and `all()` of an empty mask are `false` and `true`.
- `count()` is the number of `true` elements.
- `logical_and` and `logical_or` broadcast as arithmetic does.
- `mask.choose(when_true, when_false)` broadcasts the mask and both
  arguments to one shape. Each result element comes from `when_true` where
  the mask is `true` and from `when_false` elsewhere. Both arguments are
  evaluated in full.

```aura
def main() -> int32:
    values = Array[float64].from_list([-1.5, 2.0, 0.5, -4.0], [4])
    negative = values.less(0.0)
    print(negative.count())
    print(negative.choose(0.0, values))
    print(values.abs().greater(1.0).logical_and(negative))
    return 0
```

### Reordered Reductions

A left-to-right float sum is a chain in which each addition waits for the one
before it, so it adds one element at a time on any CPU. `fast_sum()` and
`fast_mean()` add in another order that a CPU can run many elements wide:

```aura
def main() -> int32:
    values = Array[float64].full([1000000], 0.1)
    print(values.sum())
    print(values.fast_sum())
    return 0
```

The order is fixed, so the result is the same on every CPU and in both
backends. For `n` elements, let `w` be the largest multiple of 32 that is at
most `n`:

1. Element `i < w` is added to partial sum `i % 32`, in index order. Each
   partial sum starts at zero.
2. The 32 partial sums are added in halves. Sum `s` takes sum `s + 16`, then
   `s + 8`, `s + 4`, `s + 2`, and `s + 1`.
3. Elements `w` to `n - 1` are added to that total in index order.

- `fast_sum()` adds in the element type. `fast_mean()` widens each element
  to `float64`, adds in that order, and divides by the count.
- The result can differ from `sum()` and `mean()` in the last digits. Both
  propagate NaN.
- On an integer Array `fast_sum()` is `sum()`: exact and checked.
- `fast_sum()` of an empty Array is zero, and `fast_mean()` requires at
  least one element.

### Dot And Matrix Products

`left.dot(right)` requires two Arrays of the same shape, of any rank, and
returns the sum of the products of matching elements. A float `dot` rounds
each product in the element type and adds the products in the reordered
order above. An integer `dot` is checked: the first product or running sum,
in index order, that leaves the element type's range is `AU4002`.

`left.matmul(right)` accepts three pairs of shapes. The last two
dimensions of an operand are its matrix, and `..` stands for any leading
batch dimensions:

| Left | Right | Result |
| --- | --- | --- |
| `[.., n, k]` | `[.., k, m]` | `[.., n, m]` |
| `[.., n, k]` | `[k]` | `[.., n]` |
| `[k]` | `[.., k, m]` | `[.., m]` |

The batch dimensions of the two operands broadcast like the shapes of an
arithmetic operation. The result holds one matrix product per batch
position. A rank-1 operand is one vector for every position (ADR-0090).

```aura
def main() -> int32:
    left = Array[float64].from_list([1.0, 2.0, 3.0, 4.0], [2, 2])
    right = Array[float64].from_list([5.0, 6.0, 7.0, 8.0], [2, 2])
    product = left.matmul(right)
    print(product[0, 0])
    print(product[1, 1])
    print(left.dot(right))
    return 0
```

Each result element starts at zero and adds its `k` products in ascending
order of the inner index, rounding each product before it is added. That is
the order of the plain three-loop program, so `matmul` gives that program's
bits. An integer `matmul` is checked and reports `AU4002`.

```aura
def main() -> int32:
    layers = Array[float64].full([3, 2, 2], 2.0)
    inputs = Array[float64].full([2, 4], 1.0)
    outputs = layers.matmul(inputs)
    print(outputs.shape())
    print(outputs.sum())
    return 0
```

`layers` holds three matrices and `inputs` holds one, so `outputs` has
shape `[3, 2, 4]`. Each of the three products is the plain product above.

A large float product of two matrices runs on several threads. Each thread
computes whole rows of the result, and no element is shared, so the bits
are the same on any number of threads. A product whose `n * k * m` is under
about two million runs on the calling thread. `AURA_ARRAY_THREADS` sets the
most threads one product may use. The default is the number of CPUs the
process may run on, a product never uses more than 16, and
`AURA_ARRAY_THREADS=1` keeps every product on the calling thread.

## Ownership And Evaluation Order

`Array[T]` is non-Copy and cloneable. Assignment and owned argument passing
move the Array. `.clone()` is the explicit way to duplicate it.

An Array is always structurally `Transfer`, because every dtype is a
`Transfer` scalar. A Task result that holds an Array still has the ordinary
single-consumer observation right. Bare parameters and receivers give shared
access. `set` and `fill` require exclusive mutable access.

Evaluation order:

- Constructors evaluate their arguments once, from left to right.
- A binary operation evaluates the left operand, then the right. It keeps both
  Arrays for the kernel and consumes neither shared operand.
- Coordinates evaluate from left to right.
- A direct indexed assignment captures its coordinate before it evaluates the
  new value. A slice assignment evaluates its bounds and steps from left to
  right, then its value.

Elementwise operations and `map` allocate a fresh result. `map` calls its
repeatable callback once per element in row-major order, and moves or copies
each scalar result into the output. If a trap occurs, any partial output is
cleaned up. A shape snapshot is an owned copy.

## Diagnostics

Compile-time codes:

| Code | Cause |
| --- | --- |
| `AU2001` | Unknown Array member or constructor. |
| `AU2002` | Unsupported dtype, mismatched argument, callback, or result type, or mixed dtypes. A slice assignment whose value is neither `T` nor `Array[T]`. A slice bound or step that is not an integer. |
| `AU2003` | Unsupported operator, including integer Array `/` and arithmetic on `Array[bool]`. A member called on a dtype it does not have. An Array cast to or from `Array[bool]`. |
| `AU2004` | Invalid argument binding. |
| `AU3002` | Mutation while shared access is active, or a slice assignment that moves its own target, as in `values[:] = values`. |
| `AU3003` | `set`, `fill`, or assignment through an immutable place. |

Runtime codes:

| Code | Cause |
| --- | --- |
| `AU4001` | A math member's domain error, such as `log` of a negative element, or a cast of a non-finite float. |
| `AU4002` | Checked integer Array arithmetic, `dot`, or `matmul` overflows. `abs` of an integer minimum, `exp` overflow, a `floor`, `ceil`, or `trunc` result outside `int64`, or a cast whose element does not fit or is not exact. |
| `AU4003` | A coordinate is out of range, a slice bound is outside its axis, a slice range is reversed, or a slice step is zero. `get` returns `None` instead, but method `set` traps. |
| `AU4004` | Floating Array division with any zero divisor. |
| `AU4005` | Shape product or element count overflows, or allocation fails. A write to an Array that shares its buffer allocates the copy. |
| `AU4007` | Numeric array shape or reduction violation. See below. |

`AU4007` (`numeric array shape or reduction violation`) covers:

- construction with rank zero or a negative dimension
- a `from_list` element count that does not match the shape
- an Array/Array operation, comparison, or `choose` whose shapes do not
  broadcast
- a reduction axis outside the rank
- a direct coordinate count that does not match the runtime rank
- an index with more components than the runtime rank
- a `reshape` to a shape with a different element count, or with two `-1`
  dimensions
- a `permute` that repeats or omits an axis
- a slice assignment whose value does not broadcast to the selected shape
- `min`, `max`, `mean`, or `fast_mean` on an empty Array or an empty lane
- a `dot` whose shapes differ
- a `matmul` whose shapes are not one of the three accepted pairs, or whose batch dimensions do not broadcast

These traps are defined language behavior. A backend must not panic in their
place.

## Backend Support

MIR and direct execution both implement:

- constructors, indexing, and mutation
- selections, shape members, and slice assignment
- `map` and reductions
- `dot` and `matmul`
- checked, wrapping, and saturating arithmetic
- scalar forms and broadcast elementwise operations
- axis reductions, elementwise members, conversion, comparisons, and masks

Direct native execution uses kernels specialized per dtype. Arithmetic
reads a strided or broadcast operand where it lies. Both
backends share checked types, evaluation order, row-major results, cleanup,
and exact `AU4003` and `AU4007` behavior.

## Element Access, Foreign Calls, And Tasks

A built program reads `grid[i, j]` where the element lies, with one range
check per coordinate and no runtime call. This holds for an index of one to
four integer coordinates on an `int32`, `int64`, `float32`, or `float64`
Array that is one row-major block. An element write, `grid[i, j] = value`
or `grid[i, j] += value`, also runs in place when no other Array shares the
buffer. Any other access gives the same result through the runtime: a
strided view such as a transposed matrix, a mask element, an index of five
or more coordinates, and the first write after a `clone()` or a slice, which
copies the buffer (ADR-0091).

```aura
def main() -> int32:
    mut grid = Array[float64].zeros([3, 4])
    for row in range(3):
        for column in range(4):
            grid[row, column] = (row * 4 + column) as float64
    grid[1, 1] += 0.5
    print(grid[1, 1])
    print(grid[-1, -1])
    return 0
```

A loop whose only uses of an Array are element reads and writes checks the
Array once at its entry and then reaches each element by address. Keep other
members out of the loop to get that form, apart from `len()`: read
`grid.shape` or a reduction before the loop, not inside it.

An `extern "C"` function takes an Array as a pointer to its first element.
See [FFI](ffi.md#arrays). The `simd` lane classes load from a rank-1 Array
with `load_array` and write with `store_array`. See [SIMD](simd.md).

An Array moved into a task or sent through a queue takes its buffer along.
`clone()` shares the buffer, so tasks that each receive a clone read the
same elements, and no elements are copied. A task that writes its clone
gets a private copy at the first write.

```aura
def total(values: Array[float64]) -> float64:
    return values.sum()

def main() -> int32:
    values = Array[float64].full([1000], 0.5)
    with TaskGroup() as group:
        first = group.start(total, values.clone())
        second = group.start(total, values.clone())
        print(first.result_or(0.0, timeout=1s) + second.result_or(0.0, timeout=1s))
    return 0
```

The compiler's analysis and the language server expose the same constructors,
member signatures, result types, hover, definitions, completions, and
diagnostics. The bundled VS Code extension uses that compiler-owned surface.

## Limits And Implementation-Defined Behavior

Aura 0.15 Arrays are CPU-only, row-major, and have rank one or more. They do
not have:

- mixed promotion between dtypes
- equality or ordering operators: use the comparison members
- math members on `float32`: convert with `as Array[float64]` first
- iteration with `for`
- a row selected by one coordinate: write `grid[0, :]`, not `grid[0]`
- autograd, device placement, or distributed storage
- a foreign function that keeps an Array's pointer after its call

Shape metadata is dynamic, and the checker does not prove that shapes are
compatible. Allocation is limited by host memory and the element-count checks.
Floating arithmetic follows the host IEEE-754 rules. This API is narrower than
NumPy's.

## Measured Performance

These measurements were taken on a quiet host, not after a reboot. They have
not yet been repeated under the full post-reboot protocol.

Both runs are contractual under the benchmark runner's qualification rules:

- clean detached sources
- 11 rotating pairs, with warmups excluded
- exact protocol and checksum validation
- three empty host inventories
- successful input and hash rechecks

No override was used.

| Setting | Value |
| --- | --- |
| Host | Mac14,9 / Apple M2 Pro / 16 GiB |
| Python | Xcode CPython 3.9.6 |
| NumPy | 2.0.2 |
| Rust | 1.95.0 |
| Threads | single-threaded |
| Unit | time per operation; ratios are ratios of medians |

"Before" is merge base `a368dce7e7b335c1c0cb800ba5240501a21f02bc`. "After" is
implementation head `d9fc79921ba9fed1116634ec9994bcb599aa9f90`. Each clean
checkout ran its own unchanged runner in the same session, after the full
local gates. Later documentation edits do not change compiler or runtime
sources.

| Workload (one million `float64` elements) | Aura before | Aura after | Change in time | Before Aura / Rust | After Aura / Rust |
| --- | ---: | ---: | ---: | ---: | ---: |
| Fresh owned addition | 1.244818 ms | 0.249473 ms | -79.9591% | 5.039359 | 1.006524 |
| Existing-array sum | 1.149271 ms | 1.149844 ms | +0.0498% | 1.338990 | 1.339087 |

| Control | Before median | After median | Drift |
| --- | ---: | ---: | ---: |
| NumPy add | 0.253398 ms | 0.247745 ms | -2.2310% |
| Rust add | 0.247019 ms | 0.247856 ms | +0.3387% |
| NumPy sum | 0.173397 ms | 0.172073 ms | -0.7639% |
| Rust sum | 0.858312 ms | 0.858677 ms | +0.0425% |

Addition meets the 1.5x Rust target at 1.006524x and takes 79.9591% less Aura
time. Its after Aura/NumPy ratio is 1.006974.

The sum ratio against NumPy is 6.682308. Most of that gap comes from the
deterministic reduction-order policy. Under the same left-to-right order, Aura
stays within 1.34x of Rust.

Addition allocates a fresh owned result, and sum reuses its input. These
workloads do not rank Aura or the Array API in general.

The [before raw](https://github.com/johnolafenwa/Aura/blob/main/work/2026-09-08-pre-batch-1-items-6-7/arrays-before-raw.json)
data, [after raw](https://github.com/johnolafenwa/Aura/blob/main/work/2026-09-08-pre-batch-1-items-6-7/arrays-after-raw.json)
data, [comparison and controls](https://github.com/johnolafenwa/Aura/blob/main/work/2026-09-08-pre-batch-1-items-6-7/array-publication-comparison.json),
and [SHA256SUMS](https://github.com/johnolafenwa/Aura/blob/main/work/2026-09-08-pre-batch-1-items-6-7/SHA256SUMS)
keep every observation and the identities of every source and binary.

## Kernel Implementation

The shared runtime that MIR and direct execution use picks each floating-point
operation before entering its slice loop. Release arm64 disassembly contains
`fadd.4s` and `fadd.2d`, plus the matching `fsub`, `fmul`, and `fdiv` vector
forms, including scalar broadcasts. Division checks for the first zero
divisor after the fallible output allocation. Integer arithmetic and
sequential reductions keep their existing implementations.

Both runtime paths match 1,008 frozen output-bit and diagnostic cases,
recorded before the kernels were written, in debug and optimized release
builds. The cases cover:

- all four element types
- empty and vector-boundary lengths, and 1,000,001 elements
- NaN payloads, infinities, and signed zero
- arithmetic modes and first-trap indices

The elementwise kernels do not reassociate, do not approximate division, and
add no new numeric policy.

### Kernel Width

On x86-64 the runtime holds two copies of each kernel: one for the baseline
instruction set, with 128-bit registers, and one for AVX2, with 256-bit
registers. The first Array operation of a process asks the CPU which it has,
so a program built for any x86-64 machine runs the wide copies where they
exist. On arm64 there is one copy, which uses NEON.

The copies give the same bits. No kernel uses fused multiply-add, and no
kernel lets the register width decide the order of its floating-point
operations.

With AVX2 a float `matmul` of two matrices copies a block of the right
operand into a packed buffer and computes two rows by sixteen `float64`
columns of the result at a time, which keeps the running sums in
registers. The additions for each element stay in the same order.

Set `AURA_ARRAY_KERNELS=baseline` to make a process use the baseline copies,
to compare the two or to rule the wide ones out. `--target-cpu` in
[CLI and Tooling](/manual/cli-and-tooling) is the matching choice for the
program's own compiled code.

## Status

Contiguous numeric Arrays and explicit scalar and Array integer arithmetic
modes are part of Aura 0.3. The contract is exactly the surface on this page.
It does not include tensor placement, views, shape transformations, or
distributed execution.

Design records:
[ADR-0041: Contiguous numeric arrays and explicit integer arithmetic modes](https://github.com/johnolafenwa/Aura/blob/main/architecture_docs/decisions/0041-contiguous-numeric-arrays.md),
[ADR-0085: Target CPU and kernel dispatch](https://github.com/johnolafenwa/Aura/blob/main/architecture_docs/decisions/0085-target-cpu-and-kernel-dispatch.md),
[ADR-0086: Array products and reordered reductions](https://github.com/johnolafenwa/Aura/blob/main/architecture_docs/decisions/0086-array-products-and-reordered-reductions.md),
[ADR-0088: Array views share storage and copy on write](https://github.com/johnolafenwa/Aura/blob/main/architecture_docs/decisions/0088-array-views-share-storage.md),
and [ADR-0002: Integer division and modulo](https://github.com/johnolafenwa/Aura/blob/main/architecture_docs/decisions/0002-integer-division-and-modulo.md).
