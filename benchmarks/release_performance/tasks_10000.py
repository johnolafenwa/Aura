#!/usr/bin/env python3
"""Strict CPython 3.9 asyncio side of the 10,000-task benchmark."""

from __future__ import annotations

import asyncio
import sys
from typing import List


TASK_COUNT = 10_000
EXPECTED_CHECKSUM = 49_995_000


async def task_value(value: int) -> int:
    return value


async def run_tasks() -> int:
    tasks: List[asyncio.Task[int]] = []
    try:
        for value in range(TASK_COUNT):
            tasks.append(asyncio.create_task(task_value(value)))
        results = await asyncio.gather(*tasks)
        return sum(results)
    finally:
        pending = [task for task in tasks if not task.done()]
        for task in pending:
            task.cancel()
        if pending:
            await asyncio.gather(*pending, return_exceptions=True)


def main() -> int:
    print("READY release-performance tasks 10000", flush=True)
    if sys.stdin.readline() != "GO release-performance tasks\n":
        return 3

    try:
        checksum = asyncio.run(run_tasks())
    except Exception:
        return 4
    if checksum != EXPECTED_CHECKSUM:
        return 5

    print(
        f"DONE release-performance tasks 10000 {checksum}",
        flush=True,
    )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
