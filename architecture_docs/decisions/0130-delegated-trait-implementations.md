# ADR-0130: Delegate A Trait To A Field

- Status: Proposed
- Date: 2026-11-24

## Decision

An implementation can name the field that supplies its methods.

```aura fragment
impl Named for Car by engine
```

The field must be owned and its specialized type must implement the requested trait. A missing implementation is an error even when a field has an inherent method with the right name and signature. The field supplies associated types. An indented implementation suite can override individual methods.

Forwarding includes defaults and supertraits. A field implementation that replaces a trait default keeps that behavior through the outer class. Reuse an existing implementation of a supertrait on the outer class. Otherwise generate a parent implementation so a parent bound and a trait-object upcast see the same methods.

A forwarded method takes the receiver mode and ordinary parameter modes of its trait. It projects the field and calls the corresponding trait method. The usual borrow and move checks apply to that projection. Contract checking and verification see a method with the trait's preconditions and postconditions. Traits that require the outer `Self` as a result may require a hand-written override.

## Consequences

A consumer can use the outer class anywhere its implemented trait is required. Other fields remain private to the class unless its own declarations expose them. Each type keeps its existing layout. Delegation introduces no base-class subobject or conversion between the class and field types.
