# 2026-04-13 Cross-File Tooling, Coverage, and Traits Milestone

## Goal

Finish the current stability/tooling milestone by:

- making compiler-backed navigation correct across imported files
- shrinking the JS language-server fallback so compiler analysis owns more editor behavior
- raising the enforced compiler and LSP coverage gates
- extending the maintained trait surface with specialized generic trait bounds and operator traits

## Work Completed

- Added source-path-aware compiler analysis ranges so imported functions, classes, enums, fields, methods, variants, and trait methods now resolve back to their defining files across `aura analyze`, compiler-backed LSP definition, and cross-file navigation.
- Updated the language server to prefer compiler-backed hover and definition whenever compiler analysis succeeds, keeping the JS layer as fallback only when the compiler cannot analyze the current buffer.
- Refactored generic trait bounds from raw trait-name lists into structured trait bounds with generic trait arguments, and extended the checker, analysis, interpreter, MIR lowering/runtime, and direct backend to support specialized generic trait bounds such as `T: Mapper[int32]`.
- Added operator-trait dispatch for the maintained operator subset so trait methods can back unary and binary operators across checking, interpretation, MIR execution, and direct builds.
- Added maintained examples, fixture coverage, example smoke coverage, and CLI product coverage for specialized generic trait bounds and operator traits.
- Expanded compiler and LSP regression/unit coverage, including direct runtime helper coverage and compiler-bridge/server coverage for the new compiler-owned paths.
- Raised the enforced coverage gates to the new measured safe floors:
  - compiler: lines `67%`, functions `74%`, regions `67%`
  - language server: statements `89%`, branches `78%`, functions `98%`, lines `89%`
- Fixed the direct-build runtime-library staging flake in the CLI build path so full `cargo test -p aura` runs stay stable.

## Verification

- `cargo fmt --all`
- `cargo test -p aurora-compiler`
- `cargo test -p aura`
- `npm run test:lsp`
- `npm run check:extension`
- `cargo llvm-cov -p aurora-compiler --summary-only`
- `npm run coverage:lsp`

Measured coverage at verification time:

- compiler: lines `67.45%`, functions `74.12%`, regions `67.65%`
- language server: statements `89.67%`, branches `78.00%`, functions `98.75%`, lines `89.67%`

## Follow-Up

- Keep pushing compiler coverage upward, especially in `native_codegen`, `analysis`, `interpreter`, and `sema`.
- Continue shrinking the JS analysis fallback toward recovery-only behavior as compiler-backed completion and invalid-buffer support improves further.
- Extend the trait system beyond the current specialized generic bounds and operator-trait subset toward the remaining proposal surface.
