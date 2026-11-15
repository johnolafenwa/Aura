# Calling HTTP services

`checkup` asks one thing of an HTTP server: did it answer, and with what
status? Most programs that speak HTTP want more than that. They read the
body, send data of their own, follow a response as it streams in, and try
again when a server is briefly unavailable. This section covers those four
jobs with the `http` module, the client we met in
[Section 16.3](/book/ch16-03-networking).

As before, everything runs on your own machine. We need a server to call,
so we start with one that has a few more paths than `/health`.

## A service to call

Create a new package with `aura new inventory`, and put this server in
*src/server.au*. It is Listing 16-10 with more branches:

*Filename: src/server.au*

```aura
import io
import net

def answer(exchange: own net.HttpExchange, flaky_hits: mut int64) -> Result[None, io.Error]:
    with request = exchange:
        path = request.path()
        print(f"{request.method()} {path}")
        if path == "/report":
            try request.respond_text(200, "disk: ok\nmemory: ok\nqueue: 3 waiting\n", {"Content-Type": "text/plain"})
        elif path == "/old-report":
            try request.respond_text(302, "", {"Location": "/report"})
        elif path == "/notes":
            body = try request.body_text()
            try request.respond_text(201, f"stored {body.len()} characters\n", {"Content-Type": "text/plain"})
        elif path == "/flaky":
            flaky_hits += 1
            if flaky_hits % 3 == 0:
                try request.respond_text(200, "finally\n", {"Content-Type": "text/plain"})
            else:
                try request.respond_text(503, "busy\n", {"Content-Type": "text/plain"})
        else:
            try request.respond_text(404, "not found\n", {"Content-Type": "text/plain"})
    return Result.Ok(None)

def serve(address: str) -> Result[None, io.Error]:
    mut flaky_hits = 0
    with listener = try net.http_listen(address):
        print(f"listening on {try listener.local_addr()}")
        while true:
            try answer(try listener.accept(), flaky_hits)
    return Result.Ok(None)

def main() -> int32:
    match serve("127.0.0.1:8081"):
        case Result.Ok(_):
            return 0
        case Result.Err(error):
            print(f"server failed: {error}")
            return 1
```

<span class="caption">Listing 16-15: A small service with a report, a redirect, a place to post notes, and an unreliable path</span>

`/report` returns three lines of text. `/old-report` redirects to it.
`/notes` accepts a request body and says how long it was. `/flaky` answers
`503 Service Unavailable` twice out of every three requests, using a counter
that `serve` owns and lends to `answer` with `mut`. Start it in one terminal
and leave it running:

```console
$ aura run src/server.au
listening on 127.0.0.1:8081
```

## Reading a response

In a second terminal, write the client in *src/main.au*:

*Filename: src/main.au*

```aura
import http
import io

def show_report(url: str) -> Result[None, io.Error]:
    client = http.Client(timeout=2s, read_timeout=2s)
    with response = try client.get(url):
        print(f"{response.status} {response.reason} from {response.url}")
        print(response.header("content-type"))
        if not response.ok():
            return Result.Err(io.Error.Other(f"unexpected status {response.status}"))
        body = try response.text()
        print(body.trim_end())
    return Result.Ok(None)

def main():
    print(show_report("http://127.0.0.1:8081/report"))
    print(show_report("http://127.0.0.1:8081/old-report"))
    print(show_report("http://127.0.0.1:8081/nothing"))
```

<span class="caption">Listing 16-16: Reading the status, a header, and the body of a response</span>

```console
$ aura run src/main.au
200 OK from http://127.0.0.1:8081/report
text/plain
disk: ok
memory: ok
queue: 3 waiting
Result.Ok(None)
200 OK from http://127.0.0.1:8081/report
text/plain
disk: ok
memory: ok
queue: 3 waiting
Result.Ok(None)
404 Not Found from http://127.0.0.1:8081/nothing
text/plain
Result.Err(io.Error.Other(unexpected status 404))
```

An `http.Client` is a bundle of settings. `timeout` limits the wait for the
response to begin, and `read_timeout` limits each read of the body after
that. Neither has a default, so a client built with `http.Client()` waits
forever, and so do the module-level shortcuts `http.get` and `http.post`.
Set both whenever the server is not yours.

`get` returns as soon as the status line and headers have arrived. At that
point the `http.Response` knows its `status`, `reason`, and headers, and the
body is still on the wire. `header(name)` looks a header up without regard
to case and returns `str | None`. `ok()` is true for statuses 200 to 299.
Remember from `checkup` that a `404` is a response, not an error. `Err`
means no response arrived at all, so if a bad status should count as a
failure in your program, you say so, as `show_report` does.

The second call asked for `/old-report` and got the report. The client
followed the `302` for us, up to ten redirects by default, and
`response.url` shows where the response finally came from. Pass
`max_redirects=0` to see redirect responses yourself.

`text()` reads the rest of the body as UTF-8. It takes `mut self`, because
reading advances through the body, and that is why the response is bound
by `with`: a `with` binding can be mutated, and the block closes the
response on every path out, including the early `return`. `text()` refuses
a body over 64 MiB, and `text_up_to(max_bytes)` sets a smaller bound. For
bytes there are `bytes()` and `bytes_up_to`.

