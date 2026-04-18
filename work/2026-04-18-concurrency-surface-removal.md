# 2026-04-18 Concurrency Surface Removal

Start time: 2026-04-18 06:44:09 BST
Completion time: 2026-04-18 07:30:14 BST
Elapsed: 0h 46m

## Goal

Remove the old concurrency compatibility layer completely so the maintained Aurora surface only exposes:

- `Queue[T]`
- `queue()`
- `tasks()`
- `Task.result()`
- `TaskGroup.start(...)`
- `Queue.put(...)`
- `Queue.get(...)`

## Work Completed

- Removed the remaining compatibility-only concurrency spellings from the maintained compiler surface in `sema.rs`, including:
  - `Channel[T]`
  - `channel()`
  - `task_group()`
  - `Queue.send(...)`
  - `Queue.recv()`
  - `Queue.clone()`
  - `Task.join()`
  - `Task.clone()`
  - `TaskGroup.spawn(...)`
- Removed the compatibility-era special-case checker diagnostics that suggested replacement spellings, so the deleted forms now fail naturally as unknown names/types or unsupported members.
- Renamed maintained example files from `channels_spawn.au` / `channel_iteration.au` to `queues_spawn.au` / `queue_iteration.au`.
- Renamed positive compiler fixtures with stale `channel*` / `channels*` stems to queue-oriented names, including explicit-type and closed-queue/select coverage.
- Kept explicit negative regression fixtures for removed aliases so the deleted spellings stay rejected.
- Updated stale parser/checker/run fixtures that still passed queue handles via `.clone()` or otherwise referenced the removed compatibility surface.
- Updated LSP fallback tests to stop asserting removed names and to use queue/task-only example paths and builtin metadata.
- Cleaned remaining wording drift in internal runtime/test strings and the maintained examples README so the repo no longer advertises the removed spellings outside explicit negative regression coverage.

## Verification

- `cargo fmt --all`
- `cargo test -p aurora-compiler`
- `cargo test -p aura`
- `npm run test:lsp`
- `npm run check:extension`

## Follow-up

- None.
