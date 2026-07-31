# Aurora VS Code Extension

This extension lives in the Aurora monorepo so editor tooling evolves alongside the language.

Current features:

- `.au` language registration
- syntax highlighting via TextMate grammar
- indentation/comment/bracket language configuration
- Aurora-specific Enter handling so block indentation behaves predictably in off-side code
- snippets for common Aurora constructs
- LSP client for the Aurora language server
- top-level keyword/class/function completions
- member completions after `.`
- document symbols for classes, fields, methods, and functions
- hover for classes, functions, locals, fields, and builtin members
- go-to-definition for local and top-level symbols
- document diagnostics for duplicate declarations and obvious unknown names/members
- `lambda parameters: expression` highlighting and snippet support, with
  compiler-owned lambda scope, hover, completion, and capture diagnostics
- `extern "C" def` and `extern "C" opaque class` highlighting and snippets,
  with compiler-owned symbols, hover, definitions, completions, and
  diagnostics

The language intelligence comes from the in-repo `aurora-language-server`
package. The extension bundles its JavaScript LSP transport and that transport
starts the compiler-owned semantic service as `aura lsp`.

The VS Code extension now bundles its client and server entrypoints into `tools/vscode-aurora/dist/`, so packaged VSIX installs do not depend on sibling files outside the extension folder.

Current completion scope is intentionally lightweight. The language server understands:

- top-level classes, functions, extern C functions, and opaque handles
- top-level enums and enum variants
- built-in `Result` and `Option` variants
- class fields and methods
- function parameters and simple local bindings
- contextually typed lambda parameters and captured outer locals
- method `self`, enum match payload bindings, and `for` loop bindings
- comprehension targets in filters, nested iterables, and output expressions,
  with exact target navigation and no leakage after the comprehension
- incomplete comprehension clauses and filters keep teaching diagnostics and
  broad completions available without stale hover metadata or server failures
- constructor-style type inference such as `p = Point(...)`
- basic builtin helpers such as `range` and `float64.sqrt`

## Install In VS Code

### Development install from this repo

1. Install dependencies from the repo root:
   - `npm ci`
2. Build the repo-local compiler server:
   - `cargo build -p aura`
3. Build the extension bundle:
   - `npm run build:extension`
4. Verify the language server and extension packages:
   - `npm run check:lsp`
   - `npm run test:lsp`
   - `npm run check:extension`
   - `npm run test:extension`
5. Open the repository in VS Code.
6. Open the extension package folder:
   - `tools/vscode-aurora`
7. Press `F5` in VS Code to launch an Extension Development Host.
8. In the Extension Development Host, open an `.au` file such as:
   - `examples/classes/point_distance.au`

That will start the Aurora language server automatically and enable syntax highlighting, completions, hover, go-to-definition, diagnostics, and document symbols.

### Install the current server as a packaged extension

Always regenerate the VSIX after changing or updating the language server.
`aurora-language.vsix` is an ignored local build artifact, so a file left from
an earlier checkout may contain a stale server.

From the repository root:

```bash
npm ci
cargo build -p aura
cargo install --path crates/aura --locked --force
npm run package:extension
code --install-extension tools/vscode-aurora/aurora-language.vsix --force
```

`cargo install` installs the actual compiler-owned server as the `aura`
executable (normally under `~/.cargo/bin`); the extension starts its `aura lsp`
subcommand automatically. The preceding `cargo build` also refreshes the
repo-local `target/debug/aura`, which has priority while this repository is
open.

Then run **Developer: Reload Window** in VS Code and reopen an `.au` file. If
the `code` shell command is unavailable, open the Extensions view, choose
**… → Install from VSIX…**, select
`tools/vscode-aurora/aurora-language.vsix`, and reload when prompted.

Inside this repository the server finds `target/debug/aura` automatically.
For a workspace elsewhere, put `aura` on `PATH` or launch VS Code with the
compiler path made explicit:

```bash
AURORA_LSP_AURA_PATH="/absolute/path/to/aura" code /path/to/aurora-project
```

For a step-by-step guide inside the repo, see `tools/vscode-aurora/INSTALL.md`.

### Requirements

- Node.js 22 or later is recommended for the current workspace scripts.
- VS Code 1.91 or later.

As the compiler grows, this extension should stay a thin client while the Aurora language server becomes the canonical place for editor intelligence.
