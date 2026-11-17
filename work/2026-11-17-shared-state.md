# Shared State

The sync module exposes mutexes, atomics and maps to cooperating tasks.
Design checkpoint 31 records the API. ADR-0102 records the handle model.

## Generic Initializers

A class initializer received its unspecialized parameter type during MIR
lowering. Both a local `Container[list[int64]]([])` and the imported
`sync.Mutex[list[int64]]([])` produced a temporary of type
`list[Unknown]` and failed with AU4001. The class type arguments now
specialize the initializer parameters before lowering their arguments.

The compiler regression test failed on the original branch, then passed
with the fix. The run-pass fixture covers an empty list, narrow integer
elements, a named argument, and two successive locks on the same list.

## Remaining Work

Measure uncontended and contended locks, atomics, and map operations on
the same host as the Rust and Python comparisons. Complete the lock
lowering decision, verification tests, editor and book coverage.
