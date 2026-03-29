# Functions

Functions are declared with `def`.

## Functions With An Explicit Return Type

```python
def distance(a: Point, b: Point) -> float64:
    dx = a.x - b.x
    dy = a.y - b.y
    return (dx * dx + dy * dy).sqrt()
```

See [examples/classes/point_distance.au](../examples/classes/point_distance.au).

## Functions That Omit The Return Type

If a function does not return a value, the implemented language allows omitting `-> None`.

```python
def main():
    print(16)
```

See [examples/basics/main_function.au](../examples/basics/main_function.au).

In the current compiler:

- omitted return type means `None`
- reaching the end of the function is allowed
- `return value` must still match the declared return type when one exists

For `None`-returning functions, bare `return` is also allowed:

```python
def log_value(value: int32):
    print(value)
    return
```

## Parameters

Parameters are written with explicit types:

```python
def distance(a: Point, b: Point) -> float64:
```

Aurora also supports ordinary borrowed parameters for helper functions that should inspect or mutate an existing caller-owned value without consuming it.

```python
def read(counter: borrow Counter) -> int32:
    return counter.value

def bump(counter: borrow mut Counter):
    counter.value += 1
```

The preferred style matches the proposal and writes the borrow in the type position:

- `counter: borrow Counter`
- `counter: borrow mut Counter`

`borrow mut` parameters must receive a mutable place at the call site:

```python
mut counter = Counter(value=41)
bump(counter)
```

When a call uses borrowed parameters, Aurora also rejects overlapping arguments whenever a
`borrow mut` parameter is involved. That rule also applies to method receivers like
`borrow mut self`: the receiver cannot alias another borrowed argument in the same call. A
mutable borrow must stay exclusive for the duration of the call.

See [examples/basics/borrow_parameters.au](../examples/basics/borrow_parameters.au).

For now, borrowed parameters are only supported on ordinary calls. `spawn` and `task_group().spawn(...)` still require by-value parameters because task capture does not yet model borrowed argument lifetimes.

## Calling Functions

Aurora now supports both positional and named arguments for ordinary function calls.

```python
def subtract(left: int32, right: int32) -> int32:
    return left - right

print(subtract(10, 3))
print(subtract(left=10, right=3))
print(subtract(10, right=3))
```

The current call rules are:

- positional arguments must come before named arguments
- named arguments must match declared parameter names exactly
- the same parameter cannot be provided more than once
- parameters with defaults may be omitted
- parameters with defaults must come after required parameters

Aurora now supports default parameter values on ordinary functions and methods:

```python
def greet(name: String = "world"):
    print("hello " + name)

greet()
greet(name="aurora")
```

Default arguments in the current compiler are:

- evaluated on each call
- evaluated in parameter order
- not allowed to reference other parameters
- not allowed in trait or trait-impl method declarations

See [examples/basics/default_arguments.au](../examples/basics/default_arguments.au).

The current bootstrap also applies named arguments to the supported builtin callables that expose real parameter names:

```python
mut total: int32 = 0

for value in range(stop=3):
    total += value

for value in range(start=3, stop=5):
    total += value

print(value=total)
```

Right now that includes:

- `print(value=...)`
- `range(stop=...)`
- `range(start=..., stop=...)`

See [examples/basics/named_builtin_arguments.au](../examples/basics/named_builtin_arguments.au).

## What Functions Can Return Today

The bootstrap compiler already supports functions returning:

- scalar types such as `int32` and `float64`
- classes
- user-defined enums
- `Result[T, E]`
- `Option[T]`
- `Task[T]`
- `None`

## Generic Functions

Aurora now supports generic function declarations:

```python
def identity[T](value: T) -> T:
    return value
```

The current compiler infers generic function type arguments from the arguments you pass and, when needed, from the expected result type.

## Current Limits

Aurora’s implemented subset does not yet cover:

- borrowed return types on ordinary functions
- explicit lifetime syntax on function signatures
