# Networking and HTTP

The third probe in `checkup` asks a question over the network: does this URL
answer, and with what status? Before we write it, we need something to
check. We will build up to that in two steps. First we write a tiny TCP
server and client in one program, which shows how sockets fit with tasks.
Then we write a small HTTP server as its own program, and point `checkup` at
it. Everything in this section runs on `127.0.0.1`, so you can follow along
without internet access.

## A TCP conversation

TCP gives two programs a reliable stream of bytes between them. One side
listens on an address; the other connects to it. The `net` module calls these
a `net.TcpListener` and a `net.TcpStream`. Both are resources, so they go in
`with` blocks, and their operations return `Result[..., io.Error]`, the same
error type `fs` uses.

We want the server and the client in one program, which means they have to
run at the same time: the server is blocked in `accept` while the client
connects. That calls for a task group from
[Chapter 15](/book/ch15-00-concurrency). The natural first attempt is to
create the listener in `main` and hand it to a server task:

```aura check-fail:AU3008
import io
import net

def serve(listener: own net.TcpListener) -> Result[None, io.Error]:
    with server = listener:
        with stream = try server.accept(timeout=5s):
            try stream.write_all("hello\n", timeout=5s)
    return Result.Ok(None)

def main():
    with TaskGroup() as group:
        listener = net.listen("127.0.0.1:0")
        match own listener:
            case Result.Ok(server):
                group.start(serve, server)
            case Result.Err(error):
                print(error)
```

```console
$ aura run handoff.au
error[AU3008]: task argument `listener` cannot cross a task boundary because `net.TcpListener` is a host resource and is not Transfer
 --> handoff.au:15:36
   |
15 |                 group.start(serve, server)
   |                                    ^
  = related handoff.au:4:11: task parameter `listener` is declared here
  = help: send owned data made only from Transfer components; keep capabilities and host resources on their owning worker
```

Values that move into a task must be `Transfer`, and a live socket is not.
The help line puts the rule in one sentence: send plain owned data between
tasks, and keep host resources on the worker that owns them. A listener,
stream, file, or child process is a handle to state inside the operating
system and the runtime, not data, and Aura keeps each one with the task that
opened it. The checker enforces that at compile time, so a socket can never
be shared between two tasks by accident.

So we turn the program inside out. The server task creates its own listener,
and tells `main` where it is listening through a queue. A `Queue[str]` handle
is a copy value and can cross the task boundary, and the address is an owned
`str`, which is `Transfer`:

```aura
import io
import net

def serve(addresses: Queue[str]) -> Result[None, io.Error]:
    with listener = try net.listen("127.0.0.1:0"):
        addresses.put(try listener.local_addr())
        with stream = try listener.accept(timeout=5s):
            match try stream.read_line(timeout=5s):
                case str as name:
                    try stream.write_all(f"hello, {name}\n", timeout=5s)
                case None:
                    pass
    return Result.Ok(None)

def ask(address: str, name: str) -> Result[str, io.Error]:
    with stream = try net.connect_timeout(address, 5s):
        try stream.write_all(f"{name}\n", timeout=5s)
        match own try stream.read_line(timeout=5s):
            case str as reply:
                return Result.Ok(reply)
            case None:
                return Result.Err(io.Error.UnexpectedEof)

def main() -> int32:
    with TaskGroup() as group:
        addresses = Queue[str](capacity=1)
        server = group.start(serve, addresses)
        match own addresses.get(timeout=5s):
            case QueueReceive.Item(address):
                print(f"server listening on {address}")
                print(ask(address, "Ada"))
            case _:
                print("server never started")
                return 1
        print(server.result())
    return 0
```

<span class="caption">Listing 16-9: A TCP server task and a client in one program</span>

```console
$ aura run hello_tcp.au
server listening on 127.0.0.1:35615
Result.Ok(hello, Ada)
TaskResult.Ready(Result.Ok(None))
```

Your port number will be different. Listening on port `0` asks the operating
system for any free port, and `local_addr()` tells us which one we got. That
is the standard trick for tests and examples: no two runs fight over a fixed
port.

