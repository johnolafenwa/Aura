#!/usr/bin/env python3
"""Regression tests for release workflow and packaged-CLI safety contracts."""

from __future__ import annotations

import hashlib
import json
import os
from pathlib import Path
import platform
import re
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
AURA_COMPILER_BUILD = REPO_ROOT / "crates" / "aura-compiler" / "build.rs"
INSTALL_SCRIPT = REPO_ROOT / "docs" / "public" / "install.sh"
LANDING_INSTALL_COMPONENT = (
    REPO_ROOT / "docs" / ".vitepress" / "theme" / "HomeInstall.vue"
)
SUPPORTED_PLATFORMS = REPO_ROOT / "SUPPORTED_PLATFORMS.md"


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
    def test_full_ci_skips_documentation_and_passive_repository_metadata(self) -> None:
        workflow = CI_WORKFLOW.read_text(encoding="utf-8")
        trigger = workflow.split("\nenv:\n", 1)[0]

        self.assertIn("  push:\n    branches: [main]\n", trigger)
        self.assertEqual(trigger.count("    paths-ignore:\n"), 2)
        for path in (
            "'**/*.md'",
            "'docs/**'",
            "'work/**'",
            "'LICENSE'",
            "'.gitignore'",
            "'.gitattributes'",
            "'.editorconfig'",
            "'.github/ISSUE_TEMPLATE/**'",
            "'.github/PULL_REQUEST_TEMPLATE*'",
            "'.github/CODEOWNERS'",
        ):
            with self.subTest(path=path):
                self.assertEqual(trigger.count(f"      - {path}\n"), 2)

    def test_every_workflow_declares_a_read_only_default_token(self) -> None:
        for workflow_path in sorted((REPO_ROOT / ".github" / "workflows").glob("*.yml")):
            workflow = workflow_path.read_text(encoding="utf-8")
            with self.subTest(workflow=workflow_path.name):
                self.assertRegex(workflow, r"(?m)^permissions:\n  contents: read\n")

    def test_extension_packaging_pins_the_vsce_version(self) -> None:
        manifest = json.loads(
            (REPO_ROOT / "tools" / "vscode-aura" / "package.json").read_text(encoding="utf-8")
        )
        self.assertIn("npx --yes @vscode/vsce@4.0.0 package", manifest["scripts"]["package:vsix"])

    def test_cargo_color_is_disabled_at_workflow_scope(self) -> None:
        for workflow_path in (CI_WORKFLOW, RELEASE_WORKFLOW):
            with self.subTest(workflow=workflow_path.name):
                workflow = workflow_path.read_text(encoding="utf-8")
                workflow_scope = workflow.split("\njobs:\n", 1)[0]
                self.assertIn("\nenv:\n  CARGO_TERM_COLOR: never\n", workflow_scope)

    def test_hosted_timing_policy_keeps_the_rust_suite_parallel(self) -> None:
        workflow = CI_WORKFLOW.read_text(encoding="utf-8")
        self.assertNotIn("RUST_TEST_THREADS", workflow)
        compiler = (REPO_ROOT / "crates" / "aura-compiler" / "src" / "lib.rs").read_text(
            encoding="utf-8"
        )
        self.assertIn('var_os("GITHUB_ACTIONS")', compiler)

    def test_ci_timeout_covers_the_complete_standard_runner_gate(self) -> None:
        workflow = CI_WORKFLOW.read_text(encoding="utf-8")
        verify_header = workflow.split("jobs:\n  verify:\n", 1)[1].split(
            "\n    steps:\n", 1
        )[0]
        self.assertIn("\n    timeout-minutes: 210\n", verify_header)

    def test_ci_checkout_includes_head_parent_for_commit_hygiene(self) -> None:
        workflow = CI_WORKFLOW.read_text(encoding="utf-8")
        checkout_step = workflow.split("      - name: Checkout\n", 1)[1].split(
            "\n\n      - name:", 1
        )[0]
        self.assertIn("        with:\n          fetch-depth: 2", checkout_step)

    def test_ci_installs_reference_search_tool_on_every_hosted_os(self) -> None:
        workflow = CI_WORKFLOW.read_text(encoding="utf-8")
        self.assertIn(
            "      - name: Install ripgrep\n"
            "        uses: taiki-e/install-action@c7eb1735f09259a5035e8e5d44b1406b1cddc0fb # v2\n"
            "        with:\n"
            "          tool: ripgrep@14.1.1\n",
            workflow,
        )

    def test_ci_installs_pinned_llvm_and_runs_the_llvm_gates(self) -> None:
        workflow = CI_WORKFLOW.read_text(encoding="utf-8")
        self.assertIn(
            "      - name: Install LLVM\n"
            "        run: bash scripts/install-llvm.sh\n",
            workflow,
        )
        self.assertIn("path: ~/.cache/aura/llvm/22.1.2", workflow)
        installer = (REPO_ROOT / "scripts" / "install-llvm.sh").read_text(
            encoding="utf-8"
        )
        pins = re.findall(r"^    sha256=([0-9a-f]{64})$", installer, re.MULTILINE)
        self.assertEqual(len(pins), 1, "the Linux archive is pinned")
        self.assertIn("brew install llvm@22", installer)
        self.assertLess(
            installer.index('if [ "$actual" != "$sha256" ]'),
            installer.index("tar -xJf"),
            "the checksum is checked before anything is extracted",
        )
        scripts = json.loads((REPO_ROOT / "package.json").read_text(encoding="utf-8"))[
            "scripts"
        ]
        for gate in ("test:llvm", "test:backend-parity:llvm", "check:clippy:llvm"):
            self.assertIn(f"npm run {gate} ", scripts["ci"] + " ")
        self.assertIn("AURA_NATIVE_CODEGEN=llvm", scripts["test:backend-parity:llvm"])
        # The repeat-build test also runs on LLVM, so its objects stay repeatable.
        self.assertIn(
            "AURA_NATIVE_CODEGEN=llvm RUST_MIN_STACK=33554432 cargo test -p aura-compiler"
            " --lib --features llvm direct_objects_are_identical_across_repeated_builds",
            scripts["test:llvm"],
        )

    def test_native_binary_gate_checks_both_code_generators(self) -> None:
        workflow = CI_WORKFLOW.read_text(encoding="utf-8")
        job = workflow[workflow.index("  native-binaries:\n"):]
        self.assertIn("run: bash scripts/install-llvm.sh\n", job)
        self.assertIn(
            "cargo build --release --locked -p aura -p aura-runtime --features aura/llvm\n",
            job,
        )
        self.assertIn(
            "      - name: Check native executable contents and sizes on LLVM\n"
            "        run: python3 scripts/check-native-binaries.py\n"
            "        env:\n"
            "          AURA_NATIVE_CODEGEN: llvm\n",
            job,
        )

    def test_release_cli_links_llvm_and_checks_the_libraries_it_loads(self) -> None:
        workflow = RELEASE_WORKFLOW.read_text(encoding="utf-8")
        job = workflow[workflow.index("  cli:\n"):workflow.index("  tools:\n")]
        for runner in ("os: ubuntu-24.04", "os: macos-15-intel", "os: macos-15"):
            self.assertIn(runner, job)
        self.assertIn("        run: bash scripts/install-llvm.sh\n", job)
        self.assertNotIn("actions/cache", job, "a release links no cached LLVM")
        self.assertLess(
            job.index("run: bash scripts/install-llvm.sh"),
            job.index("run: cargo build -p aura --release --locked"),
        )
        self.assertLess(
            job.index("run: python3 scripts/check-aura-libraries.py target/release/aura"),
            job.index("run: scripts/package-cli.sh"),
            "the libraries are checked before the archive is packaged",
        )
        ci = CI_WORKFLOW.read_text(encoding="utf-8")
        native = ci[ci.index("  native-binaries:\n"):]
        self.assertIn("run: python3 scripts/check-aura-libraries.py target/release/aura\n", native)

    def test_macos_llvm_install_links_static_zstd_and_needs_a_bottle(self) -> None:
        installer = (REPO_ROOT / "scripts" / "install-llvm.sh").read_text(encoding="utf-8")
        self.assertIn("  Darwin-arm64 | Darwin-x86_64)\n", installer)
        darwin = installer[installer.index("  Darwin-arm64 | Darwin-x86_64)"):]
        self.assertLess(darwin.index("check_bottle"), darwin.index("brew install llvm@22"))
        self.assertIn('ln -sf "$zstd_static" "$static_dir/libzstd.a"', darwin)
        self.assertIn(
            'echo "LLVM_SYS_SYSTEM_LIBRARY_DIRS=${static_dir}" >> "$GITHUB_ENV"', darwin
        )

    def test_linux_coverage_tests_export_process_ffi_symbols(self) -> None:
        self.assertTrue(AURA_COMPILER_BUILD.is_file())
        build_script = AURA_COMPILER_BUILD.read_text(encoding="utf-8")
        self.assertIn('target_os == "linux"', build_script)
        self.assertIn(
            'cargo::rustc-link-arg-tests=-Wl,--export-dynamic', build_script
        )

    def test_docs_use_node24_deploy_pages_release(self) -> None:
        workflow = DOCS_WORKFLOW.read_text(encoding="utf-8")
        self.assertIn(
            "actions/deploy-pages@cd2ce8fcbc39b97be8ca5fce6e763baed58fa128 # v5.0.0",
            workflow,
        )

    def test_docs_deploy_waits_for_the_example_check(self) -> None:
        workflow = DOCS_WORKFLOW.read_text(encoding="utf-8")
        deploy = workflow.split("\n  deploy:\n", 1)[1]
        needs = re.search(r"^    needs: (.+)$", deploy, re.MULTILINE)
        self.assertIsNotNone(needs)
        self.assertEqual(needs.group(1), "[build, examples]")


