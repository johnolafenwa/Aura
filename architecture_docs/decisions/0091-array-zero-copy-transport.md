# ADR-0091: Array elements are reached without a copy

- Status: Accepted
- Date: 2026-11-12
- Builds on: ADR-0041 (numeric Arrays), ADR-0076 (`ptr[T]` array
  pointers), ADR-0087 (the `simd` module), ADR-0088 (Array views)
- Amends: ADR-0076, which passes only a `list[T]` as a pointer, and
  ADR-0087, whose lanes load only from a list

## Context

ADR-0088 made an Array a window of a shared buffer. Four paths still
copied the elements or paid a runtime call for each one:

- `grid[i, j]` in compiled code built a tuple of the coordinates and
  called the runtime. So did every element write, including `values[i] =
  x` on a rank-1 Array. A loop over a matrix ran about a hundred times
  slower than the same loop over a list.
- A foreign function could not take an Array. A program copied the Array
  into a list to pass it as `ptr[T]`, and copied the list back.
- `simd` lanes loaded from a list only.
- A program that gave several tasks the same data cloned it for each
  task. Since ADR-0088 that clone shares the buffer, but nothing said so
  or tested it.

## Decision

### Element access by coordinates

An index of one to four integer coordinates on a numeric Array lowers to
a read or a write that passes the coordinates as plain words. No tuple is
built.

A boxed Array that is one row-major block publishes a view: the address
of its first element, its extents, and the address of a word that counts
the holders of its buffer. The view's access code names the dtype and the
rank. Compiled code reads the view and then:

- checks each coordinate against its axis, after adding the extent to a
  negative one
- for a write, checks that the holder count is one
- reads or writes the element where it lies

Every other case calls the runtime with the same words: a strided view,
an Array another thread can reach, a coordinate out of range, a rank that
does not match, or a write to a buffer another Array shares. The runtime
call gives the result or the diagnostic the interpreter gives. A write to
a shared buffer copies the buffer first, as before, and the next write
finds one holder.

The holder count is kept by the buffer, not by the box. Any clone, view,
or window raises it, wherever in the runtime it is made, so a box never
holds a stale claim to be the only holder.

`Array[bool]` elements and an index of more than four coordinates keep
the runtime call.

### An Array is a pointer to a foreign function

An `extern "C"` parameter of type `Array[T]` is a pointer to the first
element, with no length. `T` is `int32`, `int64`, `float32`, or
`float64`. The elements are in row-major order.

| Parameter | C type | The function |
| --- | --- | --- |
| `values: Array[T]` | `const T *` | reads the elements |
| `values: mut Array[T]` | `T *` | reads and writes the elements |

A row-major Array lends its own elements. A strided view is packed into
a temporary block for a read. For a write, the argument first becomes one
row-major block that no other Array shares, so the function writes only
that Array. `own Array[T]` is `AU3004`, `Array[bool]` is `AU2002`, and an
Array cannot be a result.

The interpreter copies the elements in and out. The compiled program
passes the address.

### Lanes load from an Array

Each `simd` lane class has `load_array(values, at)` and
`store_array(values, at)`. They take a rank-1 Array of the lane's element
type and follow the Array index rules. A direct call lowers to the
element accesses above, as `load` and `store` lower to list accesses.

### Tasks share an Array

An Array moved into a task or a queue moves its buffer. `clone()` shares
the buffer, so each task that receives a clone reads the same elements.
The first write through a clone copies the buffer for that clone alone.
The holder count is atomic, so the check is correct on any thread.

## Consequences

- A loop that reads or writes `grid[i, j]` runs without a runtime call
  or an allocation.
- A numeric library takes an Array directly. `cblas_ddot(n, x, 1, y, 1)`
  reads the Array's own memory.
- A box grows by no bytes: the holder address takes what was padding.
- The executable's layout fingerprint covers the new view, so a program
  built for an older runtime stops at startup.
- A loop whose only uses of an Array are element accesses of one rank
  reads the view once, as list loops do. Its copy of the loop
  knows the dtype, the extents, and the elements address, and a loop that
  writes also knows the Array holds its buffer alone. A loop that calls
  another member of the Array, or passes it to a function, reads the view
  at each access.

## Alternatives considered

- **A flag in the box that says the buffer is unshared.** Any runtime
  path that clones an Array would have to clear it in every box that
  holds the buffer. The count in the buffer cannot go stale.
- **Reading `Arc`'s own strong count.** Its position is not part of the
  standard library's contract.
- **Overloading `load` for lists and Arrays.** Aura has no overloading,
  and a second name keeps the list methods as they are.
- **A `ptr[T]` parameter that accepts an Array argument.** The checker
  compares an argument with one declared type. Declaring the parameter
  `Array[T]` says what the binding expects.
