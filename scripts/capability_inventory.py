"""ADR-0022 §1 syntax-aware inventory.

Counts, over maintained source, the three populations the migration must
handle:

  1. `borrow` keyword tokens (not the English word, not string contents),
  2. bare `match` statements, split by scrutinee shape,
  3. bare (`mode: Default`) parameters and receivers whose declared type is
     a declaration-known copy type.

Aurora source is tokenized well enough to skip comments and string bodies.
Markdown is scanned only inside fenced code blocks and inline code spans,
because ADR-0022 Q7 retires the keyword but not the English word.
"""

import json
import re
import subprocess
import sys
from pathlib import Path

ROOT = Path(
    subprocess.run(
        ["git", "rev-parse", "--show-toplevel"],
        capture_output=True, text=True, check=True,
    ).stdout.strip()
)
AURA = ROOT / "target/debug/aura"

# Declaration-known copy types. Anything not listed is treated as non-copy,
# which is the conservative direction for this inventory.
COPY_SCALARS = {
    "int8", "int16", "int32", "int64",
    "uint8", "uint16", "uint32", "uint64",
    "float32", "float64",
    "bool", "char", "int",
    "Duration", "Instant",
}


def strip_aurora(source: str) -> str:
    """Blank comments and string bodies, preserving offsets."""
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
            i += len(close)
            continue
        i += 1
    return "".join(out)


def markdown_code(source: str) -> str:
    """Keep fenced-block and inline-code text; blank the prose."""
    out, fenced = [], False
    for line in source.splitlines():
        if line.lstrip().startswith("```"):
            fenced = not fenced
            out.append("")
        elif fenced:
            out.append(line)
        else:
            out.append(" ".join(re.findall(r"`([^`]*)`", line)))
    return "\n".join(out)


def borrow_tokens(text: str) -> int:
    return len(re.findall(r"(?<![A-Za-z0-9_])borrow(?![A-Za-z0-9_])", text))


def tracked(pattern: str) -> list[Path]:
    listing = subprocess.run(
        ["git", "ls-files", pattern],
        cwd=ROOT, capture_output=True, text=True, check=True,
    ).stdout.split()
    return [ROOT / name for name in listing]


def is_copy_type(node) -> bool:
    if not isinstance(node, dict):
        return False
    name, args = node.get("name"), node.get("args") or []
    if args:
        # Tuples of copies are copy; every builtin container is not.
        return name == "Tuple" and all(is_copy_type(arg) for arg in args)
    return name in COPY_SCALARS


def walk(node, key, hits):
    if isinstance(node, dict):
        if key in node:
            hits.append(node[key])
        for value in node.values():
            walk(value, key, hits)
    elif isinstance(node, list):
        for value in node:
            walk(value, key, hits)


def scrutinee_shape(match_node) -> str:
    """Classify a bare match by what the flip to shared would change."""
    kind = (match_node.get("scrutinee") or {}).get("kind") or {}
    if "Name" in kind:
        return "place"
    if "Member" in kind or "Index" in kind:
        return "place"
    return "temporary"


def arm_moves_payload(match_node) -> bool:
    """True when any arm binds a pattern variable, i.e. extracts a payload."""
    hits = []
    walk(match_node.get("arms") or [], "bindings", hits)
    if any(hits):
        return True
    hits = []
    walk(match_node.get("arms") or [], "pattern", hits)
    for pattern in hits:
        names = []
        walk(pattern, "Binding", names)
        walk(pattern, "Name", names)
        if names:
            return True
    return False


