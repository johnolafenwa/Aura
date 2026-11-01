# Error handling

Programs fail. A file is missing, a user types `80a` where a port number should
be, a loop reads one element past the end of a list. Python treats all of these
the same way: something raises an exception, the exception travels up the call
stack until a `try` block catches it, and if nothing catches it the program
prints a traceback and exits. That model is convenient to write, but a reader
looking at a function call cannot tell whether it might raise, or what it might
raise, without reading every function it calls.

Aura splits failures into two kinds and gives each its own mechanism. A failure
that means the program itself is wrong, such as an index out of range or a
broken invariant, is a *trap*: the current task stops at once, cleanup runs,
and the diagnostic points at the line that failed. A failure that a working
program should expect, such as bad input or a missing file, is a value. The
function returns a `Result`, its signature says so, and the compiler makes the
caller decide what to do with it. There is no third route. Aura has no
exceptions, no `raise`, and no way to catch a trap.

In this chapter we'll write a small loader for a settings file made of `key =
value` lines. The first version trusts its input and traps when the input is
wrong. We'll read those traps together, then rebuild the loader around
`Result`, the `try` operator, and an error enum of our own, so every way the
input can be wrong becomes something the caller can report. We finish with the
question you'll face in every program you write: for this particular failure,
should the program trap, or should it return an error?

By the end you will be able to read a trap diagnostic and its call chain, use
`assert` to state what must be true, design an error type for a small module,
propagate errors with `try` without losing information, and choose between the
two mechanisms with reasons you can explain to a reviewer.
