# Tooling

This chapter covers the tools that ship in the Aura monorepo: the `aura` command-line interface (CLI), the example library, and the VS Code extension.

## CLI

Use the `aura` CLI to check, run, and build Aura programs:

```bash
cargo run -p aura -- check myfile.au     # type-check without running
cargo run -p aura -- run myfile.au       # execute through the MIR runtime
cargo run -p aura -- build -o out myfile.au  # compile to a native binary
```

MIR is the compiler's mid-level intermediate representation. These commands inspect it and the other compiler internals:

```bash
cargo run -p aura -- ast myfile.au       # print the syntax tree
cargo run -p aura -- ast-json myfile.au  # syntax tree as JSON
cargo run -p aura -- mir myfile.au       # print the lowered MIR
cargo run -p aura -- analyze myfile.au   # diagnostics, symbols, hover info
cargo run -p aura -- complete --line 5 --character 11 --trigger . myfile.au
```

`aura fmt` formats source files in one fixed style. It sets four-space indentation, normalizes spacing and blank lines, and reflows bracketed lists at 88 columns. Comments and string literals keep their text:

```bash
cargo run -p aura -- fmt src             # format every .au file under src
cargo run -p aura -- fmt --check .       # list files that would change
cargo run -p aura -- fmt --stdin < myfile.au  # format stdin to stdout
```

The [CLI manual](../docs/manual/cli-and-tooling.md#formatting) lists every formatting rule.

[01-running-programs.md](01-running-programs.md) walks through each command. [crates/aura/README.md](../crates/aura/README.md) is the CLI reference.

## Documenting Code

A docstring is a triple-quoted string as the first statement of a module,
class, enum, trait, or function. A field docstring goes on the line directly
after the field. Document parameters under an `Args:` heading:

```aura
"""Temperature helpers."""

public class Reading:
    """One sensor sample."""

    public celsius: float64
    """The temperature in degrees Celsius."""

public def to_fahrenheit(reading: Reading) -> float64:
    """Convert a reading to degrees Fahrenheit.

    Args:
        reading: The sample to convert.
    """
    return reading.celsius * 9.0 / 5.0 + 32.0

def main():
    print(to_fahrenheit(Reading(celsius=100.0)))
```

Docstrings do not run. Editors show them on hover, in completions, and in
signature help. `aura doc` turns them into one Markdown page per module:

```bash
cargo run -p aura -- doc examples/modules/pkg/inventory.au
cargo run -p aura -- doc src --out api        # one file per module
cargo run -p aura -- doc --json src --out api # JSON instead of Markdown
```

`examples/modules/documented_api.au` uses the documented module
`examples/modules/pkg/inventory.au`. [Docstrings And API Docs](../docs/manual/docstrings.md)
has the full rules.

## Examples

The `examples/` directory holds runnable programs, sorted by category. Compiler tests run them, so they stay valid as the language changes. Read them alongside these tutorials to see working code for each feature.

## VS Code

The repository includes two editor packages:

- a VS Code extension under `tools/vscode-aura`
- an Aura language server under `tools/aura-language-server`

### Editor Features

- **Syntax highlighting** for `.au` files.
- **Completions**, including member completion after `.`.
- **Hover** that shows types, signatures, and docstrings.
- **Signature help** that shows the docstring and each parameter's `Args:` entry.
- **Go-to-definition**, including definitions of imported symbols in other files.
- **Diagnostics** from the compiler's type checker.
- **Document symbols** for navigation.
- **Formatting** with the same rules as `aura fmt`. Set `editor.formatOnSave` to format on save.

The editor gets its analysis from `aura analyze` and `aura complete`, so the editor and the CLI use the same type checker. A local JavaScript analysis layer runs only as a fallback when the compiler cannot analyze the current buffer.

### Installation

To run the extension from source:

1. Run `npm install` from the repo root.
2. Run `npm run build:extension`.
3. Open the repo in VS Code.
4. Press `F5` to launch an Extension Development Host.
5. Open any `.au` file.

For a packaged install, see [tools/vscode-aura/INSTALL.md](../tools/vscode-aura/INSTALL.md).

## Keeping Tutorials Current

These tutorials describe what the compiler implements, not the language proposal. When a feature is added, changed, or removed:

1. Update the tutorial chapter that covers it.
2. Update or add an example program.
3. Update `14-current-language-surface.md` if the supported surface changed.
