# I/O And Networking

Aura now has a maintained I/O surface through four builtin modules:

- `io`
- `fs`
- `net`
- `process`

These modules are imported like ordinary namespaces:

```python
import io
import fs
import net
import process
```

The current runtime model uses scheduler-backed lightweight tasks. Queue waits,
timer waits, and the maintained socket/HTTP surface share the same evented
runtime scheduler. They park until an event is ready. Hostname resolution and
blocking connect syscalls run on the generic
blocking-I/O pool, so a slow DNS resolver or connect attempt does not pin the
lightweight-task scheduler.

## Standard Input And Output

Use `io.write(...)`, `io.flush()`, and `io.read_line()` for explicit terminal I/O:

```python
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
        case Result.Ok(Option.Some(line)):
            print(line)
            return 0
        case Result.Ok(Option.None):
            return 0
        case Result.Err(_):
            return 1
```

`io.read_line()` returns `Result[Option[str], io.Error]`:

- `Result.Ok(Option.Some(text))` when a line was read
- `Result.Ok(Option.None)` on end-of-file
- `Result.Err(...)` on I/O failure

## File I/O

The `fs` module provides one-shot helpers and scoped file handles.

Text and binary one-shot helpers:

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

The one-shot helpers and `fs.File` whole-file reads are capped at 256 MiB of remaining content in both `aura run` and built binaries. Aura 0.2 has no incremental file-read member, so larger files need a host helper or pre-splitting.

Scoped file-handle constructors:

- `fs.open(path)`
- `fs.create(path)`
- `fs.append(path)`

Those constructors return `Result[fs.File, io.Error]`. `fs.File` works with `with` and exposes:

- `read_all()`
- `read_bytes()`
- `write_all(text)`
- `write_bytes(bytes)`
- `flush()`
- `close()`

Text example:

```python
import fs
import io

def load_text(path: str) -> Result[str, io.Error]:
    with file = try fs.open(path):
        return file.read_all()
```

Binary example:

```python
import fs
import io

def copy_bytes(path: str) -> Result[list[uint8], io.Error]:
    with file = try fs.open(path):
        bytes = try file.read_bytes()
        return Result.Ok(bytes)
```

Crossing between raw data and UTF-8 text is explicit. Use `text.to_bytes()` or
`str.from_bytes(payload)`. [22-bytes.md](22-bytes.md) covers those
conversions plus strict
hex/base64 codecs and SHA-256.

See:

- [examples/io/read_text_file.au](../examples/io/read_text_file.au)
- [examples/io/bytes_file_io.au](../examples/io/bytes_file_io.au)

## Processes

The `process` module provides shell-free subprocess helpers that fit the current Aura runtime model.

Process constructors:

- `process.supervisor()`
- `process.start(command, cwd=..., env=..., stdin=..., stdout=..., stderr=..., group=false)`
- `process.run(command, cwd=..., env=..., stdin=..., stdout=..., stderr=..., timeout=..., group=false)`
- `process.inherit()`
- `process.null()`
- `process.pipe()`

`command` is always an explicit `list[str]` argv list. Aura does not provide a shell-string subprocess API.
When `group=true`, Aura starts the child in its own process group and applies terminate/kill/close cleanup to that full group. On current maintained hosts, grouped children are supported on Unix.

`process.start(...)` returns `Result[process.Child, process.Error]`. `process.Child` works with `with` and exposes:

- `stdin()`
- `stdout()`
- `stderr()`
- `wait(timeout=...)`
- `wait_or_none(timeout=...)`
- `wait_ok(timeout=...)`
- `kill()`
- `terminate()`
- `close()`

`process.pipe()` is used to request captured child stdio streams. `process.Pipe` works with `with` and exposes:

- `read_all()`
- `read_line(timeout=...)`
- `read_bytes(max_bytes, timeout=...)`
- `write_all(text, timeout=...)`
- `write_bytes(bytes, timeout=...)`
- `flush()`
- `close()`

`process.run(...)` returns `Result[process.Completed, process.Error]`. `process.Completed` exposes:

- `status()`
- `success()`
- `stdout()` for UTF-8 text
- `stderr()` for UTF-8 text
- `stdout_bytes()` for raw bytes
- `stderr_bytes()` for raw bytes
- `check()`

`process.supervisor()` returns `process.Supervisor`. `process.Supervisor` works with `with` and exposes:

- `start(name, command, cwd=..., env=..., stdin=..., stdout=..., stderr=..., restart=..., backoff=..., max_restarts=..., group=true)`
- `wait(timeout=...)`
- `wait_or_none(timeout=...)`
- `stop()`
- `is_empty()`
- `close()`

Related process-supervisor enums:

- `process.RestartPolicy`
- `process.SupervisorEvent`
- `process.SupervisorWait`

Supervisor children default to `group=true`, so `stop()` and `close()` shut
down the leader process and its full child tree.
When `restart` is `process.RestartPolicy.OnFailure` or `process.RestartPolicy.Always`, `backoff` must be at least `10ms` to prevent zero-delay restart loops.

`Supervisor.start` retains the configuration it may need for a restart, so all
of its configuration slots are explicit `own` parameters. This includes the
copy-valued restart, backoff, count, and group settings; `own` is harmless for
copy values and keeps the retention contract uniform. Clone a clone-safe move
value before the call only when the caller also needs an independent copy;
otherwise ownership must transfer.

`process.Child.close()` is cleanup-oriented: it sends a graceful terminate signal first, waits briefly, and escalates to kill if the child does not exit promptly. For grouped children it waits for the full child process group to disappear before returning.

One-shot example:

```python
import process

def run_echo() -> Result[None, process.Error]:
    completed = try process.run(["/bin/echo", "aura process"], stdout=process.pipe(), stderr=process.pipe(), timeout=1s, group=true)
    try completed.check()
    print(completed.stdout().trim())
    print(completed.stdout_bytes().len())
    return Result.Ok(None)
```

Interactive pipe example:

```python
import process

def roundtrip() -> Result[None, process.Error]:
    with child = try process.start(["/bin/cat"], stdin=process.pipe(), stdout=process.pipe(), stderr=process.null(), group=true):
        match child.stdin():
            case Option.Some(stdin_pipe):
                try stdin_pipe.write_all("ping\n", timeout=500ms)
                try stdin_pipe.flush()
                stdin_pipe.close()
            case Option.None:
                pass

        match child.stdout():
            case Option.Some(stdout_pipe):
                match try stdout_pipe.read_line(timeout=500ms):
                    case Option.Some(text):
                        print(text.trim())
                    case Option.None:
                        pass
            case Option.None:
                pass
        print(try child.wait_ok(timeout=2s))
        return Result.Ok(None)
```

See:

- [examples/io/process_run.au](../examples/io/process_run.au)
- [examples/io/process_pipes.au](../examples/io/process_pipes.au)
- [examples/io/process_supervisor.au](../examples/io/process_supervisor.au)

Supervisor example:

```python
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

## TCP

The `net` module provides TCP clients and listeners on the maintained nonblocking socket runtime:

- `net.connect(address)`
- `net.connect_timeout(address, timeout)`
- `net.listen(address)`

`net.TcpListener` methods:

- `accept(timeout=...)`
- `local_addr()`
- `close()`

`net.TcpStream` methods:

- `read_all(timeout=...)`
- `read_line(timeout=...)`
- `read_bytes(max_bytes, timeout=...)`
- `read_exact(count, timeout=...)`
- `write_all(text, timeout=...)`
- `write_bytes(bytes, timeout=...)`
- `flush()`
- `local_addr()`
- `peer_addr()`
- `shutdown_read()`
- `shutdown_write()`
- `shutdown_both()`
- `close()`

Both listener and stream resources work with `with`.

Text example:

```python
import io
import net

