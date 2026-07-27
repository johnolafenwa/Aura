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

An unmodified parameter grants shared access for every type. An implementation
may pass copy bits directly, but that does not change the source-level
contract. Write `own` when the function takes ownership:

```python
def archive(doc: own Document):
    print(doc.title)
```

The choice is fixed at the declaration. For an unresolved generic `T`, the
bare form is a declaration-stable shared borrow even if a later call uses a
copy type; use `value: own T` for an identity, storing, or consuming helper.

## Borrowed Parameters

When a function only needs to read a value, it should borrow rather than take ownership. This lets the caller keep using the value after the call. If you are new to borrowing, see [06-ownership-and-borrowing.md](06-ownership-and-borrowing.md) for the full explanation.

Use `T` for read-only access:

```python
def read(counter: Counter) -> int32:
    return counter.value
```

Use `mut T` for mutable access -- the function can modify the value and changes persist back to the caller:

```python
def bump(counter: mut Counter):
    counter.value += 1
```

A `mut` parameter requires a mutable binding at the call site:

```python
mut counter = Counter(value=41)
bump(counter)
print(counter.value)    # 42
```

Aurora rejects overlapping arguments when `mut` is involved. Mutable access
must be exclusive -- no other overlapping access can exist in the same call:

```python
# This would be rejected:
# bad(a: mut Counter, b: Counter) called with bad(c, c)
```

This rule prevents subtle bugs where a function reads from and writes to the same value through different parameters.

See [examples/basics/borrow_parameters.au](../examples/basics/borrow_parameters.au).

Task targets may use bare shared or `own` parameters.
Arguments are moved or copied into task-owned capture storage before the child
runs, and a shared target borrows that capture. `mut` targets are
rejected.

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

Default values are evaluated on each call, in parameter order. They cannot
reference other parameters, and are not allowed in trait or trait-impl method
declarations. Bare shared defaults are valid and the temporary lives through
the call; `own` defaults are consumed. `mut` defaults are
rejected because mutations to a caller-invisible temporary would be lost.

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

Every function return is an owned value. Returning a copy type produces an
ordinary independent copy:

```python
class User:
    score: int32

def score(user: User) -> int32:
    return user.score
```

The call produces an ordinary `int32` copy. Methods use the same `-> T`
return annotation.

When several shared parameters have copy types, the function can select and
return any one of their values without a source label:

```python
def choose_positive(left: int32, right: int32) -> int32:
    if left > 0:
        return left
    return right
```

Returning a non-copy value requires ownership. Clone from shared input when the
type is clone-safe, accept an `own` parameter and move from it, or provide an
owner operation such as an `own self` method. A shared parameter cannot expose
one of its non-copy fields as a return value.

The legacy-named
[borrowed_returns.au](../examples/basics/borrowed_returns.au) and
[borrowed_lifetime_labels.au](../examples/basics/borrowed_lifetime_labels.au)
examples now demonstrate ordinary owned copy results. Aurora has no
borrow-source or return-label syntax. Any future loan or view design will be
specified from scratch rather than inferred from today's `-> T`.

## Generic Functions

Functions can be generic over type parameters:

```python
def identity[T](value: own T) -> T:
    return value
```

The compiler infers type arguments from the arguments you pass and, when needed, from the expected return type. See [15-generics.md](15-generics.md) for the full story.

## Current Limits

- return values are always owned; first-class loan or view return values are
  not part of Aurora 0.1
- clone-based non-copy returns require the returned type to be clone-safe
