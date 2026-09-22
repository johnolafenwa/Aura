# Results And Optional Values

Values can be absent, and operations can fail. Aura puts both facts in the type system, so the code shows which calls can go wrong and how.

This chapter covers optional values written `T | None`, `Result[T, E]`, the `try` expression, and the outcome enums the runtime uses for lookups, queues, tasks, and I/O. Together they replace exceptions, null pointers, and sentinel values with ordinary control flow.

## `T | None`: A Value May Be Missing

An optional value is a union whose last member is `None`. Aura has no builtin `Option` type, `Some` constructor, or `T?` suffix. You write the type as `str | None`. At a typed destination, a bare value enters the union and `None` selects its absence member.

Use it when absence is expected and is not an error:

- a prefix may not be present (`str.strip_prefix`)
- an environment variable may be unset (`sys.env`)
- a stream may reach end-of-file (`io.read_line`)
- a search may find nothing

```aura
def find_name(names: list[str], wanted: str) -> str | None:
    for name in names:
        if name == wanted:
            return name.clone()
    return None

def describe(found: str | None):
    if found is None:
        print("missing")
        return
    print(found.len())

names = ["Ada", "Grace"]
describe(find_name(names, "Grace"))

match find_name(names, "Katherine"):
    case str as name:
        print(name)
    case None:
        print("missing")
```

This prints `5` and `missing`.

- **`is None` and `is not None`** test presence and narrow a stable place. A stable place is an owned or `mut` local, a parameter, a field of one of those, or a view. After `if found is None: return`, `found` has type `str` for the rest of the function.
- **A call result** is not a stable place. Bind it to a local first.
- **`match`** selects a member with a type pattern such as `case str as name:`. `case None:` covers absence.
- **`print`** renders an optional's payload. A `None` prints an empty line.

## `Lookup[T]`: Was There An Entry?

`list.get`, `dict.get`, and `dict.remove` return the builtin enum `Lookup[T]` instead of `T | None`. This keeps a missing entry distinct from a present entry whose value is `None`: `Lookup.Found(None)` differs from `Lookup.Missing`.

```aura
names = ["Ada", "Grace"]

match names.get(3):
    case Lookup.Found(name):
        print(name)
    case Lookup.Missing:
        print("missing")
```

The short patterns `Found(name)` and `Missing` also work when the compiler already knows the scrutinee's type. The qualified form is always clear, so reference material uses it.

`Queue.poll` and `Task.poll` make the same distinction with `Poll[T]`. They return `Poll.Ready(value)` when a value is available and `Poll.Unavailable` otherwise.

## `Result[T, E]`: A Caller Must Decide

`Result[T, E]` is either `Result.Ok(value)` or `Result.Err(error)`. Use it when an operation may fail and the caller should decide what to do:

```aura
def divide(left: int32, right: int32) -> Result[int32, str]:
    if right == 0:
        return Result.Err("division by zero")
    return Result.Ok(left // right)

match divide(10, 2):
    case Result.Ok(value):
        print(value)
    case Result.Err(message):
        print(message)
```

A command-line tool might print the error and exit with a non-zero code. A server might turn it into a response. A parser might recover and move on. None of those choices belongs in the library. They belong at the call site, which is where the `match` lives.

## Parsing Is A `Result`

The parsing builtins return `Result`:

```aura
def read_limit(text: str) -> Result[int32, str]:
    match parse_int32(text):
        case Result.Ok(value):
            if value < 0:
                return Result.Err("limit must be non-negative")
            return Result.Ok(value)
        case Result.Err(message):
            return Result.Err(message)
```

The signature says what the caller receives: an `int32` or a `str` error message. The function has no hidden path by which it might throw.

## `try`: Propagate Failure

`try expr` evaluates `expr`:

- If the result is `Result.Ok(value)`, the expression produces `value` and execution continues.
- If the result is `Result.Err(error)`, the current function returns that error at once.

```aura
def parse_pair(left: str, right: str) -> Result[int32, str]:
    a = try parse_int32(left)
    b = try parse_int32(right)
    return Result.Ok(a + b)
```

`read_limit` above shows the pattern that `try` shortens: call a sub-operation, check for failure, and hand the same error back. Use `try` when the current function cannot usefully recover. It keeps the success path readable and keeps an explicit `Result` return type.

Use `match` instead when the function has a local way to recover:

```aura
def parse_or_zero(text: str) -> int32:
    match parse_int32(text):
        case Result.Ok(value):
            return value
        case Result.Err(_message):
            return 0
```

Two constraints apply to `try`:

1. It can only appear in a function whose return type is a compatible `Result`.
2. The inner `Result`'s error type must match the outer function's error type. If they differ, convert explicitly with a `match` or a helper.

## Domain-Specific Outcomes

A plain `Result[T, str]` does not describe every failure well. Aura APIs use richer enums when the caller gains from telling outcomes apart.

| API family | Outcome type | Why this shape |
| --- | --- | --- |
| `fs`, `io`, `net` | `Result[T, io.Error]` | Operating-system and protocol failures have named categories (`NotFound`, `TimedOut`, `BrokenPipe`, ...). |
| `process` | `Result[T, process.Error]` | Spawning, waiting, status checks, and pipes have process-specific failure modes. |
| `list.get`, `dict.get`, `dict.remove` | `Lookup[T]` | A missing entry is `Lookup.Missing`. A present entry is `Lookup.Found(value)`, even when the stored value is `None`. |
| `Queue.poll`, `Task.poll` | `Poll[T]` | `Poll.Ready(value)` when a value is available. `Poll.Unavailable` when nothing is, whether through closure, failure, timeout, or cancellation. It suits callers that treat those cases alike. |
| `Queue.put` | `Result[None, SendError[T]]` | A failed send returns the unsent value, so the caller can retry, queue it elsewhere, or log it. |
| `Queue.get` | `QueueReceive[T]` | A receive has four distinct outcomes: it produces an item, sees a close, times out, or is cancelled. |
| `Task.result` | `TaskResult[T]` | A task can finish normally, fail, time out, or be cancelled. |
| `wait_any` | `WaitAny[T]` | The caller sees which task completed, with its value or error. |
| `wait_all` | `WaitAll[T]` | Either every value is available, or the first failing index is reported. |

Task-result and multi-task wait APIs clone a stored successful value, so their result type must be clone-safe. An observation that would return `random.Rng`, even through a wrapper, fails with `AU3007`. Queue receive outcomes transfer one owned item and have no such restriction.

With the right enum, a program can handle one case specifically and still cover the rest:

```aura
import fs
import io

def read_config(path: str) -> str:
    match fs.read_to_string(path):
        case Result.Ok(text):
            return text
        case Result.Err(io.Error.NotFound):
            return "mode=default"
        case Result.Err(error):
            print(error)
            return "mode=default"
```

A missing config file is a normal condition when a default exists. Every other I/O error is reported and falls back to the same default. The policy is local, specific, and visible.

## Retrying A Result Worker

To retry every `Err` under one attempt budget, pass a capture-free worker to `control.retry`:

```aura
import control

def fetch_once() -> Result[str, str]:
    return Result.Err("service unavailable")

result = control.retry(
    fetch_once,
    max_attempts=3,
    initial_backoff=10ms
)
```

- The first attempt runs at once. Later attempts wait `10ms`, then `20ms`, and so on.
- There is no delay after the final attempt. The helper returns that attempt's exact `Err`.
- A zero backoff skips sleeping.
- Worker traps and task cancellation propagate. They never appear as the worker's error type.

The helper retries every error. Write an explicit loop when the application must classify errors, add jitter, or stop on a status such as an HTTP `429`. [`retrying_network_worker.au`](../../examples/agents/retrying_network_worker.au) shows that policy-rich form. [`retry_with_backoff.au`](../../examples/agents/retry_with_backoff.au) shows the generic helper.

## Choosing Between Them

For day-to-day code:

- Use **`T | None`** when absence is an ordinary state.
- Use **`Lookup[T]`** and **`Poll[T]`** when a present `None` must stay distinct from absence. The collection and concurrency APIs hand them to you.
- Use **`Result[T, E]`** when failure needs a reason.
- Use **`try`** when the only useful local behaviour is "return this error to my caller."
- Use **`match`** when the current function can make a decision.
- Use the **domain-specific outcome enum** when the API hands you one. A `QueueReceive`, a `TaskResult`, or a `process.Wait` already has the right shape. Collapsing it into a string throws away information.

The next chapter turns to program structure: splitting code across files, modules, and packages so domain types and their error shapes stay organised as programs grow.

Reference: [Types](/manual/types#optional-and-result-types), [Enums And Pattern Matching](/manual/enums-and-match), [Expressions](/manual/expressions).
