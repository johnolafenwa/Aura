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
./target/release/aura check examples/point.au
./target/release/aura run examples/point.au
./target/release/aura ast examples/point.au
```

You can do the same with the other current examples:

```bash
./target/release/aura run examples/basic_addition.au
./target/release/aura run examples/top_level_addition.au
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
aura run examples/point.au
```

## Command Summary

- `aura check <file.au>`
  - parse and type check a program
- `aura run <file.au>`
  - run a program
- `aura ast <file.au>`
  - print the parsed syntax tree

## Current Limitation

There is not yet a non-Cargo build system for the compiler itself.

The supported build path today is:

1. build once with `cargo build -p aura --release`
2. use the resulting `aura` binary directly after that
