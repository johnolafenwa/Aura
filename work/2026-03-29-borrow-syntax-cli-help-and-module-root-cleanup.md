# 2026-03-29: Borrow Syntax, CLI Help, And Module-Root Cleanup

## Goal

Remove the duplicate free-function borrow parameter spelling, fix direct nested-package module resolution, add normal CLI help/version entrypoints, and clean up the maintained docs/examples so they match the implemented surface without machine-local paths.

## Work Completed

- Added parser regression coverage for rejecting prefix ordinary borrowed parameters such as `borrow counter: Counter` and `borrow mut counter: Counter`.
- Removed parser acceptance of the prefix spelling for ordinary parameters while preserving `borrow self` / `borrow mut self` receiver syntax.
- Updated the maintained borrowed-parameter example and compiler pass fixtures to use only the canonical `name: borrow Type` form.
- Updated the JS fallback analysis parser so it no longer accepts the removed ordinary-parameter borrow spelling.
- Added compiler and CLI regression coverage for directly checking and analyzing nested package modules.
- Fixed package-root inference so direct entrypoints like `examples/modules/pkg/user.au` resolve imports from the nearest ancestor that satisfies the module imports, instead of always rooting at the file’s parent directory.
- Added normal success paths for `aura help`, `aura --help`, `aura version`, and `aura --version`.
- Replaced machine-local absolute repo paths in the maintained READMEs/tutorials with portable relative links or `$(pwd)` examples.
- Refreshed `examples/README.md` to include `examples/modules/trait_impl_imports.au` and clarified that `examples/modules/pkg/` contains helper modules rather than runnable entrypoints.
- Documented the then-current user-facing limitations called out by review, including list literals being absent at that point, no `String(...)` constructor, no bare `Ok(...)` / `Err(...)` constructors, required `Channel[T]` context for `channel()`, and named-function-only `spawn` targets.

## Verification

- `cargo fmt --all`
- `cargo test -p aurora-compiler`
- `cargo test -p aura`
- `npm run test:lsp`
- `npm run check:extension`

## Follow-Up

- List literals and a real built-in collection surface are still outside the maintained compiler subset.
- If Aurora wants a softer migration story for future syntax removals, the compiler will need a warning/deprecation channel instead of binary accept/reject behavior only.
