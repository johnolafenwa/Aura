## Goal

Redesign Aurora's public concurrency experience to feel lighter and more Python-friendly while keeping the existing thread-based runtime and compatibility surface working.

## Work Completed

- Added a compatibility-first ergonomic concurrency layer over the existing runtime so the maintained public surface now centers on `Queue[T]`, `queue()`, `tasks()`, `Task.result()`, `TaskGroup.start(...)`, and `Queue.get(timeout=...)`.
- Kept `Channel[T]`, `channel()`, `Task.join()`, and `task_group()` working as compatibility aliases instead of breaking older source programs.
- Made queue and task handles cheap copy-like values in the checker so passing them into spawned work no longer requires `.clone()` in the common case.
- Extended the checker, MIR lowering/runtime, direct backend codegen/runtime, compiler analysis, and JS LSP fallback to understand `put`, `get`, `result`, `start`, `queue`, and `tasks`.
- Updated maintained runnable examples under `examples/concurrency/` to use the new queue/task surface, and added `queue_timeout.au` as the maintained timeout-first example.
- Rewrote the concurrency tutorial and refreshed the maintained docs/READMEs so queues and structured tasks are the primary teaching path instead of the older Go-like channel wording.
- Updated compiler fixtures and helper tests to match the new public diagnostic wording (`Queue[...]` rather than `Channel[...]`) while preserving compatibility behavior.

## Verification

- `cargo fmt --all`
- `cargo test -p aurora-compiler`
- `cargo test -p aura`
- `npm run test:lsp`
- `npm run check:extension`

## Follow-up

- The lower-level `select`, `spawn detached`, and compatibility `Channel[...]` / `channel()` surface still exist. If we want to push further toward a Python-first experience, the next coherent pass would be bounded queues, `Task.result(timeout=...)`, and higher-level `wait_any` / `wait_all` helpers so ordinary coordination code reaches for `select` less often.
