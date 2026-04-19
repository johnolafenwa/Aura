## Goal

Replace Aurora's mixed concurrency surface with one structured, Pythonic model only:

- `Queue[T]()`
- `TaskGroup()`
- `TaskGroup.start(...)`
- `TaskGroup.start_soon(...)`
- explicit wait helpers
- redesigned queue receive/send outcomes

Remove the older spellings and model completely:

- `spawn`
- `spawn detached`
- `select`
- `queue()`
- `queue[T]()`
- `tasks()`

## Session

- Start: 2026-04-19 15:07:46 BST
- Completed: 2026-04-19 18:44:09 BST

## Work Completed

- Removed the legacy concurrency surface from the maintained language, compiler, tooling, examples, and docs: `spawn`, `spawn detached`, `select`, `after(...)`, `queue()`, `queue[T]()`, and `tasks()` are no longer part of the public Aurora model.
- Kept one structured concurrency model only: `Queue[T]()`, `TaskGroup()`, `TaskGroup.start(...)`, `TaskGroup.start_soon(...)`, `Task.result(timeout=...)`, `wait_any(...)`, and `wait_all(...)`.
- Renamed and rewired maintained example and fixture files from the old `spawn` / `select` naming and behavior to the new queue/task semantics, including the queue timeout, task-group wait-helper, and module-crossing task-start coverage.
- Updated compiler lowering/runtime terminology away from the old spawn/detached model by renaming the MIR/direct task-start machinery and removing dead direct-runtime helpers that only existed for the removed `select`-era lowering.
- Updated the fallback language-server analysis metadata and inference to the maintained `QueueReceive[T]`, `TaskResult[T]`, `WaitAny[T]`, `WaitAll[T]`, and expanded `SendError[T]` surfaces, including payload hover inference inside `match` arms.
- Updated READMEs, tutorials, examples, VS Code syntax/snippets/indentation, and negative regressions so the maintained docs/tooling now describe only the new structured Pythonic concurrency surface.

## Verification

- `cargo fmt --all`
- `cargo test -p aurora-compiler`
- `cargo test -p aura`
- `npm run test:lsp`
- `npm run check:extension`
- `cargo clippy -p aurora-compiler -p aura -- -D clippy::correctness`

## Follow-up

- None.
