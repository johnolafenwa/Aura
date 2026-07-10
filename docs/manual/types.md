# Types

Aurora is statically typed. Every expression has a type, and type annotations are part of the public shape of functions, fields, methods, and many empty literals.

The type system is designed to keep three facts visible:

- what kind of value a program has
- whether the value is copied or moved
- whether failure is represented in the return type

## Scalar Types

| Type | Description |
| --- | --- |
| `bool` | Boolean value: `true` or `false`. |
| `int8`, `int16`, `int32`, `int64`, `int128`, `intsize` | Signed integers. |
| `uint8`, `uint16`, `uint32`, `uint64`, `uint128`, `uintsize` | Unsigned integers. |
| `float32`, `float64` | Floating-point values. |
| `String` | Owned UTF-8 string. |
| `str` | Borrowed string type in borrowed positions. |
| `None` | Unit type and unit value. |
| `Duration` | Runtime duration used by sleeps, timeouts, and scheduling APIs. |
| `Range` | Integer range returned by `range(...)`. |

Numeric literals are checked against the target type. Integer literals must fit the annotated integer type. Integer-to-float casts reject silent precision loss.

`str` is a borrowed string view used by the checker in borrowed positions. Most everyday Aurora programs write owned `String` values and `borrow String` parameters.

## Copy And Move Categories

Copy values may be reused after assignment or by-value calls:

- numbers
- `bool`
- `Duration`
- `Queue[T]`
- `Task[T]`
- `copy class` values whose fields are all copyable

Move values transfer ownership:

- `String`
- `Vec[T]`
- `Map[K, V]`
- `Set[T]`
- ordinary user classes
- `TaskGroup`
- file, process, supervisor, and network resources

Move values can still be shared through `borrow` and `borrow mut`, or duplicated explicitly through methods such as `.clone()` when the type supports cloning.

`Queue[T]` and `Task[T]` are copy handles to shared runtime state. Copying the handle does not copy queued values or task results; it gives another reference to the same queue or task.

## Builtin Generic Types

| Type | Meaning |
| --- | --- |
| `Option[T]` | `Some(T)` or `None`; use for ordinary absence. |
| `Result[T, E]` | `Ok(T)` or `Err(E)`; use for recoverable failure. |
| `Vec[T]` | Owned ordered collection. |
| `Map[K, V]` | Owned key/value map. |
| `Set[T]` | Owned set of unique values. |
| `MapEntry[K, V]` | Entry value returned by `Map.items()` and `Map.entries()`. |
| `Queue[T]` | Scheduler-aware typed queue handle. |
| `Task[T]` | Copy handle to a task result. |
| `SendError[T]` | Queue send failure that carries the unsent value. |
| `QueueReceive[T]` | Queue receive outcome. |
| `TaskResult[T]` | Task result outcome. |
| `WaitAny[T]` | `wait_any(...)` outcome. |
| `WaitAll[T]` | `wait_all(...)` outcome. |

## Resource And Module Types

These types are provided by builtin modules and are reserved names.

| Module | Types |
| --- | --- |
| `io` | `io.Error` |
| `fs` | `fs.File` |
| `net` | `net.TcpListener`, `net.TcpStream`, `net.UdpSocket`, `net.UdpDatagram`, `net.HttpListener`, `net.HttpExchange`, `net.HttpResponse`, `net.WebSocketListener`, `net.WebSocket`, `net.UnixListener`, `net.UnixStream`, `net.TlsListener`, `net.TlsStream` |
| `process` | `process.Child`, `process.Pipe`, `process.Completed`, `process.Supervisor`, `process.ExitStatus`, `process.Wait`, `process.Stdio`, `process.Error`, `process.RestartPolicy`, `process.SupervisorEvent`, `process.SupervisorWait` |

Resource types should usually be scoped with `with` or closed explicitly.

## Type Annotations

Simple annotations:

```python
count: int32 = 0
name: String = "aurora"
```

Collection annotations:

```python
names: Vec[String] = []
lookup: Map[String, int32] = {}
seen: Set[int32] = {}
```

Empty collection literals need an expected type. Constructors are also available:

```python
names = Vec[String]()
lookup = Map[String, int32]()
seen = Set[int32]()
```

## Option And Result Types

Construct `Option` and `Result` with their enum names:

```python
maybe: Option[String] = Option.Some("name")
missing: Option[String] = Option.None

result: Result[int32, String] = Result.Ok(42)
failure: Result[int32, String] = Result.Err("bad number")
```

Pattern matching may use qualified or short-form variants when the type is known:

```python
match result:
    case Result.Ok(value):
        print(value)
    case Result.Err(message):
        print(message)
```

## User Types

Classes create product types:

```python
class Point:
    x: float64
    y: float64
```

Enums create sum types:

```python
enum Load[T]:
    Ready(value: T)
    Empty
    Failed(message: String)
```

Traits define shared behavior:

```python
trait Named:
    def name(borrow self) -> String
```

## Recursive Fields

Direct recursive fields are not implemented. Use `indirect` for recursive class fields:

```python
class Node:
    value: int32
    next: indirect Option[Node] = Option.None
```

`indirect` gives the recursive field a level of indirection so the value has a finite size.

## Casts

Numeric casts are supported where the compiler can enforce the current conversion rules. Non-numeric casts are not implemented.

Use parsing functions for text-to-number conversion:

```python
def parse_answer() -> Result[int32, String]:
    value = try parse_int32("42")
    return Result.Ok(value)
```
