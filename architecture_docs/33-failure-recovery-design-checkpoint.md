# Failure Recovery Design Checkpoint

Status: **Accepted 2026-11-16 as ADR-0104 and ADR-0105.** The owner chose
B1 on 2026-11-16. The other choices proceeded on the defaults below.

A trap used to end the task it happened in with no way to continue in
place, and both backends stopped a program at 256 nested calls. This
checkpoint settles how a program recovers from a trap and what bounds
call depth.

## Starting Point

- A trap is a diagnostic raised by the runtime: a failed `assert`, an
  index out of bounds, an integer overflow, a division by zero.
- A task already contains a trap. `task.result()` returns
  `TaskResult.Error(message)`, the task's `with` exits run, and its
  ownership ledger frees what it owned. Measured cost: 6 microseconds per
  successful call natively, 21 per trap, and 1.6 milliseconds under
  `aura run`.
- Native frames cannot be unwound. A failed task's machine stack is
  dropped whole, which is sound because every owned value is in the
  task's ledger and every `with` exit is on its cleanup stack.
- The 256 limit was a count in the frame stack (native) and in the
  interpreter. The interpreter also probed the remaining machine stack.

## A. What Can Be Recovered

- **A1. Every trap (default).** One rule: whatever would have ended the
  task with a diagnostic becomes the error value.
- A2. A chosen subset, such as assertions only. Rejected: a parser that
  must survive hostile input cares most about the traps nobody wrote.
- Cancellation and `sys.exit` are not traps under either choice. They
  pass through.

## B. The Surface

- **B1. A boundary function, `control.recover(function, arguments...)`
  returning `Result[T, control.Trap]` (owner's choice).**
- B2. Exceptions with `raise`, `try`/`except`. Rejected by the owner.
  Every call becomes a possible exit, and native code needs unwind
  information for every frame that owns a value.
- B3. Both. Rejected with B2.
- B4. A `recover:` block statement. Rejected: a block shares locals with
  the code around it, so a trap could leave them half-written.

## C. What The Boundary Lets Through

- **C1. The rules of `TaskGroup.start` (default).** Arguments and the
  return value are `Transfer`, non-Copy arguments move in, no `mut`
  parameters. The checker shares the code.
- C2. Any call. Rejected: a `mut` argument or a captured view could be
  left half-written by the failed call.

## D. What The Program Learns

- **D1. A `control.Trap` object with `code`, `message`, `path`, `line`,
  `column`, `frames` and `describe()` (default).**
- D2. The message only, as `TaskResult.Error` gives. Rejected: tests
  want to assert on the code, hosts want to log the position.

## E. How It Runs Natively

- **E1. A ledger of its own for the call, and a jump back to the caller
  (default).** For a named function the call runs on the caller's stack
  and a trap returns through `_longjmp`. Other targets run as a
  coroutine on a pooled stack. ADR-0104 has the detail.
- E2. A real task per call. Rejected on cost.
- E3. Unwind tables. Rejected: a cost in every function, for a rare
  event.

## F. Call Depth

- **F1. No count. A stack check at the entry of functions that can call
  themselves, and larger first-task stacks (default).** ADR-0105.
- F2. A larger or configurable count. Rejected: any count is wrong for
  some frame size.
- F3. Growing stacks. Rejected: generated code holds stack addresses.

## Order Of Work

1. Call depth on both backends, with the diagnostic and shortened call
   chains.
2. `control.recover` in the checker, the interpreter and the general
   native path.
3. The direct native path.
4. Verification, freestanding rejection, editor support, manual, book,
   benchmarks.
