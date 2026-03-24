# Aurora Testing Strategy

This document defines the testing model for the Aurora repository.

The goal is not just high test counts. The goal is early failure when language behavior, diagnostics, or editor tooling regress.

## Principles

### 1. Test First

For new language features and bug fixes:

1. add a failing test or fixture first
2. implement the feature or fix
3. verify the full relevant surface

### 2. Test The Product Surface

For a language project, product behavior includes:

- parse results
- type-checking behavior
- runtime behavior
- MIR lowering behavior
- diagnostics
- CLI output
- LSP/editor behavior
- runnable examples

### 3. Regression Tests Are Mandatory

Every bug fix should add a regression test that reproduces the real failing case as directly as possible.

If a bug appeared in a real example file, test that real example file.

## Test Layers

### Compiler Library: `crates/aurora-compiler`

Use these layers together:

- unit tests for small helper logic
- fixture tests for parser/checker/runtime/diagnostic behavior
- example smoke tests for maintained `.au` programs
- MIR tests for structural lowering invariants

Recommended fixture categories:

- `parse-pass`
- `check-pass`
- `check-fail`
- `run-pass`
- `mir-pass`

### CLI: `crates/aura`

CLI tests should verify:

- command success
- command failure
- annotated diagnostics
- stable command semantics for `check`, `run`, `ast`, `ast-json`, `analyze`, `complete`, and `mir`

### Language Server: `tools/aurora-language-server`

The LSP test suite should cover:

- compiler-bridge behavior
- completions
- diagnostics
- hover
- go-to-definition
- scope tracking
- parenthesized receiver/member resolution
- real example files

Coverage should be measured continuously. The package is moving toward enforced 100% coverage before its semantic surface expands further.

### VS Code Extension: `tools/vscode-aurora`

Extension tests should stay focused on:

- build integrity
- bundle integrity
- packaged install integrity

The semantic logic belongs in the language server, not in the extension package.

### Examples And Tutorials

Examples are part of the supported implementation surface and should be exercised in tests where practical.

Tutorials are not executable themselves, but each tutorial chapter should point to maintained examples that are runnable in CI or local verification.

## Coverage Policy

Coverage is useful when tied to stable modules and real behavior tests.

Current direction:

- mature packages should move toward 100% enforced coverage
- fast-changing packages may temporarily operate below that threshold, but the gap must be visible and intentional
- every regression should add direct behavior coverage, not only line coverage

Current repo commands:

- compiler and examples
  - `cargo test`
- compiler coverage
  - `npm run coverage:compiler`
  - `npm run coverage:compiler:check`
- language server coverage
  - `npm run coverage:lsp`
  - `npm run coverage:lsp:check`
- full repo gate
  - `npm run ci`

Current enforced floor:

- compiler
  - lines: `71%`
  - functions: `75%`
  - regions: `74%`
- language server
  - statements: `87%`
  - branches: `78%`
  - functions: `90%`
  - lines: `87%`

These are baseline non-regression gates, not the final target. The project is still moving toward enforced 100% coverage as the implementation stabilizes.

## Workflow For A New Feature

When adding a new Aurora feature:

1. add a failing compiler fixture
2. add a failing runtime or diagnostic fixture if needed
3. add or update an example
4. add or update an LSP test if editor behavior should change
5. implement the compiler/runtime/tooling change
6. update tutorial chapters that teach the feature
7. record the work in `work/`

## Workflow For A Bug Fix

When fixing a bug:

1. reproduce it with a failing test first
2. prefer testing the real file or exact expression that failed
3. fix the behavior
4. run the relevant package test suites
5. record the regression in `work/` if it was non-trivial
