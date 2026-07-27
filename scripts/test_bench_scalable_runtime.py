#!/usr/bin/env python3
"""Focused tests for the scalable-runtime benchmark host runner."""

from __future__ import annotations

import importlib.util
import io
import os
import stat
import struct
import tempfile
import textwrap
import time
import unittest
from pathlib import Path
from unittest import mock


SCRIPT = Path(__file__).with_name("bench-scalable-runtime.py")
SPEC = importlib.util.spec_from_file_location("bench_scalable_runtime", SCRIPT)
assert SPEC is not None and SPEC.loader is not None
bench = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(bench)


class ProtocolTests(unittest.TestCase):
    def test_phase_lines_are_exact_and_phase_specific(self) -> None:
        self.assertEqual(
            bench.parse_phase_line(
                b"BASELINE sleepers 10000\n",
                phase="BASELINE",
                benchmark="sleepers",
                expected_fields=("10000",),
            ),
            ("10000",),
        )
        self.assertEqual(
            bench.parse_phase_line(
                b"READY sleepers 10000\n",
                phase="READY",
                benchmark="sleepers",
                expected_fields=("10000",),
            ),
            ("10000",),
        )
        with self.assertRaisesRegex(bench.BenchmarkError, "unexpected BASELINE"):
            bench.parse_phase_line(
                b"READY sleepers 10000\n",
                phase="BASELINE",
                benchmark="sleepers",
                expected_fields=("10000",),
            )

    def test_ready_line_is_exact_and_bounded(self) -> None:
        self.assertEqual(
            bench.parse_ready_line(
                b"READY sleepers 10000\n",
                benchmark="sleepers",
                expected_fields=("10000",),
            ),
            ("10000",),
        )
        with self.assertRaisesRegex(bench.BenchmarkError, "unexpected READY"):
            bench.parse_ready_line(
                b"note\nREADY sleepers 10000\n",
                benchmark="sleepers",
                expected_fields=("10000",),
            )
        with self.assertRaisesRegex(bench.BenchmarkError, "exceeded"):
            bench.read_bounded_line(io.BytesIO(b"x" * 33 + b"\n"), 32)

    def test_timer_protocol_requires_all_unique_samples_and_done(self) -> None:
        ready = bench.parse_timer_ready_line(
            b"READY timers 3 10 100.0 101.2\n",
            expected_count=3,
            expected_duration_ms=10,
        )
        self.assertEqual(ready["count"], 3)
        self.assertEqual(ready["duration_ms"], 10)
        self.assertEqual(ready["min_start_ms"], 100.0)
        self.assertEqual(ready["max_start_ms"], 101.2)
        self.assertAlmostEqual(ready["arm_span_ms"], 1.2)
        output = io.BytesIO(
            b"SAMPLE timer 2 0.30\n"
            b"SAMPLE timer 0 0.10\n"
            b"SAMPLE timer 1 0.20\n"
            b"DONE timers 3\n"
        )
        samples = bench.parse_timer_samples(output, expected_count=3)
        self.assertEqual([sample["index"] for sample in samples], [0, 1, 2])
        self.assertEqual(
            [sample["overshoot_ms"] for sample in samples],
            [0.10, 0.20, 0.30],
        )

        duplicate = io.BytesIO(
            b"SAMPLE timer 0 0.1\n"
            b"SAMPLE timer 0 0.2\n"
            b"DONE timers 2\n"
        )
        with self.assertRaisesRegex(bench.BenchmarkError, "duplicate"):
            bench.parse_timer_samples(duplicate, expected_count=2)
        negative = io.BytesIO(
            b"SAMPLE timer 0 -0.1\n"
            b"DONE timers 1\n"
        )
        with self.assertRaisesRegex(bench.BenchmarkError, "nonnegative"):
            bench.parse_timer_samples(negative, expected_count=1)

    def test_timer_ready_protocol_is_specialized_and_strict(self) -> None:
        cases = [
            (b"READY timers 2 10 1 2\n", "count"),
            (b"READY timers 3 11 1 2\n", "duration"),
            (b"READY timers 3 10 nan 2\n", "min_start_ms"),
            (b"READY timers 3 10 -1 2\n", "nonnegative"),
            (b"READY timers 3 10 2 1\n", "before"),
            (b"READY timers 3 10 1 2 extra\n", "READY"),
        ]
        for line, expected in cases:
            with self.subTest(line=line):
                with self.assertRaisesRegex(bench.BenchmarkError, expected):
                    bench.parse_timer_ready_line(
                        line,
                        expected_count=3,
                        expected_duration_ms=10,
                    )

    def test_timer_protocol_rejects_extra_output(self) -> None:
        output = io.BytesIO(
            b"SAMPLE timer 0 0.1\n"
            b"DONE timers 1\n"
            b"unexpected\n"
        )
        with self.assertRaisesRegex(bench.BenchmarkError, "trailing"):
            bench.parse_timer_samples(output, expected_count=1)

    def test_massive_protocol_requires_all_timer_samples_and_exact_done(self) -> None:
        ready = bench.parse_massive_ready_line(
            b"READY massive 100000 1000 10 500 507\n",
            expected_sleepers=100000,
            expected_timer_count=1000,
            expected_duration_ms=10,
        )
        self.assertEqual(ready["sleepers"], 100000)
        self.assertEqual(ready["timer_count"], 1000)
        self.assertEqual(ready["arm_span_ms"], 7.0)

        output = io.BytesIO(
            b"SAMPLE massive_timer 1 0.20\n"
            b"SAMPLE massive_timer 0 0.10\n"
            b"DONE massive 100000 2\n"
        )
        samples = bench.parse_massive_samples(
            output, expected_sleepers=100000, expected_timer_count=2
        )
        self.assertEqual(
            [sample["overshoot_ms"] for sample in samples], [0.10, 0.20]
        )

        duplicate = io.BytesIO(
            b"SAMPLE massive_timer 0 0.1\n"
            b"SAMPLE massive_timer 0 0.2\n"
            b"DONE massive 100000 2\n"
        )
        with self.assertRaisesRegex(bench.BenchmarkError, "duplicate"):
            bench.parse_massive_samples(
                duplicate, expected_sleepers=100000, expected_timer_count=2
            )

    def test_starvation_protocol_is_exact_and_bounded(self) -> None:
        self.assertEqual(
            bench.parse_starvation_output(
                b"SAMPLE starvation 10 17\nDONE starvation\n",
                expected_sleep_ms=10,
            ),
            {"sleep_ms": 10, "elapsed_ms": 17},
        )
        cases = [
            (b"SAMPLE starvation 11 17\nDONE starvation\n", "sleep duration"),
            (b"SAMPLE starvation 10 -1\nDONE starvation\n", "nonnegative"),
            (b"SAMPLE starvation 10 17\nWRONG\n", "DONE"),
            (
                b"SAMPLE starvation 10 17\nDONE starvation\nunexpected\n",
                "trailing",
            ),
        ]
        for output, expected in cases:
            with self.subTest(output=output):
                with self.assertRaisesRegex(bench.BenchmarkError, expected):
                    bench.parse_starvation_output(output, expected_sleep_ms=10)


