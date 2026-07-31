#!/usr/bin/env python3
"""CPython reference for the 20-connection loopback fan-out workload."""

from __future__ import annotations

import asyncio
import sys
from typing import List, Tuple


CLIENT_COUNT = 20
HANDLER_DELAY_SECONDS = 0.100
PAYLOAD = b"ping\n"
REPLY = b"pong\n"
IO_TIMEOUT_SECONDS = 5.0

READY = b"READY release-performance tcp-fanout 20 100 4\n"
GO = b"GO release-performance tcp-fanout\n"
DONE = b"DONE release-performance tcp-fanout 20 80\n"


async def close_writer(writer: asyncio.StreamWriter) -> None:
    writer.close()
    await writer.wait_closed()


async def handle_connection(
    reader: asyncio.StreamReader,
    writer: asyncio.StreamWriter,
    handled: List[bool],
    index: int,
) -> None:
    try:
        payload = await asyncio.wait_for(reader.readline(), IO_TIMEOUT_SECONDS)
        if payload != PAYLOAD:
            raise RuntimeError("unexpected TCP fan-out payload")
        await asyncio.sleep(HANDLER_DELAY_SECONDS)
        writer.write(REPLY)
        await asyncio.wait_for(writer.drain(), IO_TIMEOUT_SECONDS)
        handled[index] = True
    finally:
        await close_writer(writer)


async def run_client(address: Tuple[str, int]) -> int:
    reader, writer = await asyncio.wait_for(
        asyncio.open_connection(address[0], address[1]), IO_TIMEOUT_SECONDS
    )
    try:
        writer.write(PAYLOAD)
        await asyncio.wait_for(writer.drain(), IO_TIMEOUT_SECONDS)
        reply = await asyncio.wait_for(reader.readline(), IO_TIMEOUT_SECONDS)
        if reply != REPLY:
            raise RuntimeError("unexpected TCP fan-out reply")
        return len(reply.rstrip(b"\n"))
    finally:
        await close_writer(writer)


async def run() -> int:
    servers: List[asyncio.AbstractServer] = []
    addresses: List[Tuple[str, int]] = []
    handler_tasks: List[asyncio.Task[None]] = []
    handled = [False] * CLIENT_COUNT

    try:
        for index in range(CLIENT_COUNT):
            def connected(
                reader: asyncio.StreamReader,
                writer: asyncio.StreamWriter,
                *,
                connection_index: int = index,
            ) -> None:
                handler_tasks.append(
                    asyncio.create_task(
                        handle_connection(
                            reader,
                            writer,
                            handled,
                            connection_index,
                        )
                    )
                )

            server = await asyncio.start_server(connected, "127.0.0.1", 0)
            socket = server.sockets[0]
            host, port = socket.getsockname()[:2]
            servers.append(server)
            addresses.append((str(host), int(port)))

        sys.stdout.buffer.write(READY)
        sys.stdout.buffer.flush()
        if sys.stdin.buffer.readline() != GO:
            return 2

        client_checksums = await asyncio.gather(
            *(run_client(address) for address in addresses)
        )
        if len(handler_tasks) != CLIENT_COUNT:
            return 3
        await asyncio.gather(*handler_tasks)

        checksum = sum(client_checksums)
        if checksum != CLIENT_COUNT * len(REPLY.rstrip(b"\n")):
            return 4
        if not all(handled):
            return 5
    finally:
        for server in servers:
            server.close()
        await asyncio.gather(*(server.wait_closed() for server in servers))
        unfinished = [task for task in handler_tasks if not task.done()]
        for task in unfinished:
            task.cancel()
        if unfinished:
            await asyncio.gather(*unfinished, return_exceptions=True)

    sys.stdout.buffer.write(DONE)
    sys.stdout.buffer.flush()
    return 0


def main() -> int:
    return asyncio.run(run())


if __name__ == "__main__":
    raise SystemExit(main())
