# Results And Optional Values

Aura handles recoverable errors with values, not exceptions. This chapter covers the four types involved:

- **`Result[T, E]`** is success with a `T` or failure with an `E`.
- **`T | None`** is a value that may be absent.
- **`Lookup[T]`** is the outcome of a collection lookup.
- **`SendError[T]`** is a failed queue send.

## `Result[T, E]`

Return `Result[T, E]` when an operation can succeed with a value of type `T` or fail with an error of type `E`:

```aura check-pass
def divide(a: int32, b: int32) -> Result[int32, str]:
    if b == 0:
        return Result.Err("division by zero")
    return Result.Ok(a // b)
```

Handle the result with `match`:

```aura fragment
match divide(10, 3):
    case Ok(value):
        print(f"result: {value}")
    case Err(message):
        print(f"error: {message}")
```

This is Aura's main error-handling pattern. Python uses `try/except`. Aura puts the error in the return type, so the compiler makes sure you handle it.

## Optional Values: `T | None`

Use the union `T | None` for a value that may be missing. The function returns the value when it has one and `None` when it does not:

```aura check-pass
def find_user(id: int32) -> str | None:
    if id == 1:
        return "Ada"
    return None
```

Aura has no builtin `Option` type, no `Some(...)` constructor, and no `T?` suffix. `Option[str]` is an unknown type, and `str?` is a parse error.

A plain value converts to the union at any typed destination. A binding, parameter, field, or return position that expects `str | None` accepts a plain `str`.

Test for a value with `is None` or `is not None`. When the operand is a stable place, such as a local or a parameter, the test narrows it in the selected branch. After `is not None`, `name` is a plain `str`:

```aura check-pass
def find_user(id: int32) -> str | None:
    if id == 1:
        return "Ada"
    return None

def main():
    name = find_user(1)
    if name is not None:
        print(f"found: {name.len()}")
    else:
        print("not found")
```

A call result is not a stable place, so bind it to a name first, as `name` is bound above.

A `match` selects a member with a type pattern instead:

```aura check-pass
def describe(name: str | None):
    match name:
        case str as text:
            print(f"found: {text}")
        case None:
            print("not found")
```

These library functions return optional unions:

- `str.strip_prefix()`
- `sys.env()`
- `path.extension()`
- `json.as_int()`
- `io.read_line()`

[09-enums-and-match.md](09-enums-and-match.md) covers narrowing through `not`, `and`, and `or`, and `match mut` and `match own` over unions.

## `None` vs an optional `None`

`None` has two roles, and the destination's type decides which one applies:

- **On its own**, `None` is the unit type and its only value. It means "no meaningful return value". A function with no `-> ...` returns `None`, and an unannotated `x = None` is a unit binding.
- **At a destination typed `T | None`**, `None` is the absence member of that union. It means "no value in this optional slot".

```aura check-pass
done: None = None                # the unit value
missing: int32 | None = None     # an absent optional
```

An unannotated binding never infers an optional type. Write the union explicitly to get one:

```aura check-pass
count: int32 | None = 5
```

## Lookups: `Lookup[T]`

A collection lookup must tell a missing entry apart from a stored value that is itself `None`. So `list.get()`, `dict.get()`, and `dict.remove()` return the builtin enum `Lookup[T]`. Its variants are `Lookup.Found(value)` and `Lookup.Missing`:

```aura check-pass
def main():
    names = ["Ada", "Grace"]
    match names.get(5):
        case Lookup.Found(name):
            print(name)
        case Lookup.Missing:
            print("no such position")

    mut counts = {"a": 1}
    match counts.remove("a"):
        case Lookup.Found(count):
            print(count)
        case Lookup.Missing:
            print("no such key")
```

The methods get the value in different ways:

- `dict.remove` transfers the stored value out of the collection.
- Collection `get` clones the stored value, so `T` must be clone-safe.

A stored value that contains `random.Rng` must be removed or transferred some other way.

Printing a `Lookup` shows its qualified variant, such as `Lookup.Found(7)` or `Lookup.Missing`.

`Queue.poll()` and `Task.poll()` make a similar distinction for values that are not ready yet. They return `Poll[T]`, whose variants are `Poll.Ready(value)` and `Poll.Unavailable`. See [13-concurrency.md](13-concurrency.md).

## `SendError[T]`

A failed queue send returns `SendError[T]`. It holds the value that could not be sent, so you can get it back.

| Variant | Returned by | Cause |
| --- | --- | --- |
| `Closed(value)` | `put` | the queue is closed |
| `Cancelled(value)` | `put` | the send was cancelled |
| `TimedOut(value)` | `put` | the send timed out |
| `Full(value)` | `try_put` | a bounded queue has no capacity |

```aura check-pass
ch = Queue[int32]()
ch.close()

match ch.put(4):
    case Ok(done):
        print("sent")
    case Err(SendError.Closed(value)):
        print(f"queue closed, could not send {value}")
    case Err(SendError.Cancelled(value)):
        print(f"send cancelled, could not send {value}")
    case Err(SendError.TimedOut(value)):
        print(f"send timed out, could not send {value}")
    case Err(SendError.Full(value)):
        print(f"queue full, could not send {value}")
```

See [examples/concurrency/send_result.au](../examples/concurrency/send_result.au) for a full example.

## Composing Results

To chain operations that each return `Result`, unwrap each step with `match`:

```aura check-pass
def process(input: str) -> Result[int32, str]:
    match own parse_int32(input):
        case Ok(value):
            if value < 0:
                return Result.Err("negative value")
            return Result.Ok(value * 2)
        case Err(message):
            return Result.Err(message)
```

`try expr` removes most of this nesting. See [12-error-propagation.md](12-error-propagation.md).

## Retrying A Result Worker

`control.retry` calls a worker again after each `Err`, treating every error as retryable:

```aura check-pass
import control

def attempt() -> Result[int32, str]:
    return Result.Err("not ready")

result = control.retry(
    attempt,
    max_attempts=3,
    initial_backoff=10ms
)
```

How it runs:

- The first attempt runs immediately.
- The second attempt waits `initial_backoff`. Each later attempt waits twice the previous delay.
- A zero delay skips sleeping.
- After the last permitted attempt, it returns that `Err` exactly. It does not sleep or compute another delay first.
- Worker traps, backoff overflow, and task cancellation are not turned into an `Err`. They propagate.

The helper does not classify errors or add jitter. When only some failures should retry, write the policy as a loop. See [examples/agents/retry_with_backoff.au](../examples/agents/retry_with_backoff.au) for the helper and [examples/agents/retrying_network_worker.au](../examples/agents/retrying_network_worker.au) for a network policy that checks the status code.

## Current Limits

The compiler supports:

- `Result[T, E]`, `T | None`, `Lookup[T]`, `Poll[T]`, and `SendError[T]` in type positions
- constructing values with `Result.Ok(...)`, `Result.Err(...)`, `Lookup.Found(...)`, and `Lookup.Missing`
- constructing a `T | None` from a plain value or `None` at a typed destination
- every `SendError` variant: `Closed(...)`, `Cancelled(...)`, `TimedOut(...)`, and `Full(...)`
- exhaustive `match` over all of these, including `case T as name:` and `case None:` type patterns for unions
- unqualified variants such as `Ok`, `Err`, `Found`, and `Missing` when the scrutinee type is known
- `is None` and `is not None` narrowing of stable places
- `try` propagation when the error types match exactly, or when a visible, applicable `impl From[SourceError] for TargetError` exists

See [examples/enums/result_option.au](../examples/enums/result_option.au).
