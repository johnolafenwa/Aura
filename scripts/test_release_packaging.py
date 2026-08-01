#!/usr/bin/env python3
"""Regression tests for release workflow and packaged-CLI safety contracts."""

from __future__ import annotations

import os
from pathlib import Path
import runpy
import shutil
import signal
import subprocess
import tarfile
import tempfile
import textwrap
import time
import unittest


REPO_ROOT = Path(__file__).resolve().parents[1]
CI_WORKFLOW = REPO_ROOT / ".github" / "workflows" / "ci.yml"
DOCS_WORKFLOW = REPO_ROOT / ".github" / "workflows" / "docs.yml"
RELEASE_WORKFLOW = REPO_ROOT / ".github" / "workflows" / "release.yml"
PACKAGE_SCRIPT = REPO_ROOT / "scripts" / "package-cli.sh"
LINK_ARG_WRITER = REPO_ROOT / "scripts" / "write-native-link-args.py"
SMOKE_SCRIPT = REPO_ROOT / "scripts" / "smoke-cli-archive.sh"
FINAL_REPORT = REPO_ROOT / "work" / "2026-07-31-batch6-final-report.md"
DOWNLOADS_DOC = REPO_ROOT / "docs" / "downloads.md"
RELEASE_PROCESS_DOC = REPO_ROOT / "docs" / "release-process.md"


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


