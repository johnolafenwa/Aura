"""Compare compiled and Python ordered-collection workloads on the same keys."""

import statistics
import subprocess
import sys
from pathlib import Path

CHECKSUMS = {
    "deque": 33_550_336,
    "heap": 183_218_384_896,
    "map": 33_550_336,
    "set": 33_550_336,
}


def run(command):
    output = subprocess.check_output(command, text=True)
    times = {}
    checksums = {}
    for line in output.splitlines():
        name, elapsed, total = line.split()
        times[name] = float(elapsed)
        checksums[name] = int(total)
    if checksums != CHECKSUMS:
        raise ValueError(f"{command} produced {checksums}, expected {CHECKSUMS}")
    return times


def main():
    commands = {
        "Aura": [sys.argv[1]],
        "Rust": [sys.argv[2]],
        "Python": [sys.executable, str(Path(__file__).with_name("ordered.py"))],
    }
    samples = {name: [] for name in commands}
    for command in commands.values():
        run(command)
    # Alternate implementations between samples and keep their startup times
    # outside the timed sections in all four workloads.
    order = list(commands)
    for _ in range(7):
        for name in order:
            samples[name].append(run(commands[name]))
        order.append(order.pop(0))
    print("Workload\tAura ms\tRust ms\tPython ms\tAura/Rust\tAura/Python")
    for workload in CHECKSUMS:
        aura, rust, python = (
            statistics.median(sample[workload] for sample in timings)
            for timings in samples.values()
        )
        print(
            f"{workload}\t{aura:.3f}\t{rust:.3f}\t{python:.3f}"
            f"\t{aura / rust:.2f}\t{aura / python:.2f}"
        )


if __name__ == "__main__":
    main()