Follow the conversation. The server binds, publishes its address, and waits
in `accept` for up to five seconds. `main` takes the address off the queue
and calls `ask`, which connects, writes `Ada` and a newline, and waits for a
line back. The server's `read_line` returns `Result[str | None, io.Error]`,
the shape you have now seen for terminal input and process pipes: a line, a
clean end of stream (`None`), or a failure. It writes the greeting, its `with`
blocks close the stream and then the listener, and its task finishes with
`Ok(None)`, which `server.result()` reports.

Every wait in this program has a timeout, and that is a habit worth forming.
A network peer can stop responding at any point without closing anything.
Without a deadline, `accept`, `read_line`, and `write_all` would wait forever.
With one, a TCP wait that expires returns `io.Error.TimedOut`, and the program
decides what that means. If the task group is cancelled while a task waits on
a socket, the wait returns `io.Error.Cancelled` promptly, so shutting down a
group of network tasks does not hang either.

When a server needs to handle connections one after another, the task that
owns the listener calls an ordinary function for each accepted stream,
passing the stream with `own`. That is the pattern our HTTP server will use.

## An HTTP server to check against

HTTP is the protocol most health checks speak. The `net` module has a
high-level HTTP listener: `net.http_listen` binds an address, and each
`accept` returns a `net.HttpExchange`, one request plus the means to answer
it. Put this in *src/server.au* in the `checkup` package:

*Filename: src/server.au*

```aura
import io
import net

def answer(exchange: own net.HttpExchange) -> Result[None, io.Error]:
    with request = exchange:
        path = request.path()
        print(f"{request.method()} {path}")
        if path == "/health":
            try request.respond_text(200, "ok\n", {"Content-Type": "text/plain"})
        else:
            try request.respond_text(404, "not found\n", {"Content-Type": "text/plain"})
    return Result.Ok(None)

def serve(address: str) -> Result[None, io.Error]:
    with listener = try net.http_listen(address):
        print(f"listening on {try listener.local_addr()}")
        while true:
            try answer(try listener.accept())
    return Result.Ok(None)

def main() -> int32:
    match serve("127.0.0.1:8080"):
        case Result.Ok(_):
            return 0
        case Result.Err(error):
            print(f"server failed: {error}")
            return 1
```

<span class="caption">Listing 16-10: A minimal HTTP server with a `/health` endpoint</span>

`answer` takes the exchange with `own` and binds it in a `with`, so the
connection is closed after the response whatever happens. `method()` and
`path()` describe the request; `headers()` and `body_text()` are there too
when you need them. `respond_text` takes a status code, a body, and a header
dictionary, and consumes the body and headers, because they are handed to the
connection. This server knows one path and answers everything else with
`404`. Here the `accept` has no timeout on purpose: a server's job is to wait.

Start it in one terminal:

```console
$ aura run src/server.au
listening on 127.0.0.1:8080
```

From a second terminal, `curl` can check it:

```console
$ curl -s -i http://127.0.0.1:8080/health
HTTP/1.1 200 OK
Content-Type: text/plain
Content-Length: 3
Connection: close

ok
```

The listener added `Content-Length` and `Connection: close` for us. If you try
to start a second copy while the first is running, it fails at the bind:

```console
$ aura run src/server.au
server failed: io.Error.AddrInUse
```

## The HTTP probe

Now the client side. `net.http_request_text_timeout` sends one request and
returns a `net.HttpResponse`. Add `import net` to *src/main.au* and write
`fetch`:

```aura fragment
def fetch(url: str) -> str:
    headers: dict[str, str] = {}
    match own net.http_request_text_timeout("GET", url, "", headers, 2s):
        case Result.Ok(response):
            status = response.status()
            if status >= 200 and status < 400:
                return "ok"
            return f"failed (HTTP {status})"
        case Result.Err(error):
            return f"error ({error})"
```

<span class="caption">Listing 16-11: Checking a URL over HTTP</span>

and point the last arm of `run_check` at it:

```aura fragment
        case Probe.Http(url):
            return fetch(url)
```

The arguments are the method, the URL, a text body (empty for a `GET`), a
header dictionary, and the deadline. The empty dictionary needs its type
written out, because there is nothing in `{}` for the compiler to infer it
from. There are four client functions: text or byte bodies, each with and
without a timeout. For a check, always use a timeout.

Notice the two different ways a check can fail here, as with commands. A
response with status `404` or `500` is a successful request that brought bad
news, and we report it from the `Ok` arm. A refused connection, a DNS failure,
or a timeout means we never got a response at all, and that comes back as an
`io.Error`. Keeping those apart is the difference between "the service is up
and says it is unhealthy" and "the service is not there".

