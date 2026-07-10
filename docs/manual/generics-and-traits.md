# Generics And Traits

Generics parameterize code over types. Traits describe required behavior.

## Generic Classes And Enums

```python
class Box[T]:
    value: T

enum MaybePair[T]:
    One(value: T)
    Two(left: T, right: T)
```

## Generic Functions

```python
def identity[T](value: T) -> T:
    return value
```

## Traits

```python
trait Greeter:
    def greet(borrow self) -> String
```

Implement a trait:

```python
class Person:
    name: String

impl Greeter for Person:
    def greet(borrow self) -> String:
        return "hello " + self.name
```

## Bounds

```python
def say_hello[T: Greeter](value: borrow T):
    print(value.greet())
```

Multiple bounds:

```python
def use_value[T: Display + Score](value: borrow T) -> int32:
    print(value.display())
    return value.score()
```

Specialized generic bounds:

```python
trait Mapper[T]:
    def map(borrow self, value: T) -> T

def apply[M: Mapper[int32]](mapper: borrow M, value: int32) -> int32:
    return mapper.map(value)
```

## Supertraits

```python
trait Child: Parent:
    def child_method(borrow self) -> int32
```

Implementations must satisfy inherited trait requirements.

## Operator Traits

Aurora supports operator-trait dispatch for:

- binary `+`, `-`, `*`, `/`, `%`
- unary `-`
- `not`
- ordering operators through supported ordering traits

Use operator traits when a domain type has a natural numeric-like operation.
