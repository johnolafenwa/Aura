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

    def test_public_measurements_use_plain_factual_voice(self) -> None:
        roots = (
            ROOT / "README.md",
            ROOT / "CHANGELOG.md",
            ROOT / "docs",
            ROOT / "tutorials",
            ROOT / "examples",
            ROOT / "benchmarks",
            ROOT / "llms",
            ROOT / "marketplace",
            ROOT / "release-notes",
        )
        patterns = (
            re.compile(r"\bmeasured snapshot,\s+not\b", re.IGNORECASE),
            re.compile(
                r"\b(?:observations?|measurements?|results?|numbers?)\b"
                r"[^.]{0,180}\bnot (?:an? )?(?:portable|general|broad)\b",
                re.IGNORECASE,
            ),
            re.compile(
                r"\bnot (?:an? )?(?:portable|general|broad)\s+"
                r"(?:performance|speed|capacity|benchmark)\b",
                re.IGNORECASE,
            ),
            re.compile(
                r"\bnot (?:an? )?(?:CI\s+)?(?:performance|benchmark|release)\s+"
                r"(?:claim|promise|gate)\b",
                re.IGNORECASE,
            ),
            re.compile(r"\bno (?:floating-kernel )?vectorization claim\b", re.IGNORECASE),
            re.compile(
                r"\bmakes? no (?:portable )?"
                r"(?:performance|benchmark|vectorization)[^\n.]{0,40}\bclaim\b",
                re.IGNORECASE,
            ),
            re.compile(r"\bmust not be presented as\s+measurements?\b", re.IGNORECASE),
            re.compile(r"\bshould not be treated as\b", re.IGNORECASE),
            re.compile(r"\bmust not be presented as\b", re.IGNORECASE),
            re.compile(
                r"\bnot an? (?:portable claim|benchmark promise)\b",
                re.IGNORECASE,
            ),
            re.compile(
                r"\b(?:observations?|measurements?|results?|numbers?)\b"
                r"[^.]{0,180}\bnot an? guarantee\b",
                re.IGNORECASE,
            ),
            re.compile(
                r"\bwithout (?:becoming|turning it into) an? "
                r"(?:product|performance|marketing) claim\b",
                re.IGNORECASE,
            ),
            re.compile(r"\bnot representative of every application\b", re.IGNORECASE),
            re.compile(r"\bnot a robust capacity guarantee\b", re.IGNORECASE),
            re.compile(r"\b(?:do|does) not imply that\s+all integer work\b", re.IGNORECASE),
            re.compile(r"\bnot a claim of NumPy\b", re.IGNORECASE),
            re.compile(r"\bnot a stable\s+contract\b", re.IGNORECASE),
            re.compile(
                r"\b(?:does not|do not) (?:support|maintain)\s+"
                r"(?:an? )?[^.]{0,60}\bclaim\b",
                re.IGNORECASE,
            ),
            re.compile(
                r"\bmakes? no\s+[^.]{0,60}\b(?:claim|promise)\b",
                re.IGNORECASE,
            ),
            re.compile(
                r"\b(?:not claiming|does not claim)\s+"
                r"(?:feature parity|production stability|GPU programming)",
                re.IGNORECASE,
            ),
            re.compile(r"\bWhat Aura Does Not Claim Yet\b", re.IGNORECASE),
            re.compile(r"\bThose wider claims require separate evidence\b", re.IGNORECASE),
        )
        files: set[Path] = set()
        for root in roots:
            if root.is_file():
                files.add(root)
            elif root.exists():
                files.update(root.rglob("*.md"))
                files.update(root.rglob("*.txt"))
        files.discard(ROOT / f"docs/{OLD_LOWER}_language_proposal.md")
        stale: list[str] = []
        for path in sorted(files):
            text = path.read_text(encoding="utf-8")
            seen: set[tuple[int, str]] = set()
            for pattern in patterns:
                for match in pattern.finditer(text):
                    number = text.count("\n", 0, match.start()) + 1
                    line = text.splitlines()[number - 1].strip()
                    seen.add((number, line))
            stale.extend(
                f"{path.relative_to(ROOT)}:{number}: {line}"
                for number, line in sorted(seen)
            )
        self.assertEqual(
            stale,
            [],
            "defensive measurement disclaimers:\n" + "\n".join(stale),
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
