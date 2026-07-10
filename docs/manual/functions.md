# Functions

Functions use `def`, typed parameters, and an explicit return type unless they return `None`.

```python
def add(left: int32, right: int32) -> int32:
    return left + right
```

Functions are the main unit of type checking, borrowing, `try` propagation, and task starting.

## Return Types

Use `-> Type` for functions that return a value:

```python
def square(value: int32) -> int32:
    return value * value
```

Functions that return `None` may omit the annotation:

```python
def log(message: String):
    print(message)
```

An explicit `-> None` is also valid when it makes an interface clearer.

## Parameters

Ordinary parameters receive owned values:

```python
def consume(name: String):
    print(name)
```

For move types, the caller loses ownership after passing by value. Use borrowed parameters when the function should not take ownership.

Shared borrow:

```python
def length(text: borrow String) -> int32:
    return text.len()
```

Mutable borrow:

```python
def push_name(names: borrow mut Vec[String], name: String):
    names.push(name)
```

Borrowed ordinary parameters use `name: borrow T` or `name: borrow mut T`.

## Default Arguments

Parameters may have default values:

```python
def greet(name: String = "world"):
    print("hello " + name)
```

Default values are evaluated for each call.

```python
greet()
greet("aurora")
greet(name="aurora")
```

Use defaults for common policy, not for hiding required data. A timeout default can make sense. A missing file path usually should not.

## Named Arguments

Calls may use named arguments:

```python
greet(name="aurora")
```

Positional arguments must come before named arguments:

```python
import process

process.run(["/bin/echo", "hi"], stdout=process.pipe(), group=true)
```

Named arguments are especially useful for builtin APIs that accept several values of the same type:

```python
import net

net.http_request_text_timeout(method="POST", url="http://127.0.0.1:8080/jobs", body="{}", headers={}, timeout=2s)
```

## `return`

`return expression` exits the current function with a value:

```python
def classify(value: int32) -> String:
    if value < 0:
        return "negative"
    return "non-negative"
```

`return` without an expression is valid in `None` functions:

```python
def maybe_log(enabled: bool):
    if not enabled:
        return
    print("enabled")
```

Expression-form `match` is often useful in return positions:

```python
def status_name(code: int32) -> String:
    return match code:
        case 0:
            "ok"
        case _:
            "other"
```

## `try` In Functions

`try` can only appear inside a function whose return type is a compatible `Result`.

```python
def parse_total(left: String, right: String) -> Result[int32, String]:
    a = try parse_int32(left)
    b = try parse_int32(right)
    return Result.Ok(a + b)
```

When `parse_int32(left)` returns `Result.Err(message)`, `parse_total` returns that error immediately.

## Borrowed Returns

Borrowed returns use labels to say which input the returned borrow comes from:

```python
def identity(value: borrow[source] String) -> borrow[source] String:
    return value
```

This is for advanced zero-copy APIs. Most application code should return owned values until a borrowed return is clearly worth the extra precision.

## Generic Functions

Type parameters go after the function name:

```python
def identity[T](value: T) -> T:
    return value
```

Trait bounds restrict type parameters:

```python
def describe[T: Greeter](value: borrow T) -> String:
    return value.greet()
```

Multiple bounds use `+`:

```python
def use_both[T: First + Second](value: T) -> int32:
    return value.score()
```

## Function Values And Task Starts

Task groups can start named functions and associated methods without `self`:

```python
def work(value: int32) -> int32:
    return value * 2

with group = TaskGroup():
    task = group.start(work, 21)
```

Spawned functions receive owned arguments. Borrowed task parameters are not currently supported.

## main

Valid entrypoint forms:

```python
def main() -> int32:
    return 0
```

```python
def main():
    print("done")
```

`main` cannot take parameters. `main` may return `int32` or `None`. Other return types are rejected.

Do not mix executable top-level statements and `main` in the same file.
