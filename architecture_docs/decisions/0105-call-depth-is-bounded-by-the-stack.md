# ADR-0105: Nested calls are bounded by the stack, not by a count

- Status: Accepted
- Date: 2026-11-16

## Context

Both backends stopped a program at 256 nested calls with `maximum call
depth exceeded`. The count was a stand-in for "the machine stack is about
to run out", chosen when tasks had small fixed stacks. It ruled out
ordinary recursive code: a recursive-descent parser on deep input, a walk
over a linked list or an unbalanced tree, a flood fill.

The count also did not protect anything. A function with large frames
could exhaust a 768 KiB task stack in fewer than 256 calls, which is why
the interpreter already had a second check on the remaining stack.

## Decision

There is no limit on the number of nested calls. A call that would not
fit on the machine stack traps:

```text
error[AU4005]: call stack exhausted while calling `walk`
help: the calls are nested too deeply for this task's stack; a task started with `start_with_stack` gets a larger one
```

`AU4005` is the existing code for stack capacity. The trap is an ordinary
trap: `with` exits run, a task that hits it fails alone, and
`control.recover` (ADR-0104) returns it.

### Where the check is

**Native.** Only a function on a call cycle can nest without bound, and
the compiler already knows which functions those are
(`mir_recursion::recursive_functions`). Each of them compares the address
of a stack slot with a limit word in the task's frame stack header at
entry. That is one load and one compare on a predicted branch. Functions
off every cycle pay nothing. The runtime's own slow path for pushing a
call frame makes the same check.

The limit is the lowest usable address of the machine stack the task is
running on plus a reserve of 128 KiB, which is the room the trap itself
and the `with` exits need. While the exits drain the reserve drops to
32 KiB so that an exit can make calls. The runtime rewrites the limit
whenever the task's code moves to another machine stack: a task switch, a
generator body starting or stopping, a recovered call.

**Interpreter.** Every call asks the runtime how much of the current
stack is left and traps when less than its reserve remains. This check
existed; the count in front of it is removed.

### Stack sizes

- The program's first task has 64 MiB natively (it was 16 MiB) and
  256 MiB under `aura run`, where one Aura call costs about 11 KiB of
  interpreter stack. Stacks are reserved, not committed: a page costs
  memory once it is touched.
- A started task keeps its 768 KiB default, and `start_with_stack` sets
  another size.
- A function run by `control.recover` gets 16 MiB natively.

On the development machine a one-argument recursive function reaches
about 2,090,000 nested calls natively and about 23,000 under `aura run`
before the trap.

### Call chains in diagnostics

A trap used to record every active call. With a million calls active
that is neither affordable nor readable. A trap keeps the 128 innermost
and the 128 outermost calls and the number left out. The rendered chain
shows 8 calls from each end around `... N more calls ...`. Structured
output gains the field `call_frames_omitted`, present only when it is not
zero.

## Why not other ways

- **A larger count.** Any count is wrong for some frame size, and the
  stack check is needed anyway.
- **Growing stacks (segmented or copied).** Generated code holds
  addresses of stack slots, so a stack cannot move, and segments cost
  every call. Large lazily committed stacks give the same practical
  depth.
- **A guard page and a signal handler.** A fault in generated code
  cannot run the `with` exits or name the function, and the handler
  would need its own stack and per-platform code. The explicit check is
  one compare in recursive functions only.
- **Checking every function.** Functions off every cycle nest to a
  depth the program text bounds, and the reserve covers them.

## Consequences

- A program that relied on the 256 limit to stop runaway recursion now
  stops later, with a different message and code (`AU4005`, was
  `AU4001`).
- Runaway recursion touches the whole stack before it traps, so it
  commits up to the stack size in memory for that moment. The pages go
  back to the pool with the stack.
- Deep recursion under `aura run` is shallower than in a built program.
  The manual says so.
- A freestanding build has no runtime stack bounds and keeps no check.
  Its stack is whatever the embedding provides.
