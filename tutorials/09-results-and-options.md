# Results And Options

Aurora now supports the built-in generic enums `Result[T, E]`, `Option[T]`, and `SendError[T]`.

## `Result[T, E]`

Use `Result[T, E]` for operations that either succeed with a value of type `T` or fail with an error of type `E`.

```python
def divide(a: int32, b: int32) -> Result[int32, String]:
    if b == 0:
        return Result.Err("division by zero")
    return Result.Ok(a / b)
```

## `Option[T]`

Use `Option[T]` when a value may or may not be present.

```python
def first_value(flag: bool) -> Option[int32]:
    if flag:
        return Option.Some(7)
    return Option.None
```

## `None`

Aurora also supports bare `None` as the unit value and unit type:

```python
done: None = None
```

This is different from `Option.None`, which is the empty variant of `Option[T]`.

## `SendError[T]`

`SendError[T]` is the built-in error type returned by channel sends:

```python
match ch.send(4):
    case Result.Ok(done):
        print("sent")
    case Result.Err(SendError.Closed(value)):
        print(value)
```

## Matching Exhaustively

Both built-in enums use the same `match` syntax as user-defined enums:

```python
match result:
    case Result.Ok(value):
        print(value)
    case Result.Err(message):
        print(message)
```

```python
match maybe:
    case Option.Some(value):
        print(value)
    case Option.None:
        print(0)
```

## Current Limits

The bootstrap compiler currently supports:

- `Result[T, E]` and `Option[T]` in type positions
- `SendError[T]` in type positions
- constructing values with `Result.Ok(...)`, `Result.Err(...)`, `Option.Some(...)`, and `Option.None`
- constructing values with `SendError.Closed(...)`
- exhaustive `match` over those values

It does not yet support:

- user-defined generic enums
- implicit error conversion for `try`

See [examples/enums/result_option.au](../examples/enums/result_option.au).
