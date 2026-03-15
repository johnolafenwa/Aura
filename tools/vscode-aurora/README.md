# Aurora VS Code Extension

This extension lives in the Aurora monorepo so editor tooling evolves alongside the language.

Current features:

- `.au` language registration
- syntax highlighting via TextMate grammar
- indentation/comment/bracket language configuration
- snippets for common Aurora constructs
- LSP client for the Aurora language server
- top-level keyword/class/function completions
- member completions after `.`
- document symbols for classes, fields, methods, and functions

The language intelligence currently comes from the in-repo `aurora-language-server` package.

The VS Code extension now bundles its client and server entrypoints into `tools/vscode-aurora/dist/`, so packaged VSIX installs do not depend on sibling files outside the extension folder.

Current completion scope is intentionally lightweight. The language server understands:

- top-level classes and functions
- class fields and methods
- function parameters and simple local bindings
- constructor-style type inference such as `p = Point(...)`
- basic builtin members like `float64.sqrt`

## Install In VS Code

### Development install from this repo

1. Install dependencies from the repo root:
   - `npm install`
2. Build the extension bundle:
   - `npm run build:extension`
3. Verify the language server and extension packages:
   - `npm run check:lsp`
   - `npm run test:lsp`
   - `npm run check:extension`
   - `npm run test:extension`
4. Open the repository in VS Code.
5. Open the extension package folder:
   - `tools/vscode-aurora`
6. Press `F5` in VS Code to launch an Extension Development Host.
7. In the Extension Development Host, open an `.au` file such as:
   - `examples/point.au`

That will start the Aurora language server automatically and enable syntax highlighting, completions, and document symbols.

### Install as a packaged extension

1. From the repo root, install dependencies:
   - `npm install`
2. Package the extension:
   - `npm run package:extension`
3. In VS Code:
   - open the Extensions view
   - open the `...` menu
   - choose `Install from VSIX...`
   - select `tools/vscode-aurora/aurora-language.vsix`

For a step-by-step guide inside the repo, see `tools/vscode-aurora/INSTALL.md`.

### Requirements

- Node.js 22 or later is recommended for the current workspace scripts.
- VS Code 1.90 or later.

As the compiler grows, this extension should stay a thin client while the Aurora language server becomes the canonical place for editor intelligence.
