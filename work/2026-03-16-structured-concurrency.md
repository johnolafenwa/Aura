# 2026-03-16 Structured Concurrency

## Scope

Implemented the remaining structured-concurrency bootstrap slice from the Aurora proposal:

- `task_group()`
- `with task_group() as group:`
- `group.spawn(...)`
- `group.cancel()`
- `cancelled()`
- `select:`
- `case binding = channel.recv():`
- `case after(5ms):`
- duration literals: `ms`, `s`, `m`

## Compiler and Runtime

- Added parser and lexer support for:
  - `select`
  - `with expr as name:`
  - duration literals
  - keyword member access for `group.spawn(...)`
- Added semantic checking for:
  - built-in `TaskGroup` and `Duration`
  - `task_group()` and `cancelled()`
  - `TaskGroup.spawn` and `TaskGroup.cancel`
  - `select` arm validation
- Added runtime support for:
  - task-group-managed child tasks
  - cooperative cancellation flags
  - task-group cleanup on `with` exit
  - non-blocking `recv()` probing for `select`
  - timer-based `after(...)` arms

## Examples and Tutorials

- Added:
  - `examples/concurrency/task_group_select.au`
  - `examples/concurrency/task_group_cancel.au`
  - `examples/concurrency/select_timeout.au`
- Updated:
  - `examples/README.md`
  - `tutorials/12-concurrency.md`
  - `tutorials/README.md`

## Tests

- Added parse/check/run fixtures for structured concurrency.
- Added a new `check-fail` fixture for invalid `select` binding on `after(...)`.
- Added LSP tests for:
  - `task_group` and `cancelled` builtins
  - task-group member completion
  - select-arm binding hover/type inference

## Verification

- `cargo test`
- `cargo run -p aura -- run examples/concurrency/task_group_select.au`
- `cargo run -p aura -- run examples/concurrency/task_group_cancel.au`
- `cargo run -p aura -- run examples/concurrency/select_timeout.au`
- `cargo run -p aura -- mir examples/concurrency/task_group_select.au`
- `npm run test:lsp`
- `npm run check:extension`
- `npm run test:extension`
