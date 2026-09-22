# Getting Aura Running

This chapter installs Aura, runs a first program, and builds a native binary.
It covers both a release archive and a source checkout.

Aura release archives ship a command-line tool called `aura` and its private
native runtime under `lib/aura`. The tool parses, type-checks, runs, and
builds Aura source files. It is also the entry point for editor tooling.

Aura 0.3 is a technical preview.

## Install A Release Archive

The quickest install path supports Linux x64, macOS x64, and macOS arm64:

```bash
curl -fsSL https://johnolafenwa.github.io/Aura/install.sh | sh
```

The script verifies the release checksum and installs the compiler and its
native runtime under `~/.local`. If `~/.local/bin` is not on `PATH`, the
installer prints the exact export command. To choose another prefix, set
`AURA_INSTALL_PREFIX` before you run the command.

Once Aura is installed, this command updates the compiler and its bundled
native runtime:

```bash
aura upgrade
```

To install by hand, download the archive for a supported host and extract it.
Keep its directory layout intact:

```text
aura-v0.3.4-preview-<target>/
├── bin/aura
├── lib/aura/
│   ├── libaura_compiler.a
│   └── native-link-args.json
└── examples/
    ├── basic_addition.au
    └── agents/retrying_network_worker.au
```

Add the extracted `bin` directory to `PATH`. Running and checking programs
needs no Rust installation. Building a native executable needs a host C
compiler, because `aura` performs the final host link itself.

Aura has no native Windows archive. On Windows 11, run the Linux x86-64
release inside Ubuntu on Windows Subsystem for Linux 2 (WSL 2). Before you
rely on a host that is not listed, check the detailed
[platform installation guides](/install/) and the repository's
supported-platform matrix.

## Choose Your Platform Guide

Use the guide for the system where the `aura` command will run:

- [Install on macOS](/install/macos): Apple silicon and Intel Macs,
  persistent `PATH` setup, Xcode command-line tools, and verification.
- [Install on Linux](/install/linux): Ubuntu 24.04 and compatible x86-64
  glibc systems, required packages, and the native build toolchain.
- [Install on Windows with WSL 2](/install/windows-wsl): Ubuntu setup,
  where to keep files in the Linux filesystem, installing Aura inside WSL, and
  remote VS Code.
- [Install the VS Code extension](/install/vscode): the Marketplace, Open
  VSX, manual VSIX installation, WSL, compiler paths, and editor verification.

## Build From Source

To build Aura itself, you need the pinned Rust toolchain and a host C
compiler.

