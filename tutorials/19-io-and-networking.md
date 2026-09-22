# I/O And Networking

This chapter covers terminal, file, process, and network I/O. Four builtin
modules provide it:

- `io`
- `fs`
- `net`
- `process`

Import them like ordinary namespaces:

```aura check-pass
import io
import fs
import net
import process
```

Aura's runtime uses scheduler-backed lightweight tasks. A task that waits on a
queue, a timer, or a socket parks until its event is ready.
[Current Model](#current-model) at the end of the chapter explains the details.

## Standard Input And Output

Use `io.write(...)`, `io.flush()`, and `io.read_line()` for explicit terminal
I/O:

```aura check-pass
import io

def main() -> int32:
    match io.write("name> "):
        case Result.Ok(_):
            pass
        case Result.Err(_):
            return 1

    match io.flush():
        case Result.Ok(_):
            pass
        case Result.Err(_):
            return 1

    match io.read_line():
        case Result.Ok(str as line):
            print(line)
            return 0
        case Result.Ok(None):
            return 0
        case Result.Err(_):
            return 1
```

`io.read_line()` returns `Result[str | None, io.Error]`:

- `Result.Ok(text)` when it read a line. An empty line is a present `""`.
- `Result.Ok(None)` at end-of-file.
- `Result.Err(...)` on an I/O failure.

## File I/O

The `fs` module has one-shot helpers and scoped file handles.

The one-shot helpers work on text, bytes, and directories:

- `fs.exists(path)`
- `fs.read_to_string(path)`
- `fs.read_bytes(path)`
- `fs.write_string(path, text)`
- `fs.write_bytes(path, bytes)`
- `fs.append_string(path, text)`
- `fs.append_bytes(path, bytes)`
- `fs.create_dir(path)`
- `fs.read_dir(path)`
- `fs.remove_file(path)`

For a scoped handle, call one of these constructors:

- `fs.open(path)`
- `fs.create(path)`
- `fs.append(path)`

Each returns `Result[fs.File, io.Error]`. An `fs.File` works with `with` and
has these members:

- `read_all()`
- `read_bytes()`
- `write_all(text)`
- `write_bytes(bytes)`
- `flush()`
- `close()`

This function reads a whole file as text:

```aura check-pass
import fs
import io

def load_text(path: str) -> Result[str, io.Error]:
    with file = try fs.open(path):
        return file.read_all()
```

This one reads it as raw bytes:

```aura check-pass
import fs
import io

def copy_bytes(path: str) -> Result[list[uint8], io.Error]:
    with file = try fs.open(path):
        bytes = try file.read_bytes()
        return Result.Ok(bytes)
```

Whole-file reads have a size cap. The one-shot helpers and `fs.File`
whole-file reads accept at most 256 MiB of remaining content, in both
`aura run` and built binaries. Aura 0.3 has no incremental file-read member.
For a larger file, use a host helper or split the file first.

Conversion between raw data and UTF-8 text is always explicit. Use
`text.to_bytes()` or `str.from_bytes(payload)`. [22-bytes.md](22-bytes.md)
covers those conversions, plus strict hex and base64 codecs and SHA-256.

See:

- [examples/io/read_text_file.au](../examples/io/read_text_file.au)
- [examples/io/bytes_file_io.au](../examples/io/bytes_file_io.au)

## Processes

The `process` module starts subprocesses without a shell. The `command`
argument is always an explicit `list[str]` argv list. Aura has no
shell-string subprocess API.

These are the constructors:

- `process.supervisor()`
- `process.start(command, cwd=..., env=..., stdin=..., stdout=..., stderr=..., group=false)`
- `process.run(command, cwd=..., env=..., stdin=..., stdout=..., stderr=..., timeout=..., group=false)`
- `process.inherit()`
- `process.null()`
- `process.pipe()`

Use `process.pipe()` to capture a child's stdin, stdout, or stderr stream.

`group=true` starts the child in its own process group. Aura then applies
terminate, kill, and close cleanup to the whole group. On current maintained
hosts, grouped children are supported on Unix.

Each process type has these members:

| Type | Returned by | Members |
| --- | --- | --- |
| `process.Child` | `process.start(...)`, as `Result[process.Child, process.Error]` | `stdin()`, `stdout()`, `stderr()`, `wait(timeout=...)`, `wait_or_none(timeout=...)`, `wait_ok(timeout=...)`, `kill()`, `terminate()`, `close()` |
| `process.Pipe` | a child's captured stream | `read_all()`, `read_line(timeout=...)`, `read_bytes(max_bytes, timeout=...)`, `write_all(text, timeout=...)`, `write_bytes(bytes, timeout=...)`, `flush()`, `close()` |
| `process.Completed` | `process.run(...)`, as `Result[process.Completed, process.Error]` | `status()`, `success()`, `stdout()` for UTF-8 text, `stderr()` for UTF-8 text, `stdout_bytes()` for raw bytes, `stderr_bytes()` for raw bytes, `check()` |
| `process.Supervisor` | `process.supervisor()` | `start(name, command, cwd=..., env=..., stdin=..., stdout=..., stderr=..., restart=..., backoff=..., max_restarts=..., group=true)`, `wait(timeout=...)`, `wait_or_none(timeout=...)`, `stop()`, `is_empty()`, `close()` |

`process.Child`, `process.Pipe`, and `process.Supervisor` all work with
`with`.

`process.Child.close()` is built for cleanup. It sends a graceful terminate
signal, waits briefly, and escalates to kill if the child does not exit
promptly. For a grouped child, it waits until the whole process group is gone
before it returns.

There is no PTY surface and there are no pipeline helpers.

### Running A Command To Completion

`process.run(...)` starts a command, waits for it, and collects its output:

```aura check-pass
import process

def run_echo() -> Result[None, process.Error]:
    completed = try process.run(["/bin/echo", "aura process"], stdout=process.pipe(), stderr=process.pipe(), timeout=1s, group=true)
    try completed.check()
    print(completed.stdout().trim())
    print(completed.stdout_bytes().len())
    return Result.Ok(None)
```

### Talking To A Child Through Pipes

`process.start(...)` returns a live child. Take its pipes with `match own` to
write to it and read from it:

```aura check-pass
import process

def roundtrip() -> Result[None, process.Error]:
    with child = try process.start(["/bin/cat"], stdin=process.pipe(), stdout=process.pipe(), stderr=process.null(), group=true):
        match own child.stdin():
            case process.Pipe as stdin_pipe:
                try stdin_pipe.write_all("ping\n", timeout=500ms)
                try stdin_pipe.flush()
                stdin_pipe.close()
            case None:
                pass

        match own child.stdout():
            case process.Pipe as stdout_pipe:
                match try stdout_pipe.read_line(timeout=500ms):
                    case str as text:
                        print(text.trim())
                    case None:
                        pass
            case None:
                pass
        print(try child.wait_ok(timeout=2s))
        return Result.Ok(None)
```

### Supervising Children

A supervisor starts named children and can restart them when they exit. Three
enums go with it:

- `process.RestartPolicy`
- `process.SupervisorEvent`
- `process.SupervisorWait`

```aura check-pass
import process

def supervise() -> Result[None, process.Error]:
    with supervisor = process.supervisor():
        try supervisor.start(name="flaky", command=["/usr/bin/false"], restart=process.RestartPolicy.OnFailure, backoff=10ms, max_restarts=1, group=true)
        print(try supervisor.wait_or_none(timeout=500ms))
        print(try supervisor.wait_or_none(timeout=500ms))
        print(supervisor.is_empty())

        try supervisor.start(name="sleeper", command=["/bin/sleep", "1"], restart=process.RestartPolicy.Never, group=true)
        print(supervisor.is_empty())
        try supervisor.stop()
        print(supervisor.is_empty())
        return Result.Ok(None)
```

Supervisor children default to `group=true`. So `stop()` and `close()` shut
down the leader process and its whole child tree.

When `restart` is `process.RestartPolicy.OnFailure` or
`process.RestartPolicy.Always`, `backoff` must be at least `10ms`. This
prevents zero-delay restart loops.

`Supervisor.start` keeps the configuration it may need for a restart. So every
configuration slot is an explicit `own` parameter. That includes the
copy-valued restart, backoff, count, and group settings. `own` is harmless for
copy values and keeps the rule the same for every slot. A move value's
ownership transfers into the call. If the caller also needs an independent
copy, clone a clone-safe value before the call.

See:

- [examples/io/process_run.au](../examples/io/process_run.au)
- [examples/io/process_pipes.au](../examples/io/process_pipes.au)
- [examples/io/process_supervisor.au](../examples/io/process_supervisor.au)

## TCP

The `net` module provides TCP clients and listeners on nonblocking sockets:

- `net.connect(address)`
- `net.connect_timeout(address, timeout)`
- `net.listen(address)`

| Type | Members |
| --- | --- |
| `net.TcpListener` | `accept(timeout=...)`, `local_addr()`, `close()` |
| `net.TcpStream` | `read_all(timeout=...)`, `read_line(timeout=...)`, `read_bytes(max_bytes, timeout=...)`, `read_exact(count, timeout=...)`, `write_all(text, timeout=...)`, `write_bytes(bytes, timeout=...)`, `flush()`, `local_addr()`, `peer_addr()`, `shutdown_read()`, `shutdown_write()`, `shutdown_both()`, `close()` |

Both types work with `with`.

This worker accepts one connection and echoes one line:

```aura check-pass
import io
import net

def serve(addresses: Queue[str]) -> Result[None, io.Error]:
    with server = try net.listen("127.0.0.1:0"):
        addresses.put(try server.local_addr())
        with stream = try server.accept(timeout=1s):
            match try stream.read_line(timeout=1s):
                case str as text:
                    try stream.write_all("echo:" + text, timeout=1s)
                    try stream.flush()
                case None:
                    pass
        return Result.Ok(None)
```

Listeners and other live network resources are not `Transfer`, so they cannot
cross a task boundary. The worker task therefore creates and owns its
listener. The queue handle is a copy value, so it can cross. After binding,
the worker sends the listener's owned `str` address back through it.

See:

- [examples/io/tcp_echo.au](../examples/io/tcp_echo.au)
- [examples/io/tcp_bytes.au](../examples/io/tcp_bytes.au)

## UDP

UDP sockets run on the same poll-driven runtime. Create one with:

- `net.udp_bind(address)`

| Type | Members |
| --- | --- |
| `net.UdpSocket` | `send_text(address, text, timeout=...)`, `send_bytes(address, bytes, timeout=...)`, `recv(max_bytes, timeout=...)`, `recv_from(max_bytes, timeout=...)`, `local_addr()`, `peer_addr()`, `close()` |
| `net.UdpDatagram` | `address()`, `bytes()`, `text()` |

`recv_from(...)` returns `Result[net.UdpDatagram | None, io.Error]`. It
returns `Ok(None)` when the deadline expires.

See [examples/io/udp_echo.au](../examples/io/udp_echo.au).

## HTTP

The HTTP helpers are:

- `net.http_listen(address)`
- `net.http_request_text(method, url, body, headers)`
- `net.http_request_text_timeout(method, url, body, headers, timeout)`
- `net.http_request_bytes(method, url, bytes, headers)`
- `net.http_request_bytes_timeout(method, url, bytes, headers, timeout)`

| Type | Members |
| --- | --- |
| `net.HttpListener` | `accept(timeout=...)`, `local_addr()`, `close()` |
| `net.HttpExchange` | `method()`, `path()`, `headers()`, `body_text()`, `body_bytes()`, `respond_text(status, text, headers)`, `respond_bytes(status, bytes, headers)` |
| `net.HttpResponse` | `status()`, `reason()`, `headers()`, `text()`, `bytes()` |

See [examples/io/http_roundtrip.au](../examples/io/http_roundtrip.au).

### Application-Level Retries

Each HTTP helper performs one request. To retry, use the generic
`control.retry` helper. It repeats a capture-free `def() -> Result[T, E]`
worker when every `Err` is retryable. It uses a fixed attempt budget and
exponential `Duration` backoff. Your application decides which HTTP statuses
to retry, and handles jitter and any richer policy.

The maintained [retrying network worker](../examples/agents/retrying_network_worker.au)
shows one such policy:

- It retries only `503`.
- It returns other statuses, such as `429`, unchanged.
- It returns the last `503` when its attempt budget runs out.
- It uses `random.Rng(42)` for deterministic jitter.
- It doubles a `Duration` backoff after each retry.
- It checks the final-attempt guard before the RNG draw, the retry log, and
  `sleep(...)`.

The example makes seven real requests against an ephemeral loopback listener.
It sets explicit five-second deadlines on listener acceptance, HTTP requests,
and task results. `TaskGroup` and `with` scopes close the worker, server,
listener, exchanges, and responses deterministically. A CLI conformance test
runs the exact trace on both the MIR backend and the forced direct backend.

## WebSockets

The WebSocket constructors are:

- `net.websocket_listen(address)`
- `net.websocket_connect(url)`
- `net.websocket_connect_timeout(url, timeout)`

| Type | Members |
| --- | --- |
| `net.WebSocketListener` | `accept(timeout=...)`, `local_addr()`, `close()` |
| `net.WebSocket` | `send_text(text, timeout=...)`, `send_bytes(bytes, timeout=...)`, `recv_text(timeout=...)`, `recv_bytes(timeout=...)`, `close()` |

See [examples/io/websocket_roundtrip.au](../examples/io/websocket_roundtrip.au).

## Unix Sockets And TLS

Unix domain stream sockets and TLS streams also run on nonblocking sockets.
Unix domain sockets need a Unix host at runtime.

The Unix-socket constructors are:

- `net.unix_listen(path)`
- `net.unix_connect(path)`
- `net.unix_connect_timeout(path, timeout)`

The TLS constructors are:

- `net.tls_listen(address, cert_pem_path, key_pem_path)`
- `net.tls_connect(address, server_name, ca_pem_path)`
- `net.tls_connect_timeout(address, server_name, ca_pem_path, timeout)`

| Type | Members |
| --- | --- |
| `net.UnixListener` | `accept(timeout=...)`, `close()` |
| `net.UnixStream` | `read_line(timeout=...)`, `read_exact(count, timeout=...)`, `write_all(text, timeout=...)`, `close()` |
| `net.TlsListener` | `accept(timeout=...)`, `local_addr()`, `close()` |
| `net.TlsStream` | `read_line(timeout=...)`, `read_exact(count, timeout=...)`, `write_all(text, timeout=...)`, `close()` |

[examples/io/unix_tls_roundtrip.au](../examples/io/unix_tls_roundtrip.au)
embeds a self-signed certificate, so it runs without extra setup.

## Timeouts And Cancellation

Most socket operations take an optional `timeout=...` argument. A timeout is
an Aura `Duration` value such as `100ms`, `1s`, or `2m`. For a computed
timeout, use `Duration.ms(n)` or arithmetic such as `attempt * 1ms`.

An explicit timeout must be non-negative and must fit the host deadline.
Otherwise the call returns `io.Error.InvalidInput`. Only an omitted timeout is
unlimited.

`process.run(...)` follows the same rule and reports an invalid timeout as
`process.Error.Io(io.Error.InvalidInput)`. Aura tracks an omitted timeout
separately, so an explicit negative Duration never counts as omitted.

A connect operation has one timeout budget. That budget covers admission to
the blocking-I/O pool, hostname resolution, every resolved-address attempt,
and the rest of the protocol handshake. Aura does not restart the timeout for
each address that DNS returns.

Cancellation or expiry before the pool accepts the job prevents submission.
After the pool accepts it, cancellation stops the Aura task's wait. The host
resolver or connect syscall may still finish later, and Aura discards its
result safely.

Task-group cancellation reaches socket waits too. If a task group is cancelled
while a child waits on a network operation, that operation returns
`io.Error.Cancelled` promptly.

## Current Model

All of this I/O uses one scheduler-backed model:

- Aura tasks are scheduler-backed lightweight coroutines. A task does not need
  its own OS thread.
- Queue waits, `sleep(...)`, socket waits, and the HTTP helpers all run
  through the pinned-worker runtime scheduler. They park until an event is
  ready.
- Socket-backed networking and the HTTP helpers use nonblocking descriptors
  with timeout and cancellation support.
- Process waits and captured child stdio pipes use the same scheduler-backed
  wait path.
- Ordinary file operations offload through the pinned-worker scheduler-backed
  runtime, so the lightweight task is not tied to a blocking host thread.
- Hostname resolution, listener binding, UDP destination resolution, and
  blocking TCP and Unix connect syscalls offload to a generic blocking-I/O
  pool. A slow DNS resolver or connect attempt therefore does not pin the
  lightweight-task scheduler.

Two environment variables configure the blocking-I/O pool:

| Variable | Effect |
| --- | --- |
| `AURA_BLOCKING_WORKERS=<positive integer>` | Sets an exact worker count, with no clamping. When unset, Aura uses host parallelism, with fallback `4` and a derived `2..=8` clamp. |
| `AURA_BLOCKING_QUEUE_CAPACITY=<positive integer>` | Bounds accepted pending jobs only. When unset, the queue is unbounded. |

When the queue is full, admission is FIFO and scheduler-aware. The bound
limits the accepted pending backlog, not the tasks waiting for admission. It
cannot interrupt accepted work. It also cannot guarantee progress for
unrelated blocking I/O while every worker stays stuck.
