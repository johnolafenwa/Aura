# Traits

Aurora now supports trait declarations, explicit `impl Trait for Type` conformance blocks, and bounded generic calls.

## Declaring A Trait

Trait methods are signature-only in the current compiler, but empty marker traits are also allowed with `pass`:

```python
trait Greeter:
    def greet(borrow self) -> String
```

```python
trait Marker:
    pass
```

## Implementing A Trait

Use a conformance block:

```python
class User:
    name: String

impl Greeter for User:
    def greet(borrow self) -> String:
        return "hello " + self.name
```

The current compiler also supports impls for specialized generic instances:

```python
class Box[T]:
    value: T

impl Greeter for Box[String]:
    def greet(borrow self) -> String:
        return self.value.clone()
```

## Calling Through A Trait Bound

Generic functions can require a trait with inline bounds:

```python
def speak[T: Greeter](value: T):
    print(value.greet())
```

Class and enum type parameters may also use trait bounds:

```python
class Wrapper[T: Greeter]:
    value: T
```

Multiple bounds use `+`:

```python
def use_both[T: A + B](value: T) -> int32:
    return value.a() + value.b()
```

At the call site, Aurora checks that the concrete type implements the required trait:

```python
def main() -> int32:
    speak(value=User(name="aurora"))
    return 0
```

## Current Limits

The implemented trait surface currently supports:

- `trait Name:` declarations
- empty marker traits with `pass`
- method signatures inside trait bodies
- `impl Trait for Type:` blocks
- `impl Trait for GenericType[ConcreteType]:` specialized impls
- bounded generic functions and methods with `T: Trait`
- bounded generic classes and enums with `T: Trait`
- multiple bounds with `T: A + B`
- direct trait-method calls on concrete types that implement the trait

Still outside the current bootstrap compiler:

- generic trait declarations like `trait Add[T]:`
- generic impl headers
- operator overloading traits
