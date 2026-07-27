## Goal

Validate Claude's external Aurora review corpus under `/tmp/aurora_review`, confirm which reported issues still reproduce on the current tree, and record any stale or already-fixed findings.

## Work Completed

- Replayed Claude's top critical repros directly from `/tmp/aurora_review` against the current `./target/debug/aura`.
- Confirmed these reported issues still reproduce:
  - `Queue.get_or_none()` / `Task.result_or_none()` matched without an explicit `Option[...]` annotation can skip every `Some`/`None` arm at runtime.
  - `aura run` drops buffered stdout when a runtime error is raised.
  - `with TaskGroup() as group:` does not join queued child work before scope exit.
  - `cancelled()` still never flips in a tight CPU-bound polling loop.
  - task runtime failures remain unrecoverable by user code and abort the whole Aurora program rather than surfacing as a task-result value.
  - literal `match` still forgets move tracking after the block exits.
  - `with` still forgets move tracking after the block exits.
  - the `with` ownership hole still causes an observable `aura run` / direct-backend divergence on the same checker-accepted program.
  - binding a `self`-receiver method call to a name still triggers a spurious `use of moved value`.
  - `Vec.insert(index=OOB, value=...)` still silently discards the inserted value.
  - `Vec.swap(a, OOB)` still silently becomes a no-op.
- Narrowed one report claim: the task-failure case does not host-panic the process, but it is still an unrecoverable Aurora-program error rather than a task-local result.

## Verification

- `./target/debug/aura check /tmp/aurora_review/collections/13g_no_annot.au`
- `./target/debug/aura run /tmp/aurora_review/collections/13g_no_annot.au`
- `./target/debug/aura build --backend direct -o /tmp/aurora_review/collections/13g_no_annot.bin /tmp/aurora_review/collections/13g_no_annot.au && /tmp/aurora_review/collections/13g_no_annot.bin`
- `./target/debug/aura run /tmp/aurora_review/real_world/print_buffer.au`
- `./target/debug/aura run /tmp/aurora_review/concurrency/t28_detached.au`
- `./target/debug/aura run /tmp/aurora_review/concurrency/t07d_cancel_tight.au`
- `printf '6\n' > /tmp/aurora_review/concurrency/idx.txt && ./target/debug/aura run /tmp/aurora_review/concurrency/t13c_runtime_fail.au`
- `./target/debug/aura check /tmp/aurora_review/ownership/20b_match_unconditional.au`
- `./target/debug/aura run /tmp/aurora_review/ownership/20b_match_unconditional.au`
- `./target/debug/aura check /tmp/aurora_review/ownership/WITH_BUG_confirmation.au`
- `./target/debug/aura run /tmp/aurora_review/ownership/WITH_BUG_confirmation.au`
- `./target/debug/aura run /tmp/aurora_review/ownership/double_free_attempt.au`
- `./target/debug/aura build --backend direct -o /tmp/aurora_review/ownership/double_free_attempt.bin /tmp/aurora_review/ownership/double_free_attempt.au && /tmp/aurora_review/ownership/double_free_attempt.bin`
- `./target/debug/aura run /tmp/aurora_review/basics/verify_self_bind.au`
- `./target/debug/aura run /tmp/aurora_review/collections/03c_insert_oob.au`
- `./target/debug/aura build --backend direct -o /tmp/aurora_review/collections/03c_insert_oob.bin /tmp/aurora_review/collections/03c_insert_oob.au && /tmp/aurora_review/collections/03c_insert_oob.bin`
- `./target/debug/aura run /tmp/aurora_review/collections/04_vec_reverse_swap_eq.au`
- `./target/debug/aura build --backend direct -o /tmp/aurora_review/collections/04_vec_reverse_swap_eq.bin /tmp/aurora_review/collections/04_vec_reverse_swap_eq.au && /tmp/aurora_review/collections/04_vec_reverse_swap_eq.bin`

## Follow-up

- I did not fully rerun the performance/memory section or the lower-priority API/docs section from Claude's report in this pass. The critical correctness set is still live and is the right next fix target.

## Additional Validation

- Rechecked the separate twelve-finding review list against the current tree.
- Confirmed those listed issues are already fixed in source:
  - MIR `fs.read_to_string(...)` / `fs.read_bytes(...)` now route through the capped `read_file_limited(...)` path in both runtimes.
  - compiler-backed LSP document state is globally invalidated on open/change/close, so imported-file edits no longer leave open dependents stale.
  - UNC `file://` URIs are parsed through the shared `src/uri.js` helper with explicit Windows UNC handling.
  - the architecture docs now describe the maintained `wait_any(...)` / `wait_all(...)` concurrency surface instead of `select`.
  - stale `match mut` binding use after scrutinee reassignment or `mut ` helper calls is rejected by the checker.
  - module-qualified builtin enum constructors preserve `io.Error` / `process.Error` identity through sema and MIR.
  - malformed HTTP requests return `400 Bad Request` and the listener continues serving later clients.
  - TLS listener backlog tests now cover both the Unix multi-slowloris regression and the non-Unix wait-policy helper.

- Verification for that twelve-finding list:
  - `cargo test -p aura --test cli run_caps_fs_read_to_string_and_read_bytes -- --nocapture`
  - `cargo test -p aura --test cli run_and_direct_backend_preserve_builtin_module_enum_identity -- --nocapture`
  - `cargo test -p aurora-compiler --lib tls_listener_accept_is_not_linearly_delayed_by_multiple_stalled_peers -- --nocapture`
  - `cargo test -p aurora-compiler --lib http_listener_replies_with_400_for_malformed_requests_and_continues_accepting -- --nocapture`
  - `cargo test -p aurora-compiler --lib module_qualified_builtin_io_error_variants_type_check -- --nocapture`
  - `cargo test -p aurora-compiler --lib non_unix_tls_listener_wait_timeout_blocks_when_no_handshakes_are_pending -- --nocapture`
  - `cd tools/aurora-language-server && npm test -- --test-name-pattern='uri helper preserves UNC and local file paths|document state cache|invalidateAll|compiler bridge'`
