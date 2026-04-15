# Generics

Aurora now supports user-defined generic classes, enums, and functions, including bounds on class and enum type parameters.

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

Aurora also supports explicit type arguments on constructor calls when you want to state the instantiation directly at the call site:

```python
boxed = Box[int32](value=7)
```

The current compiler supports inferring the constructor type arguments from:

- the surrounding expected type
- the provided field values

Generic classes may also carry trait bounds on their type parameters:

```python
class Wrapper[T: Greeter]:
    value: T
```

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

Generic enums may also use bounded type parameters and unit variants:

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

Aurora currently infers function type arguments from:

- call arguments
- the expected return type when one is available

Example:

```python
print(identity(7))
text: String = identity("aurora")
print(text)
```

Method calls on generic class instances also work inside generic functions:

```python
class Box[T]:
    value: T

    def get(borrow self) -> T:
        return self.value

def extract[T](box: Box[T]) -> T:
    return box.get()
```

## Current Limits

The implemented generic surface currently supports:

- generic `class`, `enum`, and `def` declarations
- generic `trait` declarations
- trait bounds on class and enum type parameters
- generic type arguments in type positions such as `Box[int32]`
- explicit type arguments on class constructor calls such as `Box[int32](...)`
- inference for generic function calls
- inference for generic class constructors and enum payload constructors
- method calls on generic class instances inside generic functions
- generic enum unit variants with instantiated types
- generic trait impl headers such as `impl Mapper[T] for Box[T]:`

It does not yet support:

- generic trait bounds such as `T: Mapper[int32]`

See [examples/generics/box_and_wrapper.au](../examples/generics/box_and_wrapper.au), [examples/generics/generic_method_calls.au](../examples/generics/generic_method_calls.au), [examples/generics/generic_constructor_specialization.au](../examples/generics/generic_constructor_specialization.au), and [examples/generics/bounded_types.au](../examples/generics/bounded_types.au).
