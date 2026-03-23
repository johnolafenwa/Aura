# 2026-03-15 Diagnostics, MIR, and LSP Pass

## Goal

Close the open task-board items around diagnostics, frontend expansion, MIR staging, and richer editor features without breaking the working bootstrap path.

## Work Completed

- Extended the bootstrap compiler with more of the frozen v1 control-flow surface:
  - `if`
  - `elif`
  - `else`
  - `while`
  - `break`
  - `continue`
  - booleans
  - strings
  - comparison operators
  - compound assignment
- Added `examples/control_flow.au` as a runnable control-flow sample.
- Added annotated diagnostic rendering so CLI failures now show:
  - error message
  - file/line/column
  - source line
  - caret marker
- Added a first MIR lowering layer with:
  - explicit functions
  - basic blocks
  - instructions
  - terminators
  - a new `aura mir <file.au>` command for inspection
- Expanded the Aurora language server with:
  - hover
  - go-to-definition
  - document diagnostics
  - retained completion and document-symbol support

## LSP Direction

The repo now has a concrete interim decision on LSP architecture:

- keep the packaged VS Code experience on the lightweight in-repo JavaScript analysis for now
- do not couple the extension directly to Rust compiler internals yet
- revisit compiler-backed language intelligence after the compiler exposes stable machine-readable output for AST and diagnostics

This keeps the VSIX build self-contained while the frontend is still changing quickly.

## Verification Results

- `cargo test`
  - passed
- `cargo run -p aura -- run examples/control_flow.au`
  - passed
  - output: `ok`
- `npm run test:lsp`
  - passed

## Notes

This pass intentionally staged MIR without replacing the interpreter. The interpreter remains the execution path while MIR becomes the stable bridge toward future backend work.
