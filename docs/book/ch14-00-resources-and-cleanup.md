# Resources and cleanup

Programs hold on to things that belong to the outside world. An open file is a
slot in the operating system's file table. A socket is a connection that the
machine on the other end is keeping alive for us. A temporary file sits on disk
until somebody deletes it. Each of these has to be given back, and the hard part
is not remembering to give it back on the happy path. The hard part is giving it
back when the function returns early, when an error propagates out through
`try`, or when a trap stops the task halfway through.

Python answers this with the `with` statement and context managers, and if you
have written `with open(path) as f:` you already know the shape. Aura keeps that
shape and tightens the contract. A `with` block in Aura runs its cleanup exactly
once on every way out of the block, including traps, in an order you can predict
from the source. Because Aura has no garbage collector, there is no "eventually"
in that sentence: cleanup happens at the line where the block ends.

In this chapter we'll first use `with` on files the standard library gives us,
and look closely at when and in what order cleanup runs. Then we'll write our
own resource manager, a scratch file that deletes itself, and push it through
every failure we can think of: an error from `try`, a cleanup that fails, a trap
in the middle of the body, and a trap inside the cleanup itself. By the end you
will be able to read any `with` block and say exactly what runs when it ends,
and why.

The last piece of the chapter points forward. A `TaskGroup`, which we use for
concurrency in [Chapter 15](/book/ch15-00-concurrency), is also a resource
managed by `with`. The rules you learn here are the reason a group of tasks can
promise that none of them outlives the block that started it.
