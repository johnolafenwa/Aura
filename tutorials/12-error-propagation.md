# Error Propagation

`try expr` passes an error up to the caller so you do not have to write a `match` for every fallible call. This chapter shows how it works and where you can use it.

## `try expr`

`try expr` evaluates an expression that produces a `Result[T, E]`:

- If the result is `Ok(value)`, the `try` expression evaluates to `value`.
- If the result is `Err(e)`, the current function returns that error immediately.

```aura check-pass
def divide(a: int32, b: int32) -> Result[int32, str]:
    if b == 0:
        return Result.Err("division by zero")
    return Result.Ok(a // b)

def add_one_after_divide(a: int32, b: int32) -> Result[int32, str]:
    value = try divide(a, b)
    return Result.Ok(value + 1)
```

Here `try divide(a, b)` does one of two things:

- It puts the `Ok` payload in `value`, and the function continues.
- It returns `Result.Err("division by zero")` from `add_one_after_divide`.

Without `try`, the same function needs a `match`:

```aura fragment
def add_one_after_divide(a: int32, b: int32) -> Result[int32, str]:
    match divide(a, b):
        case Ok(value):
            return Result.Ok(value + 1)
        case Err(message):
            return Result.Err(message)
```

## Chaining Multiple Operations

`try` is most useful when a function makes several fallible calls:

```aura fragment
def compute(input: str) -> Result[int32, str]:
    parsed = try parse_int32(input)
    doubled = try divide(parsed * 2, 3)
    return Result.Ok(doubled + 1)
```

Each `try` either continues to the next line or returns the error from the whole function. The code reads top to bottom.

## Using `try` Inside Expressions

A `try` can appear inside a larger expression:

```aura check-pass
def add_parsed(a: str, b: str) -> Result[int32, str]:
    return Result.Ok(try parse_int32(a) + try parse_int32(b))
```

## Using `try` Inside `with` Blocks

When `try` returns early from inside a `with` block, the resource is still closed:

```aura fragment
def process_file(handle: own FileHandle) -> Result[str, str]:
    with file = handle:
        value = try validate(file.read())
        return Result.Ok(value)
    # file.close() runs even if try propagates an error
```

## Rules

- `try` is valid only inside a function body.
- The enclosing function must return `Result[T, E]`.
- The `try` operand must produce `Result[U, SourceError]`.
- `try` unwraps `Ok(value)` to the inner type `U`.
- The enclosing function's error type must be `SourceError`, or there must be a visible, applicable `impl From[SourceError] for TargetError`.

When the error types match, the error propagates as is. When they differ, Aura calls the selected `From.from` implementation and returns the converted `Result.Err(...)`. The Manual's [`From` And `try`](../docs/manual/generics-and-traits.md#from-and-try) section gives the trait contract and conversion rules.

See also:

- [examples/error_handling/try_result.au](../examples/error_handling/try_result.au)
- [10-results-and-options.md](./10-results-and-options.md)
