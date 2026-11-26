# ADR-0138 Conditional Suites

## Context

An ordinary runtime branch cannot hide an unavailable import or a foreign declaration. Removing unselected source before parsing would also hide syntax errors and lose editor locations.

## Decision

Reuse Aura's indented `if`, `elif` and `else` suites. Parse each arm in the surrounding syntactic context. A condition composed of build constants and marked module bindings can select declarations. Evaluate the predicate using the selected compilation configuration. Record half-open source line ranges for discarded arms. Select active declarations before resolving their imports and collecting types.

Keep function scope boundaries for the selected suite. A function parameter or local name in a condition makes it a runtime condition. Nested selection never resolves names inside an enclosing discarded arm.

## Consequences

Two definitions with the same name can occupy exclusive arms. Inactive declarations create no exports or link requirements. A syntax error in either arm still has its original source location.