class StatisticsTests(unittest.TestCase):
    def test_nearest_rank_percentiles_and_summary(self) -> None:
        values = [5.0, 1.0, 4.0, 2.0, 3.0]
        self.assertEqual(bench.nearest_rank(values, 0.50), 3.0)
        self.assertEqual(bench.nearest_rank(values, 0.95), 5.0)
        self.assertEqual(
            bench.timer_summary(values),
            {"p50_ms": 3.0, "p95_ms": 5.0, "p99_ms": 5.0, "max_ms": 5.0},
        )

    def test_v6_summary_has_median_mad_p95_and_best(self) -> None:
        summary = bench.duration_summary([1.0, 2.0, 10.0])
        self.assertEqual(summary["median_s"], 2.0)
        self.assertEqual(summary["mad_s"], 1.0)
        self.assertEqual(summary["p95_s"], 10.0)
        self.assertEqual(summary["best_s"], 1.0)

    def test_timer_gate_uses_worst_valid_run_and_reports_invalid_runs(self) -> None:
        runs = [
            {
                "arm_span_ms": 2.0,
                "arm_span_valid": True,
                "summary": {"p99_ms": 6.0},
            },
            {
                "arm_span_ms": 3.0,
                "arm_span_valid": True,
                "summary": {"p99_ms": 0.0},
            },
            {
                "arm_span_ms": 11.0,
                "arm_span_valid": False,
                "summary": {"p99_ms": 100.0},
            },
        ]
        summary = bench.timer_gate_summary(runs)
        self.assertEqual(summary["worst_valid_run_p99_ms"], 6.0)
        self.assertEqual(summary["valid_run_indexes"], [0, 1])
        self.assertEqual(summary["invalid_overlap_runs"], [2])

    def test_starvation_gate_uses_worst_repetition(self) -> None:
        summary = bench.starvation_gate_summary(
            [{"elapsed_ms": 12}, {"elapsed_ms": 49}, {"elapsed_ms": 20}]
        )
        self.assertEqual(summary["observed_max_ms"], 49)
        self.assertEqual(summary["limit_ms"], 50)
        self.assertTrue(summary["passed"])
        self.assertFalse(
            bench.starvation_gate_summary([{"elapsed_ms": 51}])["passed"]
        )

    def test_massive_gate_is_joint_rss_timer_and_overlap_evidence(self) -> None:
        passing_runs = [
            {
                "peak_rss_bytes": 1024,
                "incremental_peak_rss_bytes": 1024,
                "arm_span_ms": 3.0,
                "arm_span_valid": True,
                "summary": {"p99_ms": 4.0},
            },
            {
                "peak_rss_bytes": 2048,
                "incremental_peak_rss_bytes": 2048,
                "arm_span_ms": 5.0,
                "arm_span_valid": True,
                "summary": {"p99_ms": 5.0},
            },
        ]
        gate = bench.massive_gate_summary(passing_runs)
        self.assertEqual(gate["observed_peak_rss_bytes"], 2048)
        self.assertEqual(gate["observed_incremental_peak_rss_bytes"], 2048)
        self.assertEqual(gate["observed_timer_p99_ms"], 5.0)
        self.assertTrue(gate["passed"])

        over_rss = [dict(passing_runs[0])]
        over_rss[0]["peak_rss_bytes"] = (
            bench.MASSIVE_RSS_LIMIT_BYTES + 1
        )
        self.assertFalse(bench.massive_gate_summary(over_rss)["passed"])

        invalid_overlap = [dict(passing_runs[0], arm_span_valid=False)]
        self.assertFalse(
            bench.massive_gate_summary(invalid_overlap)["passed"]
        )

    def test_sleepers_gate_uses_whole_process_peak_but_reports_incremental(self) -> None:
        runs = [
            {
                "peak_rss_bytes": bench.SLEEPER_RSS_LIMIT_BYTES + 1,
                "incremental_peak_rss_bytes": 1024,
            }
        ]
        gate = bench.rss_gate_summary(
            runs,
            limit_bytes=bench.SLEEPER_RSS_LIMIT_BYTES,
        )
        self.assertEqual(
            gate["observed_peak_rss_bytes"],
            bench.SLEEPER_RSS_LIMIT_BYTES + 1,
        )
        self.assertEqual(gate["observed_incremental_peak_rss_bytes"], 1024)
        self.assertFalse(gate["passed"])

    def test_massive_ready_and_cleanup_timeouts_are_both_300_seconds(self) -> None:
        self.assertEqual(bench.MASSIVE_READY_TIMEOUT_SECONDS, 300.0)
        self.assertEqual(bench.MASSIVE_COMPLETION_TIMEOUT_SECONDS, 300.0)


