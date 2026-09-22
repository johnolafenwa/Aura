# Network Module

The `net` module provides networking resources that cooperate with Aura's task scheduler:

- TCP listeners and streams
- UDP sockets and datagrams
- HTTP listeners, exchanges, and client responses
- WebSocket listeners and sockets
- Unix domain sockets on Unix hosts
- TLS listeners and streams

```aura
import net
import io
```

Most operations return `Result[..., io.Error]`.

## Timeouts, Cancellation, And Text

Waiting operations usually take `timeout: Duration = ...`. Omitting it means no caller deadline, unless this page states a hard limit for the protocol. Pass explicit timeouts for services that need bounded latency or a clean shutdown.

An explicit timeout must be non-negative, fit the host timer range, and give a deadline that can be represented. Otherwise the operation returns `io.Error.InvalidInput`. A deadline overflow never means "no deadline".

Deadlines and cancellation report differently by protocol:

- TCP, Unix, and TLS deadlines return `io.Error.TimedOut`.
- A UDP receive deadline returns `Ok(None)`.
- Operations that take part in scheduler cancellation report it as `io.Error.Cancelled`.

Text reads decode UTF-8 strictly. They return `io.Error.InvalidData` for invalid bytes.

### Blocking-I/O Pool

Some work runs on Aura's generic blocking-I/O pool, not on the lightweight-task scheduler:

- hostname resolution
- socket binding
- UDP destination resolution
- blocking TCP or Unix connect system calls

The pool has a configurable worker count and an optional bound on its pending queue. When a bounded queue is full, Aura callers wait in first-in, first-out order, and the scheduler parks them while they wait.

A connect timeout is one end-to-end budget. It covers queue admission and DNS resolution, and every resolved-address attempt shares it. What remains then covers any TLS, HTTP, or WebSocket handshake.

If cancellation or expiry happens before the pool accepts the work, the work is never submitted. The pool cannot interrupt host work once it accepts it. That work may finish later, and the runtime safely discards its result.

## Constructors

| API | Signature | Contract |
| --- | --- | --- |
| `net.connect` | `connect(address: str) -> Result[net.TcpStream, io.Error]` | Opens a TCP connection to `host:port`. |
| `net.connect_timeout` | `connect_timeout(address: str, timeout: Duration) -> Result[net.TcpStream, io.Error]` | Opens a TCP connection with a deadline. |
| `net.listen` | `listen(address: str) -> Result[net.TcpListener, io.Error]` | Binds a TCP listener. Use `127.0.0.1:0` to ask for any free local port. |
| `net.udp_bind` | `udp_bind(address: str) -> Result[net.UdpSocket, io.Error]` | Binds a UDP socket. |
| `net.http_listen` | `http_listen(address: str) -> Result[net.HttpListener, io.Error]` | Binds a simple HTTP listener. |
| `net.websocket_listen` | `websocket_listen(address: str) -> Result[net.WebSocketListener, io.Error]` | Binds a WebSocket listener. |
| `net.websocket_connect` | `websocket_connect(url: str) -> Result[net.WebSocket, io.Error]` | Connects to a WebSocket URL. |
| `net.websocket_connect_timeout` | `websocket_connect_timeout(url: str, timeout: Duration) -> Result[net.WebSocket, io.Error]` | Connects to a WebSocket URL with a deadline. |
| `net.unix_listen` | `unix_listen(path: str) -> Result[net.UnixListener, io.Error]` | Binds a Unix domain socket path on Unix hosts. |
| `net.unix_connect` | `unix_connect(path: str) -> Result[net.UnixStream, io.Error]` | Connects to a Unix domain socket path. |
| `net.unix_connect_timeout` | `unix_connect_timeout(path: str, timeout: Duration) -> Result[net.UnixStream, io.Error]` | Connects to a Unix domain socket path with a deadline. |
| `net.tls_listen` | `tls_listen(address: str, cert_pem_path: str, key_pem_path: str) -> Result[net.TlsListener, io.Error]` | Binds a TLS listener with PEM certificate and key files. |
| `net.tls_connect` | `tls_connect(address: str, server_name: str, ca_pem_path: str) -> Result[net.TlsStream, io.Error]` | Connects over TLS and verifies the peer against a CA PEM file. |
| `net.tls_connect_timeout` | `tls_connect_timeout(address: str, server_name: str, ca_pem_path: str, timeout: Duration) -> Result[net.TlsStream, io.Error]` | Connects over TLS with verification and a deadline. |

