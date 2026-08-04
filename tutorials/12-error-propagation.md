# Error Propagation

When functions return `Result[T, E]`, chaining multiple fallible operations with `match` can get deeply nested. Aura provides `try expr` to flatten this pattern.

## `try expr`

`try expr` evaluates the expression, which must produce a `Result[T, E]`. If the result is `Ok(value)`, `try` unwraps it and the expression evaluates to the inner value. If the result is `Err(e)`, Aura returns that error from the current function immediately.

```aura
def divide(a: int32, b: int32) -> Result[int32, str]:
    if b == 0:
        return Result.Err("division by zero")
    return Result.Ok(a // b)

def add_one_after_divide(a: int32, b: int32) -> Result[int32, str]:
    value = try divide(a, b)
    return Result.Ok(value + 1)
```

In `add_one_after_divide`, `try divide(a, b)` either:

- unwraps the `Ok` payload into `value` and continues, or
- returns `Result.Err("division by zero")` from `add_one_after_divide` immediately

Without `try`, the same function would need a nested `match`:

```aura
def add_one_after_divide(a: int32, b: int32) -> Result[int32, str]:
    match divide(a, b):
        case Ok(value):
            return Result.Ok(value + 1)
        case Err(message):
            return Result.Err(message)
```

## Chaining Multiple Operations

`try` shines when chaining several fallible calls:

```aura
def compute(input: str) -> Result[int32, str]:
    parsed = try parse_int32(input)
    doubled = try divide(parsed * 2, 3)
    return Result.Ok(doubled + 1)
```

Each `try` either succeeds and continues to the next line, or short-circuits the entire function with the error. This reads top-to-bottom like normal code.

## Using `try` Inside Expressions

`try` can appear inside larger expressions:

```aura
def add_parsed(a: str, b: str) -> Result[int32, str]:
    return Result.Ok(try parse_int32(a) + try parse_int32(b))
```

## Using `try` Inside `with` Blocks

`try` works inside `with` blocks. The resource cleanup still runs when `try` triggers an early return:

```aura
def process_file(handle: own FileHandle) -> Result[str, str]:
    with file = handle:
        value = try validate(file.read())
        return Result.Ok(value)
    # file.close() runs even if try propagates an error
```

## Rules

- `try` is only valid inside a function body
- the enclosing function must return `Result[T, E]`
- the `try` expression must produce `Result[U, E]` -- the error type `E` must match exactly
- `try` unwraps `Ok(value)` to the inner type `U`

The error type matching is strict in the bootstrap compiler. If your function returns `Result[int32, str]`, every `try` expression must also use `str` as its error type.

See:

- [examples/error_handling/try_result.au](../examples/error_handling/try_result.au)
- [10-results-and-options.md](./10-results-and-options.md)
