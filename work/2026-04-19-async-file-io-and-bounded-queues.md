# 2026-04-19 Async File I/O And Bounded Queues

## Goal

- Implement async file I/O plus bounded queues across the maintained Aurora surface.

## Work Completed

- Added bounded `Queue[T]` capacity support across the maintained runtime and compiler surface:
  - `queue(capacity=...)` now constructs bounded queues.
  - bounded sends wait for free capacity on the shared scheduler instead of growing the queue without bound.
  - queue sends now surface both `SendError.Closed(value)` and `SendError.Cancelled(value)`.
  - `select` send arms now understand bounded queue readiness across both maintained runtime paths.
- Added scheduler-aware async file I/O underneath the existing `fs` and `fs.File` surface:
  - lightweight Aurora tasks now offload blocking file reads and writes through the shared blocking-I/O pool instead of pinning a scheduler task on a host thread.
  - top-level `fs.*` helpers and `fs.File` methods both use the same shared runtime path.
- Added and updated maintained regression coverage:
  - runtime unit coverage for bounded queue capacity behavior.
  - compiler regression proving a second `put(...)` blocks until a bounded queue frees capacity.
  - compiler regression proving a lightweight task blocked on FIFO file I/O does not stall other scheduled work.
  - updated fixtures for the expanded `SendError[T]` surface.
  - updated compiler smoke coverage and example-output expectations for the new bounded queue example.
- Added the maintained runnable example `examples/concurrency/bounded_queue.au`.
- Updated maintained docs and examples:
  - `README.md`
  - `crates/aura/README.md`
  - `examples/README.md`
  - `tutorials/10-results-and-options.md`
  - `tutorials/13-concurrency.md`
  - `tutorials/14-current-language-surface.md`
  - `tutorials/19-io-and-networking.md`
  - `tutorials/README.md`

## Verification

- `cargo fmt --all`
- `cargo test -p aurora-compiler`
- `cargo test -p aura`
- `npm run test:lsp`
- `npm run check:extension`
- `cargo clippy -p aurora-compiler -p aura -- -D clippy::correctness`

## Follow-up

- No remaining follow-up is required for the requested async file I/O plus bounded-queue milestone.
