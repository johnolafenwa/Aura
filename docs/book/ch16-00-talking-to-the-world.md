# Talking to the world

Every program we have written so far has lived inside its own process. It
computed something, printed it, and stopped. Real tools spend most of their
time on the edges: they read configuration from disk, start other programs,
make HTTP requests, and write results somewhere another program can pick them
up. Those edges are where things go wrong in ways your code does not control.
A file is missing, a command hangs, a server refuses the connection, a JSON
document has a typo in line 40.

In this chapter we will build `checkup`, a small health checker. It reads a
list of checks from a file, runs each one, and writes a report. A check can
ask whether a file exists, whether a command exits successfully, or whether
an HTTP endpoint answers. We start with plain text files, then add
subprocesses, then networking, and then replace the hand-rolled text format
with typed JSON on both the input and the output side. Two further sections
go past what `checkup` needs: one on HTTP clients that read bodies, follow
streams, and retry, and one on UDP, WebSockets, Unix sockets, and TLS.

Aura's built-in modules for the outside world, `fs`, `process`, `net`,
`http`, and `json`, were designed together, and you will notice they share a shape.
Anything that can fail returns a `Result`, so failure is in the type and the
compiler makes you decide what to do with it. Anything that holds an
operating-system resource, such as an open file, a child process, or a
socket, is an owned value that belongs in a `with` block, so cleanup happens
on every path out of the scope. Anything that waits takes a `timeout`, because
a program that talks to the world has to assume the world can be slow. Once
you have seen the pattern in one module, the others read the same way.

By the end of the chapter you will be able to write a command-line tool that
reads and writes files safely, runs commands without a shell, serves and
calls HTTP, and converts between JSON text and your own classes and enums
with errors that point at the exact field that was wrong. You will also know
which kind of socket to reach for when HTTP is not the right fit.
