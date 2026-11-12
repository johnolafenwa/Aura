# ADR-0088: Array views share storage and copy on write

- Status: Accepted
- Date: 2026-11-11
- Builds on: ADR-0041 (numeric Arrays), ADR-0066 (stepped slices),
  ADR-0086 (Array products)
- Amends: ADR-0041, which makes every Array own a private contiguous
  buffer, lists views, reshape, transpose, step slices, multidimensional
  slices, and slice assignment as non-goals, and has `clone()` and
  first-axis slices copy their elements

## Context

An Array could be sliced along its first axis only, and the slice copied
its rows. A program that wanted a column, every second element, or a
matrix read the other way round wrote a loop over `array[i, j]`. Nothing
changed the shape of an Array, and nothing wrote more than one element
at a time.

NumPy answers all of these with views: a second Array that reads the
first one's memory. A NumPy view is also a second name for that memory,
so a write through either is seen through both. That is the aliasing
Aura's ownership rules exist to prevent. `Span[T]` allows it for lists
under borrow rules: the view locks its source, and it cannot be stored
or returned.

Arrays are passed to kernels, kept in lists, returned from functions,
and sent to tasks. A view that locks its source can do none of that.

## Decision

### An Array is a value, and its buffer is shared

- **An Array stays an owned value.** No Array ever observes a write made
  through another Array. The borrow rules do not change, and no new view
  type appears. A slice, a transpose, and a reshape all have type
  `Array[T]`, and can be stored, returned, and sent like any Array.
- **The element buffer is reference counted and immutable while
  shared.** `clone()`, a selection, `reshape`, `flatten`, `transpose`,
  and `permute` return an Array that reads the same buffer. None of them
  copies an element when the result is one row-major block of the
  buffer.
- **A write copies first when the buffer is shared.** `set`, `fill`,
  indexed assignment, and slice assignment write in place when the Array
  is the only one reading its buffer. Otherwise the Array first copies
  its own elements into a new buffer. The other Arrays keep the old one.
- **A view that is not one row-major block keeps strides.** A column, a
  stepped slice, and a transposed matrix record the position of their
  first element and the distance between neighbours along each axis. A
  stride is negative for a reversed axis. Reductions, products,
  elementwise arithmetic, `map`, and printing read such a view in
  row-major order, so every result is the one a copied Array would give.
- **A layout that is row-major is stored as a plain window.** A slice of
  whole rows, a reshape of a contiguous Array, and the transpose of a
  vector have no strides, so they keep every fast path a fresh Array
  has.

### Selections

- **An index may mix coordinates and slices:** `m[1, :]`, `m[:, 2]`,
  `m[::2, 1:3]`, `m[::-1]`. Each component applies to one axis, in
  order. Axes after the last component are taken whole.
- **The static type follows the syntax.** An index with a slice among
  its components has type `Array[T]`. An index of coordinates only has
  type `T`, and still needs one coordinate per axis at run time.
- **A coordinate removes its axis and a slice keeps it.** `m[1, :]` of a
  `[3, 4]` Array has shape `[4]`. `m[1:2, :]` has shape `[1, 4]`.
- **Slice bounds and steps follow the list rules of ADR-0066.** Bounds
  are `int64`, a negative bound counts from the end once, nothing is
  clamped, and a bound outside the axis, a reversed range, or a zero
  step is `AU4003`. More components than axes is `AU4007`.
- **A slice among several components is an Array form only.** On a list,
  a `str`, or a tuple it is `AU2003`.

### Shape members

| Member | Result |
| --- | --- |
| `reshape(shape: list[int64])` | The same elements in row-major order under `shape`. One dimension may be `-1`, which takes the length the others leave. A different element count is `AU4007`. |
| `flatten()` | `reshape([-1])`. |
| `transpose()` | The axes in reverse order. A rank-1 Array is unchanged. |
| `permute(axes: list[int64])` | The axes in the order `axes` names them. Each axis appears once; a negative axis counts from the last. Anything else is `AU4007`. |

`reshape` and `flatten` share the buffer of a contiguous Array and copy
the elements of a strided view, because a strided layout cannot always
be described under another shape.

### Slice assignment

- **`m[selection] = value`** needs a mutable Array place. `value` is one
  element of type `T`, which fills the selection, or an `Array[T]` whose
  shape broadcasts to the selected shape: equal dimensions, or a
  dimension of one, aligned at the last axis. Any other shape is
  `AU4007`, and any other type is `AU2002`.
- **A numeric literal takes the Array's dtype**, as it does beside an
  Array operand: `m[0, :] = 1` writes `1.0` into an `Array[float64]`.
- **`m[selection] op= value`** is `m[selection] = m[selection] op value`
  with the selection evaluated once.
- **The value is complete before the write starts.** It may read the
  Array it is assigned into: `v[1:] = v[:-1]` shifts the elements as
  they were. Moving the target into its own assignment, as in
  `v[:] = v`, is `AU3002`.
- **The selection and the value's shape are checked before anything is
  written.** A refused assignment leaves the Array unchanged.
- **The assignment owns its value.** A value that is a view of the
  target, as in `m[0, :] = m[1, :]`, gives up its view before the
  write, so the target is written in place. When both sides are whole
  blocks of the buffer, the elements move inside it without a
  temporary. Otherwise the value is copied out first.
- **Slice assignment on a list or `str` stays `AU2005`.** Their slices
  are copies with no place to write back to.

### `clone()`

`clone()` of a whole Array shares its buffer. `clone()` of an Array
that reads part of a larger buffer copies its elements into a buffer of
their own. The result is the same Array either way. The copy is what
lets a program keep a row without keeping the matrix, and without
making the next write to the matrix copy it.

## Alternatives rejected

- **Views that alias, as in NumPy.** A write through one name changes
  another. Every ownership rule Aura has says a reader may rely on a
  value it holds.
- **A borrowed view type, as `Span[T]` is for lists.** It cannot be
  stored, returned, or sent to a task, and kernels would need a second
  set of signatures. Arrays are passed around far more than list
  windows are.
- **Copy on every slice, as before.** It keeps one representation and
  makes a column sum of a large matrix allocate once per column.
- **Strides on every Array.** One representation, and every rank-1
  read then multiplies by a stride and loses the inline element read.
  Only a view that needs strides pays for them.
- **Mutable buffers shared by reference count with a check on write.**
  That is what this design does. The alternative of writing through a
  shared buffer when no element overlaps needs unsynchronized writes to
  memory another task may read.
- **`a[0]` selecting a row of a matrix.** The static type of an index
  of coordinates is the element type, because rank is a run-time fact.
  `a[0, :]` and `a[0:1]` say which one is meant.

## Consequences

- `clone()`, slicing, `reshape`, `flatten`, `transpose`, `permute`, and
  sending an Array to a task cost the same for any element count.
- The first write to an Array that another Array still reads costs one
  copy of the written Array. A loop that keeps a view of a matrix alive
  while it writes the matrix pays that copy once per view taken, so
  such a loop should `clone()` the view.
- A small view keeps its whole buffer alive. `clone()` releases it.
- Two Arrays that print the same are equal for every operation, however
  they are laid out. Layout is not observable.
- An elementwise operation on a strided view first copies the view's
  elements. Broadcasting replaces that copy with a strided walk.
- ADR-0041's allocation guarantees change: a first-axis slice and
  `clone()` no longer allocate element storage, and an element write may
  allocate, with `AU4005` when the copy cannot be made.
