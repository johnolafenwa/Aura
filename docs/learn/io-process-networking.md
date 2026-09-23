# Talking To The World

This chapter covers the four built-in modules that reach outside the program:

- `io` for standard streams
- `fs` for files and directories
- `process` for subprocesses and supervisors
- `net` for sockets, HTTP, and WebSockets

The APIs in all four share a shape:

- An operation that can fail returns `Result`.
- A resource that the runtime cleans up belongs inside a `with` block.
- A wait that might block forever accepts a `timeout` argument and tells the caller when that timeout fires.

They all work with `match`, `try`, `with`, and `TaskGroup`.

## Files: Read, Parse, Report

The simplest filesystem calls do the whole job in one step:

```aura fragment
import fs

path = "tmp.txt"
try fs.write_string(path, "limit=42\n")

match fs.read_to_string(path):
    case Result.Ok(text):
        print(text.trim())
    case Result.Err(error):
        print(error)
```

`fs.read_to_string` and `fs.read_bytes` are capped at 256 MiB. The same cap applies to the remaining contents that `fs.File.read_all()` and `fs.File.read_bytes()` read. An accidental whole-file read of a very large log fails at the cap. Aura has no incremental file-read member, so a larger file needs a host helper or pre-splitting.

For more control, open the file:

```aura fragment
import fs
import io

def copy_text(source: str, dest: str) -> Result[None, io.Error]:
    with input = try fs.open(source):
        text = try input.read_all()

    with output = try fs.create(dest):
        try output.write_all(text)
        try output.flush()

    return Result.Ok(None)
```

`fs.File` is a resource. Put it in a `with` block and the compiler handles cleanup. The `with` block closes the file on both normal and error paths.

## Standard Streams

`print(value)` renders a value and adds a newline. The `io` module covers the rest: writing without a newline, flushing before a prompt, and reading a line from standard input.

```aura fragment
import io

try io.write("name> ")
try io.flush()

match io.read_line():
    case Result.Ok(str as line):
        print("hello " + line.trim())
    case Result.Ok(None):
        print("end of input")
    case Result.Err(error):
        print(error)
```

`io.read_line()` returns `Result[str | None, io.Error]`. The `Result` carries I/O failures. The payload is `None` at end of input. Because both cases are in the type, a caller can treat them differently:

- `Result.Ok(str as line)` selects a present line. It is a nested type pattern.
- `Result.Ok(None)` selects a clean end of input.

## Processes: No Shell By Default

`process.run` runs a subprocess from a list of arguments and returns a `process.Completed` record. No shell interprets the arguments, so they are not re-split and there are no quoting hazards.

```aura fragment
import process

completed = try process.run(command=["/bin/echo", "aura process"], stdout=process.pipe(), stderr=process.pipe(), timeout=1s, group=true)

try completed.check()
print(completed.stdout().trim())
```

The arguments in that call do the following:

- `stdout=process.pipe()` captures the child's output so the parent can read it.
- `stderr=process.pipe()` does the same for standard error.
- `group=true` puts the child in its own process group on Unix hosts. Termination then reaches the leader and all its descendants.

If you omit `timeout`, the call has no deadline. A negative Duration does not mean "no deadline". Invalid timeout or deadline values return `process.Error.Io(io.Error.InvalidInput)`.

When a child writes bytes that are not valid UTF-8, use `stdout_bytes()` and `stderr_bytes()`:

```aura fragment
bytes = completed.stdout_bytes()
print(bytes.len())
```

## Interacting With A Child

`process.start` returns a `process.Child` that you can talk to while it runs:

```aura fragment
import process

child = try process.start(command=["/bin/cat"], stdin=process.pipe(), stdout=process.pipe(), stderr=process.pipe(), group=true)

match own child.stdin():
    case process.Pipe as pipe:
        try pipe.write_all("hello\n")
        pipe.close()
    case None:
        print("stdin was not piped")

match own child.stdout():
    case process.Pipe as pipe:
        text = try pipe.read_all()
        print(text.trim())
    case None:
        print("stdout was not piped")

match child.wait(timeout=1s):
    case process.Wait.Exited(status):
        print(status)
    case process.Wait.TimedOut:
        child.kill()
    case process.Wait.Cancelled:
        child.terminate()
    case process.Wait.Failed(error):
        print(error)

child.close()
```

`child.stdin()`, `child.stdout()`, and `child.stderr()` return `process.Pipe | None`. That lets the program tell "the stream was not piped" apart from "the stream is available".

