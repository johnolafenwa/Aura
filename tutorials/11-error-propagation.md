# Error Propagation

Aurora supports built-in `Result[T, E]` values and `try expr`.

## `try expr`

`try expr` unwraps the `Ok(...)` payload from a `Result`.

If the value is `Err(...)`, Aurora returns that error from the current function immediately.

```aurora
def divide(a: int32, b: int32) -> Result[int32, String]:
    if b == 0:
        return Result.Err("division by zero")
    return Result.Ok(a / b)

def add_one_after_divide(a: int32, b: int32) -> Result[int32, String]:
    value = try divide(a, b)
    return Result.Ok(value + 1)
```

## Rules in the current compiler

- `try` is only valid inside a function body
- the enclosing function must return `Result[T, E]`
- the `try` expression must also produce `Result[U, E]`
- the error type must match exactly in the bootstrap compiler
- `try` can be used inside larger expressions and inside `with` blocks as long as those rules hold

See:

- [examples/error_handling/try_result.au](../examples/error_handling/try_result.au)
- [tutorials/09-results-and-options.md](./09-results-and-options.md)
