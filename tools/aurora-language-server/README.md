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

If the compiler process cannot be started, the lexical recovery layer provides only:

- recovered top-level declarations and nested method declarations
- top-level keywords, builtins, and recovered declaration completions
- same-file hover and definition for recovered declarations

The recovery path deliberately has no semantic diagnostics or member inference. Incomplete buffers are normally handled by compiler recovery; JavaScript no longer carries a second Aurora type system.

## Development

From the repo root:

- `npm install`
- `npm run check:lsp`
- `npm run test:lsp`
- `npm run coverage:lsp`

If you want the VS Code extension package to carry the current language server implementation, also run:

- `npm run build:extension`

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
