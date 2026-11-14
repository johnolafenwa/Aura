#!/usr/bin/env python3
"""Tests for the Homebrew, Debian/apt and Chocolatey packaging."""

from __future__ import annotations

import hashlib
import importlib.util
import os
from pathlib import Path
import shutil
import subprocess
import tarfile
import tempfile
import unittest
import xml.dom.minidom
import zipfile


REPO_ROOT = Path(__file__).resolve().parents[1]
SCRIPTS = REPO_ROOT / "scripts"
CI_WORKFLOW = REPO_ROOT / ".github" / "workflows" / "ci.yml"
DOCS_WORKFLOW = REPO_ROOT / ".github" / "workflows" / "docs.yml"
RELEASE_WORKFLOW = REPO_ROOT / ".github" / "workflows" / "release.yml"
FORMULA_TEMPLATE = REPO_ROOT / "packaging" / "homebrew" / "aura.rb.in"
TAG = "v9.8.7-preview"


def load_formula_renderer():
    spec = importlib.util.spec_from_file_location(
        "render_homebrew_formula", SCRIPTS / "render-homebrew-formula.py"
    )
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


def run(command, **options):
    return subprocess.run(
        [str(part) for part in command],
        text=True,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        timeout=120,
        check=False,
        **options,
    )


def write_archive_tree(root: Path, executable: str, runtime: str) -> None:
    (root / "bin").mkdir(parents=True)
    (root / "lib" / "aura").mkdir(parents=True)
    (root / "examples" / "agents").mkdir(parents=True)
    (root / "bin" / executable).write_text("#!/bin/sh\necho aura\n", encoding="utf-8")
    (root / "bin" / executable).chmod(0o755)
    (root / "lib" / "aura" / runtime).write_text("runtime\n", encoding="utf-8")
    (root / "lib" / "aura" / "native-link-args.json").write_text("[]\n", encoding="utf-8")
    (root / "examples" / "basic_addition.au").write_text("def main():\n    print(16)\n")
    shutil.copy(REPO_ROOT / "LICENSE", root / "LICENSE")
    (root / "README.md").write_text("# Aura\n", encoding="utf-8")
    (root / "AURA_CLI_README.md").write_text("# aura\n", encoding="utf-8")


def write_linux_archive(directory: Path, machine: str = "x86_64") -> Path:
    name = f"aura-{TAG}-{machine}-unknown-linux-gnu"
    write_archive_tree(directory / name, "aura", "libaura_runtime.a")
    archive = directory / f"{name}.tar.gz"
    with tarfile.open(archive, "w:gz") as tar:
        tar.add(directory / name, arcname=name)
    return archive


