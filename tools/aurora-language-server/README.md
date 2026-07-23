# Aurora Language Server

This package contains the Aurora Language Server Protocol implementation.

Current LSP features:

- completion items
- member completion after `.`
- document symbols
- hover
- go-to-definition
- document diagnostics

Current compiler-backed analysis covers:

- completion items
- document diagnostics
- document symbols
- hover
- go-to-definition

The server starts one persistent compiler service:

- `aura lsp`

Requests and responses are newline-delimited JSON. The server caches compiler analysis per document version, debounces changes, cancels obsolete completion work, guards asynchronous responses by document version, and invalidates only changed documents and their dependents.

Compiler diagnostics keep the stable `AU####` code, related source spans,
notes, help, and machine-applicable edits through the LSP mapping. The bridge
does not classify or recreate semantic diagnostics independently.

If the compiler process cannot be started, the lexical recovery layer provides only:

- recovered top-level declarations and nested method declarations
- top-level keywords, builtins, and recovered declaration completions
- same-file hover and definition for recovered declarations

The recovery path deliberately has no semantic diagnostics or member inference. Incomplete buffers are normally handled by compiler recovery; JavaScript no longer carries a second Aurora type system.

## Development

From the repo root:

- `npm ci`
- `cargo build -p aura`
- `npm run check:lsp`
- `npm run test:lsp`
- `npm run coverage:lsp`

The build command provides `target/debug/aura` while working in this checkout.
To install the actual compiler-owned server binary on `PATH` for use from any
Aurora workspace, run:

```bash
cargo install --path crates/aura --locked --force
```

That installs the `aura` executable; the editor launches its `aura lsp`
subcommand over stdio. You do not need to run `aura lsp` separately.

The VS Code extension bundles this package's JavaScript transport, which then
starts `aura lsp` for compiler-owned semantic analysis. After installing or
building `aura` as described above, rebuild, package, and force-install the
transport so VS Code does not keep an older local VSIX:

```bash
npm run package:extension
code --install-extension tools/vscode-aurora/aurora-language.vsix --force
```

Run **Developer: Reload Window** afterward. For a workspace outside this
repository, put `aura` on `PATH` or set `AURORA_LSP_AURA_PATH` to its absolute
path before launching VS Code.

## Architecture

- `src/server.js`
  - LSP transport and request handlers
- `src/compiler_bridge.js`
  - owns the persistent compiler process and machine-readable request lifecycle
- `src/recovery.js`
  - lexical compiler-unavailable recovery only

The current direction is:

- keep diagnostics and navigation on compiler-owned analysis
- keep recovery lexical so semantic behavior has exactly one implementation
