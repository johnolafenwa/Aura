# `aura run` vs `aura build`, and what a native binary contains

Aura has two ways to execute a program, and you have been using one of them
without having to think about it. Both start the same way. The compiler reads
your source and every module it imports, checks types and ownership, and
lowers the result to MIR, the compiler's mid-level intermediate
representation. MIR is a simplified form of the program in which every
borrow, move, and drop is explicit. What happens next is the difference.

`aura run` executes the MIR directly. Nothing is written to disk and there is
no link step, so the time from saving a file to seeing its output is mostly
the time it takes to check it. That is what you want while editing.

`aura build` hands the MIR to the direct native backend, which generates
machine code with LLVM, links it against Aura's runtime library with your
system's C compiler, and writes an executable. That takes longer, but the
result runs without the compiler, without the source, and much faster.

Both paths share everything up to MIR: the parser, the checker, the module
resolver, the diagnostics. Aura's test suite runs programs through both and
treats any difference in output, exit status, or diagnostic as a bug. So a
program that behaves correctly under `aura run` should behave the same way as
a binary, and if it does not, that is worth reporting.

## Building `checkup`

From the `checkup` package directory:

```console
$ aura build -o checkup src/main.au
aura: built `checkup` with direct backend
```

`-o` names the output file. The build needs a C compiler on the machine doing
the building, because the final link goes through it; on Linux that is
usually `cc` from GCC or Clang, and on macOS the Xcode command line tools. The
machine that *runs* the binary needs neither.

The binary takes its arguments directly, without the `--` that `aura run`
needs to separate its own options from your program's:

```console
$ ./checkup
notes: missing
admin: failed (HTTP 404)
3 passed, 2 failed
$ echo $?
1
```

(That run had the HTTP server from Chapter 16 listening on port 8080.) The
exit status is the `int32` that `main` returned, exactly as with `aura run`.
`sys.args()` in a built program reads the real command line, so copying the
binary somewhere else and passing it a config path works the way you would
expect:

```console
$ cp checkup checks.json /some/other/dir/
$ cd /some/other/dir
$ ./checkup checks.json
notes: missing
admin: failed (HTTP 404)
3 passed, 2 failed
$ ./checkup nothing.json
cannot read nothing.json: io.Error.NotFound
$ echo $?
2
```

## What is inside the binary

Let's look at what we built. On a Linux x86-64 container:

```console
$ file checkup
checkup: ELF 64-bit LSB pie executable, x86-64, version 1 (SYSV), dynamically linked, interpreter /lib64/ld-linux-x86-64.so.2, BuildID[sha1]=61a9d4503264d9a4fa74282afbcd9b641ae03859, for GNU/Linux 3.2.0, not stripped
$ ldd checkup
	linux-vdso.so.1 (0x00007f3ffdeaf000)
	libgcc_s.so.1 => /lib/x86_64-linux-gnu/libgcc_s.so.1 (0x00007f3ffde6e000)
	libm.so.6 => /lib/x86_64-linux-gnu/libm.so.6 (0x00007f3ffd917000)
	libc.so.6 => /lib/x86_64-linux-gnu/libc.so.6 (0x00007f3ffd600000)
	/lib64/ld-linux-x86-64.so.2 (0x00007f3ffdeb1000)
```

The only shared libraries are the system C library, the math library, and
GCC's small support library, which every Linux machine has. Everything Aura
needs is linked in statically: your compiled functions, and the parts of
Aura's runtime library that they can reach. The runtime is what provides
lists and dictionaries, the task scheduler, the `fs`, `process`, and `net`
modules, and the HTTP and JSON implementations. To ship `checkup` you copy one
file.

The linker drops runtime code your program never calls, so a program that
does not use TLS, WebSockets, or JSON does not carry those libraries. The
Manual's [Performance](/manual/performance#executable-size) page records the
sizes of release builds on Linux x86-64: 973,016 bytes for hello world,
1,506,456 bytes for the JSON example, and 3,303,264 bytes for the TLS
example. `checkup` uses `fs`, `process`, `net` with HTTP, and JSON, so it
lands above all three: 4,224,152 bytes with a release compiler on a Linux
x86-64 container. Most of that is the runtime code it reaches, not the few
hundred lines we wrote.

