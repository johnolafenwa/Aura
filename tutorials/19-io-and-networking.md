# I/O And Networking

Aurora now has a maintained I/O surface through three builtin modules:

- `io`
- `fs`
- `net`

These modules are imported like ordinary namespaces:

```python
import io
import fs
import net
```

The current runtime model uses scheduler-backed lightweight tasks, and queue waits, timer waits, and the maintained socket/HTTP surface now share the same evented runtime scheduler underneath instead of spinning or blocking on per-operation sleeps.

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

`io.read_line()` returns `Result[Option[String], io.Error]`:

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

def load_text(path: String) -> Result[String, io.Error]:
    with file = try fs.open(path):
        return file.read_all()
```

Binary example:

```python
import fs
import io

def copy_bytes(path: String) -> Result[Vec[uint8], io.Error]:
    with file = try fs.open(path):
        bytes = try file.read_bytes()
        return Result.Ok(bytes)
```

See:

- [examples/io/read_text_file.au](../examples/io/read_text_file.au)
- [examples/io/bytes_file_io.au](../examples/io/bytes_file_io.au)

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

def serve(listener: net.TcpListener) -> Result[None, io.Error]:
    with server = listener:
        with stream = try server.accept(timeout=1s):
            match try stream.read_line(timeout=1s):
                case Option.Some(text):
                    try stream.write_all("echo:" + text, timeout=1s)
                    try stream.flush()
                case Option.None:
                    pass
        return Result.Ok(None)
```

See:

- [examples/io/tcp_echo.au](../examples/io/tcp_echo.au)
- [examples/io/tcp_bytes.au](../examples/io/tcp_bytes.au)

## UDP

Aurora also supports UDP sockets on the same poll-driven runtime:

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

Aurora also supports Unix domain stream sockets and TLS streams on the maintained nonblocking socket runtime.

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

Most maintained socket operations accept optional `timeout=...` arguments. Timeouts are expressed with Aurora `Duration` values such as `100ms`, `1s`, or `2m`.

The socket runtime also threads task-group cancellation into maintained socket waits. If a task group is cancelled while a child is waiting on a maintained network operation, that operation returns an `io.Error` instead of waiting forever.

## Current Model

This surface is deliberately explicit but no longer relies on the old blocking/polling split:

- queue waits, `sleep(...)`, `select`, socket waits, and the maintained HTTP helpers all run through the shared runtime scheduler
- socket-backed networking and HTTP convenience helpers use nonblocking descriptors with timeout and cancellation support
- Aurora tasks are scheduler-backed lightweight coroutines rather than one-OS-thread-per-task workers
- ordinary file operations still execute synchronously for now

That keeps the execution model straightforward while removing the old timeout-spin loops and blocking HTTP special case.
