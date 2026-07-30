#!/usr/bin/env python3
"""Measures the V6 workload: a ten-million iteration counter loop at `int32`
and `int64` width, built with the direct backend.

Both widths are always reported, so the relationship between them stays visible
rather than being summarized away. The reported figure is the best of several
runs, which is the appropriate statistic for a CPU-bound loop: the minimum is
the run least disturbed by unrelated system activity.
"""
from __future__ import annotations

import argparse
import pathlib
import subprocess
import sys
import tempfile
import time

try:
    from scripts import benchmark_process
except ImportError:
    import benchmark_process

ROOT = pathlib.Path(__file__).resolve().parent.parent
WIDTHS = ("int32", "int64")


def resolve_aura() -> pathlib.Path:
    for candidate in (ROOT / "target/release/aura", ROOT / "target/debug/aura"):
        if candidate.is_file():
            return candidate
    sys.exit("no aura binary found; run `cargo build -p aura` first")


def measure(binary: pathlib.Path, repeats: int) -> float:
    best = None
    for _ in range(repeats):
        started = time.perf_counter()
        result = benchmark_process.run_process_group(
            [str(binary)],
            "direct integer-loop workload",
            stdout=subprocess.DEVNULL,
            stderr=subprocess.PIPE,
        )
        result.check_returncode()
        elapsed = time.perf_counter() - started
        best = elapsed if best is None else min(best, elapsed)
    assert best is not None
    return best


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--repeats", type=int, default=5)
    args = parser.parse_args()

    aura = resolve_aura()
    results: dict[str, float] = {}
    with tempfile.TemporaryDirectory(prefix="aurora-bench-") as work:
        work = pathlib.Path(work)
        for width in WIDTHS:
            program = ROOT / f"benchmarks/direct_integer_loops/{width}_loop.au"
            binary = work / f"{width}_loop"
            build = benchmark_process.run_process_group(
                [
                    str(aura),
                    "build",
                    "--backend",
                    "direct",
                    "-o",
                    str(binary),
                    str(program),
                ],
                "direct integer-loop build",
                stdout=subprocess.DEVNULL,
                stderr=subprocess.PIPE,
            )
            build.check_returncode()
            results[width] = measure(binary, args.repeats)

    print(f"{'width':<8}{'best_s':>10}")
    for width in WIDTHS:
        print(f"{width:<8}{results[width]:>10.4f}")
    ratio = results["int32"] / results["int64"]
    print(f"int32/int64 {ratio:.2f}x")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
