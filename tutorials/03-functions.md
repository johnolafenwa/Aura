# Functions

You declare a function with `def`. Every parameter needs an explicit type.

## Basic Functions

```aura check-pass
def add(a: int32, b: int32) -> int32:
    return a + b
```

The return type follows `->`. A function that returns no value can omit the
return type. It then returns `None`:

```aura check-pass
def greet():
    print("hello")
```

A `None`-returning function may run off its end. It may also use a bare
`return`:

```aura check-pass
def log_value(value: int32):
    print(value)
    return
```

See [examples/basics/main_function.au](../examples/basics/main_function.au).

## Parameters

Each parameter has an explicit type:

```aura fragment
def distance(a: Point, b: Point) -> float64:
    dx = a.x - b.x
    dy = a.y - b.y
    return (dx * dx + dy * dy).sqrt()
```

See [examples/classes/point_distance.au](../examples/classes/point_distance.au).

A bare parameter, one with no `own` or `mut`, gives the function shared access
for every type. The compiler may pass a copy type's bits directly, but the
function still has shared access in the source. Write `own` when the function
takes ownership:

```aura fragment
def archive(doc: own Document):
    print(doc.title)
```

The declaration fixes the choice. For an unresolved generic `T`, a bare
parameter is always a shared borrow, even if a later call uses a copy type.
Write `value: own T` for a helper that returns, stores, or consumes its
argument.

## Borrowed Parameters

When a function only reads a value, give it shared access. The caller keeps
ownership and can use the value after the call.
[06-ownership-and-borrowing.md](06-ownership-and-borrowing.md) explains
borrowing in full.

Use `T` for read-only access:

```aura fragment
def read(counter: Counter) -> int32:
    return counter.value
```

Use `mut T` for mutable access. The function can change the value, and the
caller sees the changes:

```aura fragment
def bump(counter: mut Counter):
    counter.value += 1
```

The caller must pass a mutable binding to a `mut` parameter:

```aura fragment
mut counter = Counter(value=41)
bump(counter)
print(counter.value)    # 42
```

Mutable access must be exclusive. Aura rejects a call where a `mut` argument
overlaps any other argument:

```aura check-pass
# This would be rejected:
# bad(a: mut Counter, b: Counter) called with bad(c, c)
```

This rule stops a function from reading and writing one value through two
different parameters.

See [examples/basics/borrow_parameters.au](../examples/basics/borrow_parameters.au).

A task target may take bare shared or `own` parameters. The arguments move or
copy into task-owned capture storage before the child task runs. A shared
parameter borrows that capture. The compiler rejects `mut` parameters on task
targets.

## Calling Functions

Pass arguments by position, by name, or both:

```aura check-pass
def subtract(left: int32, right: int32) -> int32:
    return left - right

print(subtract(10, 3))
print(subtract(left=10, right=3))
print(subtract(10, right=3))
```

A `*` in the parameter list makes the parameters after it keyword-only. They
bind by name only. A positional argument that would reach one is an `AU2004`
error.

```aura check-pass
def scale(value: int32, *, factor: int32 = 2) -> int32:
    return value * factor

print(scale(4))
print(scale(4, factor=3))
```

Rules:

- positional arguments come before named arguments
- named arguments match declared parameter names exactly
- a parameter cannot be provided more than once

## Default Parameter Values

A parameter can have a default. Defaults come after required parameters:

```aura check-pass
def greet(name: str = "world"):
    print("hello " + name)

greet()               # "hello world"
greet(name="aura")  # "hello aura"
```

- Defaults are evaluated on each call, in parameter order.
- A default cannot refer to another parameter.
- Trait and trait-impl method declarations cannot have defaults.
- A bare shared default is valid. Its temporary lives through the call.
- An `own` default is consumed.
- A `mut` default is rejected, because changes to a temporary the caller
  cannot see would be lost.

See [examples/basics/default_arguments.au](../examples/basics/default_arguments.au).

## Builtin Named Arguments

Some builtins also take named arguments:

```aura check-pass
for value in range(stop=3):
    print(value)

for value in range(start=3, stop=5):
    print(value)

print(value=42)
```

See [examples/basics/named_builtin_arguments.au](../examples/basics/named_builtin_arguments.au).

## What Functions Can Return

A function can return any concrete type that a return annotation accepts.
That includes scalars, tuples, strings, collections, numeric arrays, classes,
enums, generic specializations, function values, `Result[T, E]`, optional
`T | None` unions, `Task[T]`, and `None`.

An ordinary `-> T` result is always an owned value. Returning a copy type
gives the caller an independent copy:

```aura check-pass
class User:
    score: int32

def score(user: User) -> int32:
    return user.score
```

The call produces an ordinary `int32` copy. Methods use the same `-> T`
annotation.

When several shared parameters have copy types, the function can return any
one of their values. It does not need to name a source:

```aura check-pass
def choose_positive(left: int32, right: int32) -> int32:
    if left > 0:
        return left
    return right
```

An ordinary result never names an argument, field, or lifetime source.
Returning a non-copy value requires a value the function owns. You can get one
in three ways:

- clone from shared input, when the type is clone-safe
- take an `own` parameter and move from it
- call an owner operation, such as an `own self` method

A shared parameter cannot hand out one of its non-copy fields as a return
value.

### Returned Views

A returned view keeps borrowing one place the caller owns. Declare it with
`-> view T from source` or `-> view mut T from source`, where `source` is one
receiver or parameter:

```aura check-pass
class User:
    name: str

class Counter:
    value: int64

def name(user: User) -> view str from user:
    return view user.name

def value_mut(counter: mut Counter) -> view mut int64 from counter:
    return view mut counter.value

def bump(value: mut int64):
    value += 1

def main():
    user = User(name="Ada")
    view display = name(user)
    print(display)
    print(name(user))

    mut counter = Counter(value=0)
    bump(value_mut(counter))
    print(counter.value)
```

The origin after `from` is part of the function type. The caller must pass an
addressable place. The caller can then use the result in these ways:

- bind it with matching `view` or `view mut` syntax
- read a shared result directly within one containing expression
- reborrow a mutable result straight into a `mut` call

A returned view cannot be stored as an ordinary owned value or inside an
aggregate.

## Generic Functions

A function can be generic over type parameters:

```aura check-pass
def identity[T](value: own T) -> T:
    return value
```

The compiler infers type arguments from the arguments you pass. When needed,
it also uses the expected return type. See [15-generics.md](15-generics.md)
for the full story.

## Function Values

You can store and pass a module-level named function like any other copy
value. Write its type in the same shape as a declaration:

```aura check-pass
type Transform = def(int32) -> int32

class Pipeline:
    transform: def(int32) -> int32

def double(value: int32) -> int32:
    return value * 2

def apply(transform: def(int32) -> int32, value: int32) -> int32:
    return transform(value)

selected = double
pipeline = Pipeline(transform=Transform(selected))
transforms: list[def(int32) -> int32] = [Transform(selected)]

print(apply(pipeline.transform, 3))
print(transforms[0](4))
```

A type like `def(T1, mut T2, own T3) -> R` has unnamed slots with no default
values. Bare parameters are shared. The `Transform(selected)` adapter hides
the source parameter name before the value is stored.

An inferred binding such as `selected = consume` keeps the exact contract. You
can also write the contract explicitly:

```aura fragment
type Update = def(mut Counter) -> None
type Consume = def(own str) -> str
mutate: Update = Update(increment)
consume: Consume = Consume(take)
callbacks: list[def(mut Counter) -> None] = [mutate]
```

Calling `mutate` requires a mutable place. Calling `consume` moves a non-copy
argument. A function with either contract does not fit a bare shared
`def(T) -> R` annotation.

### Names And Defaults

Whether a call can use parameter names and defaults depends on where the
function value came from:

- A binding whose target declaration is known statically keeps that
  declaration's names and defaults. `selected(name="Aura")` and `selected()`
  work when the original parameter is named `name` and has a default.
- The structural function type keeps neither. A value returned through a
  structural annotation needs the complete positional argument list.
- A direct conditional selection keeps names and defaults when all candidates
  agree. An omitted argument runs the selected function's own default.
- Class fields and mutable collections keep the full parameter types and the
  `mut` and `own` capabilities. They erase names and defaults on purpose. Call
  a value loaded from either with the complete positional list.

### Copying, Tasks, And Generics

Function values are code pointers. They are copy values and satisfy
`Transfer`. You can use one as the target of `TaskGroup.start(...)` or
`start_soon(...)`.

To use a generic function as a value, specialize it explicitly, as in
`show_int = show[int32]`. You can also give it a concrete expected function
type. The expected type may come from an annotation, argument, field,
collection element, or function-typed parameter default.

### Methods As Values

`Class.method` names an associated method, one without `self`, as a function
value with the method's contract. `receiver.method` outside a call binds a
closure over the receiver. See [Expression Closures](#expression-closures)
below and the [Closures](../docs/manual/closures.md) manual page. A generic
method's type arguments are written as `method[T]` or come from an expected
contract.

See [examples/basics/function_values.au](../examples/basics/function_values.au).

## Expression Closures

A lambda is an unnamed function written as one expression. Use one when its
parameter types are already clear from context:

```aura check-pass
def main():
    offset: int32 = 40
    add: def(int32) -> int32 = lambda value: value + offset
    print(add(2))
```

The annotation supplies `value: int32` and the `int32` result. A lambda's
parameter list has no types or defaults. Write `own value` or `mut value` only
when the expected function type has that same capability. Logic that needs
several statements belongs in a named `def`.

A lambda with no parameters does not need context. `lambda: 42` infers
`def() -> int64` from its body. A lambda with parameters still needs all of
their types from context.

### Captures

A lambda captures values when it is created. It snapshots copy values such as
`offset`. A non-copy owned value moves into the closure:

```aura check-pass
def main():
    name = "Aura"
    length: def() -> int64 = lambda: name.len()
    print(length())
    print(length())
```

You can call `length` twice because the body only reads `name`. A body that
returns or otherwise consumes a non-copy capture makes the closure single-use.
Clone before you create the closure if the outer code must keep its own owner.

Without a capture list, a lambda does not capture shared or mutable enclosing
parameters. An explicit, exhaustive capture list may request `[value]`,
`[mut value]`, or `[own value]`. You call a mutable-loan closure through a
`mut` local, and it writes through to its source.

A closure can cross a task boundary only when every capture is owned and
`Transfer`. Any loan capture makes the closure non-`Transfer`.

### Where Closures Can Go

A capture-free lambda works anywhere a function value works. A capturing
closure can:

- stay in an immutable local
- be called directly
- enter a compiler-known repeatable callback
- move into a qualifying task start

A capturing closure cannot be stored in a `def` field or a collection. It
cannot be returned through an annotated `def` result.

See [examples/basics/closures.au](../examples/basics/closures.au) and the
normative [Closures](../docs/manual/closures.md) page.

## Current Limits

- An ordinary `-> T` return value is owned. `-> view [mut] T from origin` is
  the explicit non-owning exception.
- A clone-based non-copy return requires a clone-safe return type.
- A closure body is one expression. Multi-statement closure bodies are not
  implemented.