def main() -> int:
    au_files = tracked("*.au")
    md_files = [p for p in tracked("*.md") if ".vitepress/dist" not in str(p)]
    rs_files = tracked("*.rs")

    au_borrow_files = au_borrow_tokens = 0
    receivers = {"bare": 0, "borrow": 0, "borrow_mut": 0, "own": 0}
    for path in au_files:
        stripped = strip_aurora(path.read_text(errors="replace"))
        count = borrow_tokens(stripped)
        if count:
            au_borrow_files += 1
            au_borrow_tokens += count
        for prefix in re.findall(
            r"def\s+[A-Za-z_][A-Za-z0-9_]*\s*\(\s*((?:borrow\s+mut|borrow|own)\s+)?self\b",
            stripped,
        ):
            key = " ".join(prefix.split()).replace(" ", "_") or "bare"
            receivers[key] += 1

    md_borrow_files = md_borrow_tokens = md_prose = 0
    for path in md_files:
        text = path.read_text(errors="replace")
        code = borrow_tokens(markdown_code(text))
        if code:
            md_borrow_files += 1
            md_borrow_tokens += code
        md_prose += borrow_tokens(text) - code

    # Rust sources carry `borrow` in embedded Aurora test sources and in
    # identifiers such as `ReceiverKind::Borrow`; only the quoted-source
    # occurrences migrate, so report them separately from identifiers.
    rs_lower = rs_ident = 0
    for path in rs_files:
        text = path.read_text(errors="replace")
        rs_lower += borrow_tokens(text)
        rs_ident += len(re.findall(r"(?<![A-Za-z0-9_])Borrow(?![A-Za-z0-9_])", text))

    stats = {
        "matches": 0,
        "bare_matches": 0,
        "explicit_borrow_matches": 0,
        "explicit_borrow_mut_matches": 0,
        "bare_matches_place_scrutinee": 0,
        "bare_matches_temporary_scrutinee": 0,
        "bare_matches_binding_payload": 0,
        "parameters": 0,
        "bare_parameters": 0,
        "bare_copy_parameters": 0,
        "explicit_borrow_parameters": 0,
        "explicit_borrow_mut_parameters": 0,
        "own_parameters": 0,
        "receivers": 0,
        "bare_receivers": 0,
        "explicit_borrow_receivers": 0,
        "explicit_borrow_mut_receivers": 0,
        "own_receivers": 0,
    }
    stats["receivers"] = sum(receivers.values())
    stats["bare_receivers"] = receivers["bare"]
    stats["explicit_borrow_receivers"] = receivers["borrow"]
    stats["explicit_borrow_mut_receivers"] = receivers["borrow_mut"]
    stats["own_receivers"] = receivers["own"]

    parsed, unparsed = 0, []

    for path in au_files:
        result = subprocess.run(
            [str(AURA), "ast-json", str(path)],
            cwd=ROOT, capture_output=True, text=True,
        )
        if result.returncode != 0:
            unparsed.append(str(path.relative_to(ROOT)))
            continue
        try:
            tree = json.loads(result.stdout)
        except json.JSONDecodeError:
            unparsed.append(str(path.relative_to(ROOT)))
            continue
        parsed += 1

        hits = []
        walk(tree, "Match", hits)
        for node in hits:
            stats["matches"] += 1
            mode = node.get("borrow_mode")
            if mode is None:
                stats["bare_matches"] += 1
                stats[f"bare_matches_{scrutinee_shape(node)}_scrutinee"] += 1
                if arm_moves_payload(node):
                    stats["bare_matches_binding_payload"] += 1
            elif mode == "Borrow":
                stats["explicit_borrow_matches"] += 1
            elif mode == "BorrowMut":
                stats["explicit_borrow_mut_matches"] += 1

        hits = []
        walk(tree, "params", hits)
        for group in hits:
            if not isinstance(group, list):
                continue
            for param in group:
                if not isinstance(param, dict) or "mode" not in param:
                    continue
                stats["parameters"] += 1
                mode = param["mode"]
                if mode == "Default":
                    stats["bare_parameters"] += 1
                    if is_copy_type(param.get("ty")):
                        stats["bare_copy_parameters"] += 1
                elif mode == "Borrow":
                    stats["explicit_borrow_parameters"] += 1
                elif mode == "BorrowMut":
                    stats["explicit_borrow_mut_parameters"] += 1
                elif mode == "Own":
                    stats["own_parameters"] += 1


    print(json.dumps({
        "au_files": len(au_files),
        "md_files": len(md_files),
        "rs_files": len(rs_files),
        "borrow_keyword_au_files": au_borrow_files,
        "borrow_keyword_au_tokens": au_borrow_tokens,
        "borrow_keyword_md_files": md_borrow_files,
        "borrow_keyword_md_tokens": md_borrow_tokens,
        "borrow_prose_md_words": md_prose,
        "borrow_lowercase_rs_tokens": rs_lower,
        "borrow_identifier_rs_tokens": rs_ident,
        "parsed_au_files": parsed,
        "unparsed_au_files": len(unparsed),
        **stats,
    }, indent=2))
    if unparsed:
        print(f"\nunparsed sample: {unparsed[:8]}", file=sys.stderr)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
