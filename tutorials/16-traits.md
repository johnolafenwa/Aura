# Traits

Aurora now supports trait declarations, generic trait declarations, explicit `impl Trait for Type` conformance blocks, generic impl headers, bounded generic calls, specialized generic trait bounds, and the current operator-trait surface.

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

Generic traits use the same `Name[T]` header syntax as classes and enums:

```python
trait Mapper[T]:
    def map(borrow self, value: T) -> T
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

Those specialized impls also dispatch correctly through bounded generic calls:

```python
trait Show:
    def show(borrow self) -> String

impl Show for Box[int32]:
    def show(borrow self) -> String:
        return f"{self.value}"

def render[T: Show](value: T) -> None:
    print(value.show())
```

Open generic impl headers now work too:

```python
impl[T] Showable for Box[T]:
    def show(borrow self) -> String:
        return "box"
```

Generic traits can also be implemented for generic classes:

```python
trait Mapper[T]:
    def map(borrow self, value: T) -> T

impl Mapper[T] for Box[T]:
    def map(borrow self, value: T) -> T:
        return value
```

Traits may also declare associated methods with no receiver, and those methods are callable through the implementing type name:

```python
trait Factory:
    def make() -> int32

class Widget:
    value: int32

impl Factory for Widget:
    def make() -> int32:
        return 7

print(Widget.make())
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

Generic trait bounds also work when the bound itself is specialized:

```python
trait Mapper[T]:
    def map(borrow self, value: T) -> T

class Doubler:
    factor: int32

impl Mapper[int32] for Doubler:
    def map(borrow self, value: int32) -> int32:
        return value * self.factor

def apply[T: Mapper[int32]](mapper: T, value: int32) -> int32:
    return mapper.map(value=value)
```

That bounded dispatch also works across multiple different implementing types in the same program:

```python
trait Describe:
    def describe(borrow self) -> String

class Dog:
    name: String

class Cat:
    label: String

impl Describe for Dog:
    def describe(borrow self) -> String:
        return "dog"

impl Describe for Cat:
    def describe(borrow self) -> String:
        return "cat"

def show[T: Describe](animal: T) -> None:
    print(animal.describe())
```

See [examples/traits/generic_dispatch_multiple_types.au](../examples/traits/generic_dispatch_multiple_types.au) for a runnable maintained example.

See [examples/traits/generic_trait_bounds.au](../examples/traits/generic_trait_bounds.au) for maintained specialized generic trait bounds, [examples/traits/specialized_trait_dispatch.au](../examples/traits/specialized_trait_dispatch.au) for bounded dispatch across specialized generic impls, and [examples/traits/trait_associated_factory.au](../examples/traits/trait_associated_factory.au) for trait-associated methods through the type name.

## Operator Traits

The current compiler supports operator traits for this subset:

- `Add[Rhs, Out]` via `+`
- `Sub[Rhs, Out]` via binary `-`
- `Mul[Rhs, Out]` via `*`
- `Div[Rhs, Out]` via `/`
- `Mod[Rhs, Out]` via `%`
- `Neg[Out]` via unary `-`
- `Not[Out]` via unary `not`

The operator method must use the conventional method shape for that operator:

```python
trait Add[Rhs, Out]:
    def add(borrow self, rhs: Rhs) -> Out

trait Neg[Out]:
    def neg(borrow self) -> Out
```

With matching impls in place, ordinary operator syntax works through trait bounds too:

```python
class Point:
    x: int32
    y: int32

impl Add[Point, Point] for Point:
    def add(borrow self, rhs: Point) -> Point:
        return Point(x=self.x + rhs.x, y=self.y + rhs.y)

impl Neg[Point] for Point:
    def neg(borrow self) -> Point:
        return Point(x=0 - self.x, y=0 - self.y)

def add_all[T: Add[T, T]](left: T, right: T) -> T:
    return left + right
```

See [examples/traits/operator_traits.au](../examples/traits/operator_traits.au) for the maintained runnable example.

## Current Limits

The implemented trait surface currently supports:

- `trait Name:` declarations
- empty marker traits with `pass`
- method signatures inside trait bodies
- `impl Trait for Type:` blocks
- `impl Trait for GenericType[ConcreteType]:` specialized impls
- generic trait declarations like `trait Mapper[T]:`
- generic impl headers such as `impl[T] Trait for Box[T]:`
- generic trait impl headers such as `impl Mapper[T] for Box[T]:`
- bounded generic functions and methods with `T: Trait`
- bounded generic functions and methods with specialized bounds like `T: Mapper[int32]`
- bounded generic classes and enums with `T: Trait`
- multiple bounds with `T: A + B`
- direct trait-method calls on concrete types that implement the trait
- associated trait methods declared without `self`
- operator traits for `+`, binary `-`, `*`, `/`, `%`, unary `-`, and `not`