Add three HTTP lines to *checks.txt*, two for our server and one for a port
where nothing is listening. Drop the slow command checks from the last
section so the output stays short:

*Filename: checks.txt*

```text
# name    kind  target
config    file  checks.txt
notes     file  notes.txt
greeting  cmd   /bin/echo hello
web       http  http://127.0.0.1:8080/health
admin     http  http://127.0.0.1:8080/admin
metrics   http  http://127.0.0.1:9090/metrics
```

With the server stopped, every HTTP check fails to connect:

```console
$ aura run src/main.au
config: ok
notes: missing
greeting: ok
web: error (io.Error.ConnectionRefused)
admin: error (io.Error.ConnectionRefused)
metrics: error (io.Error.ConnectionRefused)
```

Start the server in the other terminal and run `checkup` again:

```console
$ aura run src/main.au
config: ok
notes: missing
greeting: ok
web: ok
admin: failed (HTTP 404)
metrics: error (io.Error.ConnectionRefused)
```

and the server's terminal shows the two requests it answered:

```console
$ aura run src/server.au
listening on 127.0.0.1:8080
GET /health
GET /admin
```

A name that does not resolve fails before any connection is made. With the
URL `http://no-such-host.invalid/`, `fetch` reports:

```text
error (io.Error.Other(failed to lookup address information: Name or service not known))
```

The message after `Other` comes from the host resolver and varies between
systems.

## Timeouts, DNS, and the blocking pool

The two-second timeout we passed to `fetch` covers the whole request:
resolving the host name, trying each address DNS returned, connecting, and
the HTTP exchange. It is one budget, not two seconds per step, so a host with
five unreachable addresses still gives up after two seconds.

Some of that work cannot be done without blocking an operating-system thread.
Host name lookup in particular goes through the C library's resolver, which
offers no way to wait for it without a thread. If that happened on a
scheduler worker, one slow DNS server could freeze every Aura task pinned to
the same worker. So Aura sends name resolution, binding, and blocking connect
calls to a separate pool of threads and parks the waiting task until the pool
answers. If the deadline passes or the task is cancelled before the pool
picks up the job, the job is never started. Once a pool thread has started a
lookup it cannot be interrupted, so Aura stops waiting and throws away the
late answer.

You rarely need to think about the pool, but two environment variables tune
it: `AURA_BLOCKING_WORKERS` sets the exact thread count (the default is
derived from the number of cores, between 2 and 8), and
`AURA_BLOCKING_QUEUE_CAPACITY` bounds how many accepted jobs may wait. A
program that resolves thousands of names at once is where these start to
matter.

## What the HTTP helpers do not do

The HTTP client in Aura 0.11 is deliberately small. Each call makes one
request on a fresh connection. It does not follow redirects, pool
connections, speak HTTP/2, use a proxy, or decompress bodies. `https://` URLs
work and verify the server's certificate against the standard public root
set. Headers are a `dict[str, str]`, which cannot represent a header that
appears more than once, such as several `Set-Cookie` lines, so do not use
these helpers where that matters. Incoming messages are limited to 16 MiB and
64 headers.

None of this matters for a health checker, but it is the honest list of what
you would have to build yourself for a general-purpose client. Retrying is
one of those things, and the `control.retry` helper in the
[control-plane modules](/manual/control-plane) runs a function again with
exponential backoff until it returns `Ok`. Which statuses deserve a retry
(`503` probably, `404` probably not) is an application decision, so it stays
in your code.

The `net` module has more than TCP and HTTP. UDP sockets
(`net.udp_bind`) send and receive datagrams, WebSockets
(`net.websocket_listen` and `net.websocket_connect_timeout`) exchange whole
text or byte messages, and on Unix hosts there are Unix domain sockets and
TLS streams with your own certificates. They follow the same rules you have
seen: resources in `with`, `Result[..., io.Error]` everywhere, and a `timeout`
on every wait. The [Network Module](/manual/network) page lists them all with
their limits.

`checkup` now has all three probes. The configuration format is starting to
strain, though: a command argument containing a space cannot be written at
all, and the report is text that another program would have to parse. Both
problems have the same answer, which is the subject of the last section.
