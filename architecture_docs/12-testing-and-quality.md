# Testing And Quality

This chapter explains how Aurora validates the language implementation and why the repo is organized around test-first development.

## The repo-level rule

The repository's `AGENTS.md` makes the default engineering mode explicit: test-first development.

For new features or bug fixes, the expected order is:

1. add or update a failing test
2. implement the change
3. run relevant verification
4. update examples, tutorials, and work logs when the user-facing surface changes

That is not an informal preference. It is an architectural rule for how changes are supposed to land.

## Aurora's test layers

Aurora uses several complementary testing layers.

### Compiler unit tests

These live alongside compiler modules in files such as:

- `lexer_tests.rs`
- `parser_tests.rs`
- `sema_tests.rs`
- `mir_tests.rs`
- `mir_runtime_tests.rs`
- `native_codegen_tests.rs`
- `native_runtime_tests.rs`

These are good for focused behavior in one subsystem.

### Fixture tests

The compiler README documents fixture categories such as:

- parse pass
- parse fail
- check pass
- check fail
- run pass

Fixture tests are especially valuable for language behavior because they look like real Aurora programs.

### Example smoke tests

The example library under `examples/` is maintained, runnable surface area. Aurora treats it as product behavior, not disposable documentation.

### Tooling tests

The language server and extension have their own tests and coverage scripts under `tools/`.

### Coverage gates

The repo root `package.json` includes coverage commands and minimum thresholds for:

- the Rust compiler library
- the language server

## Why this matters architecturally

A language implementation has many cross-cutting surfaces:

- parser behavior
- type checker behavior
- runtime behavior
- CLI behavior
- editor behavior
- docs and tutorial accuracy

Aurora's layered test strategy exists because a single test style will not catch all of those.

## Quality is broader than tests

Aurora also keeps these aligned:

- examples
- tutorials
- READMEs
- work logs
- package docs

That matters because stale examples or stale tutorials are effectively user-facing bugs in a language repo.

## Suggested reading when adding a feature

If you are extending Aurora, the stable order is:

1. add or update a targeted compiler unit or fixture test
2. update any example that should demonstrate the feature
3. update any tutorial chapter that teaches the feature
4. update CLI or tooling docs if command behavior changed
5. run the appropriate verification commands

## Core verification commands

The repo documents and scripts several important commands:

- `cargo test -p aurora-compiler`
- `cargo test -p aura`
- `npm run test:lsp`
- `npm run check:extension`
- `npm run coverage:compiler`
- `npm run coverage:lsp`
- `npm run ci`

Which one is relevant depends on the change.

## Work tracking as quality infrastructure

Aurora also treats `work/task-board.md` and dated notes under `work/` as maintained project state. Large changes are expected to leave a trace there.

That is useful because compiler work often spans:

- implementation detail
- user-facing docs
- follow-up risks
- verification status

## Files to study

- [`AGENTS.md`](../AGENTS.md)
- [`crates/aurora-compiler/README.md`](../crates/aurora-compiler/README.md)
- [`tutorials/README.md`](../tutorials/README.md)
- [`examples/README.md`](../examples/README.md)
- [`package.json`](../package.json)

## What comes next

Read [13-end-to-end-walkthrough.md](13-end-to-end-walkthrough.md) to connect all the architectural layers with one small Aurora program.
