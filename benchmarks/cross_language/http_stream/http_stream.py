"""Streamed HTTP: a client that reads long replies as they arrive.

A server thread speaks HTTP/1.1 by hand over loopback TCP and keeps its
connection open between requests. The client uses http.client on one kept
connection: it reads N log lines from a chunked body, N/4 server-sent
events one at a time, then sends N/100 small requests. See http_stream.au.
"""
import http.client
import socket
import sys
import threading


class Rng:
    def __init__(self, seed):
        self.state = seed

    def next(self):
        self.state = self.state * 48271 % 2147483647
        return self.state


WORDS = ["connect", "read", "write", "close", "retry", "timeout", "accept", "flush"]


def send_chunk(out, piece):
    data = piece.encode()
    out.write(b"%x\r\n%s\r\n" % (len(data), data))
    out.flush()


def send_lines(out, count):
    rng = Rng(41)
    out.write(b"HTTP/1.1 200 OK\r\nContent-Type: text/plain\r\nTransfer-Encoding: chunked\r\n\r\n")
    piece = []
    for i in range(count):
        word = WORDS[rng.next() % 8]
        piece.append(f"{i} {word} worker-{rng.next() % 64} {rng.next() % 100000}\n")
        if i % 64 == 63:
            send_chunk(out, "".join(piece))
            piece = []
    if piece:
        send_chunk(out, "".join(piece))
    out.write(b"0\r\n\r\n")
    out.flush()


def send_events(out, count):
    rng = Rng(43)
    out.write(b"HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nTransfer-Encoding: chunked\r\n\r\n")
    for i in range(count):
        send_chunk(out, f'event: delta\nid: {i}\ndata: {{"index":{i},"text":"tok-{rng.next() % 5000}"}}\n\n')
    send_chunk(out, "event: done\ndata: [DONE]\n\n")
    out.write(b"0\r\n\r\n")
    out.flush()


def send_text(out, body, close):
    data = body.encode()
    head = f"HTTP/1.1 200 OK\r\nContent-Type: text/plain\r\nContent-Length: {len(data)}\r\n"
    if close:
        head += "Connection: close\r\n"
    out.write(head.encode() + b"\r\n" + data)
    out.flush()


def serve(listener):
    accepted = 0
    while True:
        conn, _ = listener.accept()
        conn.setsockopt(socket.IPPROTO_TCP, socket.TCP_NODELAY, 1)
        accepted += 1
        with conn, conn.makefile("rb") as reader, conn.makefile("wb") as out:
            while True:
                start = reader.readline()
                if not start:
                    break
                target = start.decode().split(" ")[1]
                while reader.readline() not in (b"\r\n", b"\n", b""):
                    pass
                if target == "/quit":
                    send_text(out, str(accepted), True)
                    return
                if target.startswith("/lines/"):
                    send_lines(out, int(target[7:]))
                elif target.startswith("/events/"):
                    send_events(out, int(target[8:]))
                else:
                    send_text(out, "item " + target, False)


def read_events(response):
    """Yields (event, data, id) for each server-sent event."""
    event, data, last_id = "message", [], ""
    for raw in response:
        line = raw.decode().rstrip("\r\n")
        if line == "":
            if data:
                yield event, "\n".join(data), last_id
            event, data = "message", []
            continue
        if line.startswith(":"):
            continue
        name, _, value = line.partition(":")
        if value.startswith(" "):
            value = value[1:]
        if name == "event":
            event = value
        elif name == "data":
            data.append(value)
        elif name == "id":
            last_id = value


def run(n):
    event_count = n // 4
    request_count = n // 100
    listener = socket.socket()
    listener.bind(("127.0.0.1", 0))
    listener.listen()
    server = threading.Thread(target=serve, args=(listener,))
    server.start()
    client = http.client.HTTPConnection("127.0.0.1", listener.getsockname()[1])

    lines = value_sum = retries = 0
    client.request("GET", f"/lines/{n}")
    for raw in client.getresponse():
        fields = raw.decode().rstrip("\n").split(" ")
        lines += 1
        value_sum += int(fields[3])
        if fields[1] == "retry":
            retries += 1

    events = event_bytes = 0
    last_id = ""
    client.request("GET", f"/events/{event_count}")
    for event, data, event_id in read_events(client.getresponse()):
        if event == "done":
            last_id = event_id
            continue
        events += 1
        event_bytes += len(data.encode())

    body_bytes = 0
    for i in range(request_count):
        client.request("GET", f"/item/{i}")
        body_bytes += len(client.getresponse().read())

    client.request("GET", "/quit")
    connections = client.getresponse().read().decode()
    client.close()
    server.join()
    listener.close()
    return (f"http_stream lines={lines} value_sum={value_sum} retries={retries} events={events} "
            f"event_bytes={event_bytes} last_id={last_id} requests={request_count} "
            f"body_bytes={body_bytes} connections={connections}")


if __name__ == "__main__":
    print(run(int(sys.argv[1])))
