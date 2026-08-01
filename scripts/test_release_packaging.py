#!/usr/bin/env python3
"""Regression tests for release workflow and packaged-CLI safety contracts."""

from __future__ import annotations

import os
from pathlib import Path
import signal
import subprocess
import tarfile
import tempfile
import textwrap
import time
import unittest


REPO_ROOT = Path(__file__).resolve().parents[1]
RELEASE_WORKFLOW = REPO_ROOT / ".github" / "workflows" / "release.yml"
PACKAGE_SCRIPT = REPO_ROOT / "scripts" / "package-cli.sh"
SMOKE_SCRIPT = REPO_ROOT / "scripts" / "smoke-cli-archive.sh"
FINAL_REPORT = REPO_ROOT / "work" / "2026-07-31-batch6-final-report.md"


RETRY_STDOUT = """\
recover request 1
recover retry 4ms
recover request 2
recover result 200
rate request 1
rate retry 6ms
rate request 2
rate result 429
exhaust request 1
exhaust retry 3ms
exhaust request 2
exhaust retry 5ms
exhaust request 3
exhaust result 503
requests 7
"""


class ReleaseWorkflowTests(unittest.TestCase):
    def setUp(self) -> None:
        self.workflow = RELEASE_WORKFLOW.read_text(encoding="utf-8")

    def test_manual_dispatch_is_build_only_by_default(self) -> None:
        self.assertIn("source_ref:", self.workflow)
        self.assertIn("release_tag:", self.workflow)
        dispatch_inputs = self.workflow.split("  workflow_dispatch:\n", 1)[1].split(
            "\npermissions:\n", 1
        )[0]
        self.assertIn(
            "      publish:\n"
            "        description: 'Publish a GitHub Release after all artifacts pass (manual opt-in)'\n"
            "        required: true\n"
            "        default: false\n"
            "        type: boolean",
            dispatch_inputs,
        )
        self.assertIn(
            "if: ${{ github.event_name == 'push' || inputs.publish }}",
            self.workflow,
        )

    def test_manual_source_and_release_identity_are_separate(self) -> None:
        checkout_ref = (
            "ref: ${{ github.event_name == 'workflow_dispatch' "
            "&& inputs.source_ref || github.ref }}"
        )
        release_identity = (
            "${{ github.event_name == 'workflow_dispatch' "
            "&& inputs.release_tag || github.ref_name }}"
        )
        self.assertEqual(self.workflow.count(checkout_ref), 1)
        self.assertGreaterEqual(self.workflow.count(release_identity), 4)
        self.assertIn(
            "target_commitish: "
            "${{ needs.release_identity.outputs.implementation_commit }}",
            self.workflow,
        )
        self.assertNotIn("inputs.tag", self.workflow)
        self.assertIn(f"path: aurora-docs-{release_identity}.tar.gz", self.workflow)

    def test_manual_publish_requires_tag_to_identify_immutable_source_commit(self) -> None:
        immutable_ref = "ref: ${{ needs.release_identity.outputs.implementation_commit }}"
        self.assertIn("release_identity:\n", self.workflow)
        self.assertIn(
            "implementation_commit: "
            "${{ steps.source.outputs.implementation_commit }}",
            self.workflow,
        )
        self.assertIn("id: source", self.workflow)
        self.assertEqual(self.workflow.count(immutable_ref), 2)
        self.assertIn("needs: [release_identity, cli, tools]", self.workflow)
        self.assertIn(
            "if: ${{ github.event_name == 'workflow_dispatch' && inputs.publish }}",
            self.workflow,
        )
        self.assertIn(
            'git fetch --force --no-tags origin '
            '"refs/tags/$RELEASE_TAG:refs/tags/$RELEASE_TAG"',
            self.workflow,
        )
        self.assertIn(
            'tag_commit="$(git rev-parse "$RELEASE_TAG^{commit}")"',
            self.workflow,
        )
        self.assertIn('if [[ "$source_commit" != "$tag_commit" ]]; then', self.workflow)
        self.assertIn(
            "target_commitish: "
            "${{ needs.release_identity.outputs.implementation_commit }}",
            self.workflow,
        )

    def test_docs_stamp_the_checked_out_source_commit(self) -> None:
        self.assertIn("id: release-metadata", self.workflow)
        self.assertIn(
            'echo "implementation_commit=$(git rev-parse HEAD)" >> "$GITHUB_OUTPUT"',
            self.workflow,
        )
        self.assertIn(
            "AURORA_DOCS_COMMIT: "
            "${{ steps.release-metadata.outputs.implementation_commit }}",
            self.workflow,
        )

    def test_release_identity_is_validated_and_not_interpolated_into_shell(self) -> None:
        release_identity = (
            "${{ github.event_name == 'workflow_dispatch' "
            "&& inputs.release_tag || github.ref_name }}"
        )
        self.assertGreaterEqual(
            self.workflow.count(f"RELEASE_TAG: {release_identity}"),
            2,
        )
        self.assertIn(
            '[[ ! "$RELEASE_TAG" =~ '
            '^v[0-9]+\\.[0-9]+\\.[0-9]+(-[0-9A-Za-z.-]+)?$ ]]',
            self.workflow,
        )
        self.assertIn("ARCHIVE_NAME: ${{ matrix.archive }}", self.workflow)
        self.assertIn('scripts/package-cli.sh "$ARCHIVE_NAME"', self.workflow)
        self.assertNotIn(
            'scripts/package-cli.sh "${{ matrix.archive }}"',
            self.workflow,
        )
        self.assertIn(
            f"DOCS_ARCHIVE_NAME: aurora-docs-{release_identity}.tar.gz",
            self.workflow,
        )
        self.assertIn('tar -czf "$DOCS_ARCHIVE_NAME"', self.workflow)

    def test_pushed_version_tags_still_publish(self) -> None:
        self.assertIn("push:\n    tags:\n      - 'v*'", self.workflow)
        self.assertIn(
            "if: ${{ github.event_name == 'push' || inputs.publish }}",
            self.workflow,
        )
        self.assertIn("permissions:\n      contents: write", self.workflow)

    def test_published_preview_is_prerelease_with_checksum_manifest(self) -> None:
        self.assertIn("- name: Generate SHA256SUMS", self.workflow)
        self.assertIn("sha256sum", self.workflow)
        self.assertIn("sha256sum -c SHA256SUMS", self.workflow)
        self.assertIn("prerelease: true", self.workflow)
        self.assertIn("files: release-assets/*", self.workflow)

    def test_handoff_documents_github_cli_authentication(self) -> None:
        handoff = FINAL_REPORT.read_text(encoding="utf-8")
        self.assertIn("gh auth login", handoff)
        self.assertIn("gh auth status", handoff)


