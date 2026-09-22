# Traits

A trait names behavior that different types can implement. Traits play the
role of Python's abstract base classes or Go's interfaces. Code written
against a trait works with any type that provides the trait's methods.

## Declaring A Trait

A trait lists method signatures. A method can leave out its body:

```aura check-pass
trait Greeter:
    def greet(self) -> str
```

A method can also provide a default implementation, which may call the
trait's other methods:

```aura check-pass
trait Named:
    def name(self) -> str
    def label(self) -> str:
        return "name=" + self.name()
```

An empty marker trait uses `pass`:

```aura check-pass
trait Marker:
    pass
```

A generic trait uses the same `Name[T]` syntax as a class:

```aura check-pass
trait Mapper[T]:
    def map(self, value: own T) -> T
```

Trait methods and impl methods can use `Self` in parameter and return
positions:

```aura check-pass
trait Combine:
    def combine(self, other: Self) -> Self
```

A trait can inherit from another trait:

```aura check-pass
trait Named:
    def name(self) -> str

trait Labelled: Named:
    def label(self) -> str:
        return "name=" + self.name()
```

A type that implements `Labelled` must also implement `Named`. A bound such as
`T: Labelled` inherits the methods and obligations of the supertraits.

## Implementing A Trait

`impl Trait for Type:` provides the trait's methods for a concrete type:

```aura fragment
class User:
    name: str

impl Greeter for User:
    def greet(self) -> str:
        return "hello " + self.name
```

You can implement a trait for one specialized instance of a generic class:

```aura fragment
class Box[T]:
    value: T

impl Greeter for Box[str]:
    def greet(self) -> str:
        return self.value.clone()
```

An open generic impl header covers every instance:

```aura fragment
impl[T] Showable for Box[T]:
    def show(self) -> str:
        return "box"
```

A generic trait can be implemented for a generic class:

```aura fragment
impl Mapper[T] for Box[T]:
    def map(self, value: own T) -> T:
        return value
```

## Clone-Safety Is Part Of The Trait Contract

When a generic trait default method clones, Aura infers a clone-safety
obligation as part of that method's contract:

```aura check-pass
trait Duplicator[T]:
    def duplicate(self, values: list[T]) -> list[T]:
        return values.copy()
```

The requirement follows `T` and `Self` through every implementation, concrete
call, associated call, and bounded generic call. A safe specialization works.
One that contains `random.Rng` is rejected with `AU3007`. Operator dispatch
enforces the selected trait method's inferred contract, and so does the
`From.from` method that `try` selects.

The contract comes only from default bodies:

- A signature-only trait method has no inferred obligation.
- An explicit `impl` may satisfy the trait contract, but it may not
  strengthen it with hidden generic clone-producing behavior.
- Aura 0.3 has no written clone-safety bound. When cloning is part of the
  intended contract, put it in a default trait body.

See [examples/traits/clone_safety_contract.au](../examples/traits/clone_safety_contract.au) for a runnable default-method contract.

## Trait Bounds On Generic Functions

An inline bound requires a type parameter to implement a trait:

```aura fragment
def speak[T: Greeter](value: T):
    print(value.greet())
```

At the call site, Aura checks that the concrete type implements the trait.
This call prints `hello aura`:

```aura fragment
speak(value=User(name="aura"))   # User implements Greeter, so this works
```

Join multiple bounds with `+`:

```aura fragment
def use_both[T: A + B](value: T) -> int32:
    return value.a() + value.b()
```

## Trait Bounds On Classes And Enums

Class and enum type parameters can carry bounds too:

```aura fragment
class Wrapper[T: Greeter]:
    value: T
```

See [15-generics.md](15-generics.md) for more on generic type parameters.

## Specialized Generic Trait Bounds

A bound on a generic trait can name its type argument:

```aura fragment
def apply[T: Mapper[int32]](mapper: T, value: int32) -> int32:
    return mapper.map(value=value)
```

This bound means `T` must implement `Mapper` for `int32` specifically.

Several types can implement the same trait in one program. Each call
dispatches to the implementation for its concrete type, so `show` prints
`dog` for a `Dog` and `cat` for a `Cat`:

```aura check-pass
trait Describe:
    def describe(self) -> str

class Dog:
    name: str

class Cat:
    label: str

impl Describe for Dog:
    def describe(self) -> str:
        return "dog"

impl Describe for Cat:
    def describe(self) -> str:
        return "cat"

def show[T: Describe](animal: T) -> None:
    print(animal.describe())
```

Runnable examples:

- [examples/traits/generic_dispatch_multiple_types.au](../examples/traits/generic_dispatch_multiple_types.au)
- [examples/traits/generic_trait_bounds.au](../examples/traits/generic_trait_bounds.au)
- [examples/traits/specialized_trait_dispatch.au](../examples/traits/specialized_trait_dispatch.au)
- [examples/traits/supertraits.au](../examples/traits/supertraits.au), for supertraits
- [examples/traits/self_parameters.au](../examples/traits/self_parameters.au), for `Self` parameters

## Associated Methods

A trait method without a receiver is an associated method. Call it through
the implementing type's name. This prints `7`:

