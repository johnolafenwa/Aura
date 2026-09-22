# Installing Aura In VS Code

This guide covers two setups: development from this monorepo, and installing
Aura as a packaged VS Code extension.

## Requirements

- Node.js 22 or later
- VS Code 1.91 or later
- a Rust toolchain, for compiler-backed diagnostics and navigation from the
  repo workspace

## Install From This Repo For Development

1. Open a terminal at the repo root, `Aura/`.
2. Install workspace dependencies with `npm ci`.
3. Build the repo-local compiler server with `cargo build -p aura`.
4. Build the extension bundles with `npm run build:extension`.
5. Verify the editor packages:
   - `npm run check:lsp`
   - `npm run test:lsp`
   - `npm run check:extension`
   - `npm run test:extension`
6. Open the repo in VS Code.
7. Open the extension package folder, `tools/vscode-aura`.
8. Press `F5` to launch an Extension Development Host.
9. In the Extension Development Host, open an Aura file such as
   `examples/classes/point_distance.au`.

The extension activates for `.au` files and starts the bundled Aura language
server.

Step 3 creates the repo-local `target/debug/aura`. It powers compiler-backed
diagnostics, symbols, hover, go-to-definition, and completions. To install the
compiler-owned server on `PATH` for all Aura workspaces, run:

```bash
cargo install --path crates/aura --locked --force
```

This installs the `aura` executable, normally under `~/.cargo/bin`. There is
no second semantic-server executable. The extension starts `aura lsp` over
stdio on its own, so do not start a separate server process by hand.

The extension bundles the JavaScript Language Server Protocol (LSP) transport,
which starts the semantic service as `aura lsp`. It looks for the compiler in this order:

1. `AURA_LSP_AURA_PATH`
2. `target/debug/aura`
3. `target/release/aura`
4. `cargo run -q -p aura --` inside the repo workspace
5. `aura` on `PATH`

If it finds no compiler command, the extension falls back to a small lexical
recovery layer. Semantic diagnostics and member intelligence need the
`aura lsp` service.

## Package A VSIX From This Repo

Rebuild the VSIX after every server update before you install it. The VSIX is
an ignored local artifact, so an old copy can contain an older language-server
bundle.

From the repo root:

```bash
npm ci
cargo build -p aura
npm run package:extension
```

The packaging command rebuilds `dist/server.js` from the current
`tools/aura-language-server/src/server.js`. It writes the self-contained
package to `tools/vscode-aura/aura-language.vsix`.

To install that exact package right away:

```bash
code --install-extension tools/vscode-aura/aura-language.vsix --force
```

## Install The Packaged VSIX In VS Code

If the `code` shell command is unavailable:

1. Open VS Code and the Extensions view.
2. Open the `...` menu.
3. Choose `Install from VSIX...`.
4. Select `tools/vscode-aura/aura-language.vsix`.
5. Run **Developer: Reload Window** when installation finishes.

For Aura workspaces outside this repository, put `aura` on `PATH`, or launch
VS Code with the compiler service path:

```bash
AURA_LSP_AURA_PATH="/absolute/path/to/aura" code /path/to/aura-project
```

## Update Workflow While The Language Evolves

After the Aura language server or extension changes:

1. Run `npm run package:extension`.
2. Reinstall the generated VSIX with `--force`.
3. Reload VS Code.

`npm run build:extension` updates `dist/` for Extension Development Host
testing. It does not update an installed extension.

## Current Scope

The Aura VS Code tooling provides:

- `.au` file recognition
- syntax highlighting
- snippets
- document symbols
- top-level completions
- member completions after `.`
- hover
- go-to-definition
- document diagnostics

The language server comes from `tools/aura-language-server`. The extension
build bundles that server into the extension's own `dist/` output.
