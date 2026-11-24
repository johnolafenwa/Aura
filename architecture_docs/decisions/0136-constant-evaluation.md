# ADR-0136 Constant Evaluation

## Context

Module bindings have a declared initialization order and a read-only ownership contract. Deciding a pure initializer at build time must preserve both.

## Decision

Use the typed MIR interpreter for computed initializers. The evaluator classifies reachable calls before executing an initializer. It can use local state and previously frozen bindings. An initializer with an observable effect keeps runtime initialization. Writing `const` requires a build-time value.

Give each initializer one budget shared with its callees, comprising 100,000 MIR steps and 8 MiB of logical allocation. Report traps and exhausted budgets at compile time. Preserve Aura call frames. Integer width, floating-point rounding and the order of collection elements belong to the Aura target.

## Consequences

An impure initializer can keep the existing module initialization order. A pure initializer that traps has one diagnostic on both execution backends. A compiler can skip the interpreter for scalar literal bindings.
