## Goal

Follow up on the latest Aurora review by fixing the remaining package hardening, MIR runtime safety, IntegerValue ordering cleanup, and reviewed defense-in-depth issues.

## Session

- Start time: 2026-04-17 20:15:45 BST
- Stop time: 2026-04-17 20:37:38 BST
- Elapsed: 0h 22m

## Work Completed

- harden git dependency fetches against symlinked checkouts and interactive credential hangs
- tighten cached checkout reads against symlink races
- add structural/runtime limits for hostile embedded MIR payloads
- fix the remaining IntegerValue ordering/clippy landmine by removing the inherent `cmp`
- diagnose `Instant::checked_add` overflow instead of firing timers immediately
- add small defense-in-depth fixes around field-path guarding, runtime invariants, and dead refcount fencing
- validate cached git revision markers through `O_NOFOLLOW` reads and reject symlinked checkout content before cache adoption
- keep git subprocesses non-interactive with prompt-disabling environment variables
- retain the hex-validated detached checkout flow after confirming `git checkout --detach -- <rev>` is invalid
- add regression coverage for symlink rejection, non-interactive git command construction, MIR deadline overflow handling, MIR structural complexity limits, stricter git revision validation, and empty field-path rejection

## Verification

- `cargo fmt --all`
- `cargo test -p aurora-compiler`
- `cargo test -p aura`
- `npm run test:lsp`
- `npm run check:extension`
- `cargo clippy -p aurora-compiler -p aura -- -D clippy::correctness`

## Follow-up

- No correctness blockers remain from the latest review batch.
- Non-correctness clippy style/perf warnings still exist in the compiler and CLI crates; they were left untouched because this pass was scoped to the reviewed hardening items.
