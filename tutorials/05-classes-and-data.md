# Classes And Data

The implemented class model currently covers fields, default values, keyword construction, member access, `public` fields and methods, instance methods, associated methods, mutating methods, explicit `copy class` declarations, and indirect recursive fields.

## Declaring A Class

```python
class Point:
    x: float64
    y: float64
```

See [examples/classes/point_distance.au](../examples/classes/point_distance.au).

Generic classes are also supported:

```python
class Box[T]:
    value: T
```

Aurora also supports explicit copy classes when every field is itself copyable:

```python
copy class Point:
    x: int32
    y: int32
```

See [examples/classes/copy_class.au](../examples/classes/copy_class.au).

## Constructing A Value

Aurora currently uses keyword-style construction:

```python
p1 = Point(x=0.0, y=0.0)
```

## Accessing Fields

```python
dx = a.x - b.x
```

## Default Field Values

The implemented subset already supports field defaults:

```python
class ServerConfig:
    host: String = "localhost"
    port: int32 = 8080
```

You can then omit those fields during construction:

```python
local = ServerConfig()
named = ServerConfig(host="aurora.dev")
```

See [examples/classes/default_fields.au](../examples/classes/default_fields.au).

## Recursive Fields With `indirect`

Recursive class fields must be marked `indirect` so the value is stored out of line instead of inline:

```python
class Node:
    value: int32
    next: indirect Node?
```

The `?` suffix is shorthand for `Option[...]`, so `indirect Node?` means an optional owned child stored indirectly.

See [examples/classes/indirect_recursive.au](../examples/classes/indirect_recursive.au).

## `public` Fields And Methods

Aurora now enforces class visibility across module boundaries. Fields and methods are private by default and must be marked `public` to be used from another module:

```python
class User:
    public name: String
    age: int32

    public def read_name(borrow self) -> String:
        return self.name.clone()
```

Within the same module, private fields and methods are still accessible. Across modules:

- constructing a class by keyword arguments only exposes `public` participating fields
- reading a private field is rejected
- calling a private method is rejected

See [examples/modules/simple_import.au](../examples/modules/simple_import.au).

## Methods

Aurora supports methods declared directly inside the class body.

```python
class Counter:
    value: int32

    def read(borrow self) -> int32:
        return self.value
```

## Receiver Forms

The current compiler accepts these receiver forms:

- `self`
  - by-value receiver
- `borrow self`
  - shared receiver
- `borrow mut self`
  - mutable receiver
- no receiver
  - associated method

Example:

```python
class Counter:
    value: int32

    def take(self) -> int32:
        return self.value

    def read(borrow self) -> int32:
        return self.value

    def bump(borrow mut self):
        self.value += 1

    def zero() -> Counter:
        return Counter(value=0)
```

Call them through an instance:

```python
counter = Counter(value=4)
print(counter.read())
```

Method calls follow the same argument rules as ordinary functions, so methods and associated methods can also use named arguments:

```python
class Greeter:
    prefix: String

    def say(borrow self, name: String) -> String:
        return self.prefix + name

    def named(prefix: String) -> Greeter:
        return Greeter(prefix=prefix)

greeter = Greeter.named(prefix="hello, ")
print(greeter.say(name="aurora"))
```

## Associated Methods

Methods without a receiver are called through the class name:

```python
class Counter:
    value: int32

    def zero() -> Counter:
        return Counter(value=0)
```

```python
print(Counter.zero().read())
```

See [examples/classes/methods.au](../examples/classes/methods.au).

## Mutating Methods

Aurora now supports `borrow mut self` methods and member-target assignment.

```python
class Counter:
    value: int32

    def bump(borrow mut self):
        self.value += 1

    def reset(borrow mut self):
        self.value = 0
```

```python
mut counter = Counter(value=4)
counter.bump()
counter.reset()
```

See [examples/classes/mutating_methods.au](../examples/classes/mutating_methods.au).

## Current Limits

The bootstrap compiler does not yet support:

- separate `impl` blocks
- method visibility modifiers
- positional class constructor arguments; bootstrap constructors are keyword-only
