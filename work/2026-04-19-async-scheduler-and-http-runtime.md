# 2026-04-19 Async Scheduler And HTTP Runtime

- Session start: 2026-04-19 09:24:09 BST
- Session complete: 2026-04-19 10:07:40 BST
- Elapsed: 0h 43m

## Goal

- Replace the remaining polling/thread-blocking concurrency scheduling with a maintained async/event-loop scheduler path.
- Replace the remaining blocking higher-level HTTP convenience layer with the maintained evented runtime implementation.

## Work Completed

- Replaced the remaining MIR-runtime polling waits for `sleep(...)`, queue receives, and `select` with the shared runtime scheduler in `runtime_value.rs`.
- Added direct-runtime scheduler support for `select`, including opaque channel/deadline buffers and cancellation-aware wakeups, and updated direct codegen to use that path instead of `sleep_ms(1)` polling.
- Fixed the select-cancellation semantics in both MIR and direct runtime paths so a cancelled wait falls through the select terminator promptly instead of sitting in the current `after(...)` arm until timeout expiry.
- Replaced the higher-level HTTP listener/request implementation with the maintained nonblocking `TcpStreamValue` / `TcpListenerValue` runtime surface and explicit HTTP request/response parsing/writing.
- Removed the old `tiny_http` / `ureq` dependency path from the compiler crate.
- Added and kept focused regressions for:
  - cancellation waking sleeping tasks promptly
  - cancellation waking `select` waits promptly
  - runtime-scheduler wakeups for timer and select waits
  - HTTP resources using nonblocking descriptors internally
- Updated the maintained tutorial and README surface so it no longer claims that HTTP is blocking or that queue/select/timer waits lack the shared scheduler.

## Verification

- `cargo fmt --all`
- `cargo test -p aurora-compiler cancellation_wakes_select_tasks_promptly -- --nocapture`
- `cargo test -p aurora-compiler`
- `cargo test -p aura`
- `npm run test:lsp`
- `npm run check:extension`
- `cargo clippy -p aurora-compiler -p aura -- -D clippy::correctness`

## Follow-up

- Within this maintained-surface scope, none.
- Broader future runtime work, if desired later, would be coroutine-style task multiplexing and async file I/O. The current tree now has a shared evented wait scheduler while keeping Aurora tasks thread-backed.
