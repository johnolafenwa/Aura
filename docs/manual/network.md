# Network Module

The `net` module exposes scheduler-aware networking resources:

- TCP listeners and streams
- UDP sockets and datagrams
- HTTP listeners, exchanges, and client responses
- WebSocket listeners and sockets
- Unix domain sockets on Unix hosts
- TLS listeners and streams

```python
import net
import io
```

Most operations return `Result[..., io.Error]`. Waiting operations usually accept `timeout: Duration = ...`; omitting it means no caller deadline unless a protocol-specific hard limit is stated below. Pass explicit timeouts for services that need bounded latency or clean shutdown behavior. An explicit timeout must be non-negative, fit the host timer range, and produce a representable deadline; otherwise the operation returns `io.Error.InvalidInput`. Deadline overflow never means no deadline. This input policy is Provisional under ADR-0019.

Hostname resolution, socket binding, UDP destination resolution, and blocking TCP or Unix connect syscalls run on Aurora's bounded blocking service rather than on the lightweight-task scheduler. A connect timeout is one end-to-end budget: it includes DNS resolution and is shared by every resolved-address attempt, then by any remaining TLS, HTTP, or WebSocket handshake work. Cancellation ends the Aurora wait promptly; host work that cannot be interrupted may finish later and is discarded safely.

Text reads decode UTF-8 strictly and return `io.Error.InvalidData` for invalid bytes. TCP, Unix, and TLS deadlines return `io.Error.TimedOut`; a UDP receive deadline returns `Ok(None)`. Cancellation is reported as `io.Error.Cancelled` where the operation participates in scheduler cancellation.

## Constructors

| API | Signature | Contract |
| --- | --- | --- |
| `net.connect` | `connect(address: String) -> Result[net.TcpStream, io.Error]` | Opens a TCP connection to `host:port`. |
| `net.connect_timeout` | `connect_timeout(address: String, timeout: Duration) -> Result[net.TcpStream, io.Error]` | Opens a TCP connection with a deadline. |
| `net.listen` | `listen(address: String) -> Result[net.TcpListener, io.Error]` | Binds a TCP listener. Use `127.0.0.1:0` to request an available local port. |
| `net.udp_bind` | `udp_bind(address: String) -> Result[net.UdpSocket, io.Error]` | Binds a UDP socket. |
| `net.http_listen` | `http_listen(address: String) -> Result[net.HttpListener, io.Error]` | Binds a simple HTTP listener. |
| `net.websocket_listen` | `websocket_listen(address: String) -> Result[net.WebSocketListener, io.Error]` | Binds a WebSocket listener. |
| `net.websocket_connect` | `websocket_connect(url: String) -> Result[net.WebSocket, io.Error]` | Connects to a WebSocket URL. |
| `net.websocket_connect_timeout` | `websocket_connect_timeout(url: String, timeout: Duration) -> Result[net.WebSocket, io.Error]` | Connects to a WebSocket URL with a deadline. |
| `net.unix_listen` | `unix_listen(path: String) -> Result[net.UnixListener, io.Error]` | Binds a Unix domain socket path on Unix hosts. |
| `net.unix_connect` | `unix_connect(path: String) -> Result[net.UnixStream, io.Error]` | Connects to a Unix domain socket path. |
| `net.unix_connect_timeout` | `unix_connect_timeout(path: String, timeout: Duration) -> Result[net.UnixStream, io.Error]` | Connects to a Unix domain socket path with a deadline. |
| `net.tls_listen` | `tls_listen(address: String, cert_pem_path: String, key_pem_path: String) -> Result[net.TlsListener, io.Error]` | Binds a TLS listener using PEM certificate and key files. |
| `net.tls_connect` | `tls_connect(address: String, server_name: String, ca_pem_path: String) -> Result[net.TlsStream, io.Error]` | Connects with TLS verification using a CA PEM file. |
| `net.tls_connect_timeout` | `tls_connect_timeout(address: String, server_name: String, ca_pem_path: String, timeout: Duration) -> Result[net.TlsStream, io.Error]` | Connects with TLS verification and a deadline. |

## TCP

`net.TcpListener`:

| API | Signature | Contract |
| --- | --- | --- |
| `accept` | `accept(timeout: Duration = ...) -> Result[net.TcpStream, io.Error]` | Waits for the next incoming connection. |
| `local_addr` | `local_addr() -> Result[String, io.Error]` | Returns the bound local address. |
| `close` | `close() -> None` | Closes the listener. |

`net.TcpStream`:

| API | Signature | Contract |
| --- | --- | --- |
| `read_all` | `read_all(timeout: Duration = ...) -> Result[String, io.Error]` | Reads strict UTF-8 text until EOF, capped at 64 MiB. Use byte APIs for arbitrary data. |
| `read_line` | `read_line(timeout: Duration = ...) -> Result[Option[String], io.Error]` | Reads one strict UTF-8 line without its trailing LF/CRLF, capped at 64 MiB. Returns `Ok(None)` only on EOF. |
| `read_bytes` | `read_bytes(max_bytes: int32, timeout: Duration = ...) -> Result[Option[Vec[uint8]], io.Error]` | Reads up to `max_bytes` raw bytes. The count must be in `1..=67108864`; `Ok(None)` means EOF. |
| `read_exact` | `read_exact(count: int32, timeout: Duration = ...) -> Result[Vec[uint8], io.Error]` | Reads exactly `count` bytes or returns an error. The count must be in `1..=67108864`. |
| `write_all` | `write_all(text: String, timeout: Duration = ...) -> Result[None, io.Error]` | Writes all UTF-8 text. |
| `write_bytes` | `write_bytes(bytes: Vec[uint8], timeout: Duration = ...) -> Result[None, io.Error]` | Writes all raw bytes. |
| `flush` | `flush() -> Result[None, io.Error]` | Flushes pending stream writes. |
| `local_addr` | `local_addr() -> Result[String, io.Error]` | Returns the local socket address. |
| `peer_addr` | `peer_addr() -> Result[String, io.Error]` | Returns the peer socket address. |
| `shutdown_read` | `shutdown_read() -> Result[None, io.Error]` | Shuts down the read half. |
| `shutdown_write` | `shutdown_write() -> Result[None, io.Error]` | Shuts down the write half. |
| `shutdown_both` | `shutdown_both() -> Result[None, io.Error]` | Shuts down both halves. |
| `close` | `close() -> None` | Closes the stream handle. |

Example echo handler:

```python
import io
import net

def handle(stream: own net.TcpStream) -> Result[None, io.Error]:
    with conn = stream:
        match try conn.read_line(timeout=5s):
            case Option.Some(line):
                try conn.write_all(line, timeout=5s)
            case Option.None:
                pass
    return Result.Ok(None)
```

## UDP

`net.UdpSocket`:

| API | Signature | Contract |
| --- | --- | --- |
| `send_text` | `send_text(address: String, text: String, timeout: Duration = ...) -> Result[None, io.Error]` | Sends UTF-8 text to an address. |
| `send_bytes` | `send_bytes(address: String, bytes: Vec[uint8], timeout: Duration = ...) -> Result[None, io.Error]` | Sends raw bytes to an address. |
| `recv` | `recv(max_bytes: int32, timeout: Duration = ...) -> Result[Option[Vec[uint8]], io.Error]` | Receives bytes from a connected UDP socket. `Ok(None)` means the deadline expired. |
| `recv_from` | `recv_from(max_bytes: int32, timeout: Duration = ...) -> Result[Option[net.UdpDatagram], io.Error]` | Receives a datagram plus source address. `Ok(None)` means the deadline expired. |
| `local_addr` | `local_addr() -> Result[String, io.Error]` | Returns the local address. |
| `peer_addr` | `peer_addr() -> Result[String, io.Error]` | Returns the connected peer address when available. |
| `close` | `close() -> None` | Closes the socket handle. |

`net.UdpDatagram`:

| API | Signature | Contract |
| --- | --- | --- |
| `address` | `address() -> String` | Returns the source address. |
| `bytes` | `bytes() -> Vec[uint8]` | Returns the datagram payload as raw bytes. |
| `text` | `text() -> Result[String, io.Error]` | Decodes the payload as UTF-8 text. |

UDP preserves datagram boundaries. `max_bytes` must be in `1..=65535`; zero or a larger request returns `io.Error.InvalidInput` before receiving. A receive with a small buffer may truncate data according to platform behavior. Sends larger than the host datagram limit return `InvalidInput` where the host exposes that condition.

## HTTP Server

`net.HttpListener`:

| API | Signature | Contract |
| --- | --- | --- |
| `accept` | `accept(timeout: Duration = ...) -> Result[net.HttpExchange, io.Error]` | Waits for the next HTTP request and returns an exchange. |
| `local_addr` | `local_addr() -> Result[String, io.Error]` | Returns the bound local address. |
| `close` | `close() -> None` | Closes the listener. |

`net.HttpExchange`:

| API | Signature | Contract |
| --- | --- | --- |
| `method` | `method() -> String` | Returns the request method. |
| `path` | `path() -> String` | Returns the request path. |
| `headers` | `headers() -> Map[String, String]` | Returns request headers. |
| `body_text` | `body_text() -> Result[String, io.Error]` | Decodes the request body as UTF-8. |
| `body_bytes` | `body_bytes() -> Vec[uint8]` | Returns the raw request body. |
| `respond_text` | `respond_text(status: int32, text: own String, headers: own Map[String, String]) -> Result[None, io.Error]` | Consumes and sends a text response. |
| `respond_bytes` | `respond_bytes(status: int32, bytes: own Vec[uint8], headers: own Map[String, String]) -> Result[None, io.Error]` | Consumes and sends a byte response. |

Malformed HTTP requests are rejected by the listener path and do not permanently poison the listener. Content-length and chunked request bodies are supported. An incoming parsed HTTP message is limited to 16 MiB of wire data and 64 headers; oversized or invalid requests are surfaced as HTTP errors where the protocol allows it.

Headers are exposed as `Map[String, String]`. This boundary cannot faithfully represent repeated fields such as multiple `Set-Cookie` lines. The current conversion can expose duplicate equal keys internally despite the normal `Map` uniqueness rule, so applications that require lossless or canonical repeated-header handling must not use this 0.1 high-level HTTP surface.

## HTTP Client

| API | Signature | Contract |
| --- | --- | --- |
| `net.http_request_text` | `http_request_text(method: String, url: String, body: String, headers: Map[String, String]) -> Result[net.HttpResponse, io.Error]` | Sends an HTTP request with a text body. |
| `net.http_request_text_timeout` | `http_request_text_timeout(method: String, url: String, body: String, headers: Map[String, String], timeout: Duration) -> Result[net.HttpResponse, io.Error]` | Sends a text request with a deadline. |
| `net.http_request_bytes` | `http_request_bytes(method: String, url: String, bytes: Vec[uint8], headers: Map[String, String]) -> Result[net.HttpResponse, io.Error]` | Sends an HTTP request with a byte body. |
| `net.http_request_bytes_timeout` | `http_request_bytes_timeout(method: String, url: String, bytes: Vec[uint8], headers: Map[String, String], timeout: Duration) -> Result[net.HttpResponse, io.Error]` | Sends a byte request with a deadline. |

`net.HttpResponse`:

| API | Signature | Contract |
| --- | --- | --- |
| `status` | `status() -> int32` | Returns the numeric status code. |
| `reason` | `reason() -> String` | Returns the reason phrase. |
| `headers` | `headers() -> Map[String, String]` | Returns response headers. |
| `text` | `text() -> Result[String, io.Error]` | Decodes the body as UTF-8. |
| `bytes` | `bytes() -> Vec[uint8]` | Returns the raw response body. |

Use byte request and response APIs for binary payloads or unknown encodings. Client URLs may use `http://` or certificate-validated `https://`; responses support content length, chunked transfer encoding, and connection-close framing. The same 16 MiB incoming-message and 64-header limits apply. Redirect following, connection pooling, HTTP/2, proxies, decompression, and custom-CA arguments on the high-level HTTP helpers are not part of 0.1.

## WebSocket

`net.WebSocketListener`:

| API | Signature | Contract |
| --- | --- | --- |
| `accept` | `accept(timeout: Duration = ...) -> Result[net.WebSocket, io.Error]` | Waits for the next WebSocket connection. |
| `local_addr` | `local_addr() -> Result[String, io.Error]` | Returns the bound local address. |

`net.WebSocketListener` has no explicit `close()` member in Aurora 0.1. It is released when its value is dropped, but it cannot currently be used as a user-defined `with` resource. This is a known resource-surface limitation.

`net.WebSocket`:

| API | Signature | Contract |
| --- | --- | --- |
| `send_text` | `send_text(text: String, timeout: Duration = ...) -> Result[None, io.Error]` | Sends a text frame. |
| `send_bytes` | `send_bytes(bytes: Vec[uint8], timeout: Duration = ...) -> Result[None, io.Error]` | Sends a binary frame. |
| `recv_text` | `recv_text(timeout: Duration = ...) -> Result[Option[String], io.Error]` | Receives the next text or binary message decoded as strict UTF-8; `Ok(None)` on close. |
| `recv_bytes` | `recv_bytes(timeout: Duration = ...) -> Result[Option[Vec[uint8]], io.Error]` | Receives the next text or binary message as bytes; `Ok(None)` on close. |
| `close` | `close() -> None` | Closes the WebSocket. |