class ArchiveLayoutTests(unittest.TestCase):
    def test_package_layout_is_stable_and_self_contained(self) -> None:
        script = PACKAGE_SCRIPT.read_text(encoding="utf-8")
        required_contracts = (
            'mkdir -p "$archive_root/bin" "$archive_root/lib/aurora" '
            '"$archive_root/examples/agents"',
            'cp target/release/aura "$archive_root/bin/aura"',
            'cp target/release/libaurora_compiler.a '
            '"$archive_root/lib/aurora/libaurora_compiler.a"',
            'cp examples/basic_addition.au "$archive_root/examples/basic_addition.au"',
            'cp examples/agents/retrying_network_worker.au '
            '"$archive_root/examples/agents/retrying_network_worker.au"',
            'Path(os.environ["ARCHIVE_ROOT"]) / '
            '"lib/aurora/native-link-args.json"',
            'cp README.md LICENSE "$archive_root/"',
            'cp crates/aura/README.md "$archive_root/AURA_CLI_README.md"',
            'tar -czf "$archive_name.tar.gz" -C release "$archive_name"',
        )
        for contract in required_contracts:
            with self.subTest(contract=contract):
                self.assertIn(contract, script)

    def test_package_script_rejects_path_like_archive_names_before_build(self) -> None:
        result = subprocess.run(
            ["bash", str(PACKAGE_SCRIPT), "../escape"],
            cwd=REPO_ROOT,
            text=True,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            timeout=10,
            check=False,
        )
        self.assertEqual(result.returncode, 2)
        self.assertIn("invalid release archive name", result.stderr)


