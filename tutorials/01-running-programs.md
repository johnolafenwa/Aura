# Running Programs

Aurora currently runs through the bootstrap CLI, `aura`.

## The Main Commands

From the repository root:

```bash
cargo run -p aura -- check examples/classes/point_distance.au
cargo run -p aura -- run examples/classes/point_distance.au
cargo run -p aura -- run-mir examples/classes/methods.au
cargo run -p aura -- build -o ./target/aurora-point examples/point.au
cargo run -p aura -- build --backend direct -o ./target/aurora-direct examples/basic_addition.au
cargo run -p aura -- ast examples/classes/point_distance.au
cargo run -p aura -- ast-json examples/classes/point_distance.au
cargo run -p aura -- mir examples/control_flow/while_break_continue.au
cargo run -p aura -- analyze examples/classes/point_distance.au
cargo run -p aura -- complete --line 5 --character 11 --trigger . examples/point.au
```

- `check`
  - parse and type check the file
- `run`
  - execute it through the interpreter-backed runtime
- `run-mir`
  - execute it through the current native MIR runtime path
  - it now covers the current implemented Aurora surface, including `spawn`, `select`, channels, task groups, `try`, and `with`
- `build`
  - compile a standalone native binary
  - `--backend auto` is the default
  - `--backend direct` forces the true direct native backend for the full currently implemented Aurora language surface
  - the generated binary does not depend on the original `.au` source file at runtime
  - built binaries now preserve file, line, and caret context for arithmetic runtime failures
  - the current build step still requires Cargo/Rust and a host C compiler
- `ast`
  - print the parsed syntax tree
- `ast-json`
  - print the parsed syntax tree as JSON
- `mir`
  - print the lowered MIR for the checked program
- `analyze`
  - print machine-readable compiler analysis for diagnostics, symbols, hover, and definition
- `complete`
  - print machine-readable completion items for a position in the file
  - `--line` and `--character` use zero-based positions
  - member completion expects the cursor to be positioned just after `.`
  - the current compiler also tolerates the common incomplete-editor state where the buffer contains one or more dangling member accesses like `counter.` or `helpers.math.`, including at EOF

The machine-readable commands support stdin for editor integration, and the ordinary `check`, `run`, `run-mir`, and `build` commands can also resolve local imports from stdin when you provide a real workspace path:

```bash
cat examples/point.au | cargo run -p aura -- analyze --stdin /virtual/point.au
cat examples/point.au | cargo run -p aura -- complete --line 5 --character 11 --trigger . --stdin /virtual/point.au
cat examples/point.au | cargo run -p aura -- build -o ./target/aurora-point --stdin /virtual/point.au
cat examples/modules/simple_import.au | cargo run -p aura -- run --stdin /Users/johnolafenwa/source2/Aurora/examples/modules/simple_import.au
```

## Scripts And `main`

Aurora supports two entry styles in the implemented subset.

### Top-level script

```python
a = 56
b = 100
print(a + b)
```

See [examples/basics/top_level_script.au](../examples/basics/top_level_script.au).

### Explicit `main`

```python
def main() -> int32:
    print(5)
    return 0
```

See [examples/classes/point_distance.au](../examples/classes/point_distance.au).

If a file has top-level executable statements, it must not also declare `main`.

## Editor Tooling Uses The Compiler

The VS Code language server now uses compiler-owned `analyze` and `complete` output when possible.

That means the editor and CLI are now sharing the same semantic source for:

- diagnostics
- symbols
- hover
- go-to-definition
- completions
