#!/usr/bin/env python3
"""Compile every classified Aura tutorial fence against the current CLI."""

from __future__ import annotations

import argparse
import concurrent.futures
import os
from pathlib import Path
import re
import subprocess
import sys


CLASSIFICATIONS = {"check-pass", "fragment"}
EXPECTED_FAILURE = re.compile(r"check-fail:(AU\d{4})\Z")
DIAGNOSTIC = re.compile(r"^error\[(AU\d{4})\]:", re.MULTILINE)


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser()
    parser.add_argument("--aura", help="path to the aura executable")
    parser.add_argument("--jobs", type=int, default=min(8, os.cpu_count() or 1))
    return parser.parse_args()


def find_aura(repo: Path, requested: str | None) -> Path:
    candidate = requested or os.environ.get("AURA_BIN")
    path = Path(candidate) if candidate else repo / "target" / "debug" / "aura"
    if not path.is_absolute():
        path = repo / path
    if not path.is_file():
        raise SystemExit(
            f"Aura executable not found at {path}; run `cargo build -p aura` first"
        )
    return path


def fences(path: Path) -> list[tuple[int, str, str]]:
    lines = path.read_text(encoding="utf-8").splitlines()
    found: list[tuple[int, str, str]] = []
    index = 0
    line = 0
    while line < len(lines):
        opening = re.fullmatch(r"```aura(?:\s+(\S+))?", lines[line])
        if not opening:
            line += 1
            continue
        index += 1
        classification = opening.group(1)
        if classification is None:
            raise ValueError(f"{path}:{line + 1}: Aura fence has no classification")
        if classification not in CLASSIFICATIONS and not EXPECTED_FAILURE.fullmatch(
            classification
        ):
            raise ValueError(
                f"{path}:{line + 1}: unknown Aura fence classification `{classification}`"
            )
        start = line + 1
        line = start
        while line < len(lines) and lines[line] != "```":
            line += 1
        if line == len(lines):
            raise ValueError(f"{path}:{start}: unclosed Aura fence")
        found.append((index, classification, "\n".join(lines[start:line]) + "\n"))
        line += 1
    return found


def check_one(
    aura: Path, repo: Path, path: Path, index: int, classification: str, source: str
) -> str | None:
    virtual_path = f"{path.resolve()}#fence-{index}.au"
    try:
        result = subprocess.run(
            [str(aura), "check", "--stdin", virtual_path],
            cwd=repo,
            input=source,
            text=True,
            capture_output=True,
            timeout=15,
            check=False,
        )
    except subprocess.TimeoutExpired:
        return f"{path} fence {index}: compiler timed out"

    if result.returncode not in (0, 1):
        return (
            f"{path} fence {index}: compiler exited {result.returncode}\n"
            f"{result.stderr}"
        )
    if classification == "check-pass":
        if result.returncode == 0:
            return None
        return f"{path} fence {index}: expected check-pass\n{result.stderr}"

    if classification == "fragment":
        if result.returncode == 1:
            return None
        return (
            f"{path} fence {index}: contextual fragment now checks successfully; "
            "reclassify it as check-pass"
        )

    expected = EXPECTED_FAILURE.fullmatch(classification).group(1)
    actual = DIAGNOSTIC.search(result.stderr)
    if result.returncode == 1 and actual and actual.group(1) == expected:
        return None
    return (
        f"{path} fence {index}: expected error[{expected}], found "
        f"{actual.group(1) if actual else 'no diagnostic'}\n{result.stderr}"
    )


def main() -> int:
    args = parse_args()
    repo = Path(__file__).resolve().parent.parent
    aura = find_aura(repo, args.aura)
    work: list[tuple[Path, int, str, str]] = []
    counts = {"check-pass": 0, "check-fail": 0, "fragment": 0}

    try:
        for path in sorted((repo / "tutorials").glob("*.md")):
            for index, classification, source in fences(path):
                category = "check-fail" if classification.startswith("check-fail:") else classification
                counts[category] += 1
                work.append((path.relative_to(repo), index, classification, source))
    except ValueError as error:
        print(error, file=sys.stderr)
        return 1

    if len(work) < 300 or counts["check-pass"] < 190 or counts["check-fail"] < 5:
        print(f"tutorial classification coverage unexpectedly low: {counts}", file=sys.stderr)
        return 1

    failures: list[str] = []
    with concurrent.futures.ThreadPoolExecutor(max_workers=max(1, args.jobs)) as pool:
        futures = [
            pool.submit(check_one, aura, repo, path, index, classification, source)
            for path, index, classification, source in work
        ]
        for future in concurrent.futures.as_completed(futures):
            if failure := future.result():
                failures.append(failure)

    if failures:
        for failure in sorted(failures):
            print(failure, file=sys.stderr)
        print(
            f"tutorial fence gate failed: {len(failures)} of {len(work)} fences",
            file=sys.stderr,
        )
        return 1

    print(
        "tutorial fence gate passed: "
        f"{len(work)} total, {counts['check-pass']} check-pass, "
        f"{counts['check-fail']} check-fail, {counts['fragment']} fragments"
    )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
