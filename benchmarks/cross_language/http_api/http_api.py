"""HTTP JSON API over loopback: an item service and a client that drives it.

The server answers POST /items/<id>, GET /items/<id>, and GET /stats with
JSON bodies. The client sends one request per connection, as the Aura HTTP
client does, and checks every reply. HTTP/1.1 framing is written by hand on
asyncio streams, as in benchmarks/release_performance/retrying_worker.py.
"""
import asyncio
import json
import sys


class Rng:
    def __init__(self, seed):
        self.state = seed

    def next(self):
        self.state = self.state * 48271 % 2147483647
        return self.state


def dumps(value):
    return json.dumps(value, sort_keys=True, separators=(",", ":"))


async def read_message(reader):
    start = (await reader.readline()).decode().rstrip("\r\n")
    headers = {}
    while True:
        line = (await reader.readline()).decode().rstrip("\r\n")
        if line == "":
            break
        name, value = line.split(":", 1)
        headers[name.strip().lower()] = value.strip()
    length = int(headers.get("content-length", "0"))
    body = (await reader.readexactly(length)).decode() if length else ""
    return start, body


def route(items, method, path, body):
    if method == "POST" and path.startswith("/items/"):
        item = json.loads(body)
        items[int(path[7:])] = item
        return 201, dumps({"id": item["id"], "ok": True})
    if method == "GET" and path.startswith("/items/"):
        item = items.get(int(path[7:]))
        if item is None:
            return 404, dumps({"error": "not found"})
        return 200, dumps(item)
    if method == "GET" and path == "/stats":
        return 200, dumps({"count": len(items)})
    return 400, dumps({"error": "bad request"})


async def serve(port_ready, requests):
    items = {}
    remaining = [requests]
    done = asyncio.get_running_loop().create_future()

    async def handle(reader, writer):
        start, body = await read_message(reader)
        method, path, _ = start.split(" ")
        status, payload = route(items, method, path, body)
        data = payload.encode()
        writer.write(
            f"HTTP/1.1 {status} OK\r\nContent-Type: application/json\r\n"
            f"Content-Length: {len(data)}\r\nConnection: close\r\n\r\n".encode() + data
        )
        await writer.drain()
        writer.close()
        await writer.wait_closed()
        remaining[0] -= 1
        if remaining[0] == 0:
            done.set_result(None)

    server = await asyncio.start_server(handle, "127.0.0.1", 0)
    port_ready.set_result(server.sockets[0].getsockname()[1])
    await done
    server.close()
    await server.wait_closed()
    return len(items)


async def request(port, method, path, body):
    reader, writer = await asyncio.open_connection("127.0.0.1", port)
    data = body.encode()
    writer.write(
        f"{method} {path} HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nContent-Type: application/json\r\n"
        f"Content-Length: {len(data)}\r\nConnection: close\r\n\r\n".encode() + data
    )
    await writer.drain()
    start, reply = await read_message(reader)
    writer.close()
    await writer.wait_closed()
    return int(start.split(" ")[1]), reply


async def run(n):
    ready = asyncio.get_running_loop().create_future()
    server = asyncio.create_task(serve(ready, n))
    port = await ready
    rng = Rng(11)
    statuses = 0
    quantities = 0
    body_bytes = 0
    for i in range(n):
        r = rng.next()
        if i % 50 == 49:
            status, reply = await request(port, "GET", "/stats", "")
            quantities += json.loads(reply)["count"]
        elif i % 3 != 2:
            item = {"id": i, "name": f"item-{i}", "qty": r % 100, "tags": ["new", f"batch-{r % 7}"]}
            status, reply = await request(port, "POST", f"/items/{i}", dumps(item))
            if json.loads(reply)["id"] != i:
                raise RuntimeError("wrong id")
        else:
            status, reply = await request(port, "GET", f"/items/{r % (i + 1)}", "")
            value = json.loads(reply)
            if status == 200:
                quantities += value["qty"]
        statuses += status
        body_bytes += len(reply)
    stored = await server
    print(f"http_api requests={n} stored={stored} statuses={statuses} quantities={quantities} body_bytes={body_bytes}")


if __name__ == "__main__":
    asyncio.run(run(int(sys.argv[1])))
