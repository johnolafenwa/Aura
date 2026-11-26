# ADR-0133: Decorators Wrap Methods Before Receiver Binding

- Status: Accepted
- Date: 2026-11-24

## Decision

A decorator on an inherent method takes an unbound callable. Its first
parameter is named `self` and has the type of the enclosing class. The
method's receiver determines whether the parameter takes shared, mutable,
or owned access. Associated methods use their ordinary parameter list.

```aura fragment
type Read = Callable[def(self: Counter) -> int64]

def double(f: own Read) -> Read:
    return Read(lambda [own f] self: f(self) * 2)
```

A method retains its visibility, receiver checks, and parameter names when
decorated. Binding a method to a value captures its receiver as usual.
Direct calls, bound calls, and recursion through the member reach the same
decorated callable. A mutable receiver still requires a mutable place.

The defining module initializes the decorator group once, in declaration
order. It evaluates decorator expressions from top to bottom and applies
them from bottom to top. A method with a generic signature has no single
callable type at module initialization. Generic methods and methods of
generic classes therefore follow the restriction on generic free functions.
An initializer needs exclusive access to a partly constructed object, which
a decorator's ordinary callable cannot accept.

## Lowering

Keep the source method as the entry point for member lookup. Pack its
original body as a private associated function with an explicit receiver
parameter. Apply the decorators using the module-level constant machinery
from ADR-0053. The method's entry point passes its checked arguments to the
constant. Both back ends lower these calls using ordinary callable MIR.

The resulting callable can be shared by all instances. Decorating a bound
method instead would evaluate the decorator for each receiver and prevent
module initialization from establishing a single binding.
