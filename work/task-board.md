# Task Board

Last updated: 2026-03-15

## In Progress

- Expand the frontend beyond the first milestone subset without breaking the working bootstrap path.
- Improve diagnostics so parse/type errors include better source context.
- Decide the next concrete implementation slice after the bootstrap: control flow, more types, or richer declarations.
- Evolve the Aurora language server from lightweight single-document analysis toward richer compiler-backed intelligence.
- Expand the LSP feature set beyond completion/document symbols without breaking the bundled VS Code install path.

## Todo

- Add parser and checker support for more of the frozen v1 syntax, starting with control flow.
- Add richer builtin/type support and clearer semantic errors for unimplemented features.
- Stage MIR/codegen work after the frontend bootstrap is stable enough to stop changing every pass.
- Decide when the language server should start consuming compiler data structures directly instead of its current lightweight analysis pass.
- Add richer LSP features such as hover, go-to-definition, and diagnostics backed by compiler semantics.

## Done

- Added a Rust workspace root with `aurora-compiler` and `aura`.
- Added the first compiler modules: diagnostics, AST, lexer, parser, semantic checker, and evaluator.
- Added the first milestone sample program at `examples/point.au`.
- Added `examples/README.md` with instructions for running, checking, and inspecting example programs.
- Added in-repo work tracking under `work/`.
- Verified `cargo test` passes.
- Verified `cargo run -p aura -- run examples/point.au` prints `5`.
- Added `tools/vscode-aurora` as an in-repo VS Code extension package.
- Added `tools/aurora-language-server` as an in-repo LSP package.
- Added a root npm workspace manifest for repo-managed tools.
- Verified the VS Code extension analysis/tests with `npm run check:extension` and `npm run test:extension`.
- Switched the VS Code package from local editor analysis to an LSP client.
- Added a bundled `dist/` build for the VS Code extension so VSIX packaging stays self-contained inside the monorepo.
- Verified `npm run package:extension` produces `tools/vscode-aurora/aurora-language.vsix`.

## Blocked

- None currently.
