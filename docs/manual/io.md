# I/O Module

The `io` module covers standard input/output and the common error enum shared by filesystem and networking APIs.

```aura
import io
```

The top-level `print(value)` builtin is separate. It renders a value, writes a newline, and is meant for simple line output. `float32` and `float64` values use their own shortest round-trip decimal spelling, including a decimal marker for integral values and preservation of `-0.0`. Use `io.write(...)` and `io.flush()` when you need prompt-style or protocol-style control.

## Standard Streams

| API | Signature | Contract |
| --- | --- | --- |
| `io.write` | `write(text: str) -> Result[None, io.Error]` | Writes `text` to standard output without adding a newline. |
| `io.flush` | `flush() -> Result[None, io.Error]` | Flushes standard output. |
| `io.read_line` | `read_line() -> Result[Option[str], io.Error]` | Reads one strict UTF-8 line from standard input, removes trailing LF/CRLF, and returns `Ok(None)` on EOF. |

Example:

```aura
import io

def prompt() -> Result[None, io.Error]:
    try io.write("name> ")
    try io.flush()
    return Result.Ok(None)

match io.read_line():
    case Result.Ok(Option.Some(line)):
        print("hello " + line.trim())
    case Result.Ok(Option.None):
        print("no input")
    case Result.Err(error):
        print(error)
```

## io.Error

`io.Error` is used by `io`, `fs`, and `net`. It is also wrapped by `process.Error.Io(...)` when a subprocess operation fails because of an I/O condition.

| Variant | Meaning |
| --- | --- |
| `NotFound` | A file, directory, socket path, or other target was not found. |
| `PermissionDenied` | The operating system denied access. |
| `AlreadyExists` | Creation failed because the target already exists. |
| `IsDirectory` | A file operation was attempted on a directory. |
| `ConnectionRefused` | A peer refused the connection. |
| `ConnectionReset` | A connection was reset by the peer. |
| `ConnectionAborted` | A connection was aborted. |
| `NotConnected` | An operation requires a connected stream or socket. |
| `AddrInUse` | A local address is already bound. |
| `AddrNotAvailable` | The requested address is not available. |
| `BrokenPipe` | The write side was closed by the peer. |
| `TimedOut` | The operation timed out. |
| `WouldBlock` | The operation would block in a non-blocking context. |
| `UnexpectedEof` | The stream ended before the requested data was read. |
| `InvalidInput` | The caller supplied invalid input, such as a negative byte count. |
| `InvalidData` | Data could not be decoded or was malformed for the operation. |
| `Closed` | The resource was already closed or closed while waiting. |
| `Cancelled` | Cancellation interrupted the operation. |
| `Other(message: own str)` | A remaining platform or runtime error with a message. |

## Matching Errors

Handle specific cases when the program has specific policy:

```aura
match io.read_line():
    case Result.Ok(Option.Some(line)):
        print(line)
    case Result.Ok(Option.None):
        print("end of input")
    case Result.Err(io.Error.InvalidData):
        print("input was not valid text")
    case Result.Err(error):
        print(error)
```

Avoid turning `io.Error` into a string too early. Error variants carry useful control flow.

## Grammar

The `io` module and top-level `print` builtin add no source-language grammar. They use ordinary imports, calls, `Result`, `Option`, `try`, and pattern matching. `io.Error.Other(message: own str)` uses the normal owned enum-payload rule; the other variants carry no payload.

Line endings are runtime input, not token syntax. `io.read_line()` removes one trailing LF or CRLF sequence from the returned line; it does not strip other whitespace.

## Typing Rules

`print(value)` accepts one value and returns `None`. `io.write` accepts `str`; `io.flush` accepts no arguments; both return `Result[None, io.Error]`. `io.read_line` returns `Result[Option[str], io.Error]`, distinguishing a line, clean EOF, and an I/O failure.

The `io.Error` variants in the table above are the common typed failure vocabulary for `io`, `fs`, and `net`; `process.Error.Io` owns an `io.Error` payload. Exhaustiveness and payload ownership follow the ordinary enum and match rules.

## Runtime Semantics

`print` renders its value, writes the rendered text, and appends a newline. Floating-point rendering uses the type-specific shortest finite decimal that round-trips to the same `float32` or `float64` value, retains a decimal marker for integral values, and preserves negative zero. `io.write` adds no newline, and `io.flush` requests that buffered standard output be delivered to the host.

`io.read_line` reads from process standard input as strict UTF-8. It returns `Ok(Some(line))` after removing LF or CRLF, `Ok(None)` only when EOF is reached before any bytes are read, and `Err(io.Error.InvalidData)` for invalid text. Other host failures map to the closest `io.Error` variant.

## Ownership And Evaluation Order

The argument to `print` or `io.write` is evaluated before output occurs. The write call shares its `str` for the duration of the operation and does not retain it. A successfully read line and every payload-bearing error are fresh owned values returned to the caller.

Standard input and output are process-global resources. Output calls are observable in source evaluation order within one task, but ordering between concurrent tasks follows scheduling. A successful write or read is not rolled back if later evaluation fails. Pattern matching can move the owned message from `io.Error.Other`; matching payload-free variants introduces no owned payload.

## Diagnostics

Unknown I/O members use `AU2001`, wrong types use `AU2002`, invalid argument binding uses `AU2004`, and remaining static rejections use `AU2999`. The documented stream failures are typed `Result.Err(io.Error)` values, not language diagnostics. Invalid UTF-8 therefore produces `io.Error.InvalidData`, and a broken stream produces the applicable error variant.

An uncaught failure outside the typed stream boundary uses the general runtime categories, including `AU4005` for a resource or I/O trap. The `aura` CLI treats its own broken output pipe as clean termination so compiler commands compose with pipe consumers; that tooling policy does not change an Aura program's `io.write` return type.

## Backend Support

`print`, all three `io` functions, and every `io.Error` variant are supported by the MIR runtime and direct native backend. Text decoding, line-ending removal, EOF distinction, floating-point spelling, and error mapping are backend-parity contracts.

The actual standard streams are supplied by the host process. A backend may buffer them differently, but `io.flush` and each documented typed outcome must remain observable as specified.

## Limits And Implementation-Defined Behavior

Aura 0.3 exposes line-oriented text input only; it has no standard-input byte API, terminal mode API, stream replacement API, asynchronous console API, or built-in formatted-output language. `io.read_line` has no separate Aura line-length cap and therefore allocates according to the incoming line and host memory limits.

Terminal encoding before bytes reach the process, host pipe buffering, scheduling between concurrent writers, and the precise message stored in `io.Error.Other` are host-dependent. Stable control flow should match the specific non-message variants where possible.

## Status

The standard-stream functions, `print` behavior, `io.Error` enum, strict UTF-8 policy, EOF distinction, and shortest-roundtrip float rendering are implemented and maintained in Aura 0.3. No I/O semantics on this page are provisional.

Aura 0.3 has no binary standard input, async stream handles, terminal control,
configurable formatting, or user-defined error derivation.