Use text receive when the payload must be valid UTF-8 and bytes otherwise. Messages are capped at 64 MiB; individual frames and the write buffer are capped at 16 MiB. WebSocket accept/send/receive cancellation is not yet as complete as the TCP/UDP scheduler surface, and `close()` currently discards host close errors.

## Unix Domain Sockets

Unix socket APIs are available on Unix hosts.

`net.UnixListener`:

| API | Signature | Contract |
| --- | --- | --- |
| `accept` | `accept(timeout: Duration = ...) -> Result[net.UnixStream, io.Error]` | Waits for the next Unix stream connection. |
| `close` | `close() -> None` | Closes the listener. |

`net.UnixStream`:

| API | Signature | Contract |
| --- | --- | --- |
| `read_line` | `read_line(timeout: Duration = ...) -> Result[Option[String], io.Error]` | Reads one strict UTF-8 line without its trailing LF/CRLF, `Ok(None)` on EOF. |
| `read_exact` | `read_exact(count: int32, timeout: Duration = ...) -> Result[Vec[uint8], io.Error]` | Reads exactly `count` bytes; count must be in `1..=67108864`. |
| `write_all` | `write_all(text: String, timeout: Duration = ...) -> Result[None, io.Error]` | Writes all text. |
| `close` | `close() -> None` | Closes the stream. |

`net.unix_listen(...)` refuses to clobber a non-socket filesystem path.

## TLS

TLS APIs use PEM files for certificates and keys. Maintained examples keep test certificates under `examples/io/certs`.

`net.TlsListener`:

| API | Signature | Contract |
| --- | --- | --- |
| `accept` | `accept(timeout: Duration = ...) -> Result[net.TlsStream, io.Error]` | Waits for a TLS connection and handshake. |
| `local_addr` | `local_addr() -> Result[String, io.Error]` | Returns the bound local address. |
| `close` | `close() -> None` | Closes the listener. |

`net.TlsStream`:

| API | Signature | Contract |
| --- | --- | --- |
| `read_line` | `read_line(timeout: Duration = ...) -> Result[Option[String], io.Error]` | Reads one strict UTF-8 line without its trailing LF/CRLF, `Ok(None)` on EOF. |
| `read_exact` | `read_exact(count: int32, timeout: Duration = ...) -> Result[Vec[uint8], io.Error]` | Reads exactly `count` decrypted bytes; count must be in `1..=67108864`. |
| `write_all` | `write_all(text: String, timeout: Duration = ...) -> Result[None, io.Error]` | Writes all text through the TLS stream. |
| `close` | `close() -> None` | Closes the TLS stream. |

Handshake and accept paths use scheduler-aware waits. A TLS handshake also has a hard 10-second cap even when the caller omits a shorter timeout. Use explicit timeouts for public-facing services.

## Resource Cleanup

Network listeners and streams are owned resources. Prefer `with` when the lifetime is lexical:

```python
import io
import net

def show_addr() -> Result[None, io.Error]:
    with listener = try net.listen("127.0.0.1:0"):
        print(try listener.local_addr())
    return Result.Ok(None)
```

When a resource is not scoped with `with`, call its `close()` method when one is provided by the type. Cancellation stops Aurora's wait but cannot roll back host I/O that already completed.

## Grammar

The network module adds no source-language grammar. Network programs use ordinary imports, calls, named arguments, `Duration` literals, `Result`, `Option`, `try`, `match`, task constructs, and `with`. Addresses, URLs, server names, and Unix socket paths are runtime `String` values, not specialized literals.

Omitting a parameter displayed with `= ...` selects its builtin default. In particular, an omitted timeout means no caller-supplied deadline unless this page states a protocol hard cap. Text and byte operations are distinct members; the selected member determines UTF-8 decoding.

## Typing Rules

The constructor and method signatures in all tables above are normative. Listeners, streams, sockets, exchanges, and WebSockets are non-copy resource values. Fallible operations return `Result[..., io.Error]`; EOF and UDP receive timeout use `Option` only in the positions explicitly documented. `Duration` is required for timeout parameters, and byte-count parameters are `int32` checked against each API's runtime range.

Text members accept or return `String` and enforce UTF-8. Byte members accept or return `Vec[uint8]`. HTTP headers use `Map[String, String]`. `HttpExchange.respond_text` and `respond_bytes` consume their response body and header map. Other data arguments are shared for the call unless their displayed signature explicitly says `own`.

## Runtime Semantics

