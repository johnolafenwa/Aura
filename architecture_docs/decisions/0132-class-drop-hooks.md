# ADR-0132: An Owner Runs Its Class Drop Hook

- Status: Accepted
- Date: 2026-11-24

## Decision

A class can define `def __drop__(mut self)`. When the last source owner
releases an initialized instance, the hook runs once with exclusive
access to its fields. The runtime releases the fields after the call.
Locals leave each lexical scope in reverse declaration order.

The hook takes no ordinary parameters or type parameters and returns
`None`. Its caller is implicit, so it cannot impose a `requires` clause.
Users can verify the hook's body and postconditions. A copy class cannot
declare one. A class with a hook must keep its fields initialized until
the hook has run. A whole-value move transfers the obligation.

Overwriting an owner releases its old value once the replacement is
ready. Removal from a collection transfers ownership when the operation
returns an element. Discarding that result, clearing the collection, or
releasing an enclosing class releases its owned elements. An explicit
collection copy creates a separate cleanup obligation for its elements.

A class with a hook follows the existing structural Transfer rules. A
task receiving a moved value runs its hook when it releases the value.
Returning the value sends that obligation to its next owner.

Trapping inside a hook still releases its fields and the remaining
owners. The first trap is the one reported by the task or
`control.recover`. The runtime disarms a hook before entering its body.

## Implementation

The fully initialized instance carries a shared obligation, using the
same distinction between source ownership and temporary runtime copies
as opaque FFI handles. The last internal reference queues the disarmed
instance for its backend to call. Generated code can skip this machinery
for programs without hooks. A lexical marker keeps an owner live through
its scope, including when the last source read is earlier.

[Opaque handle release](0083-ffi-handle-drop-functions.md)
