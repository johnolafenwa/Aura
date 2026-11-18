# Recovering From Traps

A trap is a failure nobody planned for: an index out of bounds, a failed
`assert`, an integer overflow, a division by zero, an exhausted call stack.
A trap ends the task unless a recovery call contains it. This page covers the two ways a program
keeps going after one, and the rule that bounds how deeply calls may nest.

Errors a program expects are not traps. They are `Result` values, and `try`
or `?` passes them along. See
[Enums And Pattern Matching](/manual/enums-and-match).

## `control.recover`

```aura
import control

def parse(text: str) -> int64:
    assert len(text) > 0, "empty input"
    return len(text)

def main() -> None:
    match control.recover(parse, ""):
        case Ok(value):
            print(value)
        case Err(trap):
            print(trap.describe())
```

```text
error[AU4001]: empty input (parse.au:4:5)
```

| API | Signature |
| --- | --- |
| `control.recover` | `recover(function, arguments...) -> Result[T, control.Trap]` |
| `control.Trap` | class with public fields `code: str`, `message: str`, `path: str`, `line: int64`, `column: int64`, `frames: list[str]` |
| `Trap.describe` | `describe(self) -> str` |

`control.recover(function, arguments...)` calls `function(arguments...)` and
waits for it to end. `T` is the function's return type. The result is `Ok`
with the return value, or `Err` with the trap that ended the call.

Aura has no exceptions. There is no `raise` and no `try`/`except`, and a trap
cannot carry a value of the program's choosing. `control.recover` is a
boundary: the place where a program decides that a defect ends one unit of
work and not the whole task.

### The Trap Value

| Field | Meaning |
| --- | --- |
| `code` | The diagnostic code, such as `AU4003`. [Diagnostics](/manual/diagnostics) lists them. |
| `message` | The diagnostic's message without the code. |
| `path` | The source file of the innermost call, or `""` when it is not known. |
| `line`, `column` | The position of the operation that trapped, from 1, or 0 when it is not known. |
| `frames` | The calls that were active inside the recovered call, innermost first, each written `function at line:column` with the position of the function's definition. |

`describe()` returns one line: `error[CODE]: message (path:line:column)`.

`frames` never names a call outside the recovered function. A chain deeper
than 256 calls keeps its 128 innermost and 128 outermost calls, with one entry
of the form `... N more calls ...` between them.

If an Aura callback traps during a foreign call, C finishes the call
before an enclosing `control.recover` returns the trap. A callback can
also call `control.recover` itself, handle the trap, and return to C.

### Typing Rules

The function and its arguments follow the rules of `TaskGroup.start`
([Concurrency](/manual/concurrency)), and the checker reports the same
diagnostics for both:

- The function is a named function, a lambda or closure, or a stored
  `TaskCallable`.
- Every argument type and the return type implement `Transfer`.
- A parameter cannot be `mut`.
- Arguments bind by position or by name, and a default fills an omitted one.

`control.recover` takes no explicit type arguments. `recover` is not a
function value: it can be called, and it cannot be stored or passed.

### Ownership

A non-Copy argument moves into the call, exactly as it moves into a started
task. A Copy argument is copied. A closure's captures move with the closure.
A failed call leaves its external effects in place. For example, text it
printed stays on stdout and values sent to a queue stay sent.

When the call traps, everything it owned is released, including the
arguments. A caller that needs a value afterwards passes a copy, such as
`text.clone()` or `items.copy()`.

These rules are what make recovery safe. The caller never holds a view or a
half-written value that the failed call was in the middle of changing.

### Runtime Semantics

On a trap inside the call:

1. The `with` exits registered inside the call run, innermost first, as they
   do when a task fails. An exit that fails adds a note to the trap.
2. Every value the call owned is released.
3. The trap becomes the `Err` value, and the caller continues.

The caller's own calls, `with` scopes and values are untouched.

The call runs to its end before `control.recover` returns. It runs in the
calling task: it starts no task, needs no `TaskGroup`, and blocking
operations inside it suspend the calling task as usual.

