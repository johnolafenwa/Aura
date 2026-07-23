## Goal

Follow up on the newest Aurora review by fixing the remaining FFI-safety annotations, git revision validation, MIR/runtime safety guards, and the final reviewed landmines in MIR lowering and pattern checking.

## Planned Scope

- mark the remaining raw-pointer FFI entrypoints as `unsafe extern "C"`
- tighten git revision validation to reject overly-short prefixes
- add sanity caps for embedded MIR/source buffer lengths in `aurora_native_run`
- replace the remaining `is_some_and(...).unwrap()` MIR lowering sites
- re-audit and lock nested pattern payload arity handling with a targeted regression
- document the remaining refcount and SIGPIPE invariants where behavior is intentionally constrained by caller expectations
- run clippy as part of verification so the FFI lint regression is actually caught

## Work Completed

- Marked the remaining raw-pointer FFI boundary functions as `unsafe extern "C"` and added explicit `# Safety` documentation for the maintained entrypoints.
- Added embedded-input length caps in `aurora_native_run(...)` so obviously corrupt MIR/source/source-path lengths are rejected before raw-pointer slices are formed.
- Tightened git revision validation to reject short hashes below seven hex characters while keeping full hex-only validation in place for manifest, lockfile, and resolved revision paths.
- Replaced the remaining `is_some_and(...).unwrap()` MIR-lowering sites with direct filtered lookups so future refactors do not inherit latent unwrap landmines.
- Re-audited the nested pattern payload arity concern and confirmed the maintained checker already rejects it recursively; the existing regression in `sema_tests.rs` covers the malformed nested `Leaf.Value(left, right)` shape directly, so no semantic change was required there.
- Added invariant comments for the direct-runtime refcount helpers and the deliberate BrokenPipe/SIGPIPE interaction in the direct stdout path.
- Added regression coverage for the new oversized embedded-input rejection and the stricter short git revision rejection.
- Ran a `clippy::correctness` pass to confirm the raw-pointer FFI issue is now caught by tooling and no longer reported.

## Verification

- `cargo fmt --all`
- `cargo test -p aurora-compiler`
- `cargo test -p aura`
- `npm run test:lsp`
- `npm run check:extension`
- `cargo clippy -p aurora-compiler -p aura -- -D clippy::correctness`

## Follow-up

- `cargo clippy -D clippy::correctness` is now clean for `aurora-compiler` and `aura`, but the repo still has non-correctness style/perf warnings outside that lint group.
- The recursion-cap decision from pass 2 remains unchanged: the cap stays conservative until the parser is made structurally less stack-recursive.
