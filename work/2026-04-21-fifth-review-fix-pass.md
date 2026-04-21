## Goal

Validate the fifth-pass externally reviewed defects, fix the confirmed compiler/runtime/tooling issues end to end with failing regressions first, and reverify the maintained surface.

## Session

- Start: 2026-04-21 00:16:26 BST
- Stop: 2026-04-21 01:30:37 BST
- Stop rule: Complete the work or reach 12 continuous hours.

## Work Completed

- Added failing-first compiler, runtime, and CLI regressions for the fifth-pass review defects around `match borrow mut` writeback suppression after dead-path scrutinee assignment, native direct-backend bare `None` matching, TLS accept-loop timeout skipping, HTTP `413`/`431` listener recovery, `Self` in trait/impl parameter positions, builtin-variant shadowing by user classes, and the expanded filesystem read-all cap.
- Reworked MIR `match borrow mut` lowering so synthetic writebacks are suppressed only on executed scrutinee-write paths instead of any arm that merely contains a syntactic `x = ...`, which closes the broad silent-regression blast radius identified in the fifth pass.
- Fixed direct-backend builtin enum coercion for bare `None` values so `Option[T] = None` round-trips through native pattern matching the same way it does through `aura run`.
- Hardened the runtime listener paths by making TLS accepts discard timed-out handshakes internally, making HTTP accepts reject declared oversize bodies immediately, adding `431 Request Header Fields Too Large`, and continuing past per-connection `413`/`431` failures instead of surfacing them as listener failures.
- Raised the shared read-all cap from `1 MiB` to `64 MiB`, updated the maintained documentation to match, and converted the limit regressions to sparse/chunked setups so they stay fast and stable.
- Enabled `Self` in trait and impl method parameter positions, added the maintained `examples/traits/self_parameters.au` example, and updated the traits tutorial plus examples index.
- Fixed builtin bare-variant resolution so user classes named like builtin variants (`Item`, `Some`, `Ok`, `Err`) resolve to the user class in local/module scope instead of being silently shadowed.
- Added `io.Error.Cancelled` alongside the existing `io.Error.Closed` mapping, and replaced the Unix websocket/TLS raw-fd `unreachable!()` fallback with a normal `Unsupported` error.

## Verification

- `cargo fmt --all`
- `cargo test -p aurora-compiler --lib -- --nocapture`
- `cargo test -p aurora-compiler --test fixtures -- --nocapture`
- `cargo test -p aura --test cli -- --nocapture`
- `./target/debug/aura run examples/traits/self_parameters.au`

## Follow-up

- Aurora HTTP still uses an explicit one-request-per-connection model (`HttpExchangeValue::respond_*` closes the socket after the response), so full keep-alive support remains larger product work rather than a localized bug fix.
- `fs.write_string(...)` and `fs.write_bytes(...)` still use direct overwrite semantics; cross-platform atomic replacement would need a separate design pass to avoid platform-specific rename pitfalls.
