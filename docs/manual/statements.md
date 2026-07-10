# Statements

Statements control execution and bind values.

## Assignment

```python
name = "aurora"
mut count = 0
count += 1
point.x = 4.0
values[0] = 9
```

Use `mut` for bindings that will be reassigned or passed as `borrow mut`.

## Return

```python
def answer() -> int32:
    return 42
```

`return` is only valid inside functions.

## Conditionals

```python
if value < 0:
    print("negative")
elif value == 0:
    print("zero")
else:
    print("positive")
```

## Loops

```python
for value in range(10):
    if value == 5:
        break

while not cancelled():
    sleep(10ms)
```

`break` and `continue` work inside loops.

## For Iteration

Supported iterable forms:

| Form | Behavior |
| --- | --- |
| `for i in range(n):` | Iterates integers. |
| `for value in vec:` | Iterates owned vector values. |
| `for value in borrow vec:` | Shared-borrow vector iteration. |
| `for value in borrow mut vec:` | Mutable vector iteration; iterable must be mutable. |
| `for value in set:` | Iterates set values. |
| `for value in borrow set:` | Shared-borrow set iteration. |
| `for value in queue:` | Receives queue items until close/cancellation/producers complete. |

## Match Statements

```python
match result:
    case Result.Ok(value):
        print(value)
    case Result.Err(message):
        print(message)
```

Statement-form enum matches must be exhaustive unless a wildcard arm covers the remainder.

## With

```python
import fs
import io

def show_file() -> Result[None, io.Error]:
    with file = try fs.open("data.txt"):
        text = try file.read_all()
        print(text)
    return Result.Ok(None)
```

`with` calls `close()` on scope exit. Cleanup runs on normal exit and runtime error paths in the maintained interpreter and direct backend.

## pass

`pass` is a no-op statement:

```python
def placeholder():
    pass
```
