# Results, Options, And `try`

A program that runs for any length of time has to deal with two uncomfortable facts: values can be absent, and operations can fail. Aura represents both in the type system so the code is honest about which calls might go wrong and how.

This chapter introduces `Option[T]`, `Result[T, E]`, the `try` expression, and the outcome enums the runtime uses for queues, tasks, and I/O. Together they are how Aura replaces exceptions, null pointers, and sentinel values with ordinary control flow.

## `Option[T]`: A Value May Be Missing

`Option[T]` is either `Some(value)` or `None`. Use it when absence is expected and is not itself an error:

- a vector index may be out of range
- a map key may be absent
- a stream may reach end-of-file
- a timed wait may finish with no value

```python
names = ["Ada", "Grace"]

match names.get(3):
    case Option.Some(name):
        print(name)
    case Option.None:
        print("missing")
```

The short-form patterns `Some(name)` and `None` also work when the compiler already knows the scrutinee's type, but the qualified form is always clear and reads well in reference material.

## `Result[T, E]`: A Caller Must Decide

`Result[T, E]` is either `Result.Ok(value)` or `Result.Err(error)`. Use it when an operation may fail and the caller should decide what to do:

```python
def divide(left: int32, right: int32) -> Result[int32, String]:
    if right == 0:
        return Result.Err("division by zero")
    return Result.Ok(left // right)

match divide(10, 2):
    case Result.Ok(value):
        print(value)
    case Result.Err(message):
        print(message)
```

A command-line tool might print the error and stop with a non-zero exit code. A server might turn it into a response. A parser might recover and move on. None of those choices belongs in the library; they belong at the call site, which is exactly where the `match` lives.

## Parsing Is A `Result`

The parsing builtins return `Result`:

```python
def read_limit(text: String) -> Result[int32, String]:
    match parse_int32(text):
        case Result.Ok(value):
            if value < 0:
                return Result.Err("limit must be non-negative")
            return Result.Ok(value)
        case Result.Err(message):
            return Result.Err(message)
```

The signature is honest: the caller will receive either an `int32` or a `String` error message. There is no hidden path through which this function might throw.

## `try`: Propagate Failure

That last function has a familiar shape. It calls a sub-operation, checks whether it failed, and if it did, hands the same error back to its caller. That pattern is common enough to deserve a short form, so Aura provides one: **`try`**.

`try expr` evaluates `expr`. If the result is `Result.Ok(value)`, the expression produces `value` and execution continues. If the result is `Result.Err(error)`, the current function returns that error immediately.

```python
def parse_pair(left: String, right: String) -> Result[int32, String]:
    a = try parse_int32(left)
    b = try parse_int32(right)
    return Result.Ok(a + b)
```

`try` is for the common case where the current function cannot usefully recover. It keeps the happy path readable while preserving an explicit `Result` return type.

Use `match` instead when the function has a local recovery strategy:

```python
def parse_or_zero(text: String) -> int32:
    match parse_int32(text):
        case Result.Ok(value):
            return value
        case Result.Err(_message):
            return 0
```

Two constraints on `try`:

1. It can only appear in a function whose return type is a compatible `Result`.
2. The error type of the inner `Result` must match the outer function's error type. If they differ, convert explicitly with a `match` or a helper.

## Domain-Specific Outcomes

Not every failure is well-described by a plain `Result[T, String]`. Aura APIs use richer enums when the caller benefits from distinguishing outcomes.

| API family | Outcome type | Why this shape |
| --- | --- | --- |
| `fs`, `io`, `net` | `Result[T, io.Error]` | Operating-system and protocol failures have named categories (`NotFound`, `TimedOut`, `BrokenPipe`, ...). |
| `process` | `Result[T, process.Error]` | Spawning, waiting, status checks, and pipes have process-specific failure modes. |
| `Queue.put` | `Result[None, SendError[T]]` | A failed send returns the unsent value so the caller can retry, queue elsewhere, or log it. |
| `Queue.get` | `QueueReceive[T]` | A receive can produce an item, observe a close, time out, or be cancelled — four distinct outcomes. |
| `Task.result` | `TaskResult[T]` | A task can finish normally, fail, time out, or be cancelled. |
| `wait_any` | `WaitAny[T]` | The caller sees which task completed, with its value or error. |
| `wait_all` | `WaitAll[T]` | Either every value is available, or the first failing index is reported. |

Task-result and multi-task wait APIs clone a stored successful value. Their
result type must therefore be clone-safe: an observation that would return
`random.Rng`, including through a wrapper, is rejected with `AU3007`. Queue
receive outcomes transfer one owned item and do not have this restriction.

Using the right enum lets a program handle one case specifically while still handling the others:

```python
import fs
import io

def read_config(path: String) -> String:
    match fs.read_to_string(path):
        case Result.Ok(text):
            return text
        case Result.Err(io.Error.NotFound):
            return "mode=default"
        case Result.Err(error):
            print(error)
            return "mode=default"
```

"File not found" is a normal condition for a config file with a default value. Every other I/O error is reported and falls back to the same default. Policy is local, specific, and visible.

## Retrying A Result Worker

When every `Err` should be retried under one simple attempt budget, pass a
capture-free worker to `control.retry`:

```python
import control

def fetch_once() -> Result[String, String]:
    return Result.Err("service unavailable")

result = control.retry(
    fetch_once,
    max_attempts=3,
    initial_backoff=10ms
)
```

The first attempt runs immediately. Later attempts wait for `10ms`, then
`20ms`, and so on. There is no delay after the final attempt, and the helper
returns that final attempt's exact `Err`. A zero backoff skips sleeping.
Worker traps and task cancellation propagate; they do not masquerade as the
worker's error type.

This helper retries every error. Keep an explicit loop when the application
must classify errors, add jitter, or stop on a status such as an HTTP `429`.
The maintained
[`retrying_network_worker.au`](../../examples/agents/retrying_network_worker.au)
shows that policy-rich form, while
[`retry_with_backoff.au`](../../examples/agents/retry_with_backoff.au)
demonstrates the generic helper.

## Choosing Between Them

A rule of thumb for day-to-day code:

- Use **`Option[T]`** when absence is an ordinary state.
- Use **`Result[T, E]`** when failure needs a reason.
- Use **`try`** when the only useful local behaviour is "return this error to my caller."
- Use **`match`** when the current function can make a decision.
- Use the **domain-specific outcome enum** when the API hands you one. A `QueueReceive`, a `TaskResult`, or a `process.Wait` is already the right shape; collapsing it into a string loses information the API went to some trouble to preserve.

The next chapter turns to program structure: splitting code across files, modules, and packages so domain types and their error shapes stay organised as programs grow.

Reference: [Enums And Pattern Matching](/manual/enums-and-match), [Expressions](/manual/expressions).
