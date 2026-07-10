# I/O Module

The `io` module covers standard input/output and the common error enum shared by filesystem and networking APIs.

```python
import io
```

The top-level `print(value)` builtin is separate. It renders a value, writes a newline, and is meant for simple line output. Use `io.write(...)` and `io.flush()` when you need prompt-style or protocol-style control.

## Standard Streams

| API | Signature | Contract |
| --- | --- | --- |
| `io.write` | `write(text: String) -> Result[None, io.Error]` | Writes `text` to standard output without adding a newline. |
| `io.flush` | `flush() -> Result[None, io.Error]` | Flushes standard output. |
| `io.read_line` | `read_line() -> Result[Option[String], io.Error]` | Reads one line from standard input. Returns `Ok(None)` on EOF. |

Example:

```python
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
| `Other(message: String)` | A remaining platform or runtime error with a message. |

## Matching Errors

Handle specific cases when the program has specific policy:

```python
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