class HomebrewFormulaTests(unittest.TestCase):
    def setUp(self) -> None:
        self.renderer = load_formula_renderer()
        self.template = FORMULA_TEMPLATE.read_text(encoding="utf-8")
        self.sums = {
            f"aura-{TAG}-{target}.tar.gz": hashlib.sha256(target.encode()).hexdigest()
            for target in self.renderer.TARGETS
        }

    def test_formula_names_every_archive_with_its_checksum(self) -> None:
        formula = self.renderer.render(
            self.template, TAG, f"https://example.com/releases/{TAG}/", self.sums
        )
        self.assertNotIn("@", formula)
        self.assertIn('version "9.8.7-preview"\n', formula)
        for archive, checksum in self.sums.items():
            with self.subTest(archive=archive):
                self.assertIn(
                    f'url "https://example.com/releases/{TAG}/{archive}"\n'
                    f'      sha256 "{checksum}"\n',
                    formula,
                )

    def test_formula_installs_the_runtime_beside_the_command(self) -> None:
        for contract in (
            'bin.install "bin/aura"',
            '(lib/"aura").install Dir["lib/aura/*"]',
            '(lib/"aura/install-manager").write "homebrew\\n"',
            'pkgshare.install "examples"',
            "on_macos do",
            "on_linux do",
            "test do",
        ):
            with self.subTest(contract=contract):
                self.assertIn(contract, self.template)

    def test_checksum_lines_are_read_in_text_and_binary_form(self) -> None:
        sums = self.renderer.checksums("abc  one.tar.gz\ndef *two.tar.gz\n\nbroken\n")
        self.assertEqual(sums, {"one.tar.gz": "abc", "two.tar.gz": "def"})

    def test_renderer_rejects_missing_archives_and_unsafe_values(self) -> None:
        missing = dict(self.sums)
        del missing[f"aura-{TAG}-x86_64-apple-darwin.tar.gz"]
        bad_checksum = dict(self.sums)
        bad_checksum[f"aura-{TAG}-x86_64-apple-darwin.tar.gz"] = 'x"; system("id")'
        for tag, base_url, sums in (
            (TAG, "https://example.com", missing),
            (TAG, "https://example.com", bad_checksum),
            ('v1.2.3"', "https://example.com", self.sums),
            (TAG, 'https://example.com/"#{`id`}', self.sums),
            (TAG, "http://example.com", self.sums),
        ):
            with self.subTest(tag=tag, base_url=base_url):
                with self.assertRaises(ValueError):
                    self.renderer.render(self.template, tag, base_url, sums)

    def test_command_writes_the_formula_and_reports_a_missing_archive(self) -> None:
        with tempfile.TemporaryDirectory(prefix="aura-formula-") as temp:
            sums = Path(temp) / "SHA256SUMS"
            sums.write_text(
                "".join(f"{checksum}  {name}\n" for name, checksum in self.sums.items()),
                encoding="utf-8",
            )
            output = Path(temp) / "aura.rb"
            script = SCRIPTS / "render-homebrew-formula.py"
            result = run(["python3", script, "--tag", TAG, "--sha256sums", sums, "--output", output])
            self.assertEqual(result.returncode, 0, result.stderr)
            formula = output.read_text(encoding="utf-8")
            self.assertIn(
                f"https://github.com/johnolafenwa/Aura/releases/download/{TAG}/", formula
            )
            printed = run(["python3", script, "--tag", TAG, "--sha256sums", sums])
            self.assertEqual(printed.stdout, formula)
            if shutil.which("ruby"):
                self.assertEqual(run(["ruby", "-c", output]).returncode, 0)
            sums.write_text("", encoding="utf-8")
            result = run(["python3", script, "--tag", TAG, "--sha256sums", sums])
            self.assertEqual(result.returncode, 2)
            self.assertIn("is missing from SHA256SUMS", result.stderr)