## TCP

`net.TcpListener`:

| API | Signature | Contract |
| --- | --- | --- |
| `accept` | `accept(timeout: Duration = ...) -> Result[net.TcpStream, io.Error]` | Waits for the next incoming connection. |
| `local_addr` | `local_addr() -> Result[str, io.Error]` | Returns the bound local address. |
| `close` | `close() -> None` | Closes the listener. |

`net.TcpStream`:

| API | Signature | Contract |
| --- | --- | --- |
| `read_all` | `read_all(timeout: Duration = ...) -> Result[str, io.Error]` | Reads strict UTF-8 text until EOF. Capped at 64 MiB. Use the byte APIs for arbitrary data. |
| `read_line` | `read_line(timeout: Duration = ...) -> Result[str \| None, io.Error]` | Reads one strict UTF-8 line without its trailing LF or CRLF. Capped at 64 MiB. Returns `Ok(None)` only at EOF. An empty line is a present `""`. |
| `read_bytes` | `read_bytes(max_bytes: int32, timeout: Duration = ...) -> Result[list[uint8] \| None, io.Error]` | Reads up to `max_bytes` raw bytes. The count must be in `1..=67108864`. `Ok(None)` means EOF. |
| `read_exact` | `read_exact(count: int32, timeout: Duration = ...) -> Result[list[uint8], io.Error]` | Reads exactly `count` bytes, or returns an error. The count must be in `1..=67108864`. |
| `write_all` | `write_all(text: str, timeout: Duration = ...) -> Result[None, io.Error]` | Writes all the UTF-8 text. |
| `write_bytes` | `write_bytes(bytes: list[uint8], timeout: Duration = ...) -> Result[None, io.Error]` | Writes all the raw bytes. |
| `flush` | `flush() -> Result[None, io.Error]` | Flushes pending stream writes. |
| `local_addr` | `local_addr() -> Result[str, io.Error]` | Returns the local socket address. |
| `peer_addr` | `peer_addr() -> Result[str, io.Error]` | Returns the peer socket address. |
| `shutdown_read` | `shutdown_read() -> Result[None, io.Error]` | Shuts down the read half. |
| `shutdown_write` | `shutdown_write() -> Result[None, io.Error]` | Shuts down the write half. |
| `shutdown_both` | `shutdown_both() -> Result[None, io.Error]` | Shuts down both halves. |
| `close` | `close() -> None` | Closes the stream handle. |

Example echo handler:

```aura
import io
import net

def handle(stream: own net.TcpStream) -> Result[None, io.Error]:
    with conn = stream:
        match try conn.read_line(timeout=5s):
            case str as line:
                try conn.write_all(line, timeout=5s)
            case None:
                pass
    return Result.Ok(None)
```

## UDP

`net.UdpSocket`:

| API | Signature | Contract |
| --- | --- | --- |
| `send_text` | `send_text(address: str, text: str, timeout: Duration = ...) -> Result[None, io.Error]` | Sends UTF-8 text to an address. |
| `send_bytes` | `send_bytes(address: str, bytes: list[uint8], timeout: Duration = ...) -> Result[None, io.Error]` | Sends raw bytes to an address. |
| `recv` | `recv(max_bytes: int32, timeout: Duration = ...) -> Result[list[uint8] \| None, io.Error]` | Receives bytes on a connected UDP socket. `Ok(None)` means the deadline expired. An empty datagram is a present list. |
| `recv_from` | `recv_from(max_bytes: int32, timeout: Duration = ...) -> Result[net.UdpDatagram \| None, io.Error]` | Receives a datagram and its source address. `Ok(None)` means the deadline expired. |
| `local_addr` | `local_addr() -> Result[str, io.Error]` | Returns the local address. |
| `peer_addr` | `peer_addr() -> Result[str, io.Error]` | Returns the connected peer address, when there is one. |
| `close` | `close() -> None` | Closes the socket handle. |

