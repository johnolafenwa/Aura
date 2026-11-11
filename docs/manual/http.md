# HTTP Module

The `http` module is the HTTP client. A request returns an `http.Response`
as soon as the status and headers have arrived. The body stays on the
connection and the program reads it whole, in chunks, by line, or as
server-sent events. The client follows redirects, keeps connections for
reuse, and decodes gzip and deflate bodies.

```aura
import http
import io

def fetch(url: str) -> Result[str, io.Error]:
    mut response = try http.get(url)
    if not response.ok():
        return Result.Err(io.Error.Other(f"status {response.status}"))
    return response.text()
```

The HTTP server is in the [`net` module](/manual/network).

## Public API

| API | Signature | Result |
| --- | --- | --- |
| `http.get` | `get(url: str) -> Result[http.Response, io.Error]` | Sends a GET request. |
| `http.post` | `post(url: str, body: str, headers: dict[str, str]) -> Result[http.Response, io.Error]` | Sends a POST request with a text body. |
| `http.request` | `request(method: str, url: str, body: str, headers: dict[str, str]) -> Result[http.Response, io.Error]` | Sends a request with a text body. |
| `http.request_bytes` | `request_bytes(method: str, url: str, body: list[uint8], headers: dict[str, str]) -> Result[http.Response, io.Error]` | Sends a request with a byte body. |
| `http.idle_connections` | `idle_connections() -> int64` | The finished connections kept for reuse. |
| `http.close_idle_connections` | `close_idle_connections() -> None` | Closes every kept connection. |
| `http.Client` | `Client(timeout=None, read_timeout=None, max_redirects=10, decompress=true, keep_alive=true, ca_file="")` | The settings of a request. It has the methods `get`, `post`, `request`, and `request_bytes` with the signatures above. |

The module functions use the default settings. They are the same as the
methods of `http.Client()`.

`http.Response`:

| API | Signature | Result |
| --- | --- | --- |
| `status` | `status: int64` | The status code. |
| `reason` | `reason: str` | The reason phrase. When the server sends none, the standard phrase of the status. |
| `url` | `url: str` | The URL the response came from, after any redirects. |
| `ok` | `ok(self) -> bool` | Whether the status is from 200 to 299. |
| `header` | `header(self, name: str) -> str \| None` | The value of a header. The name is compared without regard to case. A header sent more than once has its values joined with `", "`. |
| `header_values` | `header_values(self, name: str) -> list[str]` | Every value of a header, in the order sent. |
| `headers` | `headers(self) -> dict[str, str]` | All headers, with names in lower case. |
| `text` | `text(mut self) -> Result[str, io.Error]` | The rest of the body as UTF-8 text. |
| `bytes` | `bytes(mut self) -> Result[list[uint8], io.Error]` | The rest of the body. |
| `text_up_to` | `text_up_to(mut self, max_bytes: int64) -> Result[str, io.Error]` | The rest of the body as text, when it is at most `max_bytes` long. |
| `bytes_up_to` | `bytes_up_to(mut self, max_bytes: int64) -> Result[list[uint8], io.Error]` | The rest of the body, when it is at most `max_bytes` long. |
| `read_chunk` | `read_chunk(mut self, max_bytes: int64) -> Result[list[uint8] \| None, io.Error]` | The next bytes of the body, or `None` at its end. |
| `read_line` | `read_line(mut self) -> Result[str \| None, io.Error]` | The next line without its line ending, or `None` at the end. |
| `read_lines` | `read_lines(mut self, max_lines: int64) -> Result[list[str], io.Error]` | The next lines, at most `max_lines`. An empty list is the end. |
| `read_event` | `read_event(mut self) -> Result[http.Event \| None, io.Error]` | The next server-sent event, or `None` at the end. |
| `close` | `close(mut self) -> None` | Releases the connection. |

`http.Event` has the fields `event: str`, `data: str`, `id: str`, and
`retry: Duration | None`.

## Requests

A URL starts with `http://` or `https://`. HTTPS checks the server
certificate against the built-in roots, or against the PEM file named by
`ca_file` when it is not empty.

The client adds `Host`, `User-Agent`, `Accept`, `Accept-Encoding`,
`Connection`, and `Content-Length`. A header the caller gives replaces
the one the client would add. A method that is not an HTTP token, a header
with a line break, and a URL the client cannot parse all fail with
`io.Error.InvalidInput` before anything is sent.

A status such as 404 or 500 is a response, not an error. `Result.Err` means
that no response arrived.

## Settings

