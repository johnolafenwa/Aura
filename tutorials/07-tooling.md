# Tooling

Aurora already ships with basic compiler and editor tooling inside the monorepo.

## CLI

Use `aura` to:

- type check files
- run programs
- inspect AST
- inspect JSON AST
- inspect MIR
- inspect compiler analysis
- inspect compiler completions

The CLI is documented in [crates/aura/README.md](../crates/aura/README.md).

The current command set includes:

- `check`
- `run`
- `ast`
- `ast-json`
- `mir`
- `analyze`
- `complete`

## Examples

The categorized example library under `examples/` is part of the development workflow, not just sample code.

The compiler tests exercise both the original bootstrap examples and the categorized example set so examples stay valid as the language evolves.

## VS Code

The repo also contains:

- a VS Code extension under `tools/vscode-aurora`
- an Aurora language server under `tools/aurora-language-server`

Current editor support includes:

- syntax highlighting
- completions
- hover
- go-to-definition
- diagnostics
- document symbols

The editor now prefers compiler-backed analysis and completions through `aura analyze` and `aura complete`. That compiler path now resolves local imports for both file-backed programs and stdin-backed editor buffers when the supplied path points at a real workspace file, keeps imported trait methods visible in completions, and recovers dangling-member buffers even when the dot is currently at EOF. The local JS analysis layer is kept only as a fallback when the compiler cannot analyze the current buffer.

## Keeping Tutorials Current

This tutorial set should track the compiler, not just the proposal.

When a feature is added, changed, or removed:

1. update the relevant tutorial chapter
2. update or add an example program
3. update the work log or task board if the change shifts the current implementation boundary
