## Goal

Fix the remaining April 21 post-follow-up review findings around builtin module enum constructor lowering in MIR and the non-Unix TLS listener wait regression without regressing the maintained compiler/runtime behavior.

## Session

- Start: 2026-04-21 14:30:39 BST
- Stop: 2026-04-21 14:46:59 BST
- Stop rule: Complete the work or reach 12 continuous hours.

## Work Completed

- Added failing-first regressions for:
  - builtin module enum identity across `aura run` and direct-built binaries, including qualified `io.Error` construction, printing, `match`, payload constructors, and equality against runtime-produced `io.Error.NotFound`
  - non-Unix TLS listener wait-policy selection so an empty pending-handshake queue blocks until real listener readiness instead of defaulting to a fixed 50 ms slice
- Preserved qualified builtin enum names through checker canonicalization so explicitly qualified patterns like `case io.Error.NotFound:` keep the builtin module path instead of collapsing to bare `Error`.
- Preserved the same qualified builtin enum identity through MIR enum-constructor lowering and MIR match-pattern lowering so `io.Error.*` and `process.Error.*` values round-trip consistently through construction, printing, equality, and `match` in both `aura run` and direct-built binaries.
- Replaced the non-Unix TLS listener polling path with a readiness wait backed by `mio`, while keeping short slices when handshake work is already pending and retaining the shared timeout/cancellation behavior.

## Verification

- `cargo test -p aura --test cli run_and_direct_backend_preserve_builtin_module_enum_identity -- --nocapture`
- `cargo test -p aurora-compiler --lib non_unix_tls_listener_wait_timeout -- --nocapture`
- `cargo fmt --all`
- `cargo test -p aurora-compiler --lib -- --nocapture`
- `cargo test -p aurora-compiler --test fixtures -- --nocapture`
- `cargo test -p aura --test cli -- --nocapture`

## Follow-up

- The non-Unix TLS readiness path now uses `mio` rather than the previous fixed-sleep loop, but I still only runtime-verified the TLS accept flow on Unix locally. Cross-platform CI remains the place to catch any Windows-specific socket behavior drift in the new readiness wait.
