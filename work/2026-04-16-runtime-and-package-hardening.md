## Goal

Fix the confirmed runtime and package-system security/correctness issues from the latest review, with priority on native direct-runtime ownership leaks, git dependency command hardening, import/package path validation, runtime FFI hygiene, and remaining production panic/validation gaps.

## Work Completed

- Reworked the direct native runtime ownership model so opaque values are `Arc`-backed, with explicit `aurora_direct_retain_value(...)` / `aurora_direct_release_value(...)` hooks and direct-backend codegen that now emits retain/release traffic for opaque temporaries, locals, returns, spawned thunk arguments, and deadline cleanup.
- Removed the unsafe borrowed `decode_bytes(...) -> &str` shape in the direct runtime and switched it to owned-string decoding with runtime diagnostics instead of panics on invalid UTF-8.
- Replaced the old global `SIGPIPE` ignore with thread-local `SIGPIPE` blocking/consumption around native runtime stdout writes so direct-built binaries now exit cleanly on broken pipes without changing process-wide signal behavior.
- Hardened git dependency resolution in the package system:
  - reject dash-prefixed/control-character git sources
  - pass `--` before user-controlled git sources
  - hash cache keys with SHA-256
  - verify cached revisions via `.aurora-cache-rev`
  - validate lockfile versions
  - validate package names, editions, and package-version syntax
  - escape lockfile string fields
  - cap dependency counts to avoid manifest-driven dependency explosions
- Hardened package/import path resolution so canonicalized import targets are checked against canonicalized roots, including symlink/canonicalization-safe package-root behavior on macOS temp paths.
- Replaced several remaining production `expect(...)`/generic panic surfaces with direct diagnostics or more specific runtime errors in the package loader, CLI paths, and MIR runtime, and improved MIR FFI panic reporting to include the panic payload.
- Added/updated regression coverage for:
  - git manifest validation, lockfile validation, package-version validation, dependency-count caps, and source-root escape rejection
  - native runtime retain/release behavior, arg-buffer ownership, deadline cleanup, and broken-pipe product behavior through direct-built binaries
  - native-codegen retain/release emission and spawned-thunk cleanup paths
  - canonical import-root inference/tests that previously failed on `/var` vs `/private/var` path normalization

## Verification

- `cargo fmt --all`
- `cargo test -p aurora-compiler`
- `cargo test -p aura`
- `npm run test:lsp`
- `npm run check:extension`

## Follow-up

- Remaining low-priority review items are mostly non-blocking quality work rather than current correctness/security bugs:
  - optional git-cache cleanup UX (`aura cache clean` or similar)
  - more opinionated normalization of semantically equivalent git URLs for cache reuse
  - broader cleanup of internal-invariant `expect(...)` sites in checker/lowering code that are not currently user-triggerable
