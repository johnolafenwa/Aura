#!/usr/bin/env python3
"""Whole-process CPython counterpart to Aura's V6 integer loops."""

from __future__ import annotations


def main() -> int:
    index = 0
    while index < 10_000_000:
        index += 1
    print(index)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
