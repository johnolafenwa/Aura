# ADR-0104: `control.recover` hands a trap back as a value

- Status: Accepted
- Date: 2026-11-16

## Context

A trap ends the task it happens in: an index out of bounds, a failed
`assert`, an integer overflow, a division by zero, an exhausted stack.
Before this decision the only way to survive one was to run the code in a
child task and read `task.result()`. That works, and it is how a server
keeps one bad request from ending the process, but it has three costs:

- It needs a `TaskGroup`, a scheduler round trip and a new task for one
  call. A started task costs about 6 microseconds natively and 1.6
  milliseconds under `aura run`.
- `TaskResult.Error` carries only the message. The code, the position and
  the call chain are lost.
- Nothing in the book or the manual presents it as the way to recover, so
  the language reads as if a trap cannot be caught at all.

Programs need this in a few places: a test that asserts "this traps", a
plug-in host, a request handler, a REPL, a parser that must reject hostile
input instead of dying on it.

## Decision

The builtin `control` module gets one function and one class.

```python
import control

match control.recover(parse, text):
    case Ok(tree):
        use(tree)
    case Err(trap):
        print(trap.describe())
```

`control.recover(function, arguments...)` calls `function(arguments...)`
and waits for it to end. It returns `Result[T, control.Trap]`, where `T`
is the function's return type: `Ok` with the return value, or `Err` with
the trap that ended the call.

`control.Trap` has public fields `code`, `message`, `path`, `line`,
`column` and `frames` (the active calls, innermost first, each written
`function at line:column`), and a method `describe()`.

### What the function may be and may take

The rules are exactly those of `TaskGroup.start`, and the checker uses the
same code for both:

- The target is a named function, a lambda or closure, or a stored
  `TaskCallable`.
- Arguments and the return value are `Transfer`.
- A non-Copy argument moves into the call. A Copy argument is copied.
- A parameter cannot be `mut`. External effects, including writes to a
  file or sends to a queue, survive a trap.

These rules are what make recovery sound. When the call traps, everything
it built is thrown away, and the rules guarantee that the caller holds no
view or half-written value that the call was in the middle of changing.

### What happens on a trap

The call's `with` exits run, innermost first, as they do when a task
fails. Every value the call owned is released. The trap becomes the `Err`
value. The caller's own calls, cleanups and values are untouched.

A cancelled task is not a trap. Cancellation passes through
`control.recover` and ends the task as before, after the call's `with`
exits ran. `sys.exit` is not a trap either.

### Not exceptions

There is no `raise`, no `try`/`except`, and no way to make a trap carry a
program-defined value. Errors a program expects stay `Result` values with
`?`. A trap stays a defect in the program or its input that nobody
planned for. `control.recover` is the boundary at which a program decides
that such a defect ends one unit of work and not the whole task.

## How it runs

**Typing.** The checker reads the call as two calls it already knows,
`control.__aura_caught(control.<raw>(function, arguments...))`, where the
inner one is checked by the code that checks a task start and
`__aura_caught` is a generic signature in `std/control.au`. This is a
typing device only. Neither call exists at run time.

**Lowering.** The call lowers to the MIR task start instruction with
`recover` set and no group, so ownership checking, specialisation of a
generic target and thunk generation are the task start's own.

**The trap value.** The runtime keeps an immutable snapshot of the diagnostic.
The first field access formats its source position and call chain. Copies
share the snapshot until mutation gives the changed instance its own fields.
No Aura code runs between the trap and the `Err`.

**Interpreter.** The function runs in place, in a runtime forked from the
caller's (the same cheap fork a generator body uses). A trap is a Rust
`Err` there, so it simply returns.

**Native.** Generated frames cannot be unwound. A trap ends a task from
where it stands: the runtime drains the task's cleanups, records the
failure and hands the scheduler an exit. `control.recover` stops that
exit. For the length of the call the function gets a frame stack, cleanup
stack and ownership ledger of its own, installed under the caller's task
identity, so that "everything the call owned" is exactly one ledger.

For a small scalar function whose only possible failures are literal
assertions or contract checks, the compiler can return the scalar and an
error directly to a local match. Such a function cannot call out, register
cleanup, loop or acquire resources. A failed check describes the callee
using its source descriptor. The ordinary entry point and every other use
of the function keep their usual behavior.

The other calls use one of these boundaries.

- *Direct call.* When the target is a named function with positional
  arguments on a Unix target, generated code saves a jump buffer in its
  own frame (`_setjmp`), swaps in a pooled child state, and calls the
  function on the same machine stack. On return it moves the result from
  the child's ledger to the caller's and swaps the states back. On a trap
  the runtime drains the child's cleanups, releases its ledger, and jumps
  back to the buffer (`_longjmp`), abandoning the function's frames. The
  abandoned frames hold nothing: every owned value is in the ledger. The
  target is called through its address so that it cannot be inlined into
  the caller, which keeps the two states' frames apart.
- *Own stack.* A closure, a stored `TaskCallable`, a call with named or
  default arguments, and every call on Windows run the function as a
  coroutine on a pooled 16 MiB machine stack, as a generator body runs. A
  scheduler wait inside is forwarded to the caller's context. On a trap
  the exit reaches the driver on the caller's stack, which drops the
  function's stack and releases its state.

A trap that happens inside a generator body or another coroutine that the
function started is that coroutine's exit first: its stack is retired,
and the exit continues outward until it reaches the recovered call.

Measured on x86-64 Linux, per call: see the benchmark table in
`benchmarks/failure_recovery/README.md`.

## Why not other ways

- **Exceptions (`raise`, `try`/`except`).** They would make every call a
  possible exit, which is what `Result` and `?` exist to avoid, and native
  code would need unwind tables and landing pads for every frame that
  owns a value. The benefit over a boundary is catching inside a function
  that shares mutable state with the code that failed, which is exactly
  the case where the state can no longer be trusted.
- **Only `task.result()`.** Kept, and unchanged. It costs a task per call
  and loses the trap's code and position.
- **Unwinding native frames to the boundary.** It needs every generated
  function to register what it owns. The ledger already knows that per
  task, so a state per recovered call gets the same result with no cost
  on calls that never recover.
- **A statement (`recover:` block).** A block shares locals with the
  code around it, so a trap could leave them half-written. A function
  boundary with moved arguments cannot.

## Consequences

- A value passed to the call is gone if the call traps. A caller that
  needs it afterwards passes a copy.
- The call cannot change the caller's variables. It returns what it
  produced.
- A generator cannot be passed in or out, since a generator is not
  `Transfer`.
- `aura verify` treats the call as it treats a task start: the target's
  contracts are checked at the call, and the result is an unknown
  `Result`.
- A freestanding build has no tasks and no ledger, so it rejects
  `control.recover` as it rejects a task start.
- `TaskResult.Error` still carries only a message. Giving it the same
  `Trap` is possible later without changing this decision.