The callee observes the calling task's cancellation. A cancellation-aware
operation can return a value such as `QueueReceive.Cancelled`, which the
callee can handle. An actual task exit propagates through the recovery
call. The same applies to `sys.exit`.

A call may itself call `control.recover`. Each recovery returns the traps of
its own call.

A trap inside a task the call started is that task's failure, reported by its
`TaskGroup` under the usual rules. It becomes the recovered call's trap only
when the group's scope exit raises it there.

If a callee traps while holding a `sync.Mutex`, later attempts to acquire
that mutex report `AU4001`. Locks that its caller already held stay valid.
See [Sync Module](./sync.md).

### Tasks Or `control.recover`

A started task already contains a trap: `task.result()` returns
`TaskResult.Error(message)` and the parent continues. Use a task when the
work should run concurrently, needs a timeout, or should be cancellable.
Use `control.recover` when the work should run here and now. It costs far
less than a task, and it reports the code, position and call chain, where
`TaskResult.Error` carries only the message.

### Limits

- A generator cannot be passed in or returned, because a generator is not
  `Transfer`.
- In a built program a named function called with positional arguments runs
  on the caller's stack. A closure, a stored `TaskCallable`, a call with named
  or default arguments, and every call on Windows run on a 16 MiB stack of
  their own. The stack is reserved, and a page costs memory once it is
  touched.
- A freestanding build rejects `control.recover` (`AU2052`).
- `aura verify` proves the function's `requires` at the call and knows
  nothing about the `Result` it returns.

## Call Depth

Stack space bounds nested calls. A
call that would not fit traps:

```text
error[AU4005]: call stack exhausted while calling `walk`
 --> tree.au:1:1
  = note: Aura call chain (innermost first): walk at 1:1 -> walk at 1:1 -> ... 23,480 more calls ... -> walk at 1:1 -> main at 9:1
  = help: the calls are nested too deeply for this task's stack; a task started with `start_with_stack` gets a larger one
```

It is an ordinary trap: `with` exits run, a task that hits it fails alone,
and `control.recover` returns it.

| Where the calls run | Stack | Nested calls of a small function |
| --- | --- | --- |
| The program's first task, built program | 64 MiB | about 2,000,000 |
| The program's first task, `aura run` | 256 MiB | about 20,000 |
| A started task | 768 KiB by default, or the size given to `start_with_stack` | about 20,000 built, about 60 under `aura run` |
| Inside `control.recover` | the caller's stack, or 16 MiB of its own (see Limits above) | the caller's remaining depth, or about 500,000 |

The counts are measurements of a one-argument function on x86-64 Linux, not
guarantees. A call's cost depends on its locals, and under `aura run` on how
the interpreter itself was built. Stacks are reserved, not committed.

A built program checks the stack at the entry of every function that can
call itself, directly or through other functions. Other functions pay
nothing. `aura run` checks at every call.

### Call Chains In Diagnostics

A trap keeps at most the 128 innermost and the 128 outermost active calls.
The rendered chain shows 8 from each end. Structured output
(`--format json`) lists the kept calls in `call_frames` and adds
`call_frames_omitted`, the number of calls left out of the middle, when it is
not zero.

## Diagnostics

| Code | Condition |
| --- | --- |
| `AU2002` | An argument does not match the function's parameter, or the function is not callable. |
| `AU2004` | `control.recover` was given explicit type arguments. |
| `AU3001` | The caller reuses an owned argument after moving it into the call. |
| `AU3002` | The target takes a `mut` parameter. |
| `AU3008` | An argument, capture or return type cannot cross a task boundary. |
| `AU2052` | A freestanding build calls `control.recover`. |
| `AU4005` | A call would not fit on the stack, or the stack for a recovered call cannot be allocated. |

## Backend Support

`control.recover` and the stack rule behave the same under `aura run` and in
built programs. The measured depths differ, as the table above shows.

## Status

See the Unreleased section of the changelog for availability. ADR-0104 records the recovery design and ADR-0105
the call depth rule.