`net.UdpDatagram`:

| API | Signature | Contract |
| --- | --- | --- |
| `address` | `address() -> str` | Returns the source address. |
| `bytes` | `bytes() -> list[uint8]` | Returns the datagram payload as raw bytes. |
| `text` | `text() -> Result[str, io.Error]` | Decodes the payload as UTF-8 text. |

UDP preserves datagram boundaries.

- `max_bytes` must be in `1..=65535`. Zero or a larger value returns `io.Error.InvalidInput` before anything is received.
- A receive with a small buffer may truncate data, as the platform decides.
- A send larger than the host datagram limit returns `InvalidInput` where the host reports that condition.

## HTTP Server

`net.HttpListener`:

| API | Signature | Contract |
| --- | --- | --- |
| `accept` | `accept(timeout: Duration = ...) -> Result[net.HttpExchange, io.Error]` | Waits for the next HTTP request and returns an exchange. |
| `local_addr` | `local_addr() -> Result[str, io.Error]` | Returns the bound local address. |
| `close` | `close() -> None` | Closes the listener. |

`net.HttpExchange`:

| API | Signature | Contract |
| --- | --- | --- |
| `method` | `method() -> str` | Returns the request method. |
| `path` | `path() -> str` | Returns the request path. |
| `headers` | `headers() -> dict[str, str]` | Returns the request headers. |
| `body_text` | `body_text() -> Result[str, io.Error]` | Decodes the request body as UTF-8. |
| `body_bytes` | `body_bytes() -> list[uint8]` | Returns the raw request body. |
| `respond_text` | `respond_text(status: int32, text: own str, headers: own dict[str, str]) -> Result[None, io.Error]` | Consumes `text` and `headers`, and sends a text response. |
| `respond_bytes` | `respond_bytes(status: int32, bytes: own list[uint8], headers: own dict[str, str]) -> Result[None, io.Error]` | Consumes `bytes` and `headers`, and sends a byte response. |

The listener rejects malformed HTTP requests. A malformed request does not leave the listener permanently broken. Request bodies can use content-length or chunked framing.

An incoming parsed HTTP message is limited to 16 MiB of wire data and 64 headers. The listener reports an oversized or invalid request as an HTTP error where the protocol allows it.

Headers are exposed as `dict[str, str]`. That type cannot faithfully hold repeated fields, such as several `set-Cookie` lines. The current conversion can also produce duplicate equal keys internally, despite the normal `dict` rule that keys are unique. If an application needs lossless or canonical handling of repeated headers, it must not use this 0.3 high-level HTTP API.

## HTTP Client

| API | Signature | Contract |
| --- | --- | --- |
| `net.http_request_text` | `http_request_text(method: str, url: str, body: str, headers: dict[str, str]) -> Result[net.HttpResponse, io.Error]` | Sends an HTTP request with a text body. |
| `net.http_request_text_timeout` | `http_request_text_timeout(method: str, url: str, body: str, headers: dict[str, str], timeout: Duration) -> Result[net.HttpResponse, io.Error]` | Sends a text request with a deadline. |
| `net.http_request_bytes` | `http_request_bytes(method: str, url: str, bytes: list[uint8], headers: dict[str, str]) -> Result[net.HttpResponse, io.Error]` | Sends an HTTP request with a byte body. |
| `net.http_request_bytes_timeout` | `http_request_bytes_timeout(method: str, url: str, bytes: list[uint8], headers: dict[str, str], timeout: Duration) -> Result[net.HttpResponse, io.Error]` | Sends a byte request with a deadline. |

`net.HttpResponse`:

| API | Signature | Contract |
| --- | --- | --- |
| `status` | `status() -> int32` | Returns the numeric status code. |
| `reason` | `reason() -> str` | Returns the reason phrase. |
| `headers` | `headers() -> dict[str, str]` | Returns the response headers. |
| `text` | `text() -> Result[str, io.Error]` | Decodes the body as UTF-8. |
| `bytes` | `bytes() -> list[uint8]` | Returns the raw response body. |

Use the byte request and response APIs for binary payloads or an unknown encoding.

Client URLs can use `http://` or `https://`. HTTPS validates the server certificate. Responses can use content-length, chunked transfer encoding, or connection-close framing. The same limits of 16 MiB per incoming message and 64 headers apply.

The high-level HTTP helpers in 0.3 do not follow redirects, pool connections, speak HTTP/2, use proxies, decompress bodies, or take a custom-CA argument.

## WebSocket

`net.WebSocketListener`:

| API | Signature | Contract |
| --- | --- | --- |
| `accept` | `accept(timeout: Duration = ...) -> Result[net.WebSocket, io.Error]` | Waits for the next WebSocket connection. |
| `local_addr` | `local_addr() -> Result[str, io.Error]` | Returns the bound local address. |

`net.WebSocketListener` has no `close()` member in Aura 0.3. Dropping its value releases it. You cannot use it as a user-defined `with` resource yet. This is a known limitation of the resource API.

`net.WebSocket`:

| API | Signature | Contract |
| --- | --- | --- |
| `send_text` | `send_text(text: str, timeout: Duration = ...) -> Result[None, io.Error]` | Sends a text frame. |
| `send_bytes` | `send_bytes(bytes: list[uint8], timeout: Duration = ...) -> Result[None, io.Error]` | Sends a binary frame. |
| `recv_text` | `recv_text(timeout: Duration = ...) -> Result[str \| None, io.Error]` | Receives the next text or binary message, decoded as strict UTF-8. Returns `Ok(None)` on close. An empty message is present. |
| `recv_bytes` | `recv_bytes(timeout: Duration = ...) -> Result[list[uint8] \| None, io.Error]` | Receives the next text or binary message as bytes. Returns `Ok(None)` on close. |
| `close` | `close() -> None` | Closes the WebSocket. |

Use text receive when the payload must be valid UTF-8. Use byte receive otherwise.

- Messages are capped at 64 MiB.
- Individual frames and the write buffer are capped at 16 MiB.
- Cancellation of WebSocket accept, send, and receive is less complete than on the TCP and UDP scheduler APIs.
- `close()` discards host close errors.

## Unix Domain Sockets

The Unix socket APIs are available on Unix hosts.

`net.UnixListener`:

| API | Signature | Contract |
| --- | --- | --- |
| `accept` | `accept(timeout: Duration = ...) -> Result[net.UnixStream, io.Error]` | Waits for the next Unix stream connection. |
| `close` | `close() -> None` | Closes the listener. |

`net.UnixStream`:

| API | Signature | Contract |
| --- | --- | --- |
| `read_line` | `read_line(timeout: Duration = ...) -> Result[str \| None, io.Error]` | Reads one strict UTF-8 line without its trailing LF or CRLF. Returns `Ok(None)` at EOF. |
| `read_exact` | `read_exact(count: int32, timeout: Duration = ...) -> Result[list[uint8], io.Error]` | Reads exactly `count` bytes. The count must be in `1..=67108864`. |
| `write_all` | `write_all(text: str, timeout: Duration = ...) -> Result[None, io.Error]` | Writes all the text. |
| `close` | `close() -> None` | Closes the stream. |

`net.unix_listen(...)` refuses to overwrite a filesystem path that is not a socket.

## TLS

The TLS APIs read certificates and keys from PEM files. The maintained examples keep test certificates under `examples/io/certs`.

`net.TlsListener`:

| API | Signature | Contract |
| --- | --- | --- |
| `accept` | `accept(timeout: Duration = ...) -> Result[net.TlsStream, io.Error]` | Waits for a TLS connection and its handshake. |
| `local_addr` | `local_addr() -> Result[str, io.Error]` | Returns the bound local address. |
| `close` | `close() -> None` | Closes the listener. |

`net.TlsStream`:

| API | Signature | Contract |
| --- | --- | --- |
| `read_line` | `read_line(timeout: Duration = ...) -> Result[str \| None, io.Error]` | Reads one strict UTF-8 line without its trailing LF or CRLF. Returns `Ok(None)` at EOF. |
| `read_exact` | `read_exact(count: int32, timeout: Duration = ...) -> Result[list[uint8], io.Error]` | Reads exactly `count` decrypted bytes. The count must be in `1..=67108864`. |
| `write_all` | `write_all(text: str, timeout: Duration = ...) -> Result[None, io.Error]` | Writes all the text through the TLS stream. |
| `close` | `close() -> None` | Closes the TLS stream. |

The handshake and accept paths wait through the scheduler. A TLS handshake also has a hard 10-second cap, even when the caller gives no shorter timeout. Use explicit timeouts for public-facing services.

## Resource Cleanup

Network listeners and streams are owned resources. Use `with` when the lifetime is lexical:

```aura
import io
import net

def show_addr() -> Result[None, io.Error]:
    with listener = try net.listen("127.0.0.1:0"):
        print(try listener.local_addr())
    return Result.Ok(None)
```

When a resource is not scoped with `with`, call its `close()` method if the type has one. Cancellation stops Aura's wait. It cannot roll back host I/O that already finished.

## Grammar

The network module adds no grammar. Network programs use ordinary imports, calls, named arguments, `Duration` literals, `Result`, `T | None` unions, `try`, `match`, task constructs, and `with`. Addresses, URLs, server names, and Unix socket paths are runtime `str` values, not special literals.

A parameter shown with `= ...` takes its builtin default when omitted. An omitted timeout means no caller deadline, unless this page states a hard cap for the protocol. Text and byte operations are separate members. The member you call decides whether UTF-8 decoding happens.

## Typing Rules

The constructor and method signatures in every table above are normative.

- Listeners, streams, sockets, exchanges, and WebSockets are non-copy resource values.
- Fallible operations return `Result[..., io.Error]`.
- EOF, WebSocket close, and UDP receive timeout appear as the `None` member of a `T | None` success payload. This happens only where the tables say so. Empty text, empty bytes, and empty datagrams are present values.
- Timeout parameters need a `Duration`.
- Byte-count parameters are `int32`. Each API checks its own range at run time.

Text members accept or return `str` and enforce UTF-8. Byte members accept or return `list[uint8]`. HTTP headers use `dict[str, str]`.

`HttpExchange.respond_text` and `respond_bytes` consume their response body and header dictionary. Other data arguments are shared for the call, unless the signature marks them `own`.

## Runtime Semantics

Resolution, binding, and connect work run on the blocking-I/O pool. An explicit connect timeout is one end-to-end budget. Queue admission, name resolution, each resolved-address attempt, and the remaining protocol handshake all share it.

Before host work begins, the runtime rejects a timeout that is negative, cannot be represented by the host, or overflows the deadline. It returns `io.Error.InvalidInput` and never treats such a value as an omitted timeout.

- TCP, Unix, TLS, HTTP, and WebSocket wait failures return the typed errors on this page.
- A UDP receive timeout returns `Ok(None)`.
- Cancellation ends the Aura wait. Cancellation-aware operations return `io.Error.Cancelled`.
- Cancellation before the pool accepts the work prevents submission. After acceptance, the host work may finish later, and its result is discarded.

Protocol behavior:

- TCP is a byte stream. UDP preserves datagrams.
- Text reads decode strictly and remove only the line ending documented for them.
- HTTP supports content-length, chunked, and connection-close framing, within the parser caps.
- WebSocket receives whole text or binary messages. Text mode enforces UTF-8.
- TLS verifies the named peer against the configured CA file. The high-level HTTPS client verifies against the maintained Web PKI root set instead.

## Ownership And Evaluation Order

Arguments are evaluated left to right. A successful constructor or accept returns a fresh owned resource. Moving a resource invalidates the source binding.

