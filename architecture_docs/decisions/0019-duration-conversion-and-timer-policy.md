# ADR-0019: Duration conversion and timer policy

- Status: Accepted
- Date: 2026-07-22
- Roadmap decision: Phase 3 Duration gap-fill policy

## Context

ADR-0007 fixes the accepted language representation, constructors, operators,
ordering, and exact direct ABI. Those decisions leave four observable policy
questions: conversion rounding, rendering of sub-millisecond values, rejection
of values that a host timer cannot represent, and the representation of an
omitted `process.run` timeout.

## Accepted decision

- `Duration.to_ms() -> float64` and `Duration.to_seconds() -> float64` convert
  the exact signed nanosecond value to the named unit and choose the nearest
  representable IEEE-754 binary64 value, ties to even.
- Human rendering is an exact decimal millisecond value followed by `ms`.
  It uses at most six fractional digits because the stored unit is one
  nanosecond, and removes trailing fractional zeros and an empty decimal point.
  Examples include `2000ms`, `1.5ms`, and `-0.000001ms`.
- Negative Duration values are valid language values but invalid host waits,
  deadlines, and restart backoffs. A non-negative value can also be invalid for
  a host API when it cannot be converted to the host timer range or when adding
  it to the current instant would overflow. Deadline overflow is an error; it
  never means an unlimited wait.
- A timer-taking API whose declared outcome can carry `io.Error` reports
  `io.Error.InvalidInput`. A process outcome that can carry `process.Error`
  reports `process.Error.Io(io.Error.InvalidInput)`. An API with no such typed
  error carrier traps with `AU4001`. Both maintained backends apply the same
  classification.
- `process.Supervisor.wait` has no direct error arm, so an invalid timer is
  represented exactly as
  `SupervisorWait.Event(SupervisorEvent.Failed("<supervisor>", Error.Io(io.Error.InvalidInput), 0))`.
  `wait_or_none` has a result error arm and therefore returns
  `Result.Err(Error.Io(io.Error.InvalidInput))` instead.
- Omitting `process.run(timeout=...)` uses an internal absence marker that no
  Aurora `Duration` value can spell. An explicit negative Duration is invalid
  input and never means an omitted or unlimited timeout.

These choices were accepted at the Batch 3 entry checkpoint. The
accepted signed-i128 nanosecond representation and arithmetic in ADR-0007 do
not depend on their ratification.

## Completion tests

- Conversion tests at exact, rounded, negative, and signed-128-bit boundary
  values through MIR and direct execution.
- Rendering tests for integral milliseconds, all six fractional positions,
  negative sub-millisecond values, and trimmed zeros.
- Negative, host-range, and deadline-overflow tests across scheduler, network,
  process, and supervisor timer entry points, with their declared typed outcome
  or `AU4001` diagnostic, including the exact supervisor sentinel payload.
- `process.run` tests that distinguish omission, explicit zero, and explicit
  negative timeouts without a sentinel collision.
