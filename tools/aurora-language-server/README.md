# Aurora Language Server

This package contains the Aurora Language Server Protocol implementation.

Current LSP features:

- completion items
- member completion after `.`
- document symbols

The current implementation uses a lightweight Aurora-aware analysis pass over the open document. It is intentionally simple, but it lives behind LSP boundaries now so the analysis can evolve without changing the VS Code extension contract.

## Development

From the repo root:

- `npm install`
- `npm run check:lsp`
- `npm run test:lsp`

If you want the VS Code extension package to carry the current language server implementation, also run:

- `npm run build:extension`

## Architecture

- `src/server.js`
  - LSP transport and request handlers
- `src/analysis.js`
  - Aurora document analysis used by the server

The long-term direction should be to replace or augment this analysis layer with compiler-backed semantic information while preserving the LSP surface.
