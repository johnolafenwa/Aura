# 2026-04-13 Coverage Ratchet Follow-Up

## Goal

Push the compiler and language-server coverage floors higher after the cross-file tooling and trait milestone, while keeping the new coverage work test-first and low-risk.

## Work Completed

- Added direct fallback-analysis helper coverage for nested top-level comma/colon splitting, including escaped strings and nested delimiter handling.
- Added compiler-bridge regression coverage for non-file URI subprocess calls, empty-diagnostic/symbol defaults, fallback workspace-root handling, Windows-style URI path conversion, and empty-stderr subprocess failures.
- Added focused compiler unit coverage for:
  - lexer tokenization, indentation handling, string/f-string escapes, numeric and duration literals, and lexer diagnostics
  - call binding and builtin metadata/binding surfaces across builtin functions and builtin members
  - AST `Item::name()` dispatch
- Brought `tools/aurora-language-server/src/compiler_bridge.js` to full statement/line/function/branch coverage.
- Raised enforced coverage gates again:
  - compiler: lines `68%`, functions `74%`, regions `68%`
  - language server: statements `91%`, branches `82%`, functions `98%`, lines `91%`

## Verification

- `cargo test -p aurora-compiler`
- `npm run test:lsp`
- `cargo llvm-cov -p aurora-compiler --summary-only`
- `npm run coverage:lsp`

Measured coverage at verification time:

- compiler: lines `68.15%`, functions `74.38%`, regions `68.37%`
- language server: statements `91.24%`, branches `82.08%`, functions `98.75%`, lines `91.24%`

## Follow-Up

- The remaining compiler gap is still dominated by large semantic/runtime modules: `native_codegen`, `analysis`, `interpreter`, `sema`, `mir_runtime`, and `native_runtime`.
- The remaining LSP gap is now concentrated almost entirely in `analysis.js` fallback branches; continue shrinking or replacing that layer rather than just adding broad line coverage around it.
- Reaching literal 100% enforced coverage from here is a substantial multi-pass effort, not a small ratchet.