Resolution, binding, and connect work use the bounded blocking service. One explicit connect timeout is an end-to-end budget shared across name resolution, resolved-address attempts, and remaining protocol handshake work. Before host work begins, the runtime rejects a negative, host-unrepresentable, or deadline-overflowing timeout as `io.Error.InvalidInput`; it never treats such a value as omission. TCP, Unix, TLS, HTTP, and WebSocket waiting failures return typed errors as specified; UDP receive timeout returns `Ok(None)`. Cancellation ends the Aurora wait and returns `io.Error.Cancelled` on cancellation-aware operations, while already-started host work may complete later and is discarded.

TCP is a byte stream; UDP preserves datagrams. Text reads decode strictly and remove only their documented line ending. HTTP supports content-length, chunked, and connection-close framing under the stated parser caps. WebSocket receives complete text or binary messages, with text mode enforcing UTF-8. TLS verifies the named peer using the configured CA file or, for the high-level HTTPS client, the maintained Web PKI root set.

## Ownership And Evaluation Order

Arguments are evaluated left to right. Successful constructors and accept operations return fresh owned resources. Moving a resource invalidates the source binding. Read and accept operations mutate host protocol state internally but are callable through their documented shared receiver; write, send, shutdown, response, and explicit close operations require a mutable receiver place. Response bodies and header maps marked `own` are moved before the response operation begins.

`with` closes a resource exactly once on every lexical scope exit when the type has `close()`. Cleanup cannot undo bytes already sent or host operations already completed. `WebSocketListener` has no `close()` member and therefore cannot satisfy the user-visible `with` resource contract; dropping its owned value is its only current release path.

## Diagnostics

Unknown network members use `AU2001`, type mismatches use `AU2002`, invalid argument binding uses `AU2004`, and remaining static rejections use `AU2999`. Use after moving a resource uses `AU3001`, borrow conflicts use `AU3002`, and a mutating network method called through an immutable place uses `AU3003`.

DNS failures, connection refusal, timeout, invalid UTF-8, invalid byte counts,
invalid timeout values or deadlines, closed resources, cancellation, TLS
verification failure, and protocol errors are documented typed
`Result.Err(io.Error)` outcomes, not language diagnostics. An invariant
failure escaping that typed boundary uses the general runtime registry,
including `AU4005` for a resource or I/O trap.

## Backend Support

TCP, UDP, HTTP, WebSocket, and TLS APIs are implemented by the MIR runtime and direct native backend. Unix domain sockets are implemented by both execution backends on maintained Unix hosts. Timeout accounting, typed error mapping, read caps, protocol parsing, ownership, and cleanup are backend-parity contracts.

Address selection, DNS answers, socket options chosen by the host libraries, and exact host error messages may differ by machine. The high-level HTTPS client uses the same platform-independent Web PKI root policy in both backends.

## Limits And Implementation-Defined Behavior

Whole TCP text reads, TCP line reads, and individual byte-count reads are capped at 64 MiB; TCP/Unix/TLS exact counts must be `1..=67108864`. UDP receive counts must be `1..=65535`, and truncation with a smaller receive buffer follows the host. Incoming parsed HTTP messages are capped at 16 MiB of wire data and 64 headers. The parser cap includes the start line, headers, transfer framing, trailers, and body; outbound HTTP writers have no separate size cap. The string-map header boundary is not lossless for repeated fields and can currently expose duplicate equal keys internally.

WebSocket messages are capped at 64 MiB; frames and the write buffer are capped at 16 MiB. WebSocket listener close is unavailable, WebSocket cancellation coverage is incomplete, and WebSocket close currently discards host close errors. TLS handshakes have a hard 10-second cap in addition to any shorter caller deadline. Unix sockets are unavailable on non-Unix hosts, and `unix_listen` will not replace a non-socket path. Redirects, pooling, HTTP/2, proxies, decompression, high-level custom-CA arguments, and lossless repeated-header APIs are absent.

## Status

The constructors, protocols, resources, typed errors, timeouts, cancellation behavior, scheduler integration, cleanup rules, and caps documented on this page are implemented and maintained for Aurora 0.1. The fixed resource-cap policy recorded by ADR-0018 remains Provisional pending the Batch 2 checkpoint review, and the invalid host-timer policy recorded by ADR-0019 remains Provisional pending the Phase 3 checkpoint review; no other network semantics on this page are provisional.

The repeated-header representation, missing WebSocket-listener close operation, incomplete WebSocket cancellation, and discarded WebSocket close errors are documented current limitations. Protocol additions and richer APIs listed above are unavailable future work and are non-normative.
