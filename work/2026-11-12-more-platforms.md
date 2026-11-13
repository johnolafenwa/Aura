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
wait with a zero-timeout `WSAPoll` for 200 microseconds and only then
moves it to the watcher thread. Tests for it run on every host.

## Open

- musl Linux and cross-compilation are not started.
