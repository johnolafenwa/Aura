# Building, debugging, and shipping

Up to now every program in this book has been started with `aura run`. That
is the right command while you are writing code: it checks the program and
runs it in one step, with nothing left behind on disk. But a tool is not
finished until it runs somewhere else, on a server or in a CI job or on a
colleague's laptop, without the compiler and without its source code.

This chapter takes `checkup`, the health checker from
[Chapter 16](/book/ch16-00-talking-to-the-world), the rest of the way. We will
look at the two ways Aura executes a program and why both exist, build a
standalone native binary, and see what goes into it. Then we will debug that
binary: first with the runtime's own diagnostics, which already tell you a
lot, and then with breakpoints and variable inspection in lldb and VS Code.
Finally we will talk about performance honestly: what is fast in Aura today,
what still costs more than it should, and how to measure your own program
instead of guessing.

By the end you will be able to ship an Aura program as a single executable,
find out why it misbehaves when it does, and know which numbers about its
speed you can trust.
