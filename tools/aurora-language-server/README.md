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
- contextual lambda parameter scope, captured-name navigation, callable hover,
  and closure ownership diagnostics
- progressively scoped comprehension targets, including hover, exact
  go-to-definition, nested-clause completion, and owned result-type inference
- incomplete comprehension clauses and filters retain exact `AU1101`
  diagnostics, broad recovery completions, and safe empty hover responses
- owned Vec/String slices use compiler-owned result types, exact endpoint
  diagnostics, retained-source ownership analysis, and hover/navigation for
  names inside base and endpoint expressions
- incomplete or reserved slice forms preserve the compiler's exact `AU2005`
  step/assignment guidance without JavaScript-side reinterpretation
- extern C and opaque-handle symbols, hover, definitions, completions, and
  package-authorization diagnostics

The server starts one persistent compiler service:

- `aura lsp`

Requests and responses are newline-delimited JSON and carry compiler-owned
`semantic_interface_version: 3`. Version 3 adds structural function types and
function-value and closure operands to compiler-owned semantic data. This
identity is distinct from the public
diagnostic document's numeric schema version. The transport rejects and
disposes a compiler with a missing or different semantic identity, invalidates
all cached document analysis, and uses lexical recovery for the failed request;
pre-function-value type metadata therefore cannot survive a compiler upgrade.
Responses remain bounded to 16 MiB.
With a matching compiler, the server caches analysis per document version,
debounces changes, cancels obsolete completion work, guards asynchronous
responses by document version, and invalidates only changed documents and their
dependents.

Compiler diagnostics keep the stable `AU####` code, related source spans,
notes, help, and machine-applicable edits through the LSP mapping. The bridge
does not classify or recreate semantic diagnostics independently.
`Diagnostic.data` also preserves the compiler-owned `call_frames` and
`task_ancestry` arrays. Their frame spans use zero-based `line`,
`start_character`, and `end_character` coordinates and retain each frame's
optional `file_path`; the bridge neither parses human backtrace notes nor
reconstructs paths or ancestry. Updated compiler responses always include both
arrays. Records from an older compatible semantic-interface-v2 compiler that
omit the additive fields are treated as empty arrays. Compile-time diagnostics
normally carry empty frame arrays today, while the populated shape is ready for
editor workflows that present runtime diagnostics.

If the compiler process cannot be started, the lexical recovery layer provides only:

- recovered top-level declarations, extern C functions, opaque handles, and
  nested method declarations
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
