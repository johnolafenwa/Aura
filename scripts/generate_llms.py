#!/usr/bin/env python3
"""Generate the public llms.txt and llms-full.txt documentation artifacts."""

from __future__ import annotations

import argparse
import re
import sys
from dataclasses import dataclass
from pathlib import Path
from typing import Dict, Iterable, List


SITE_BASE = "https://johnolafenwa.github.io/Aura"
OUTPUT_DIR = Path("docs/public")


@dataclass(frozen=True)
class Source:
    relative: Path
    title: str
    description: str
    content: str


def _without_frontmatter(text: str) -> str:
    normalized = text.replace("\r\n", "\n").replace("\r", "\n")
    if not normalized.startswith("---\n"):
        return normalized
    closing = normalized.find("\n---\n", 4)
    if closing < 0:
        return normalized
    return normalized[closing + len("\n---\n") :].lstrip("\n")


def _without_vue_components(text: str) -> str:
    """Drop self-closing Vue component tags used only by the rendered site.

    Files such as docs/index.md embed components (`<AgentDocs />`) that carry
    no meaning for a reader of the generated text, so they must not leak into
    the machine-readable documentation.
    """
    stripped = re.sub(r"^[ \t]*<[A-Z][A-Za-z0-9]*\s*/>[ \t]*\n?", "", text, flags=re.MULTILINE)
    return re.sub(r"\n{3,}", "\n\n", stripped)


def _title(text: str, fallback: str) -> str:
    match = re.search(r"^#\s+(.+?)\s*$", text, flags=re.MULTILINE)
    return match.group(1).strip() if match else fallback


def _description(text: str) -> str:
    in_fence = False
    paragraph: List[str] = []
    for raw_line in text.splitlines():
        line = raw_line.strip()
        if line.startswith("```"):
            in_fence = not in_fence
            continue
        if in_fence or not line or line.startswith(("#", "|", "- ", "* ", ">")):
            if paragraph:
                break
            continue
        paragraph.append(line)
    return " ".join(paragraph) or "Maintained Aura documentation."


def _ordered_markdown(directory: Path) -> Iterable[Path]:
    if not directory.exists():
        return []
    paths = sorted(directory.glob("*.md"), key=lambda path: path.name)
    return sorted(paths, key=lambda path: (path.name != "index.md", path.name))


def discover_sources(root: Path) -> List[Source]:
    candidates: List[Path] = []
    for relative in ("README.md", "docs/index.md", "docs/positioning.md", "docs/downloads.md"):
        path = root / relative
        if path.is_file():
            candidates.append(path)
    candidates.extend(_ordered_markdown(root / "docs/install"))
    candidates.extend(_ordered_markdown(root / "docs/manual"))
    candidates.extend(_ordered_markdown(root / "docs/learn"))
    candidates.extend(_ordered_markdown(root / "tutorials"))

    sources: List[Source] = []
    seen = set()
    for path in candidates:
        relative = path.relative_to(root)
        if relative in seen:
            continue
        seen.add(relative)
        content = (
            _without_vue_components(
                _without_frontmatter(path.read_text(encoding="utf-8"))
            ).strip()
            + "\n"
        )
        sources.append(
            Source(
                relative=relative,
                title=(
                    "Aura"
                    if relative == Path("docs/index.md")
                    else _title(content, path.stem.replace("-", " ").title())
                ),
                description=_description(content),
                content=content,
            )
        )
    return sources


def _public_url(relative: Path) -> str:
    parts = list(relative.parts)
    if parts == ["README.md"]:
        return "https://github.com/johnolafenwa/Aura#readme"
    if parts[:1] == ["tutorials"]:
        return f"https://github.com/johnolafenwa/Aura/blob/main/{relative.as_posix()}"
    if parts[:1] == ["docs"]:
        parts = parts[1:]
    if parts[-1] == "index.md":
        parts = parts[:-1]
    else:
        parts[-1] = Path(parts[-1]).stem
    suffix = "/".join(parts)
    return SITE_BASE + (f"/{suffix}" if suffix else "")


def _summary(sources: List[Source]) -> str:
    groups = [
        ("Project", lambda source: source.relative == Path("README.md") or source.relative.parent == Path("docs")),
        ("Install Aura", lambda source: source.relative.parts[:2] == ("docs", "install")),
        ("Language Manual", lambda source: source.relative.parts[:2] == ("docs", "manual")),
        ("Learn Aura", lambda source: source.relative.parts[:2] == ("docs", "learn")),
        ("Tutorials", lambda source: source.relative.parts[:1] == ("tutorials",)),
    ]
    lines = [
        "# Aura",
        "",
        "> Aura is a compiled, statically typed systems language with Python-like syntax, deterministic ownership, native execution, and no garbage collector. It is designed for reliable ML systems and agents.",
        "",
    ]
    for heading, predicate in groups:
        selected = [source for source in sources if predicate(source)]
        if not selected:
            continue
        lines.extend((f"## {heading}", ""))
        for source in selected:
            lines.append(
                f"- [{source.title}]({_public_url(source.relative)}): {source.description}"
            )
        lines.append("")
    return "\n".join(lines).rstrip() + "\n"


def _full(sources: List[Source]) -> str:
    lines = [
        "# Aura full documentation",
        "",
        "> Build-derived from the maintained Aura README, website, Manual, Learn track, and tutorials.",
        "",
    ]
    for source in sources:
        lines.extend(
            (
                f"## Source: {source.relative.as_posix()}",
                "",
                source.content.rstrip(),
                "",
            )
        )
    return "\n".join(lines).rstrip() + "\n"


def render_outputs(root: Path) -> Dict[str, str]:
    sources = discover_sources(root)
    return {"llms.txt": _summary(sources), "llms-full.txt": _full(sources)}


def write_outputs(root: Path, outputs: Dict[str, str]) -> None:
    output_dir = root / OUTPUT_DIR
    output_dir.mkdir(parents=True, exist_ok=True)
    for name, content in outputs.items():
        (output_dir / name).write_text(content, encoding="utf-8")


def stale_outputs(root: Path, outputs: Dict[str, str]) -> List[str]:
    stale = []
    for name, expected in outputs.items():
        path = root / OUTPUT_DIR / name
        if not path.is_file() or path.read_text(encoding="utf-8") != expected:
            stale.append(name)
    return stale


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--check", action="store_true")
    parser.add_argument("--root", type=Path, default=Path(__file__).resolve().parent.parent)
    args = parser.parse_args()
    root = args.root.resolve()
    outputs = render_outputs(root)
    if args.check:
        stale = stale_outputs(root, outputs)
        if stale:
            print(
                "stale build-derived LLM documentation: " + ", ".join(stale),
                file=sys.stderr,
            )
            return 1
        print("Build-derived LLM documentation is current.")
        return 0
    write_outputs(root, outputs)
    print("Generated docs/public/llms.txt and docs/public/llms-full.txt.")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
