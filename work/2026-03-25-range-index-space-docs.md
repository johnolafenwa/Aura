# 2026-03-25 - range Signed Index Space Docs

## Goal

Document the current bootstrap `range(...)` limitation accurately in the maintained implementation docs without turning it into a frozen language-spec rule.

## Work Completed

- Updated `tutorials/04-control-flow.md` to list the current signed-index-space limit under `range(...)` limits.
- Updated `tutorials/13-current-language-surface.md` to describe the exact currently supported `range(...)` forms and note that bounds must fit the bootstrap compiler's signed index space.
- Added the same note to `examples/README.md` next to the maintained `for_range.au` example.
- Updated `work/task-board.md` to record the clarification pass.

## Verification

- Manual consistency check across the updated docs.

## Follow-up

- If Aurora removes the signed-index-space restriction later, these maintained docs should be updated in the same implementation pass.
- The proposal remains unchanged, since this is an implementation limitation rather than a frozen v1 rule.
