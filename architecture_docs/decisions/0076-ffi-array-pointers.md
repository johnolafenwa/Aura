# ADR-0076: FFI passes a list of scalars as an array pointer

- Status: Accepted
- Date: 2026-11-10
- Builds on: ADR-0074 (direct calls), ADR-0075 (`cstr` and out-parameters)

## Context

After ADR-0075 a foreign function can take one scalar through a pointer, and
bytes with a length through `list[uint8]`. Numeric libraries take neither.
BLAS, image codecs, and model runtimes take `const double *` or `float *`
with the count in an argument of its own, in whatever position and type the
function chose. A `list[float64]` could not reach them without a C adapter
that copied the elements.

A list of fixed-width scalars already keeps its elements unboxed and side by
side, which is the layout of a C array.

## Decision

- **`ptr[T]` is a pointer to the first element of a `list[T]`.** It is a
  type name only in an extern parameter. The parser rewrites it to `list[T]`
  and records where it was, as it does for `cstr`, so a call takes an
  ordinary list.
- **`T` is a fixed-width scalar.** That is every type of the scalar table,
  `bool` included. Anything else is `AU2002`.
- **No length crosses.** The declaration states the count the way the C
  function takes it, as an ordinary argument.
- **`ptr[T]` reads and `mut ptr[T]` writes.** `mut ptr[T]` needs an
  exclusive mutable list, and what the function writes is in the list when
  the call returns. The list keeps its length. `own ptr[T]` is `AU3004`.
- **An empty list passes a null pointer**, as an empty view does.
- **A built program lends the list's own storage.** The call reads the
  element address from the list in line. A list that keeps its elements
  another way is moved to the scalar form first, in the runtime, and a
  shared list is copied before a function may write through it.
- **The interpreter copies.** It packs the elements into a C array for the
  call and unpacks a `mut` array afterward, so both backends print the same
  result.
- **A `bool` the function wrote must be 0 or 1.** A `mut ptr[bool]` list is
  checked after the call, and another byte is `AU4001`, as it is for a
  `mut bool`.

## Alternatives rejected

- **Pass every `list[T]` as a pointer and a length**, as `list[uint8]`
  passes. Few numeric C functions take the count next to the pointer or as
  a `size_t`, so most declarations could not use it.
- **A raw pointer type usable anywhere.** It would bring pointer lifetime
  and arithmetic into Aura code. The pointer exists for one call.
- **Copy the list for every call.** A million-element vector would cost a
  million loads to pass. The unboxed list is already a C array.
- **Validate the count against the list.** The compiler cannot know which
  argument, if any, is the count. The contract is the declaration's, as the
  rest of the C signature is.

## Consequences

- BLAS-style functions can be declared without an adapter.
- A `ptr[T]` argument costs a few loads, whatever the length of the list.
- A function that reads or writes past the end of the list has undefined
  behavior. So does one that writes through a `ptr[T]` declared without
  `mut`. The manual says so.
- `Array[T]` does not cross as a pointer yet.
- Pointers to structs are ADR-0077.