def serve(addresses: Queue[str]) -> Result[None, io.Error]:
    with server = try net.listen("127.0.0.1:0"):
        addresses.put(try server.local_addr())
        with stream = try server.accept(timeout=1s):
            match try stream.read_line(timeout=1s):
                case Option.Some(text):
                    try stream.write_all("echo:" + text, timeout=1s)
                    try stream.flush()
                case Option.None:
                    pass
        return Result.Ok(None)
```

Listeners and other live network resources are not `Transfer`, so a worker task
creates and owns its listener. The copy queue handle crosses the task boundary,
and the worker sends the listener's owned `str` address back after binding.

See:

- [examples/io/tcp_echo.au](../examples/io/tcp_echo.au)
- [examples/io/tcp_bytes.au](../examples/io/tcp_bytes.au)

## UDP

Aura also supports UDP sockets on the same poll-driven runtime:

- `net.udp_bind(address)`

`net.UdpSocket` methods:

- `send_text(address, text, timeout=...)`
- `send_bytes(address, bytes, timeout=...)`
- `recv(max_bytes, timeout=...)`
- `recv_from(max_bytes, timeout=...)`
- `local_addr()`
- `peer_addr()`
- `close()`

`recv_from(...)` returns `Option[net.UdpDatagram]`. `net.UdpDatagram` exposes:

- `address()`
- `bytes()`
- `text()`

See [examples/io/udp_echo.au](../examples/io/udp_echo.au).

## HTTP

The maintained HTTP convenience surface includes:

- `net.http_listen(address)`
- `net.http_request_text(method, url, body, headers)`
- `net.http_request_text_timeout(method, url, body, headers, timeout)`
- `net.http_request_bytes(method, url, bytes, headers)`
- `net.http_request_bytes_timeout(method, url, bytes, headers, timeout)`

`net.HttpListener` methods:

- `accept(timeout=...)`
- `local_addr()`
- `close()`

`net.HttpExchange` methods:

- `method()`
- `path()`
- `headers()`
- `body_text()`
- `body_bytes()`
- `respond_text(status, text, headers)`
- `respond_bytes(status, bytes, headers)`

`net.HttpResponse` methods:

- `status()`
- `reason()`
- `headers()`
- `text()`
- `bytes()`
- `close()`

See [examples/io/http_roundtrip.au](../examples/io/http_roundtrip.au).

### Application-Level Retries

Aura's HTTP helpers perform one request. The generic
`control.retry` helper can repeat a capture-free `def() -> Result[T, E]`
worker when every `Err` is retryable, using a fixed attempt budget and
exponential `Duration` backoff. HTTP status classification, jitter, and richer
policy remain an application concern. The maintained
[retrying network worker](../examples/agents/retrying_network_worker.au)
retries only `503`, returns other statuses such as `429` unchanged, and returns
the last `503` when its attempt budget is exhausted.

The example uses `random.Rng(42)` for deterministic jitter, doubles a
`Duration` backoff after each retry, and checks the final-attempt guard before
the RNG draw, retry log, and `sleep(...)`. It makes seven real requests against
an ephemeral loopback listener and applies explicit five-second deadlines to
listener acceptance, HTTP requests, and task results. `TaskGroup` and `with`
scopes ensure the worker, server, listener, exchanges, and responses are all
closed deterministically. The CLI conformance test executes the exact trace on
both the MIR and forced-direct backends.

## WebSockets

The maintained WebSocket surface includes:

- `net.websocket_listen(address)`
- `net.websocket_connect(url)`
- `net.websocket_connect_timeout(url, timeout)`

`net.WebSocketListener` methods:

- `accept(timeout=...)`
- `local_addr()`
- `close()`

`net.WebSocket` methods:

- `send_text(text, timeout=...)`
- `send_bytes(bytes, timeout=...)`
- `recv_text(timeout=...)`
- `recv_bytes(timeout=...)`
- `close()`

See [examples/io/websocket_roundtrip.au](../examples/io/websocket_roundtrip.au).

## Unix Sockets And TLS

Aura also supports Unix domain stream sockets and TLS streams on the maintained nonblocking socket runtime.

Unix-socket constructors:

- `net.unix_listen(path)`
- `net.unix_connect(path)`
- `net.unix_connect_timeout(path, timeout)`

Unix-socket resource methods:

- `net.UnixListener.accept(timeout=...)`
- `net.UnixListener.close()`
- `net.UnixStream.read_line(timeout=...)`
- `net.UnixStream.read_exact(count, timeout=...)`
- `net.UnixStream.write_all(text, timeout=...)`
- `net.UnixStream.close()`

TLS constructors:

- `net.tls_listen(address, cert_pem_path, key_pem_path)`
- `net.tls_connect(address, server_name, ca_pem_path)`
- `net.tls_connect_timeout(address, server_name, ca_pem_path, timeout)`

TLS resource methods:

- `net.TlsListener.accept(timeout=...)`
- `net.TlsListener.local_addr()`
- `net.TlsListener.close()`
- `net.TlsStream.read_line(timeout=...)`
- `net.TlsStream.read_exact(count, timeout=...)`
- `net.TlsStream.write_all(text, timeout=...)`
- `net.TlsStream.close()`

Unix domain sockets require a Unix host at runtime.

See [examples/io/unix_tls_roundtrip.au](../examples/io/unix_tls_roundtrip.au), which embeds a self-signed certificate so it stays runnable without extra setup.

## Timeouts And Cancellation

Most maintained socket operations accept optional `timeout=...` arguments.
Timeouts are expressed with Aura `Duration` values such as `100ms`, `1s`, or
`2m`. Computed timeouts may use `Duration.ms(n)` or arithmetic such as
`attempt * 1ms`. Explicit values must be non-negative and fit the host
deadline; invalid values return `io.Error.InvalidInput`. Only an omitted
timeout is unlimited.

For connect operations, one timeout budget covers blocking-pool admission,
hostname resolution, every resolved-address attempt, and the remaining
protocol handshake. Aura does not restart the full timeout for each address
returned by DNS. Cancellation or expiry before pool acceptance prevents
submission. After acceptance it stops the Aura task's wait, but the host
resolver or connect syscall may finish later and its result is discarded
safely.

`AURA_BLOCKING_WORKERS=<positive integer>` selects an exact worker count
without clamping; the absent default uses host parallelism with fallback `4`
and a derived `2..=8` clamp.
`AURA_BLOCKING_QUEUE_CAPACITY=<positive integer>` optionally bounds accepted
pending jobs only, and omission preserves an unbounded queue. Full-queue
admission is FIFO and scheduler-aware. The queue bound limits accepted pending
backlog, not admission waiters, and cannot interrupt accepted work or guarantee
unrelated blocking-I/O progress while every worker remains stuck.

`process.run(...)` follows the same rule through
`process.Error.Io(io.Error.InvalidInput)`. Omitting its timeout uses an
internal absence marker; explicit negative Duration values never act as that
marker.

The socket runtime also threads task-group cancellation into maintained socket
waits. If a task group is cancelled while a child is waiting on a maintained
network operation, that operation returns `io.Error.Cancelled` promptly.

## Current Model

This surface uses one explicit scheduler-backed I/O model:

- queue waits, `sleep(...)`, socket waits, and the maintained HTTP helpers all
  run through the pinned-worker runtime scheduler
- socket-backed networking and HTTP convenience helpers use nonblocking descriptors with timeout and cancellation support
- hostname resolution, listener binding, UDP destination resolution, and blocking TCP/Unix connect syscalls offload through the configurable generic blocking-I/O pool
- process waits and captured child stdio pipes use the same scheduler-backed wait path
- Aura tasks are scheduler-backed lightweight coroutines; each task does not require its own OS thread
- ordinary file operations offload through the pinned-worker scheduler-backed
  runtime, keeping the lightweight task free from a blocking host thread

Current process notes:

- subprocess APIs are shell-free and take explicit argv vectors only
- grouped children are supported through `group=true` on `process.start(...)` and `process.run(...)`
- there is not yet a PTY surface
- there are not yet pipeline helpers

This keeps lightweight tasks schedulable while host operations wait.
