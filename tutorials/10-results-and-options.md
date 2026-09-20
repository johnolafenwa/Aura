# Results And Optional Values

Aura represents typed success and failure with `Result[T, E]`, presence and
absence with the optional union `T | None`, collection lookups with
`Lookup[T]`, and failed queue sends with the queue-specific `SendError[T]`.
These types form the foundation of recoverable error handling in Aura.

## `Result[T, E]`

Use `Result[T, E]` when an operation can succeed with a value of type `T` or fail with an error of type `E`:

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

This is Aura's primary error-handling pattern. Instead of exceptions (like Python's `try/except`), Aura makes errors part of the return type so the compiler ensures you handle them.

## Optional Values: `T | None`

Use the union `T | None` when a value may or may not be present. A function
returns the value itself when it is present and an explicit `None` when it is
absent:

```aura check-pass
def find_user(id: int32) -> str | None:
    if id == 1:
        return "Ada"
    return None
```

There is no builtin `Option` type, no `Some(...)` constructor, and no `T?`
suffix: `Option[str]` is an unknown type and `str?` is a parse error. A bare
value is injected into the union at any typed destination, so a binding,
parameter, field, or return position that expects `str | None` accepts a
plain `str`.

Test presence with `is None` and `is not None`. When the tested operand is a
stable place such as a local or a parameter, the test narrows that place in
the branch it selects, so `name` is a plain `str` after `is not None`:

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

A call result is not a stable place, so bind it first, as `name` is bound
above. A `match` selects a member with a type pattern instead:

```aura check-pass
def describe(name: str | None):
    match name:
        case str as text:
            print(f"found: {text}")
        case None:
            print("not found")
```

`str.strip_prefix()`, `sys.env()`, `path.extension()`, `json.as_int()`, and
`io.read_line()` all return optional unions. See
[09-enums-and-match.md](09-enums-and-match.md) for narrowing through `not`,
`and`, and `or`, and for `match mut` and `match own` over unions.

## `None` vs an optional `None`

`None` is one identifier with two roles:

- On its own, **`None`** is the unit type and value. It means "no meaningful return value." A function with no `-> ...` returns `None`, and an unannotated `x = None` is a unit binding.
- At a destination typed `T | None`, **`None`** selects the absence member of that union. It means "no value present in this optional slot."

```aura check-pass
done: None = None                # the unit value
missing: int32 | None = None     # an absent optional
```

The annotation decides which role applies. An unannotated binding never
infers an optional type, so write the union explicitly to keep it:

```aura check-pass
count: int32 | None = 5
```

## Lookups: `Lookup[T]`

A collection lookup must tell a missing entry apart from a present value
that is itself `None`. `list.get()`, `dict.get()`, and `dict.remove()`
therefore return the builtin enum `Lookup[T]`, whose variants are
`Lookup.Found(value)` and `Lookup.Missing`:

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

Those APIs do not all obtain `T` the same way. `dict.remove` transfers a stored
value, while collection `get` clones one and therefore requires clone-safe `T`; a
stored value containing `random.Rng` must be removed or otherwise transferred.
Printing a `Lookup` shows its qualified variant, such as `Lookup.Found(7)` or
`Lookup.Missing`.

`Queue.poll()` and `Task.poll()` make the same distinction for values that
are not ready yet with `Poll[T]`, whose variants are `Poll.Ready(value)` and
`Poll.Unavailable`; see [13-concurrency.md](13-concurrency.md).

## `SendError[T]`

`SendError[T]` is the error type returned when a queue send fails. It wraps the
value that could not be sent, so you can recover it. `put` can report a closed
queue, cancellation, or timeout; `try_put` reports `Full` when a bounded queue
has no capacity:

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

A common pattern is chaining operations that each return `Result`. Use `match` to unwrap each step:

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

For simpler cases, Aura provides `try expr` to reduce the nesting. See [12-error-propagation.md](12-error-propagation.md).

## Retrying A Result Worker

`control.retry` handles the common policy where every `Err` is retryable:

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

The first attempt runs immediately. Later attempts wait for the initial
backoff and then twice the previous delay. A zero delay skips sleeping. The
last permitted `Err` is returned exactly, with no final sleep or unused
multiply. Worker traps, backoff overflow, and task cancellation propagate
outside the returned `Err` value.

This helper does not classify errors or add jitter. Write an explicit policy
loop when only selected failures should retry. See
[examples/agents/retry_with_backoff.au](../examples/agents/retry_with_backoff.au)
for the helper and
[examples/agents/retrying_network_worker.au](../examples/agents/retrying_network_worker.au)
for a status-aware network policy.

## Current Limits

The bootstrap compiler supports:

- `Result[T, E]`, `T | None`, `Lookup[T]`, `Poll[T]`, and `SendError[T]` in type positions
- constructing values with `Result.Ok(...)`, `Result.Err(...)`, a bare value
  or `None` at a `T | None` destination, `Lookup.Found(...)`, `Lookup.Missing`,
  and every `SendError` variant: `Closed(...)`, `Cancelled(...)`,
  `TimedOut(...)`, and `Full(...)`
- exhaustive `match` over all of these, including `case T as name:` and
  `case None:` type patterns for unions
- unqualified variants (`Ok`, `Err`, `Found`, `Missing`) when the scrutinee type is known
- `is None` and `is not None` narrowing of stable places
- `try` propagation with either the exact error type or a visible applicable
  `impl From[SourceError] for TargetError`

See [examples/enums/result_option.au](../examples/enums/result_option.au).