- **Rust**: install it through [rustup](https://rustup.rs).
  `rust-toolchain.toml` selects Rust 1.95.0.
- **C compiler**: on macOS, the Xcode command-line tools provide one. Install
  them with `xcode-select --install`. On Linux and on Ubuntu under WSL 2,
  `build-essential` supplies the supported host toolchain. Native Windows
  source builds are outside the distribution matrix.

## Build The Compiler

Clone the repository and build a release binary:

```bash
git clone https://github.com/johnolafenwa/Aura.git
cd Aura
cargo build --release -p aura
```

The release build is at `./target/release/aura`. In a source checkout,
`aura build` can use the sibling runtime that Cargo built. A distributed
archive uses the runtime installed beside the executable instead.

Put `aura` on your `PATH` so the commands in this book work as written:

```bash
export PATH="$PWD/target/release:$PATH"
aura --version
```

On Unix shells, consider adding that export to your shell profile.

Preview builds print their channel and source commit, for example
`aura 0.3.4-preview (0123456789ab)`. Source-checkout builds print the channel
as `aura 0.3.4-dev (0123456789ab)`.

## Install The VS Code Extension

Install the CLI first, and confirm that VS Code will be able to find it:

```bash
command -v aura
aura --version
```

Install **Aura Programming Language** from the Visual Studio Marketplace. Or
run this command from a terminal where `code` is available:

```bash
code --install-extension JohnOlafenwa.vscode-aura-lang
```

Open an `.au` file and check that the language mode reads **Aura**. Syntax
highlighting is bundled with the extension. Diagnostics, completion, hover,
definitions, and symbols come from the compiler server, which the extension
launches through `aura lsp`.

On Windows with WSL 2, open the project from the Ubuntu terminal with
`code .`. In the **WSL: Ubuntu** window that opens, select
**Install in WSL: Ubuntu** for the Aura extension. The extension and the
`aura` CLI must both run inside WSL.

The [complete VS Code installation guide](/install/vscode) also covers Open
VSX, manual VSIX installation, custom compiler paths, and troubleshooting.

## Your First Program

Save the following as `hello.au`:

```aura
print("hello from aura")
```

Run it:

```bash
aura run hello.au
```

You should see:

```
hello from aura
```

This program is a **top-level script**. Aura runs the file line by line and
exits when it reaches the end.

## Using `main`

To give a program an explicit entry point, define a function named `main`:

```aura
def main() -> int32:
    print("ready")
    return 0
```

The rules for `main`:

- It takes no parameters.
- It returns either `int32` or `None`.
- When the program is built as a native binary, a returned `int32` becomes the
  process exit code.
- A file may use top-level script statements or define `main`, but not both.

## The CLI At A Glance

The commands you will use day to day:

| Command | What it does |
| --- | --- |
| `aura run file.au` | Parse, type-check, and execute the program. |
| `aura check file.au` | Parse and type-check without running. |
| `aura check --format json file.au` | Emit schema-versioned structured diagnostics for tooling. |
| `aura build -o path file.au` | Compile a standalone native binary to `path`. |
| `aura ast file.au` | Print the parsed syntax tree. |
| `aura mir file.au` | Print the lowered intermediate representation. |
| `aura analyze file.au` | Emit compiler-backed analysis used by editor tooling. |
| `aura complete --line N --character M file.au` | Emit completion items at a source position. |
| `aura deps update [name]` | Refresh git dependencies and rewrite `Aura.lock`. |

`aura help` lists every command. `aura --version` shows the preview channel
and the exact source revision you are running.

`aura run` uses the MIR runtime by default, for a fast edit-run loop. MIR is
Aura's mid-level intermediate representation. Two flags change the backend:

- `--backend direct` requires native execution.
- `--backend auto` prefers native execution. When direct execution is
  unavailable, it falls back to MIR and says so.

## Building A Native Binary

```bash
aura build -o ./hello hello.au
./hello
```

`aura build` defaults to `auto`. It first tries direct native emission. It may
fall back to a standalone launcher that contains embedded MIR and the MIR
runtime. The resulting binary does not need the original `.au` source at run
time. Producing the binary still needs the host C compiler. Use
`--backend direct` when fallback is unacceptable.

The [Running And Shipping](/learn/native-builds) chapter explains when to pick
`run` or `build` and what each path gives you.

## When Something Goes Wrong

Aura's error messages usually point at the exact place in the source where
the compiler or runtime found the problem:

```
error[AU4002]: integer value `2147483648` does not fit in `int32`
 --> overflow.au:3:14
  |
3 |     c: int32 = a + b
  |              ^
```

How to read it:

- The bracketed `AU####` code is stable.
- The `-->` line names the file, line, and column.
- The caret points at the offending expression.
- Related spans, guidance, and safe source edits follow when available.

A program with a checker error does not run. A program with a runtime error
prints the diagnostic and exits with a non-zero status.

When a tool needs these fields, use `--format json` with `check`, `run`, or
`build` instead of parsing the human layout. Runtime diagnostics also carry
typed `call_frames`, innermost first, and `task_ancestry`, youngest child
first. Every schema-version-1 diagnostic includes both arrays. They are `[]`
when no runtime frames apply.

## Next

The next chapter builds a small program that counts and classifies values.
Along the way it introduces bindings, functions, control flow, and `match`.
