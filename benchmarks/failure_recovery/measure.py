"""Measure already-built recovery programs in rotating order."""

from os import environ
from statistics import median
from subprocess import PIPE, run
import sys


def main():
    aura, rust, python = sys.argv[1:]
    commands = [("Aura", [aura]), ("Rust", [rust]), ("Python", [sys.executable, python])]
    repeats = int(environ.get("AURA_BENCH_REPEATS", "5"))
    if repeats < 1:
        raise ValueError("AURA_BENCH_REPEATS must be positive")
    counts = {"success": (1_000_000, 127_639_971), "trap": (100_000, 100_000)}
    times = {}
    for sample in range(repeats):
        start = sample % len(commands)
        for language, command in commands[start:] + commands[:start]:
            output = run(command, check=True, stdout=PIPE, text=True).stdout
            lines = output.splitlines()
            if len(lines) != len(counts):
                raise ValueError(f"unexpected {language} output: {output}")
            for expected, line in zip(counts, lines):
                phase, count, milliseconds, total = line.split()
                if phase != expected or (int(count), int(total)) != counts[expected]:
                    raise ValueError(f"unexpected {language} result: {line}")
                ns_per_call = float(milliseconds) * 1_000_000 / int(count)
                times.setdefault((language, phase), []).append(ns_per_call)
            print(f"run {sample + 1} {language}\n{output}", end="", flush=True)
    print("\nMedian nanoseconds per call")
    print("Language   Success        Trap")
    for language, _ in commands:
        print(f"{language:8} {median(times[language, 'success']):9.2f} {median(times[language, 'trap']):11.2f}")


if __name__ == "__main__":
    main()