class ProcessUnitTests(unittest.TestCase):
    def test_rss_units_are_normalized_to_bytes(self) -> None:
        self.assertEqual(bench.parse_macos_ps_rss_bytes("  2048\n"), 2 * 1024 * 1024)
        status = "Name:\taura\nVmRSS:\t1536 kB\n"
        self.assertEqual(bench.parse_linux_status_rss_bytes(status), 1536 * 1024)
        with self.assertRaisesRegex(bench.BenchmarkError, "VmRSS"):
            bench.parse_linux_status_rss_bytes("Name:\taura\n")

    def test_cpu_time_parser_handles_portable_ps_shapes(self) -> None:
        self.assertEqual(bench.parse_ps_cpu_seconds("02:03"), 123.0)
        self.assertEqual(bench.parse_ps_cpu_seconds("01:02:03"), 3723.0)
        self.assertEqual(bench.parse_ps_cpu_seconds("2-01:02:03"), 176523.0)

    def test_macos_rusage_parser_reads_resident_bytes_and_nanosecond_cpu(self) -> None:
        record = bytearray(160)
        struct.pack_into("=Q", record, 16, 1_250_000_000)
        struct.pack_into("=Q", record, 24, 750_000_000)
        struct.pack_into("=Q", record, 64, 123_456_789)
        stats = bench.parse_macos_rusage_v2(bytes(record))
        self.assertEqual(stats.rss_bytes, 123_456_789)
        self.assertEqual(stats.cpu_seconds, 2.0)

    def test_timer_monitor_never_uses_ps_as_a_sampling_fallback(self) -> None:
        with mock.patch.object(bench.platform, "system", return_value="Darwin"):
            with mock.patch.object(
                bench, "read_macos_proc_pid_rusage", side_effect=OSError("unavailable")
            ):
                with mock.patch.object(bench.subprocess, "run") as run:
                    monitor = bench.ProcessMonitor(
                        os.getpid(),
                        sample_interval_seconds=0.001,
                        allow_macos_ps_fallback=False,
                    )
                    monitor.start()
                    time.sleep(0.01)
                    monitor.stop()
        run.assert_not_called()
        self.assertEqual(monitor.samples, [])
        self.assertFalse(monitor.is_alive)

    def test_sampling_errors_invalidate_benchmark_evidence(self) -> None:
        monitor = mock.Mock()
        monitor.sampling_error = "proc_pid_rusage failed"
        monitor.samples = [{"rss_bytes": 1}]
        with self.assertRaisesRegex(bench.BenchmarkError, "sampling failed"):
            bench.require_monitor_evidence(monitor, "massive")

        monitor.sampling_error = None
        monitor.samples = []
        with self.assertRaisesRegex(bench.BenchmarkError, "no process samples"):
            bench.require_monitor_evidence(monitor, "massive")


