# More platforms: Linux on arm64 and native Windows

Date: 2026-11-12. John asked for the "more platforms" roadmap item. There
is no Windows machine, so Windows is developed and tested on GitHub's
hosted `windows-2025` runner only.

## Goal

Support Linux on arm64 and native x86-64 Windows: build, tests in CI, a
release archive, and an installer. musl Linux and cross-compilation
follow.

## Work completed

### Linux on arm64

- CI job `platform-tests` builds Aura and runs the workspace tests on
  `ubuntu-24.04-arm`. `native-binaries` and the release workflow build an
  `aarch64-unknown-linux-gnu` archive, and `install.sh` selects it.
- `scripts/install-llvm.sh` installs the pinned LLVM 22.1.2 archive for
  arm64.

### Native Windows

- Target `x86_64-pc-windows-msvc`. `.cargo/config.toml` links the C
  runtime statically, so `aura.exe` loads only system DLLs.
- LLVM: the official `clang+llvm-22.1.2-x86_64-pc-windows-msvc` archive.
  With a static C runtime `llvm-sys` links LLVM's system libraries as
  static libraries, so the installer copies the Windows SDK import
  libraries into LLVM's library directory and writes an empty
  `xml2s.lib`, which the archive names and does not ship.
- Runtime: process spawning, the diagnostic channel between `aura run`
  and the program (inheritable pipe handles), Win64 struct passing for
  foreign calls, and a 1 ms timer period. A duplicated listener socket
  starts in blocking mode on Windows and is set non-blocking again.
- Code generation: COFF objects put each function in its own COMDAT
  section so the linker can drop unused ones.
- `aura build` and `aura run` link with `link.exe`, located with
  `find-msvc-tools`. The linker reports errors on standard output. The
  runtime's static library names `windows.<version>.lib` import
  libraries from the Rust `windows-targets` crates. A source checkout
  finds them in the Cargo registry. A release archive carries them
  beside `aura_runtime.lib`.
- The build lock, the launch lease and stale launch cleanup use
  `File::lock`. The runtime archive stamp uses the volume and file
  identifiers and the change time of the open file.
- `scripts/package-cli-windows.sh` writes the `.zip` archive,
  `scripts/smoke-cli-archive-windows.sh` runs it outside the checkout
  with no Cargo, and `docs/public/install.ps1` installs it.

## Verification

- CI job `windows`: builds `aura.exe`, runs a program in the
  interpreter and as a native executable, runs the runtime, command and
  compiler unit tests, packages the archive, runs the archive smoke
  test, and installs the archive with `install.ps1`.
- CI job `windows-integration (command)`: all 474 integration tests of
  the `aura` command. Test sources swap Unix tool paths for the bare
  names Git for Windows puts on `PATH`, and write paths with forward
  slashes. One test stays Unix only because it compares `pwd` output
  with a directory path. A cached program that Windows refuses to load
  (errors 191, 192, 193, 216) is rebuilt.
- CI job `windows-integration (compiler)`: the 60 integration suites of
  the compiler and runtime crates. Fixtures that spell host paths carry a
  `<name>.windows.stdout` expected file.
- Local: `cargo check --target x86_64-pc-windows-gnu` type-checks the
  Windows code on Linux, and Wine runs the runtime tests.

## Speed

The `Platform speed` workflow times the cross-language programs on
hosted runners. Compare ratios within one runner, not milliseconds
across runners. First run, on main `7af5cf1`:

- Linux arm64 tracks x86-64 Linux on every program.
- Windows compute programs are level with or ahead of Linux against
  Rust. The slowest are `lzw_compress` (5.0x Rust) and `markdown_render`
  (4.9x).
- Windows loopback network programs are behind: `kv_server` 4911 ms
  (0.91x Python, 1.5x Rust), `pubsub_broker` 5816 ms (0.97x, 1.6x),
  `http_api` 1545 ms (0.62x, 1.2x). On Linux the same programs run at
  0.3x to 0.6x of Rust. Cause: every socket wait went through the shared
  watcher thread, three thread wake-ups per wait.
- `template_render` is slower than Python on every host (1.17x to
  1.46x). It is not a platform matter.

Socket waits on Windows now start on the reactor's own thread
(`LocalSocketWaits` in `socket_watcher.rs`): the reactor polls a new
wait with a zero-timeout `WSAPoll` for 50 microseconds and only then
moves it to the watcher thread. Tests for it run on every host.

Measured on one Windows runner, Aura milliseconds and the ratio to Rust,
with the wait handed to the watcher at once and with the 50 microsecond
local wait:

| Program | At once | 50 microseconds |
| --- | --- | --- |
| `http_api` | 1398 to 1556, 1.6x to 1.7x | 1240 to 1435, 1.4x to 1.5x |
| `kv_server` | 1505 to 1557, 0.8x | 799 to 939, 0.4x to 0.5x |
| `pubsub_broker` | 4347 to 4373, 2.0x | 1157 to 1286, 0.5x to 0.6x |
| `http_stream` | 1337 to 1343, 1.5x | 1035 to 1045, 1.1x to 1.2x |

- A 200 microsecond local wait made `http_api` slower (2062 to 2815 ms,
  2.4x to 3.2x): each request opens a connection, a loopback connect
  takes about that long, and a wait that outlasts the local time pays for
  it and then for the watcher. 1000 microseconds gave 1429 ms.
