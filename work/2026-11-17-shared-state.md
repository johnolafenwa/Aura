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
since a recorded depth. Child tasks receive independent wrappers around
the same synchronized state. Native collection locks can transfer the
unique box between critical sections. ADR-0103 gives the storage and
waiting rules.

The editor regression covers module and method completion, the type of
the lock binding and its definition in the example. The CLI test compares
the example's five output lines across both backends. Verification checks
that atomic reads and successive lock blocks each see fresh values.

## Verification In Progress

The full comparative benchmark checks each phase's result across Aura,
Rust and Python. Runtime and compiler testing, the cross-backend CLI and
editor checks, and hosted CI are in progress.
