# Classes And Data

The implemented class model currently covers fields, default values,
positional and named construction, member access, `public` fields and methods,
instance methods, associated methods, mutating methods, explicit `copy class`
declarations, and indirect recursive fields.

## Declaring A Class

```aura check-pass
class Point:
    x: float64
    y: float64
```

See [examples/classes/point_distance.au](../examples/classes/point_distance.au).

Generic classes are also supported:

```aura check-pass
class Box[T]:
    value: T
```

Aura also supports explicit copy classes when every field is itself copyable:

```aura check-pass
copy class Point:
    x: int32
    y: int32
```

See [examples/classes/copy_class.au](../examples/classes/copy_class.au).

## Constructing A Value

Class construction accepts named fields:

```aura fragment
p1 = Point(x=0.0, y=0.0)
```

## Accessing Fields

```aura fragment
dx = a.x - b.x
```

Reading a non-copy field from an owned value moves that field out of the instance. You can still
read other untouched fields, but you cannot read the moved field again until you assign a new value
back into it. See [06-ownership-and-borrowing.md](06-ownership-and-borrowing.md) for a full
explanation of move semantics, copy types, and common patterns for working with fields.

## Default Field Values

The implemented subset supports field defaults:

```aura check-pass
class ServerConfig:
    host: str = "localhost"
    port: int32 = 8080
```

You can then omit those fields during construction:

```aura fragment
local = ServerConfig()
named = ServerConfig(host="aura.dev")
```

See [examples/classes/default_fields.au](../examples/classes/default_fields.au).

## Recursive Fields With `indirect`

Recursive class fields must be marked `indirect`. This gives the child an
out-of-line representation and keeps the parent size finite:

```aura check-pass
class Node:
    value: int32
    next: indirect Node?
```

The `?` suffix is shorthand for `Option[...]`, so `indirect Node?` means an optional owned child stored indirectly.

See [examples/classes/indirect_recursive.au](../examples/classes/indirect_recursive.au).

## `public` Fields And Methods

Aura enforces class visibility across module boundaries. Fields and methods are private by default and must be marked `public` to be used from another module:

```aura check-pass
class User:
    public name: str
    age: int32

    public def read_name(self) -> str:
        return self.name.clone()
```

Within the same module, private fields and methods are still accessible. Across modules:

- constructing a class by keyword arguments only exposes `public` participating fields
- reading a private field is rejected
- calling a private method is rejected

See [examples/modules/simple_import.au](../examples/modules/simple_import.au).

## Methods

Aura supports methods declared directly inside the class body.

```aura check-pass
class Counter:
    value: int32

    def read(self) -> int32:
        return self.value
```

## Receiver Forms

The current compiler accepts these receiver forms. For a full explanation of how borrowing works and why these distinctions matter, see [06-ownership-and-borrowing.md](06-ownership-and-borrowing.md).

- `self`
  - shared receiver and the default spelling; read-only access
- `own self`
  - consuming receiver; takes ownership of a non-copy instance
- `mut self`
  - mutable receiver; exclusive access, can modify fields in place
- no receiver
  - associated method; called on the class, not an instance

A receiver must be first and is never typed explicitly. `self: Counter` is
rejected because it looks like an instance receiver but would otherwise be an
ordinary parameter; use `self`, `own self`, or `mut self`.

Example:

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

Call them through an instance:

```aura fragment
counter = Counter(value=4)
print(counter.read())
```

Method calls follow the same argument rules as ordinary functions, so methods and associated methods can also use named arguments:

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

Methods without a receiver are called through the class name:

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

Aura supports `mut self` methods and member-target assignment.

```aura check-pass
class Counter:
    value: int32

    def bump(mut self):
        self.value += 1

    def reset(mut self):
        self.value = 0
```

```aura fragment
mut counter = Counter(value=4)
counter.bump()
counter.reset()
```

See [examples/classes/mutating_methods.au](../examples/classes/mutating_methods.au).

Constructors support positional field arguments as long as they come before any named fields:

```aura check-pass
class Point:
    x: int32
    y: int32 = 9

first = Point(1, 2)
second = Point(7)
```

## Current Limits

The bootstrap compiler does not yet support:

- separate `impl` blocks
