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

## Persistence And Stop Conditions

When the user gives an explicit completion condition such as:

- "do not stop until it is fully done"
- "keep going until X reaches 100%"
- "take as much time as you need"
- or any request that clearly authorizes multi-hour or multi-day work

then treat that condition literally.

In those cases:

- do not stop at an internal milestone just because the remaining work is large
- do not substitute a "reasonable stopping point" for the user's stated target
- do not pause only to report partial progress unless the user asked for a checkpoint
- do not make a judgment call that the rest should be a separate project if the user explicitly asked to continue
- for substantial work, record the session start time in `work/task-board.md` before deep implementation work begins
- keep the active session entry updated with elapsed wall-clock time while the work is in progress
- do not stop for time-budget reasons unless the session has reached 12 continuous hours of work

Only stop before the stated target is reached if one of these is true:

- the user redirects or cancels the work
- there is a real blocker that cannot be resolved through normal implementation, testing, or local investigation
- the next step would be destructive, irreversible, or otherwise unsafe without confirmation
- the work session has reached 12 continuous hours without completion

If you do hit a blocker, say exactly what the blocker is, what was attempted, and what decision or missing resource is preventing further progress.

If the 12-hour limit is reached before completion, record the stop time, total elapsed time, remaining work, and exact stop reason in both `work/task-board.md` and the dated work note for that pass.

For quantitative targets, partial improvement is not completion. For example, if the user says to reach 100% coverage, raising the coverage floor is useful progress but it is not the end state.

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

For substantial work, use an active work-session entry in `work/task-board.md` while the work is live. That entry must include:

- the exact local start time
- the current elapsed wall-clock time
- the current target or task being worked
- the stop rule: complete the work or reach 12 continuous hours

When the work is complete, clear the active session entry from `work/task-board.md`. Do not leave stale active-timer information behind after completion.

For substantial work, add a dated note under `work/` describing:

- goal
- work completed
- verification
- follow-up

When a substantial work session starts, the dated note for that pass should also capture the session start time. If the work stops because the 12-hour limit was reached, the dated note must record the stop time and total elapsed time as well.