class InstalledArchiveSmokeTests(unittest.TestCase):
    def test_archive_smoke_uses_copied_sources_without_cargo(self) -> None:
        with tempfile.TemporaryDirectory(prefix="aurora-release-test-") as temp:
            root = Path(temp)
            commit = subprocess.run(
                ["git", "rev-parse", "--verify", "--short=12", "HEAD^{commit}"],
                cwd=REPO_ROOT,
                text=True,
                stdout=subprocess.PIPE,
                check=True,
            ).stdout.strip()
            archive_root = root / "aurora-vtest-aarch64-apple-darwin"
            binary = archive_root / "bin" / "aura"
            binary.parent.mkdir(parents=True)
            packaged_basic = archive_root / "examples" / "basic_addition.au"
            packaged_retry = (
                archive_root / "examples" / "agents" / "retrying_network_worker.au"
            )
            packaged_retry.parent.mkdir(parents=True)
            packaged_basic.write_text("def main():\n    print(16)\n", encoding="utf-8")
            packaged_retry.write_text("def main():\n    print(\"requests 7\")\n", encoding="utf-8")
            log = root / "fake-aura.log"
            ambient_cache = root / "ambient-cache"
            ambient_cache.mkdir()
            stubborn_pid_file = root / "stubborn-child.pid"
            binary.write_text(
                textwrap.dedent(
                    f"""\
                    #!/usr/bin/env bash
                    set -euo pipefail
                    printf 'cwd=%s\\n' "$PWD" >> "$AURORA_SMOKE_TEST_LOG"
                    printf 'cargo=%s\\n' "${{CARGO:-}}" >> "$AURORA_SMOKE_TEST_LOG"
                    printf 'args=%s\\n' "$*" >> "$AURORA_SMOKE_TEST_LOG"
                    printf 'cache-args=%s|%s\\n' \
                      "${{AURORA_CACHE_DIR:-}}" "$*" >> "$AURORA_SMOKE_TEST_LOG"
                    if [[ -e "${{CARGO:-}}" ]]; then
                      echo "CARGO unexpectedly exists" >&2
                      exit 90
                    fi
                    if [[ "${{1:-}}" == "--version" ]]; then
                      echo "aura 0.2.0-preview ({commit})"
                    elif [[ "$*" == *"basic_addition.au" ]]; then
                      test -f "${{@: -1}}"
                      mkdir -p "$AURORA_CACHE_DIR"
                      echo "16"
                    elif [[ "$*" == *"retrying_network_worker.au" ]]; then
                      test -f "${{@: -1}}"
                      test -d "$AURORA_CACHE_DIR"
                      if [[ -n "${{AURORA_SMOKE_STUBBORN_PID:-}}" ]]; then
                        (trap '' TERM; exec sleep 300) >/dev/null 2>&1 &
                        printf '%s\n' "$!" > "$AURORA_SMOKE_STUBBORN_PID"
                      fi
                      printf '%b' {RETRY_STDOUT!r}
                    else
                      echo "unexpected arguments: $*" >&2
                      exit 91
                    fi
                    """
                ),
                encoding="utf-8",
            )
            binary.chmod(0o755)
            archive = root / "aurora-vtest-aarch64-apple-darwin.tar.gz"
            with tarfile.open(archive, "w:gz") as handle:
                handle.add(archive_root, arcname=archive_root.name)

            result = subprocess.run(
                ["bash", str(SMOKE_SCRIPT), str(archive)],
                cwd=REPO_ROOT,
                env={
                    **os.environ,
                    "AURORA_SMOKE_TEST_LOG": str(log),
                    "AURORA_SMOKE_STUBBORN_PID": str(stubborn_pid_file),
                    "AURORA_CACHE_DIR": str(ambient_cache),
                },
                text=True,
                stdout=subprocess.PIPE,
                stderr=subprocess.PIPE,
                timeout=30,
                check=False,
            )

            self.assertEqual(result.returncode, 0, result.stderr)
            self.assertIn(f"aura 0.2.0-preview ({commit})\n", result.stdout)
            self.assertIn("16\n", result.stdout)
            self.assertTrue(result.stdout.endswith(RETRY_STDOUT), result.stdout)
            self.assertNotIn(
                'cp "$repo_root/examples/basic_addition.au"',
                SMOKE_SCRIPT.read_text(encoding="utf-8"),
            )
            self.assertNotIn(
                'cp "$repo_root/examples/agents/retrying_network_worker.au"',
                SMOKE_SCRIPT.read_text(encoding="utf-8"),
            )
            self.assertIn(
                'grep -Fxq "$expected_version"',
                SMOKE_SCRIPT.read_text(encoding="utf-8"),
            )

            records = log.read_text(encoding="utf-8").splitlines()
            self.assertEqual(len([line for line in records if line.startswith("cwd=")]), 3)
            for line in records:
                if line.startswith("cwd="):
                    cwd = Path(line.removeprefix("cwd=")).resolve()
                    self.assertNotEqual(cwd, REPO_ROOT)
                    self.assertNotIn(REPO_ROOT, cwd.parents)
                elif line.startswith("cargo="):
                    cargo = Path(line.removeprefix("cargo="))
                    self.assertFalse(cargo.exists())
            self.assertTrue(
                any("run --backend direct" in line and "basic_addition.au" in line for line in records)
            )
            self.assertTrue(
                any(
                    "run --backend direct" in line
                    and "retrying_network_worker.au" in line
                    for line in records
                )
            )
            direct_cache_records = [
                line.removeprefix("cache-args=")
                for line in records
                if line.startswith("cache-args=")
                and "run --backend direct" in line
            ]
            self.assertEqual(len(direct_cache_records), 2)
            direct_caches = [Path(line.split("|", 1)[0]) for line in direct_cache_records]
            self.assertEqual(direct_caches[0], direct_caches[1])
            self.assertNotEqual(direct_caches[0], ambient_cache)
            self.assertEqual(direct_caches[0].name, "cache")
            self.assertFalse(direct_caches[0].exists())
            self.assertNotIn(REPO_ROOT, direct_caches[0].resolve().parents)

            stubborn_pid = int(stubborn_pid_file.read_text(encoding="utf-8"))
            for _ in range(100):
                try:
                    os.kill(stubborn_pid, 0)
                except ProcessLookupError:
                    break
                time.sleep(0.02)
            else:
                os.kill(stubborn_pid, signal.SIGKILL)
                self.fail(f"archive smoke left descendant process {stubborn_pid} running")


if __name__ == "__main__":
    unittest.main()