class ValidationAndExecutionTests(unittest.TestCase):
    def make_executable(self, root: Path, name: str, body: str) -> Path:
        path = root / name
        path.write_text(
            "#!/bin/sh\n" + body,
            encoding="utf-8",
        )
        path.chmod(path.stat().st_mode | stat.S_IXUSR)
        return path

    def test_cli_validation_rejects_debug_aura_and_target_json(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            debug = root / "target/debug/aura"
            debug.parent.mkdir(parents=True)
            self.make_executable(debug.parent, "aura", 'printf "aura 0.1.0\\n"\n')
            with self.assertRaisesRegex(bench.BenchmarkError, "debug"):
                bench.validate_options(
                    bench.Options(
                        label="baseline",
                        aura=debug,
                        repeats=1,
                        timer_repeats=1,
                        v6_repeats=1,
                        idle_seconds=0.01,
                        json_path=root / "result.json",
                        allow_competing_processes=False,
                    ),
                    root=root,
                )

            release = root / "target/release/aura"
            release.parent.mkdir(parents=True)
            self.make_executable(release.parent, "aura", 'printf "aura 0.1.0\\n"\n')
            with self.assertRaisesRegex(bench.BenchmarkError, "outside target"):
                bench.validate_options(
                    bench.Options(
                        label="baseline",
                        aura=release,
                        repeats=1,
                        timer_repeats=1,
                        v6_repeats=1,
                        idle_seconds=0.01,
                        json_path=root / "target/result.json",
                        allow_competing_processes=False,
                    ),
                    root=root,
                )

    def test_aura_qualification_requires_checkout_release_binary_and_fresh_build(
        self,
    ) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            expected = root / "target/release/aura"
            expected.parent.mkdir(parents=True)
            self.make_executable(expected.parent, "aura", 'printf "aura\\n"\n')
            outside = self.make_executable(root, "aura", 'printf "aura\\n"\n')

            with self.assertRaisesRegex(bench.BenchmarkError, "target/release"):
                bench.qualify_aura_binary(outside, root=root)

            completed = mock.Mock(returncode=0, stdout=b"", stderr=b"")
            with mock.patch.object(
                bench.subprocess, "run", return_value=completed
            ) as run:
                record = bench.qualify_aura_binary(expected, root=root)
            self.assertEqual(record["path"], str(expected.resolve()))
            self.assertTrue(record["fresh_cargo_build"])
            command = run.call_args.args[0]
            self.assertEqual(
                command,
                [
                    "cargo",
                    "build",
                    "--release",
                    "--locked",
                    "-p",
                    "aura",
                    "--target-dir",
                    str((root / "target").resolve()),
                ],
            )

    def test_cli_validation_rejects_nonpositive_counts(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            aura = self.make_executable(root, "aura", 'printf "aura 0.1.0\\n"\n')
            with self.assertRaisesRegex(bench.BenchmarkError, "positive"):
                bench.validate_options(
                    bench.Options(
                        label="baseline",
                        aura=aura,
                        repeats=0,
                        timer_repeats=1,
                        v6_repeats=1,
                        idle_seconds=0.01,
                        json_path=root / "result.json",
                        allow_competing_processes=False,
                    ),
                    root=root,
                )

    def test_v6_fake_binary_requires_exact_stdout(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            valid = self.make_executable(root, "valid", 'printf "10000000\\n"\n')
            invalid = self.make_executable(
                root, "invalid", 'printf "10000000 extra\\n"\n'
            )
            result = bench.run_v6_once(valid)
            self.assertEqual(result["stdout"], "10000000\n")
            self.assertGreaterEqual(result["elapsed_s"], 0.0)
            with self.assertRaisesRegex(bench.BenchmarkError, "stdout"):
                bench.run_v6_once(invalid)

    def test_starvation_run_records_elapsed_sleep_and_rejects_noise(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            valid = self.make_executable(
                root,
                "valid-starvation",
                'printf "SAMPLE starvation 10 17\\nDONE starvation\\n"\n',
            )
            invalid = self.make_executable(
                root,
                "invalid-starvation",
                'printf "SAMPLE starvation 10 17\\nDONE starvation\\nnoise\\n"\n',
            )
            result = bench.run_starvation(valid)
            self.assertEqual(result["sleep_ms"], 10)
            self.assertEqual(result["elapsed_ms"], 17)
            self.assertEqual(result["returncode"], 0)
            with self.assertRaisesRegex(bench.BenchmarkError, "trailing"):
                bench.run_starvation(invalid)

    def test_massive_run_records_incremental_rss_and_timer_evidence(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            binary = self.make_executable(
                Path(directory),
                "massive",
                'printf "BASELINE massive 2 2 10\\n"\n'
                "sleep 0.02\n"
                'printf "READY massive 2 2 10 100 101\\n"\n'
                "sleep 0.02\n"
                'printf "SAMPLE massive_timer 1 0.2\\n"\n'
                'printf "SAMPLE massive_timer 0 0.1\\n"\n'
                'printf "DONE massive 2 2\\n"\n',
            )
            with mock.patch.object(bench, "MASSIVE_SLEEPER_COUNT", 2):
                with mock.patch.object(bench, "MASSIVE_TIMER_COUNT", 2):
                    result = bench.run_massive(binary)
        self.assertEqual(result["ready_observation"]["sleepers"], 2)
        self.assertEqual(result["ready_observation"]["timer_count"], 2)
        self.assertEqual(result["summary"]["p99_ms"], 0.2)
        self.assertGreaterEqual(result["incremental_peak_rss_bytes"], 0)
        self.assertEqual(result["returncode"], 0)

    def test_sleepers_waits_for_exact_natural_completion(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            binary = self.make_executable(
                Path(directory),
                "sleepers",
                'printf "BASELINE sleepers 10000\\n"\n'
                'printf "READY sleepers 10000\\n"\n'
                "sleep 0.05\n"
                'printf "DONE sleepers 10000\\n"\n',
            )
            result = bench.run_sleepers(binary, stable_seconds=0.01)
        self.assertEqual(result["completion"]["returncode"], 0)
        self.assertEqual(result["completion"]["stdout"], "DONE sleepers 10000\n")
        self.assertEqual(result["completion"]["stderr"], "")
        self.assertGreaterEqual(result["ready_to_done_s"], 0.01)
        self.assertGreaterEqual(result["incremental_peak_rss_bytes"], 0)

    def test_sleepers_rejects_done_before_required_stable_window(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            binary = self.make_executable(
                Path(directory),
                "sleepers",
                'printf "BASELINE sleepers 10000\\n"\n'
                'printf "READY sleepers 10000\\n"\n'
                "sleep 0.01\n"
                'printf "DONE sleepers 10000\\n"\n',
            )
            with self.assertRaisesRegex(bench.BenchmarkError, "completed too early"):
                bench.run_sleepers(binary, stable_seconds=0.1)

    def test_idle_waits_for_exact_natural_completion(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            binary = self.make_executable(
                Path(directory),
                "idle",
                'printf "READY idle 10 30000\\n"\n'
                "sleep 0.05\n"
                'printf "DONE idle 10\\n"\n',
            )
            result = bench.run_idle(binary, stable_seconds=0.01)
        self.assertEqual(result["completion"]["returncode"], 0)
        self.assertEqual(result["completion"]["stdout"], "DONE idle 10\n")
        self.assertEqual(result["completion"]["stderr"], "")

    def test_natural_completion_rejects_stderr_nonzero_and_wrong_done(self) -> None:
        cases = [
            ('printf "WRONG\\n"\n', "stdout"),
            ('printf "DONE sleepers 10000\\n"\nprintf "noise" >&2\n', "stderr"),
            ('printf "DONE sleepers 10000\\n"\nexit 7\n', "status 7"),
        ]
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            for index, (tail, expected) in enumerate(cases):
                binary = self.make_executable(
                    root,
                    "sleepers-" + str(index),
                    'printf "BASELINE sleepers 10000\\n"\n'
                    'printf "READY sleepers 10000\\n"\n'
                    "sleep 0.01\n"
                    + tail,
                )
                with self.subTest(expected=expected):
                    with self.assertRaisesRegex(bench.BenchmarkError, expected):
                        bench.run_sleepers(binary, stable_seconds=0.001)

    def test_validation_allows_the_advertised_idle_window(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            aura = self.make_executable(root, "aura", 'printf "aura 0.1.0\\n"\n')
            bench.validate_options(
                bench.Options(
                    label="baseline",
                    aura=aura,
                    repeats=1,
                    timer_repeats=1,
                    v6_repeats=1,
                    idle_seconds=30.0,
                    json_path=root / "result.json",
                    allow_competing_processes=False,
                ),
                root=root,
            )

    def test_process_inventory_filters_to_repo_competitors(self) -> None:
        root = Path("/repo")
        rows = [
            bench.ProcessRow(10, "cargo", "cargo test", Path("/repo")),
            bench.ProcessRow(11, "rustc", "rustc --crate-name x", Path("/other")),
            bench.ProcessRow(12, "aura", "/repo/target/release/aura run x.au", None),
            bench.ProcessRow(os.getpid(), "cargo", "cargo test", Path("/repo")),
        ]
        competitors = bench.find_competing_processes(
            root, rows=rows, ignored_pids={os.getpid()}
        )
        self.assertEqual([process.pid for process in competitors], [10, 12])

    def test_contractual_status_requires_both_quiet_checks_and_no_override(self) -> None:
        competitor = bench.ProcessRow(
            10, "cargo", "cargo test", Path("/repo")
        )
        self.assertTrue(bench.benchmark_is_contractual(False, ([], [])))
        self.assertFalse(
            bench.benchmark_is_contractual(False, ([], [competitor]))
        )
        self.assertFalse(bench.benchmark_is_contractual(True, ([], [])))
        self.assertEqual(
            bench.benchmark_noncontractual_reasons(True, ([], [competitor])),
            [
                "the competing-process override was enabled",
                "competing Aurora-repository processes were observed",
            ],
        )

    def test_contractual_status_requires_clean_mac14_9_evidence(self) -> None:
        clean_repository = {"dirty_files": []}
        dirty_repository = {"dirty_files": [" M crates/runtime.rs"]}
        baseline_host = {"hardware_model": "Mac14,9"}
        other_host = {"hardware_model": "Mac15,6"}

        self.assertTrue(
            bench.benchmark_is_contractual(
                False,
                ([], []),
                host=baseline_host,
                repository=clean_repository,
            )
        )
        self.assertEqual(
            bench.benchmark_noncontractual_reasons(
                False,
                ([], []),
                host=other_host,
                repository=dirty_repository,
            ),
            [
                "host hardware model is not the contractual Mac14,9 baseline",
                "repository worktree was dirty",
            ],
        )

    def test_execute_rechecks_process_inventory_after_workload_builds(self) -> None:
        competitor = bench.ProcessRow(
            10, "cargo", "cargo test", Path("/repo")
        )
        options = bench.Options(
            label="baseline",
            aura=Path("/tmp/release/aura"),
            repeats=1,
            timer_repeats=1,
            v6_repeats=1,
            idle_seconds=1.0,
            json_path=Path("/tmp/result.json"),
            allow_competing_processes=False,
        )
        with mock.patch.object(bench, "validate_options"):
            with mock.patch.object(
                bench, "qualify_aura_binary", return_value={}
            ):
                with mock.patch.object(
                    bench,
                    "find_competing_processes",
                    side_effect=[[], [competitor]],
                ):
                    with mock.patch.object(
                        bench, "hardware_record", return_value={}
                    ):
                        with mock.patch.object(
                            bench, "repository_record", return_value={}
                        ):
                            with mock.patch.object(
                                bench, "build_workloads", return_value=({}, [])
                            ):
                                with mock.patch.object(
                                    bench,
                                    "compiler_runtime_inputs",
                                    return_value={},
                                ):
                                    with self.assertRaisesRegex(
                                        bench.BenchmarkError,
                                        "immediately before timing",
                                    ):
                                        bench.execute(options)


if __name__ == "__main__":
    unittest.main()
