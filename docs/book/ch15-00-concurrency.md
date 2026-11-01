# Structured concurrency

Sooner or later a program needs to do more than one thing at a time. A server
handles many connections. A build tool compiles several files at once. A
crawler waits on dozens of slow network requests and would like to do something
useful while it waits. Concurrency is how a program overlaps that work, and it
has a reputation for producing the worst bugs in software: work that keeps
running after everyone has forgotten it, failures that vanish because nobody
was listening, two pieces of code changing the same data at the same moment.

Aura's concurrency is built to rule out the first two by structure and the
third by the type system. Every task starts inside a `TaskGroup`, and a task
group is a resource managed by `with`, exactly like the files and scratch
managers of [Chapter 14](/book/ch14-00-resources-and-cleanup). When the block
ends, the group waits for its tasks. So a task can't outlive the block that
started it, and a failure in a task always has somewhere to go. Tasks don't
share mutable data at all: they exchange owned values through typed queues, and
the compiler checks that everything crossing from one task to another is safe to
move.

If you've used Python, you have probably met two older models. Threads from the
`threading` module can be started anywhere and run until they finish, and the
code that started them can return long before. `asyncio.create_task` has the
same shape for coroutines. Python 3.11 added `asyncio.TaskGroup`, which works
much like Aura's groups, and we'll compare the two along the way. The
difference is that in Aura the structured form is the only form there is.

Through the chapter we'll grow one program, a word counter that splits its
input between tasks, from a single background task up to a worker pool with a
producer, several workers, a results queue, and a deadline that cancels
everything if the pool gets stuck. By the end you'll be able to read any Aura
concurrent program and answer two questions about every task in it: which block
started it, and which block waits for it.
