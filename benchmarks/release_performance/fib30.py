#!/usr/bin/env python3
"""CPython counterpart to Aura's naive recursive fib(30) workload."""

from __future__ import annotations

import sys


def fib(n: int) -> int:
    if n < 2:
        return n
    return fib(n - 1) + fib(n - 2)


def main() -> int:
    print("READY release-performance fib30 30", flush=True)
    if sys.stdin.readline() != "GO release-performance fib30\n":
        return 4

    result = fib(30)
    if result != 832_040:
        return 5

    print(f"DONE release-performance fib30 {result}", flush=True)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
