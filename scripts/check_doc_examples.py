#!/usr/bin/env python3
"""Compile every Aura code block in the documentation with the current CLI.

A fenced block opened with ```aura must pass `aura check`. Two markers in the
fence itself change that:

- ```aura fragment: a partial snippet that is not meant to compile alone.
  The check fails if the snippet starts compiling, so the marker never goes
  stale.
- ```aura check-fail:AU3004: an example of an error. It must fail with exactly
  that diagnostic code.

There is no manifest, no hash, and no expected output. Prose is never read.
"""

from __future__ import annotations

import argparse
import concurrent.futures
import os
import re
import subprocess
import sys
from dataclasses import dataclass
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]

# Documentation that may contain Aura code blocks.
SOURCES = (
    "README.md",
    "CONTRIBUTING.md",
    "docs",
    "tutorials",
    "examples",
    "crates/aura/README.md",
    "crates/aura-compiler/README.md",
    "tools/aura-language-server/README.md",
    "tools/vscode-aura/README.md",
    "tools/vscode-aura/INSTALL.md",
)
EXCLUDED_PARTS = {"node_modules", ".vitepress", "public"}
# Historical design documents keep their original, pre-implementation syntax.
EXCLUDED_FILES = {
    "docs/" + "auro" + "ra_language_proposal.md",
    "docs/ml_systems_support_plan.md",
}

OPENING = re.compile(r"^(?P<indent>[ \t]*)(?P<fence>`{3,})aura(?:[ \t]+(?P<marker>\S+))?[ \t]*$")
EXPECTED_FAILURE = re.compile(r"check-fail:(AU\d{4})")
DIAGNOSTIC = re.compile(r"^error\[(AU\d{4})\]:", re.MULTILINE)


@dataclass(frozen=True)
class Example:
    path: Path
    line: int
    marker: str
    source: str

    @property
    def label(self) -> str:
        return f"{self.path.relative_to(ROOT)}:{self.line}"


def markdown_files() -> list[Path]:
    files: list[Path] = []
    for entry in SOURCES:
        path = ROOT / entry
        candidates = [path] if path.is_file() else sorted(path.rglob("*.md"))
        for candidate in candidates:
            relative = candidate.relative_to(ROOT)
            if EXCLUDED_PARTS.intersection(relative.parts):
                continue
            if relative.as_posix() in EXCLUDED_FILES:
                continue
            files.append(candidate)
    return files


def examples(path: Path) -> list[Example]:
    lines = path.read_text(encoding="utf-8").splitlines()
    found: list[Example] = []
    index = 0
    while index < len(lines):
        opening = OPENING.match(lines[index])
        if not opening:
            index += 1
            continue
        indent = opening.group("indent")
        fence = opening.group("fence")
        marker = opening.group("marker") or "check-pass"
        start = index + 1
        end = start
        while end < len(lines) and lines[end].strip() != fence:
            end += 1
        if end == len(lines):
            raise ValueError(f"{path.relative_to(ROOT)}:{index + 1}: unclosed aura fence")
        body = [line[len(indent):] if line.startswith(indent) else line for line in lines[start:end]]
        found.append(Example(path, index + 1, marker, "\n".join(body) + "\n"))
        index = end + 1
    return found


def check(aura: Path, example: Example) -> str | None:
    if example.marker not in {"check-pass", "fragment"} and not EXPECTED_FAILURE.fullmatch(
        example.marker
    ):
        return f"{example.label}: unknown marker `{example.marker}`; use `fragment` or `check-fail:AUxxxx`"
    virtual_path = f"{example.path}#line-{example.line}.au"
    try:
        result = subprocess.run(
            [str(aura), "check", "--stdin", virtual_path],
            cwd=example.path.parent,
            input=example.source,
            text=True,
            capture_output=True,
            timeout=30,
            check=False,
        )
    except subprocess.TimeoutExpired:
        return f"{example.label}: `aura check` timed out"
    if result.returncode not in (0, 1):
        return f"{example.label}: `aura check` exited {result.returncode}\n{result.stderr}"
    actual = DIAGNOSTIC.search(result.stderr)
    if example.marker == "check-pass":
        if result.returncode == 0:
            return None
        return f"{example.label}: example does not compile\n{result.stderr}"
    if example.marker == "fragment":
        if result.returncode == 1:
            return None
        return f"{example.label}: fragment now compiles; drop the `fragment` marker"
    expected = EXPECTED_FAILURE.fullmatch(example.marker).group(1)
    if result.returncode == 1 and actual and actual.group(1) == expected:
        return None
    found = actual.group(1) if actual else "no error"
    return f"{example.label}: expected error[{expected}], found {found}\n{result.stderr}"


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("--aura", help="aura executable (default: $AURA_BIN or target/debug/aura)")
    parser.add_argument("--jobs", type=int, default=min(8, os.cpu_count() or 1))
    parser.add_argument("paths", nargs="*", help="limit the check to these Markdown files")
    args = parser.parse_args()

    aura = Path(args.aura or os.environ.get("AURA_BIN") or ROOT / "target" / "debug" / "aura")
    if not aura.is_absolute():
        aura = Path.cwd() / aura
    if not aura.is_file():
        print(f"aura executable not found at {aura}; run `cargo build -p aura`", file=sys.stderr)
        return 2

    files = [Path(p).resolve() for p in args.paths] if args.paths else markdown_files()
    try:
        work = [example for path in files for example in examples(path)]
    except ValueError as error:
        print(error, file=sys.stderr)
        return 1

    with concurrent.futures.ThreadPoolExecutor(max_workers=max(1, args.jobs)) as pool:
        failures = [failure for failure in pool.map(lambda e: check(aura, e), work) if failure]

    for failure in sorted(failures):
        print(failure, file=sys.stderr)
    counts: dict[str, int] = {}
    for example in work:
        kind = "check-fail" if example.marker.startswith("check-fail:") else example.marker
        counts[kind] = counts.get(kind, 0) + 1
    summary = ", ".join(f"{count} {kind}" for kind, count in sorted(counts.items()))
    if failures:
        print(f"{len(failures)} of {len(work)} documentation examples failed ({summary})", file=sys.stderr)
        return 1
    print(f"all {len(work)} documentation examples pass ({summary})")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
