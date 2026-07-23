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
   - `npm ci`
3. Build the repo-local compiler server:
   - `cargo build -p aura`
4. Build the extension bundles:
   - `npm run build:extension`
5. Verify the editor packages:
   - `npm run check:lsp`
   - `npm run test:lsp`
   - `npm run check:extension`
   - `npm run test:extension`
6. Open the repo in VS Code.
7. Open the extension package folder:
   - `tools/vscode-aurora`
8. Press `F5` to launch an Extension Development Host.
9. In the Extension Development Host, open an Aurora file such as:
   - `examples/classes/point_distance.au`

The Aurora extension will activate automatically for `.au` files and start the bundled Aurora language server.

Step 3 creates the repo-local `target/debug/aura` used for compiler-backed
diagnostics, symbols, hover, go-to-definition, and completions. To install the
actual compiler-owned server on `PATH` for all Aurora workspaces, run:

```bash
cargo install --path crates/aura --locked --force
```

This installs the `aura` executable (normally under `~/.cargo/bin`). The
extension starts `aura lsp` over stdio automatically; do not start a separate
server process by hand. There is no second semantic-server executable to
install.

The extension bundles the JavaScript LSP transport. That transport starts the
actual semantic service as `aura lsp` and looks for the executable in this
order:

- `AURORA_LSP_AURA_PATH`
- `target/debug/aura`
- `target/release/aura`
- `cargo run -q -p aura --` inside the repo workspace
- `aura` on `PATH`

If no compiler command is available, the extension falls back to a small
lexical recovery layer; semantic diagnostics and member intelligence require
the `aura lsp` service.

## Package A VSIX From This Repo

Do not install an existing VSIX without rebuilding it after a server update.
The VSIX is an ignored local artifact and can otherwise contain an older
language-server bundle.

From the repo root:

```bash
npm ci
cargo build -p aura
npm run package:extension
```

The packaging command rebuilds `dist/server.js` from the current
`tools/aurora-language-server/src/server.js` and writes the self-contained
package to `tools/vscode-aurora/aurora-language.vsix`.

To use that exact package immediately:

```bash
code --install-extension tools/vscode-aurora/aurora-language.vsix --force
```

## Install The Packaged VSIX In VS Code

If the `code` shell command is unavailable:

1. Open VS Code and the Extensions view.
2. Open the `...` menu.
3. Choose `Install from VSIX...`.
4. Select:
   - `tools/vscode-aurora/aurora-language.vsix`
5. Run **Developer: Reload Window** when installation finishes.

For Aurora workspaces outside this repository, put `aura` on `PATH` or launch
VS Code with the compiler service path:

```bash
AURORA_LSP_AURA_PATH="/absolute/path/to/aura" code /path/to/aurora-project
```

## Update Workflow While The Language Evolves

When the Aurora language server or extension changes, run
`npm run package:extension`, reinstall the generated VSIX with `--force`, and
reload VS Code. `npm run build:extension` updates `dist/` for development-host
testing but does not update an already installed extension.

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
