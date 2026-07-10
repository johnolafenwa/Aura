# Enums And Pattern Matching

Enums model alternatives. Pattern matching selects an alternative and binds any payload values.

Aurora uses enums for user data and for much of the runtime API surface: `Option`, `Result`, queue receives, task results, process status, supervisor events, and I/O errors.

## Enum Declaration

```python
enum Status:
    Ready(count: int32)
    Failed(message: String)
    Empty
```

Variants may be:

- payload-free, such as `Empty`
- positional, such as `Some(value)`
- named in constructor calls, such as `Ready(count=3)`

Payload fields are part of the variant shape.

## Construction

Use the enum name and variant name:

```python
ready = Status.Ready(count=3)
failed = Status.Failed(message="disk full")
empty = Status.Empty
```

Generic builtin enums can be constructed through their enum name:

```python
ok: Result[int32, String] = Result.Ok(7)
missing: Option[String] = Option.None
```

## Statement Match

```python
match ready:
    case Status.Ready(count):
        print(count)
    case Status.Failed(message):
        print(message)
    case Status.Empty:
        print("empty")
```

Enum matches must be exhaustive. A wildcard arm handles remaining variants:

```python
match ready:
    case Status.Ready(count):
        print(count)
    case _:
        print("not ready")
```

## Match Expressions

`match` can also produce a value:

```python
def status_label(status: Status) -> String:
    return match status:
        case Status.Ready(count):
            f"ready: {count}"
        case Status.Failed(message):
            message
        case Status.Empty:
            "empty"
```

Every expression arm must produce a compatible type.

## Short-Form Patterns

When the scrutinee type is known, variants may be matched without the enum prefix:

```python
result: Result[int32, String] = Result.Ok(7)

match result:
    case Ok(value):
        print(value)
    case Err(message):
        print(message)
```

Use short-form patterns sparingly. The qualified form is always valid and is the safer reference style, especially for `Option.Some(...)` and `Option.None`, because those names depend on local type context:

```python
match result:
    case Result.Ok(value):
        print(value)
    case Result.Err(message):
        print(message)
```

## Borrowed Matches

Matching by value may move payloads out of the scrutinee. Use `match borrow` when the match should inspect without consuming:

```python
result: Result[String, String] = Result.Ok("hello")

match borrow result:
    case Result.Ok(message):
        print(message)
    case Result.Err(error):
        print(error)

print("still have result")
```

Use `match borrow mut` when an arm needs to mutate through a pattern binding.

## Literal Patterns

`match` supports literal patterns for `bool`, integers, floats, and `String`:

```python
match code:
    case 200:
        print("ok")
    case 404:
        print("missing")
    case _:
        print("other")
```

Integer, float, and string matches require a wildcard or otherwise exhaustive coverage because their value spaces are open-ended. Boolean matches are exhaustive if both `true` and `false` are covered.

## Builtin Enums

| Type | Variants |
| --- | --- |
| `Option[T]` | `Some(value: T)`, `None` |
| `Result[T, E]` | `Ok(value: T)`, `Err(error: E)` |
| `SendError[T]` | `Closed(value: T)`, `Cancelled(value: T)`, `TimedOut(value: T)`, `Full(value: T)` |
| `QueueReceive[T]` | `Item(value: T)`, `Closed`, `TimedOut`, `Cancelled` |
| `TaskResult[T]` | `Ready(value: T)`, `Error(message: String)`, `TimedOut`, `Cancelled` |
| `WaitAny[T]` | `Ready(index: int32, value: T)`, `Error(index: int32, message: String)`, `TimedOut`, `Cancelled` |
| `WaitAll[T]` | `Ready(values: Vec[T])`, `Error(index: int32, message: String)`, `TimedOut`, `Cancelled` |
| `io.Error` | see [I/O Module](/manual/io) |
| `process.ExitStatus` | see [Process Module](/manual/process) |
| `process.Wait` | see [Process Module](/manual/process) |
| `process.RestartPolicy` | see [Process Module](/manual/process) |
| `process.Error` | see [Process Module](/manual/process) |
| `process.SupervisorEvent` | see [Process Module](/manual/process) |
| `process.SupervisorWait` | see [Process Module](/manual/process) |

## Exhaustiveness And Wildcards

Aurora checks enum match exhaustiveness. Prefer explicit variants when the program has meaningful behavior for each case. Use `_` when all remaining cases intentionally share one behavior.

Do not use `_` to hide an outcome you have not thought about. APIs such as `QueueReceive`, `TaskResult`, and `process.Wait` distinguish timeout and cancellation because those states often require different cleanup or retry behavior.
