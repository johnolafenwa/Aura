#!/usr/bin/env python3
"""CPython reference for the deterministic retrying-worker workload."""

from __future__ import annotations

import asyncio
import sys
from typing import List, Tuple


CYCLES = 16
REQUESTS_PER_CYCLE = 7
TOTAL_REQUESTS = CYCLES * REQUESTS_PER_CYCLE
SCHEDULED_DELAY_MS = CYCLES * 18
FINAL_STATUS_CHECKSUM = CYCLES * (200 + 429 + 503)
IO_TIMEOUT_SECONDS = 5.0

READY = b"READY release-performance retrying-worker 16 112 288\n"
GO = b"GO release-performance retrying-worker\n"
DONE = b"DONE release-performance retrying-worker 112 18112\n"

STATUS_SCHEDULE = (503, 200, 503, 429, 503, 503, 503)
NAME_SCHEDULE = (
    "recover",
    "recover",
    "rate",
    "rate",
    "exhaust",
    "exhaust",
    "exhaust",
)


async def close_writer(writer: asyncio.StreamWriter) -> None:
    writer.close()
    await writer.wait_closed()


async def read_headers(reader: asyncio.StreamReader) -> List[bytes]:
    headers: List[bytes] = []
    while True:
        line = await asyncio.wait_for(reader.readline(), IO_TIMEOUT_SECONDS)
        if line == b"":
            raise RuntimeError("HTTP request ended before its headers")
        if line == b"\r\n":
            return headers
        headers.append(line)


async def run_http_request(address: Tuple[str, int], path: str) -> int:
    reader, writer = await asyncio.wait_for(
        asyncio.open_connection(address[0], address[1]), IO_TIMEOUT_SECONDS
    )
    try:
        request = (
            f"GET {path} HTTP/1.1\r\n"
            f"Host: {address[0]}:{address[1]}\r\n"
            "Connection: close\r\n"
            "\r\n"
        ).encode("ascii")
        writer.write(request)
        await asyncio.wait_for(writer.drain(), IO_TIMEOUT_SECONDS)
        status_line = await asyncio.wait_for(reader.readline(), IO_TIMEOUT_SECONDS)
        fields = status_line.rstrip(b"\r\n").split(b" ")
        if len(fields) < 2 or fields[0] != b"HTTP/1.1":
            raise RuntimeError("unexpected HTTP response status line")
        status = int(fields[1])
        await read_headers(reader)
        return status
    finally:
        await close_writer(writer)


async def request_with_retry(
    address: Tuple[str, int],
    path: str,
    delays_ms: Tuple[int, ...],
) -> int:
    attempt = 0
    while True:
        status = await run_http_request(address, path)
        if status != 503 or attempt == len(delays_ms):
            return status
        await asyncio.sleep(delays_ms[attempt] / 1000.0)
        attempt += 1


async def run_worker(address: Tuple[str, int]) -> int:
    checksum = 0
    for cycle in range(CYCLES):
        checksum += await request_with_retry(
            address, f"/{cycle}/recover", (4,)
        )
        checksum += await request_with_retry(address, f"/{cycle}/rate", (6,))
        checksum += await request_with_retry(
            address, f"/{cycle}/exhaust", (3, 5)
        )
    return checksum


async def run() -> int:
    request_index = 0
    handler_tasks: List[asyncio.Task[None]] = []

    async def handle_request(
        reader: asyncio.StreamReader, writer: asyncio.StreamWriter
    ) -> None:
        nonlocal request_index
        try:
            request_line = await asyncio.wait_for(
                reader.readline(), IO_TIMEOUT_SECONDS
            )
            fields = request_line.rstrip(b"\r\n").split(b" ")
            if len(fields) != 3 or fields[0] != b"GET" or fields[2] != b"HTTP/1.1":
                raise RuntimeError("unexpected HTTP request line")
            await read_headers(reader)

            index = request_index
            request_index += 1
            cycle, offset = divmod(index, REQUESTS_PER_CYCLE)
            expected_path = f"/{cycle}/{NAME_SCHEDULE[offset]}".encode("ascii")
            if fields[1] != expected_path:
                raise RuntimeError("unexpected retry request path")
            status = STATUS_SCHEDULE[offset]
            reason = "OK" if status == 200 else "Too Many Requests" if status == 429 else "Service Unavailable"
            response = (
                f"HTTP/1.1 {status} {reason}\r\n"
                "Content-Length: 0\r\n"
                "Connection: close\r\n"
                "\r\n"
            ).encode("ascii")
            writer.write(response)
            await asyncio.wait_for(writer.drain(), IO_TIMEOUT_SECONDS)
        finally:
            await close_writer(writer)

    def connected(
        reader: asyncio.StreamReader, writer: asyncio.StreamWriter
    ) -> None:
        handler_tasks.append(asyncio.create_task(handle_request(reader, writer)))

    server = await asyncio.start_server(connected, "127.0.0.1", 0)
    socket = server.sockets[0]
    host, port = socket.getsockname()[:2]
    address = (str(host), int(port))

    try:
        sys.stdout.buffer.write(READY)
        sys.stdout.buffer.flush()
        if sys.stdin.buffer.readline() != GO:
            return 2

        checksum = await run_worker(address)
        if checksum != FINAL_STATUS_CHECKSUM:
            return 3
        if request_index != TOTAL_REQUESTS:
            return 4
        if len(handler_tasks) != TOTAL_REQUESTS:
            return 5
        await asyncio.gather(*handler_tasks)
    finally:
        server.close()
        await server.wait_closed()
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