@unittest.skipUnless(shutil.which("dpkg-deb"), "needs dpkg-deb")
class DebianPackageTests(unittest.TestCase):
    def test_package_installs_the_archive_under_usr(self) -> None:
        with tempfile.TemporaryDirectory(prefix="aura-deb-test-") as temp:
            archive = write_linux_archive(Path(temp), "aarch64")
            result = run(["bash", SCRIPTS / "package-deb.sh", archive], cwd=temp)
            self.assertEqual(result.returncode, 0, result.stderr)
            self.assertEqual(result.stdout, f"aura-{TAG}-arm64.deb\n")
            package = Path(temp) / f"aura-{TAG}-arm64.deb"
            fields = run(["dpkg-deb", "-f", package, "Package", "Version", "Architecture", "Depends"])
            self.assertEqual(
                fields.stdout,
                "Package: aura\n"
                "Version: 9.8.7~preview\n"
                "Architecture: arm64\n"
                "Depends: libc6 (>= 2.39), libstdc++6, zlib1g, libzstd1\n",
            )
            # A preview sorts before the release it previews.
            compare = run(["dpkg", "--compare-versions", "9.8.7~preview", "lt", "9.8.7"])
            self.assertEqual(compare.returncode, 0)
            listing = run(["dpkg-deb", "-c", package]).stdout
            for line in (
                "-rwxr-xr-x root/root",
                "./usr/bin/aura\n",
                "./usr/lib/aura/libaura_runtime.a\n",
                "./usr/lib/aura/native-link-args.json\n",
                "./usr/lib/aura/install-manager\n",
                "./usr/share/aura/examples/basic_addition.au\n",
                "./usr/share/doc/aura/copyright\n",
            ):
                with self.subTest(line=line):
                    self.assertIn(line, listing)
            extracted = Path(temp) / "extracted"
            self.assertEqual(run(["dpkg-deb", "-x", package, extracted]).returncode, 0)
            self.assertEqual(
                (extracted / "usr/lib/aura/install-manager").read_text(encoding="utf-8"), "apt\n"
            )
            copyright_text = (extracted / "usr/share/doc/aura/copyright").read_text(encoding="utf-8")
            self.assertIn("License: Expat\n Copyright (c)", copyright_text)
            self.assertNotIn("\n\n \n", copyright_text)

    def test_package_script_rejects_other_archive_names(self) -> None:
        for name in (
            "../escape.tar.gz",
            f"aura-{TAG}-x86_64-apple-darwin.tar.gz",
            "aura-v1.2.3-pre_view-x86_64-unknown-linux-gnu.tar.gz",
        ):
            with self.subTest(name=name):
                result = run(["bash", SCRIPTS / "package-deb.sh", name], cwd=REPO_ROOT)
                self.assertEqual(result.returncode, 2)
                self.assertIn("invalid Linux release archive name", result.stderr)

    @unittest.skipUnless(
        all(shutil.which(tool) for tool in ("apt-ftparchive", "dpkg-scanpackages", "gpg", "gpgv")),
        "needs apt-ftparchive, dpkg-scanpackages and gpg",
    )
    def test_apt_repository_is_signed_and_lists_every_architecture(self) -> None:
        with tempfile.TemporaryDirectory(prefix="aura-apt-test-") as temp:
            root = Path(temp)
            debs = root / "debs"
            debs.mkdir()
            for machine in ("x86_64", "aarch64"):
                work = root / machine
                work.mkdir()
                archive = write_linux_archive(work, machine)
                built = run(["bash", SCRIPTS / "package-deb.sh", archive], cwd=debs)
                self.assertEqual(built.returncode, 0, built.stderr)
            gnupg = root / "gnupg"
            gnupg.mkdir(mode=0o700)
            environment = dict(os.environ, GNUPGHOME=str(gnupg))
            generated = run(
                ["gpg", "--batch", "--quiet", "--passphrase", "", "--quick-generate-key",
                 "Aura test <test@example.invalid>", "ed25519", "sign", "never"],
                env=environment,
            )
            self.assertEqual(generated.returncode, 0, generated.stderr)
            key = run(["gpg", "--batch", "--armor", "--export-secret-keys"], env=environment).stdout
            run(["gpgconf", "--kill", "all"], env=environment)

            output = root / "site" / "apt"
            output.parent.mkdir()
            script = SCRIPTS / "build-apt-repository.sh"
            without_key = run(
                ["bash", script, debs, output],
                env={name: value for name, value in os.environ.items() if name != "APT_SIGNING_KEY"},
            )
            self.assertEqual(without_key.returncode, 2)
            self.assertFalse(output.exists())
            built = run(["bash", script, debs, output], env=dict(os.environ, APT_SIGNING_KEY=key))
            self.assertEqual(built.returncode, 0, built.stderr)

            release = (output / "dists/stable/Release").read_text(encoding="utf-8")
            self.assertIn("Architectures: amd64 arm64\n", release)
            self.assertIn("Components: main\n", release)
            self.assertIn("Suite: stable\n", release)
            for architecture in ("amd64", "arm64"):
                index = output / f"dists/stable/main/binary-{architecture}/Packages"
                packages = index.read_text(encoding="utf-8")
                self.assertEqual(packages.count("Package: aura\n"), 1)
                self.assertIn(
                    f"Filename: pool/main/a/aura/aura_9.8.7~preview_{architecture}.deb\n", packages
                )
                self.assertTrue((output / f"pool/main/a/aura/aura_9.8.7~preview_{architecture}.deb").is_file())
                self.assertTrue(index.with_name("Packages.gz").is_file())
                digest = hashlib.sha256(index.read_bytes()).hexdigest()
                self.assertIn(f" {digest} ", release)
            keyring = output / "aura-archive-keyring.gpg"
            for signed in (
                [output / "dists/stable/InRelease"],
                [output / "dists/stable/Release.gpg", output / "dists/stable/Release"],
            ):
                verified = run(["gpgv", "--keyring", keyring, *signed])
                self.assertEqual(verified.returncode, 0, verified.stderr)
            # The private key never reaches the published directory.
            for path in output.rglob("*"):
                if path.is_file() and path.suffix != ".deb":
                    self.assertNotIn(b"PRIVATE KEY", path.read_bytes())

    def test_site_deploys_without_a_signing_key(self) -> None:
        with tempfile.TemporaryDirectory(prefix="aura-apt-skip-") as temp:
            output = Path(temp) / "apt"
            result = run(
                ["bash", SCRIPTS / "publish-apt-repository.sh", output],
                env={name: value for name, value in os.environ.items() if name != "APT_SIGNING_KEY"},
            )
            self.assertEqual(result.returncode, 0, result.stderr)
            self.assertIn("::notice::APT_SIGNING_KEY is not configured", result.stdout)
            self.assertFalse(output.exists())


