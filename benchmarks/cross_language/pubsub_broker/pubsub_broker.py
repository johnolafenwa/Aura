"""Messaging: a publish/subscribe broker with mailboxes over loopback TCP.

One server task owns the broker and serves client sessions one after another,
like the key-value server. Clients send SUB and UNSUB to manage topic
subscriptions, PUB to fan a message out to the mailbox of every subscriber, and
POLL to drain up to K waiting messages, which arrive as MSG lines followed by
END. Each session sends 250 commands from 40 clients over 16 topics and waits
for every reply. Reports the messages fanned out and received, and a checksum.
"""
import asyncio
import sys

COMMANDS_PER_SESSION = 250
CLIENTS = 40
TOPICS = 16


class Rng:
    def __init__(self, seed):
        self.state = seed

    def next(self):
        self.state = self.state * 48271 % 2147483647
        return self.state


class Broker:
    def __init__(self):
        self.topics = {}
        self.mailboxes = [[] for _ in range(CLIENTS)]
        self.heads = [0] * CLIENTS
        self.delivered = 0

    def execute(self, line):
        parts = line.split(" ")
        verb = parts[0]
        if verb == "PUB":
            subscribers = self.topics.get(parts[1])
            if subscribers is None:
                return "SENT 0"
            message = "MSG " + parts[1] + " " + parts[2]
            for client in subscribers:
                self.mailboxes[client].append(message)
            self.delivered += len(subscribers)
            return "SENT " + str(len(subscribers))
        client = int(parts[1][1:])
        if verb == "SUB":
            subscribers = self.topics.get(parts[2])
            if subscribers is None:
                subscribers = []
                self.topics[parts[2]] = subscribers
            if client not in subscribers:
                subscribers.append(client)
            return "OK"
        if verb == "UNSUB":
            subscribers = self.topics.get(parts[2])
            if subscribers is not None and client in subscribers:
                subscribers.remove(client)
                return "OK"
            return "NONE"
        limit = int(parts[2])
        mailbox = self.mailboxes[client]
        head = self.heads[client]
        end = min(len(mailbox), head + limit)
        lines = mailbox[head:end]
        self.heads[client] = end
        lines.append("END")
        return "\n".join(lines)


async def serve(listener_ready, sessions):
    broker = Broker()
    commands = 0
    done = asyncio.get_running_loop().create_future()
    remaining = [sessions]

    async def handle(reader, writer):
        nonlocal commands
        while True:
            line = await reader.readline()
            if not line:
                break
            text = line.decode().rstrip("\n")
            if text == "QUIT":
                writer.write(b"BYE\n")
                await writer.drain()
                break
            writer.write((broker.execute(text) + "\n").encode())
            await writer.drain()
            commands += 1
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
    return commands, broker.delivered


async def run(n):
    sessions = n // COMMANDS_PER_SESSION
    ready = asyncio.get_running_loop().create_future()
    server = asyncio.create_task(serve(ready, sessions))
    port = await ready
    rng = Rng(97)
    published = 0
    received = 0
    checksum = 0
    sequence = 0
    for _ in range(sessions):
        reader, writer = await asyncio.open_connection("127.0.0.1", port)
        for _ in range(COMMANDS_PER_SESSION):
            r = rng.next()
            op = r % 100
            client = (r // 100) % CLIENTS
            topic = (r // 10000) % TOPICS
            poll = False
            if op < 12:
                request = "SUB c" + str(client) + " t" + str(topic)
            elif op < 15:
                request = "UNSUB c" + str(client) + " t" + str(topic)
            elif op < 70:
                sequence += 1
                request = "PUB t" + str(topic) + " m" + str(sequence) + "-" + str(r % 1000)
            else:
                request = "POLL c" + str(client) + " " + str(1 + r % 20)
                poll = True
            writer.write((request + "\n").encode())
            await writer.drain()
            if poll:
                while True:
                    reply = (await reader.readline()).decode().rstrip("\n")
                    if reply == "END":
                        break
                    received += 1
                    checksum = (checksum * 31 + len(reply)) % 1000000007
            else:
                reply = (await reader.readline()).decode().rstrip("\n")
                if reply.startswith("SENT "):
                    published += int(reply[5:])
        writer.write(b"QUIT\n")
        await writer.drain()
        if (await reader.readline()) != b"BYE\n":
            raise RuntimeError("missing BYE")
        writer.close()
        await writer.wait_closed()
    commands, delivered = await server
    print(
        f"pubsub_broker sessions={sessions} commands={commands} published={published} "
        f"delivered={delivered} received={received} checksum={checksum}"
    )


if __name__ == "__main__":
    asyncio.run(run(int(sys.argv[1])))
