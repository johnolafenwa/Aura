## Goal

Review the landed fifth-pass fix set for regressions, newly introduced problems, and remaining uncaught issues.

## Session

- Start: 2026-04-21 01:35:00 BST
- Stop: 2026-04-21 10:05:51 BST
- Stop rule: Complete the work or reach 12 continuous hours.

## Work Completed

- Reviewed the landed `16e724a` fifth-pass fix commit and the high-risk compiler/runtime files it touched, with most of the inspection concentrated in `mir.rs`, `runtime_value.rs`, `sema.rs`, and the new regressions added under `crates/aura/tests/cli.rs` and `crates/aurora-compiler/tests/fixtures/`.
- Reproduced a remaining `match borrow mut` semantic hole where a real scrutinee reassignment suppresses all later binding writebacks in the same arm, yielding stale observable state in both `aura run` and direct-built binaries.
- Reproduced that the TLS listener still performs handshakes inline inside `accept()`, so multiple inert TCP clients delay a legitimate TLS client by roughly `10s * N` even though handshake failures are now discarded internally.
- Reproduced that malformed HTTP requests outside the explicit `413`/`431` paths still bubble out of `HttpListener.accept()` and can terminate a naive listener loop before the next valid client is accepted.
- Reproduced that namespaced builtin enum constructors like `io.Error.NotFound` / `io.Error.Closed` / `io.Error.Cancelled` still type-check as bare `Error`, so they cannot be bound to `io.Error` from source code.

## Verification

- `cargo test -p aurora-compiler --lib --quiet`
- `cargo test -p aura --test cli --quiet`
- `git show --stat --summary --oneline HEAD`
- `./target/debug/aura check <temp files>` for `io.Error.NotFound`, `io.Error.Closed`, and `io.Error.Cancelled`
- `./target/debug/aura run <temp file>` and `./target/debug/aura build --backend direct ...` for a `match borrow mut` arm that reassigns the scrutinee and then mutates the old binding
- A Python socket/TLS repro that opened two inert TCP connections ahead of a legitimate TLS client against a temporary Aurora TLS server, confirming about 20 seconds of delay before the valid client was accepted
- A Python raw-HTTP repro that sent one malformed request followed by a valid request to a temporary Aurora HTTP server, confirming that the malformed request still caused the listener task to exit before the valid request could be served

## Follow-up

- Fix the post-reassignment `match borrow mut` binding-use hole by either rejecting use of stale arm bindings after a scrutinee write or by remapping later binding writes onto the new scrutinee state.
- Move TLS server handshakes off the blocking accept path, or otherwise decouple queued bad TCP clients from the latency of the next successful TLS accept.
- Treat malformed HTTP request syntax and invalid headers as per-connection failures inside `HttpListener.accept()` instead of surfacing them as listener-fatal errors.
- Preserve module-qualified enum type identity in `resolve_member_type(...)` so source expressions like `io.Error.NotFound` actually produce `io.Error`.
