# Generics

Generics let one `Box[T]` definition work across the types that satisfy its
requirements. Separate `BoxInt`, `BoxString`, and `BoxFloat` classes are not
needed.

## Generic Classes

```python
class Box[T]:
    value: T

    def get(own self) -> T:
        return self.value
```

Use it with any type:

```python
int_box: Box[int32] = Box(value=7)
print(int_box.get())

text_box: Box[str] = Box(value="hello")
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
wrapped: Wrapper[str] = Wrapper.Item("ok")

match wrapped:
    case Wrapper.Item(value):
        print(value)    # value is str
    case Wrapper.Empty:
        print("empty")
```

Payload matching uses the instantiated payload type, so `value` is a `str` in the example above.

Generic enums may also use bounded type parameters:

```python
enum MaybeNamed[T: Greeter]:
    Some(T)
    Empty
```

## Generic Functions

```python
def identity[T](value: own T) -> T:
    return value
```

The compiler infers type arguments from call arguments and the expected return type:

```python
print(identity(7))               # infers T = int64
text: str = identity("aura") # infers T = str
print(text)
```

Method calls on generic class instances work inside generic functions:

```python
def extract[T](box: own Box[T]) -> T:
    return box.get()
```

The `own` spelling matters for unresolved generics. A bare `value: T` is fixed
as a shared borrow when this declaration is checked and stays shared even if a
call later uses a copy type. Use `own T` when the body returns, stores, or
otherwise consumes the value.

## Inferred Clone-Safety

A generic body may clone values without rejecting the declaration merely
because `T` is unresolved:

```python
def duplicate[T](values: list[T]) -> list[T]:
    return values.copy()

def forward[T](values: list[T]) -> list[T]:
    return duplicate(values)
```

Aura infers that `T` must be clone-safe. A call with `int32` or `str`
works. A call with `random.Rng`, including through a class, enum, or collection
wrapper, is rejected with `AU3007`. `forward` receives the same requirement
through its generic-to-generic call. The inferred contract also survives a
module import; callers do not gain a clone route by moving the helper to
another file.

Clone safety and task transport are separate rules. A `Queue[T]` handle has
copyable identity, but constructing or sending through the queue requires
concrete `T: Transfer`, so `Queue[random.Rng]()` is rejected
with `AU3008`. `Task[T]` is always a transferable handle, but is copyable only
when `T` is repeatable; `random.Rng` is neither `Transfer` nor repeatable, so a
task may not return it.

Aura does not infer a deferred `Transfer` obligation for an unresolved type
parameter. A generic task target must be fully specialized by inference,
defaults, or the narrow explicit target form `function[Types]` (and the
equivalent associated-method form) before `TaskGroup.start(...)` can validate
its captured arguments and result.

## Current Limits

The implemented generic surface supports:

- generic `class`, `enum`, and `def` declarations
- generic `trait` declarations
- trait bounds on type parameters
- explicit type arguments on constructors like `Box[int32](...)`
- inference for generic function calls and constructors
- method calls on generic instances inside generic functions
- generic enum unit variants with explicit type arguments such as `Maybe[int32].Nothing`
- generic trait impl headers like `impl Mapper[T] for Box[T]:`
- inferred clone-safety obligations with generic-to-generic and imported
  propagation

See [examples/generics/box_and_wrapper.au](../examples/generics/box_and_wrapper.au), [examples/generics/generic_method_calls.au](../examples/generics/generic_method_calls.au), [examples/generics/generic_constructor_specialization.au](../examples/generics/generic_constructor_specialization.au), [examples/generics/bounded_types.au](../examples/generics/bounded_types.au), and [examples/generics/clone_safety_obligations.au](../examples/generics/clone_safety_obligations.au).
