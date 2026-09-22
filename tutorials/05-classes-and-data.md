# Classes And Data

This chapter shows how to declare a class, construct and read its values, and
give it methods. It also covers default values, visibility, copy classes, and
recursive fields.

## Declaring A Class

A class lists its fields and their types:

```aura check-pass
class Point:
    x: float64
    y: float64
```

See [examples/classes/point_distance.au](../examples/classes/point_distance.au).

A class can take type parameters:

```aura check-pass
class Box[T]:
    value: T
```

Declare a `copy class` to make the class a copy type. Every field must itself
be copyable:

```aura check-pass
copy class Point:
    x: int32
    y: int32
```

See [examples/classes/copy_class.au](../examples/classes/copy_class.au).

## Constructing A Value

Call the class name with a named argument for each field:

```aura fragment
p1 = Point(x=0.0, y=0.0)
```

Positional field arguments also work, as long as they come before any named
fields. Here `Point(7)` leaves `y` at its default value of 9:

```aura check-pass
class Point:
    x: int32
    y: int32 = 9

first = Point(1, 2)
second = Point(7)
```

## Accessing Fields

Read a field with a dot:

```aura fragment
dx = a.x - b.x
```

Reading a non-copy field from a value you own moves that field out of the
instance. The other fields stay readable. You cannot read the moved field again
until you assign a new value to it. [Ownership And Borrowing](06-ownership-and-borrowing.md)
explains move semantics, copy types, and common patterns for working with
fields.

## Default Field Values

A field can declare a default value:

```aura check-pass
class ServerConfig:
    host: str = "localhost"
    port: int32 = 8080
```

Construction can then leave that field out. `local` uses both defaults, and
`named` overrides only `host`:

```aura fragment
local = ServerConfig()
named = ServerConfig(host="aura.dev")
```

See [examples/classes/default_fields.au](../examples/classes/default_fields.au).

## Recursive Fields With `indirect`

A field whose type refers back to its own class must be marked `indirect`. An
`indirect` field stores the child out of line, so the parent keeps a finite
size:

```aura check-pass
class Node:
    value: int32
    next: indirect Node | None
```

An optional field is a union with `None`. The `|` operator binds more loosely
than `indirect`, so `indirect Node | None` is an optional owned child stored
indirectly. The `indirect` marker on the member marks the whole field.

See [examples/classes/indirect_recursive.au](../examples/classes/indirect_recursive.au).

## `public` Fields And Methods

Fields and methods are private by default. Mark them `public` to use them from
another module:

```aura check-pass
class User:
    public name: str
    age: int32

    public def read_name(self) -> str:
        return self.name.clone()
```

Code in the same module can use private fields and methods. Aura enforces
visibility across module boundaries, so code in another module:

- sees only the `public` participating fields when it constructs the class by
  keyword arguments
- cannot read a private field
- cannot call a private method

See [examples/modules/simple_import.au](../examples/modules/simple_import.au).

## Methods

Declare methods inside the class body:

```aura check-pass
class Counter:
    value: int32

    def read(self) -> int32:
        return self.value
```

Call a method through an instance:

```aura fragment
counter = Counter(value=4)
print(counter.read())
```

## Receiver Forms

The receiver is a method's first parameter. It decides what the method may do
with the instance:

| Receiver | Access |
|----------|--------|
| `self` | Shared, read-only access. This is the default spelling. |
| `own self` | Consuming. Takes ownership of a non-copy instance. |
| `mut self` | Mutable. Exclusive access that can change fields in place. |
| No receiver | Associated method. Called on the class, not an instance. |

[Ownership And Borrowing](06-ownership-and-borrowing.md) explains how
borrowing works and why these forms differ.

A receiver must come first and never has a type annotation. The compiler
rejects `self: Counter`, because it looks like an instance receiver but would
be an ordinary parameter. Write `self`, `own self`, or `mut self` instead.

This class uses all four forms:

```aura check-pass
class Counter:
    value: int32

    def take(own self) -> int32:
        return self.value

    def read(self) -> int32:
        return self.value

    def bump(mut self):
        self.value += 1

    def zero() -> Counter:
        return Counter(value=0)
```

Method calls follow the same argument rules as ordinary functions. Methods and
associated methods both accept named arguments:

```aura check-pass
class Greeter:
    prefix: str

    def say(self, name: str) -> str:
        return self.prefix + name

    def named(prefix: own str) -> Greeter:
        return Greeter(prefix=prefix)

greeter = Greeter.named(prefix="hello, ")
print(greeter.say(name="aura"))
```

## Associated Methods

A method without a receiver is an associated method. Call it through the class
name:

```aura check-pass
class Counter:
    value: int32

    def zero() -> Counter:
        return Counter(value=0)
```

```aura fragment
print(Counter.zero().read())
```

See [examples/classes/methods.au](../examples/classes/methods.au).

## Mutating Methods

A `mut self` method can assign to the fields of `self`:

```aura check-pass
class Counter:
    value: int32

    def bump(mut self):
        self.value += 1

    def reset(mut self):
        self.value = 0
```

Call it on a `mut` binding:

```aura fragment
mut counter = Counter(value=4)
counter.bump()
counter.reset()
```

See [examples/classes/mutating_methods.au](../examples/classes/mutating_methods.au).

## Current Limits

Separate `impl` blocks are not implemented. Declare every method inside the
class body.
