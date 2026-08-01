# Traits

Traits define shared behavior that different types can implement. If you know Python's abstract base classes or Go's interfaces, traits serve a similar purpose -- they let you write code that works with any type that provides the required methods.

## Declaring A Trait

A trait lists method signatures. Methods may omit a body or provide a default implementation:

```python
trait Greeter:
    def greet(self) -> String
```

```python
trait Named:
    def name(self) -> String
    def label(self) -> String:
        return "name=" + self.name()
```

Empty marker traits use `pass`:

```python
trait Marker:
    pass
```

Generic traits use the same `Name[T]` syntax as classes:

```python
trait Mapper[T]:
    def map(self, value: own T) -> T
```

Trait methods and impl methods may also use `Self` in parameter and return positions:

```python
trait Combine:
    def combine(self, other: Self) -> Self
```

Traits may also inherit from other traits:

```python
trait Named:
    def name(self) -> String

trait Labelled: Named:
    def label(self) -> String:
        return "name=" + self.name()
```

When a type implements `Labelled`, it must also implement `Named`. Generic bounds such as `T: Labelled` inherit the methods and obligations of the supertraits.

## Implementing A Trait

Use `impl Trait for Type:` to provide the trait's methods for a concrete type:

```python
class User:
    name: String

impl Greeter for User:
    def greet(self) -> String:
        return "hello " + self.name
```

You can also implement traits for specialized generic instances:

```python
class Box[T]:
    value: T

impl Greeter for Box[String]:
    def greet(self) -> String:
        return self.value.clone()
```

Open generic impl headers work too:

```python
impl[T] Showable for Box[T]:
    def show(self) -> String:
        return "box"
```

And generic traits can be implemented for generic classes:

```python
impl Mapper[T] for Box[T]:
    def map(self, value: own T) -> T:
        return value
```

## Clone-Safety Is Part Of The Trait Contract

When a generic trait default method performs a clone-producing operation,
Aura infers a clone-safety obligation as part of that method's contract:

```python
trait Duplicator[T]:
    def duplicate(self, values: Vec[T]) -> Vec[T]:
        return values.clone()
```

The requirement follows `T` and `Self` through every implementation, concrete
call, associated call, and bounded generic call. A safe specialization works;
one containing `random.Rng` is rejected with `AU3007`.

A signature-only trait method has no inferred obligation. An explicit `impl`
may satisfy the trait contract but may not strengthen it by adding hidden
generic clone-producing behavior. Aura 0.2 has no written clone-safety bound,
so put that behavior in a default trait body when it is part of the intended
contract.

## Trait Bounds On Generic Functions

Generic functions can require that a type parameter implements a trait using inline bounds:

```python
def speak[T: Greeter](value: T):
    print(value.greet())
```

At the call site, Aura checks that the concrete type implements the required trait:

```python
speak(value=User(name="aura"))   # User implements Greeter, so this works
```

Multiple bounds use `+`:

```python
def use_both[T: A + B](value: T) -> int32:
    return value.a() + value.b()
```

## Trait Bounds On Classes And Enums

Class and enum type parameters can also carry trait bounds:

```python
class Wrapper[T: Greeter]:
    value: T
```

See [15-generics.md](15-generics.md) for more on generic type parameters.

## Specialized Generic Trait Bounds

Bounds can be specialized, which is useful when the trait itself is generic:

```python
def apply[T: Mapper[int32]](mapper: T, value: int32) -> int32:
    return mapper.map(value=value)
```

This says: `T` must implement `Mapper` specifically for `int32`.

Specialized dispatch works across multiple implementing types in the same program:

```python
trait Describe:
    def describe(self) -> String

class Dog:
    name: String

class Cat:
    label: String

impl Describe for Dog:
    def describe(self) -> String:
        return "dog"

impl Describe for Cat:
    def describe(self) -> String:
        return "cat"

def show[T: Describe](animal: T) -> None:
    print(animal.describe())
```

