# 0141. Fixed-size arrays

## Decision

Use `fixed[T, N]` to store exactly N consecutive elements of T inline.
The length belongs to the type. A literal in a typed destination supplies
each element, and `[value] * N` repeats one evaluation of a Copy value.
Iteration visits the elements in index order. A fixed array is Copy when
its element is Copy.

Use the same length range as C array fields, 1 through 1048576. The
storage size must fit a signed 32-bit byte offset. Zero-length objects
have no portable C array field representation. Indexes start at zero
and must be strictly less than N. A run-time bounds failure carries the
source position. The object writer emits contiguous target-sized elements
with the element's alignment. Length and literal indexes require no
dynamic shape data.

## Rationale

Square brackets match the spelling of Aura's other parameterized types.
A distinct name preserves the FFI contract for existing `array[T, N]`
fields. Such fields currently accept lists and check their length when
crossing into C. A C field of type `fixed[T, N]` stores the same C bytes
and has fixed length on the Aura side too.

The numerical `Array[T]` type carries a shape and can share views of its
storage. Fixed arrays serve records, bounded working storage, and programs
that run without an allocator.