class HostedWorkflowHardeningTests(unittest.TestCase):
    def test_cargo_color_is_disabled_at_workflow_scope(self) -> None:
        for workflow_path in (CI_WORKFLOW, RELEASE_WORKFLOW):
            with self.subTest(workflow=workflow_path.name):
                workflow = workflow_path.read_text(encoding="utf-8")
                workflow_scope = workflow.split("\njobs:\n", 1)[0]
                self.assertIn("\nenv:\n  CARGO_TERM_COLOR: never\n", workflow_scope)

    def test_macos_ci_isolates_wall_clock_assertions_from_the_full_rust_suite(self) -> None:
        workflow = CI_WORKFLOW.read_text(encoding="utf-8")
        self.assertIn("if: runner.os == 'macOS'", workflow)
        self.assertIn('echo "RUST_TEST_THREADS=1" >> "$GITHUB_ENV"', workflow)

    def test_docs_use_node24_deploy_pages_release(self) -> None:
        workflow = DOCS_WORKFLOW.read_text(encoding="utf-8")
        self.assertIn(
            "actions/deploy-pages@cd2ce8fcbc39b97be8ca5fce6e763baed58fa128 # v5.0.0",
            workflow,
        )


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
            "      source_ref:\n"
            "        description: 'Optional commit, branch, or tag to build; defaults to release_tag'\n"
            "        required: false\n"
            "        default: ''\n"
            "        type: string",
            dispatch_inputs,
        )
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
        self.assertIn(
            "      publish_extension:\n"
            "        description: 'Publish the VSIX to configured extension marketplaces'\n"
            "        required: true\n"
            "        default: false\n"
            "        type: boolean",
            dispatch_inputs,
        )

    def test_extension_publish_job_supports_tag_dispatch_and_secretless_skips(self) -> None:
        self.assertIn("publish-extension:\n", self.workflow)
        self.assertIn("name: Publish VS Code extension", self.workflow)
        self.assertIn("name: Download VS Code extension", self.workflow)
        self.assertIn("name: Download VSIX from existing GitHub Release", self.workflow)
        self.assertIn("name: Confirm existing GitHub Release", self.workflow)
        self.assertIn("VSCE_PAT: ${{ secrets.VSCE_PAT }}", self.workflow)
        self.assertIn("OVSX_TOKEN: ${{ secrets.OVSX_TOKEN }}", self.workflow)
        self.assertIn("if: env.VSCE_PAT != ''", self.workflow)
        self.assertIn("if: env.VSCE_PAT == ''", self.workflow)
        self.assertIn("if: env.OVSX_TOKEN != ''", self.workflow)
        self.assertIn("if: env.OVSX_TOKEN == ''", self.workflow)
        self.assertIn(
            "npx @vscode/vsce publish --packagePath \"$VSIX_PATH\" -p \"$VSCE_PAT\"",
            self.workflow,
        )
        self.assertIn(
            "npx ovsx publish \"$VSIX_PATH\" -p \"$OVSX_TOKEN\"",
            self.workflow,
        )
        self.assertIn("::notice::VSCE_PAT is not configured", self.workflow)
        self.assertIn("::notice::OVSX_TOKEN is not configured", self.workflow)
        self.assertIn("inputs.publish_extension", self.workflow)
        self.assertIn(
            'gh release download "$RELEASE_TAG" --repo "$GITHUB_REPOSITORY"',
            self.workflow,
        )

    def test_extension_publish_job_validates_plain_marketplace_identity(self) -> None:
        self.assertIn("name: Validate VSIX Marketplace identity", self.workflow)
        self.assertIn("expected_version=\"${RELEASE_TAG#v}\"", self.workflow)
        self.assertIn("expected_version=\"${expected_version%%-*}\"", self.workflow)
        self.assertIn("JohnOlafenwa.vscode-aura", self.workflow)
        self.assertIn("0.2.0", self.workflow)

    def test_manual_source_and_release_identity_are_separate(self) -> None:
        checkout_ref = (
            "ref: ${{ github.event_name == 'workflow_dispatch' "
            "&& (inputs.source_ref || inputs.release_tag) || github.ref }}"
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
        self.assertIn(f"path: aura-docs-{release_identity}.tar.gz", self.workflow)

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
            "AURA_DOCS_COMMIT: "
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
            f"DOCS_ARCHIVE_NAME: aura-docs-{release_identity}.tar.gz",
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

    def test_downloads_and_release_process_cover_extension_distribution(self) -> None:
        downloads = DOWNLOADS_DOC.read_text(encoding="utf-8")
        release_process = RELEASE_PROCESS_DOC.read_text(encoding="utf-8")
        self.assertIn(
            "https://marketplace.visualstudio.com/items?itemName=JohnOlafenwa.vscode-aura",
            downloads,
        )
        self.assertIn(
            "https://open-vsx.org/extension/JohnOlafenwa/vscode-aura",
            downloads,
        )
        self.assertIn("aura-language.vsix", downloads)
        self.assertIn(
            "VSCE_PAT: global PATs are unsupported after 2026-12-01; renew as an "
            "org-scoped token (Marketplace -> Manage) and verify with "
            "`npx @vscode/vsce verify-pat JohnOlafenwa`.",
            release_process,
        )
        self.assertIn("OVSX_TOKEN", release_process)
        self.assertIn("hosted CI", release_process)
        self.assertIn("reliably green", release_process)


class ArchiveLayoutTests(unittest.TestCase):
    def test_package_layout_is_stable_and_self_contained(self) -> None:
        script = PACKAGE_SCRIPT.read_text(encoding="utf-8")
        required_contracts = (
            'mkdir -p "$archive_root/bin" "$archive_root/lib/aura" '
            '"$archive_root/examples/agents"',
            'cp target/release/aura "$archive_root/bin/aura"',
            'cp target/release/libaura_compiler.a '
            '"$archive_root/lib/aura/libaura_compiler.a"',
            'cp examples/basic_addition.au "$archive_root/examples/basic_addition.au"',
            'cp examples/agents/retrying_network_worker.au '
            '"$archive_root/examples/agents/retrying_network_worker.au"',
            'CARGO_TERM_COLOR=never cargo rustc',
            'python3 scripts/write-native-link-args.py',
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

    def test_packaged_native_link_args_strip_ansi_and_contain_no_controls(self) -> None:
        with tempfile.TemporaryDirectory(prefix="aura-link-args-") as temp:
            archive_root = Path(temp)
            result = subprocess.run(
                ["python3", str(LINK_ARG_WRITER)],
                cwd=REPO_ROOT,
                env={
                    **os.environ,
                    "ARCHIVE_ROOT": str(archive_root),
                    "NATIVE_STATIC_LIBS": (
                        "note: colored cargo output\n"
                        "native-static-libs: -lc\x1b[0m -lm\x1b[1;32m\x1b[0m\n"
                    ),
                },
                text=True,
                stdout=subprocess.PIPE,
                stderr=subprocess.PIPE,
                timeout=10,
                check=False,
            )
            self.assertEqual(result.returncode, 0, result.stderr)
            manifest = archive_root / "lib" / "aura" / "native-link-args.json"
            contents = manifest.read_text(encoding="utf-8")
            self.assertEqual(contents, '["-lc", "-lm"]\n')
            self.assertFalse(
                any(
                    ord(character) < 32 or ord(character) == 127
                    for character in contents.rstrip("\n")
                )
            )

    def test_packaged_native_link_args_reject_control_characters_before_write(self) -> None:
        with tempfile.TemporaryDirectory(prefix="aura-link-args-invalid-") as temp:
            archive_root = Path(temp)
            result = subprocess.run(
                ["python3", str(LINK_ARG_WRITER)],
                cwd=REPO_ROOT,
                env={
                    **os.environ,
                    "ARCHIVE_ROOT": str(archive_root),
                    "NATIVE_STATIC_LIBS": "native-static-libs: -lc\x7f-tainted\n",
                },
                text=True,
                stdout=subprocess.PIPE,
                stderr=subprocess.PIPE,
                timeout=10,
                check=False,
            )
            self.assertNotEqual(result.returncode, 0)
            self.assertIn("'-lc\\x7f-tainted'", result.stderr)
            self.assertIn("control character", result.stderr)
            self.assertFalse(
                (archive_root / "lib" / "aura" / "native-link-args.json").exists()
            )

    def test_packaged_link_arg_parser_rejects_malformed_ansi_sequences(self) -> None:
        native_link_args = runpy.run_path(str(LINK_ARG_WRITER))["native_link_args"]
        for cargo_output, rendered_prefix, rendered_control in (
            ("native-static-libs: -lc\x1b[\x00m", "'-lc", "\\x00"),
            (
                "native-static-libs: -lm\x1b]title\x01\x07",
                "'-lm",
                "\\x01",
            ),
        ):
            with self.subTest(cargo_output=repr(cargo_output)):
                with self.assertRaises(SystemExit) as raised:
                    native_link_args(cargo_output)
                self.assertIn(rendered_prefix, str(raised.exception))
                self.assertIn(rendered_control, str(raised.exception))
                self.assertIn("control character", str(raised.exception))

    def test_packaged_link_arg_parser_strips_complete_ansi_string_controls(self) -> None:
        native_link_args = runpy.run_path(str(LINK_ARG_WRITER))["native_link_args"]
        for introducer in ("P", "X", "^", "_"):
            with self.subTest(introducer=introducer):
                cargo_output = (
                    f"native-static-libs: -lc\x1b{introducer}payload\x1b\\ -lm"
                )
                self.assertEqual(native_link_args(cargo_output), ["-lc", "-lm"])
        for terminator in ("\x07", "\x1b\\"):
            with self.subTest(osc_terminator=repr(terminator)):
                cargo_output = f"native-static-libs: -lc\x1b]title{terminator} -lm"
                self.assertEqual(native_link_args(cargo_output), ["-lc", "-lm"])


class InstalledArchiveSmokeTests(unittest.TestCase):
    def test_archive_smoke_uses_copied_sources_without_cargo(self) -> None:
        with tempfile.TemporaryDirectory(prefix="aura-release-test-") as temp:
            root = Path(temp)
            commit = subprocess.run(
                ["git", "rev-parse", "--verify", "--short=12", "HEAD^{commit}"],
                cwd=REPO_ROOT,
                text=True,
                stdout=subprocess.PIPE,
                check=True,
            ).stdout.strip()
            archive_root = root / "aura-vtest-aarch64-apple-darwin"
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
                    #!/bin/sh
                    set -eu
                    printf 'cwd=%s\\n' "$PWD" >> "$AURA_SMOKE_TEST_LOG"
                    printf 'cargo=%s\\n' "${{CARGO:-}}" >> "$AURA_SMOKE_TEST_LOG"
                    printf 'args=%s\\n' "$*" >> "$AURA_SMOKE_TEST_LOG"
                    printf 'cache-args=%s|%s\\n' \
                      "${{AURA_CACHE_DIR:-}}" "$*" >> "$AURA_SMOKE_TEST_LOG"
                    if [ -f "${{CARGO:-}}" ]; then
                      echo "CARGO unexpectedly exists" >&2
                      exit 90
                    fi
                    case "${{1:-}}" in
                      --version)
                        echo "aura 0.2.0-preview ({commit})"
                        ;;
                      *)
                        last_argument=
                        for argument in "$@"; do
                          last_argument=$argument
                        done
                        case "$*" in
                          *basic_addition.au*)
                            test -f "$last_argument"
                            mkdir -p "$AURA_CACHE_DIR"
                            echo "16"
                            ;;
                          *retrying_network_worker.au*)
                            test -f "$last_argument"
                            test -d "$AURA_CACHE_DIR"
                            if [ -n "${{AURA_SMOKE_STUBBORN_PID:-}}" ]; then
                              (trap '' TERM; exec sleep 300) >/dev/null 2>&1 &
                              printf '%s\\n' "$!" > "$AURA_SMOKE_STUBBORN_PID"
                            fi
                            printf '%b' {RETRY_STDOUT!r}
                            ;;
                          *)
                            echo "unexpected arguments: $*" >&2
                            exit 91
                            ;;
                        esac
                        ;;
                    esac
                    """
                ),
                encoding="utf-8",
            )
            binary.chmod(0o755)
            wrapper_source = binary.read_text(encoding="utf-8")
            self.assertEqual(wrapper_source.splitlines()[0], "#!/bin/sh")
            self.assertNotIn("pipefail", wrapper_source)
            dash = shutil.which("dash")
            if dash is not None:
                dash_syntax = subprocess.run(
                    [dash, "-n", str(binary)],
                    cwd=REPO_ROOT,
                    text=True,
                    stdout=subprocess.PIPE,
                    stderr=subprocess.PIPE,
                    timeout=10,
                    check=False,
                )
                self.assertEqual(dash_syntax.returncode, 0, dash_syntax.stderr)
            archive = root / "aura-vtest-aarch64-apple-darwin.tar.gz"
            with tarfile.open(archive, "w:gz") as handle:
                handle.add(archive_root, arcname=archive_root.name)

            result = subprocess.run(
                ["bash", str(SMOKE_SCRIPT), str(archive)],
                cwd=REPO_ROOT,
                env={
                    **os.environ,
                    "AURA_SMOKE_TEST_LOG": str(log),
                    "AURA_SMOKE_STUBBORN_PID": str(stubborn_pid_file),
                    "AURA_CACHE_DIR": str(ambient_cache),
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
