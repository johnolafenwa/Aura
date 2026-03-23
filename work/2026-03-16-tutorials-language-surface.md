# 2026-03-16 Tutorial Surface Sync

## Goal

Bring the tutorial track up to the full currently implemented Aurora bootstrap surface instead of leaving it scoped only to the example-backed fundamentals.

## Work Completed

- Expanded the tutorial chapters to cover the broader implemented surface, including:
  - bare `None`
  - `public` field syntax
  - built-in `Result[T, E]`, `Option[T]`, and `SendError[T]`
  - `try expr`
  - `with` for resources and task groups
  - channels, spawned tasks, detached tasks, `task_group()`, `select`, `after(...)`, and cooperative cancellation
  - compiler-backed tooling commands such as `ast-json`, `analyze`, and `complete`
- Added `tutorials/13-current-language-surface.md` as the compact reference chapter for the actual supported subset.
- Updated the tutorial overview and README so the tutorial set now treats the implemented compiler surface, not just the examples, as the thing that must stay in sync.
- Clarified that `aura complete --line/--character` uses zero-based positions.

## Verification

- Documentation-only pass
- Performed a manual consistency sweep across the tutorial set and tutorial references

## Notes

The tutorials still intentionally stop at the bootstrap compiler boundary. Proposal-only areas such as traits, imports/modules, user-defined generics, and backend code generation remain documented elsewhere but are not taught as if they are already implemented.
