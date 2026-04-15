# Functions

Functions are declared with `def` and require explicit parameter types.

## Basic Functions

```python
def add(a: int32, b: int32) -> int32:
    return a + b
```

The return type follows `->`. If a function does not return a value, you can omit the return type and it defaults to `None`:

```python
def greet():
    print("hello")
```

Reaching the end of a `None`-returning function is allowed. You can also use a bare `return`:

```python
def log_value(value: int32):
    print(value)
    return
```

See [examples/basics/main_function.au](../examples/basics/main_function.au).

## Parameters

Parameters are written with explicit types:

```python
def distance(a: Point, b: Point) -> float64:
    dx = a.x - b.x
    dy = a.y - b.y
    return (dx * dx + dy * dy).sqrt()
```

See [examples/classes/point_distance.au](../examples/classes/point_distance.au).

## Borrowed Parameters

When a function only needs to read a value, it should borrow rather than take ownership. This lets the caller keep using the value after the call. If you are new to borrowing, see [06-ownership-and-borrowing.md](06-ownership-and-borrowing.md) for the full explanation.

Use `borrow T` for read-only access:

```python
def read(counter: borrow Counter) -> int32:
    return counter.value
```

Use `borrow mut T` for mutable access -- the function can modify the value and changes persist back to the caller:

```python
def bump(counter: borrow mut Counter):
    counter.value += 1
```

A `borrow mut` parameter requires a mutable binding at the call site:

```python
mut counter = Counter(value=41)
bump(counter)
print(counter.value)    # 42
```

Aurora rejects overlapping arguments when `borrow mut` is involved. A mutable borrow must be exclusive -- no other borrow of the same value can exist in the same call:

```python
# This would be rejected:
# bad(a: borrow mut Counter, b: borrow Counter) called with bad(c, c)
```

This rule prevents subtle bugs where a function reads from and writes to the same value through different parameters.

See [examples/basics/borrow_parameters.au](../examples/basics/borrow_parameters.au).

Borrowed parameters are supported on ordinary calls. `spawn` and `task_group().spawn(...)` still require by-value parameters because task capture does not yet model borrowed argument lifetimes.

## Calling Functions

Aurora supports positional and named arguments:

```python
def subtract(left: int32, right: int32) -> int32:
    return left - right

print(subtract(10, 3))
print(subtract(left=10, right=3))
print(subtract(10, right=3))
```

Rules:

- positional arguments come before named arguments
- named arguments match declared parameter names exactly
- a parameter cannot be provided more than once

## Default Parameter Values

Parameters can have defaults, which must come after required parameters:

```python
def greet(name: String = "world"):
    print("hello " + name)

greet()               # "hello world"
greet(name="aurora")  # "hello aurora"
```

Default values are evaluated on each call, in parameter order. They cannot reference other parameters, and are not allowed in trait or trait-impl method declarations.

See [examples/basics/default_arguments.au](../examples/basics/default_arguments.au).

## Builtin Named Arguments

Some builtins also support named arguments:

```python
for value in range(stop=3):
    print(value)

for value in range(start=3, stop=5):
    print(value)

print(value=42)
```

See [examples/basics/named_builtin_arguments.au](../examples/basics/named_builtin_arguments.au).

## What Functions Can Return

The bootstrap compiler supports functions returning:

- scalar types (`int32`, `float64`, `bool`, etc.)
- classes
- user-defined enums
- `Result[T, E]` and `Option[T]`
- `Task[T]`
- `None`

## Generic Functions

Functions can be generic over type parameters:

```python
def identity[T](value: T) -> T:
    return value
```

The compiler infers type arguments from the arguments you pass and, when needed, from the expected return type. See [15-generics.md](15-generics.md) for the full story.

## Current Limits

- borrowed return types on ordinary functions are not yet supported
- explicit lifetime syntax on function signatures is not yet supported
