#!/usr/bin/env python3
"""Release-metadata regression tests for the Aura 0.3 technical preview."""

from __future__ import annotations

import json
import pathlib
import re
import unittest


ROOT = pathlib.Path(__file__).resolve().parents[1]
VERSION = "0.3.3"
EXTENSION_VERSION = "0.3.4"


class ReleaseMetadataTests(unittest.TestCase):
    def test_product_manifests_and_locks_record_current_versions(self) -> None:
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
                and name_match.group(1) in {"aura", "aura-compiler"}
            ):
                workspace_packages[name_match.group(1)] = version_match.group(1)
        self.assertEqual(
            workspace_packages,
            {"aura": VERSION, "aura-compiler": VERSION},
        )

        fuzz_lock = (ROOT / "fuzz/Cargo.lock").read_text()
        fuzz_compiler = re.search(
            r'\[\[package\]\]\nname = "aura-compiler"\nversion = "([^"]+)"',
            fuzz_lock,
        )
        self.assertIsNotNone(fuzz_compiler)
        self.assertEqual(fuzz_compiler.group(1), VERSION)

        manifests = {
            "root": ROOT / "package.json",
            "lsp": ROOT / "tools/aura-language-server/package.json",
        }
        for label, path in manifests.items():
            with self.subTest(label=label):
                self.assertEqual(json.loads(path.read_text())["version"], VERSION)
        self.assertEqual(
            json.loads((ROOT / "tools/vscode-aura/package.json").read_text())["version"],
            EXTENSION_VERSION,
        )

        root_lock = json.loads((ROOT / "package-lock.json").read_text())
        self.assertEqual(root_lock["version"], VERSION)
        self.assertEqual(root_lock["packages"][""]["version"], VERSION)
        self.assertEqual(
            root_lock["packages"]["tools/aura-language-server"]["version"],
            VERSION,
        )
        self.assertEqual(
            root_lock["packages"]["tools/vscode-aura"]["version"],
            EXTENSION_VERSION,
        )

        # This npm workspace intentionally uses one root lock. The LSP and
        # extension entries above are their lock records; package-local lock
        # files would split dependency resolution and are not maintained.

    def test_scheduled_safety_jobs_pin_the_instrumented_toolchain_and_leaks(self) -> None:
        workflow = (ROOT / ".github/workflows/safety.yml").read_text()
        tsan = (ROOT / "scripts/sanitizer-scheduler-tsan.sh").read_text()
        asan = (ROOT / "scripts/sanitizer-native-runtime.sh").read_text()
        native_runtime = (
            ROOT / "crates/aura-compiler/src/native_runtime.rs"
        ).read_text()
        native_runtime_exports = (ROOT / "crates/aura-compiler/src/lib.rs").read_text()
        ffi_tests = (
            ROOT / "crates/aura-compiler/tests/native_runtime_ffi.rs"
        ).read_text()

        self.assertIn("RUSTUP_TOOLCHAIN: nightly-2026-07-01", workflow)
        self.assertEqual(
            workflow.count("cargo +nightly-2026-07-01 fuzz run"),
            2,
        )
        self.assertEqual(
            workflow.count("--target x86_64-unknown-linux-gnu"),
            2,
        )
        self.assertIn("CARGO_TARGET_", tsan)
        self.assertNotIn("export RUSTFLAGS=", tsan)
        self.assertNotIn("--test cli", tsan)
        self.assertIn("CARGO_TARGET_", asan)
        self.assertNotIn("export RUSTFLAGS=", asan)
        self.assertIn('ASAN_OPTIONS="$asan_options" cargo', asan)
        self.assertNotIn('"$aura_bin" build', asan)
        self.assertIn("DIRECT_VALUE_LIVE_COUNT", native_runtime)
        self.assertIn("DIRECT_VALUE_LIVE_COUNT", native_runtime_exports)
        self.assertNotIn("aura_direct_coverage_live_value_count", native_runtime_exports)
        self.assertEqual(ffi_tests.count("direct_runtime_ffi_test_guard()"), 8)

    def test_scheduler_stress_filters_resolve_and_fit_the_hosted_budget(self) -> None:
        workflow = (ROOT / ".github/workflows/safety.yml").read_text()
        stress = (ROOT / "scripts/stress-scheduler.sh").read_text()
        cli_tests = (ROOT / "crates/aura/tests/cli.rs").read_text()

        configured = re.findall(r'^  "([a-z0-9_]+)"$', stress, re.MULTILINE)
        self.assertEqual(
            configured,
            [
                "queue_consumers_share_work_fairly_on_one_worker",
                "cancelled_sleeping_children_resume_and_can_observe_cancellation",
                "scheduler_mixed_wakeups_complete_in_mir_and_direct_backends",
            ],
        )
        for test_name in configured:
            with self.subTest(test_name=test_name):
                self.assertIn(f"fn {test_name}()", cli_tests)

        self.assertIn("timeout-minutes: 45", workflow)
        self.assertIn("AURA_STRESS_RUNS=10 scripts/stress-scheduler.sh", workflow)
        self.assertNotIn("AURA_STRESS_RUNS=50 scripts/stress-scheduler.sh", workflow)


if __name__ == "__main__":
    unittest.main()
