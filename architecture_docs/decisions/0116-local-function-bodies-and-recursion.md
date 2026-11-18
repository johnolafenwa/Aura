# ADR-0116: Local Function Bodies And Recursion

Status: Accepted

## Context

A lambda gives an expression a callable signature and a capture environment.
A helper that needs a loop, intermediate bindings or an early return needs a
statement body. Its enclosing function already determines which values are
available to capture.

## Decision

A `def` inside a function declares a block-scoped closure with written
parameter and result types. Its optional capture list follows the function
name. The declared name is immutable. A mutable closure can update its
environment through its own declaration. Returning or storing the value
uses the same owned callable contract as a lambda.

The checker gives the body its own return target and loop context. It
computes the call kind from the capture operations. A shared or capture-free
local function can call itself directly. A recursive call reuses the
active captures. A mutable or consuming function must pass recursive
state as a parameter. Each statement sees declarations that precede it.

## Consequences

Statement bodies and expression bodies share capture, ownership and
calling-convention rules. A parameter list with explicit types can stand
alone. Capture-free functions retain the thin function representation.
The formatter keeps local definitions in the surrounding block.

[First-Class Closures Design Checkpoint](../30-first-class-closures-design-checkpoint.md)