`match own` moves the pipe out of the result. The arm then holds an owned resource that it can write, read, and close. A bare `match` would bind only a shared view of the pipe.

## Supervisors

Use `process.supervisor` to manage several named subprocesses: start them, watch their lifetimes, and restart them under a policy.

```aura fragment
import process

with supervisor = process.supervisor():
    try supervisor.start(name="worker", command=["/bin/sleep", "1"], restart=process.RestartPolicy.Never, group=true)

    match supervisor.wait(timeout=2s):
        case process.SupervisorWait.Event(event):
            print(event)
        case process.SupervisorWait.TimedOut:
            print("no event")
        case process.SupervisorWait.Cancelled:
            print("cancelled")
```

- Names are unique within a supervisor. Starting a second child with the same name returns an error and keeps the existing child.
- Leaving the `with` block stops every child the supervisor still manages.

[A Supervised Process Runner](/learn/case-studies/process-supervisor) builds a complete example.

## Networking: TCP

Network APIs return `Result[..., io.Error]`. Waits accept `timeout=...`. Listeners, streams, and other resources belong in `with` blocks.

```aura fragment
import net

with listener = try net.listen("127.0.0.1:0"):
    address = try listener.local_addr()

    with stream = try net.connect_timeout(address, timeout=1s):
        try stream.write_all("ping\n", timeout=1s)
        try stream.shutdown_write()
```

### Blocking Lookups And The Worker Pool

Hostname lookup and blocking connect syscalls run on the generic blocking-I/O pool, so they do not freeze sibling Aura tasks. The `1s` timeout above is one shared budget for queue admission, DNS, and every candidate address.

Task-group cancellation stops the wait promptly:

- Before the pool accepts the job, cancellation prevents submission.
- After the pool accepts it, the host resolver usually cannot be interrupted. Aura discards its eventual result.

Two environment variables tune the pool:

| Variable | Value | When absent |
| --- | --- | --- |
| `AURA_BLOCKING_WORKERS` | An exact positive worker count. | `2..=8` workers derived from host parallelism, with fallback `4`. |
| `AURA_BLOCKING_QUEUE_CAPACITY` | A positive bound on accepted pending jobs. | Unbounded. |

Admission to a full queue is FIFO and scheduler-aware. A queue bound limits the accepted pending backlog, not the tasks waiting for admission. It cannot guarantee progress for unrelated blocking I/O while every worker stays stuck.

### Keep Sockets On Their Task

A live listener or stream is not `Transfer`, so a new task cannot capture it. The task that creates a listener keeps it and every stream it accepts. That task can call an ordinary helper to handle a connection:

```aura
import io
import net

def handle(stream: own net.TcpStream) -> Result[None, io.Error]:
    with conn = stream:
        match try conn.read_line(timeout=5s):
            case str as text:
                try conn.write_all(text, timeout=5s)
            case None:
                pass
    return Result.Ok(None)
```

`read_line` returns `Result[str | None, io.Error]` for the same reason `io.read_line` does: the client might close cleanly, and the program must decide what that means. `try` unwraps the `Result`. The `match` then selects either the present line or the `None` that marks end of input.

To run a server as a child task, let the child create the listener. A `Queue[str]` handle is a copy value, so it can cross the task boundary. The child uses it to publish its bound address to the parent. The live listener never leaves its owning task.

## HTTP And WebSockets

HTTP client helpers return `net.HttpResponse`:

```aura fragment
import net

headers: dict[str, str] = {}
response = try net.http_request_text_timeout(method="GET", url="http://127.0.0.1:8080/", body="", headers=headers, timeout=2s)

print(response.status())
```

HTTP servers call `net.http_listen` to create an `HttpListener`. Accepting a connection returns an `HttpExchange`, which carries the request data and the methods that send a response.

WebSocket APIs follow the same resource style: create or accept a socket, send and receive text or bytes, then close it. See [Network Module](/manual/network) for the full surface.

## The Common Shape

Most system-facing Aura code follows one outline:

```aura
import fs
import io

def load(path: str) -> Result[str, io.Error]:
    with file = try fs.open(path):
        text = try file.read_all()
        return Result.Ok(text)
```

- `import` the module.
- Call an API that returns `Result`.
- Use `try` when the caller should receive the failure.
- Use `match` when the current function makes a decision.
- Put resources in `with`.
- Pass a `timeout` to any wait that should not block forever.

Reference: [Filesystem Module](/manual/filesystem), [Process Module](/manual/process), [Network Module](/manual/network), [I/O Module](/manual/io).
