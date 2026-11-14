# ADR-0101: `aura build --target` builds programs for other Linux machines

- Status: Accepted
- Date: 2026-11-13
- Builds on: ADR-0085 (target CPU), ADR-0094 (freestanding builds)

## Context

`aura build` wrote a program for the machine it ran on: the same
architecture and the same C library. A program for an arm64 server had to
be built on an arm64 machine. A program for Alpine Linux, or for a
container with no C library at all, could not be built, because Aura has
no musl host.

The compiler already writes objects for other architectures: a
freestanding build picks its machine with `--target` (ADR-0094). A hosted
program also needs the Aura runtime library and a C library for the
machine it runs on, and a linker that reads that machine's objects.

## Decision

- **`aura build --target <name>` builds a hosted program for a Linux
  machine other than the build machine.** The names are the Rust and LLVM
  target names:

  | Target | Program |
  | --- | --- |
  | `x86_64-unknown-linux-gnu` | x86-64, linked against glibc |
  | `aarch64-unknown-linux-gnu` | arm64, linked against glibc |
  | `x86_64-unknown-linux-musl` | x86-64, static, linked against musl |
  | `aarch64-unknown-linux-musl` | arm64, static, linked against musl |

  The build machine's own target is an ordinary build. Without
  `--freestanding` a freestanding target name is refused with a message
  that names the flag.
- **The build machine is Linux.** The option is refused on macOS and
  Windows. The four targets share one object format and one calling
  convention per architecture with the Linux hosts, so the compiler needs
  no second code path.
- **A musl program is fully static.** It holds the C library and names no
  dynamic linker, so it runs on any Linux kernel of its architecture:
  Alpine, a glibc distribution, or an empty container.
- **A musl program links against the C library the runtime ships.** The
  Rust toolchain carries musl's `libc.a`, its start files and
  `libunwind.a` for each musl target. The link passes `-static -nostdlib
  -nostartfiles` and names those files, so the driver of a glibc toolchain
  links it. No musl C compiler is needed to build a program. The link
  also passes `--eh-frame-hdr`, which the unwinder needs and a driver
  leaves out of a static link, and `-lgcc` for the C objects in the
  runtime library.
- **The linker is a C compiler driver.** `AURA_TARGET_CC` names it. Without
  it, a target of the build machine's architecture uses `CC` or `cc`, and
  a target of another architecture uses `<arch>-linux-gnu-gcc`. A missing
  driver is an error that names the package to install.
- **Each target has its own runtime library.** An installation keeps it in
  `lib/aura/targets/<target>` beside the build machine's runtime library,
  with the same link manifest, and a musl target's directory also holds
  the C library, start files and `libunwind.a`. A source checkout builds
  it with Cargo for the Rust target of the same name. A target with no
  runtime library is an error that names the directory.
- **`--target-cpu` names a CPU of the target's architecture.** `native`
  names the build machine's CPU and is refused with `--target`.
- **The linker strips a target build.** The build machine's `strip` may
  not read another architecture, so the link passes `--strip-debug` and
  `--discard-all`. Global symbols stay, as in an ordinary build.
- **`aura run`, `aura test` and the native run cache are unchanged.** They
  build for the build machine.

## Consequences

- One Linux machine builds programs for x86-64 and arm64 servers and for
  musl systems.
- A static musl program cannot load a shared library when it runs. A
  program whose packages name native libraries under `[ffi]` links them
  statically or fails at the link.
- A musl program copies and compares memory with the runtime's routines,
  not musl's. musl's x86-64 `memcpy` and `memmove` are `rep movsq` for
  every size and its `memcmp` is a byte loop, and with them seven
  cross-language programs take 1.05 to 1.8 times as long as their glibc
  builds for the same instruction counts. The crate `aura-mem` defines
  `memcpy`, `memmove`, `memcmp` and `bcmp` on x86-64 musl, and `memcmp`
  and `bcmp` on arm64 musl, where musl's copies are tuned assembly. The
  runtime library comes before the C library in the link, so the linker
  takes these. The seven programs then take 0.94 to 1.09 times as long
  as their glibc builds. The runtime's own allocator is used on both.
- The runtime reads a musl program's arguments from `/proc/self/cmdline`.
  Rust's standard library learns the arguments of a program whose `main`
  is not Rust's only from glibc.
- The compiler picks the target through one process-wide setting, like the
  target CPU. One `aura build` process builds for one target.

## Alternatives considered

- **An Aura compiler that runs on musl.** It needs LLVM built from source
  for musl and a second release archive. Static musl programs built from a
  glibc host cover containers and Alpine deployment without it.
- **Link musl programs with a musl C compiler.** `musl-gcc` exists only
  for the build machine's architecture on common distributions, and no
  packaged musl cross compiler exists for the other. Shipping the C
  library with the runtime removes the requirement.
- **Call the linker directly.** The driver knows the dynamic linker path,
  the library search paths and `libgcc` of a glibc target. Finding those by
  hand repeats what every distribution's cross toolchain already encodes.
- **macOS and Windows targets.** Each needs that platform's SDK and linker
  on the build machine. They are a separate decision.
