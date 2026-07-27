## Goal

Fix the remaining fifth-pass review findings around `match mut` stale bindings, TLS accept-loop slowloris handling, and malformed HTTP listener recovery without regressing the maintained examples or CLI/runtime behavior.

## Work Completed

- Added and kept the failing-first regressions for:
  - stale `match mut` pattern-binding use after a real scrutinee reassignment
  - dead-branch `match mut` writeback preservation
  - malformed HTTP request recovery with a `400 Bad Request` response
  - multi-client TLS slowloris resistance without linearly delaying the next valid client
- Extended the checker’s local-binding state to track `match mut` pattern bindings, invalidate them after a real overlapping scrutinee reassignment, and reject later stale binding use with a targeted diagnostic.
- Kept the dead-branch behavior correct by making `if false` / `while false` bodies continue to type-check but stop merging unreachable writeback-invalidating state back into the live branch state.
- Reworked `TlsListenerValue::accept()` so it can keep draining the TCP backlog while prior TLS handshakes remain pending, and replaced the interim `thread::sleep(...)` loop with the runtime’s existing deadline-aware fd wait path so Aurora task scheduling still makes progress during in-runtime TLS handshakes.
- Updated `HttpListenerValue::accept()` to treat malformed request syntax and premature EOF as per-connection `400 Bad Request` cases, close that client, and continue accepting the next request.
- Updated `sema_tests.rs` helper bindings and the new check-fail fixture expectation so the broader compiler suites match the new `LocalBinding` fields and the final assignment-target diagnostic span.

## Verification

- `cargo fmt --all`
- `cargo test -p aurora-compiler --lib -- --nocapture`
- `cargo test -p aurora-compiler --test fixtures -- --nocapture`
- `cargo test -p aura --test cli -- --nocapture`
- `cargo test -p aurora-compiler public_run_path_executes_unix_and_tls_example_with_path_context --lib -- --nocapture`
- `cargo test -p aurora-compiler tls_listener_accept_is_not_linearly_delayed_by_multiple_stalled_peers --lib -- --nocapture`
- `./target/debug/aura run examples/io/unix_tls_roundtrip.au`

## Follow-up

- The TLS accept loop now avoids the linear `10s * N` backlog delay from stalled peers, but it still uses listener-fd time slices rather than polling pending handshake sockets directly. If TLS listener load becomes a larger product concern, the next step is a dedicated multi-fd readiness wait for pending server handshakes.
