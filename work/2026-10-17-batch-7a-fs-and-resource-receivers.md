# 2026-10-17 Batch 7a: fs.rename, File.sync, And Resource Receivers

## Goal

Two Batch 7a items that John approved on 2026-10-17:

- Add `fs.rename` and an fsync call, so programs such as AuraDB can save a
  file atomically without spawning `/bin/mv`. This was bug 8 in the AuraDB
  benchmark review.
- Close the gap recorded in `2026-10-16-batch-5-step-2b.md`. The checker
  did not require a mutable receiver for most file, process, and network
  resource methods, although ADR-0061 H1 says the receiver table is
  enforced at every call site. John chose option 1: the checker requires a
  mutable receiver and points a shared match binding to `match own`.

## Work Completed

- `fs.rename(source, target) -> Result[None, io.Error]` and
  `fs.File.sync() -> Result[None, io.Error]` on the MIR runtime and the
  direct backend. `sync` flushes, then calls `File::sync_all`. Each new
  native entry point has runtime unit tests for success, typed `io.Error`,
  and the argument type trap.
- New example `examples/io/atomic_save.au`, run on both backends by the CLI
  tests and listed in the filesystem chapter, the API index, the I/O
  tutorial, and the examples README.
- The checker calls `require_mutable_receiver` for every builtin member of
  a host resource type whose receiver passing is `BorrowMut`. That is the
  same table MIR validation uses. The two per-method calls for
  `fs.File.write_all` and `write_bytes` became redundant and were removed.
- `AU3003` on a binding from a bare `match` names the binding and the
  scrutinee and suggests `match own <scrutinee>`, or `match mut` too when
  the scrutinee is a place. A bare match on an owned call result now records
  a scrutinee spelling such as `child.stdin()`, so the move diagnostic for
  that binding also suggests `match own`. A call that returns a view keeps
  the older wording.
- `check-pass/pythonic_convenience_methods.au` uses `match own`. Four new
  check-fail fixtures cover a pipe closed through a shared match on a call,
  a file written through a shared match on a field, an immutable file local,
  and a shared `TcpStream` parameter.

## Source Changes

Programs that called a writing or closing resource method through an
immutable local, an owned parameter, or a bare-match binding now fail with
`AU3003`. No example or tutorial program changed. The test sources that
did so were updated: one run-pass fixture used a bare match on
`net.connect_timeout(...)`, five task-transfer check-fail fixtures closed an
owned parameter, and several Rust tests bound an `accept()` result or a
resource parameter without `mut`.

## Verification

- The new fixtures failed before the checker change and pass after it.
- `examples/io/atomic_save.au` prints `second`, `false`,
  `io.Error.NotFound` on `aura run` and on a direct build.
- Every workspace test target passes with `RUST_MIN_STACK=33554432`,
  except `package_io_helpers_cover_local_error_paths`, which needs a
  non-root user. One run hit a timing failure in
  `native_runtime_direct_cancellation_releases_frame_owned_argument_references`.
  It passed on three isolated reruns and two full `aura-runtime` reruns.
- `npm run check:clippy`, `cargo fmt --all --check`,
  `scripts/check-hygiene.sh`, `scripts/generate_llms.py --check`, all 789
  documentation examples, and the 136 language server tests pass.

## Follow-Up

- Aura has no directory sync call, so a rename is atomic but not yet
  guaranteed durable across power loss. Add one if AuraDB needs it.
- The hash `dict` and `set` and value-representation work stays with the
  other Batch 7a thread.
