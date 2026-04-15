# 2026-04-15 Deps Update Command

## Session

- Started: `2026-04-15 23:49:03 BST`
- Completed: `2026-04-15 23:57:27 BST`
- Total elapsed: `0h 08m`

## Goal

Add package-manager update commands for the implemented git dependency surface:

- `aura deps update`
- `aura deps update <package>`

with lockfile refresh behavior for branch/tag/default-main git dependencies.

## Work Completed

- Added a compiler-level dependency refresh API that reloads branch/tag/default-main git dependencies and rewrites `Aurora.lock` without deleting it manually.
- Added CLI support for `aura deps update` and `aura deps update <package>`, using the current package or workspace directory as the update context.
- Kept `rev = "..."` dependencies fixed while allowing moving branch/tag selectors to refresh on demand.
- Added CLI product tests for updating all git dependencies and for updating a specific named package.
- Added a compiler regression test covering targeted and all-package git dependency updates through the package API.
- Updated the maintained README/tutorial surface to teach the new dependency refresh workflow.

## Verification

- `cargo fmt --all`
- `cargo test -p aurora-compiler`
- `cargo test -p aura`

## Follow-Up

- Add explicit path selection later only if there is a real need to run dependency updates from outside the package/workspace tree.
- Add richer dependency-management commands such as fetch/status only if the package workflow needs them.
