#!/usr/bin/env python3
"""Release-metadata regression tests for the Aurora 0.2 preview."""

from __future__ import annotations

import json
import pathlib
import re
import unittest


ROOT = pathlib.Path(__file__).resolve().parents[1]
VERSION = "0.2.0"


class ReleaseMetadataTests(unittest.TestCase):
    def test_product_manifests_and_locks_agree_on_0_2_0(self) -> None:
        cargo_manifest = (ROOT / "Cargo.toml").read_text()
        workspace_version = re.search(
            r"\[workspace\.package\].*?^version\s*=\s*\"([^\"]+)\"",
            cargo_manifest,
            re.MULTILINE | re.DOTALL,
        )
        self.assertIsNotNone(workspace_version)
        self.assertEqual(workspace_version.group(1), VERSION)

        cargo_lock = (ROOT / "Cargo.lock").read_text()
        workspace_packages = {}
        for package in cargo_lock.split("[[package]]")[1:]:
            name_match = re.search(r'^name = "([^"]+)"$', package, re.MULTILINE)
            version_match = re.search(r'^version = "([^"]+)"$', package, re.MULTILINE)
            if (
                name_match is not None
                and version_match is not None
                and name_match.group(1) in {"aura", "aurora-compiler"}
            ):
                workspace_packages[name_match.group(1)] = version_match.group(1)
        self.assertEqual(
            workspace_packages,
            {"aura": VERSION, "aurora-compiler": VERSION},
        )

        fuzz_lock = (ROOT / "fuzz/Cargo.lock").read_text()
        fuzz_compiler = re.search(
            r'\[\[package\]\]\nname = "aurora-compiler"\nversion = "([^"]+)"',
            fuzz_lock,
        )
        self.assertIsNotNone(fuzz_compiler)
        self.assertEqual(fuzz_compiler.group(1), VERSION)

        manifests = {
            "root": ROOT / "package.json",
            "lsp": ROOT / "tools/aurora-language-server/package.json",
            "extension": ROOT / "tools/vscode-aurora/package.json",
        }
        for label, path in manifests.items():
            with self.subTest(label=label):
                self.assertEqual(json.loads(path.read_text())["version"], VERSION)

        root_lock = json.loads((ROOT / "package-lock.json").read_text())
        self.assertEqual(root_lock["version"], VERSION)
        self.assertEqual(root_lock["packages"][""]["version"], VERSION)
        self.assertEqual(
            root_lock["packages"]["tools/aurora-language-server"]["version"],
            VERSION,
        )
        self.assertEqual(
            root_lock["packages"]["tools/vscode-aurora"]["version"], VERSION
        )

        # This npm workspace intentionally uses one root lock. The LSP and
        # extension entries above are their lock records; package-local lock
        # files would split dependency resolution and are not maintained.

    def test_changelog_is_a_complete_0_2_preview_release_story(self) -> None:
        changelog = (ROOT / "CHANGELOG.md").read_text()
        self.assertIn("## 0.2.0 — 2026-07-31 (technical preview)", changelog)
        self.assertIn("technical preview", changelog.lower())
        for heading in (
            "Breaking changes and migration",
            "Language",
            "Runtime and structured concurrency",
            "Callables and closures",
            "Foreign function interface",
            "Numeric arrays",
            "Tooling and diagnostics",
            "Current limits",
        ):
            with self.subTest(heading=heading):
                self.assertIn(f"### {heading}", changelog)
        self.assertIn("scripts/capability_migrate.py apply", changelog)
        self.assertIn("one compatibility release", changelog)
        self.assertIn("ADR-0038", changelog)

    def test_manual_declares_release_and_dynamic_implementation_stamp(self) -> None:
        manual = (ROOT / "docs/manual/index.md").read_text()
        language_spec = (ROOT / "docs/manual/language-specification.md").read_text()
        current_limits = (ROOT / "docs/manual/current-limits.md").read_text()
        config = (ROOT / "docs/.vitepress/config.mts").read_text()
        metadata = (ROOT / "docs/.vitepress/release-metadata.mjs").read_text()
        component = (ROOT / "docs/.vitepress/theme/ReleaseStamp.vue").read_text()
        theme = (ROOT / "docs/.vitepress/theme/index.ts").read_text()

        self.assertIn("Aurora 0.2.0", manual)
        self.assertIn("technical preview", manual.lower())
        self.assertIn("implementation baseline commit", manual.lower())
        self.assertIn("AURORA_DOCS_COMMIT", metadata)
        self.assertIn("GITHUB_SHA", metadata)
        self.assertIn("local-uncommitted-checkout", manual)
        self.assertIn("git", metadata)
        self.assertIn("release-metadata.mjs", config)
        self.assertIn("implementationCommit", config)
        self.assertIn("__AURORA_IMPLEMENTATION_COMMIT__", component)
        self.assertIn("Implementation baseline commit", component)
        self.assertIn("ReleaseStamp", theme)

        self.assertNotIn(
            "passing for a copy type and shared borrowing for a non-copy",
            language_spec,
        )
        self.assertIn("Shared access for every type", language_spec)
        self.assertIn("Same-dtype scalar\n  broadcast is implemented", current_limits)
        self.assertIn("no array-shape broadcasting", current_limits)
        self.assertNotIn("There is no broadcasting", current_limits)

    def test_current_release_prose_no_longer_calls_itself_0_1(self) -> None:
        roots = (
            ROOT / "docs/manual",
            ROOT / "docs/learn",
            ROOT / "tutorials",
        )
        files = [ROOT / "SUPPORTED_PLATFORMS.md", ROOT / "crates/aura/README.md"]
        for directory in roots:
            files.extend(directory.glob("*.md"))
            files.extend(directory.glob("**/*.md"))

        stale: list[str] = []
        pattern = re.compile(r"\bAurora 0\.1(?:\.x)?\b|\b0\.1\.x\b")
        for path in sorted(set(files)):
            for number, line in enumerate(path.read_text().splitlines(), start=1):
                if pattern.search(line):
                    stale.append(f"{path.relative_to(ROOT)}:{number}: {line.strip()}")
        self.assertEqual(stale, [], "stale current-release prose:\n" + "\n".join(stale))


if __name__ == "__main__":
    unittest.main()
