# ADR-0008: Task-result ownership

- Status: Accepted
- Date: 2026-07-13
- Roadmap decision: D8

> **Static enforcement amendment (Phase 5.6).** Provisional ADR-0033 defines
> the compiler-derived mechanism. `Task[T]` is copyable only when `T` is
> copyable, a Queue handle, or a recursively repeatable Task handle.
> Otherwise direct observation consumes the unique right on every outcome,
> multi-task waits consume the whole task vector, and `wait_any` abandons
> unchosen rights.

## Decision

Task results containing exclusive resources are single-consumer. Repeated
observation is permitted only for copy data and explicitly shared,
synchronized handles.

## Completion tests

- Semantic capability tests for nested result types.
- MIR/direct task-result tests for repeatable data and single-consumer resources.
- Scheduler model/stress tests for competing observers.
- Concurrency, ownership, API, and limits documentation.
