#!/usr/bin/env python3
"""Repository-wide identity and current-surface regression tests for Aura."""

from __future__ import annotations

from pathlib import Path
import re
import subprocess
import unittest


ROOT = Path(__file__).resolve().parents[1]
OLD = "Auro" + "ra"
OLD_LOWER = OLD.lower()


def tracked_paths() -> list[Path]:
    output = subprocess.run(
        ["git", "ls-files", "-z"],
        cwd=ROOT,
        check=True,
        stdout=subprocess.PIPE,
    ).stdout
    return [ROOT / item.decode() for item in output.split(b"\0") if item]


def is_history(path: Path) -> bool:
    relative = path.relative_to(ROOT).as_posix()
    if relative == "CHANGELOG.md" or relative.startswith("work/"):
        return True
    if relative.startswith("architecture_docs/decisions/"):
        return relative != "architecture_docs/decisions/README.md"
    return relative in {
        f"docs/{OLD_LOWER}_language_proposal.md",
        f"docs/{OLD_LOWER}_language_proposal.html",
    }


def readable_text(path: Path) -> str | None:
    try:
        return path.read_text(encoding="utf-8")
    except (UnicodeDecodeError, IsADirectoryError):
        return None


class AuraIdentityTests(unittest.TestCase):
    def test_maintained_tree_uses_one_product_identity(self) -> None:
        stale: list[str] = []
        for path in tracked_paths():
            if is_history(path):
                continue
            relative = path.relative_to(ROOT).as_posix()
            if OLD_LOWER in relative.lower():
                stale.append(f"path: {relative}")
            text = readable_text(path)
            if text is None:
                continue
            for number, line in enumerate(text.splitlines(), start=1):
                if (
                    relative == "architecture_docs/decisions/README.md"
                    and line.strip()
                    == "Pre-ADR-0042 documents use the former working name " + OLD + "."
                ):
                    continue
                if re.search(OLD_LOWER, line, re.IGNORECASE):
                    stale.append(f"{relative}:{number}: {line.strip()}")
        self.assertEqual(stale, [], "stale product identity:\n" + "\n".join(stale))

    def test_old_technical_contracts_are_absent(self) -> None:
        old_contracts = (
            OLD.upper() + "_",
            OLD_LOWER + "_direct_",
            OLD_LOWER + "-compiler",
            OLD_LOWER + "_compiler",
            OLD + ".toml",
            OLD + ".lock",
            "github.com/johnolafenwa/" + OLD,
            "source." + OLD_LOWER,
            OLD.encode().hex(),
            ", ".join(str(byte) for byte in OLD.encode()),
            OLD_LOWER.encode().hex(),
            ", ".join(str(byte) for byte in OLD_LOWER.encode()),
        )
        stale: list[str] = []
        for path in tracked_paths():
            if is_history(path):
                continue
            text = readable_text(path)
            if text is None:
                continue
            relative = path.relative_to(ROOT)
            for contract in old_contracts:
                if contract in text:
                    stale.append(f"{relative}: {contract}")
        for contract in old_contracts:
            with self.subTest(contract=contract):
                self.assertFalse(
                    any(item.endswith(f": {contract}") for item in stale),
                    "stale technical contracts:\n" + "\n".join(stale),
                )

    def test_old_identity_paths_are_absent_outside_history(self) -> None:
        stale = []
        for path in tracked_paths():
            if is_history(path):
                continue
            relative = path.relative_to(ROOT).as_posix()
            if OLD_LOWER in relative.lower():
                stale.append(relative)
            if path.name in {OLD + ".toml", OLD + ".lock"}:
                stale.append(relative)
        self.assertEqual(stale, [])

    def test_current_public_docs_do_not_narrate_removed_or_future_features(self) -> None:
        roots = (
            ROOT / "README.md",
            ROOT / "SECURITY.md",
            ROOT / "SUPPORTED_PLATFORMS.md",
            ROOT / "architecture_docs",
            ROOT / "crates",
            ROOT / "docs",
            ROOT / "examples",
            ROOT / "tutorials",
            ROOT / "tools",
        )
        patterns = (
            re.compile(r"\blegacy\b", re.IGNORECASE),
            re.compile(r"\bretired\b", re.IGNORECASE),
            re.compile(r"\bformerly\b", re.IGNORECASE),
            re.compile(r"\bsuperseded\b", re.IGNORECASE),
            re.compile(r"\bhistorical(?:ly)?\b", re.IGNORECASE),
            re.compile(r"\bfuture work\b", re.IGNORECASE),
            re.compile(r"\bAura\s+0\.3\b", re.IGNORECASE),
            re.compile(r"\b(?:borrow|return)[- ]source\b", re.IGNORECASE),
            re.compile(r"\breturn[- ]label\b", re.IGNORECASE),
            re.compile(r"\blifetime[- ]label\b", re.IGNORECASE),
            re.compile(r"\bfirst-class (?:loan|view)", re.IGNORECASE),
            re.compile(r"\bloan/view\b", re.IGNORECASE),
        )
        files: set[Path] = set()
        for root in roots:
            if root.is_file():
                files.add(root)
            elif root.exists():
                files.update(root.rglob("*.md"))
        stale: list[str] = []
        for path in sorted(files):
            if is_history(path):
                continue
            for number, line in enumerate(path.read_text().splitlines(), start=1):
                if any(pattern.search(line) for pattern in patterns):
                    stale.append(
                        f"{path.relative_to(ROOT)}:{number}: {line.strip()}"
                    )
        self.assertEqual(
            stale,
            [],
            "stale feature-history narrative:\n" + "\n".join(stale),
        )

    def test_required_aura_identity_surfaces_exist(self) -> None:
        required = (
            ROOT / "crates/aura-compiler/Cargo.toml",
            ROOT / "tools/aura-language-server/package.json",
            ROOT / "tools/vscode-aura/package.json",
            ROOT / "docs/public/aura-mark.svg",
        )
        self.assertEqual([path for path in required if not path.exists()], [])
        self.assertTrue(any(path.name == "Aura.toml" for path in tracked_paths()))


if __name__ == "__main__":
    unittest.main()
