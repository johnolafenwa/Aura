"""Time each executable repeatedly, checking all six results before reporting."""
import os
from pathlib import Path
from statistics import median
import subprocess
import sys

names = ["factorial", "fibonacci", "small_integers", "modular_powers", "decimal_money", "mandelbrot"]
expected = ["4269323", "20827", "124999750000", "10018095003163", "11970000.00", "1551224"]
commands = {
    "Aura": [sys.argv[1]],
    "Rust": [sys.argv[2]],
    "Python": [sys.executable, str(Path(__file__).with_name("main.py"))],
}
repeats = int(os.environ.get("REPEATS", "3"))
if repeats <= 0:
    raise SystemExit("REPEATS must be positive")
samples = {language: {name: [] for name in names} for language in commands}
for cycle in range(repeats):
    # Rotate the order so the first measurement always belongs to a different language.
    languages = list(commands)
    order = languages[cycle % len(languages):] + languages[:cycle % len(languages)]
    for language in order:
        output = subprocess.check_output(commands[language], text=True).splitlines()
        rows = [line.split() for line in output]
        if [(row[0], row[2]) for row in rows] != list(zip(names, expected)):
            raise SystemExit(f"{language} checksums disagree: {output!r}")
        for name, elapsed, _ in rows:
            samples[language][name].append(float(elapsed))

print("case Aura_ms Rust_ms Python_ms checksum")
for name, checksum in zip(names, expected):
    times = [f"{median(samples[language][name]):.3f}" for language in commands]
    print(name, *times, checksum)
