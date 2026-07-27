#!/usr/bin/env python3
"""ADR-0022 capability-syntax migrator.

Rewrites the retired `borrow` spellings to the ratified bare/`mut`/`own`
surface. The rewrite is token-aware: `borrow` inside comments, string bodies,
or identifiers is never touched, which is what lets the same pass run over the
78 scratch-corpus files that no longer parse under the current grammar.

Mechanical rewrites, all local to the token and its immediate neighbours:

    borrow self          -> self
    borrow mut self      -> mut self
    borrow T             -> T
    borrow mut T         -> mut T
    borrow[label] T      -> T                (labels retired with ADR-0009)
    -> borrow[label] T   -> -> T
    match borrow X       -> match X
    match borrow mut X   -> match mut X
    for v in borrow X    -> for v in X
    for v in borrow mut X-> for v in mut X
    for v in mut range() -> for v in range()  (additional ruling)
    for v in own range() -> for v in range()

One semantic rewrite. Bare `match` flips from consuming to shared, so a bare
match that both selects a *place* and *binds a payload* is annotated
`match own` to preserve today's behavior. A match over a temporary is left
bare: the scrutinee has no surviving owner, so the flip is unobservable.
A match that binds nothing moves nothing.

That rule is deliberately conservative rather than exhaustive. Anything it
misses becomes a compile error after the flip, because moving a payload out of
a bare match is rejected with an exact "write `match own <place>` to consume"
diagnostic. The migrator never has to guess silently.

Usage:
    python3 scripts/capability_migrate.py build   [--manifest PATH]
    python3 scripts/capability_migrate.py check   [--manifest PATH]
    python3 scripts/capability_migrate.py apply   [--manifest PATH]
"""

from __future__ import annotations

import argparse
import hashlib
import json
import re
import subprocess
import sys
from pathlib import Path

MANIFEST_VERSION = 1
DEFAULT_MANIFEST = "scripts/capability-migration.json"

BORROW = re.compile(r"(?<![A-Za-z0-9_])borrow(?![A-Za-z0-9_])")
LABEL = r"(?:\[\s*[A-Za-z_][A-Za-z0-9_]*\s*\])?"


class HashMismatch(RuntimeError):
    """Raised when a file changed after its manifest entry was recorded."""


def _mask(source: str) -> str:
    """Return `source` with comments and string bodies blanked.

    Offsets and newlines are preserved, so a match found in the mask indexes
    directly into the original text.
    """
    out = list(source)
    i, n = 0, len(source)
    while i < n:
        ch = source[i]
        if ch == "#":
            while i < n and source[i] != "\n":
                out[i] = " "
                i += 1
            continue
        if ch in "\"'":
            quote = ch
            close = quote * 3 if source.startswith(quote * 3, i) else quote
            for offset in range(len(close)):
                out[i + offset] = " "
            i += len(close)
            while i < n and not source.startswith(close, i):
                if source[i] == "\\":
                    out[i] = " "
                    i += 1
                    if i < n:
                        out[i] = " "
                        i += 1
                    continue
                if source[i] != "\n":
                    out[i] = " "
                i += 1
            for offset in range(min(len(close), n - i)):
                out[i + offset] = " "
            i += len(close)
            continue
        i += 1
    return "".join(out)


def count_borrow_keywords(source: str) -> int:
    """Count `borrow` keyword tokens, ignoring comments and string bodies."""
    return len(BORROW.findall(_mask(source)))


def _substitute(source: str, pattern: re.Pattern[str], replace) -> str:
    """Apply `pattern` to the masked view, splicing edits into the original."""
    result = []
    cursor = 0
    for found in pattern.finditer(_mask(source)):
        start, end = found.span()
        # Re-match against the real text so the replacement sees real content.
        real = pattern.match(source, start, end)
        if real is None:
            continue
        result.append(source[cursor:start])
        result.append(replace(real))
        cursor = end
    result.append(source[cursor:])
    return "".join(result)


