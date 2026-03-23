# Generics

Aurora now supports user-defined generic classes, enums, and functions.

## Generic Classes

```python
class Box[T]:
    value: T

    def get(borrow self) -> T:
        return self.value
```

You can use a generic class through an explicit type:

```python
boxed: Box[int32] = Box(value=7)
print(boxed.get())
```

The current compiler supports inferring the constructor type arguments from:

- the surrounding expected type
- the provided field values

## Generic Enums

```python
enum Wrapper[T]:
    Item(T)
```

```python
wrapped: Wrapper[String] = Wrapper.Item("ok")

match wrapped:
    case Wrapper.Item(value):
        print(value)
```

Payload matching uses the instantiated payload type, so `value` is a `String` in the example above.

## Generic Functions

```python
def identity[T](value: T) -> T:
    return value
```

Aurora currently infers function type arguments from:

- call arguments
- the expected return type when one is available

Example:

```python
print(identity(7))
text: String = identity("aurora")
print(text)
```

## Current Limits

The implemented generic surface currently supports:

- generic `class`, `enum`, and `def` declarations
- generic type arguments in type positions such as `Box[int32]`
- inference for generic function calls
- inference for generic class constructors and enum payload constructors

It does not yet support:

- traits and generic trait bounds
- explicit type arguments on call expressions
- generic imports across modules, because the module system is still being implemented

See [examples/generics/box_and_wrapper.au](../examples/generics/box_and_wrapper.au).
