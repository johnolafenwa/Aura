# ADR-0080: The `http` client module

- Status: Accepted
- Date: 2026-11-11
- Builds on: ADR-0057, ADR-0073
- Replaces: the `net.http_request_*` functions and `net.HttpResponse`

## Context

The HTTP client was four functions in `net`. Each opened a connection, sent
one request, read the whole response into memory under a 16 MiB cap, and
closed the connection. It did not follow redirects, decode compressed
bodies, or take a certificate authority file. A program could not read a
long download in pieces or a model reply as it was produced, and every
request to the same server paid for a new connection and, over HTTPS, a new
handshake. This is the third step of the streaming I/O and standard-library
work.

## Decision

### A module of its own, written mostly in Aura

The client is the builtin module `http`. Its public types, `Client`,
`Response`, and `Event`, are Aura classes in `std/http.au`. The runtime
supplies ten host functions (`http.__aura_send`, the body readers, and the
pool functions) and one opaque resource type, `http.Stream`, which
`Response` holds in a private field. Header lookup, the settings, and the
shape of an event are ordinary Aura code. New settings are new fields with
defaults, which does not break callers.

The old functions and `net.HttpResponse` are removed (ADR-0057). `net`
keeps the HTTP server.

### The response is a stream

`__aura_send` returns when the response head has arrived. The body is read
from the connection on demand by `read_chunk`, `read_line`, `read_lines`,
`read_event`, or the whole-body `text` and `bytes`. The framing decoder
(content-length, chunked with trailers, and until-close) and the optional
gzip or deflate decoder work on a block at a time, so memory use does not
grow with the body. Only the whole-body reads have a size limit: 64 MiB by
default and 256 MiB at most, counted after decoding.

`read_lines(max)` waits for one line and returns it with the lines that
have already arrived. A fast body is handed over in large batches, and a
slow one is not held back to fill a batch.

`read_event` implements the server-sent event format: `data` lines joined
with newlines, the default event name `message`, the persistent last `id`,
`retry`, comments, and the rule that an event without data is not
dispatched. A lone carriage return as a line ending is not supported.

### Errors

A request fails with `io.Error`, like the rest of the I/O library. A status
of 400 or above is a response and not an error, because the caller often
needs its body.

### Connection reuse

The process keeps one pool of idle connections keyed by scheme, host, port,
and certificate authority file. At most 8 are kept per key and 64 in all,
and one idle for 90 seconds is closed. A response returns its connection
only when the body was read to its end and both sides allow reuse.
Dropping a response early closes the connection, since the unread
remainder cannot be skipped at a known cost.

A pooled connection can have been closed by the server. The client checks
for that before use, and when the first write or read on a reused
connection still fails with nothing received, it sends the request once
more on a new connection. It does so only when the request was not written
in full or the method is idempotent, so a `POST` is never sent twice.

### Redirects

The client follows 301, 302, 303, 307, and 308, at most `max_redirects`
times (default 10). 303 always, and 301 and 302 after a `POST`, turn the
request into a `GET` without a body. A redirect to another origin drops
the `Authorization`, `Cookie`, and `Proxy-Authorization` headers.

### Timeouts and cancellation

`timeout` bounds the time from the start of the request to the arrival of
the final response head. `read_timeout` bounds each body read. Both are
`Duration | None` fields. Every wait also observes the cancellation of the
current task.

### A fix this needed

`with` rejected a class declared in another module with `AU2017`, because
the check looked the class up by its unqualified name. It now resolves the
name the way other uses of a class do.

## Consequences

- Programs that called `net.http_request_*` must change to `http`.
- A response is no longer a snapshot. Its reading methods take `mut self`,
  and it holds a connection until it is read to the end, closed, or
  dropped.
- Header names are reported in lower case, and repeated headers are
  available through `header_values`.
- Not done: HTTP/2, proxies, cookies, streamed request bodies.
