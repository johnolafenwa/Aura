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

[01-running-programs.md](01-running-programs.md) walks through each command. [crates/aura/README.md](../crates/aura/README.md) is the CLI reference.

## Examples

The `examples/` directory holds runnable programs, sorted by category. Compiler tests run them, so they stay valid as the language changes. Read them alongside these tutorials to see working code for each feature.

## VS Code

The repository includes two editor packages:

- a VS Code extension under `tools/vscode-aura`
- an Aura language server under `tools/aura-language-server`

### Editor Features

- **Syntax highlighting** for `.au` files.
- **Completions**, including member completion after `.`.
- **Hover** that shows types and signatures.
- **Go-to-definition**, including definitions of imported symbols in other files.
- **Diagnostics** from the compiler's type checker.
- **Document symbols** for navigation.

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
