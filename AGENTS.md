# AGENTS.md

This repository is a monorepo for the Aurora language, compiler, examples, tutorials, and editor tooling.

The default engineering mode for this repo is test-first development.

## Core Rule

Before implementing a new feature or fixing a bug:

1. add or update a failing test
2. implement the change
3. run the relevant verification commands
4. update examples, tutorials, and work logs when the user-facing surface changes

Do not treat examples and tutorials as passive documentation. They are part of the maintained surface of the project.

## Required Updates When Behavior Changes

If a language or tooling behavior changes, update these in the same pass when relevant:

- compiler tests
- language-server tests
- examples under `examples/`
- tutorials under `tutorials/`
- package or root README files
- `work/task-board.md`
- a dated note under `work/`

## Package Expectations

### `crates/aurora-compiler`

Use layered tests:

- unit tests for lexer, parser, checker, interpreter, and MIR helpers
- fixture tests for parse/check/run/diagnostic behavior
- regression tests for every reported compiler bug
- example smoke tests for runnable language features

When adding a feature, prefer adding fixtures first.

### `crates/aura`

Treat CLI behavior as product behavior:

- validate command success paths
- validate annotated diagnostic output
- keep command examples in README files current

### `tools/aurora-language-server`

The LSP must have regression tests for:

- diagnostics
- completions
- hover
- go-to-definition
- scope handling
- real example files that previously broke

Use `npm run coverage:lsp` regularly and move the package toward enforced 100% coverage before expanding the semantic surface further.

### `tools/vscode-aurora`

Keep the extension thin and test packaging/build behavior whenever the LSP surface changes.

## Tutorials And Examples

The `tutorials/` directory should track the implemented subset of Aurora, not just the proposal.

The `examples/` directory should stay categorized, runnable, and aligned with tutorial chapters.

If a feature is not implemented in the compiler, do not teach it as if it exists.

## Work Tracking

Keep `work/task-board.md` current.

For substantial work, add a dated note under `work/` describing:

- goal
- work completed
- verification
- follow-up