There is a small economy hiding here. A response owns its connection until
its body has been read to the end. Then the connection goes back to a pool
that the whole process shares, and the next request to the same host reuses
it without a new TCP handshake. A response that is closed before its body
is finished cannot be reused, so its connection is closed. Reading bodies
to the end is polite to the server and faster for you.

## Sending data

`post` sends a text body, and takes the headers as a dictionary:

*Filename: src/main.au*

```aura
import http
import io

def store_note(url: str, note: str) -> Result[str, io.Error]:
    headers = {"Content-Type": "text/plain", "Authorization": "Bearer demo-token"}
    with response = try http.Client(timeout=2s).post(url, note, headers):
        if response.status != 201:
            return Result.Err(io.Error.Other(f"unexpected status {response.status}"))
        return response.text()

def main():
    print(store_note("http://127.0.0.1:8081/notes", "remember the milk"))
    print(store_note("http://127.0.0.1:8081/report", "remember the milk"))
    print(http.Client(timeout=2s).get("http://127.0.0.1:9/"))
    print(http.get("ftp://127.0.0.1/"))
```

<span class="caption">Listing 16-17: Posting a body with headers</span>

```console
$ aura run src/main.au
Result.Ok(stored 17 characters
)
Result.Err(io.Error.Other(unexpected status 200))
Result.Err(io.Error.ConnectionRefused)
Result.Err(io.Error.InvalidInput)
```

The client adds `Host`, `Content-Length`, and a few other headers itself,
and a header you pass replaces the one it would have added. For other
methods there is `request(method, url, body, headers)`, and
`request_bytes` takes the body as a `list[uint8]`. To send JSON, encode it
with `json.dumps` from [Section 16.4](/book/ch16-04-json) and set
`Content-Type` to `application/json`.

The last two lines show the two ways a request can fail before it is
answered. Nothing listens on port 9, so the connection is refused. And a URL
that does not start with `http://` or `https://` is rejected as invalid
input before anything is sent. `https://` URLs verify the server's
certificate against a built-in set of public roots. A client for a server
with a private certificate authority names it with
`http.Client(ca_file="ca.pem")`.

## Following a stream

Some responses are not documents. They are streams: a build log that grows
while the build runs, or a language model's reply arriving a few words at a
time. The server keeps the connection open and writes as things happen, and
the client should show each piece when it arrives, not when the whole
response ends.

The usual format for this is *server-sent events*. The body is plain text
in which each event is a few `name: value` lines followed by a blank line.
Our `net.http_listen` server can only send a complete body, so to produce a
slow stream we drop down a level and write the HTTP response by hand over
TCP. Put this in *src/ticker.au*:

*Filename: src/ticker.au*

```aura
import io
import net

def stream_events(connection: own net.TcpStream) -> Result[None, io.Error]:
    with stream = connection:
        while true:
            match try stream.read_line(timeout=5s):
                case str as line:
                    if line == "":
                        break
                case None:
                    return Result.Ok(None)
        try stream.write_all("HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nConnection: close\r\n\r\n", timeout=5s)
        for step in range(1, 4):
            sleep(300ms)
            try stream.write_all(f"event: progress\ndata: step {step} of 3\n\n", timeout=5s)
        try stream.write_all("data: finished\n\n", timeout=5s)
    return Result.Ok(None)

def serve(address: str) -> Result[None, io.Error]:
    with listener = try net.listen(address):
        print(f"ticking on {try listener.local_addr()}")
        while true:
            try stream_events(try listener.accept())
    return Result.Ok(None)

def main() -> int32:
    match serve("127.0.0.1:8082"):
        case Result.Ok(_):
            return 0
        case Result.Err(error):
            print(f"ticker failed: {error}")
            return 1
```

<span class="caption">Listing 16-18: A hand-written HTTP response that sends one event every 300 milliseconds</span>

`stream_events` reads the request line and headers up to the blank line
that ends them and ignores what they say. It writes a status line and two
headers, and then three `progress` events with a pause before each, and a
final event with no name. It is the least HTTP that a client will accept,
which is fine for a test fixture and nothing more. Start it in a third
terminal with `aura run src/ticker.au`.

The client reads events with `read_event`:

*Filename: src/main.au*

```aura
import http
import io
import time

def follow(url: str) -> Result[int64, io.Error]:
    started = time.monotonic()
    mut count = 0
    with stream = try http.Client(timeout=2s, read_timeout=5s).get(url):
        while true:
            event = try stream.read_event()
            if event is None:
                break
            count += 1
            waited = (time.monotonic() - started).to_ms() as int64
            print(f"{waited} ms  {event.event}: {event.data}")
    return Result.Ok(count)

def main():
    print(follow("http://127.0.0.1:8082/"))
```

<span class="caption">Listing 16-19: Printing server-sent events as they arrive</span>

```console
$ aura run src/main.au
301 ms  progress: step 1 of 3
601 ms  progress: step 2 of 3
902 ms  progress: step 3 of 3
902 ms  message: finished
Result.Ok(4)
```

