# 2026-04-15 Git Dependencies

## Goal

Extend the Aurora package system from local path dependencies to git-backed dependencies with:

- `git = "..."`
- `rev = "..."`
- `tag = "..."`
- `branch = "..."`
- default branch fallback to `main` when no selector is provided
- lockfile support
- compiler, CLI, and LSP regression coverage
- maintained examples, tutorials, and README updates

## Work Completed

- Extended `Aurora.toml` dependency parsing to support `git = "..."` alongside the existing `path = "..."` form.
- Added git selector support for `rev`, `tag`, and `branch`, defaulting to `branch = "main"` when no selector is present.
- Added lockfile loading plus git lockfile entries so git dependencies are pinned by resolved revision in `Aurora.lock`.
- Added git dependency materialization through cached checkouts backed by the system `git` binary.
- Kept package imports package-name-prefixed for git dependencies just like path dependencies.
- Added compiler regression coverage for default-`main` git dependencies, lockfile pinning, explicit branch selection, explicit tag selection, and invalid mutually exclusive dependency source fields.
- Added CLI product coverage for git-backed package `check`, `run`, `run-mir`, `build`, `analyze`, `complete`, and stdin-backed tooling.
- Added a compiler-bridge regression for analysis and completion through a manifest-rooted git dependency.
- Updated the maintained README/tutorial/examples surface to document git dependencies, default `main`, and lockfile-pinned revisions.

## Verification

- `cargo fmt --all`
- `cargo test -p aurora-compiler`
- `cargo test -p aura`
- `npm run test:lsp`
- `npm run check:extension`

## Follow-Up

- Add explicit dependency-management commands such as fetch/update only when the package surface needs intentional lockfile refresh workflows.
- Extend git dependencies later with optional workspace subdirectory selection if multi-package dependency repos become a real requirement.
