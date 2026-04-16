# Tooling

Aurora ships with compiler and editor tooling inside the monorepo.

## CLI

The `aura` CLI is the primary interface for working with Aurora programs:

```bash
cargo run -p aura -- check myfile.au     # type-check without running
cargo run -p aura -- run myfile.au       # execute through the MIR runtime
cargo run -p aura -- build -o out myfile.au  # compile to a native binary
```

For inspecting compiler internals:

```bash
cargo run -p aura -- ast myfile.au       # print the syntax tree
cargo run -p aura -- ast-json myfile.au  # syntax tree as JSON
cargo run -p aura -- mir myfile.au       # print the lowered MIR
cargo run -p aura -- analyze myfile.au   # diagnostics, symbols, hover info
cargo run -p aura -- complete --line 5 --character 11 --trigger . myfile.au
```

See [01-running-programs.md](01-running-programs.md) for a full walkthrough of each command. The CLI is also documented in [crates/aura/README.md](../crates/aura/README.md).

## Examples

The categorized example library under `examples/` is part of the development workflow, not just sample code. Compiler tests exercise the examples, so they stay valid as the language evolves. Browse them alongside these tutorials to see runnable code for every feature.

## VS Code

The repo includes:

- a VS Code extension under `tools/vscode-aurora`
- an Aurora language server under `tools/aurora-language-server`

### Editor Features

- **Syntax highlighting** for `.au` files
- **Completions** with member completion after `.`
- **Hover** information showing types and signatures
- **Go-to-definition** including cross-file definitions for imported symbols
- **Diagnostics** from the compiler's type checker
- **Document symbols** for navigation

The editor uses compiler-backed analysis through `aura analyze` and `aura complete`. This means the editor and CLI share the same type-checking engine. The local JS analysis layer is kept only as a fallback when the compiler cannot analyze the current buffer.

### Installation

For development:

1. Run `npm install` from the repo root
2. Run `npm run build:extension`
3. Open the repo in VS Code
4. Press `F5` to launch an Extension Development Host
5. Open any `.au` file

For a packaged install, see [tools/vscode-aurora/INSTALL.md](../tools/vscode-aurora/INSTALL.md).

## Keeping Tutorials Current

This tutorial set tracks the compiler, not the proposal. When a feature is added, changed, or removed:

1. update the relevant tutorial chapter
2. update or add an example program
3. update `14-current-language-surface.md` if the supported surface changed
