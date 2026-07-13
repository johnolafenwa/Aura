# ADR-0008: Task-result ownership

- Status: Accepted
- Date: 2026-07-13
- Roadmap decision: D8

## Decision

Task results containing exclusive resources are single-consumer. Repeated
observation is permitted only for copy data and explicitly shared,
synchronized handles.

## Completion tests

- Semantic capability tests for nested result types.
- MIR/direct task-result tests for repeatable data and single-consumer resources.
- Scheduler model/stress tests for competing observers.
- Concurrency, ownership, API, and limits documentation.