Besides machine code, every binary carries source metadata: the names, files,
and line numbers of your functions. A normal build strips debugger symbols
(the `file` output says "not stripped" because a few global symbols are kept
on purpose), but this metadata is separate and stays in. It is what lets a
built program report a trap with the same source-located diagnostic that
`aura run` prints. Here is a small program with a bug:

*Filename: src/main.au*

```aura
def average(values: list[int64]) -> int64:
    mut total: int64 = 0
    for value in values:
        total += value
    return total // values.len()

def summarize(name: str, values: list[int64]):
    print(f"{name}: {average(values)}")

def main():
    summarize("week 1", [70, 85, 90])
    summarize("week 2", [])
```

<span class="caption">Listing 17-1: A program that divides by zero on its second call</span>

Run it both ways:

```console
$ aura run src/main.au
week 1: 81
error[AU4004]: division by zero
 --> src/main.au:5:12
  |
5 |     return total // values.len()
  |            ^
  = note: Aura call chain (innermost first): average at 1:1 -> summarize at 7:1 -> main at 10:1
$ aura build -o scores src/main.au
aura: built `scores` with direct backend
$ ./scores
week 1: 81
error[AU4004]: division by zero
 --> src/main.au:5:12
  |
5 |     return total // values.len()
  |            ^
  = note: Aura call chain (innermost first): average at 1:1 -> summarize at 7:1 -> main at 10:1
```

The two diagnostics are identical, down to the call chain, which names each
function and where it is defined. Both runs exit with status `1`. A trap in a
child task gets an extra note naming the tasks that started it. You get this
from every binary, without a debug build, and in practice it answers most
"where did it fail?" questions on its own. We will come back to the cases it
does not answer in the next section.

## A third way: `--backend direct`

`aura run` has a `--backend` option. The default is `mir`. `--backend direct`
builds a native binary behind the scenes and runs it, so you get native speed
and native behavior with the convenience of `aura run`. The first run of a
program pays for the build:

```console
$ aura run --backend direct src/main.au
aura: building native program...
notes: missing
web: error (io.Error.ConnectionRefused)
admin: error (io.Error.ConnectionRefused)
2 passed, 3 failed
```

(This time the HTTP server was not running.) With a release compiler on a
Linux x86-64 container, that run took about 2 seconds. The second, with no
changes to the source, took about 0.12 seconds and printed no build notice,
because the binary came from a cache. Aura keeps
built programs in a content-addressed cache, by default under
`~/.cache/aura/native` (set `AURA_CACHE_DIR` to move it). The cache key covers
the compiler version, the target, the runtime library, and the complete
lowered program including every imported module, so any change to any of
those means a rebuild, and nothing stale is ever run. Before running a cached
binary, Aura checks its SHA-256 and runs a private copy of exactly the bytes
it verified.

`--backend auto` prefers the native backend and falls back to MIR only if
building or launching a native binary is not possible on this machine, for
example because there is no C compiler. `aura build` itself always produces a
fresh executable and does not read the cache.

Here is a reasonable routine for choosing between them:

1. `aura check` while you type, or let the editor run it for you. It is the
   fastest way to find type and ownership errors.
2. `aura run` to try the program. It starts quickly, which matters more than
   execution speed when the program runs for a second.
3. `aura run --backend direct` when the program does enough work that speed
   matters, or when you want to see native behavior without managing a
   binary.
4. `aura build` for anything you will copy, deploy, or hand to someone else.

## Before you ship

One last point, which is easy to skip and expensive to learn. A program that
does I/O depends on its environment: which files exist, which commands are on
`PATH`, which ports are open, what the working directory is. `checkup` reads
`checks.json` relative to the directory it is started in, and runs whatever
commands that file names. Those things change when the program moves to
another machine, and no compiler can check them. Before a binary goes
anywhere important, run `aura check` on the source, build it with
`aura build`, and then run the built binary against the real files, services,
and commands it will meet, not only against the ones on your laptop.

The [CLI and Tooling](/manual/cli-and-tooling) page in the Manual documents
every command and flag, the exit status rules, and the native cache. Next,
let's look inside a running binary with a debugger.
