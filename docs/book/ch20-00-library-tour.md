# A tour of the standard library

The chapters so far introduced a library module whenever a program needed
one. Files, processes, sockets, and JSON arrived with the health checker in
[Chapter 16](/book/ch16-00-talking-to-the-world), tasks and queues with
[Chapter 15](/book/ch15-00-concurrency), and arrays with
[Chapter 8](/book/ch08-00-collections). That left out a handful of modules
that almost every real program ends up importing and that never had a
project of their own: the ones for raw bytes and their encodings, for dates
and times, for regular expressions, for random numbers, and for telling an
operator what the program is doing.

This chapter visits them in turn. To keep the visits connected we will build
one small tool across the first five sections. `courier` wraps a text message in
an *envelope* that can be pasted into a chat, a URL, or a configuration file
and checked at the other end. An envelope is compressed, so it is short. It
is signed with a shared secret, so the receiver knows who made it and that
nobody changed it. It carries the time it was sealed, so an old one can be
refused. Each has a random identifier, and the tool keeps a log that we will
then search. None of those jobs is hard, and each one leans on a different
module.

Everything here ships with `aura`. There is nothing to install and no
dependency to declare: `import bytes` works in any file. All of these
modules behave the same under `aura run` and in a built binary, and the
linker leaves out of a binary the library code its program never calls.
They are also deliberately small. Where a module leaves something out, such as named
time zones or a hash other than SHA-256, we will say so, because knowing the
edge of a library saves an afternoon of looking for a function that is not
there.

They share one more edge. `aura verify` does not read library code, so a
decoded byte list, a parsed instant, a pattern match, or a random number is
a value it knows nothing about beyond its type. `courier` is nearly all
library calls, with very little arithmetic of its own, so this chapter adds
no contracts to it. When your own code computes with what a library
returned, use the pattern from the
[JSON section of Chapter 16](/book/ch16-04-json#checking-what-the-decoder-accepts):
test the value once where it arrives, and state what you tested in the
`requires` of the code behind it.

By the end of the chapter you will be able to move between text and bytes
without guessing at an encoding, sign and verify a message, do arithmetic on
instants in time, pull fields out of text with a pattern, choose between
reproducible and unpredictable random numbers, and emit structured logs and
counters from a running program. A last section, which stands apart from
`courier`, reads and writes pictures.
