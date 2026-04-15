# Generics

Generics let you write code that works with any type. Instead of writing separate `BoxInt`, `BoxString`, and `BoxFloat` classes, you write one `Box[T]` that works with all of them.

## Generic Classes

```python
class Box[T]:
    value: T

    def get(borrow self) -> T:
        return self.value
```

Use it with any type:

```python
int_box: Box[int32] = Box(value=7)
print(int_box.get())

text_box: Box[String] = Box(value="hello")
print(text_box.get())
```

You can also provide the type argument explicitly at the constructor:

```python
boxed = Box[int32](value=7)
```

The compiler infers type arguments from the surrounding expected type or the provided field values, so explicit arguments are optional when the type is clear.

## Bounded Type Parameters

Sometimes a generic class should only accept types that implement a specific trait (see [16-traits.md](16-traits.md)):

```python
class Wrapper[T: Greeter]:
    value: T
```

This restricts `T` to types that implement `Greeter`. Attempting to construct a `Wrapper` with a type that does not implement `Greeter` produces a compile error.

## Generic Enums

Enums can also be generic. Unit variants and payload variants both work:

```python
enum Wrapper[T]:
    Item(T)
    Empty
```

```python
wrapped: Wrapper[String] = Wrapper.Item("ok")

match wrapped:
    case Wrapper.Item(value):
        print(value)    # value is String
    case Wrapper.Empty:
        print("empty")
```

Payload matching uses the instantiated payload type, so `value` is a `String` in the example above.

Generic enums may also use bounded type parameters:

```python
enum MaybeNamed[T: Greeter]:
    Some(T)
    Empty
```

## Generic Functions

```python
def identity[T](value: T) -> T:
    return value
```

The compiler infers type arguments from call arguments and the expected return type:

```python
print(identity(7))               # infers T = int32
text: String = identity("aurora") # infers T = String
print(text)
```

Method calls on generic class instances work inside generic functions:

```python
def extract[T](box: Box[T]) -> T:
    return box.get()
```

## Current Limits

The implemented generic surface supports:

- generic `class`, `enum`, and `def` declarations
- generic `trait` declarations
- trait bounds on type parameters
- explicit type arguments on constructors like `Box[int32](...)`
- inference for generic function calls and constructors
- method calls on generic instances inside generic functions
- generic enum unit variants with instantiated types
- generic trait impl headers like `impl Mapper[T] for Box[T]:`

See [examples/generics/box_and_wrapper.au](../examples/generics/box_and_wrapper.au), [examples/generics/generic_method_calls.au](../examples/generics/generic_method_calls.au), [examples/generics/generic_constructor_specialization.au](../examples/generics/generic_constructor_specialization.au), and [examples/generics/bounded_types.au](../examples/generics/bounded_types.au).
