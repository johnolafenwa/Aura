# 2026-03-22 Module-Aware Compiler Analysis

## Goal

Close the tooling gap where `run`, `check`, and `build` understood local modules, but compiler-backed `analyze` / `complete` and the LSP bridge still treated stdin-backed editor buffers as source-only files with no import graph.

## Work Completed

- Added module-aware compiler analysis and completions for file-backed and stdin-backed paths by routing `aura analyze` and `aura complete` through the same module loader path used by checking and execution.
- Added public compiler APIs for path-aware analysis/completion from an overridden entry source.
- Taught compiler analysis to understand imported module namespaces directly for:
  - hover
  - diagnostics
  - member completions after `.`
  - top-level module-name completions
- Added product regression tests in `crates/aura/tests/cli.rs` for:
  - `analyze --stdin <real-path>` with local imports
  - `complete --stdin <real-path>` with module member completion
- Added language-server regression coverage in `tools/aurora-language-server/test/compiler_bridge.test.js` using `examples/modules/simple_import.au`.
- Updated tooling docs and the task board to reflect that local imports now participate in compiler-backed analysis/completions.

## Verification

- `cargo test -p aura --test cli analyze_stdin_resolves_local_module_imports -- --exact`
- `cargo test -p aura --test cli complete_stdin_resolves_local_module_member_completions -- --exact`
- `cargo test -p aurora-compiler --test modules -- --nocapture`
- `npm run test:lsp -- compiler_bridge`

## Follow-Up

- Extend compiler-owned definition data so imported items can navigate across files accurately instead of falling back to same-document assumptions.
- Keep narrowing the JS fallback now that more real editor behavior runs through compiler-owned analysis.