| Field | Default | Meaning |
| --- | --- | --- |
| `timeout` | `None` | The limit from the start of a request to the arrival of the response head, redirects included. `None` waits without limit. |
| `read_timeout` | `None` | The limit of each read of the response body. |
| `max_redirects` | `10` | The redirects followed before the request fails. `0` returns each redirect response to the caller. |
| `decompress` | `true` | Ask for gzip and deflate bodies and decode them. |
| `keep_alive` | `true` | Keep finished connections and use them for later requests. |
| `ca_file` | `""` | A PEM file of certificate authorities to trust instead of the built-in roots. |

```aura
import http
import io

def fetch_status(url: str) -> Result[int64, io.Error]:
    client = http.Client(timeout=2s, max_redirects=0)
    response = try client.get(url)
    return Result.Ok(response.status)
```

A request that passes its limit fails with `io.Error.TimedOut`. A request
in a cancelled task fails with `io.Error.Cancelled`.

## Reading The Body

`text` and `bytes` read the rest of the body. They fail with
`io.Error.InvalidData` when the body is larger than 64 MiB. `text_up_to`
and `bytes_up_to` take another limit, up to 256 MiB. A response that
declares a longer body fails before any of it is read.

`read_chunk` returns the bytes that have arrived, at most `max_bytes`, and
waits only when none have. `read_line` accepts `\n` and `\r\n` line
endings. `read_lines` waits for one line and returns it with the lines that
have already arrived, so one call hands over many lines when the body
arrives quickly and still returns each line promptly when it arrives
slowly.

```aura
import http
import io

def count_lines(url: str) -> Result[int64, io.Error]:
    mut total = 0
    with response = try http.get(url):
        while true:
            lines = try response.read_lines(1024)
            if lines.len() == 0:
                break
            total += lines.len()
    return Result.Ok(total)
```

The reading methods can be mixed. Each one continues where the last one
stopped.

## Server-Sent Events

`read_event` reads a `text/event-stream` body one event at a time. It
joins the `data` lines of an event with newlines, names an event without an
`event` field `"message"`, keeps the last `id` the stream sent, and skips
comment lines. An event without `data` is not returned. Lines must end with
`\n` or `\r\n`.

```aura
import http
import io

def print_events(url: str) -> Result[None, io.Error]:
    with stream = try http.get(url):
        while true:
            event = try stream.read_event()
            if event is None:
                break
            print(f"{event.event}: {event.data}")
    return Result.Ok(None)
```

The
[reference streaming agent](https://github.com/johnolafenwa/Aura/tree/main/examples/agents/streaming_agent)
reads a model's reply this way. It prints the text as it arrives, runs the
tool the model calls, and sends the result back.

## Connections

A response owns its connection until the body has been read to its end.
The connection then goes back to the client for the next request to the
same scheme, host, port, and `ca_file`. Closing or dropping a response
before the end of its body closes the connection, because the unread part
cannot be skipped safely. `with` closes the response at the end of its
block.

The kept connections belong to the process and are shared by every task.
At most 8 are kept for one host and 64 in all, and one that has been idle
for 90 seconds is closed. When a kept connection turns out to be closed by
the server, the client opens a new one and sends the request again. It
does this only when no part of a response arrived, and only for a request
that failed while being written or whose method is safe to repeat (`GET`,
`HEAD`, `PUT`, `DELETE`, `OPTIONS`, `TRACE`).

`keep_alive=false` sends `Connection: close` and never keeps the
connection.

## Redirects

The client follows statuses 301, 302, 303, 307, and 308 that carry a
`Location` header. After 303, and after 301 or 302 in answer to a `POST`,
the next request is a `GET` without a body. After 307 and 308 the method
and body are kept. When the redirect leads to another scheme, host, or
port, the client drops the `Authorization`, `Cookie`, and
`Proxy-Authorization` headers. A request that meets more than
`max_redirects` redirects fails with `io.Error.Other`.

## Compression

The client sends `Accept-Encoding: gzip, deflate` and decodes a body with
either `Content-Encoding`. The reading methods return decoded bytes, and
the size limits count decoded bytes. With `decompress=false`, or when the
caller sets `Accept-Encoding`, the body is returned as sent.

## Limits

- The client speaks HTTP/1.1. It does not speak HTTP/2 or HTTP/3.
- It does not use proxies and does not keep cookies.
- A request body is sent whole. It cannot be streamed.
- A response head is limited to 256 KiB and 64 headers, and one line of a
  body to 64 MiB.
- Server-sent event lines that end with a lone `\r` are not supported.