The times in the left column are the proof that this is a stream: each
event was printed about 300 milliseconds after the one before, as the
server wrote it, and not all together at the end. Your times will differ by
a few milliseconds.

`read_event` returns the next `http.Event`, or `None` when the server
closes the stream. An event has an `event` name, its `data`, an `id`, and
an optional `retry` delay. The method does the format's bookkeeping: it
joins the `data` lines of one event, skips comment lines, and names an
event that has no `event` field `"message"`, which is what happened to our
last one.

Now the two timeouts earn their separate names. `timeout=2s` covers getting
the response started. `read_timeout=5s` applies to each read of the body, so
this client tolerates a stream that goes on for an hour as long as it never
goes quiet for more than five seconds. Set `read_timeout` below the
ticker's 300 milliseconds and the first `read_event` returns
`io.Error.TimedOut`.

Events are one of several ways to read a body in pieces. `read_line`
returns one line at a time, `read_lines(max_lines)` returns the lines that
have already arrived in one call, and `read_chunk(max_bytes)` returns raw
bytes, which is how you download a large file without holding it in memory.
They can be mixed, and each continues where the last one stopped. The
repository's `examples/agents/streaming_agent` is a complete program built
on `read_event`: it prints a model's reply as it arrives and runs the tools
the model asks for.

## Trying again

Networks fail in passing ways. A server restarts, a load balancer drops a
connection, a service answers `503` for a second while it catches up. A
client that gives up at the first of these is fragile, and one that retries
everything forever is a nuisance. The `control` module has a helper for the
middle ground. `control.retry` calls a function until it returns `Ok`, up
to a number of attempts, waiting longer before each new attempt.

The catch is the one from Listing 16-16: to `retry`, a failure is an `Err`
and nothing else, and to the HTTP client a `503` is an `Ok`. So the function
we hand to `retry` has to translate. That translation is where your
program's retry policy lives:

*Filename: src/main.au*

```aura
import control
import http
import io

enum Fetched:
    Body(text: str)
    Missing

def fetch_once(url: str) -> Result[Fetched, io.Error]:
    with response = try http.Client(timeout=2s).get(url):
        print(f"  got {response.status}")
        if response.status == 404:
            return Result.Ok(Fetched.Missing)
        if response.status >= 500:
            return Result.Err(io.Error.Other(f"server said {response.status}"))
        return Result.Ok(Fetched.Body(try response.text()))

def fetch(url: own str) -> Result[Fetched, io.Error]:
    return control.retry(lambda: fetch_once(url), max_attempts=4, initial_backoff=100ms)

def main():
    print(fetch("http://127.0.0.1:8081/flaky"))
    print(fetch("http://127.0.0.1:8081/nothing"))
    print(fetch("http://127.0.0.1:9/"))
```

<span class="caption">Listing 16-20: Retrying server errors and connection failures, but not a `404`</span>

```console
$ aura run src/main.au
  got 503
  got 503
  got 200
Result.Ok(Fetched.Body(finally
))
  got 404
Result.Ok(Fetched.Missing)
Result.Err(io.Error.ConnectionRefused)
```

`fetch_once` makes one attempt and sorts the outcome into three kinds. A
status of 500 or above becomes an `Err`, so it will be retried. A `404`
becomes `Ok(Fetched.Missing)`: the server answered clearly, asking again
will not change its mind, and the caller gets a value it can match on. A
connection failure is already an `Err` and passes through `try`.

`control.retry` takes a function of no arguments that returns a `Result`.
The lambda captures `url` by value, as [Chapter 13](/book/ch13-01-closures)
explained, and a function cannot hand a closure a string it only has shared
access to. That is why `fetch` takes `own str`. `max_attempts`
counts the first try, so four attempts means at most three retries.
`initial_backoff` is the wait before the second attempt, and each later
wait is double the one before: 100, 200, and 400 milliseconds here.

The first call met two `503` responses and succeeded on the third attempt,
after waiting 300 milliseconds in all. The second call was answered `404`
and returned at once. The third had nothing to connect to, so it tried four
times, waited 700 milliseconds in between, and returned the error from the
last attempt.

`retry` is deliberately plain. Every `Err` is retried, and there is no
jitter, the random variation in delays that keeps many clients from
retrying in step. [Chapter 20](/book/ch20-04-randomness) shows how to
compute jittered delays with a seeded generator, and a loop with `sleep`
is all you need to apply them. Think also about what you are repeating.
Asking for a report twice is harmless. Repeating a `POST` that creates an
order may create two, unless the server offers a way to recognize the
duplicate.

## What the client does not do

The client speaks HTTP/1.1, with redirects, connection reuse, and gzip and
deflate decompression handled for you. It does not speak HTTP/2 or HTTP/3,
it does not use a proxy, and it does not keep cookies between requests. A
request body is sent whole and cannot be streamed, though a response body
can be read in pieces as we saw. The [HTTP Module](/manual/http) page lists
the exact limits.

HTTP is the protocol most programs need. The last section of this chapter
is a short visit to the other four that the `net` module provides.
