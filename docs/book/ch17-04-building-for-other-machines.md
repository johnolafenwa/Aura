# Building for other machines

A binary from `aura build` is made for the machine that built it. That is
easy to forget, because it so often works anyway: build on your Linux
laptop, copy to a Linux server, run. Then one day the server is an arm64
instance, or the destination is a minimal container image, and the binary
that ran everywhere will not start.

Two things tie a binary to its birthplace. The first is the processor
architecture: x86-64 machine code means nothing to an arm64 CPU. The second
is the C library. We saw in
[Section 17.1](/book/ch17-01-run-and-build) that a built program loads
`libc.so.6` and a dynamic linker from the system it runs on. Those files
come from glibc on Ubuntu, Debian, and Fedora. Alpine Linux uses a
different C library, musl, and an empty container image has no C library at
all.

`aura build --target` builds for a machine other than the one you are
sitting at. In Aura 0.16 it runs on a Linux host and produces Linux
programs for four targets, the two architectures with each of the two C
libraries. We will use a program small enough to keep in view:

*Filename: src/main.au*

```aura
import sys

def main():
    args = sys.args()
    if args.len() == 0:
        print("hello from a small binary")
    else:
        print(f"hello, {args[0]}")
```

<span class="caption">Listing 17-3: A program to build four ways</span>

## A fully static program

The target that solves the most problems is the musl one, so let's ask for
it:

```console
$ aura build --target x86_64-unknown-linux-musl -o hello-static src/main.au
error[AU2002]: this Aura installation has no runtime library for `x86_64-unknown-linux-musl`: expected `/home/ada/.local/lib/aura/targets/x86_64-unknown-linux-musl/libaura_runtime.a`; install the target pack of this release (see `aura build --target` in the manual)
 --> src/main.au
```

(The path will show your own installation directory.) Every Aura binary
contains Aura's runtime library, and the runtime is
itself compiled code. An installation comes with the runtime for its own
machine and nothing else. The runtime for another target arrives as a
*target pack*, a separate download of a few dozen megabytes, and the error
names the directory where it expects to find one. The installer fetches
packs when you name targets:

```console
$ curl -fsSL https://johnolafenwa.github.io/Aura/install.sh | sh -s -- --target x86_64-unknown-linux-musl
```

The pack lands under `lib/aura/targets` in the installation. A pack belongs
to one exact build of `aura`, and the compiler refuses to link against a
pack from another release, so the installer also refreshes the packs you
already have whenever it runs. That keeps them in step when you later run
`aura upgrade`. With the pack in place the build goes through:

```console
$ aura build --target x86_64-unknown-linux-musl -o hello-static src/main.au
aura: built `hello-static` with direct backend for x86_64-unknown-linux-musl
$ ./hello-static Ada
hello, Ada
$ file hello-static
hello-static: ELF 64-bit LSB executable, x86-64, version 1 (SYSV), statically linked, BuildID[sha1]=431b4ff86a2b158841d08a8190fb194681e3f467, not stripped
$ ldd hello-static
	not a dynamic executable
```

Compare that with the `file` and `ldd` output in Section 17.1. This program
names no dynamic linker and loads no shared libraries. The C library is
inside it, and the pack supplied that too, so you did not need to install a
musl compiler. All the binary asks of its host is a Linux kernel for
x86-64.

The clearest way to see what that buys is to run it where there is nothing
else. A container image built `FROM scratch` is an empty filesystem:

*Filename: Dockerfile*

```text
FROM scratch
COPY hello-static /hello
ENTRYPOINT ["/hello"]
```

```console
$ docker build -q -t hello-aura .
sha256:ac34ab37eccce32e6d1e387737a445e8a628667ea87c1d45035bf376ce93382b
$ docker run --rm hello-aura Ada
hello, Ada
$ docker images hello-aura --format '{{.Size}}'
2.16MB
```

The whole image is our one file. Put the ordinary glibc build of the same
program in the same empty image and it cannot even begin:

```console
$ docker run --rm hello-dynamic
exec /hello: no such file or directory
```

The file that is missing is not `/hello`. It is the dynamic linker,
`/lib64/ld-linux-x86-64.so.2`, which the kernel has to load before the
program, and which an empty image does not have. This is one of the more
confusing error messages in Linux, and now you know what it means.

A static program gives up one thing in exchange for its independence. It
cannot load a shared library at run time, so a package that uses the FFI
from [Chapter 18](/book/ch18-00-calling-c) must link its C libraries
statically. Speed is not one of the costs. The Manual measured the musl
builds of its benchmark programs at 0.94 to 1.09 times the running time of
the glibc builds.

## Another architecture

The same option builds for arm64. The target names put the architecture
first and the C library last:

| Target | Runs on |
| --- | --- |
| `x86_64-unknown-linux-gnu` | x86-64 Linux with glibc |
| `aarch64-unknown-linux-gnu` | arm64 Linux with glibc |
| `x86_64-unknown-linux-musl` | any x86-64 Linux, fully static |
| `aarch64-unknown-linux-musl` | any arm64 Linux, fully static |

A build for the other architecture needs two things: that target's pack,
and, for the final link, a C compiler that can link for that architecture.
On Ubuntu the cross compiler is one package:

