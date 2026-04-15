# 2026-04-14 Work Session Tracking Rule

## Goal

Make long-running work persistence explicit in the repo so substantial tasks track their session start time, elapsed time, and 12-hour stop condition in maintained work tracking.

## Work Completed

- Updated `AGENTS.md` so substantial work now requires:
  - recording the session start time in `work/task-board.md`
  - tracking elapsed wall-clock time while the work is active
  - continuing until the work is complete or the session reaches 12 continuous hours
  - recording stop details if the 12-hour limit is what ends the session
  - clearing the active session entry from `work/task-board.md` when the work is done
- Updated `work/task-board.md` with an `Active Work Session` convention and template so the rule is visible where work is tracked.

## Verification

- Reviewed the updated `AGENTS.md` persistence and work-tracking rules.
- Reviewed the updated `work/task-board.md` active-session template and cleanup instructions.

## Follow-up

- Use the `Active Work Session` section on the next substantial task.
- Keep the live session entry updated during the work and clear it immediately when the work completes.