```aura check-pass
trait Factory:
    def make() -> int32

class Widget:
    value: int32

impl Factory for Widget:
    def make() -> int32:
        return 7

print(Widget.make())    # 7
```

See [examples/traits/trait_associated_factory.au](../examples/traits/trait_associated_factory.au).

## Operator Traits

Implement an operator trait and the matching operator works on your type:

| Operator | Trait | Method |
|----------|-------|--------|
| `a + b` | `Add[Rhs, Out]` | `add(self, rhs: Rhs) -> Out` |
| `a - b` | `Sub[Rhs, Out]` | `sub(self, rhs: Rhs) -> Out` |
| `a * b` | `Mul[Rhs, Out]` | `mul(self, rhs: Rhs) -> Out` |
| `a / b` | `Div[Rhs, Out]` | `div(self, rhs: Rhs) -> Out` |
| `a // b` | `FloorDiv[Rhs, Out]` | `floor_div(self, rhs: Rhs) -> Out` |
| `a % b` | `Mod[Rhs, Out]` | `mod(self, rhs: Rhs) -> Out` |
| `a < b` | `Ord[Rhs]` | `lt(self, rhs: Rhs) -> bool` |
| `a <= b` | `Ord[Rhs]` | `le(self, rhs: Rhs) -> bool` |
| `a > b` | `Ord[Rhs]` | `gt(self, rhs: Rhs) -> bool` |
| `a >= b` | `Ord[Rhs]` | `ge(self, rhs: Rhs) -> bool` |
| `-a` | `Neg[Out]` | `neg(self) -> Out` |
| `not a` | `Not[Out]` | `not(self) -> Out` |

Builtin rules win over trait dispatch:

- Builtin numeric floor division and `Duration // int64` take precedence.
  When neither applies, `//` and `//=` resolve through `FloorDiv.floor_div`.
- Equal integer operands with `/` are rejected before trait dispatch. `/` on
  an applicable non-numeric user type still resolves through `Div.div`.

These impls give `Point` a `+` and a unary `-`:

```aura fragment
class Point:
    x: int32
    y: int32

impl Add[Point, Point] for Point:
    def add(self, rhs: Point) -> Point:
        return Point(x=self.x + rhs.x, y=self.y + rhs.y)

impl Neg[Point] for Point:
    def neg(self) -> Point:
        return Point(x=0 - self.x, y=0 - self.y)
```

The operators also work through generic bounds:

```aura fragment
def add_all[T: Add[T, T]](left: T, right: T) -> T:
    return left + right
```

See [examples/traits/operator_traits.au](../examples/traits/operator_traits.au).

The ordering trait `Ord[Rhs]` backs `<`, `<=`, `>`, and `>=`:

```aura check-pass
trait Ord[Rhs]:
    def lt(self, rhs: Rhs) -> bool
    def le(self, rhs: Rhs) -> bool
    def gt(self, rhs: Rhs) -> bool
    def ge(self, rhs: Rhs) -> bool
```

A bound on `Ord` lets you write generic ordered code:

```aura fragment
def choose_smaller[T: Ord[T]](left: own T, right: own T) -> T:
    if left < right:
        return left
    return right
```

See [examples/traits/ordering_traits.au](../examples/traits/ordering_traits.au).

## Traits On Builtin Types

A trait can target a builtin type as well as your own classes and enums:

```aura check-pass
trait Describe:
    def describe(self) -> str

impl Describe for list[int32]:
    def describe(self) -> str:
        return f"list of {self.len()}"

impl Describe for str:
    def describe(self) -> str:
        return f"text of {self.len()}"
```

The method name must not already be a builtin member of the target. The
builtin member always wins at the call site, so the trait body would never
run. The checker rejects the collision with `AU2006`. For a method named
`len`:

```text
error[AU2006]: trait method `len` collides with builtin method `list.len`
  = help: rename the trait method; builtin methods cannot be shadowed by trait implementations
```

To fix it, rename the trait method.

The rule covers every builtin target:

- runtime handles such as `Queue[T]`, `Task[T]`, `TaskGroup`, `random.Rng`,
  and `fs.File`
- builtin value types such as `str`, `list[T]`, `dict[K, V]`, `set[T]`,
  `Duration`, and the scalar types

See [examples/traits/builtin_target_traits.au](../examples/traits/builtin_target_traits.au).

## Current Limits

The trait surface supports:

- trait declarations with signature-only methods, default methods, or `pass`
  for a marker trait
- `impl Trait for Type:` blocks
- specialized impls like `impl Trait for GenericType[ConcreteType]:`
- generic trait declarations and generic impl headers
- supertrait declarations such as `trait Child: Parent:`
- bounded generic functions, methods, classes, and enums
- specialized bounds like `T: Mapper[int32]`
- multiple bounds with `T: A + B`
- direct trait-method calls on concrete types
- trait implementations for builtin targets, when the method name does not
  collide with a builtin member of that target
- `Self` in trait and impl method parameter and return positions
- associated methods without `self`
- operator traits for `+`, `-`, `*`, `/`, `//`, `%`, `<`, `<=`, `>`, `>=`,
  unary `-`, and `not`
- inferred clone-safety contracts from trait defaults, which explicit impls
  may not strengthen
