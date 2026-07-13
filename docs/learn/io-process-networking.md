# Talking To The World

Programs eventually need to speak to something outside themselves — a file, a subprocess, a socket, a supervised service. Aurora exposes that surface through four built-in modules: `io` for standard streams, `fs` for files and directories, `process` for subprocesses and supervisors, and `net` for sockets, HTTP, and WebSockets.

The APIs in these modules share a shape. Operations that can fail return `Result`. Resources cleaned up by the runtime are meant to live inside a `with` block. Waits that might block indefinitely accept a `timeout` argument and tell the caller explicitly when that timeout fires. Everything works together with `match`, `try`, `with`, and `TaskGroup`.

## Files: Read, Parse, Report

The simplest filesystem API is one-shot:

```python
import fs

path = "tmp.txt"
try fs.write_string(path, "limit=42\n")

match fs.read_to_string(path):
    case Result.Ok(text):
        print(text.trim())
    case Result.Err(error):
        print(error)
```

One-shot `fs.read_to_string` and `fs.read_bytes` are capped at 64 MiB. The cap is deliberate: an accidental "read the whole file" against a log that turned out to be gigabytes should fail loudly, not allocate gigabytes of memory. When a program genuinely needs to read larger files, open a handle.

```python
import fs
import io

def copy_text(source: String, dest: String) -> Result[None, io.Error]:
    with input = try fs.open(source):
        text = try input.read_all()

    with output = try fs.create(dest):
        try output.write_all(text)
        try output.flush()

    return Result.Ok(None)
```

`fs.File` is a resource. Put it in a `with` block and cleanup is the compiler's problem, not yours. The `with` ends automatically on both normal and error paths.

## Standard Streams

`print(value)` renders a value and adds a newline. When a program needs more control — writing without a newline, flushing for a prompt, reading a line from standard input — the `io` module has it:

```python
import io

try io.write("name> ")
try io.flush()

match io.read_line():
    case Result.Ok(Option.Some(line)):
        print("hello " + line.trim())
    case Result.Ok(Option.None):
        print("end of input")
    case Result.Err(error):
        print(error)
```

`io.read_line()` returns `Result[Option[String], io.Error]`. The `Option` is `None` at end of input; the `Result` captures I/O failures. Both are in the type, and a caller that wants to treat them differently can.

## Processes: No Shell By Default

`process.run` executes a subprocess from an argv vector — there is no shell interpretation, so the arguments are not re-split and there are no quoting hazards. The return value is a `process.Completed` record.

```python
import process

completed = try process.run(command=["/bin/echo", "aurora process"], stdout=process.pipe(), stderr=process.pipe(), timeout=1s, group=true)

try completed.check()
print(completed.stdout().trim())
```

Two things in that call site are worth explaining. `stdout=process.pipe()` captures the subprocess's output so the parent can read it; `stderr=process.pipe()` does the same for standard error. `group=true` places the child in its own process group on Unix hosts, which makes cleanup more reliable when a child spawns descendants — termination can target the whole group rather than only the leader.

When a child writes bytes that are not valid UTF-8, use `stdout_bytes()` and `stderr_bytes()`:

```python
bytes = completed.stdout_bytes()
print(bytes.len())
```

## Interacting With A Child

`process.start` returns a `process.Child` you can talk to while the child is running:

```python
import process

child = try process.start(command=["/bin/cat"], stdin=process.pipe(), stdout=process.pipe(), stderr=process.pipe(), group=true)

match child.stdin():
    case Option.Some(pipe):
        try pipe.write_all("hello\n")
        pipe.close()
    case Option.None:
        print("stdin was not piped")

match child.stdout():
    case Option.Some(pipe):
        text = try pipe.read_all()
        print(text.trim())
    case Option.None:
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

`child.stdin()`, `child.stdout()`, and `child.stderr()` return `Option[process.Pipe]` so the program can tell the difference between "the stream was not piped" and "the stream is available."

## Supervisors

When a program needs to manage several named subprocesses — start them, observe their lifetimes, restart them according to a policy — use a `process.supervisor`:

```python
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

Supervisor names are unique within a supervisor; starting a second child with the same name returns an error instead of silently replacing the first. Leaving the `with` block stops every child the supervisor still manages.

## Networking: TCP

Network APIs return `Result[..., io.Error]`. Waits accept `timeout=...`. Listeners, streams, and other resources belong in `with` blocks.

```python
import net

with listener = try net.listen("127.0.0.1:0"):
    address = try listener.local_addr()

    with stream = try net.connect_timeout(address, timeout=1s):
        try stream.write_all("ping\n", timeout=1s)
        try stream.shutdown_write()
```

Hostname lookup and blocking connect syscalls are sent to the bounded blocking service, so they do not freeze sibling Aurora tasks. The `1s` timeout above is a single budget for DNS and all candidate addresses rather than a fresh second for every address. Task-group cancellation stops waiting promptly even when the host resolver itself cannot be interrupted.

A server usually has its listener at the top of a scope and each accepted connection running in its own task:

```python
import io
import net

def handle(stream: net.TcpStream) -> Result[None, io.Error]:
    with conn = stream:
        line = try conn.read_line(timeout=5s)
        match line:
            case Option.Some(text):
                try conn.write_all(text, timeout=5s)
            case Option.None:
                pass
    return Result.Ok(None)
```

The `read_line` returns `Result[Option[String], io.Error]` for the same reason `io.read_line` does: the client might close cleanly, and the program might have to decide what that means.

## HTTP And WebSockets

HTTP client helpers return `net.HttpResponse`:

```python
import net

headers: Map[String, String] = {}
response = try net.http_request_text_timeout(method="GET", url="http://127.0.0.1:8080/", body="", headers=headers, timeout=2s)

print(response.status())
```

HTTP servers use `net.http_listen` to create an `HttpListener`; accepting a connection returns an `HttpExchange` carrying request data and the methods to send a response.

WebSocket APIs follow the same resource style: create or accept a socket, send and receive text or bytes, then close. See [Network Module](/manual/network) for the full surface.

## The Common Shape

Most system-facing Aurora code has the same outline:

```python
import fs
import io

def load(path: String) -> Result[String, io.Error]:
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
