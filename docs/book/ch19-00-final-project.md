# Final project: a supervised job runner

We have reached the last chapter, and it is time to put most of the book to
work in one program. We are going to build `jobrunner`, a small tool that
reads a list of jobs from a file, runs each one as a child process, and
reports what happened. By the end it will run several jobs at once through a
worker pool, retry the ones that fail under a per-job restart policy, kill
jobs that run past their timeout, and shut the whole pool down cleanly when an
overall deadline passes, without leaving a single stray process behind.

Tools like this are everywhere: CI runners, build systems, deploy scripts,
cron replacements. They are also a good test of a language, because they mix
the things that are easy to get wrong. A job runner parses untrusted text,
talks to the operating system, juggles several pieces of work at the same
time, and has to clean up after itself when something goes wrong in the
middle. Aura gives us a tool for each of those: `Result` for parsing
(Chapter 9), the `process` module for children (Chapter 16), task groups and
queues for the pool (Chapter 15), and scoped cleanup for shutdown
(Chapter 14). The interesting part is seeing them fit together.

We will grow the program in three steps, the way you would build it for real.
First a single-task runner that does one job at a time and gets the
reporting right. Then a version that hands jobs to a pool of workers through
a queue, so independent jobs overlap. Finally, supervision: restart policies,
timeouts, a deadline that cancels the pool, and a look at
`process.supervisor()` for jobs that are meant to stay running. Every listing
is compiled, and every run you see is real output from the compiler this book
was tested against.

If you have skipped ahead, the chapters on
[ownership](/book/ch04-00-understanding-ownership),
[error handling](/book/ch09-00-error-handling), and
[concurrency](/book/ch15-00-concurrency) are the ones this project leans on
most. Everything else will be introduced as we go.
