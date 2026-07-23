## Goal

Follow up on the post-hardening Aurora review by fixing the remaining parser recursion, temp-file cleanup, lock poison, git rev handling, refcount guard, SIGPIPE restoration, float division, and remaining internal-error findings.

## Planned Scope

- extend parser recursion guards beyond expressions and raise the supported recursion limit
- share recursion accounting with f-string interpolation parsing
- clean up partial temp files after unique-temp write failures
- make runtime channel/task-group/task locks poison-tolerant
- harden native opaque-value refcount overflow and underflow handling
- restore the previous SIGPIPE mask on every direct-runtime exit path
- diagnose float division by zero consistently with float modulo
- audit remaining git `rev` command paths and harden temp-file naming / write placement
- reduce the remaining reviewed `unwrap` / `expect` internal panic sites where they are still on production paths
- add regression coverage for each confirmed bug or hardening change

## Work Completed

- Extended parser recursion guards beyond expressions so statement parsing, type parsing, pattern parsing, and f-string interpolation reuse the same recursion budget.
- Kept `RECURSION_LIMIT` at `128` deliberately. A local probe showed that materially higher values can still overflow the host Rust stack before Aurora reports a diagnostic on the current recursive parser shape, so the review suggestion to raise it was not safe to apply directly.
- Added regression coverage for the new parser recursion paths and for interpolation parsing reusing the active recursion budget.
- Reworked `write_unique_temp_file(...)` in `crates/aura` so failed writes and flushes clean up the partial temp file instead of leaving stale `create_new(true)` collisions behind.
- Made runtime `Mutex` / `Condvar` access in `runtime_value.rs` poison-tolerant for channels, tasks, and task groups.
- Hardened direct-runtime opaque refcount retain/release so zero/underflow and overflow cases now diagnose instead of silently wrapping.
- Fixed direct-runtime SIGPIPE handling so the thread signal mask is restored on broken-pipe exits while built binaries still exit cleanly with status 0 when stdout closes early.
- Diagnosed float division by zero consistently with float modulo in both MIR and direct-runtime execution paths.
- Hardened git revision handling further by validating resolved revisions, validating explicit `rev` selectors from manifests and lockfiles, and adding collision-resistant temp-path generation with a per-process atomic counter.
- Improved Windows replace semantics for atomic cache and lockfile writes by using a replace-existing move path instead of delete-then-rename.
- Removed the remaining production `unwrap` / `expect` sites in `sema.rs` and `integer.rs`, replacing them with diagnostics or safe fallbacks.
- Confirmed the earlier “nested match pattern payload arity is only checked shallowly” review item was already covered by maintained checker diagnostics and existing tests; no semantic change was needed there.
- Updated the maintained `float_special_values` run-pass fixture so it stays valid after the float-special-value coverage adjustments.

## Verification

- `cargo fmt --all`
- `cargo test -p aurora-compiler`
- `cargo test -p aura`
- `npm run test:lsp`
- `npm run check:extension`

## Follow-up

- The parser recursion cap remains intentionally conservative until the recursive parser shape is flattened further or moved behind a different stack-safe strategy.
- The Windows atomic replace path is now stronger than the earlier delete-then-rename fallback, but it is still only compile-validated on non-Windows hosts in this repo's current CI posture.
