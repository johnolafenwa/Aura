# Ownership And Borrowing

Aurora tracks moves and borrows at compile time. The rules define who may use a value, who may mutate it, and who is responsible for closing resources.

## Copy Types

Copy types duplicate on assignment and by-value calls:

- all numeric types
- `bool`
- `Duration`
- `Queue[T]`
- `Task[T]`
- `copy class` values whose fields are copyable

```python
a = 1
b = a
print(a)
print(b)
```

## Move Types

Move types transfer ownership:

- `String`
- `Vec[T]`
- `Map[K, V]`
- `Set[T]`
- `TaskGroup`
- user-defined non-copy classes
- file, process, supervisor, and network resources

```python
name = "aurora"
other = name
print(other)
```

After a move, the moved binding or field cannot be used until it is reinitialized.

## Borrow Forms

| Form | Meaning |
| --- | --- |
| `value: borrow T` | Shared borrowed parameter. |
| `value: borrow mut T` | Exclusive mutable borrowed parameter. |
| `value` passed to `borrow T` | Shared borrow selected by the parameter type. |
| `mutable_value` passed to `borrow mut T` | Mutable borrow selected by the parameter type. |
| `borrow self` | Shared method receiver. |
| `borrow mut self` | Mutable method receiver. |
| `for value in borrow collection:` | Shared-borrow iteration. |
| `for value in borrow mut collection:` | Mutable-borrow iteration. |
| `match borrow value:` | Shared borrowed pattern matching. |
| `match borrow mut value:` | Mutable borrowed pattern matching. |

Shared borrows allow reading but do not allow moving non-copy data out of the borrowed value. Mutable borrows allow mutation through one exclusive path.

## Shared Borrow Example

```python
def render(name: borrow String) -> String:
    return name.to_upper()

name = "aurora"
print(render(name))
print(name)
```

The caller keeps ownership of `name`.

## Mutable Borrow Example

```python
def add_name(names: borrow mut Vec[String], name: String):
    names.push(name)

mut names = Vec[String]()
add_name(names, "Ada")
```

The caller keeps ownership of the vector, but the function mutates it temporarily.

## Exclusivity

When a call passes a value to a `borrow mut` parameter, no other argument may overlap the same place:

```python
update(point, point) # rejected: overlapping mutable and shared borrow
```

Overlapping mutable borrows are rejected even if the callee would happen to use them in a harmless order. The type checker works from the call boundary, not from a best-case execution path inside the function.

Sibling fields are distinct only when the checker can prove the paths do not overlap.

## Field Moves

Moving a non-copy field out of an owned class moves that field:

```python
class User:
    name: String
    id: int32

mut user = User(name="Ada", id=1)
name = user.name

print(user.id)
user.name = "Grace"
print(user.name)
```

Moving a non-copy field out of a borrowed value is rejected:

```python
def bad(user: borrow User) -> String:
    return user.name # rejected
```

Clone if the function should return an owned value:

```python
def good(user: borrow User) -> String:
    return user.name.clone()
```

## Borrowed Pattern Matching

Use `match borrow` to inspect an enum without consuming it:

```python
result: Result[String, String] = Result.Ok("ready")

match borrow result:
    case Result.Ok(value):
        print(value)
    case Result.Err(error):
        print(error)
```

Use `match borrow mut` when an arm needs mutable access to a payload.

## Clone

Use `.clone()` to explicitly create another owned move value:

```python
name = "aurora"
copy = name.clone()
print(name)
print(copy)
```

Collections clone their contents when `.clone()` is called.

## Tasks And Borrowing

Spawned task arguments must be owned or copy values. Borrowed task parameters are not supported because a task can outlive the call frame that started it.

```python
def worker(label: String):
    print(label)

with group = TaskGroup():
    label = "compile"
    group.start_soon(worker, label.clone())
    print(label)
```

## Resources

Resource ownership should usually be lexical:

```python
import fs
import io

def show_file() -> Result[None, io.Error]:
    with file = try fs.open("data.txt"):
        text = try file.read_all()
        print(text)
    return Result.Ok(None)
```

Leaving the block runs cleanup. This applies to file handles, task groups, network resources, process children, pipes, and supervisors according to their resource behavior.