See [examples/traits/generic_dispatch_multiple_types.au](../examples/traits/generic_dispatch_multiple_types.au), [examples/traits/generic_trait_bounds.au](../examples/traits/generic_trait_bounds.au), and [examples/traits/specialized_trait_dispatch.au](../examples/traits/specialized_trait_dispatch.au).

See [examples/traits/supertraits.au](../examples/traits/supertraits.au) for a runnable supertrait example.
See [examples/traits/self_parameters.au](../examples/traits/self_parameters.au) for a runnable `Self`-parameter example.

## Associated Methods

Traits can declare methods without a receiver. They are called through the implementing type name:

```python
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

Aura supports operator overloading through traits. When you implement the right trait, standard operators like `+` and `-` work with your types:

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

Builtin numeric floor division and `Duration // int64` take precedence. When
neither rule applies, `//` and `//=` resolve through
`FloorDiv.floor_div`. Equal integer operands with `/` are rejected rather than
sent to `Div`, while `/` on an applicable non-numeric user type still resolves
through `Div.div`.

Example:

```python
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

With these impls, you can use `+` and `-` with `Point` values, including through generic bounds:

```python
def add_all[T: Add[T, T]](left: T, right: T) -> T:
    return left + right
```

See [examples/traits/operator_traits.au](../examples/traits/operator_traits.au).

Operator dispatch enforces the selected trait method's inferred clone-safety
contract. The `From.from` method selected by `try` does the same.

Ordering traits work the same way for `<`, `<=`, `>`, and `>=`:

```python
trait Ord[Rhs]:
    def lt(self, rhs: Rhs) -> bool
    def le(self, rhs: Rhs) -> bool
    def gt(self, rhs: Rhs) -> bool
    def ge(self, rhs: Rhs) -> bool
```

This lets you write generic ordered code such as:

```python
def choose_smaller[T: Ord[T]](left: own T, right: own T) -> T:
    if left < right:
        return left
    return right
```

See [examples/traits/ordering_traits.au](../examples/traits/ordering_traits.au).

## Traits On Builtin Types

A trait can also target a builtin type, not only your own classes and enums:

```aura
trait Describe:
    def describe(self) -> String

impl Describe for Vec[int32]:
    def describe(self) -> String:
        return f"vec of {self.len()}"

impl Describe for String:
    def describe(self) -> String:
        return f"text of {self.len()}"
```

The one restriction is that the method name must not already be a builtin
member of that target. Naming it `len` instead of `describe` would be rejected
with `AU2006`, because the builtin `len` always wins at every call site and the
trait body would silently never run:

```text
error[AU2006]: trait method `len` collides with builtin method `Vec.len`
  = help: rename the trait method; builtin methods cannot be shadowed by trait implementations
```

This holds for every builtin target: the runtime handles such as `Queue[T]`,
`Task[T]`, `TaskGroup`, `random.Rng`, and `fs.File`, and the builtin value
types such as `String`, `Vec[T]`, `Map[K, V]`, `Set[T]`, `Duration`, and the
scalar types.

See [examples/traits/builtin_target_traits.au](../examples/traits/builtin_target_traits.au).

## Current Limits

The implemented trait surface supports:

- trait declarations (signature-only methods, default methods, marker traits with `pass`)
- `impl Trait for Type:` blocks
- specialized impls like `impl Trait for GenericType[ConcreteType]:`
- generic trait declarations and generic impl headers
- supertrait declarations such as `trait Child: Parent:`
- bounded generic functions, methods, classes, and enums
- specialized bounds like `T: Mapper[int32]`
- multiple bounds with `T: A + B`
- direct trait-method calls on concrete types
- trait implementations for builtin targets, for method names that do not
  collide with a builtin member of that target
- `Self` in trait and impl method parameter and return positions
- associated methods without `self`
- operator traits for `+`, `-`, `*`, `/`, `%`, `<`, `<=`, `>`, `>=`, unary `-`, and `not`
- inferred clone-safety contracts from trait defaults, with explicit impls
  forbidden from strengthening them

See [examples/traits/clone_safety_contract.au](../examples/traits/clone_safety_contract.au) for a runnable default-method contract.
