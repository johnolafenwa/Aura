# Aurora

Aurora is a systems programming language designed to have python like simplicity, the memory safety of rust and the concurrency of Go.

The goal is to build a systems programming language that is easy to learn and very effective for building agents and ML infrastructure.

## Monorepo layout

This repository is intended to evolve as a monorepo for the Aurora language and its associated tools.

- `crates/`
  - Rust compiler/runtime/bootstrap tooling
- `tools/`
  - editor integrations and other developer tools
- `package.json`
  - npm workspace manifest for repo-managed tools
- `examples/`
  - sample Aurora programs
- `docs/`
  - language proposal and supporting documentation
- `work/`
  - persistent task board and implementation notes

Compiler build and direct binary usage are documented in [crates/aura/README.md](/Users/johnolafenwa/source2/Aurora/crates/aura/README.md).

Current editor tooling:

- `tools/vscode-aurora`
  - VS Code extension for Aurora syntax highlighting and LSP client integration
- `tools/aurora-language-server`
  - Aurora Language Server Protocol implementation

## VS Code install

Development install:

1. Run `npm install` from the repo root.
2. Run `npm run build:extension`.
3. Run `npm run check:lsp`, `npm run test:lsp`, `npm run check:extension`, and `npm run test:extension`.
4. Open the repo in VS Code.
5. Open `tools/vscode-aurora`.
6. Press `F5` to launch an Extension Development Host.
7. Open an `.au` file such as `examples/point.au` in the Extension Development Host.

Packaged install:

1. Run `npm install`.
2. Run `npm run package:extension`.
3. In VS Code, use `Install from VSIX...` and select `tools/vscode-aurora/aurora-language.vsix`.

Full extension install and packaging steps are documented in [tools/vscode-aurora/INSTALL.md](/Users/johnolafenwa/source2/Aurora/tools/vscode-aurora/INSTALL.md).
