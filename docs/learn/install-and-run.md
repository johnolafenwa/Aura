# Getting Aurora Running

Aurora release archives ship a command-line tool called `aura` plus its private native runtime under `lib/aurora`. The tool parses, type-checks, runs, and builds Aurora source files, and it also serves as the entry point for editor tooling.

Aurora 0.1 is a technical preview. This chapter covers both a release archive and a source checkout.

## Install A Release Archive

Download the archive for a supported host from the GitHub release, extract it, and keep its directory layout intact:

```text
aurora-0.1.0-<target>/
├── bin/aura
└── lib/aurora/
    ├── libaurora_compiler.a
    └── native-link-args.json
```

Add the extracted `bin` directory to `PATH`. Running and checking programs need no Rust installation. Building a native executable needs a host C compiler because `aura` performs the final host link itself.

Aurora does not publish a Windows archive in 0.1. See the repository's supported-platform matrix before relying on a source build on an unlisted host.

## Build From Source

Contributors building Aurora itself need the pinned Rust toolchain and a host C compiler.

- **Rust**: install through [rustup](https://rustup.rs). `rust-toolchain.toml` selects Rust 1.95.0.
- **C compiler**: macOS ships one through the Xcode command-line tools (`xcode-select --install`). On Linux, `build-essential` or its distribution equivalent is enough. Windows source builds are experimental and are not part of the 0.1 support matrix.

## Build The Compiler

Clone the repository and build a release binary:

```bash
git clone https://github.com/johnolafenwa/Aurora.git
cd Aurora
cargo build --release -p aura
```

The release build lives at `./target/release/aura`. In a source checkout, `aura build` can use the sibling Cargo-built runtime. A distributed archive instead uses the runtime installed beside the executable.

Put `aura` on your path so the rest of the commands in this book read naturally:

```bash
export PATH="$PWD/target/release:$PATH"
aura --version
```

On Unix shells, consider adding that export to your shell profile.

## Your First Program

Save the following as `hello.au`:

```python
print("hello from aurora")
```

Run it:

```bash
aura run hello.au
```

You should see:

```
hello from aurora
```

The program is a **top-level script**. Aurora runs the file line by line and exits when it reaches the end.

## Using `main`

For programs that want an explicit entry point, define a function named `main`:

```python
def main() -> int32:
    print("ready")
    return 0
```

`main` takes no parameters. It returns either `int32` or `None`. A returned `int32` becomes the process exit code when the program is built as a native binary. A file may use script-style top-level statements **or** define `main`, but not both.

## The CLI At A Glance

The commands you will use day to day are:

| Command | What it does |
| --- | --- |
| `aura run file.au` | Parse, type-check, and execute the program. |
| `aura check file.au` | Parse and type-check without running. |
| `aura build -o path file.au` | Compile a standalone native binary to `path`. |
| `aura ast file.au` | Print the parsed syntax tree. |
| `aura mir file.au` | Print the lowered intermediate representation. |
| `aura analyze file.au` | Emit compiler-backed analysis used by editor tooling. |
| `aura complete --line N --character M file.au` | Emit completion items at a source position. |
| `aura deps update [name]` | Refresh git dependencies and rewrite `Aurora.lock`. |

Use `aura help` for the full list and `aura --version` to confirm which build you are on.

## Building A Native Binary

```bash
aura build -o ./hello hello.au
./hello
```

`aura build` defaults to `auto`, which first tries direct native emission and may fall back to a standalone launcher containing embedded MIR plus the MIR runtime. The resulting binary does not need the original `.au` source at runtime; it does still need the host C compiler to produce the artifact. Use `--backend direct` when fallback is unacceptable.

The [Running And Shipping](/learn/native-builds) chapter covers when to pick `run` versus `build` and what each path gives you.

## When Something Goes Wrong

Aurora's error messages usually point at the exact place in the source where the compiler or runtime found the problem:

```
error: integer value `2147483648` does not fit in `int32`
 --> overflow.au:3:14
  |
3 |     c: int32 = a + b
  |              ^
```

The `-->` line names the file, line, and column. The caret points at the offending expression. A program with a checker error will not run; a program with a runtime error prints the diagnostic and exits with a non-zero status.

## Next

The next chapter builds a small program that counts and classifies values, and in doing so introduces bindings, functions, control flow, and `match`.