# Ordered because `borrow mut` must be recognized before bare `borrow`, and
# receivers before the general parameter form.
# `borrow` is always anchored by `KW`, so `borrowed`, `reborrow`, and
# `borrow_count` stay identifiers and are never rewritten.
KW = r"(?<![A-Za-z0-9_])borrow(?![A-Za-z0-9_])"

_RULES: list[tuple[re.Pattern[str], object]] = [
    # Receivers.
    (re.compile(KW + r"\s+mut\s+self\b"), lambda m: "mut self"),
    (re.compile(KW + r"\s+self\b"), lambda m: "self"),
    # `match` and `for ... in` capability prefixes.
    (re.compile(r"\bmatch\s+" + KW + r"\s+mut\s+"), lambda m: "match mut "),
    (re.compile(r"\bmatch\s+" + KW + r"\s+"), lambda m: "match "),
    (re.compile(r"\bin\s+" + KW + r"\s+mut\s+"), lambda m: "in mut "),
    (re.compile(r"\bin\s+" + KW + r"\s+"), lambda m: "in "),
    # Range iteration takes no capability modifier (additional ruling).
    (re.compile(r"\bin\s+(?:mut|own)\s+(range\s*\()"), lambda m: f"in {m.group(1)}"),
    # Return annotations lose the loan entirely; labels are retired.
    (re.compile(r"->\s*" + KW + r"\s*" + LABEL + r"\s*"), lambda m: "-> "),
    # Parameter and local type positions.
    (re.compile(KW + r"\s+mut\s*" + LABEL + r"\s*"), lambda m: "mut "),
    (re.compile(KW + r"\s*" + LABEL + r"\s*"), lambda m: ""),
]

_BARE_MATCH = re.compile(
    r"(?m)^(?P<indent>[ \t]*)match[ \t]+"
    r"(?P<scrutinee>[A-Za-z_][A-Za-z0-9_]*(?:\.[A-Za-z_][A-Za-z0-9_]*)*"
    r"(?:\[[^\]\n]*\])?)[ \t]*:[ \t]*$"
)
_CASE_BINDING = re.compile(
    r"(?m)^[ \t]*case\b[^\n:]*\(\s*(?!_[\s,)])[A-Za-z_][A-Za-z0-9_]*"
)


def _annotate_consuming_matches(source: str) -> str:
    """Add `own` to bare matches that select a place and bind a payload."""
    masked = _mask(source)
    lines = source.splitlines(keepends=True)
    starts, offset = [], 0
    for line in lines:
        starts.append(offset)
        offset += len(line)

    edits = []
    for found in _BARE_MATCH.finditer(masked):
        # A trailing `(` in the scrutinee means a call, i.e. a temporary.
        indent = found.group("indent")
        line_index = next(
            i for i in range(len(starts) - 1, -1, -1) if starts[i] <= found.start()
        )
        block = []
        for line in lines[line_index + 1 :]:
            stripped = line.strip()
            if stripped and not line.startswith(indent + " ") and not line.startswith(indent + "\t"):
                break
            block.append(line)
        if not _CASE_BINDING.search(_mask("".join(block))):
            continue
        insert = found.start() + len(indent) + len("match")
        edits.append(insert)

    for insert in reversed(edits):
        source = source[:insert] + " own" + source[insert:]
    return source


def migrate_aurora(source: str) -> str:
    """Migrate one Aurora source text. Deterministic and idempotent."""
    for pattern, replace in _RULES:
        source = _substitute(source, pattern, replace)
    return _annotate_consuming_matches(source)


def migrate_markdown(source: str) -> str:
    """Migrate fenced code blocks and inline code spans only.

    ADR-0022 Q7 retires the keyword but not the English word, so prose is left
    exactly as written.
    """
    out, fenced = [], False
    for line in source.splitlines(keepends=True):
        body = line.rstrip("\n")
        newline = line[len(body) :]
        if body.lstrip().startswith("```"):
            fenced = not fenced
            out.append(line)
            continue
        if fenced:
            out.append(migrate_aurora(body) + newline)
            continue
        out.append(
            re.sub(
                r"`([^`\n]*)`",
                lambda m: "`" + migrate_aurora(m.group(1)) + "`",
                body,
            )
            + newline
        )
    return "".join(out)


