# 2026-03-15 VS Code Extension Session

## Goal

Add VS Code support inside the Aurora repository, switch it onto a real language-server architecture, and make the VS Code install path work cleanly from this monorepo.

## Planned Scope

- create `tools/vscode-aurora`
- create `tools/aurora-language-server`
- register the `.au` language
- add syntax highlighting and language configuration
- add snippets for common Aurora constructs
- add LSP-backed Aurora-aware completions, especially after `.`
- add basic structural editor support such as document symbols

## Notes

The first package pass started as editor-local analysis, but the repo now moves to an LSP split so the VS Code side can stay thin.

The extension/server split should be good enough to:

- open Aurora files with the right language mode
- highlight the syntax from the current proposal
- show class/field/function-aware completions
- offer member suggestions after `.` for obvious local cases

The long-term direction is still to replace or augment the current JavaScript analysis with compiler-backed language intelligence while preserving the LSP surface.

The first LSP packaging attempt exposed a monorepo problem: `vsce` followed hoisted workspace dependencies back to the repo root and produced an invalid VSIX. The current fix is to bundle both the extension client and server into `tools/vscode-aurora/dist/` so packaged installs stay self-contained.

## Repo Integration

- added a root `package.json` with npm workspaces for repo-managed tools
- kept the Rust compiler side on the existing Cargo workspace
- documented the monorepo layout in the root README
- split editor tooling into `tools/vscode-aurora` and `tools/aurora-language-server`
- added a bundled build so the VS Code extension carries self-contained client/server artifacts for VSIX packaging

## Verification Results

- `npm run check:lsp` passed
- `npm run test:lsp` passed
- `npm run check:extension` passed
- `npm run test:extension` passed
- `cargo test` passed
- `npm run package:extension` passed and produced `tools/vscode-aurora/aurora-language.vsix`

## Implemented Features

- `.au` language registration
- Aurora language configuration for comments, indentation, pairs, and folding
- TextMate grammar for current Aurora syntax
- snippets for `class`, `def`, `if`, `match`, and `with`
- VS Code extension as an LSP client
- Aurora language server package
- member completion after `.`
- document symbols for classes, fields, methods, and functions
- bundled extension build output in `tools/vscode-aurora/dist/`
- dedicated VS Code install guide in `tools/vscode-aurora/INSTALL.md`
