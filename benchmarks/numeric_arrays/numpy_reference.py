#!/usr/bin/env python3
"""NumPy side of the Phase-7.3 numeric-array protocol."""

from __future__ import annotations

import argparse
import json
import sys

import numpy as np


ELEMENT_COUNT = 1_000_000
ADD_ITERATIONS = 512
SUM_ITERATIONS = 1_024


def emit_identity() -> int:
    print(
        json.dumps(
            {
                "version": np.__version__,
                "path": np.__file__,
                "float64_itemsize": np.dtype(np.float64).itemsize,
                "configuration": np.show_config(mode="dicts"),
            },
            sort_keys=True,
        )
    )
    return 0


def wait_for_go(workload: str) -> bool:
    return sys.stdin.readline() == f"GO numeric-arrays {workload}\n"


def run_add() -> int:
    left = np.full(ELEMENT_COUNT, 1.25, dtype=np.float64)
    right = np.full(ELEMENT_COUNT, 2.75, dtype=np.float64)
    warmup = np.add(left, right)
    if float(warmup[0]) != 4.0:
        return 3
    print(
        f"READY numeric-arrays add {ELEMENT_COUNT} {ADD_ITERATIONS}",
        flush=True,
    )
    if not wait_for_go("add"):
        return 4
    checksum = 0.0
    for _ in range(ADD_ITERATIONS):
        result = np.add(left, right)
        checksum += float(result[0])
    print(
        f"DONE numeric-arrays add {ADD_ITERATIONS} {checksum}",
        flush=True,
    )
    return 0


def run_sum() -> int:
    values = np.full(ELEMENT_COUNT, 4.0, dtype=np.float64)
    if float(np.sum(values, dtype=np.float64)) != 4_000_000.0:
        return 3
    print(
        f"READY numeric-arrays sum {ELEMENT_COUNT} {SUM_ITERATIONS}",
        flush=True,
    )
    if not wait_for_go("sum"):
        return 4
    checksum = 0.0
    for _ in range(SUM_ITERATIONS):
        checksum += float(np.sum(values, dtype=np.float64))
    print(
        f"DONE numeric-arrays sum {SUM_ITERATIONS} {checksum}",
        flush=True,
    )
    return 0


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--identity", action="store_true")
    parser.add_argument("--workload", choices=("add", "sum"))
    arguments = parser.parse_args()
    if arguments.identity:
        return emit_identity()
    if arguments.workload == "add":
        return run_add()
    if arguments.workload == "sum":
        return run_sum()
    parser.error("--workload is required unless --identity is used")
    return 2


if __name__ == "__main__":
    raise SystemExit(main())