class ChocolateyPackageTests(unittest.TestCase):
    def stage(self, temp: str, name: str):
        root = Path(temp)
        write_archive_tree(root / name, "aura.exe", "aura_runtime.lib")
        archive = root / f"{name}.zip"
        with zipfile.ZipFile(archive, "w") as bundle:
            for path in sorted((root / name).rglob("*")):
                bundle.write(path, path.relative_to(root).as_posix())
        result = run(
            ["bash", SCRIPTS / "package-chocolatey.sh", archive],
            cwd=temp,
            env=dict(os.environ, AURA_CHOCOLATEY_STAGE_ONLY="1"),
        )
        return archive, result

    def test_package_carries_the_archive_and_how_to_verify_it(self) -> None:
        with tempfile.TemporaryDirectory(prefix="aura-choco-test-") as temp:
            name = f"aura-{TAG}-x86_64-pc-windows-msvc"
            archive, result = self.stage(temp, name)
            self.assertEqual(result.returncode, 0, result.stderr)
            stage = Path(temp) / "release" / "chocolatey"
            nuspec = xml.dom.minidom.parse(str(stage / "aura.nuspec"))
            metadata = {
                node.tagName: node.firstChild.data
                for node in nuspec.getElementsByTagName("metadata")[0].childNodes
                if node.nodeType == node.ELEMENT_NODE and node.firstChild is not None
            }
            self.assertEqual(metadata["id"], "aura")
            self.assertEqual(metadata["version"], "9.8.7-preview")
            self.assertEqual(
                metadata["releaseNotes"],
                f"https://github.com/johnolafenwa/Aura/releases/tag/{TAG}",
            )
            self.assertNotIn("@", (stage / "aura.nuspec").read_text(encoding="utf-8"))
            tools = stage / "tools"
            for file in (
                "aura/bin/aura.exe",
                "aura/lib/aura/aura_runtime.lib",
                "aura/lib/aura/native-link-args.json",
                "aura/examples/basic_addition.au",
                "LICENSE.txt",
                "VERIFICATION.txt",
            ):
                with self.subTest(file=file):
                    self.assertTrue((tools / file).is_file())
            self.assertEqual(
                (tools / "aura/lib/aura/install-manager").read_text(encoding="utf-8"),
                "chocolatey\n",
            )
            # Chocolatey makes a command of every .exe in the package.
            self.assertEqual([path.name for path in stage.rglob("*.exe")], ["aura.exe"])
            verification = (tools / "VERIFICATION.txt").read_text(encoding="utf-8")
            self.assertNotIn("@", verification)
            self.assertIn(hashlib.sha256(archive.read_bytes()).hexdigest(), verification)
            self.assertIn(
                hashlib.sha256((tools / "aura/bin/aura.exe").read_bytes()).hexdigest(), verification
            )
            self.assertIn(
                f"https://github.com/johnolafenwa/Aura/releases/download/{TAG}/{name}.zip",
                verification,
            )

    def test_package_script_rejects_other_archive_names(self) -> None:
        for name in (
            "../escape.zip",
            f"aura-{TAG}-x86_64-unknown-linux-gnu.zip",
            "aura-v1.2.3-rc.1-x86_64-pc-windows-msvc.zip",
        ):
            with self.subTest(name=name):
                result = run(["bash", SCRIPTS / "package-chocolatey.sh", name], cwd=REPO_ROOT)
                self.assertEqual(result.returncode, 2)
                self.assertIn("invalid Windows release archive name", result.stderr)


