# UDP, WebSockets, Unix sockets, and TLS

TCP and HTTP carry most of what programs say to each other, but not all of
it. The `net` module has four more kinds of socket, and each exists because
TCP or HTTP is the wrong shape for some job. This section is a short tour:
one small program for each, and a paragraph on when you would reach for it.

They all follow the pattern of Listing 16-9. A server task opens its own
socket and reports where it is listening through a queue, the client runs
in `main`, and every wait has a timeout. Only the shape of the conversation
changes.

## UDP: messages without a connection

TCP sets up a connection, delivers every byte in order, and tells you when
the other side has gone. That reliability costs a handshake and some
bookkeeping. UDP does none of it. A UDP socket sends a single *datagram* to
an address and forgets about it. Nothing reports whether the datagram
arrived, and nothing is set up or torn down. That suits data where the next
message replaces the last one and a lost one is not worth resending:
metrics, service discovery, game state.

Here a collector gathers readings, and a reporter sends it three:

*Filename: src/main.au*

```aura
import io
import net

def collect(addresses: Queue[str]) -> Result[list[str], io.Error]:
    mut readings: list[str] = []
    with socket = try net.udp_bind("127.0.0.1:0"):
        _ = addresses.put(try socket.local_addr())
        while true:
            match try socket.recv_from(1024, timeout=500ms):
                case net.UdpDatagram as packet:
                    readings.append(f"{try packet.text()} ({packet.bytes().len()} bytes)")
                case None:
                    break
    return Result.Ok(readings)

def report(address: str) -> Result[None, io.Error]:
    with socket = try net.udp_bind("127.0.0.1:0"):
        try socket.send_text(address, "cpu=41", timeout=1s)
        try socket.send_text(address, "memory=73", timeout=1s)
        try socket.send_bytes(address, "disk=88".to_bytes(), timeout=1s)
    return Result.Ok(None)

def main():
    with TaskGroup() as group:
        addresses = Queue[str](capacity=1)
        collector = group.start(collect, addresses)
        match own addresses.get(timeout=5s):
            case QueueReceive.Item(address):
                print(report(address))
            case _:
                print("collector never started")
        print(collector.result())
```

<span class="caption">Listing 16-21: Sending three datagrams to a collector</span>

```console
$ aura run src/main.au
Result.Ok(None)
TaskResult.Ready(Result.Ok([cpu=41 (6 bytes), memory=73 (9 bytes), disk=88 (7 bytes)]))
```

There is no listener and no `accept`. Both sides call `net.udp_bind`, and
the only difference between them is who speaks first. `send_text` and
`send_bytes` name the destination on every call, because the socket is not
connected to anyone. `recv_from` returns a `net.UdpDatagram` with the
sender's `address()` and the payload as `bytes()` or `text()`.

Two things differ from TCP. The first is that message boundaries survive.
Three sends arrived as three datagrams of 6, 9, and 7 bytes. A TCP stream
would have delivered 22 bytes with no marks between them, and the receiver
would have needed newlines or length prefixes to find the seams. The `1024`
passed to `recv_from` is the largest datagram we are prepared to accept.

The second is what a timeout means. On TCP an expired wait is an error,
`io.Error.TimedOut`. On UDP silence is normal, so an expired `recv_from`
returns `Ok(None)`, and the collector uses that to stop: half a second with
no datagram and it returns what it has. On a real network it would also be
normal for one of the three never to arrive, or for two to arrive in the
other order. A protocol built on UDP has to be content with that.

## WebSockets: messages in both directions

HTTP is a question and an answer, always started by the client. A chat
window, a live dashboard, or a multiplayer game needs the server to speak
when it has something to say. A WebSocket starts life as an HTTP request and
then becomes a long-lived connection over which either side can send whole
messages at any time. It is what browsers offer for this, so it is what
you use when the other end is a web page.

*Filename: src/main.au*

```aura
import io
import net

def serve(addresses: Queue[str]) -> Result[int64, io.Error]:
    listener = try net.websocket_listen("127.0.0.1:0")
    _ = addresses.put(try listener.local_addr())
    mut handled = 0
    with socket = try listener.accept(timeout=5s):
        while true:
            match try socket.recv_text(timeout=5s):
                case str as message:
                    handled += 1
                    try socket.send_text(message.to_upper(), timeout=5s)
                case None:
                    break
    return Result.Ok(handled)

def chat(address: str) -> Result[None, io.Error]:
    with socket = try net.websocket_connect_timeout(f"ws://{address}/", 5s):
        for message in ["hello", "is anyone there"]:
            try socket.send_text(message, timeout=5s)
            match try socket.recv_text(timeout=5s):
                case str as reply:
                    print(reply)
                case None:
                    print("server closed")
    return Result.Ok(None)

def main():
    with TaskGroup() as group:
        addresses = Queue[str](capacity=1)
        server = group.start(serve, addresses)
        match own addresses.get(timeout=5s):
            case QueueReceive.Item(address):
                print(chat(address))
            case _:
                print("server never started")
        print(server.result())
```