```console
$ sudo apt install -y gcc-aarch64-linux-gnu
$ aura build --target aarch64-unknown-linux-musl -o hello-arm64 src/main.au
aura: built `hello-arm64` with direct backend for aarch64-unknown-linux-musl
$ file hello-arm64
hello-arm64: ELF 64-bit LSB executable, ARM aarch64, version 1 (SYSV), statically linked, BuildID[sha1]=7c4c6ebcf6187fca508fd6ff33c5bbed3552cfe1, not stripped
$ ./hello-arm64
bash: ./hello-arm64: cannot execute binary file: Exec format error
```

`aura` looks for the compiler under the name `aarch64-linux-gnu-gcc` (or
`x86_64-linux-gnu-gcc` in the other direction), and `AURA_TARGET_CC` names
a different one. The x86-64 machine that built the program cannot run it,
which is the "Exec format error" at the end. To try it without an arm64
machine, the QEMU user-mode emulator runs a foreign binary one process at a
time:

```console
$ sudo apt install -y qemu-user-static
$ qemu-aarch64-static ./hello-arm64 Grace
hello, Grace
```

That is how Aura's own continuous integration tests these targets. For a
program you intend to ship, run its real test suite on real hardware of the
target architecture before you trust it. `aura run` and `aura test` always
build for the machine they run on, so the tests have to run there, or
under emulation.

This is as far as cross-building goes in 0.16. The host must be Linux and
the targets are Linux. On macOS or Windows, `aura build --target` stops
with exit status 2, and so does any target name outside the table:

```console
$ aura build --target riscv64-unknown-linux-gnu -o hello src/main.au
aura: unknown target `riscv64-unknown-linux-gnu`; the targets are `x86_64-unknown-linux-gnu`, `aarch64-unknown-linux-gnu`, `x86_64-unknown-linux-musl`, `aarch64-unknown-linux-musl`
```

To produce a macOS or a Windows program, build on that system. And `aura`
itself needs glibc: you can build *for* Alpine, but not *on* it.

## Choosing the CPU

Architecture is a coarse description. Within x86-64 there are fifteen years
of processors, and the newer ones have instructions the older ones lack,
most usefully the wide vector instructions that
[Section 8.7](/book/ch08-07-lane-values) put to work. By default `aura
build` uses only what every CPU of the architecture has, so the binary runs
on all of them. `--target-cpu` raises that floor:

```console
$ aura build --target-cpu x86-64-v3 -o hello src/main.au
aura: built `hello` with direct backend
$ aura build --target-cpu haswell -o hello src/main.au
aura: unknown target CPU `haswell`; this host accepts `generic`, `native`, `x86-64-v2`, `x86-64-v3`, `x86-64-v4`
```

The names are levels, not models. `generic` is the default. `x86-64-v2`
adds SSE4.2, `x86-64-v3` adds AVX2 and FMA, which most x86-64 machines of
the last ten years have, and `x86-64-v4` adds AVX-512. `native` means
"whatever this machine has", which is the fastest choice for a program that
will only ever run where it was built. The environment variable
`AURA_TARGET_CPU` sets the same thing when the option is absent.

Two properties make this option safer than its counterparts in C. First,
it never changes a result. Aura does not let the compiler fuse a multiply
and an add or reorder float arithmetic, so a program prints the same
numbers at every level. Second, the choice is part of the native cache key,
so `aura run --backend direct` never mixes binaries built for different
CPUs.

What it does change is where the binary can run. A program built for
`x86-64-v3` and started on an older CPU dies with an illegal-instruction
signal the first time it reaches an AVX2 instruction, which may be at
startup or may be an hour in. So match the level to the oldest machine you
deploy to, and do not ship `native` builds to machines you have not seen.
`native` is also refused together with `--target`, because the build
machine's CPU says nothing about the machine you are building for:

```console
$ aura build --target x86_64-unknown-linux-musl --target-cpu native -o hello src/main.au
aura: target CPU `native` names the build machine's CPU and does not apply with `--target`
```

Whether a higher level makes your program faster is something to measure,
as the last section argued. Code that walks lists of numbers often gains.
Code that waits on files and sockets will not notice. Array arithmetic gains
nothing from the option at all, because the runtime's array kernels already
check the CPU when the program starts and pick their widest version.

The [CLI and Tooling](/manual/cli-and-tooling#building-for-another-machine)
page has the reference for both options, including how to install a target
pack beside an `apt` installation of Aura.

## Summary

You can now take an Aura program from source to something you can ship.
`aura run` executes MIR for fast iteration, and `aura build` produces a
native binary with the runtime linked in, which runs on a machine with no
compiler and reports traps with the same source-located diagnostics. A debug
build with `--debug` adds what lldb and VS Code need for breakpoints,
stepping, and Aura-typed variables. You know how to measure a program:
time the native binary, count allocations with `AURA_RUNTIME_STATS=1`,
profile with symbols kept, and read published numbers with their context.
And when the destination is not the machine in front of you, `--target`
builds for the other Linux architecture or produces a fully static program
that runs in an empty container, and `--target-cpu` trades portability for
newer instructions.

Everything so far has been Aura calling Aura, or Aura calling the operating
system through its standard modules. Sometimes the function you need already
exists in a C library. [Chapter 18](/book/ch18-00-calling-c) shows how Aura
calls C, and why it keeps that door deliberately narrow.
