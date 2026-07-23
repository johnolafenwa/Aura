# VS Code completion server refresh

## Goal

Fix the reported `textDocument/completion` failure while editing an incomplete
function parameter annotation, and make the installation documentation explain
how to install the actual current compiler-backed language server.

## Work completed

- Reproduced the exact `Cannot read properties of null (reading '1')` failure
  against the language-server bundle installed with extension 0.1.0.
- Confirmed that the installed VSIX contained an obsolete fallback parser while
  the current source tree already handles the incomplete header safely.
- Added a framed stdio JSON-RPC regression that opens the reported buffer,
  requests completion immediately after the incomplete parameter colon, and
  pins the observable completion response.
- Bumped the extension to 0.1.1, rebuilt its server and client bundles, packaged
  a fresh VSIX, and force-installed it in VS Code.
- Updated the root, language-server, and extension READMEs plus the extension
  install guide with the full `aura lsp` build/discovery model, exact packaging
  and force-install commands, an explicit `cargo install` command for the actual
  compiler-owned server, external-workspace configuration, stale-VSIX warning,
  and required VS Code reload.

## Verification

- `npm run test:lsp` — 57 tests pass.
- `npm run coverage:lsp:check` — 100% statements, branches, functions, and
  lines.
- `npm run check:extension` — passes.
- `npm run test:extension` — 9 tests pass.
- The documented `cargo install --path crates/aura --locked --force` server
  installation path succeeds in an isolated offline root, and the installed
  binary reports `aura 0.1.0`.
- The protocol regression passes against the freshly packaged VSIX server.
- The protocol regression passes against the installed extension 0.1.1 server.
- `git diff --check` — passes.
- `npm run ci` — passes on the complete working tree, including compiler,
  parity, LSP, extension, reference, audit, coverage, Clippy, and hygiene
  gates.

## Follow-up

An already open VS Code window must run **Developer: Reload Window** once so it
replaces any 0.1.0 server process still held in memory. Future server updates
must rebuild the VSIX, reinstall it with `--force`, and reload the window.
