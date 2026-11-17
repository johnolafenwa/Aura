# ADR-0117 Ordered collections

Status: Proposed for integration.

## Decision

The `collections` source module provides a double-ended queue, a binary
heap and two key-ordered containers. A deque balances two stacks whenever
a pop finds one side empty. Moving half the elements at a transfer gives
amortized constant time for pushes and pops, including alternating ends.

A heap stores elements in a list. Construction from a list sifts down
from the last parent, giving linear construction time. Popping a min-heap
produces increasing order. The max-heap constructor reverses comparisons.

The sorted map is a B-tree of minimum degree six. Each non-root node holds
five to eleven keys. Insertion splits full children before descending.
Removal moves a predecessor into a vacated internal entry, then borrows
from a sibling or merges siblings to restore occupancy. A sorted set
stores its members as map keys with a unit payload. All comparisons use
the key's `Ord` implementation. User classes can supply that order.

## Runtime storage

The source benchmark missed the performance targets. The standard module
uses runtime storage for primitive keys and deque elements. The runtime
holds a `VecDeque`, a Rust min/max binary heap or a `BTreeMap`. Integers
retain their width and signedness, strings use scalar order, and floating
point keys put NaNs last in ascending order as list sorting does.

Cloning a store shares an immutable snapshot. A mutation consumes one
store and takes a private copy when another value still holds the snapshot.
The hidden host functions are visible only to the standard module. Classes
with custom ordering continue to use their Aura `Ord` methods.

For a local collection of `int64`, generated code passes the receiver and
integer arguments to the runtime directly. The deque and heaps keep one
word per element. The B-tree keeps word keys and payloads. Source calls
and direct calls access the same field, including calls through a generic
function or a containing class. A mutable receiver in a projected place
uses the ordinary write-back path.

A fresh local word deque or heap can retain its packed storage address
between pushes and pops. MIR checks every use in the function. The root
must be constructed once, and every use must be a standard push, pop or
release. Copies, captures, loans, arguments, other methods, projections,
constructor loops and debug builds use the guarded receiver path. The
runtime detaches snapshots before opening the backing store and declines
shared receivers. The root owns the allocation until release, including
when the element buffer grows or a task yields.

## Ownership and access

Insertion moves a value into storage. A pop or removal transfers ownership
back to the caller. Non-consuming reads return owned copies through
`Lookup`. A deque subscript has the same behavior as `at(index)`. A map or
set range is a half-open snapshot. Consuming iteration moves values out in
key order, including values that cannot be cloned.
