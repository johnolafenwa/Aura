# Installing Aurora In VS Code

This guide covers both development use from this monorepo and installing Aurora as a packaged VS Code extension.

## Requirements

- Node.js 22 or later
- VS Code 1.90 or later
- Rust toolchain if you want compiler-backed diagnostics/navigation from the repo workspace

## Install From This Repo For Development

1. Open a terminal at the repo root:
   - `Aurora/`
2. Install workspace dependencies:
   - `npm install`
3. Build the extension bundles:
   - `npm run build:extension`
4. Verify the editor packages:
   - `npm run check:lsp`
   - `npm run test:lsp`
   - `npm run check:extension`
   - `npm run test:extension`
5. Open the repo in VS Code.
6. Open the extension package folder:
   - `tools/vscode-aurora`
7. Press `F5` to launch an Extension Development Host.
8. In the Extension Development Host, open an Aurora file such as:
   - `examples/classes/point_distance.au`

The Aurora extension will activate automatically for `.au` files and start the bundled Aurora language server.

For compiler-backed diagnostics, symbols, hover, go-to-definition, and completions in this repo workspace, also build the Aurora CLI at least once:

- `cargo build -p aura`

The language server will look for:

- `AURORA_LSP_AURA_PATH`
- `target/debug/aura`
- `target/release/aura`
- `cargo run -q -p aura --` inside the repo workspace
- `aura` on `PATH`

If no compiler command is available, the extension falls back to the bundled JS document analysis layer.

## Package A VSIX From This Repo

1. From the repo root, install dependencies if you have not already:
   - `npm install`
2. Build and package the extension:
   - `npm run package:extension`
3. The VSIX will be written to:
   - `tools/vscode-aurora/aurora-language.vsix`

The packaging command builds the extension and emits self-contained bundles in `tools/vscode-aurora/dist/` so the VSIX does not depend on sibling workspace files.

## Install The Packaged VSIX In VS Code

1. Open VS Code.
2. Open the Extensions view.
3. Open the `...` menu in the Extensions view.
4. Choose `Install from VSIX...`.
5. Select:
   - `tools/vscode-aurora/aurora-language.vsix`
6. Reload VS Code when prompted.

## Update Workflow While The Language Evolves

When the Aurora language server or extension changes, rebuild the extension bundle before testing in VS Code:

- `npm run build:extension`

If you want to repackage the extension after changes:

- `npm run package:extension`

## Current Scope

The Aurora VS Code tooling currently provides:

- `.au` file recognition
- syntax highlighting
- snippets
- document symbols
- top-level completions
- member completions after `.`
- hover
- go-to-definition
- document diagnostics

The LSP implementation currently comes from `tools/aurora-language-server`, and the extension bundles that server into its own `dist/` output during build.
