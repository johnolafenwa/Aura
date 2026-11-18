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
an outer lock before acquiring an inner one. Child tasks receive independent
wrappers around the same synchronized state. Native collection locks can transfer the
unique box between critical sections. ADR-0103 gives the storage and
waiting rules.

The editor regression covers module and method completion, the type of
the lock binding and its definition in the example. The CLI test compares
the example's five output lines across both backends. The verifier
regression passed. Atomic reads and successive lock blocks each see fresh
values.

The first Linux verify job found that the source identity checker
misclassified the declaration of the synchronized map as a builtin type
use. The checker now recognizes copy-class declarations. All ten identity
tests passed, including checks for unqualified collection names.

## Verification In Progress

The full comparative benchmark checks each phase's result across Aura,
Rust and Python. Runtime and compiler testing, the cross-backend CLI and
editor checks, and hosted CI are in progress.

## Recovery inside a lock

A recovered direct call and a recovered stored callable each left an inner
mutex locked after an assertion failed. The interpreter and native runtime
now mark the caller's held locks on entry and poison only the locks the
failed call still holds. The regression checks a successful call, both
trapping call paths, and the caller's ability to update and reacquire its lock.

The interpreter and the Linux native executable matched the eight expected
lines of `recover_nested_mutex`. Both also matched `control_recover_cleanup`.

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

The integer dictionary workload also boxed each key for an indexed store.
The native entry now accepts both words, probes the existing index by
integer value, and preserves insertion order and copy-on-write snapshots.
The benchmark checks the final values as well as the number of entries.

A native parity run also exposed a mutex field in a user record being
treated as an inline record. The common layout plan now stores the
handle word in nested positions as well as at the root. The custom
manager fixture exercises the field through both backends.

A comparison on the shared build host found the four-task cache slower
than the counter and dictionary workloads. The original map lookup moved
the dictionary out of its cell and returned it on every hit. It now
copies the matching entry under the lock, then constructs the public
`Lookup` after releasing the cell. The runtime tests cover maps in
interpreter storage and detached native boxes, plus a hit and a miss in
each representation.

## Native Executable Size

Task retirement originally linked the mutex poison and channel wake paths
into programs that used no locks. A task now installs its own cleanup
callback on its first acquisition. With the same Linux x86-64 compiler,
source and linker flags, the stripped hello-world executable shrank from
1,152,776 to 1,140,008 bytes. The native binary size check passed, and
the linked symbols no longer include the held-cell poison routine or
the channel registration and send routines. With the final dictionary
storage change, the same Linux size check passed at 1,144,288 bytes.

## Shared Dictionary Measurements

An earlier release run measured 33.5 ns for an uncontended
integer lock. Across three runs the guarded dictionary took 284 ms
against 78 ms in Rust. The four-task cache took 1,115 ms against 218 ms.
Each run used the same four CPUs and verified the final checksums.

A directly matched integer lookup now copies its payload into the caller
while the lock is held and records presence independently. This removes
both heap boxes from the matched path and keeps the result stable across
a subsequent write. Eager dictionary and set entry buffers now use
strong-only references for copy-on-write. A unique buffer can apply
the next update after one reference-count load. A snapshot still retains
the old entries when the writer detaches.

The final comparison pinned Aura, Rust and CPython to the same four
CPUs and checked all six workloads. The uncontended integer lock took
36.5 ns per acquisition and update. The four-task cache took
1.22 times the Rust time. The full measurements and workload
checksums are in [the benchmark report](../benchmarks/shared_state/README.md).

A dedicated Linux runner tested the combined release at `248ffb7e`.
The four-task mutex took 823 ms in Aura, 288 ms in Rust and 494 ms in
CPython. All three programs reported 4,000,000 increments. The four-task
cache took 876 ms, 348 ms and 30,755 ms respectively.

A second dedicated runner, on `5f111af1`, measured 502 ms in Aura,
104 ms in Rust and 838 ms in Python for the four-task mutex. The shared
atomic counter took 388, 344 and 4,179 ms respectively. The uncontended
atomic counter took 257, 47 and 4,306 ms. The two runner results and
the four pinned CPUs use independent release measurements.

## Waiting for a Contended Mutex

The native reader used to clone its shared handle after the first failed
lock attempt. The clone and its later release updated the reference count
even when the owner released the lock during the spin. The reader now
uses bounded exponential backoff while it borrows the handle. Once
another waiter marks the cell or a failed owner poisons it, the reader
goes directly to the scheduler path.

The cross-backend waiter fixture uses a queue to signal when the owner
holds the mutex. Two other tasks request the same mutex and can proceed
once the owner receives a release signal. It checks all three completions
and the value published by the last release.

The complete runtime suite passed all 1,365 tests with the file-cache
fixture on tmpfs. The native waiter fixture matched its golden output in
five runs each with one, two and four workers. The dedicated combined
release at `1033d521` measured 158 ms for the four-task mutex, against
97 ms for Rust and 822 ms for CPython. On `5f111af1` the corresponding
figures were 502, 104 and 838 ms. Uncontended lock and atomic performance
on the new runner remained above three times the Rust time. The benchmark
report includes every workload and the local CPU and wall comparisons.

## Checked Atomic Addition

Profiling exposed an out-of-line checked-add call and error construction
in the native atomic path. The inlined operation still updates the same
shared word with a sequentially consistent fetch-and-add. Both workers
in the overflow regression observe the wrapped value, including when a
negative delta underflows it. Wrong-handle diagnostics keep their
original function names and codes.

The public loop took 106.0 instructions per increment after the change,
down from 124.0. In the five-round CPU comparison the prior Aura, new
Aura, Rust and CPython figures were 486, 418, 228 and 19,113 ms for
20 million increments. The focused runtime suite passed 26 tests.
A task overflow followed by a task underflow matched the MIR output
and the native output at one, two and four workers.