Read and accept operations change host protocol state internally, but you call them through a shared receiver as documented. Write, send, shutdown, response, and explicit close operations need a mutable receiver place. Response bodies and header maps marked `own` move before the response operation starts.

When a type has `close()`, `with` closes the resource exactly once on every exit from the lexical scope. Cleanup cannot undo bytes already sent or host operations already finished.

`WebSocketListener` has no `close()` member, so it cannot meet the `with` resource contract. Dropping its owned value is the only way to release it today.

## Diagnostics

| Code | Cause |
| --- | --- |
| `AU2001` | Unknown network member. |
| `AU2002` | Type mismatch. |
| `AU2004` | Invalid argument binding. |
| `AU3001` | Use of a resource after it moved. |
| `AU3002` | Borrow conflict. |
| `AU3003` | A mutating network method called through an immutable place. |
| `AU2999` | Any other static rejection. |

These failures are typed `Result.Err(io.Error)` outcomes, not language diagnostics:

- DNS failures
- connection refusal
- timeouts
- invalid UTF-8
- invalid byte counts
- invalid timeout values or deadlines
- closed resources
- cancellation
- TLS verification failure
- protocol errors

An invariant failure that escapes the typed boundary uses the general runtime registry. That includes `AU4005` for a resource or I/O trap.

## Backend Support

The MIR runtime and the direct native backend implement the TCP, UDP, HTTP, WebSocket, and TLS APIs. MIR is the compiler's mid-level intermediate representation. Both backends implement Unix domain sockets on maintained Unix hosts. Both must match on timeout accounting, typed error mapping, read caps, protocol parsing, ownership, and cleanup.

Address selection, DNS answers, socket options that the host libraries choose, and exact host error messages can differ by machine. The high-level HTTPS client uses the same platform-independent Web PKI root policy in both backends.

## Limits And Implementation-Defined Behavior

| Limit | Value |
| --- | --- |
| TCP whole text reads, TCP line reads, and single byte-count reads | 64 MiB |
| TCP, Unix, and TLS exact counts | `1..=67108864` |
| UDP receive counts | `1..=65535` |
| Incoming parsed HTTP message | 16 MiB of wire data and 64 headers |
| WebSocket message | 64 MiB |
| WebSocket frame and write buffer | 16 MiB |
| TLS handshake | hard 10-second cap, plus any shorter caller deadline |

- With a smaller receive buffer, UDP truncation follows the host.
- The HTTP parser cap covers the start line, headers, transfer framing, trailers, and body. Outbound HTTP writers have no separate size cap.
- The string-dictionary header type is not lossless for repeated fields. It can currently produce duplicate equal keys internally.
- WebSocket listeners cannot be closed. WebSocket cancellation coverage is incomplete. WebSocket close discards host close errors.
- Unix sockets are unavailable on non-Unix hosts. `unix_listen` does not replace a path that is not a socket.

These are absent: redirects, connection pooling, HTTP/2, proxies, decompression, custom-CA arguments on the high-level helpers, and lossless repeated-header APIs.

## Status

Aura 0.3 implements the constructors, protocols, resources, typed errors, timeouts, cancellation behavior, scheduler integration, cleanup rules, and caps on this page.

Nonblocking descriptors stay registered with the runtime's persistent reactor, the event loop that watches for socket readiness, across scheduler turns. Timeout deadlines share the reactor's timer heap. An idle scheduler blocks until a descriptor is ready, another runtime event arrives, or the next deadline passes. It uses no periodic tick.

The fixed resource caps are an accepted design decision. So is the policy for invalid host timers.

These are known current limitations: the repeated-header representation, the missing close operation on WebSocket listeners, incomplete WebSocket cancellation, and discarded WebSocket close errors. The protocol surface is exactly the API on this page.

Design records: [ADR-0018: Fixed resource read limits](https://github.com/johnolafenwa/Aura/blob/main/architecture_docs/decisions/0018-fixed-resource-read-limits.md) and [ADR-0019: Duration conversion and timer policy](https://github.com/johnolafenwa/Aura/blob/main/architecture_docs/decisions/0019-duration-conversion-and-timer-policy.md).
