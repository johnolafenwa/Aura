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

## Runtime And Editor

Two runtime regressions failed on the original implementation. A rejected
release could make a later task failure leave a different mutex locked.
Also, copies of an atomic handle sent to child tasks shared a wrapper
lock in addition to their atomic word.

Each task tracks the cells it holds and validates ownership before
removing a cell. A new runtime boundary can poison only cells acquired
since the recorded acquisition epoch. A unit regression covers releasing
an outer lock before acquiring an inner one. Child tasks receive independent wrappers around
the same synchronized state. Native collection locks can transfer the
unique box between critical sections. ADR-0103 gives the storage and
waiting rules.

The editor regression covers module and method completion, the type of
the lock binding and its definition in the example. The CLI test compares
the example's five output lines across both backends. The verifier regression passed. Atomic reads and successive lock blocks
each see fresh values.

The first Linux verify job found that the source identity checker
misclassified the declaration of the synchronized map as a builtin type
use. The checker now recognizes copy-class declarations. All ten identity
tests passed, including checks for unqualified collection names.

## Verification In Progress

The full comparative benchmark checks each phase's result across Aura,
Rust and Python. Runtime and compiler testing, the cross-backend CLI and
editor checks, and hosted CI are in progress.

## Early Exits And Output

A fixture that propagates a typed error while holding a mutex reproduced
a retained lock on the interpreter. The error path now releases its
managers in reverse order and preserves the original error when a
fallible manager also returns an error. The native ownership pass treats
a propagating `try` before an unreachable terminator as a possible return.

The runtime tests pass for copied handles, transfer of a collection box
and out-of-order release across a recovery boundary. The language server
passed all 159 tests. The two backends previously rendered a mutex
differently. The manual and a fixture specify `<sync-handle>` for direct
and nested rendering.

## Local Names

The Windows compiler fixture suite found five existing programs defining
their own `Cell` enum that failed MIR validation. Linux reproduced the
same failure with either `Cell` or `Atom` as a local enum. The shared
state module's two host handles now keep their qualified runtime names.
The regression declares both enums and calls the imported atomic counter.

## Lock Cost

A preliminary native run identified repeated wrapper tag changes in
`mutex.lock()` and copies of a general runtime value around each integer
step. Lowering now retains the original cell for the block. A custom
factory still runs once, and rebinding the source variable in the body
still releases the original mutex. Native integer and boolean steps read
and write the word in its guarded slot; tests cross between native word,
boxed, and interpreter accesses to the same cell.
