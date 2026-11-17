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

## Ownership and access

Insertion moves a value into storage. A pop or removal transfers ownership
back to the caller. Non-consuming reads return owned copies through
`Lookup`. A deque subscript has the same behavior as `at(index)`. A map or
set range is a half-open snapshot. Consuming iteration moves values out in
key order, including values that cannot be cloned.