class PackageWorkflowTests(unittest.TestCase):
    def setUp(self) -> None:
        self.release = RELEASE_WORKFLOW.read_text(encoding="utf-8")

    def job(self, name: str) -> str:
        start = self.release.index(f"\n  {name}:\n")
        following = self.release.find("\n  publish-", start + 1)
        return self.release[start:following if following != -1 else len(self.release)]

    def test_release_builds_installs_and_uploads_the_packages(self) -> None:
        cli = self.release[self.release.index("  cli:\n"):self.release.index("  tools:\n")]
        for contract in (
            'package="$(scripts/package-deb.sh "$ARCHIVE_NAME.tar.gz")"\n'
            '          scripts/smoke-deb.sh "$package"\n',
            'package="$(scripts/package-chocolatey.sh "$ARCHIVE_NAME.zip")"\n'
            '          scripts/smoke-chocolatey.sh "$package"\n',
            "name: ${{ matrix.archive }}-package\n",
            "path: ${{ runner.os == 'Windows' && 'aura.*.nupkg' || 'aura-*.deb' }}\n",
        ):
            with self.subTest(contract=contract):
                self.assertIn(contract, cli)

    def test_package_publishing_follows_the_github_release(self) -> None:
        for name, needs in (
            ("publish-homebrew", "needs: [release_identity, publish]"),
            ("publish-chocolatey", "needs: publish"),
            ("publish-apt", "needs: publish"),
        ):
            with self.subTest(job=name):
                job = self.job(name)
                self.assertIn(f"    {needs}\n", job)
                # No `if:` on the job, so a skipped or failed release skips it.
                self.assertNotIn("\n    if:", job)

    def test_outside_publishing_needs_a_secret_and_skips_without_it(self) -> None:
        for name, secret in (
            ("publish-homebrew", "HOMEBREW_TAP_TOKEN"),
            ("publish-chocolatey", "CHOCOLATEY_API_KEY"),
        ):
            with self.subTest(job=name):
                job = self.job(name)
                self.assertIn(f"HAS_{secret}: ${{{{ secrets.{secret} != '' }}}}\n", job)
                self.assertIn(f"if: env.HAS_{secret} == 'true'\n", job)
                self.assertIn(f"if: env.HAS_{secret} != 'true'\n", job)
                self.assertIn(f"::notice::{secret} is not configured", job)
                # Only the publishing step receives the secret itself.
                self.assertEqual(job.count(f"${{{{ secrets.{secret} }}}}"), 1)
        self.assertIn("contents: read\n", self.job("publish-homebrew"))
        self.assertIn("persist-credentials: false\n", self.job("publish-homebrew"))
        self.assertIn("HOMEBREW_TAP: johnolafenwa/homebrew-aura\n", self.job("publish-homebrew"))
        self.assertIn("--source https://push.chocolatey.org/", self.job("publish-chocolatey"))

    def test_release_starts_the_site_deployment_that_carries_apt(self) -> None:
        job = self.job("publish-apt")
        self.assertIn("    permissions:\n      actions: write\n", job)
        self.assertIn('gh workflow run docs.yml --repo "$GITHUB_REPOSITORY" --ref main\n', job)
        docs = DOCS_WORKFLOW.read_text(encoding="utf-8")
        build = docs[docs.index("\n  build:\n"):docs.index("\n  examples:\n")]
        step = build[build.index("      - name: Add the apt repository\n"):]
        step = step[:step.index("\n\n")]
        self.assertIn("if: github.event_name != 'pull_request'\n", step)
        self.assertIn("APT_SIGNING_KEY: ${{ secrets.APT_SIGNING_KEY }}\n", step)
        self.assertIn("run: scripts/publish-apt-repository.sh docs/.vitepress/dist/apt", step)
        self.assertEqual(docs.count("secrets.APT_SIGNING_KEY"), 1)
        self.assertLess(
            build.index("- name: Add the apt repository"), build.index("- name: Upload Pages artifact")
        )

    def test_ci_installs_every_package_before_a_release_depends_on_it(self) -> None:
        ci = CI_WORKFLOW.read_text(encoding="utf-8")
        windows = ci[ci.index("  windows:\n"):ci.index("  windows-integration:\n")]
        self.assertIn(
            'scripts/smoke-chocolatey.sh "$(scripts/package-chocolatey.sh "$archive.zip")"', windows
        )
        native = ci[ci.index("  native-binaries:\n"):]
        self.assertIn('run: scripts/smoke-deb.sh "$(scripts/package-deb.sh "$AURA_CI_ARCHIVE")"\n', native)
        self.assertIn('run: scripts/smoke-homebrew.sh "$AURA_CI_ARCHIVE"\n', native)

    def test_installed_commands_are_checked_for_the_upgrade_hint(self) -> None:
        for script, hint in (
            ("smoke-deb.sh", "apt-get install --only-upgrade aura"),
            ("smoke-chocolatey.sh", "choco upgrade aura --pre"),
            ("smoke-homebrew.sh", "brew upgrade aura"),
        ):
            with self.subTest(script=script):
                self.assertIn(hint, (SCRIPTS / script).read_text(encoding="utf-8"))
                self.assertIn(hint, (REPO_ROOT / "crates/aura/src/main.rs").read_text(encoding="utf-8"))


if __name__ == "__main__":
    unittest.main()
