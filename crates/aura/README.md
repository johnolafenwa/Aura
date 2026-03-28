# aura CLI

This package contains the Aurora bootstrap compiler CLI.

## Build The Binary

From the repository root:

```bash
cargo build -p aura --release
```

That produces the standalone binary at:

```text
target/release/aura
```

## Use The Binary Without Cargo

After the release build completes, run the binary directly:

```bash
./target/release/aura check examples/classes/point_distance.au
./target/release/aura run examples/classes/point_distance.au
./target/release/aura run-mir examples/classes/methods.au
./target/release/aura run examples/generics/box_and_wrapper.au
./target/release/aura run examples/basics/default_arguments.au
./target/release/aura run examples/basics/pass_keyword.au
./target/release/aura run examples/modules/simple_import.au
./target/release/aura run examples/traits/greeter.au
./target/release/aura run examples/traits/generic_trait_impl.au
./target/release/aura run examples/traits/specialized_trait_dispatch.au
./target/release/aura run examples/numbers/numeric_casts.au
./target/release/aura run examples/concurrency/sleep_builtin.au
./target/release/aura build -o ./target/aurora-point examples/point.au
./target/release/aura build --backend direct -o ./target/aurora-direct examples/basic_addition.au
./target/release/aura ast examples/classes/point_distance.au
./target/release/aura ast-json examples/classes/point_distance.au
./target/release/aura mir examples/control_flow/while_break_continue.au
./target/release/aura analyze examples/classes/point_distance.au
./target/release/aura complete --line 5 --character 11 --trigger . examples/point.au
```

You can do the same with the other current examples:

```bash
./target/release/aura run examples/basics/main_function.au
./target/release/aura run examples/basics/top_level_script.au
./target/release/aura run-mir examples/generics/box_and_wrapper.au
./target/release/aura run-mir examples/traits/greeter.au
./target/release/aura run-mir examples/numbers/numeric_casts.au
```

## Install The Binary Somewhere On Your Path

If you want to use `aura` without typing the full path, copy it into a directory on your shell `PATH`.

Example:

```bash
mkdir -p "$HOME/.local/bin"
cp target/release/aura "$HOME/.local/bin/aura"
```

Then run:

```bash
aura run examples/classes/point_distance.au
```

## Command Summary

- `aura check <file.au>`
  - parse and type check a program
- `aura run <file.au>`
  - run a program
  - this now includes the maintained `pass` statement and `sleep(duration)` builtin
  - local file imports and `public` module boundaries now work for file-backed programs
- `aura run-mir <file.au>`
  - run a program through the current native MIR runtime path
  - this now includes the current explicit numeric cast surface with `expr as Type`
- `aura build -o <output> <file.au>`
  - compile a standalone native binary for a program
  - this accepts `--backend auto|direct`
  - `auto` is the default and uses the direct native backend for the maintained Aurora surface
  - `direct` forces the new low-level native backend for the full currently implemented Aurora language surface
  - it relies on Cargo/Rust and a host C compiler for the current build step
  - file-backed and stdin-backed programs with local module imports now build correctly through this path
- `aura ast <file.au>`
  - print the parsed syntax tree
- `aura ast-json <file.au>`
  - print the parsed syntax tree as JSON
- `aura mir <file.au>`
  - print the lowered MIR for the checked program
- `aura analyze <file.au>`
  - print machine-readable compiler analysis as JSON
  - file-backed and stdin-backed analysis now resolve local imports relative to the supplied path
- `aura complete --line <n> --character <n> [--trigger .] <file.au>`
  - print machine-readable completion items as JSON
  - `--line` and `--character` are zero-based
  - member completion expects the cursor to be positioned just after `.`
  - the CLI now tolerates the common incomplete-editor state where the buffer currently contains a dangling member access such as `counter.`, including at EOF
  - local imported modules now participate in compiler-backed completions for both file-backed and stdin-backed buffers, including imported trait methods

## Stdin Mode

Compiler-facing JSON commands still use stdin for editor integration, and the ordinary `check`, `run`, `run-mir`, and `build` commands now honor the supplied stdin path when resolving local module imports.

Examples:

```bash
cat examples/classes/point_distance.au | ./target/release/aura analyze --stdin /virtual/point.au
cat examples/classes/point_distance.au | ./target/release/aura ast-json --stdin /virtual/point.au
cat examples/point.au | ./target/release/aura complete --line 5 --character 11 --trigger . --stdin /virtual/point.au
cat examples/point.au | ./target/release/aura build -o ./target/aurora-point --stdin /virtual/point.au
cat examples/modules/simple_import.au | ./target/release/aura analyze --stdin /Users/johnolafenwa/source2/Aurora/examples/modules/simple_import.au
cat examples/modules/simple_import.au | ./target/release/aura check --stdin /Users/johnolafenwa/source2/Aurora/examples/modules/simple_import.au
cat examples/modules/simple_import.au | ./target/release/aura run --stdin /Users/johnolafenwa/source2/Aurora/examples/modules/simple_import.au
cat examples/modules/simple_import.au | ./target/release/aura run-mir --stdin /Users/johnolafenwa/source2/Aurora/examples/modules/simple_import.au
```

## Diagnostics

When `aura check`, `aura run`, `aura ast`, or `aura mir` fails, the CLI now prints:

- the error message
- file, line, and column
- the relevant source line
- a caret under the failure location

## Current Limitation

There is not yet a non-Cargo build system for the compiler itself.

The supported build path today is:

1. build once with `cargo build -p aura --release`
2. use the resulting `aura` binary directly after that

The current `aura build` matrix is:

1. `--backend auto` is the default
2. `--backend direct` uses the true direct native backend for the full currently implemented Aurora language surface
3. built binaries no longer depend on the original `.au` source files at runtime
4. both backend paths still need Cargo/Rust and a host C compiler during the build step

The new `aura run-mir` command is now native for the current implemented surface:

1. it is useful for exercising the new backend path directly
2. it now covers the current implemented Aurora surface, including `spawn`, `select`, channels, task groups, `try`, and `with`
3. the direct backend now covers the maintained Aurora surface, so `run-mir` is primarily an alternate execution path and backend-debugging tool
