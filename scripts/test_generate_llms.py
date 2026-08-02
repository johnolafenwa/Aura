#!/usr/bin/env python3
"""Behavioral tests for the build-derived llms.txt artifacts."""

from __future__ import annotations

import tempfile
import unittest
from pathlib import Path

import generate_llms


class GenerateLlmsTests(unittest.TestCase):
    def test_discovers_only_maintained_reader_sources_in_stable_order(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            (root / "README.md").write_text("# Aura\n\nRoot pitch.\n", encoding="utf-8")
            for relative, body in {
                "docs/index.md": "---\nlayout: home\n---\n\n# Home\n\nLanding pitch.\n",
                "docs/manual/index.md": "# Manual\n\nReference entry.\n",
                "docs/manual/types.md": "# Types\n\nType rules.\n",
                "docs/learn/index.md": "# Learn\n\nLearning entry.\n",
                "docs/learn/start.md": "# Start\n\nStart here.\n",
                "tutorials/02-next.md": "# Next\n\nSecond tutorial.\n",
                "tutorials/01-first.md": "# First\n\nFirst tutorial.\n",
                "work/history.md": "# Historical secret\n",
                "architecture_docs/decisions/0001-old.md": "# Old ADR\n",
            }.items():
                path = root / relative
                path.parent.mkdir(parents=True, exist_ok=True)
                path.write_text(body, encoding="utf-8")

            sources = generate_llms.discover_sources(root)

            self.assertEqual(
                [source.relative.as_posix() for source in sources],
                [
                    "README.md",
                    "docs/index.md",
                    "docs/manual/index.md",
                    "docs/manual/types.md",
                    "docs/learn/index.md",
                    "docs/learn/start.md",
                    "tutorials/01-first.md",
                    "tutorials/02-next.md",
                ],
            )

    def test_generation_strips_frontmatter_and_check_detects_stale_output(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            (root / "README.md").write_text("# Aura\n\nRoot pitch.\n", encoding="utf-8")
            manual = root / "docs/manual/index.md"
            manual.parent.mkdir(parents=True)
            manual.write_text(
                "---\ntitle: Hidden metadata\n---\n\n# Manual\n\nNormative reference.\n",
                encoding="utf-8",
            )

            outputs = generate_llms.render_outputs(root)
            self.assertIn("[Manual]", outputs["llms.txt"])
            self.assertIn("## Source: docs/manual/index.md", outputs["llms-full.txt"])
            self.assertNotIn("Hidden metadata", outputs["llms-full.txt"])

            generate_llms.write_outputs(root, outputs)
            self.assertEqual(generate_llms.stale_outputs(root, outputs), [])
            (root / "docs/public/llms.txt").write_text("stale\n", encoding="utf-8")
            self.assertEqual(generate_llms.stale_outputs(root, outputs), ["llms.txt"])


if __name__ == "__main__":
    unittest.main()