def migrate_text(path: Path, text: str) -> str:
    if path.suffix == ".md":
        return migrate_markdown(text)
    return migrate_aurora(text)


def _digest(text: str) -> str:
    return hashlib.sha256(text.encode("utf-8")).hexdigest()


def build_manifest(root: Path, paths: list[Path]) -> dict:
    """Record every file the migration would change, with before/after hashes."""
    entries = []
    for path in paths:
        text = path.read_text(encoding="utf-8", errors="strict")
        migrated = migrate_text(path, text)
        if migrated == text:
            continue
        entries.append(
            {
                "path": str(path.relative_to(root)),
                "before": _digest(text),
                "after": _digest(migrated),
            }
        )
    entries.sort(key=lambda entry: entry["path"])
    return {"version": MANIFEST_VERSION, "files": entries}


def _classify(root: Path, manifest: dict) -> tuple[list[str], list[str]]:
    """Split manifest entries into (pending, already-migrated), or raise."""
    pending, done = [], []
    for entry in manifest["files"]:
        path = root / entry["path"]
        if not path.exists():
            raise HashMismatch(f"{entry['path']}: listed in the manifest but missing")
        digest = _digest(path.read_text(encoding="utf-8"))
        if digest == entry["before"]:
            pending.append(entry["path"])
        elif digest == entry["after"]:
            done.append(entry["path"])
        else:
            raise HashMismatch(
                f"{entry['path']}: content matches neither the recorded "
                f"pre-migration nor post-migration hash; rebuild the manifest"
            )
    return pending, done


def check_manifest(root: Path, manifest: dict) -> list[str]:
    """Return the still-unmigrated paths without writing anything."""
    pending, _ = _classify(root, manifest)
    return pending


def apply_manifest(root: Path, manifest: dict) -> list[str]:
    """Migrate every pending entry. Returns the paths actually rewritten."""
    pending, _ = _classify(root, manifest)
    for relative in pending:
        path = root / relative
        text = path.read_text(encoding="utf-8")
        path.write_text(migrate_text(path, text), encoding="utf-8")
    return pending


def _repo_root() -> Path:
    return Path(
        subprocess.run(
            ["git", "rev-parse", "--show-toplevel"],
            capture_output=True,
            text=True,
            check=True,
        ).stdout.strip()
    )


def _maintained(root: Path) -> list[Path]:
    listing = subprocess.run(
        ["git", "ls-files", "*.au", "*.md"],
        cwd=root,
        capture_output=True,
        text=True,
        check=True,
    ).stdout.split()
    return [
        root / name
        for name in listing
        if ".vitepress/dist" not in name
        # Historical ADR context and migration documentation keep old spellings.
        and not name.startswith("architecture_docs/decisions/")
    ]


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("mode", choices=["build", "check", "apply"])
    parser.add_argument("--manifest", default=None)
    args = parser.parse_args(argv)

    root = _repo_root()
    manifest_path = Path(args.manifest) if args.manifest else root / DEFAULT_MANIFEST

    if args.mode == "build":
        manifest = build_manifest(root, _maintained(root))
        manifest_path.write_text(json.dumps(manifest, indent=2) + "\n")
        print(f"recorded {len(manifest['files'])} files in {manifest_path}")
        return 0

    manifest = json.loads(manifest_path.read_text())
    try:
        if args.mode == "check":
            pending = check_manifest(root, manifest)
            if pending:
                print(f"{len(pending)} files still need migration:", file=sys.stderr)
                for path in pending[:20]:
                    print(f"  {path}", file=sys.stderr)
                return 1
            print(f"all {len(manifest['files'])} manifest files are migrated")
            return 0
        changed = apply_manifest(root, manifest)
        print(f"migrated {len(changed)} files")
        return 0
    except HashMismatch as error:
        print(f"error: {error}", file=sys.stderr)
        return 2


if __name__ == "__main__":
    raise SystemExit(main())
