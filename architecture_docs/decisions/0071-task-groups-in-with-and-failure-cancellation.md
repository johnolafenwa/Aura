# ADR-0071: Task groups live in `with`, and a child failure cancels the group

- Status: Accepted
- Date: 2026-11-02
- Builds on: ADR-0057 and ADR-0060

## Context

Structured concurrency rests on one promise: a task never outlives the block
that started it. Two gaps weakened it. `TaskGroup()` could be bound to an
ordinary variable, so a function could start a task and return with it still
running; when `main` ended, the runtime abandoned the task and skipped its
`with` exits. And a child that trapped left its siblings running to
completion before the group reported the failure, unlike Python's
`asyncio.TaskGroup` and Trio, which cancel the rest. John approved closing
both on 2026-11-02 (rows 1 and 6 of the language-design questions), knowing
both are breaking.

## Decision

- `TaskGroup()` is legal only as the item of a `with` statement
  (`with group = TaskGroup():` or `with TaskGroup() as group:`). Anywhere else
  it is rejected with `AU2019`. A helper that starts tasks takes the group as
  a `mut TaskGroup` parameter.
- When a child of a group fails (traps), the group is cancelled exactly as if
  `group.cancel()` had been called. Siblings observe it at their next wait or
  `cancelled()` check. The failure itself is reported as before: an unread
  failure aborts the scope, and a failure read through its `Task` is not
  raised again.
- A cancelled child is not a failure and does not cancel its siblings.

## Implementation

- The checker records the span of the `TaskGroup()` call it is checking as a
  `with` item; the constructor rule accepts only that span.
- `TaskGroupValue::register_task` also registers the group's cancel flag as
  one of the child's failure wake flags, so the shared runtime value sets it
  when the child completes with an error. The interpreter and native code use
  the same `TaskGroupValue`, so both backends change together.
