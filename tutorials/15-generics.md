# Generics

A generic definition takes type parameters, so one `Box[T]` works across
every type that meets its requirements. You do not need separate `BoxInt`,
`BoxString`, and `BoxFloat` classes.

## Generic Classes

Write the type parameters in brackets after the class name:

```aura check-pass
class Box[T]:
    value: T

    def get(own self) -> T:
        return self.value
```

Use it with any type. This prints `7` and then `hello`:

```aura fragment
int_box: Box[int32] = Box(value=7)
print(int_box.get())

text_box: Box[str] = Box(value="hello")
print(text_box.get())
```

You can also pass the type argument to the constructor:

```aura fragment
boxed = Box[int32](value=7)
```

Explicit arguments are optional when the type is clear. The compiler infers
them from the surrounding expected type or from the field values.

## Bounded Type Parameters

A bound restricts a type parameter to types that implement a trait. See
[16-traits.md](16-traits.md) for traits.

```aura fragment
class Wrapper[T: Greeter]:
    value: T
```

Here `T` must implement `Greeter`. Constructing a `Wrapper` with a type that
does not implement `Greeter` is a compile error.

## Generic Enums

Enums can be generic too. Both unit variants and payload variants work:

```aura check-pass
enum Wrapper[T]:
    Item(T)
    Empty
```

A match sees the instantiated payload type, so `value` is a `str` here:

```aura fragment
wrapped: Wrapper[str] = Wrapper.Item("ok")

match wrapped:
    case Wrapper.Item(value):
        print(value)    # value is str
    case Wrapper.Empty:
        print("empty")
```

Generic enums can use bounded type parameters:

```aura fragment
enum MaybeNamed[T: Greeter]:
    Some(T)
    Empty
```

## Generic Functions

```aura check-pass
def identity[T](value: own T) -> T:
    return value
```

The compiler infers type arguments from the call arguments and the expected
return type:

```aura fragment
print(identity(7))               # infers T = int64
text: str = identity("aura") # infers T = str
print(text)
```

A generic function can call methods on a generic class instance:

```aura fragment
def extract[T](box: own Box[T]) -> T:
    return box.get()
```

Use `own T` when the body returns, stores, or otherwise consumes the value. A
bare `value: T` is fixed as a shared borrow when the declaration is checked.
It stays shared even if a later call passes a copy type.

## Inferred Clone-Safety

A generic body may clone values of an unresolved `T`. Aura accepts the
declaration and records that `T` must be clone-safe:

```aura check-pass
def duplicate[T](values: list[T]) -> list[T]:
    return values.copy()

def forward[T](values: list[T]) -> list[T]:
    return duplicate(values)
```

- A call with `int32` or `str` works.
- A call with `random.Rng` is rejected with `AU3007`. This includes an
  `Rng` inside a class, enum, or collection wrapper.
- `forward` inherits the same requirement through its generic-to-generic
  call.
- The requirement survives a module import. Moving the helper to another file
  does not give callers a clone route.

### Clone Safety And Task Transport

Clone safety and task transport are separate rules.

- A `Queue[T]` handle has copyable identity. Constructing a queue or sending
  through it still needs a concrete `T: Transfer`, so `Queue[random.Rng]()`
  is rejected with `AU3008`.
- `Task[T]` is always a transferable handle, but it is copyable only when
  `T` is repeatable. `random.Rng` is neither `Transfer` nor repeatable, so a
  task may not return it.

`Transfer` is the property a value needs to cross into a task. Aura does not
infer a deferred `Transfer` obligation for an unresolved type parameter. A
generic task target must be fully specialized before `TaskGroup.start(...)`
can validate its captured arguments and result. Specialization can come from
inference, from defaults, or from the narrow explicit target form
`function[Types]` and the equivalent associated-method form.

## Current Limits

The generic surface supports:

- generic `class`, `enum`, and `def` declarations
- generic `trait` declarations
- trait bounds on type parameters
- explicit type arguments on constructors like `Box[int32](...)`
- inference for generic function calls and constructors
- method calls on generic instances inside generic functions
- generic enum unit variants with explicit type arguments, such as
  `Maybe[int32].Nothing`
- generic trait impl headers like `impl Mapper[T] for Box[T]:`
- inferred clone-safety obligations, propagated through generic-to-generic
  calls and imports

Runnable examples:

- [examples/generics/box_and_wrapper.au](../examples/generics/box_and_wrapper.au)
- [examples/generics/generic_method_calls.au](../examples/generics/generic_method_calls.au)
- [examples/generics/generic_constructor_specialization.au](../examples/generics/generic_constructor_specialization.au)
- [examples/generics/bounded_types.au](../examples/generics/bounded_types.au)
- [examples/generics/clone_safety_obligations.au](../examples/generics/clone_safety_obligations.au)