- A pause between the local polls made every program slower, the more
  the longer the pause.
- `http_api` stays at 1.4x of Rust and 0.74x to 0.80x of Python. Its
  waits are connects and accepts that outlast the local wait and still go
  through the watcher thread.

## Building for other Linux machines (ADR-0101)

`aura build --target <name>` builds for `x86_64-unknown-linux-gnu`,
`aarch64-unknown-linux-gnu`, `x86_64-unknown-linux-musl` and
`aarch64-unknown-linux-musl` from a Linux host.

- Compiler: `HostedTarget` and a process-wide `set_hosted_target`, which
  the host triple, the foreign-call convention, the target CPU names and
  the lane-check choice read. No lib test sets it, since other tests emit
  objects at the same time.
- Command: `--target` on `aura build`. The runtime library comes from
  `lib/aura/targets/<target>` of an installation or from Cargo in a
  checkout. The link driver is `AURA_TARGET_CC`, `CC` or `cc` for the
  build machine's architecture, and `<arch>-linux-gnu-gcc` for the other.
- musl: the link passes `-static -no-pie -nostdlib -nostartfiles` and
  names `crt1.o`, `crti.o`, `crtbegin.o`, `crtend.o`, `crtn.o`, `libc.a`
  and `libunwind.a` from the Rust toolchain's `self-contained` directory
  for the target. The runtime library and the C library are one linker
  group: musl's `printf` calls soft-float routines the runtime library
  holds.
- Building the runtime library for a musl target needs a musl C compiler
  for `ring` and `libffi`. `musl-gcc` has no kernel headers, so
  `scripts/cross-target-env.sh` points it at a directory of links.
- Tests: `crates/aura/tests/cross_targets.rs` builds a program with
  tasks, a loopback socket, files, the clock and float formatting, and
  runs it, under `qemu-<arch>-static` for the other architecture. CI job
  `cross-targets` runs it on both Linux runners for the musl target of
  the runner's architecture and the glibc target of the other.
- Checked by hand on x86-64: all four targets build and run, the arm64
  ones under QEMU.
- Found by timing release builds: a failed check aborted a musl program
  with "failed to initiate panic", since the static link had no
  `.eh_frame_hdr`; `sys.args()` was empty; and an arm64 musl debug build
  needed `-lgcc`. The test program now takes arguments and the test runs
  it once with none, which fails a bounds check.
- The arm64 musl runtime library cannot be built on x86-64 with the
  glibc cross compiler: `libffi` then names `__isoc23_sscanf`. It is
  built on arm64 with `musl-gcc`.
- Speed on x86-64, release builds, best of five, glibc then musl in
  milliseconds: `binary_trees` 129 and 136, `json_roundtrip` 254 and 457,
  `kv_server` 472 and 541, `task_pool` 238 and 328, `word_freq` 412 and
  611, `sort_records` 306 and 411, `http_api` 155 and 183. Callgrind
  counts the same instructions for both builds of `json_roundtrip`. The
  time is in musl's `memcpy` (`rep movsq`) and `memcmp` (a byte loop).
- New crate `aura-mem` (`no_std`, `no_builtins`) defines `memcpy`,
  `memmove`, `memcmp` and `bcmp` for musl targets (copies on x86-64
  only). Its tests run the routines by their Rust names on every host.
  With it, best of seven, glibc then musl in milliseconds:
  `binary_trees` 132 and 123, `json_roundtrip` 242 and 264, `kv_server`
  462 and 455, `task_pool` 245 and 236, `word_freq` 397 and 382,
  `sort_records` 299 and 302, `http_api` 149 and 160.

## Open

- arm64 musl speed is unmeasured: the cloud machine runs arm64 programs
  only under emulation.
- arm64 hosts build x86-64 musl programs only in principle: the
  `target-packs` jobs run the x86-64 CLI. The arm64 CLI is covered for
  its own musl target and the x86-64 glibc target.

## Target packs

- A target pack is `aura-<tag>-target-<target>.tar.gz` and unpacks over a
  prefix: everything is in `lib/aura/targets/<target>`. It holds the
  runtime library, `native-link-args.json`, `aura-version`, the license,
  and for musl the start files, `libc.a`, `libunwind.a` and their
  notices (`packaging/targets/MUSL-NOTICES.md`).
- `scripts/package-target.sh` builds a pack on a Linux machine of the
  target's architecture. The glibc pack repackages the release runtime
  library; the musl pack builds the runtime library for the target.
  A pack is about 30 MB, which is why the CLI archives do not hold them.
- `aura-version` is the `aura --version` line of the build. `aura build
  --target` refuses a pack whose line differs from its own.
- `install.sh --target <name>` installs packs and replaces every pack
  already under the prefix, so an upgrade does not leave a runtime
  library of the earlier release.
- `scripts/smoke-target-packs.sh` unpacks a CLI archive and packs outside
  the source tree, builds the packaged examples for each target with no
  Cargo, checks the ELF header, runs the programs (arm64 under QEMU) and
  checks that a pack of another build is refused.
- CI: `native-binaries` on both Linux runners makes both packs, runs the
  musl one and uploads them; `target-packs` runs the x86-64 CLI with the
  other three targets. The release workflow does the same in `cli` and
  `target_packs`, and `publish` waits for `target_packs`.
