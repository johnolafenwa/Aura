# Traits

Aurora now supports trait declarations, explicit `impl Trait for Type` conformance blocks, and bounded generic calls.

## Declaring A Trait

Trait methods are signature-only in the current compiler:

```python
trait Greeter:
    def greet(borrow self) -> String
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

## Calling Through A Trait Bound

Generic functions can require a trait with inline bounds:

```python
def speak[T: Greeter](value: T):
    print(value.greet())
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
- method signatures inside trait bodies
- `impl Trait for Type:` blocks
- bounded generic functions and methods with `T: Trait`
- multiple bounds with `T: A + B`
- direct trait-method calls on concrete types that implement the trait

Still outside the current bootstrap compiler:

- generic trait declarations like `trait Add[T]:`
- generic impl headers
- operator overloading traits
- trait imports across modules, because the module system is still being implemented
