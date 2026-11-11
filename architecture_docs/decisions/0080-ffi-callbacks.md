# ADR-0080: FFI passes a named function as a C callback

- Status: Accepted
- Date: 2026-11-11
- Builds on: ADR-0074 (direct calls), ADR-0075 (out-parameters), ADR-0077
  (structs)

## Context

Many C interfaces call back into their caller: `qsort` takes a comparison
function, SQLite calls a function for each row, a numeric library
integrates a function it is handed, and most libraries report log lines
through one. Function types in an extern declaration were reserved, so
each of these needed a C adapter, and the adapter could not run Aura code
either.

Three things make a callback harder than a call. C holds only a code
address, with no room for captured values. An Aura failure is a panic,
which must not unwind through C frames. And the interpreter has no machine
code for an Aura function to point C at.

## Decision

- **A function type is a callback parameter.** An extern parameter may be
  `name: def(...) -> R`. The C type is a pointer to a function with those
  parameters and that result.
- **The argument names a top-level function.** It is not generic, and its
  type is exactly the declared one. A lambda, a closure, a method, or a
  function held in a variable is `AU2005`. So a callback captures nothing,
  and its code address is valid for the life of the program.
- **Callback parameters and results are scalars.** `value: T` is a scalar
  by value. `value: mut T` is a pointer to one scalar, read when the
  callback starts and written back when it returns. It is also the
  spelling for `const T *`. A null pointer reads as zero and is not
  written. The result is a scalar or `None`. Any other type is `AU2002`.
- **A `bool` from C is true when nonzero.** A callback cannot refuse its
  arguments the way a call can refuse a result, so the value is
  normalized.
- **A failure waits for the C function to return.** The callback catches
  its own failure, parks it for the thread, and returns zero to C. A
  callback that starts while a failure is parked returns zero without
  running. When the foreign call returns, after its out-parameters are
  written back, it fails with the parked diagnostic.
- **Built programs emit two functions for each callback and signature.**
  The entry has the C signature. It widens the arguments into a frame of
  words on its stack and calls a runtime guard with the frame and the body.
  The body calls the Aura function and stores its results in the frame.
  The guard is the only frame that catches a panic. A module that passes a
  callback checks for a parked failure after each of its foreign calls.
- **The interpreter uses libffi closures.** One closure is made for each
  function and signature and kept for the life of the process. Its handler
  runs the function in the interpreter that made the foreign call.
- **C must call on the calling thread.** A callback runs on the thread that
  made the foreign call, during that call or a later foreign call. A call
  from a thread the C library created is undefined behavior. A callback
  should not suspend.

## Alternatives rejected

- **Closures as callbacks.** A closure needs a context pointer. C
  interfaces carry one in a `void *user_data` argument, in a different
  position for each library, and the context must outlive every call C
  makes, which the checker cannot see. A named function and a `uint64`
  user-data argument cover the same libraries with no lifetime question.
- **Unwind through C.** C code holds locks and half-built state across a
  callback. Unwinding past it leaks both, and is undefined behavior for
  code built without unwind tables.
- **Abort on a callback failure.** It is simple, but a failed `expect` in a
  comparison function would end the process with no diagnostic that tests
  or a supervisor could catch.
- **Text, lists, structs, and handles in callbacks.** Each needs an
  ownership rule for memory that C owns for the length of one callback.
  Scalars and scalar pointers need none.

## Consequences

- `qsort`, row and progress callbacks, and numeric drivers are bound
  without a C adapter.
- A direct call into a callback costs about 4 ns more than a C function
  pointer, for the frame and the guard.
- After a failure, C still runs to the end of its function with zeros for
  the remaining callback results.
- Callback code addresses in the interpreter are never freed. There is one
  for each function and signature a program passes.
- A callback from another thread is not detected.
