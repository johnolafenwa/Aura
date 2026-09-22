# Running Programs

You run Aura programs with the `aura` command-line interface (CLI). From a
source checkout, invoke it from the repository root with `cargo run -p aura --`.

## Your First Program

Create a file called `hello.au`:

```aura check-pass
print("hello, aura")
```

Run it:

```bash
cargo run -p aura -- run hello.au
```

The terminal prints `hello, aura`.

## The Core Commands

You will use three commands most often:

```bash
cargo run -p aura -- check examples/classes/point_distance.au
cargo run -p aura -- run examples/classes/point_distance.au
cargo run -p aura -- build -o ./target/aura-point examples/point.au
```

| Command | What it does |
|---------|--------------|
| `check` | Parses and type-checks the file without running it. Use it for fast feedback while you edit. |
| `run` | Runs the program through the MIR runtime. MIR is the compiler's mid-level intermediate representation. This is the easiest way to test your code. |
| `build` | Compiles a standalone native binary. The binary does not need the original `.au` source files at runtime. |

## Build Backends

The `build` command accepts a `--backend` flag:

| Flag | Behavior |
|------|----------|
| `--backend auto` | The default. Tries direct native emission first. It may fall back to a standalone launcher with embedded MIR. |
| `--backend direct` | Requires direct native emission. Rejects programs that cannot use it. |

The default is usually what you want. Use `direct` for backend testing, or
when an embedded-MIR fallback is not acceptable.

Building requires a host C compiler. A CLI run from a source checkout may use
Cargo to refresh the native runtime. Installed release archives carry that
runtime and do not require Rust. Built binaries keep the file, line, and caret
context for runtime failures.

## Inspection Commands

These commands help you debug and understand your code:

| Command | Prints |
|---------|--------|
| `ast` | the parsed syntax tree |
| `ast-json` | the syntax tree as machine-readable JSON |
| `mir` | the lowered MIR for the checked program |
| `analyze` | machine-readable compiler analysis: diagnostics, symbols, hover, and definition |
| `complete` | completion items for a position in the file |

```bash
cargo run -p aura -- ast examples/classes/point_distance.au
cargo run -p aura -- mir examples/control_flow/while_break_continue.au
cargo run -p aura -- analyze examples/classes/point_distance.au
cargo run -p aura -- complete --line 5 --character 11 --trigger . examples/point.au
```

For `complete`, `--line` and `--character` are zero-based. Member completion
expects the cursor right after the `.`.

`check`, `run`, and `build` accept `--format human|json` for diagnostics. The
JSON form is schema-versioned. Each diagnostic keeps:

- the compiler's stable `AU####` code
- primary and related spans
- notes, help, and machine-applicable edits
- typed runtime `call_frames` and `task_ancestry`

The two frame arrays are always present. They are empty for diagnostics
without runtime frames.

Use `help` and `--version` to see CLI usage and the current version:

```bash
cargo run -p aura -- help
cargo run -p aura -- --version
```

`--version` prints the build channel and a 12-hex-digit source commit. A
source checkout prints something like `aura 0.3.4-dev (0123456789ab)`. A
release archive prints `aura 0.3.4-preview (0123456789ab)`.

Use `deps update` to refresh git dependencies. You do not need to delete
`Aura.lock` by hand:

```bash
cargo run -p aura -- deps update
cargo run -p aura -- deps update util
```

## Stdin Mode For Editors

Every command can read the program from standard input for editor
integration. Pass a virtual path so the compiler can resolve local imports:

```bash
cat examples/modules/simple_import.au | cargo run -p aura -- run --stdin "$(pwd)/examples/modules/simple_import.au"
```

The VS Code language server uses this mode to send unsaved editor buffers to
the compiler.

## Package-Aware Commands

When a file lives in a package with an `Aura.toml`, the CLI does three things
automatically:

- resolves local modules from `src/`
- resolves path and git dependencies by package name
- updates `Aura.lock`

```bash
cargo run -p aura -- run examples/packages/local_path_dependencies/app/src/main.au
cargo run -p aura -- run examples/packages/workspace/app/src/main.au
```

To refresh moving git references, run `deps update` from a package or
workspace directory:

```bash
cd examples/packages/local_path_dependencies/app
cargo run -p aura -- deps update
cargo run -p aura -- deps update util
```

See [18-packages-and-workspaces.md](18-packages-and-workspaces.md) for details.

## Scripts And `main`

A program uses one of two entry styles. Do not mix top-level executable
statements with `main` in the same file.

### Top-level script

Write executable statements directly at the top level. This is the simplest
way to start:

```aura check-pass
a = 56
b = 100
print(a + b)
```

See [examples/basics/top_level_script.au](../examples/basics/top_level_script.au).

### Explicit `main`

Declare a `main` function when the program returns an exit code:

```aura check-pass
def main() -> int32:
    print(5)
    return 0
```

See [examples/classes/point_distance.au](../examples/classes/point_distance.au).

## Editor Tooling

The VS Code language server uses the compiler's `analyze` and `complete`
output. The editor and the CLI therefore share one type-checking engine for:

- diagnostics
- symbols
- hover
- go-to-definition
- completions

See [08-tooling.md](08-tooling.md) for setup instructions.
