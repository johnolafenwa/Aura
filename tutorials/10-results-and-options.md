# Results And Options

Aurora provides three built-in generic enums for representing success/failure and presence/absence. These are the foundation of error handling in Aurora.

## `Result[T, E]`

Use `Result[T, E]` when an operation can succeed with a value of type `T` or fail with an error of type `E`:

```python
def divide(a: int32, b: int32) -> Result[int32, String]:
    if b == 0:
        return Result.Err("division by zero")
    return Result.Ok(a / b)
```

Handle the result with `match`:

```python
match divide(10, 3):
    case Ok(value):
        print(f"result: {value}")
    case Err(message):
        print(f"error: {message}")
```

This is Aurora's primary error-handling pattern. Instead of exceptions (like Python's `try/except`), Aurora makes errors part of the return type so the compiler ensures you handle them.

## `Option[T]`

Use `Option[T]` when a value may or may not be present:

```python
def find_user(id: int32) -> Option[String]:
    if id == 1:
        return Option.Some("Ada")
    return Option.None
```

Handle it with `match`:

```python
match find_user(1):
    case Some(name):
        print(f"found: {name}")
    case None:
        print("not found")
```

You will see `Option[T]` throughout Aurora's standard library -- `Vec.pop()`, `Vec.get()`, `Map.get()`, and `String.strip_prefix()` all return `Option` values.

## `None` vs `Option.None`

These look similar but are different:

- **`None`** is the unit type and value. It means "no meaningful return value." A function with no `-> ...` returns `None`.
- **`Option.None`** is the empty variant of `Option[T]`. It means "no value present in this optional slot."

```python
done: None = None              # the unit value
missing: Option[int32] = Option.None   # an empty optional
```

In practice, the distinction is clear from context. When you see `Option.None` in a `match` arm, it always refers to the enum variant.

`Option.Some(...)` can infer `T` from its payload even without an annotation:

```python
count = Option.Some(5)
```

`Option.None` still needs an expected `Option[T]` type because there is no payload to infer from:

```python
missing: Option[int32] = Option.None
```

## `SendError[T]`

`SendError[T]` is the error type returned when a queue send fails because the queue is closed or a waiting send is cancelled. It wraps the value that could not be sent, so you can recover it:

```python
ch = Queue[int32]()
ch.close()

match ch.put(4):
    case Ok(done):
        print("sent")
    case Err(SendError.Closed(value)):
        print(f"queue closed, could not send {value}")
    case Err(SendError.Cancelled(value)):
        print(f"send cancelled, could not send {value}")
```

See [examples/concurrency/send_result.au](../examples/concurrency/send_result.au) for a full example.

## Composing Results

A common pattern is chaining operations that each return `Result`. Use `match` to unwrap each step:

```python
def process(input: String) -> Result[int32, String]:
    match parse_int32(input):
        case Ok(value):
            if value < 0:
                return Result.Err("negative value")
            return Result.Ok(value * 2)
        case Err(message):
            return Result.Err(message)
```

For simpler cases, Aurora provides `try expr` to reduce the nesting. See [12-error-propagation.md](12-error-propagation.md).

## Current Limits

The bootstrap compiler supports:

- `Result[T, E]`, `Option[T]`, and `SendError[T]` in type positions
- constructing values with `Result.Ok(...)`, `Result.Err(...)`, `Option.Some(...)`, `Option.None`, `SendError.Closed(...)`, and `SendError.Cancelled(...)`
- exhaustive `match` over all of these
- unqualified variants (`Ok`, `Err`, `Some`, `None`) when the scrutinee type is known

Not yet supported:

- implicit error conversion for `try` (error types must match exactly)

See [examples/enums/result_option.au](../examples/enums/result_option.au).