class LandingAndInstallerTests(unittest.TestCase):
    def test_installer_is_posix_and_verifies_release_checksum(self) -> None:
        installer = INSTALL_SCRIPT.read_text(encoding="utf-8")
        self.assertEqual(installer.splitlines()[0], "#!/bin/sh")
        self.assertIn("v0.6.0-preview", installer)
        self.assertIn("SHA256SUMS", installer)
        self.assertIn("sha256sum", installer)
        self.assertIn("shasum -a 256", installer)
        self.assertIn("AURA_INSTALL_PREFIX", installer)
        self.assertIn("AURA_INSTALL_BASE_URL", installer)
        self.assertNotIn("sudo", installer)
        dash = shutil.which("dash")
        if dash is not None:
            result = subprocess.run(
                [dash, "-n", str(INSTALL_SCRIPT)],
                text=True,
                stdout=subprocess.PIPE,
                stderr=subprocess.PIPE,
                timeout=10,
                check=False,
            )
            self.assertEqual(result.returncode, 0, result.stderr)

    def test_installer_does_nothing_when_a_piped_download_is_truncated(self) -> None:
        installer = INSTALL_SCRIPT.read_text(encoding="utf-8")
        lines = installer.rstrip("\n").splitlines()
        # `curl ... | sh` runs whatever arrived. Defining everything in a
        # function and calling it on the last line means a cut-off download
        # defines code but never runs it.
        self.assertEqual(lines[-1], 'main "$@"')
        with tempfile.TemporaryDirectory(prefix="aura-install-truncated-") as temp:
            root = Path(temp)
            prefix = root / "prefix"
            for cut in range(1, len(lines)):
                truncated = "\n".join(lines[:cut]) + "\n"
                result = subprocess.run(
                    ["sh"],
                    input=truncated,
                    cwd=root,
                    env={
                        **os.environ,
                        "AURA_INSTALL_BASE_URL": (root / "missing").as_uri(),
                        "AURA_INSTALL_PREFIX": str(prefix),
                    },
                    text=True,
                    stdout=subprocess.PIPE,
                    stderr=subprocess.PIPE,
                    timeout=10,
                    check=False,
                )
                self.assertEqual(result.stdout, "", f"line {cut} ran: {result.stdout}")
                self.assertFalse(prefix.exists(), f"truncation after line {cut} installed files")

    def test_installer_places_binary_and_native_runtime_under_prefix(self) -> None:
        system = platform.system()
        machine = platform.machine().lower()
        if system == "Darwin" and machine in {"arm64", "aarch64"}:
            target = "aarch64-apple-darwin"
        elif system == "Darwin" and machine == "x86_64":
            target = "x86_64-apple-darwin"
        elif system == "Linux" and machine in {"x86_64", "amd64"}:
            target = "x86_64-unknown-linux-gnu"
        else:
            self.skipTest(f"installer has no release archive for {system} {machine}")

        with tempfile.TemporaryDirectory(prefix="aura-install-test-") as temp:
            root = Path(temp)
            release = root / "release"
            release.mkdir()
            archive_name = f"aura-v0.6.0-preview-{target}"
            archive_root = root / archive_name
            (archive_root / "bin").mkdir(parents=True)
            (archive_root / "lib" / "aura").mkdir(parents=True)
            (archive_root / "examples").mkdir(parents=True)
            binary = archive_root / "bin" / "aura"
            binary.write_text("#!/bin/sh\necho aura-installer-test\n", encoding="utf-8")
            binary.chmod(0o755)
            (archive_root / "lib" / "aura" / "libaura_runtime.a").write_bytes(
                b"runtime"
            )
            (archive_root / "lib" / "aura" / "native-link-args.json").write_text(
                "[]\n", encoding="utf-8"
            )
            (archive_root / "examples" / "hello.au").write_text(
                'print("hello")\n', encoding="utf-8"
            )
            (archive_root / "README.md").write_text("Aura\n", encoding="utf-8")
            (archive_root / "LICENSE").write_text("MIT\n", encoding="utf-8")

            archive = release / f"{archive_name}.tar.gz"
            with tarfile.open(archive, "w:gz") as handle:
                handle.add(archive_root, arcname=archive_name)
            digest = hashlib.sha256(archive.read_bytes()).hexdigest()
            (release / "SHA256SUMS").write_text(
                f"{digest}  {archive.name}\n", encoding="utf-8"
            )

            prefix = root / "prefix"
            # An upgrade must not leave an earlier release's runtime archive.
            stale_runtime = prefix / "lib" / "aura" / "libaura_compiler.a"
            stale_runtime.parent.mkdir(parents=True)
            stale_runtime.write_bytes(b"stale")
            result = subprocess.run(
                ["sh", str(INSTALL_SCRIPT)],
                cwd=root,
                env={
                    **os.environ,
                    "AURA_INSTALL_BASE_URL": release.as_uri(),
                    "AURA_INSTALL_PREFIX": str(prefix),
                },
                text=True,
                stdout=subprocess.PIPE,
                stderr=subprocess.PIPE,
                timeout=30,
                check=False,
            )
            self.assertEqual(result.returncode, 0, result.stderr)
            installed = prefix / "bin" / "aura"
            self.assertTrue(installed.is_file())
            self.assertTrue(os.access(installed, os.X_OK))
            self.assertEqual(
                subprocess.run(
                    [str(installed)],
                    text=True,
                    stdout=subprocess.PIPE,
                    check=True,
                ).stdout,
                "aura-installer-test\n",
            )
            self.assertEqual(
                (prefix / "lib" / "aura" / "libaura_runtime.a").read_bytes(),
                b"runtime",
            )
            self.assertTrue((prefix / "share" / "aura" / "examples" / "hello.au").is_file())
            self.assertFalse(stale_runtime.exists())


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
        publish_job = self.workflow[self.workflow.index("  publish-extension:\n"):]
        job_env = publish_job[publish_job.index("    env:\n"):publish_job.index("    steps:\n")]
        # Publishing tokens reach only the step that uses them, never the
        # whole job, so download, validation, and setup steps cannot read them.
        self.assertNotIn("secrets.VSCE_PAT }}", job_env)
        self.assertNotIn("secrets.OVSX_TOKEN }}", job_env)
        self.assertIn("HAS_VSCE_PAT: ${{ secrets.VSCE_PAT != '' }}", job_env)
        self.assertIn("HAS_OVSX_TOKEN: ${{ secrets.OVSX_TOKEN != '' }}", job_env)
        self.assertEqual(publish_job.count("VSCE_PAT: ${{ secrets.VSCE_PAT }}"), 1)
        self.assertEqual(publish_job.count("OVSX_TOKEN: ${{ secrets.OVSX_TOKEN }}"), 1)
        self.assertIn("if: env.HAS_VSCE_PAT == 'true'", self.workflow)
        self.assertIn("if: env.HAS_VSCE_PAT != 'true'", self.workflow)
        self.assertIn("if: env.HAS_OVSX_TOKEN == 'true'", self.workflow)
        self.assertIn("if: env.HAS_OVSX_TOKEN != 'true'", self.workflow)
        # Publishing tools run with marketplace tokens, so they are pinned to
        # exact versions instead of whatever npm serves as latest.
        self.assertIn(
            "npx --yes @vscode/vsce@4.0.0 publish --packagePath \"$VSIX_PATH\" -p \"$VSCE_PAT\"",
            self.workflow,
        )
        self.assertIn(
            "npx --yes ovsx@1.2.0 publish \"$VSIX_PATH\" -p \"$OVSX_TOKEN\"",
            self.workflow,
        )
        self.assertNotRegex(self.workflow, r"npx (--yes )?(@vscode/vsce|ovsx)(?!@\d)")
        self.assertIn("::notice::VSCE_PAT is not configured", self.workflow)
        self.assertIn("::notice::OVSX_TOKEN is not configured", self.workflow)
        self.assertIn("inputs.publish_extension", self.workflow)
        self.assertIn(
            'gh release download "$RELEASE_TAG" --repo "$GITHUB_REPOSITORY"',
            self.workflow,
        )

    def test_extension_publish_job_validates_plain_marketplace_identity(self) -> None:
        self.assertIn("name: Validate VSIX Marketplace identity", self.workflow)
        self.assertIn("extension_version: ${{ steps.source.outputs.extension_version }}", self.workflow)
        self.assertIn("EXPECTED_EXTENSION_VERSION", self.workflow)
        self.assertIn('expected_version="$EXPECTED_EXTENSION_VERSION"', self.workflow)
        self.assertIn("JohnOlafenwa.vscode-aura-lang", self.workflow)
        self.assertIn("v0.6.0-preview", self.workflow)
        self.assertNotIn("v0.2.0-preview", self.workflow)

    def test_extension_only_dispatch_builds_from_an_explicit_source_ref(self) -> None:
        tools_header = self.workflow.split("\n  tools:\n", 1)[1].split(
            "\n    steps:\n", 1
        )[0]
        self.assertIn("inputs.source_ref != ''", tools_header)

        publish_extension_job = self.workflow.split(
            "\n  publish-extension:\n", 1
        )[1]
        confirm_existing_release = publish_extension_job.split(
            "      - name: Confirm existing GitHub Release\n", 1
        )[1].split("\n      - name: Download VS Code extension", 1)[0]
        self.assertIn("needs.tools.result == 'skipped'", confirm_existing_release)
        self.assertIn("needs.tools.result == 'success'", publish_extension_job)
        self.assertIn("needs.publish.result == 'skipped'", publish_extension_job)
        self.assertRegex(
            publish_extension_job,
            r"github\.event_name == 'workflow_dispatch'\s*&&\s*"
            r"inputs\.publish_extension\s*&&\s*"
            r"needs\.publish\.result == 'skipped'",
        )


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

class ArchiveLayoutTests(unittest.TestCase):
    def test_package_layout_is_stable_and_self_contained(self) -> None:
        script = PACKAGE_SCRIPT.read_text(encoding="utf-8")
        required_contracts = (
            'mkdir -p "$archive_root/bin" "$archive_root/lib/aura" '
            '"$archive_root/examples/agents"',
            'cp target/release/aura "$archive_root/bin/aura"',
            'cp target/release/libaura_runtime.a '
            '"$archive_root/lib/aura/libaura_runtime.a"',
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
            archive_root = root / "aura-v0.6.0-preview-aarch64-apple-darwin"
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
                        echo "aura 0.6.0-preview ({commit})"
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
            archive = root / "aura-v0.6.0-preview-aarch64-apple-darwin.tar.gz"
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
            self.assertIn(f"aura 0.6.0-preview ({commit})\n", result.stdout)
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
