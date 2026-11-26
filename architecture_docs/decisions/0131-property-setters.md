# ADR-0131: A Property Setter Is A Mutable Method

- Status: Accepted
- Date: 2026-11-24

## Context

A computed property gives class users field-like access while the class controls how to produce its value. A write needs a way to run class code on the same receiver without borrowing a temporary getter result.

## Decision

Pair a getter with `@name.setter` on a method named `name` in the same class. Both declarations may appear in either order. A setter takes `mut self` and one positional, shared or owning parameter of the getter's result type. Its result is `None`. Parameter defaults and method type parameters are invalid. A declaration error reports AU2064.

The checker resolves a property write using the receiver type. It lowers the write to a call of the setter. The setter has a distinct compiler name that source code cannot spell. Ordinary call checking enforces visibility, parameter modes, contracts, and exclusive receiver access.

A compound assignment reads the getter, evaluates the right operand and the operator, then calls the setter with the new value. An indexed receiver is selected once. A getter without a setter keeps AU2045 for attempted writes. Taking a view of a property or passing it as a mutable argument continues to report AU2045.

## Consequences

Both execution backends run the same method calls. Classes can validate or transform writes. Clients that need to lend mutable storage must expose a field or an explicit method returning a mutable view.