<span class="caption">Listing 16-22: A WebSocket server that answers in capitals</span>

```console
$ aura run src/main.au
HELLO
IS ANYONE THERE
Result.Ok(None)
TaskResult.Ready(Result.Ok(2))
```

The client connects with a `ws://` URL. After that, both ends hold a
`net.WebSocket` with the same four operations: `send_text` and `send_bytes`
send one message, and `recv_text` and `recv_bytes` receive one. Like UDP, a
WebSocket delivers whole messages, so there is no framing to invent. Unlike
UDP, it is reliable and ordered, because TCP is underneath.

`recv_text` returns `None` when the other side closes. That is how the
server's loop ends: the client's `with` block closes its socket after the
second reply, the server sees `None`, and reports that it handled two
messages.

One line of `serve` breaks our pattern. The listener is bound to a plain
name, not with `with`. In Aura 0.17 a `net.WebSocketListener` has no
`close()` method, so it cannot be a `with` resource. It is released when
its binding goes out of scope at the end of `serve`. The Manual lists this
among the known gaps of the preview, along with one that matters more:
cancelling a task does not interrupt a WebSocket wait as promptly as it
interrupts a TCP one. Keep the timeouts short on WebSocket operations in a
program that has to shut down quickly.

## Unix sockets: local conversations

When both programs are on the same machine, there is no need to go through
the network stack or to pick a port. A Unix domain socket is addressed by a
path in the filesystem. The conversation never leaves the machine, and the
file's permissions decide who may connect. This is how a command-line tool
talks to its own background service: `docker` to the Docker daemon, for
example. Despite the name, Aura provides them on Windows as well as on
Linux and macOS.

*Filename: src/main.au*

```aura
import fs
import io
import net

def serve(path: str, ready: Queue[bool]) -> Result[None, io.Error]:
    if fs.exists(path):
        try fs.remove_file(path)
    with listener = try net.unix_listen(path):
        _ = ready.put(true)
        with stream = try listener.accept(timeout=5s):
            match try stream.read_line(timeout=5s):
                case str as command:
                    try stream.write_all(f"ok: {command}\n", timeout=5s)
                case None:
                    pass
    return fs.remove_file(path)

def send(path: str, command: str) -> Result[str, io.Error]:
    with stream = try net.unix_connect_timeout(path, 5s):
        try stream.write_all(f"{command}\n", timeout=5s)
        match own try stream.read_line(timeout=5s):
            case str as reply:
                return Result.Ok(reply)
            case None:
                return Result.Err(io.Error.UnexpectedEof)

def main():
    path = "/tmp/courier.sock"
    with TaskGroup() as group:
        ready = Queue[bool](capacity=1)
        server = group.start(serve, path.clone(), ready)
        match ready.get(timeout=5s):
            case QueueReceive.Item(_):
                print(send(path, "reload"))
            case _:
                print("server never started")
        print(server.result())
    print(send(path, "reload"))
```

<span class="caption">Listing 16-23: A control socket at a filesystem path</span>

```console
$ aura run src/main.au
Result.Ok(ok: reload)
TaskResult.Ready(Result.Ok(None))
Result.Err(io.Error.NotFound)
```

Once connected, a `net.UnixStream` reads and writes lines like a TCP
stream. The differences are at the edges. The address is a path the server
chooses, so there is nothing to publish, and the queue only carries a "ready"
signal. And the path is a real directory entry that outlives the listener.
Closing a Unix listener does not remove its file, and `net.unix_listen`
refuses to bind a path that already exists: a second run of a careless
server fails with `io.Error.AlreadyExists`. So `serve` removes a leftover
file before it listens and removes its own when it is done. The last line
of `main` shows the result. With the file gone, a client has nothing to
connect to.

Be careful with that first `remove_file` in a real service. It deletes
whatever is at the path. Keep sockets in a directory your program owns,
such as one under `/run` or the user's runtime directory.

## TLS: your own encrypted streams

Everything so far in this chapter travelled as plain text that anyone on
the path could read or alter. The HTTP client fixes that for `https://`
URLs. For your own protocol over TCP, the `net` module has TLS listeners
and streams, which encrypt the connection and let the client check that it
reached the right server.

A TLS server needs a certificate and a private key. For an experiment on
`localhost`, OpenSSL can make a self-signed pair:

```console
$ mkdir certs
$ openssl req -x509 -newkey ec -pkeyopt ec_paramgen_curve:prime256v1 -nodes \
    -keyout certs/key.pem -out certs/cert.pem -days 365 -subj "/CN=localhost" \
    -addext "subjectAltName=DNS:localhost" \
    -addext "basicConstraints=critical,CA:FALSE"
```

That writes `certs/key.pem`, which stays with the server, and
`certs/cert.pem`, which clients are given so they know what to trust. The
last option matters. By default OpenSSL marks a self-signed certificate as
a certificate authority, and Aura's TLS library refuses a server that
presents an authority's certificate as its own.

*Filename: src/main.au*

```aura
import io
import net

def serve(addresses: Queue[str]) -> Result[None, io.Error]:
    with listener = try net.tls_listen("127.0.0.1:0", "certs/cert.pem", "certs/key.pem"):
        _ = addresses.put(try listener.local_addr())
        with stream = try listener.accept(timeout=5s):
            match try stream.read_line(timeout=5s):
                case str as question:
                    try stream.write_all(f"secret answer to: {question}\n", timeout=5s)
                case None:
                    pass
    return Result.Ok(None)

def ask(address: str, server_name: str) -> Result[str, io.Error]:
    with stream = try net.tls_connect_timeout(address, server_name, "certs/cert.pem", 5s):
        try stream.write_all("what is the password\n", timeout=5s)
        match own try stream.read_line(timeout=5s):
            case str as reply:
                return Result.Ok(reply)
            case None:
                return Result.Err(io.Error.UnexpectedEof)

def main():
    with TaskGroup() as group:
        addresses = Queue[str](capacity=1)
        server = group.start(serve, addresses)
        match own addresses.get(timeout=5s):
            case QueueReceive.Item(address):
                print(ask(address, "localhost"))
            case _:
                print("server never started")
        print(server.result())
```

<span class="caption">Listing 16-24: A TLS server and a client that verifies it</span>

```console
$ aura run src/main.au
Result.Ok(secret answer to: what is the password)
TaskResult.Ready(Result.Ok(None))
```

Compare this with Listing 16-9. `net.tls_listen` takes the certificate and
key files in addition to the address, and `net.tls_connect_timeout` takes
two extra arguments: the name the client expects the server to prove it
owns, and the file of certificates it trusts. Between those calls the code
is the TCP code. `read_line` and `write_all` work on decrypted text, and
the encryption happens underneath.

The server name is the part people skip, so let's get it wrong on purpose.
Change the call in `main` to `ask(address, "example.com")`:

```console
$ aura run src/main.au
Result.Err(io.Error.InvalidData)
TaskResult.Ready(Result.Err(io.Error.TimedOut))
```

The client connected to the same address and was shown the same
certificate. It refused, because the certificate says `localhost` and we
asked for `example.com`. Encryption without that check would only prove
that we have a private conversation with *someone*. The server's side of
the story is the second line: its handshake never completed, and its
`accept` gave up when the five seconds ran out.

Certificates for real services come from a certificate authority, and the
file you pass to the client is then the authority's certificate, not the
server's. Aura reads certificates and keys from PEM files and has no API
for making them. A TLS handshake is also capped at ten seconds whatever
timeout you pass.

## Summary

`checkup` started as a program that echoed a text file and ends as a tool you
could run from cron or CI. Along the way you have used each of Aura's modules
for the outside world. `fs` reads and writes files, with `with` closing
handles and the temporary-file-and-rename recipe keeping readers from ever
seeing half a report. `process` runs commands from an explicit argv with no
shell in between, captures their output, talks to live children through pipes,
and supervises children with restart policies. `net` serves HTTP and TCP,
keeping each socket on the task that opened it, and adds datagrams,
WebSockets, local sockets, and TLS for the conversations that HTTP does not
fit. `http` calls other services, reads their answers whole or as a stream,
and `control.retry` tries again when they stumble. And `json` turns
documents into your own types and back, with errors that point at a field.

The same three rules held throughout: failures are `Result` values with
specific error types, resources live in `with` blocks, and every wait has a
timeout. They are the reason `checkup` can tell "the command failed" from
"the command never started" from "the command is stuck", and why it leaves no
processes or open files behind on any path out. The full contracts are in the
Manual pages for [I/O](/manual/io), [the filesystem](/manual/filesystem),
[processes](/manual/process), [networking](/manual/network),
[HTTP](/manual/http), and [JSON](/manual/json).

So far we have run everything with `aura run`. In the next chapter we will
build `checkup` into a native binary you can copy to another machine, step
through it in a debugger, and look at what Aura code costs when it runs.
