# I/O Module

The `io` module covers standard input and output. It also defines `io.Error`, the error enum that the filesystem and networking APIs share.

```aura
import io
```

The top-level `print(value)` builtin is not part of `io`. It renders a value and writes a newline, for simple line output. `float32` and `float64` values print in their own shortest decimal spelling that round-trips. That spelling keeps a decimal marker on integral values and preserves `-0.0`. Use `io.write(...)` and `io.flush()` when you need control over output, such as for a prompt or a protocol.

## Standard Streams

| API | Signature | Contract |
| --- | --- | --- |
| `io.write` | `write(text: str) -> Result[None, io.Error]` | Writes `text` to standard output. Adds no newline. |
| `io.flush` | `flush() -> Result[None, io.Error]` | Flushes standard output. |
| `io.read_line` | `read_line() -> Result[str \| None, io.Error]` | Reads one strict UTF-8 line from standard input and removes a trailing LF or CRLF. Returns `Ok(None)` at EOF. |

Example:

```aura
import io

def prompt() -> Result[None, io.Error]:
    try io.write("name> ")
    try io.flush()
    return Result.Ok(None)

match io.read_line():
    case Result.Ok(str as line):
        print("hello " + line.trim())
    case Result.Ok(None):
        print("no input")
    case Result.Err(error):
        print(error)
```

## io.Error

`io`, `fs`, and `net` all report failures as `io.Error`. When a subprocess operation fails because of an I/O condition, `process.Error.Io(...)` wraps an `io.Error`.

| Variant | Meaning |
| --- | --- |
| `NotFound` | A file, directory, socket path, or other target was not found. |
| `PermissionDenied` | The operating system denied access. |
| `AlreadyExists` | Creation failed because the target already exists. |
| `IsDirectory` | A file operation was attempted on a directory. |
| `ConnectionRefused` | A peer refused the connection. |
| `ConnectionReset` | The peer reset the connection. |
| `ConnectionAborted` | The connection was aborted. |
| `NotConnected` | The operation needs a connected stream or socket. |
| `AddrInUse` | A local address is already bound. |
| `AddrNotAvailable` | The requested address is not available. |
| `BrokenPipe` | The peer closed the write side. |
| `TimedOut` | The operation timed out. |
| `WouldBlock` | The operation would block in a non-blocking context. |
| `UnexpectedEof` | The stream ended before the requested data was read. |
| `InvalidInput` | The caller supplied invalid input, such as a negative byte count. |
| `InvalidData` | The data could not be decoded or was malformed for the operation. |
| `Closed` | The resource was already closed, or closed while the operation waited. |
| `Cancelled` | Cancellation interrupted the operation. |
| `Other(message: own str)` | Any other platform or runtime error, with a message. |

## Matching Errors

Match specific variants when the program has a specific policy for them:

```aura fragment
match io.read_line():
    case Result.Ok(str as line):
        print(line)
    case Result.Ok(None):
        print("end of input")
    case Result.Err(io.Error.InvalidData):
        print("input was not valid text")
    case Result.Err(error):
        print(error)
```

Keep the `io.Error` value instead of turning it into a string early. The variants carry information that control flow can use.

## Grammar

The `io` module and the `print` builtin add no grammar. They use ordinary imports, calls, `Result`, `T | None` unions, `try`, and pattern matching. `io.Error.Other(message: own str)` follows the normal rule for owned enum payloads. The other variants carry no payload.

Line endings are runtime input, not token syntax. `io.read_line()` removes one trailing LF or CRLF from the returned line. It does not strip any other whitespace.

## Typing Rules

- `print(value)` takes one value and returns `None`.
- `io.write` takes a `str` and returns `Result[None, io.Error]`.
- `io.flush` takes no arguments and returns `Result[None, io.Error]`.
- `io.read_line` returns `Result[str | None, io.Error]`. The type separates a line, a clean EOF, and an I/O failure.

The `io.Error` variants above are the shared typed failure set for `io`, `fs`, and `net`. `process.Error.Io` owns an `io.Error` payload. Exhaustiveness and payload ownership follow the ordinary enum and `match` rules.

## Runtime Semantics

`print` renders its value, writes the text, and appends a newline. A floating-point value renders as the shortest finite decimal that round-trips to the same `float32` or `float64` value. The rendering keeps a decimal marker on integral values and preserves negative zero.

`io.write` adds no newline. `io.flush` asks for buffered standard output to be delivered to the host.

`io.read_line` reads process standard input as strict UTF-8:

- It returns `Ok(line)` after removing LF or CRLF.
- It returns `Ok(None)` only when EOF arrives before any bytes are read.
- It returns `Err(io.Error.InvalidData)` for invalid text.
- Other host failures map to the closest `io.Error` variant.

## Ownership And Evaluation Order

The argument to `print` or `io.write` is evaluated before any output happens. The write call shares its `str` for the duration of the operation and does not keep it. A line that is read, and every error that carries a payload, is a fresh owned value returned to the caller.

Standard input and output are process-global resources. Within one task, output appears in source evaluation order. Between concurrent tasks, the order follows scheduling. A completed write or read is not rolled back if later evaluation fails.

Pattern matching can move the owned message out of `io.Error.Other`. Matching a variant with no payload introduces no owned value.

## Diagnostics

| Code | Cause |
| --- | --- |
| `AU2001` | Unknown I/O member. |
| `AU2002` | Wrong type. |
| `AU2004` | Invalid argument binding. |
| `AU2999` | Any other static rejection. |

The stream failures on this page are typed `Result.Err(io.Error)` values, not language diagnostics. Invalid UTF-8 produces `io.Error.InvalidData`. A broken stream produces the matching error variant.

An uncaught failure outside the typed stream boundary uses the general runtime categories. These include `AU4005` for a resource or I/O trap.

The `aura` CLI treats a broken pipe on its own output as a clean exit, so compiler commands work with pipe consumers. That tooling policy does not change the return type of `io.write` in an Aura program.

## Backend Support

The MIR runtime and the direct native backend both support `print`, all three `io` functions, and every `io.Error` variant. MIR is the compiler's mid-level intermediate representation. Both backends must match on text decoding, line-ending removal, the EOF distinction, floating-point spelling, and error mapping.

The host process supplies the actual standard streams. A backend may buffer them differently. `io.flush` and each typed outcome on this page must still behave as specified.

## Limits And Implementation-Defined Behavior

Aura 0.3 offers line-oriented text input only. It has no standard-input byte API, terminal mode API, stream replacement API, asynchronous console API, or built-in formatted-output language.

`io.read_line` has no Aura line-length cap of its own. It allocates for the incoming line, up to host memory limits.

The host determines:

- terminal encoding before bytes reach the process
- pipe buffering
- scheduling between concurrent writers
- the exact message in `io.Error.Other`

For stable control flow, match the specific variants that carry no message where you can.

## Status

Aura 0.3 implements the standard-stream functions, `print` behavior, the `io.Error` enum, the strict UTF-8 policy, the EOF distinction, and shortest round-trip float rendering. None of the I/O semantics on this page is provisional.

Aura 0.3 has no binary standard input, async stream handles, terminal control, configurable formatting, or user-defined error derivation.
