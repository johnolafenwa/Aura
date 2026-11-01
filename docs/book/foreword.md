# Foreword

Most people who write Python for a living have a story about the bug that
only showed up in production. Maybe a function was handed `None` on a path
nobody tested, or one part of the program was still reading a list while
another part emptied it, or a network call failed, raised an exception
nobody caught, and took a long-running worker down with it at three in the
morning. None of these are exotic. They are what happens when a language lets
you write code quickly and then asks you to find out at run time whether the
pieces fit.

The usual answer is to move to a language with a stricter compiler, and the
usual price is that the code stops looking like the problem. You trade
`for line in lines:` for pages of declarations, and the program that used to
fit on a screen now needs a build system and an hour of reading before a new
colleague can change it. Aura is an attempt to avoid that trade. It keeps the
part of Python that people love, which is code that reads close to how you
would explain it out loud, and it adds a compiler that checks the things
Python leaves to luck.

## What the compiler checks

The first thing Aura checks is types. Every value has a type the compiler
knows before the program runs, and nothing converts implicitly. An `int32`
does not quietly turn into an `int64`, a string does not become a boolean in
an `if`, and an integer that overflows stops the program with a message
instead of wrapping around to a negative number. Most of the time you do not
write the types yourself, because the compiler works them out from the
values, but they are always there and always checked.

The second thing is ownership. Aura has no garbage collector. Every value has
one owner, and when the owner goes away, the value is cleaned up at that
moment, not at some later point when a collector gets round to it. A function
signature says what the function does with each argument: whether it only
reads it, changes it in place, or takes it over. The compiler holds both sides
of every call to that promise. This is what lets Aura free memory, close
files, and shut down network connections at a predictable point without you
having to remember to, and it is what stops one part of a program from
changing data that another part is still using.

The third thing is failure. An operation that can fail in a way the caller
should care about, such as reading a file, parsing a number, or waiting on a
network peer, returns a value that says so in its type. You cannot reach the
number inside a parse result without saying what happens when the text was not
a number. The compiler will not let a failure path go unhandled by accident.
Some failures, like an index past the end of a list, are bugs rather than
conditions to recover from, and those stop the task with a clear diagnostic
pointing at the line.

## What you get for it

Because the compiler knows every type and every owner, it can produce fast
native code. `aura run` gives you the quick edit-and-run loop of a scripting
language, and `aura build` turns the same source into a standalone executable
that runs on a machine where Aura is not installed.

Concurrency is structured. Tasks start inside a task group, and leaving the
group's block waits for every task in it. Aura has no way to spawn a task that
floats free of the code that started it, and the compiler checks that the
data you hand to a task is safe to hand over. The result is concurrent code
you can read top to bottom.

None of this is free. You will write `mut` when a binding changes. You will
meet errors about moved values in your first week, and some of them will feel
fussy until the reason behind them clicks. We think the trade is worth it, and
this book tries to show you why, one real error at a time.

## About this book

The book is written in the style of *The Rust Programming Language*, which
taught a generation of programmers a language with an unusual compiler by
sitting beside them and reading its messages together. We have tried to do
the same for Aura. Each chapter builds working programs, and when something
does not compile, we show you exactly what the compiler said and explain what
it noticed.

Aura is young. The version this book describes is a technical preview, and
some things you would expect from a mature language are still missing. Where
that matters, the book says so plainly and shows the workaround. We hope you
enjoy learning it, and that the programs you write with it keep working long
after you have stopped looking at them.
