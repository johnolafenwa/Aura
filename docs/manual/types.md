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
| `int` | Alias for `int64`; it is not a distinct type. |
| `int8`, `int16`, `int32`, `int64`, `int128`, `intsize` | Signed integers. |
| `uint8`, `uint16`, `uint32`, `uint64`, `uint128`, `uintsize` | Unsigned integers. |
| `float32`, `float64` | Floating-point values. |
| `String` | Owned UTF-8 string; `len()` counts Unicode scalar values and `byte_len()` counts encoded bytes. |
| `str` | Compatibility spelling that canonicalizes to `String` in Aurora 0.1; it is not a distinct runtime view type. |
| `None` | Unit type and unit value. |
| `Duration` | Runtime duration used by sleeps, timeouts, and scheduling APIs. |
| `Range` | Integer range returned by `range(...)`. |

Integer bounds are exact:

| Type | Inclusive range |
| --- | --- |
| `int8` | -128 through 127 |
| `int16` | -32,768 through 32,767 |
| `int32` | -2,147,483,648 through 2,147,483,647 |
| `int64` | -9,223,372,036,854,775,808 through 9,223,372,036,854,775,807 |
| `int128` | -2^127 through 2^127 - 1 |
| `uint8` | 0 through 255 |
| `uint16` | 0 through 65,535 |
| `uint32` | 0 through 4,294,967,295 |
| `uint64` | 0 through 18,446,744,073,709,551,615 |
| `uint128` | 0 through 2^128 - 1 |
| `intsize` | host-pointer-width signed range |
| `uintsize` | host-pointer-width unsigned range |

`float32` and `float64` use IEEE-754 binary32 and binary64 representations. Literal lexing first requires a finite binary64 value; contextual `float32` conversion may round or overflow as recorded in [Current Limits](/manual/current-limits). Runtime operations may produce NaN, but Aurora 0.1 makes `/`, `//`, or `%` by a floating zero explicit runtime failures rather than producing infinity or NaN through those operators.

`int` is an alias for `int64`, so the two spellings have identical bounds, type identity, layout, and runtime behavior. An unsuffixed integer literal uses an expected integer type when one is available and otherwise defaults to `int64`.

The default does not widen explicitly typed APIs. Existing fixed `int32` contracts remain `int32`, including `main()` exit statuses, `range(...)` bounds and yielded values, collection lengths, Vec indexes, queue capacities, and byte-count parameters. A literal passed to one of those positions adopts the expected `int32` type and must fit it.

`Duration` stores a non-negative integral count of milliseconds representable by signed 128-bit storage. Literal units are normalized to milliseconds. `Range` contains `int32` start/end values and iterates from the start inclusive to the end exclusive.

Numeric literals are checked against the target type. Integer literals must fit the annotated integer type. Integer-to-float casts reject silent precision loss. Separately, every integer type provides `.to_float() -> float64`, which intentionally permits IEEE-754 round-to-nearest, ties-to-even conversion when an application wants to enter the floating domain.

Use `borrow String` for a shared string parameter. The spelling `str` is accepted for compatibility but currently lowers to the same canonical `String` type; code must not assume a separate slice layout or lifetime-bearing runtime representation.

`String.len() -> int32` scans the text and counts Unicode scalar values in
O(n). `String.byte_len() -> int32` reads the UTF-8 byte count in O(1). Aurora
0.1 has no distinct character type, integer String indexing, slicing,
`chars()`, `ord()`, or `chr()`. The iteration and conversion APIs are scheduled
for the Phase 3 control-plane surface; String slicing remains part of the Phase
7 slicing work.

## Copy And Move Categories

Copy values may be reused after assignment or by-value calls:

- numbers
- `bool`
- `Duration`
- `Queue[T]`
- `Task[T]`
- `copy class` values whose fields are all copyable
- user enum values when every declared payload type is statically copyable
- `Option[T]`, `Result[T, E]`, `SendError[T]`, and `QueueReceive[T]` when all payload types are copyable

Move values transfer ownership:

- `String`
- `Vec[T]`
- `Map[K, V]`
- `Set[T]`
- ordinary user classes
- user enum values with any move payload
- `Option`, `Result`, and related outcome values with move payloads
- `TaskGroup`
- file, process, supervisor, and network resources

Move values can still be shared through `borrow` and `borrow mut`, or duplicated explicitly through methods such as `.clone()` when the type supports cloning.

`Queue[T]` and `Task[T]` are copy handles to shared runtime state. Copying the handle does not copy queued values or task results; it gives another reference to the same queue or task.

`TaskResult[T]`, `WaitAny[T]`, and `WaitAll[T]` are treated as move outcome values even when `T` is copyable. `Range` is also not a general copy type in Aurora 0.1; use ranges directly in iteration rather than relying on duplication.

A generic user-enum payload whose declared type is an unconstrained type parameter is not assumed copyable, even when one later instantiation supplies a copy type.

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

`T?` is shorthand for `Option[T]`:

```python
name: String? = None
```

Type arguments are invariant, nonempty when brackets are present, and must exactly match the declared arity. Aurora does not implicitly convert `Vec[int32]` to `Vec[int64]` or treat structurally identical user classes as the same type.

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

Numeric casts use `value as NumericType`. Non-numeric casts are not implemented.

- integer-to-integer casts require the value to fit the target bounds
- integer-to-float casts require exact representability and reject silent precision loss; use integer `.to_float()` when a possibly rounded `float64` result is intended
- float-to-integer casts require a finite in-range value and truncate toward zero
- `float64` to `float32` rounds through the host `float32` representation
- `float32` to `float64` preserves the represented value

Casts are checked at runtime when the source value is not a compile-time literal. A failed cast is a runtime diagnostic, not `Result.Err`.

Use parsing functions for text-to-number conversion:

```python
def parse_answer() -> Result[int32, String]:
    value = try parse_int32("42")
    return Result.Ok(value)
```
