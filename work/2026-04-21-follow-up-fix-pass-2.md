## Goal

Fix the remaining April 21 follow-up review findings around `match borrow mut` stale bindings through `borrow mut` calls, module-qualified builtin enum constructor typing, and the non-Unix TLS listener accept path without regressing the maintained compiler/runtime behavior.

## Session

- Start: 2026-04-21 13:08:01 BST
- Stop: 2026-04-21 13:34:49 BST
- Stop rule: Complete the work or reach 12 continuous hours.

## Work Completed

- Added failing-first regressions for:
  - stale `match borrow mut` pattern-binding use after a `borrow mut` helper rewrites the scrutinee
  - module-qualified builtin `io.Error` constructor typing through both sema and CLI surfaces
- Extended the checker so resolved `borrow mut` call sites invalidate overlapping `match borrow mut` pattern bindings after the call actually executes, instead of only invalidating on explicit assignment statements.
- Kept constant short-circuit branches from polluting the live local state by stopping `false and ...` / `true or ...` right-hand side checker effects from merging back when the right side is statically unreachable.
- Unified builtin module-type canonicalization so qualified builtin module enums like `io.Error` keep their qualified type identity across type lowering, member resolution, and qualified enum-constructor/member paths.
- Reworked `TlsListenerValue::accept()` to use the shared pending-handshake backlog model on every platform, replacing the old non-Unix inline one-peer-at-a-time handshake path with the same queue-driven progression used on Unix.
- Kept the accepted TLS stream in nonblocking mode only for the handshake phase on non-Unix and switched it back before returning it to the normal runtime I/O surface.

## Verification

- `cargo fmt --all`
- `cargo test -p aurora-compiler module_qualified_builtin_io_error_variants_type_check --lib -- --nocapture`
- `cargo test -p aurora-compiler --test fixtures check_fail_fixtures_match_expected_diagnostics -- --nocapture`
- `cargo test -p aura --test cli check_accepts_module_qualified_builtin_io_error_variants -- --nocapture`
- `cargo test -p aurora-compiler --lib tls_listener_accept_is_not_linearly_delayed_by_multiple_stalled_peers -- --nocapture`
- `cargo test -p aurora-compiler --lib -- --nocapture`
- `cargo test -p aurora-compiler --test fixtures -- --nocapture`
- `cargo test -p aura --test cli -- --nocapture`

## Follow-up

- The shared TLS accept logic is now source-aligned across platforms and fully reverified on Unix. The non-Unix branch was not runtime-executed locally in this session, so cross-platform CI coverage is still the next place to catch any Windows-specific socket behavior drift.
