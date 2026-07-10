# Classes

Classes group fields and methods into owned product types. Use a class when the fields exist together and the program benefits from giving that group a name.

## Declaration

```python
class Point:
    x: float64
    y: float64
```

Fields are declared with names and types. Fields can have defaults:

```python
class Server:
    host: String = "127.0.0.1"
    port: int32 = 8080
```

Construct classes with named fields:

```python
point = Point(x=3.0, y=4.0)
server = Server()
custom = Server(port=9090)
```

Ordinary classes are move types.

## Visibility

Top-level `public class` exports the class from a module:

```python
public class Counter:
    value: int32 = 0
```

Fields and methods may also be `public` where module boundaries require external access. Keep representation private unless callers genuinely need it.

## Methods

```python
class Counter:
    value: int32 = 0

    def get(borrow self) -> int32:
        return self.value

    def increment(borrow mut self):
        self.value += 1

    def zero() -> Counter:
        return Counter(value=0)
```

Receiver forms:

| Receiver | Behavior |
| --- | --- |
| `borrow self` | Read-only receiver. Cannot move non-copy fields out. |
| `borrow mut self` | Exclusive mutable receiver. May update fields. |
| `self` | Consuming receiver. Takes ownership of the instance. |
| none | Associated method called on the type. |

Call methods with member syntax:

```python
mut counter = Counter.zero()
counter.increment()
print(counter.get())
```

## Returning Owned Fields From Borrowed Methods

A borrowed method may clone an owned field:

```python
class User:
    name: String

    def name_copy(borrow self) -> String:
        return self.name.clone()
```

Returning `self.name` directly would move a `String` out through a shared borrow and is rejected.

## copy class

```python
copy class Pair:
    left: int32
    right: int32
```

`copy class` is allowed only when every field is copyable. Copy classes duplicate on assignment and by-value calls.

Use `copy class` for small value-like records. Use an ordinary class when ownership should be explicit.

## Recursive Fields

Use `indirect` for recursive references:

```python
class Node:
    value: int32
    next: indirect Option[Node] = Option.None
```

Direct recursive fields are not implemented because the value would not have a finite size.

## Resource Classes

A class can participate in `with` cleanup by defining `close(borrow mut self)`:

```python
class Resource:
    name: String

    def close(borrow mut self):
        print("closing " + self.name)

with resource = Resource(name="db"):
    print("using resource")
```

Cleanup runs when the `with` block exits, including error paths that unwind through the scope.

## Mutation

Assign to fields through a mutable owned value or a mutable borrow:

```python
mut counter = Counter.zero()
counter.value = 10
```

Inside `borrow mut self` methods, field assignment mutates the receiver owned by the caller.
