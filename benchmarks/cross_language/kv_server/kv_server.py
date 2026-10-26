"""Key-value server: a line protocol over loopback TCP.

One server task owns the store and serves client sessions one after another.
The client opens a session per 500 commands and sends SET, GET, DEL, and INCR
requests, waiting for each reply before sending the next request.
"""
import asyncio
import sys

COMMANDS_PER_SESSION = 500


class Rng:
    def __init__(self, seed):
        self.state = seed

    def next(self):
        self.state = self.state * 48271 % 2147483647
        return self.state


def execute(store, line):
    parts = line.split(" ")
    command = parts[0]
    if command == "SET" and len(parts) == 3:
        store[parts[1]] = parts[2]
        return "OK"
    if command == "GET" and len(parts) == 2:
        value = store.get(parts[1])
        return "NIL" if value is None else "VALUE " + value
    if command == "DEL" and len(parts) == 2:
        return "DELETED 1" if store.pop(parts[1], None) is not None else "DELETED 0"
    if command == "INCR" and len(parts) == 2:
        current = store.get(parts[1], "0")
        try:
            number = int(current) + 1
        except ValueError:
            return "ERR not an integer"
        store[parts[1]] = str(number)
        return "INT " + str(number)
    return "ERR unknown command"


async def serve(listener_ready, sessions):
    store = {}
    served = 0
    done = asyncio.get_running_loop().create_future()
    remaining = [sessions]

    async def handle(reader, writer):
        nonlocal served
        while True:
            line = await reader.readline()
            if not line:
                break
            text = line.decode().rstrip("\n")
            if text == "QUIT":
                writer.write(b"BYE\n")
                await writer.drain()
                break
            writer.write((execute(store, text) + "\n").encode())
            await writer.drain()
            served += 1
        writer.close()
        await writer.wait_closed()
        remaining[0] -= 1
        if remaining[0] == 0:
            done.set_result(None)

    server = await asyncio.start_server(handle, "127.0.0.1", 0)
    listener_ready.set_result(server.sockets[0].getsockname()[1])
    await done
    server.close()
    await server.wait_closed()
    return served


async def run(n):
    sessions = n // COMMANDS_PER_SESSION
    ready = asyncio.get_running_loop().create_future()
    server = asyncio.create_task(serve(ready, sessions))
    port = await ready
    rng = Rng(7)
    hits = 0
    nils = 0
    ints = 0
    checksum = 0
    for _ in range(sessions):
        reader, writer = await asyncio.open_connection("127.0.0.1", port)
        for _ in range(COMMANDS_PER_SESSION):
            r = rng.next()
            op = r % 10
            key = (r // 10) % 1000
            if op < 4:
                request = f"GET key-{key}"
            elif op < 7:
                request = f"SET key-{key} v{r // 10000}"
            elif op == 7:
                request = f"DEL key-{key}"
            else:
                request = f"INCR counter-{key % 50}"
            writer.write((request + "\n").encode())
            await writer.drain()
            reply = (await reader.readline()).decode().rstrip("\n")
            checksum += len(reply)
            if reply.startswith("VALUE "):
                hits += 1
            elif reply == "NIL":
                nils += 1
            elif reply.startswith("INT "):
                ints += int(reply[4:])
        writer.write(b"QUIT\n")
        await writer.drain()
        if (await reader.readline()) != b"BYE\n":
            raise RuntimeError("missing BYE")
        writer.close()
        await writer.wait_closed()
    served = await server
    print(f"kv_server sessions={sessions} commands={served} hits={hits} nils={nils} ints={ints} checksum={checksum}")


if __name__ == "__main__":
    asyncio.run(run(int(sys.argv[1])))
