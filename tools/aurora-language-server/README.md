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

The server queries:

- `aura analyze --stdin <virtual-path>`
- `aura complete --line <n> --character <n> [--trigger .] --stdin <virtual-path>`

and caches compiler analysis per document version.

Current lightweight fallback analysis understands:

- top-level functions, classes, and enums
- built-in generic enums `Result` and `Option`
- class fields, methods, and associated methods
- function locals, method `self`, enum match payload bindings, and `for` loop bindings
- builtin helpers such as `print`, `range`, and `float64.sqrt()`

The fallback path is kept for environments where a usable `aura` compiler command is not available yet, and for buffers the compiler cannot parse or type-check while the user is in the middle of editing.

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
  - invokes the Aurora compiler for machine-readable analysis
- `src/analysis.js`
  - fallback Aurora document analysis and completion support

The current direction is:

- keep diagnostics and navigation on compiler-owned analysis
- keep the local analysis layer only as a fallback path rather than the primary semantic engine
