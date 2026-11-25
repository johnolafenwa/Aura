# ADR-0137 Frozen Module Data

## Context

A module binding of a non-Copy type lends one immutable value to all its readers. Native heap headers include ownership and type information specific to the running process.

## Decision

Carry a typed frozen value with its MIR module binding. Use scalar immediates where the target representation permits them. Emit hosted aggregate payloads in immutable data and attach a frozen runtime box on first read. Publish that box atomically through a binding-specific cell. Exclude frozen initializers from startup and omit their synthesized native thunks.

A freestanding object cannot call the Aura runtime. Its aggregate tables use read-only native storage and checked projections. The object can contain data for a collection whose construction would otherwise need a heap.

## Consequences

A hosted program that never reads a frozen binding incurs no startup allocation or initializer call for it. Repeated reads retain its shared identity. A freestanding read can link with the same trap hook used by ordinary checked arithmetic.
