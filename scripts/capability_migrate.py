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

Three semantic populations are audited per occurrence. Every pre-flip bare
match over a place becomes `match own`, including copy matches and patterns
without bindings. Matches over temporaries enter the review queue: most can
remain bare, but nested ownership transfer can make `match own` load-bearing
even though the temporary itself has no surviving owner. Every
declaration-known bare copy parameter is emitted as an explicit review finding
so the maintainer can insert `own CopyType` only where its old value snapshot
and call-argument sequencing are load-bearing. Copy-valued borrowed returns
become ordinary owned returns, while non-copy or unresolved borrowed returns
are left for redesign and emitted as explicit findings.

The version-2 manifest is therefore both a hash ledger and a reviewable
semantic ledger. It records every silent-flip occurrence, its disposition and
the non-mechanical borrowed-return queue rather than relying on later compiler
errors to reveal heuristic misses.

Usage:
    python3 scripts/capability_migrate.py build   [--manifest PATH]
    python3 scripts/capability_migrate.py check   [--manifest PATH]
    python3 scripts/capability_migrate.py apply   [--manifest PATH]
"""

from __future__ import annotations

import argparse
from dataclasses import dataclass
import hashlib
import json
import re
import subprocess
import sys
from pathlib import Path

MANIFEST_VERSION = 2
DEFAULT_MANIFEST = "scripts/capability-migration.json"
ALLOWLIST_VERSION = 1
DEFAULT_ALLOWLIST = "scripts/capability-retired-syntax-allowlist.json"

BORROW = re.compile(r"(?<![A-Za-z0-9_])borrow(?![A-Za-z0-9_])")
LABEL = r"(?:\[\s*[A-Za-z_][A-Za-z0-9_]*\s*\])?"

# Source-shaped uses of the retired keyword. This deliberately does not match
# ordinary English such as "the parameter is borrowed" or "the borrow ends at
# the call boundary". Markdown and Rust are searched only inside code spans or
# user-facing diagnostic strings, respectively.
RETIRED_SOURCE_SYNTAX = re.compile(
    r"\bborrow\s+mut\b"
    r"|\bborrow\s+self\b"
    r"|\bborrow\s*\[\s*[A-Za-z_][A-Za-z0-9_]*\s*\]"
    r"|(?::|->)\s*borrow\b"
    r"|\bmatch\s+borrow\b"
    r"|\bin\s+borrow\b"
    r"|\bborrow\s+[A-Z][A-Za-z0-9_.]*(?:\b|\[)"
    r"|\bborrow\s+[a-z_][A-Za-z0-9_]*\s*(?=[),:])"
)
MARKDOWN_CODE = re.compile(
    r"```[\s\S]*?```"
    r"|~~~[\s\S]*?~~~"
    r"|(?<!`)`[^`]+`(?!`)"
    r"|(?m:(?:^(?: {4}|\t).*(?:\n|$))+)"
)
HTML_CODE = re.compile(r"<(?:code|pre)\b[^>]*>[\s\S]*?</(?:code|pre)>", re.IGNORECASE)
BACKTICK_CODE = re.compile(r"`([^`\n]+)`")
RETIREMENT_TEACHING = re.compile(
    r"\b(?:compatibility|migration|no longer|old spellings?|removed|replacement|"
    r"retired|used to|was formerly)\b",
    re.IGNORECASE,
)
HISTORICAL_PREFIXES = (
    "architecture_docs/decisions/",
    "work/",
)


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
    # Parameter and local type positions.
    (re.compile(KW + r"\s+mut\s*" + LABEL + r"\s*"), lambda m: "mut "),
    (re.compile(KW + r"\s*" + LABEL + r"\s*"), lambda m: ""),
]

_PLACE = re.compile(
    r"[A-Za-z_][A-Za-z0-9_]*(?:\.[A-Za-z_][A-Za-z0-9_]*)*"
    r"(?:\[[^\]\n]*\])?"
)
_VALUE_LITERAL = re.compile(
    r"(?:true|false|None|"
    r"(?:0[xX][0-9A-Fa-f_]+|0[bB][01_]+|[0-9][0-9_]*)(?:\\.[0-9_]+)?"
    r"(?:[eE][+-]?[0-9_]+)?(?:[A-Za-z][A-Za-z0-9]*)?)"
)
_BORROWED_RETURN = re.compile(
    r"->(?P<space>[ \t]*)" + KW + r"(?:[ \t]+mut)?[ \t]*" + LABEL
    + r"[ \t]*(?P<type>[^:\n]+?)(?P<trailing>[ \t]*)(?=:)"
)
_COPY_SCALARS = {
    "None",
    "bool",
    "int",
    "int8",
    "int16",
    "int32",
    "int64",
    "int128",
    "intsize",
    "uint8",
    "uint16",
    "uint32",
    "uint64",
    "uint128",
    "uintsize",
    "float32",
    "float64",
    "Duration",
}
_COPY_HANDLES = {"Queue", "Task"}
_STRUCTURAL_COPY = {"Option", "Result", "SendError", "QueueReceive"}
_KNOWN_MOVE = {
    "String",
    "Vec",
    "Map",
    "Set",
    "Range",
    "TaskResult",
    "WaitAny",
    "WaitAll",
    "TaskGroup",
    "random.Rng",
    "json.Value",
    "json.Error",
}
_BORROWED_RETURN_REVIEW_SENTINEL = "__AURORA_BORROWED_RETURN_REVIEW__"


@dataclass
class MigrationAnalysis:
    text: str
    occurrences: list[dict]
    findings: list[dict]


def _line_column(source: str, offset: int) -> tuple[int, int]:
    line = source.count("\n", 0, offset) + 1
    start = source.rfind("\n", 0, offset) + 1
    return line, offset - start + 1


def _split_top_level_with_offsets(text: str, separator: str = ",") -> list[tuple[int, str]]:
    parts: list[tuple[int, str]] = []
    start = 0
    stack: list[str] = []
    pairs = {")": "(", "]": "[", "}": "{"}
    quote = ""
    escaped = False
    for index, char in enumerate(text):
        if quote:
            if escaped:
                escaped = False
            elif char == "\\":
                escaped = True
            elif char == quote:
                quote = ""
            continue
        if char in "\"'":
            quote = char
        elif char in "([{":
            stack.append(char)
        elif char in ")]}":
            if stack and stack[-1] == pairs[char]:
                stack.pop()
        elif char == separator and not stack:
            parts.append((start, text[start:index]))
            start = index + 1
    parts.append((start, text[start:]))
    return parts


def _strip_top_level_default(type_text: str) -> str:
    parts = _split_top_level_with_offsets(type_text, "=")
    return parts[0][1].strip()


def _split_generic(type_text: str) -> tuple[str, list[str]] | None:
    type_text = type_text.strip()
    bracket = type_text.find("[")
    if bracket <= 0 or not type_text.endswith("]"):
        return None
    name = type_text[:bracket].strip()
    args = [
        part.strip()
        for _, part in _split_top_level_with_offsets(type_text[bracket + 1 : -1])
    ]
    return name, args


def _classify_copy_type(type_text: str) -> str:
    """Return copy, move, or unresolved from declaration-known type syntax."""
    text = type_text.strip()
    if text in _COPY_SCALARS:
        return "copy"
    if text in _KNOWN_MOVE:
        return "move"
    if text.startswith("(") and text.endswith(")"):
        members = [
            part.strip()
            for _, part in _split_top_level_with_offsets(text[1:-1])
            if part.strip()
        ]
        statuses = [_classify_copy_type(member) for member in members]
        if statuses and all(status == "copy" for status in statuses):
            return "copy"
        if any(status == "move" for status in statuses):
            return "move"
        return "unresolved"
    generic = _split_generic(text)
    if generic is not None:
        name, args = generic
        if name in _COPY_HANDLES and len(args) == 1:
            return "copy"
        if name in _STRUCTURAL_COPY:
            statuses = [_classify_copy_type(arg) for arg in args]
            if statuses and all(status == "copy" for status in statuses):
                return "copy"
            if any(status == "move" for status in statuses):
                return "move"
            return "unresolved"
        if name in _KNOWN_MOVE:
            return "move"
    return "unresolved"


def _matching_close(masked: str, opening: int) -> int | None:
    pairs = {"(": ")", "[": "]", "{": "}"}
    opening_char = masked[opening]
    closing_char = pairs.get(opening_char)
    if closing_char is None:
        return None
    depth = 1
    for index in range(opening + 1, len(masked)):
        if masked[index] == opening_char:
            depth += 1
        elif masked[index] == closing_char:
            depth -= 1
            if depth == 0:
                return index
    return None


def _bare_parameter_occurrences(source: str) -> list[dict]:
    masked = _mask(source)
    records: list[dict] = []
    for function in re.finditer(r"\bdef\s+[A-Za-z_][A-Za-z0-9_]*(?:\[[^\]\n]*\])?[ \t]*\(", masked):
        opening = masked.find("(", function.start(), function.end())
        closing = _matching_close(masked, opening)
        if closing is None:
            continue
        params = source[opening + 1 : closing]
        for relative, parameter in _split_top_level_with_offsets(params):
            colon = parameter.find(":")
            if colon < 0:
                continue
            after_colon = parameter[colon + 1 :]
            leading = len(after_colon) - len(after_colon.lstrip())
            type_start = opening + 1 + relative + colon + 1 + leading
            type_text = _strip_top_level_default(after_colon)
            if re.match(r"(?:own|mut|borrow)\b", type_text):
                continue
            classification = _classify_copy_type(type_text)
            if classification != "copy":
                continue
            name = parameter[:colon].strip()
            line, column = _line_column(source, type_start)
            records.append(
                {
                    "kind": "bare_copy_parameter",
                    "line": line,
                    "column": column,
                    "parameter": name,
                    "type": type_text,
                    "classification": classification,
                    "action": "review_required",
                    "reason": (
                        "the pre-flip value snapshot becomes logical shared access; "
                        "inspect maintained call sites and insert `own` only when "
                        "snapshot sequencing is load-bearing"
                    ),
                }
            )
    return records


def _bare_match_occurrences(source: str) -> list[dict]:
    records: list[dict] = []
    masked = _mask(source)
    for found in re.finditer(r"\bmatch[ \t]+", masked):
        start = found.end()
        if re.match(r"(?:borrow|mut|own)\b", masked[start:]):
            continue
        stack: list[str] = []
        pairs = {")": "(", "]": "[", "}": "{"}
        end = None
        for index in range(start, len(masked)):
            char = masked[index]
            if char in "([{":
                stack.append(char)
            elif char in ")]}":
                if stack and stack[-1] == pairs[char]:
                    stack.pop()
            elif char == ":" and not stack:
                end = index
                break
            elif char == "\n" and not stack:
                break
        if end is None:
            continue
        scrutinee = source[start:end].strip()
        if re.match(r"(?:borrow|mut|own)\b", scrutinee):
            continue
        classification = (
            "place"
            if _PLACE.fullmatch(scrutinee) and not _VALUE_LITERAL.fullmatch(scrutinee)
            else "temporary"
        )
        line, column = _line_column(source, found.start())
        record = {
            "kind": "bare_match",
            "line": line,
            "column": column,
            "scrutinee": scrutinee,
            "classification": classification,
            "action": "insert_own" if classification == "place" else "review_required",
            "reason": (
                "preserve the pre-flip consuming or copy-snapshot behavior"
                if classification == "place"
                else (
                    "the temporary has no surviving owner, but payload use must "
                    "be inspected for nested ownership transfer"
                )
            ),
        }
        if classification == "place":
            record["_insert"] = found.start() + len("match")
        records.append(record)
    return records


def _borrowed_return_occurrences(source: str) -> list[dict]:
    records: list[dict] = []
    for found in _BORROWED_RETURN.finditer(_mask(source)):
        type_text = source[found.start("type") : found.end("type")].strip()
        classification = _classify_copy_type(type_text)
        line, column = _line_column(source, found.start())
        action = "ordinary_owned_return" if classification == "copy" else "redesign_required"
        record = {
            "kind": (
                "borrowed_return"
                if classification == "copy"
                else "borrowed_return_redesign"
            ),
            "line": line,
            "column": column,
            "type": type_text,
            "classification": classification,
            "action": action,
            "reason": (
                "copy-valued borrowed returns become ordinary owned returns"
                if classification == "copy"
                else "a live non-copy loan cannot be preserved by deleting syntax"
            ),
        }
        if classification == "copy":
            record["_replace"] = (found.start(), found.end(), f"-> {type_text}")
        else:
            borrow_start = source.find("borrow", found.start(), found.end())
            record["_protect"] = (
                borrow_start,
                borrow_start + len("borrow"),
                _BORROWED_RETURN_REVIEW_SENTINEL,
            )
        records.append(record)
    return records


def _public_record(record: dict, path: str) -> dict:
    return {
        "path": path,
        **{
            key: value
            for key, value in record.items()
            if not key.startswith("_")
        },
    }


def analyze_aurora(source: str, path: str = "<memory>") -> MigrationAnalysis:
    """Migrate source and retain one auditable decision per semantic flip."""
    raw_records = (
        _bare_match_occurrences(source)
        + _bare_parameter_occurrences(source)
        + _borrowed_return_occurrences(source)
    )
    edits: list[tuple[int, int, str]] = []
    for record in raw_records:
        if "_insert" in record:
            insertion = " own" if record["kind"] == "bare_match" else "own "
            edits.append((record["_insert"], record["_insert"], insertion))
        if "_replace" in record:
            edits.append(record["_replace"])
        if "_protect" in record:
            edits.append(record["_protect"])
    for start, end, replacement in sorted(edits, reverse=True):
        source = source[:start] + replacement + source[end:]
    for pattern, replace in _RULES:
        source = _substitute(source, pattern, replace)
    source = source.replace(_BORROWED_RETURN_REVIEW_SENTINEL, "borrow")

    occurrences = sorted(
        (_public_record(record, path) for record in raw_records),
        key=lambda record: (
            record["path"],
            record["line"],
            record["column"],
            record["kind"],
        ),
    )
    findings = []
    for record in occurrences:
        if record["action"] == "redesign_required":
            findings.append(
                {
                    **record,
                    "message": (
                        "borrowed return requires redesign around an owned result, "
                        "clone, index, handle, or owner operation"
                    ),
                }
            )
        elif record["action"] == "review_required":
            if record["kind"] == "bare_copy_parameter":
                message = (
                    "bare copy parameter requires call-site review; write `own "
                    f"{record['type']}` only where pre-flip snapshot sequencing "
                    "is load-bearing"
                )
            else:
                message = (
                    "bare match over a temporary requires payload-use review; "
                    "write `match own ...` when an arm transfers nested ownership"
                )
            findings.append({**record, "message": message})
    return MigrationAnalysis(source, occurrences, findings)


def migrate_aurora(source: str) -> str:
    """Deterministically migrate one pre-flip Aurora source text.

    Semantic annotations run FIRST, against pre-migration text. `match borrow
    X`, `match borrow mut X`, and `value: borrow CopyType` are therefore not
    mistaken for the silent bare forms when their keywords are removed.
    Reapplication is guarded by the hash manifest because the canonical
    post-migration bare spelling cannot encode whether it came from an old
    explicit shared spelling.
    """
    return analyze_aurora(source).text


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
    """Record hashes plus every silent semantic-flip decision."""
    entries = []
    semantic_occurrences: list[dict] = []
    findings: list[dict] = []
    for path in paths:
        text = path.read_text(encoding="utf-8", errors="strict")
        relative = str(path.relative_to(root))
        if path.suffix == ".au":
            analysis = analyze_aurora(text, relative)
            migrated = analysis.text
            semantic_occurrences.extend(analysis.occurrences)
            findings.extend(analysis.findings)
        else:
            migrated = migrate_text(path, text)
        if migrated == text:
            continue
        entries.append(
            {
                "path": relative,
                "before": _digest(text),
                "after": _digest(migrated),
            }
        )
    entries.sort(key=lambda entry: entry["path"])
    return {
        "version": MANIFEST_VERSION,
        "files": entries,
        "semantic_occurrences": sorted(
            semantic_occurrences,
            key=lambda record: (
                record["path"],
                record["line"],
                record["column"],
                record["kind"],
            ),
        ),
        "findings": sorted(
            findings,
            key=lambda record: (
                record["path"],
                record["line"],
                record["column"],
                record["kind"],
            ),
        ),
    }


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
    """Return still-unmigrated paths without writing anything.

    The manifest is a one-shot migration ledger, not a permanent content lock.
    Once an entry has migrated, later edits are valid when running the
    migration over their current content is a no-op. ``apply_manifest`` stays
    hash-strict so it can never rewrite unreviewed drift.
    """
    pending = []
    for entry in manifest["files"]:
        path = root / entry["path"]
        if not path.exists():
            raise HashMismatch(f"{entry['path']}: listed in the manifest but missing")
        text = path.read_text(encoding="utf-8")
        digest = _digest(text)
        if digest == entry["before"]:
            pending.append(entry["path"])
        elif digest == entry["after"]:
            continue
        elif path.suffix == ".au" and count_borrow_keywords(text):
            raise HashMismatch(
                f"{entry['path']}: changed after migration and still contains "
                "retired capability syntax"
            )
    return pending


def unresolved_semantic_findings(manifest: dict) -> list[dict]:
    """Return review/redesign findings without a recorded resolution."""
    return [
        finding
        for finding in manifest.get("findings", [])
        if finding.get("status") != "resolved"
    ]


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


def _line_number(text: str, offset: int) -> int:
    return text.count("\n", 0, offset) + 1


def _paragraph(text: str, offset: int) -> str:
    start = text.rfind("\n\n", 0, offset)
    end = text.find("\n\n", offset)
    return text[start + 2 if start >= 0 else 0 : end if end >= 0 else len(text)]


def _relative(root: Path, path: Path) -> str:
    try:
        return str(path.relative_to(root))
    except ValueError:
        return str(path)


def _syntax_findings_in_code_regions(
    relative: str,
    text: str,
    regions: re.Pattern[str],
    *,
    allow_retirement_teaching: bool,
) -> list[str]:
    findings = []
    for region in regions.finditer(text):
        body = region.group(0)
        for found in RETIRED_SOURCE_SYNTAX.finditer(body):
            absolute = region.start() + found.start()
            if allow_retirement_teaching and RETIREMENT_TEACHING.search(
                _paragraph(text, absolute)
            ):
                continue
            line = _line_number(text, absolute)
            spelling = found.group(0).replace("\n", " ")
            findings.append(
                f"{relative}:{line}: retired capability syntax `{spelling}`"
            )
    return findings


def find_retired_syntax(
    root: Path,
    paths: list[Path],
    aurora_exemptions: dict[str, dict],
) -> list[str]:
    """Find retired source spellings without banning explanatory terminology.

    Aurora sources are token-scanned while ignoring comments and strings.
    Maintained Markdown/HTML is searched only in code regions. Rust and
    diagnostic snapshots are searched only in backticked user-facing syntax.
    Historical ADRs and work notes are records rather than current syntax and
    are intentionally outside this standing gate.
    """
    findings = []
    seen_aurora_exemptions = set()
    for path in paths:
        if not path.exists() or not path.is_file():
            continue
        relative = _relative(root, path)
        if relative.startswith(HISTORICAL_PREFIXES):
            continue
        text = path.read_text(encoding="utf-8", errors="strict")

        if path.suffix == ".au":
            masked = _mask(text)
            tokens = list(BORROW.finditer(masked))
            exemption = aurora_exemptions.get(relative)
            if exemption is not None:
                seen_aurora_exemptions.add(relative)
                expected = exemption["borrow_keywords"]
                if len(tokens) == expected:
                    continue
                findings.append(
                    f"{relative}: allowlist expects {expected} retired keyword "
                    f"token, found {len(tokens)}"
                    + ("s" if len(tokens) != 1 else "")
                )
                continue
            for token in tokens:
                findings.append(
                    f"{relative}:{_line_number(text, token.start())}: "
                    "retired `borrow` keyword in maintained Aurora source"
                )
            continue

        if path.suffix == ".md":
            findings.extend(
                _syntax_findings_in_code_regions(
                    relative,
                    text,
                    MARKDOWN_CODE,
                    allow_retirement_teaching=True,
                )
            )
            continue

        if path.suffix == ".html":
            findings.extend(
                _syntax_findings_in_code_regions(
                    relative,
                    text,
                    HTML_CODE,
                    allow_retirement_teaching=True,
                )
            )
            continue

        if path.suffix in {".diag", ".rs"}:
            for line_number, line in enumerate(text.splitlines(), 1):
                if path.suffix == ".rs" and line.lstrip().startswith(("//", "/*", "*")):
                    continue
                for span in BACKTICK_CODE.finditer(line):
                    found = RETIRED_SOURCE_SYNTAX.search(span.group(1))
                    if found is None or RETIREMENT_TEACHING.search(line):
                        continue
                    findings.append(
                        f"{relative}:{line_number}: retired capability syntax "
                        f"`{found.group(0)}` in maintained user-facing text"
                    )

    for relative, exemption in aurora_exemptions.items():
        if relative not in seen_aurora_exemptions:
            findings.append(
                f"{relative}: stale retired-syntax allowlist entry "
                f"({exemption['reason']})"
            )
    return sorted(set(findings))


def load_retired_syntax_allowlist(path: Path) -> dict[str, dict]:
    """Load and validate the exact Aurora-source retirement exemptions."""
    document = json.loads(path.read_text(encoding="utf-8"))
    if document.get("version") != ALLOWLIST_VERSION:
        raise ValueError(
            f"{path}: expected allowlist version {ALLOWLIST_VERSION}, "
            f"found {document.get('version')!r}"
        )
    exemptions = {}
    for entry in document.get("aurora_source_exemptions", []):
        relative = entry.get("path")
        count = entry.get("borrow_keywords")
        reason = entry.get("reason")
        if (
            not isinstance(relative, str)
            or not relative.endswith(".au")
            or not isinstance(count, int)
            or count < 1
            or not isinstance(reason, str)
            or not reason.strip()
        ):
            raise ValueError(f"{path}: invalid Aurora-source exemption {entry!r}")
        if relative in exemptions:
            raise ValueError(f"{path}: duplicate exemption for {relative}")
        exemptions[relative] = {
            "borrow_keywords": count,
            "reason": reason,
        }
    return exemptions


def _stale_syntax_paths(root: Path) -> list[Path]:
    listing = subprocess.run(
        [
            "git",
            "ls-files",
            "--cached",
            "--others",
            "--exclude-standard",
            "*.au",
            "*.md",
            "*.html",
            "*.diag",
            "*.rs",
        ],
        cwd=root,
        capture_output=True,
        text=True,
        check=True,
    ).stdout.splitlines()
    return [root / name for name in listing]


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("mode", choices=["build", "check", "apply"])
    parser.add_argument("--manifest", default=None)
    parser.add_argument("--allowlist", default=None)
    args = parser.parse_args(argv)

    root = _repo_root()
    manifest_path = Path(args.manifest) if args.manifest else root / DEFAULT_MANIFEST
    allowlist_path = (
        Path(args.allowlist) if args.allowlist else root / DEFAULT_ALLOWLIST
    )

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
            unresolved = unresolved_semantic_findings(manifest)
            if unresolved:
                print(
                    f"{len(unresolved)} unresolved semantic migration finding"
                    + ("s:" if len(unresolved) != 1 else ":"),
                    file=sys.stderr,
                )
                for finding in unresolved[:20]:
                    print(
                        f"  {finding['path']}:{finding['line']}: "
                        f"{finding['message']}",
                        file=sys.stderr,
                    )
                return 1
            exemptions = load_retired_syntax_allowlist(allowlist_path)
            stale = find_retired_syntax(
                root,
                _stale_syntax_paths(root),
                exemptions,
            )
            if stale:
                print(
                    f"{len(stale)} retired capability syntax finding"
                    + ("s:" if len(stale) != 1 else ":"),
                    file=sys.stderr,
                )
                for finding in stale:
                    print(f"  {finding}", file=sys.stderr)
                return 1
            print(f"all {len(manifest['files'])} manifest files are migrated")
            print("maintained source contains no unallowlisted retired syntax")
            return 0
        changed = apply_manifest(root, manifest)
        print(f"migrated {len(changed)} files")
        return 0
    except HashMismatch as error:
        print(f"error: {error}", file=sys.stderr)
        return 2


if __name__ == "__main__":
    raise SystemExit(main())
