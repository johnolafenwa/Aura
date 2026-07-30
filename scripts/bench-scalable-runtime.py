#!/usr/bin/env python3
"""Run the Phase-5 scalable-runtime host benchmarks reproducibly.

The runner deliberately builds standalone direct-backend executables before
starting any clock. It records raw observations and host/repository identity in
one JSON document so benchmark summaries can always be recomputed.
"""

from __future__ import annotations

import argparse
import ctypes
import functools
import hashlib
import io
import json
import math
import os
import pathlib
import platform
import selectors
import shutil
import statistics
import struct
import subprocess
import sys
import tempfile
import threading
import time
from datetime import datetime, timezone
from typing import (
    BinaryIO,
    Dict,
    Iterable,
    List,
    NamedTuple,
    Optional,
    Sequence,
    Set,
    Tuple,
)

try:
    from scripts import benchmark_process
except ImportError:
    # Direct script execution places scripts/ rather than the repository root
    # on sys.path.
    import benchmark_process


ROOT = pathlib.Path(__file__).resolve().parent.parent
REPORT_SCHEMA_VERSION = 4
MAX_PROTOCOL_LINE_BYTES = 64 * 1024
READY_LINE_BYTES = 256
READY_TIMEOUT_SECONDS = 20.0
MASSIVE_READY_TIMEOUT_SECONDS = 300.0
TIMER_TIMEOUT_SECONDS = 60.0
MASSIVE_COMPLETION_TIMEOUT_SECONDS = 300.0
STARVATION_TIMEOUT_SECONDS = 10.0
NATURAL_COMPLETION_TIMEOUT_SECONDS = 40.0
RSS_SAMPLE_INTERVAL_SECONDS = 0.05
IDLE_SAMPLE_INTERVAL_SECONDS = 0.25
TIMER_SAMPLE_INTERVAL_SECONDS = 0.05
MULTICORE_SAMPLE_INTERVAL_SECONDS = 0.025
SLEEPER_RSS_LIMIT_BYTES = 512 * 1024 * 1024
MASSIVE_RSS_LIMIT_BYTES = 1536 * 1024 * 1024
MASSIVE_SLEEPER_COUNT = 100_000
MASSIVE_TIMER_COUNT = 1_000
MASSIVE_TIMER_DURATION_MS = 10
TIMER_P99_LIMIT_MS = 5.0
IDLE_CPU_LIMIT_PERCENT = 2.0
TIMER_ARM_SPAN_LIMIT_MS = 10.0
STARVATION_SLEEP_MS = 10
STARVATION_LATENCY_LIMIT_MS = 50
EXPECTED_V6_STDOUT = b"10000000\n"
EXPECTED_V6_STARTUP_STDOUT = b""
MULTICORE_WORKERS = 4
MULTICORE_TASK_COUNTS = (1, 4)
MULTICORE_ITERATIONS = 80_000_000
MULTICORE_MULTIPLIER = 48_271
MULTICORE_MODULUS = 2_147_483_647
MULTICORE_MIN_REPEATS = 5
MULTICORE_DEFAULT_REPEATS = 7
MULTICORE_MIN_SIGNAL_SECONDS = 0.250
MULTICORE_MAX_RELATIVE_MAD = 0.15
MULTICORE_MIN_FOUR_TASK_CPU_PERCENT = 150.0
MULTICORE_GATE_RATIO = 1.6
MULTICORE_TIMEOUT_SECONDS = 120.0

WORKLOADS = {
    "sleepers": ROOT / "benchmarks/scalable_runtime/10k_sleepers.au",
    "massive": ROOT
    / "benchmarks/scalable_runtime/100k_sleepers_1000_timers.au",
    "timers": ROOT / "benchmarks/scalable_runtime/1000_timers.au",
    "idle": ROOT / "benchmarks/scalable_runtime/idle_10_tasks.au",
    "starvation": ROOT / "benchmarks/scalable_runtime/sleeper_vs_hot_loop.au",
    "multicore": ROOT / "benchmarks/scalable_runtime/cpu_scaling.au",
    "startup": ROOT / "benchmarks/direct_integer_loops/startup.au",
    "int32": ROOT / "benchmarks/direct_integer_loops/int32_loop.au",
    "int64": ROOT / "benchmarks/direct_integer_loops/int64_loop.au",
}


class BenchmarkError(RuntimeError):
    """A benchmark cannot produce trustworthy evidence."""


class Options(NamedTuple):
    label: str
    aura: pathlib.Path
    repeats: int
    timer_repeats: int
    v6_repeats: int
    multicore_repeats: int
    idle_seconds: float
    json_path: pathlib.Path
    allow_competing_processes: bool


class ProcessRow(NamedTuple):
    pid: int
    command: str
    arguments: str
    cwd: Optional[pathlib.Path]


class ProcessStats(NamedTuple):
    rss_bytes: int
    cpu_seconds: float


class MachTimebaseInfo(ctypes.Structure):
    _fields_ = [
        ("numer", ctypes.c_uint32),
        ("denom", ctypes.c_uint32),
    ]


def sha256_file(path: pathlib.Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as stream:
        for chunk in iter(lambda: stream.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def is_within(path: pathlib.Path, parent: pathlib.Path) -> bool:
    try:
        path.resolve().relative_to(parent.resolve())
        return True
    except ValueError:
        return False


def nearest_rank(values: Sequence[float], percentile: float) -> float:
    if not values:
        raise BenchmarkError("cannot summarize an empty sample")
    if not 0.0 < percentile <= 1.0:
        raise BenchmarkError("percentile must be in (0, 1]")
    ordered = sorted(float(value) for value in values)
    rank = max(1, math.ceil(percentile * len(ordered)))
    return ordered[rank - 1]


def timer_summary(values: Sequence[float]) -> Dict[str, float]:
    return {
        "p50_ms": nearest_rank(values, 0.50),
        "p95_ms": nearest_rank(values, 0.95),
        "p99_ms": nearest_rank(values, 0.99),
        "max_ms": max(values),
    }


def duration_summary(values: Sequence[float]) -> Dict[str, float]:
    if not values:
        raise BenchmarkError("cannot summarize an empty duration sample")
    numeric = [float(value) for value in values]
    median = statistics.median(numeric)
    return {
        "median_s": median,
        "mad_s": statistics.median(abs(value - median) for value in numeric),
        "p95_s": nearest_rank(numeric, 0.95),
        "best_s": min(numeric),
    }


def park_miller_checksum(
    *,
    tasks: int,
    iterations: int,
    multiplier: int = MULTICORE_MULTIPLIER,
    modulus: int = MULTICORE_MODULUS,
) -> int:
    if tasks <= 0 or iterations < 0 or multiplier <= 0 or modulus <= 1:
        raise BenchmarkError("invalid Park-Miller checksum parameters")
    factor = pow(multiplier, iterations, modulus)
    return sum((seed * factor) % modulus for seed in range(1, tasks + 1))


def parse_multicore_ready_line(
    line: bytes,
    *,
    expected_tasks: int,
    expected_iterations: int,
    expected_multiplier: int,
    expected_modulus: int,
) -> Dict[str, int]:
    fields = line.rstrip(b"\n").split(b" ")
    if (
        not line.endswith(b"\n")
        or len(fields) != 6
        or fields[:2] != [b"READY", b"multicore"]
    ):
        raise BenchmarkError("unexpected multicore READY line: " + repr(line))
    try:
        tasks, iterations, multiplier, modulus = (
            int(field.decode("ascii")) for field in fields[2:]
        )
    except (UnicodeDecodeError, ValueError) as error:
        raise BenchmarkError("invalid multicore READY integer field") from error
    observed = (tasks, iterations, multiplier, modulus)
    expected = (
        expected_tasks,
        expected_iterations,
        expected_multiplier,
        expected_modulus,
    )
    if observed != expected:
        raise BenchmarkError(
            "unexpected multicore READY values: expected "
            + repr(expected)
            + ", got "
            + repr(observed)
        )
    return {
        "tasks": tasks,
        "iterations": iterations,
        "multiplier": multiplier,
        "modulus": modulus,
    }


def parse_multicore_done_line(
    line: bytes, *, expected_tasks: int, expected_checksum: int
) -> Dict[str, int]:
    fields = line.rstrip(b"\n").split(b" ")
    if (
        not line.endswith(b"\n")
        or len(fields) != 4
        or fields[:2] != [b"DONE", b"multicore"]
    ):
        raise BenchmarkError("unexpected multicore DONE line: " + repr(line))
    try:
        tasks = int(fields[2].decode("ascii"))
        checksum = int(fields[3].decode("ascii"))
    except (UnicodeDecodeError, ValueError) as error:
        raise BenchmarkError("invalid multicore DONE integer field") from error
    if tasks != expected_tasks:
        raise BenchmarkError(
            "unexpected multicore DONE task count: expected "
            + str(expected_tasks)
            + ", got "
            + str(tasks)
        )
    if checksum != expected_checksum:
        raise BenchmarkError(
            "unexpected multicore checksum: expected "
            + str(expected_checksum)
            + ", got "
            + str(checksum)
        )
    return {"tasks": tasks, "checksum": checksum}


def read_bounded_line(stream: BinaryIO, maximum_bytes: int) -> bytes:
    line = stream.readline(maximum_bytes + 1)
    if len(line) > maximum_bytes:
        raise BenchmarkError(
            "protocol line exceeded the "
            + str(maximum_bytes)
            + "-byte bound"
        )
    if not line:
        raise BenchmarkError("benchmark closed stdout before completing its protocol")
    if not line.endswith(b"\n"):
        raise BenchmarkError("benchmark protocol line was not newline-terminated")
    return line


def parse_phase_line(
    line: bytes,
    phase: str,
    benchmark: str,
    expected_fields: Sequence[str],
) -> Tuple[str, ...]:
    expected = phase + " " + benchmark
    if expected_fields:
        expected += " " + " ".join(expected_fields)
    expected_bytes = (expected + "\n").encode("ascii")
    if line != expected_bytes:
        raise BenchmarkError(
            "unexpected "
            + phase
            + " line for "
            + benchmark
            + ": expected "
            + repr(expected_bytes)
            + ", got "
            + repr(line)
        )
    return tuple(expected_fields)


def parse_ready_line(
    line: bytes, benchmark: str, expected_fields: Sequence[str]
) -> Tuple[str, ...]:
    return parse_phase_line(
        line,
        phase="READY",
        benchmark=benchmark,
        expected_fields=expected_fields,
    )


def parse_timer_ready_line(
    line: bytes, expected_count: int, expected_duration_ms: int
) -> Dict[str, float | int]:
    fields = line.rstrip(b"\n").split(b" ")
    if (
        not line.endswith(b"\n")
        or len(fields) != 6
        or fields[:2] != [b"READY", b"timers"]
    ):
        raise BenchmarkError("unexpected timer READY line: " + repr(line))
    try:
        count = int(fields[2].decode("ascii"))
    except (UnicodeDecodeError, ValueError) as error:
        raise BenchmarkError("invalid timer READY count") from error
    if count != expected_count:
        raise BenchmarkError(
            "unexpected timer READY count: expected "
            + str(expected_count)
            + ", got "
            + str(count)
        )
    try:
        duration_ms = int(fields[3].decode("ascii"))
    except (UnicodeDecodeError, ValueError) as error:
        raise BenchmarkError("invalid timer READY duration") from error
    if duration_ms != expected_duration_ms:
        raise BenchmarkError(
            "unexpected timer READY duration: expected "
            + str(expected_duration_ms)
            + ", got "
            + str(duration_ms)
        )
    min_start_ms = finite_float(fields[4], "timer min_start_ms")
    max_start_ms = finite_float(fields[5], "timer max_start_ms")
    if min_start_ms < 0.0 or max_start_ms < 0.0:
        raise BenchmarkError("timer start observations must be nonnegative")
    if max_start_ms < min_start_ms:
        raise BenchmarkError(
            "timer max_start_ms is before timer min_start_ms"
        )
    return {
        "count": count,
        "duration_ms": duration_ms,
        "min_start_ms": min_start_ms,
        "max_start_ms": max_start_ms,
        "arm_span_ms": max_start_ms - min_start_ms,
    }


def parse_massive_ready_line(
    line: bytes,
    expected_sleepers: int,
    expected_timer_count: int,
    expected_duration_ms: int,
) -> Dict[str, float | int]:
    fields = line.rstrip(b"\n").split(b" ")
    if (
        not line.endswith(b"\n")
        or len(fields) != 7
        or fields[:2] != [b"READY", b"massive"]
    ):
        raise BenchmarkError("unexpected massive READY line: " + repr(line))
    try:
        sleepers = int(fields[2].decode("ascii"))
        timer_count = int(fields[3].decode("ascii"))
        duration_ms = int(fields[4].decode("ascii"))
    except (UnicodeDecodeError, ValueError) as error:
        raise BenchmarkError("invalid massive READY integer field") from error
    if sleepers != expected_sleepers:
        raise BenchmarkError(
            "unexpected massive sleeper count: expected "
            + str(expected_sleepers)
            + ", got "
            + str(sleepers)
        )
    if timer_count != expected_timer_count:
        raise BenchmarkError(
            "unexpected massive timer count: expected "
            + str(expected_timer_count)
            + ", got "
            + str(timer_count)
        )
    if duration_ms != expected_duration_ms:
        raise BenchmarkError(
            "unexpected massive timer duration: expected "
            + str(expected_duration_ms)
            + ", got "
            + str(duration_ms)
        )
    min_start_ms = finite_float(fields[5], "massive timer min_start_ms")
    max_start_ms = finite_float(fields[6], "massive timer max_start_ms")
    if min_start_ms < 0.0 or max_start_ms < 0.0:
        raise BenchmarkError("massive timer start observations must be nonnegative")
    if max_start_ms < min_start_ms:
        raise BenchmarkError(
            "massive timer max_start_ms is before massive timer min_start_ms"
        )
    return {
        "sleepers": sleepers,
        "timer_count": timer_count,
        "duration_ms": duration_ms,
        "min_start_ms": min_start_ms,
        "max_start_ms": max_start_ms,
        "arm_span_ms": max_start_ms - min_start_ms,
    }


def read_process_ready_line(
    process: subprocess.Popen,
    benchmark: str,
    timeout_seconds: float = READY_TIMEOUT_SECONDS,
) -> bytes:
    assert process.stdout is not None
    descriptor = process.stdout.fileno()
    deadline = time.monotonic() + timeout_seconds
    line = bytearray()
    selector = selectors.DefaultSelector()
    selector.register(descriptor, selectors.EVENT_READ)
    try:
        while True:
            remaining = deadline - time.monotonic()
            if remaining <= 0:
                raise BenchmarkError(
                    benchmark + " did not emit READY before the timeout"
                )
            if not selector.select(remaining):
                raise BenchmarkError(
                    benchmark + " did not emit READY before the timeout"
                )
            chunk = os.read(descriptor, 1)
            if not chunk:
                raise BenchmarkError(
                    benchmark + " exited before emitting a complete READY line"
                )
            line.extend(chunk)
            if len(line) > READY_LINE_BYTES:
                raise BenchmarkError(
                    benchmark + " READY line exceeded the byte bound"
                )
            if chunk == b"\n":
                break
    finally:
        selector.close()
    return bytes(line)


def read_process_ready(
    process: subprocess.Popen,
    benchmark: str,
    expected_fields: Sequence[str],
    timeout_seconds: float = READY_TIMEOUT_SECONDS,
) -> bytes:
    line = read_process_ready_line(process, benchmark, timeout_seconds)
    parse_ready_line(line, benchmark, expected_fields)
    return line


def read_process_phase(
    process: subprocess.Popen,
    phase: str,
    benchmark: str,
    expected_fields: Sequence[str],
    timeout_seconds: float = READY_TIMEOUT_SECONDS,
) -> bytes:
    line = read_process_ready_line(process, benchmark, timeout_seconds)
    parse_phase_line(line, phase, benchmark, expected_fields)
    return line


def finite_float(text: bytes, field: str) -> float:
    try:
        value = float(text.decode("ascii"))
    except (UnicodeDecodeError, ValueError) as error:
        raise BenchmarkError("invalid " + field + " value") from error
    if not math.isfinite(value):
        raise BenchmarkError(field + " must be finite")
    return value


def parse_timer_samples(
    stream: BinaryIO, expected_count: int
) -> List[Dict[str, float]]:
    by_index: Dict[int, Dict[str, float]] = {}
    while len(by_index) < expected_count:
        line = read_bounded_line(stream, MAX_PROTOCOL_LINE_BYTES)
        fields = line.rstrip(b"\n").split(b" ")
        if len(fields) != 4 or fields[:2] != [b"SAMPLE", b"timer"]:
            raise BenchmarkError("malformed timer sample line: " + repr(line))
        try:
            index = int(fields[2].decode("ascii"))
        except (UnicodeDecodeError, ValueError) as error:
            raise BenchmarkError("invalid timer sample index") from error
        if not 0 <= index < expected_count:
            raise BenchmarkError("timer sample index is outside the expected range")
        if index in by_index:
            raise BenchmarkError("duplicate timer sample index " + str(index))
        overshoot_ms = finite_float(fields[3], "timer overshoot_ms")
        if overshoot_ms < 0.0:
            raise BenchmarkError("timer overshoot_ms must be nonnegative")
        by_index[index] = {
            "index": index,
            "overshoot_ms": overshoot_ms,
        }

    done = read_bounded_line(stream, MAX_PROTOCOL_LINE_BYTES)
    expected_done = ("DONE timers " + str(expected_count) + "\n").encode("ascii")
    if done != expected_done:
        raise BenchmarkError(
            "unexpected timer DONE line: expected "
            + repr(expected_done)
            + ", got "
            + repr(done)
        )
    trailing = stream.read(1)
    if trailing:
        raise BenchmarkError("timer benchmark emitted trailing output")
    return [by_index[index] for index in sorted(by_index)]


def parse_massive_samples(
    stream: BinaryIO,
    expected_sleepers: int,
    expected_timer_count: int,
) -> List[Dict[str, float]]:
    by_index: Dict[int, Dict[str, float]] = {}
    while len(by_index) < expected_timer_count:
        line = read_bounded_line(stream, MAX_PROTOCOL_LINE_BYTES)
        fields = line.rstrip(b"\n").split(b" ")
        if len(fields) != 4 or fields[:2] != [b"SAMPLE", b"massive_timer"]:
            raise BenchmarkError("malformed massive timer sample line: " + repr(line))
        try:
            index = int(fields[2].decode("ascii"))
        except (UnicodeDecodeError, ValueError) as error:
            raise BenchmarkError("invalid massive timer sample index") from error
        if not 0 <= index < expected_timer_count:
            raise BenchmarkError(
                "massive timer sample index is outside the expected range"
            )
        if index in by_index:
            raise BenchmarkError("duplicate massive timer sample index " + str(index))
        overshoot_ms = finite_float(fields[3], "massive timer overshoot_ms")
        if overshoot_ms < 0.0:
            raise BenchmarkError("massive timer overshoot_ms must be nonnegative")
        by_index[index] = {
            "index": index,
            "overshoot_ms": overshoot_ms,
        }

    done = read_bounded_line(stream, MAX_PROTOCOL_LINE_BYTES)
    expected_done = (
        "DONE massive "
        + str(expected_sleepers)
        + " "
        + str(expected_timer_count)
        + "\n"
    ).encode("ascii")
    if done != expected_done:
        raise BenchmarkError(
            "unexpected massive DONE line: expected "
            + repr(expected_done)
            + ", got "
            + repr(done)
        )
    if stream.read(1):
        raise BenchmarkError("massive benchmark emitted trailing output")
    return [by_index[index] for index in sorted(by_index)]


def timer_gate_summary(
    runs: Sequence[Dict[str, object]]
) -> Dict[str, object]:
    valid_run_indexes = [
        index for index, run in enumerate(runs) if bool(run["arm_span_valid"])
    ]
    invalid_overlap_runs = [
        index for index, run in enumerate(runs) if not bool(run["arm_span_valid"])
    ]
    valid_p99_values = []
    for index in valid_run_indexes:
        summary = runs[index]["summary"]
        if not isinstance(summary, dict) or "p99_ms" not in summary:
            raise BenchmarkError("timer run has no p99 summary")
        valid_p99_values.append(float(summary["p99_ms"]))
    return {
        "valid_run_indexes": valid_run_indexes,
        "invalid_overlap_runs": invalid_overlap_runs,
        "worst_valid_run_p99_ms": (
            max(valid_p99_values) if valid_p99_values else None
        ),
    }


def massive_gate_summary(
    runs: Sequence[Dict[str, object]]
) -> Dict[str, object]:
    if not runs:
        raise BenchmarkError("massive benchmark has no repetitions")
    rss_gate = rss_gate_summary(
        runs,
        limit_bytes=MASSIVE_RSS_LIMIT_BYTES,
    )
    timer_gate = timer_gate_summary(runs)
    worst_valid_timer_p99 = timer_gate["worst_valid_run_p99_ms"]
    all_arm_spans_valid = all(bool(run["arm_span_valid"]) for run in runs)
    return {
        "observed_peak_rss_bytes": rss_gate["observed_peak_rss_bytes"],
        "observed_incremental_peak_rss_bytes": rss_gate[
            "observed_incremental_peak_rss_bytes"
        ],
        "rss_limit_bytes": rss_gate["limit_bytes"],
        "rss_measurement": rss_gate["measurement"],
        "observed_timer_p99_ms": worst_valid_timer_p99,
        "timer_p99_limit_ms": TIMER_P99_LIMIT_MS,
        "observed_max_timer_arm_span_ms": max(
            float(run["arm_span_ms"]) for run in runs
        ),
        "timer_arm_span_limit_ms": TIMER_ARM_SPAN_LIMIT_MS,
        "invalid_overlap_runs": timer_gate["invalid_overlap_runs"],
        "operator": "<=",
        "passed": (
            bool(rss_gate["passed"])
            and worst_valid_timer_p99 is not None
            and float(worst_valid_timer_p99) <= TIMER_P99_LIMIT_MS
            and all_arm_spans_valid
        ),
    }


def rss_gate_summary(
    runs: Sequence[Dict[str, object]], *, limit_bytes: int
) -> Dict[str, object]:
    if not runs:
        raise BenchmarkError("RSS benchmark has no repetitions")
    observed_peak = max(int(run["peak_rss_bytes"]) for run in runs)
    observed_incremental_peak = max(
        int(run["incremental_peak_rss_bytes"]) for run in runs
    )
    return {
        "observed_peak_rss_bytes": observed_peak,
        "observed_incremental_peak_rss_bytes": observed_incremental_peak,
        "limit_bytes": limit_bytes,
        "measurement": "whole-process peak RSS",
        "operator": "<=",
        "passed": observed_peak <= limit_bytes,
    }


def parse_starvation_output(
    output: bytes, expected_sleep_ms: int
) -> Dict[str, int]:
    stream = io.BytesIO(output)
    sample = read_bounded_line(stream, MAX_PROTOCOL_LINE_BYTES)
    fields = sample.rstrip(b"\n").split(b" ")
    if (
        not sample.endswith(b"\n")
        or len(fields) != 4
        or fields[:2] != [b"SAMPLE", b"starvation"]
    ):
        raise BenchmarkError("malformed starvation sample line: " + repr(sample))
    try:
        sleep_ms = int(fields[2].decode("ascii"))
    except (UnicodeDecodeError, ValueError) as error:
        raise BenchmarkError("invalid starvation sleep duration") from error
    if sleep_ms != expected_sleep_ms:
        raise BenchmarkError(
            "unexpected starvation sleep duration: expected "
            + str(expected_sleep_ms)
            + ", got "
            + str(sleep_ms)
        )
    try:
        elapsed_ms = int(fields[3].decode("ascii"))
    except (UnicodeDecodeError, ValueError) as error:
        raise BenchmarkError("invalid starvation elapsed duration") from error
    if elapsed_ms < 0:
        raise BenchmarkError("starvation elapsed duration must be nonnegative")
    done = read_bounded_line(stream, MAX_PROTOCOL_LINE_BYTES)
    if done != b"DONE starvation\n":
        raise BenchmarkError(
            "unexpected starvation DONE line: expected "
            + repr(b"DONE starvation\n")
            + ", got "
            + repr(done)
        )
    if stream.read(1):
        raise BenchmarkError("starvation benchmark emitted trailing output")
    return {"sleep_ms": sleep_ms, "elapsed_ms": elapsed_ms}


def starvation_gate_summary(
    runs: Sequence[Dict[str, object]]
) -> Dict[str, object]:
    if not runs:
        raise BenchmarkError("starvation benchmark has no repetitions")
    observed_max_ms = max(int(run["elapsed_ms"]) for run in runs)
    return {
        "observed_max_ms": observed_max_ms,
        "limit_ms": STARVATION_LATENCY_LIMIT_MS,
        "operator": "<=",
        "passed": observed_max_ms <= STARVATION_LATENCY_LIMIT_MS,
    }


def multicore_gate_summary(
    pairs: Sequence[Dict[str, object]],
    *,
    host: Dict[str, object],
) -> Dict[str, object]:
    if len(pairs) < MULTICORE_MIN_REPEATS:
        raise BenchmarkError(
            "multicore benchmark requires at least "
            + str(MULTICORE_MIN_REPEATS)
            + " paired repetitions"
        )
    if len(pairs) % 2 == 0:
        raise BenchmarkError("multicore paired repetition count must be odd")

    one_task_durations: List[float] = []
    four_task_durations: List[float] = []
    four_task_cpu_percent: List[float] = []
    paired_ratios: List[float] = []
    pass_pair_indexes: List[int] = []
    failed_pair_indexes: List[int] = []
    for index, pair in enumerate(pairs):
        expected_order = [1, 4] if index % 2 == 0 else [4, 1]
        if pair.get("repeat") != index or pair.get("order") != expected_order:
            raise BenchmarkError(
                "multicore repetitions must preserve their alternating paired order"
            )
        runs = pair.get("runs")
        if not isinstance(runs, dict):
            raise BenchmarkError("multicore pair has no run map")
        one = runs.get("1")
        four = runs.get("4")
        if not isinstance(one, dict) or not isinstance(four, dict):
            raise BenchmarkError("multicore pair must contain one- and four-task runs")
        one_elapsed = float(one["elapsed_s"])
        four_elapsed = float(four["elapsed_s"])
        four_cpu = float(four["process_cpu_percent"])
        if (
            not math.isfinite(one_elapsed)
            or not math.isfinite(four_elapsed)
            or one_elapsed <= 0.0
            or four_elapsed <= 0.0
            or not math.isfinite(four_cpu)
            or four_cpu < 0.0
        ):
            raise BenchmarkError("multicore timing observations must be finite")
        ratio = four_elapsed / one_elapsed
        one_task_durations.append(one_elapsed)
        four_task_durations.append(four_elapsed)
        four_task_cpu_percent.append(four_cpu)
        paired_ratios.append(ratio)
        if ratio <= MULTICORE_GATE_RATIO:
            pass_pair_indexes.append(index)
        else:
            failed_pair_indexes.append(index)

    one_summary = duration_summary(one_task_durations)
    four_summary = duration_summary(four_task_durations)
    ratio_summary = duration_summary(paired_ratios)
    one_relative_mad = (
        float(one_summary["mad_s"]) / float(one_summary["median_s"])
    )
    four_relative_mad = (
        float(four_summary["mad_s"]) / float(four_summary["median_s"])
    )
    ratio_of_medians = (
        float(four_summary["median_s"]) / float(one_summary["median_s"])
    )
    paired_median_ratio = float(ratio_summary["median_s"])
    median_four_task_cpu_percent = statistics.median(four_task_cpu_percent)

    affinity_cpus = host.get("affinity_cpus")
    physical_cores = host.get("physical_cores")
    logical_cpus = host.get("logical_cpus")
    qualified_cores = (
        int(affinity_cpus)
        if isinstance(affinity_cpus, int)
        else int(physical_cores)
        if isinstance(physical_cores, int)
        else int(logical_cpus)
        if isinstance(logical_cpus, int)
        else 0
    )
    core_source = (
        "affinity_cpus"
        if isinstance(affinity_cpus, int)
        else "physical_cores"
        if isinstance(physical_cores, int)
        else "logical_cpus"
    )
    invalid_reasons: List[str] = []
    if qualified_cores < MULTICORE_WORKERS:
        invalid_reasons.append(
            "process affinity permits fewer than 4 CPUs"
            if core_source == "affinity_cpus"
            else "host has fewer than 4 qualified physical cores"
            if core_source == "physical_cores"
            else "host has fewer than 4 qualified logical CPUs"
        )
    if float(one_summary["median_s"]) < MULTICORE_MIN_SIGNAL_SECONDS:
        invalid_reasons.append(
            "one-task median is below the 250 ms minimum timing signal"
        )
    if one_relative_mad > MULTICORE_MAX_RELATIVE_MAD:
        invalid_reasons.append("one-task MAD/median exceeds 15%")
    if four_relative_mad > MULTICORE_MAX_RELATIVE_MAD:
        invalid_reasons.append("four-task MAD/median exceeds 15%")
    if median_four_task_cpu_percent < MULTICORE_MIN_FOUR_TASK_CPU_PERCENT:
        invalid_reasons.append(
            "four-task median process CPU is below 150% of wall time"
        )

    return {
        "worker_environment": {"AURORA_WORKERS": str(MULTICORE_WORKERS)},
        "paired_repeats": len(pairs),
        "one_task_summary": one_summary,
        "four_task_summary": four_summary,
        "paired_ratio_summary": ratio_summary,
        "paired_median_ratio": paired_median_ratio,
        "ratio_of_medians": ratio_of_medians,
        "limit_ratio": MULTICORE_GATE_RATIO,
        "operator": "<=",
        "pass_pair_indexes": pass_pair_indexes,
        "failed_pair_indexes": failed_pair_indexes,
        "one_task_relative_mad": one_relative_mad,
        "four_task_relative_mad": four_relative_mad,
        "relative_mad_limit": MULTICORE_MAX_RELATIVE_MAD,
        "median_four_task_process_cpu_percent": median_four_task_cpu_percent,
        "minimum_four_task_process_cpu_percent": (
            MULTICORE_MIN_FOUR_TASK_CPU_PERCENT
        ),
        "core_qualification": {
            "source": core_source,
            "observed": qualified_cores,
            "required": MULTICORE_WORKERS,
            "passed": qualified_cores >= MULTICORE_WORKERS,
        },
        "invalid_reasons": invalid_reasons,
        "valid": not invalid_reasons,
        "passed": (
            not invalid_reasons
            and paired_median_ratio <= MULTICORE_GATE_RATIO
        ),
    }


def parse_macos_ps_rss_bytes(text: str) -> int:
    try:
        kibibytes = int(text.strip())
    except ValueError as error:
        raise BenchmarkError("invalid macOS ps RSS value") from error
    return kibibytes * 1024


def parse_macos_rusage_v2(
    record: bytes,
    *,
    timebase_numer: int = 1,
    timebase_denom: int = 1,
) -> ProcessStats:
    # Darwin's rusage_info_v2 starts with a 16-byte UUID, followed by
    # user/system times in mach absolute-time ticks. ri_resident_size is the
    # seventh u64 field.
    if len(record) < 72:
        raise BenchmarkError("macOS process rusage record is incomplete")
    if timebase_numer <= 0 or timebase_denom <= 0:
        raise BenchmarkError("macOS mach timebase must be positive")
    user_time_ticks, system_time_ticks = struct.unpack_from("=QQ", record, 16)
    resident_size = struct.unpack_from("=Q", record, 64)[0]
    cpu_nanoseconds = (
        (user_time_ticks + system_time_ticks)
        * timebase_numer
        / timebase_denom
    )
    return ProcessStats(
        rss_bytes=int(resident_size),
        cpu_seconds=cpu_nanoseconds / 1_000_000_000.0,
    )


@functools.lru_cache(maxsize=1)
def macos_mach_timebase() -> Tuple[int, int]:
    try:
        function = ctypes.CDLL(None).mach_timebase_info
    except (AttributeError, OSError) as error:
        raise OSError("macOS mach_timebase_info is unavailable") from error
    function.argtypes = [ctypes.POINTER(MachTimebaseInfo)]
    function.restype = ctypes.c_int
    info = MachTimebaseInfo()
    result = function(ctypes.byref(info))
    if result != 0 or info.numer == 0 or info.denom == 0:
        raise OSError("macOS mach_timebase_info returned an invalid timebase")
    return int(info.numer), int(info.denom)


@functools.lru_cache(maxsize=1)
def macos_proc_pid_rusage_function():
    try:
        libproc = ctypes.CDLL("/usr/lib/libproc.dylib", use_errno=True)
        function = libproc.proc_pid_rusage
    except (AttributeError, OSError) as error:
        raise OSError("macOS proc_pid_rusage is unavailable") from error
    function.argtypes = [ctypes.c_int, ctypes.c_int, ctypes.c_void_p]
    function.restype = ctypes.c_int
    return function


def read_macos_proc_pid_rusage(pid: int) -> ProcessStats:
    # RUSAGE_INFO_V2 is stable across the maintained macOS deployment range.
    # The oversized zeroed buffer avoids coupling the runner to a Python SDK's
    # exact trailing-field declaration while preserving the documented prefix.
    buffer = ctypes.create_string_buffer(256)
    result = macos_proc_pid_rusage_function()(
        pid, 2, ctypes.cast(buffer, ctypes.c_void_p)
    )
    if result != 0:
        error_number = ctypes.get_errno()
        if error_number == 3:
            raise ProcessLookupError(pid)
        raise OSError(error_number, os.strerror(error_number))
    timebase_numer, timebase_denom = macos_mach_timebase()
    return parse_macos_rusage_v2(
        buffer.raw,
        timebase_numer=timebase_numer,
        timebase_denom=timebase_denom,
    )


def parse_linux_status_rss_bytes(text: str) -> int:
    for line in text.splitlines():
        if not line.startswith("VmRSS:"):
            continue
        fields = line.split()
        if len(fields) != 3 or fields[2] != "kB":
            break
        try:
            return int(fields[1]) * 1024
        except ValueError:
            break
    raise BenchmarkError("Linux process status has no valid VmRSS value")


def parse_ps_cpu_seconds(text: str) -> float:
    value = text.strip()
    days = 0
    if "-" in value:
        day_text, value = value.split("-", 1)
        days = int(day_text)
    fields = value.split(":")
    if len(fields) == 2:
        hours = 0
        minutes, seconds = fields
    elif len(fields) == 3:
        hours, minutes, seconds = fields
    else:
        raise BenchmarkError("invalid ps CPU time value")
    return (
        days * 86400
        + int(hours) * 3600
        + int(minutes) * 60
        + float(seconds)
    )


def read_linux_process_stats(pid: int) -> ProcessStats:
    proc = pathlib.Path("/proc") / str(pid)
    status = (proc / "status").read_text(encoding="utf-8")
    stat_fields = (proc / "stat").read_text(encoding="utf-8").split()
    if len(stat_fields) < 15:
        raise BenchmarkError("Linux process stat record is incomplete")
    ticks = os.sysconf("SC_CLK_TCK")
    cpu_seconds = (int(stat_fields[13]) + int(stat_fields[14])) / ticks
    return ProcessStats(
        rss_bytes=parse_linux_status_rss_bytes(status),
        cpu_seconds=cpu_seconds,
    )


def read_macos_ps_process_stats(pid: int) -> ProcessStats:
    result = subprocess.run(
        ["ps", "-o", "rss=", "-o", "time=", "-p", str(pid)],
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        text=True,
        check=False,
        timeout=2.0,
    )
    if result.returncode != 0 or not result.stdout.strip():
        raise ProcessLookupError(pid)
    fields = result.stdout.split()
    if len(fields) != 2:
        raise BenchmarkError("unexpected macOS ps process-stat record")
    return ProcessStats(
        rss_bytes=parse_macos_ps_rss_bytes(fields[0]),
        cpu_seconds=parse_ps_cpu_seconds(fields[1]),
    )


def read_macos_process_stats(
    pid: int, allow_ps_fallback: bool = True
) -> ProcessStats:
    try:
        return read_macos_proc_pid_rusage(pid)
    except OSError:
        if not allow_ps_fallback:
            raise
        return read_macos_ps_process_stats(pid)


def read_process_stats(
    pid: int, allow_macos_ps_fallback: bool = True
) -> ProcessStats:
    if platform.system() == "Linux":
        return read_linux_process_stats(pid)
    if platform.system() == "Darwin":
        return read_macos_process_stats(pid, allow_ps_fallback=allow_macos_ps_fallback)
    raise BenchmarkError("process sampling is supported only on macOS and Linux")


class ProcessMonitor:
    def __init__(
        self,
        pid: int,
        sample_interval_seconds: float = RSS_SAMPLE_INTERVAL_SECONDS,
        allow_macos_ps_fallback: bool = False,
    ) -> None:
        if sample_interval_seconds <= 0:
            raise BenchmarkError("process sample interval must be positive")
        self.pid = pid
        self.sample_interval_seconds = sample_interval_seconds
        self.allow_macos_ps_fallback = allow_macos_ps_fallback
        self.samples: List[Dict[str, float]] = []
        self.sampling_error: Optional[str] = None
        self._stop = threading.Event()
        self._thread = threading.Thread(target=self._sample, daemon=False)
        self._started = False

    def _sample(self) -> None:
        started = time.perf_counter()
        while not self._stop.is_set():
            try:
                stats = read_process_stats(
                    self.pid,
                    allow_macos_ps_fallback=self.allow_macos_ps_fallback,
                )
            except ProcessLookupError:
                # Natural process completion races the monitor's last sample.
                # Completion status and protocol are validated by the caller.
                return
            except (BenchmarkError, OSError) as error:
                self.sampling_error = str(error)
                return
            else:
                self.samples.append(
                    {
                        "at_s": time.perf_counter() - started,
                        "rss_bytes": stats.rss_bytes,
                        "cpu_seconds": stats.cpu_seconds,
                    }
                )
            self._stop.wait(self.sample_interval_seconds)

    def start(self) -> None:
        if self._started:
            raise BenchmarkError("process monitor was started more than once")
        self._started = True
        self._thread.start()

    def stop(self) -> None:
        if not self._started:
            return
        self._stop.set()
        self._thread.join(timeout=2.0)
        if self._thread.is_alive():
            raise BenchmarkError("process monitor did not stop cleanly")

    @property
    def is_alive(self) -> bool:
        return self._started and self._thread.is_alive()

    @property
    def peak_rss_bytes(self) -> int:
        if not self.samples:
            raise BenchmarkError("process ended before RSS could be sampled")
        return int(max(sample["rss_bytes"] for sample in self.samples))

    @property
    def optional_peak_rss_bytes(self) -> Optional[int]:
        if not self.samples:
            return None
        return int(max(sample["rss_bytes"] for sample in self.samples))


def require_monitor_evidence(monitor: ProcessMonitor, benchmark: str) -> None:
    if monitor.sampling_error is not None:
        raise BenchmarkError(
            benchmark + " process sampling failed: " + monitor.sampling_error
        )
    if not monitor.samples:
        raise BenchmarkError(benchmark + " produced no process samples")


def controlled_runtime_environment(
    *, worker_count: Optional[int] = None
) -> Dict[str, str]:
    environment = dict(os.environ)
    environment.pop("AURORA_WORKERS", None)
    if worker_count is not None:
        if worker_count <= 0:
            raise BenchmarkError("controlled worker count must be positive")
        environment["AURORA_WORKERS"] = str(worker_count)
    return environment


def owned_process_group_exists(process_group_id: int) -> bool:
    return benchmark_process.process_group_exists(process_group_id)


def reap_owned_process_group(
    process: subprocess.Popen,
    benchmark: str,
    *,
    terminate_timeout_seconds: float = benchmark_process.TERMINATE_TIMEOUT_SECONDS,
    kill_timeout_seconds: float = benchmark_process.KILL_TIMEOUT_SECONDS,
) -> None:
    try:
        benchmark_process.reap_process_group(
            process,
            benchmark,
            terminate_timeout_seconds=terminate_timeout_seconds,
            kill_timeout_seconds=kill_timeout_seconds,
        )
    except benchmark_process.ProcessGroupCleanupError as error:
        raise BenchmarkError(str(error)) from error


def launch_owned_process(
    command: Sequence[str],
    **kwargs: object,
) -> subprocess.Popen:
    try:
        return benchmark_process.launch_process_group(command, **kwargs)
    except ValueError as error:
        raise BenchmarkError(str(error)) from error


def run_owned_process(
    command: Sequence[str],
    benchmark: str,
    *,
    timeout: Optional[float] = None,
    **kwargs: object,
) -> subprocess.CompletedProcess:
    """Run one workload while retaining descendant cleanup ownership."""

    process = launch_owned_process(command, **kwargs)
    try:
        stdout, stderr = process.communicate(timeout=timeout)
        result = subprocess.CompletedProcess(
            list(command), process.returncode, stdout, stderr
        )
    finally:
        reap_owned_process_group(process, benchmark)
    return result


def launch_binary(binary: pathlib.Path) -> subprocess.Popen:
    return launch_owned_process(
        [str(binary)],
        stdin=subprocess.DEVNULL,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        cwd=ROOT,
        env=controlled_runtime_environment(),
    )


def require_running_for(process: subprocess.Popen, seconds: float) -> None:
    deadline = time.monotonic() + seconds
    while True:
        if process.poll() is not None:
            raise BenchmarkError(
                "benchmark exited during the stable measurement window"
            )
        remaining = deadline - time.monotonic()
        if remaining <= 0:
            return
        time.sleep(min(0.05, remaining))


def collect_exact_completion(
    process: subprocess.Popen,
    benchmark: str,
    expected_stdout: bytes,
    timeout_seconds: float = NATURAL_COMPLETION_TIMEOUT_SECONDS,
) -> Dict[str, object]:
    try:
        stdout, stderr = process.communicate(timeout=timeout_seconds)
    except subprocess.TimeoutExpired as error:
        raise BenchmarkError(
            benchmark + " did not complete naturally before the timeout"
        ) from error
    if process.returncode != 0:
        raise BenchmarkError(
            benchmark + " benchmark exited with status " + str(process.returncode)
        )
    if stderr:
        raise BenchmarkError(
            benchmark + " benchmark wrote unexpected stderr: "
            + stderr.decode("utf-8", errors="replace")
        )
    if stdout != expected_stdout:
        raise BenchmarkError(
            benchmark
            + " completion stdout did not exactly match "
            + repr(expected_stdout)
            + "; got "
            + repr(stdout)
        )
    return {
        "returncode": process.returncode,
        "stdout": stdout.decode("ascii"),
        "stderr": "",
    }


def run_sleepers(binary: pathlib.Path, stable_seconds: float) -> Dict[str, object]:
    process = launch_binary(binary)
    monitor = ProcessMonitor(
        process.pid,
        sample_interval_seconds=RSS_SAMPLE_INTERVAL_SECONDS,
        allow_macos_ps_fallback=False,
    )
    try:
        baseline = read_process_phase(
            process, "BASELINE", "sleepers", ("10000",)
        )
        baseline_stats = read_process_stats(
            process.pid, allow_macos_ps_fallback=False
        )
        monitor.start()
        ready = read_process_ready(process, "sleepers", ("10000",))
        ready_stats = read_process_stats(
            process.pid, allow_macos_ps_fallback=False
        )
        started = time.perf_counter()
        completion = collect_exact_completion(
            process, "sleepers", b"DONE sleepers 10000\n"
        )
        ready_to_done = time.perf_counter() - started
        monitor.stop()
        if ready_to_done < stable_seconds:
            raise BenchmarkError(
                "sleepers completed too early: READY-to-DONE elapsed "
                + f"{ready_to_done:.6f}s, below the required "
                + f"{stable_seconds:.6f}s stable window"
            )
    finally:
        try:
            monitor.stop()
        finally:
            reap_owned_process_group(process, "sleepers")
    require_monitor_evidence(monitor, "sleepers")
    peak_rss_bytes = max(monitor.peak_rss_bytes, ready_stats.rss_bytes)
    return {
        "command": [str(binary)],
        "baseline": baseline.decode("ascii"),
        "ready": ready.decode("ascii"),
        "baseline_rss_bytes": baseline_stats.rss_bytes,
        "ready_rss_bytes": ready_stats.rss_bytes,
        "required_stable_window_s": stable_seconds,
        "ready_to_done_s": ready_to_done,
        "peak_rss_bytes": peak_rss_bytes,
        "incremental_peak_rss_bytes": max(
            0, peak_rss_bytes - baseline_stats.rss_bytes
        ),
        "process_samples": monitor.samples,
        "sample_interval_s": monitor.sample_interval_seconds,
        "sampling_error": monitor.sampling_error,
        "completion": completion,
    }


def run_idle(binary: pathlib.Path, stable_seconds: float) -> Dict[str, object]:
    process = launch_binary(binary)
    monitor = ProcessMonitor(
        process.pid,
        sample_interval_seconds=IDLE_SAMPLE_INTERVAL_SECONDS,
        allow_macos_ps_fallback=False,
    )
    try:
        ready = read_process_ready(process, "idle", ("10", "30000"))
        monitor.start()
        started_wall = time.perf_counter()
        started_stats = read_process_stats(process.pid)
        require_running_for(process, stable_seconds)
        finished_stats = read_process_stats(process.pid)
        elapsed_wall = time.perf_counter() - started_wall
        monitor.stop()
        completion = collect_exact_completion(process, "idle", b"DONE idle 10\n")
    finally:
        try:
            monitor.stop()
        finally:
            reap_owned_process_group(process, "idle")
    require_monitor_evidence(monitor, "idle")
    cpu_delta = max(0.0, finished_stats.cpu_seconds - started_stats.cpu_seconds)
    return {
        "command": [str(binary)],
        "ready": ready.decode("ascii"),
        "stable_window_s": elapsed_wall,
        "cpu_start_s": started_stats.cpu_seconds,
        "cpu_end_s": finished_stats.cpu_seconds,
        "cpu_delta_s": cpu_delta,
        "cpu_percent": cpu_delta / elapsed_wall * 100.0,
        "peak_rss_bytes": monitor.optional_peak_rss_bytes,
        "process_samples": monitor.samples,
        "sample_interval_s": monitor.sample_interval_seconds,
        "sampling_error": monitor.sampling_error,
        "completion": completion,
    }


def run_timers(binary: pathlib.Path) -> Dict[str, object]:
    process = launch_binary(binary)
    monitor = ProcessMonitor(
        process.pid,
        sample_interval_seconds=TIMER_SAMPLE_INTERVAL_SECONDS,
        allow_macos_ps_fallback=False,
    )
    try:
        ready = read_process_ready_line(process, "timers")
        ready_observation = parse_timer_ready_line(
            ready,
            expected_count=1000,
            expected_duration_ms=10,
        )
        monitor.start()
        try:
            stdout, stderr = process.communicate(timeout=TIMER_TIMEOUT_SECONDS)
        except subprocess.TimeoutExpired as error:
            raise BenchmarkError("timer benchmark timed out") from error
    finally:
        try:
            monitor.stop()
        finally:
            reap_owned_process_group(process, "timers")
    require_monitor_evidence(monitor, "timers")
    if process.returncode != 0:
        raise BenchmarkError(
            "timer benchmark exited with status " + str(process.returncode)
        )
    if stderr:
        raise BenchmarkError(
            "timer benchmark wrote unexpected stderr: "
            + stderr.decode("utf-8", errors="replace")
        )
    samples = parse_timer_samples(io.BytesIO(stdout), expected_count=1000)
    arm_span = float(ready_observation["arm_span_ms"])
    overshoots = [sample["overshoot_ms"] for sample in samples]
    return {
        "command": [str(binary)],
        "ready": ready.decode("ascii"),
        "ready_observation": ready_observation,
        "arm_span_ms": arm_span,
        "arm_span_limit_ms": TIMER_ARM_SPAN_LIMIT_MS,
        "arm_span_valid": arm_span <= TIMER_ARM_SPAN_LIMIT_MS,
        "samples": samples,
        "summary": timer_summary(overshoots),
        "peak_rss_bytes": monitor.optional_peak_rss_bytes,
        "process_samples": monitor.samples,
        "sample_interval_s": monitor.sample_interval_seconds,
        "sampling_error": monitor.sampling_error,
    }


def run_massive(binary: pathlib.Path) -> Dict[str, object]:
    process = launch_binary(binary)
    monitor = ProcessMonitor(
        process.pid,
        sample_interval_seconds=RSS_SAMPLE_INTERVAL_SECONDS,
        allow_macos_ps_fallback=False,
    )
    try:
        baseline = read_process_phase(
            process,
            "BASELINE",
            "massive",
            (
                str(MASSIVE_SLEEPER_COUNT),
                str(MASSIVE_TIMER_COUNT),
                str(MASSIVE_TIMER_DURATION_MS),
            ),
        )
        baseline_stats = read_process_stats(
            process.pid, allow_macos_ps_fallback=False
        )
        monitor.start()
        ready = read_process_ready_line(
            process,
            "massive",
            timeout_seconds=MASSIVE_READY_TIMEOUT_SECONDS,
        )
        ready_observation = parse_massive_ready_line(
            ready,
            expected_sleepers=MASSIVE_SLEEPER_COUNT,
            expected_timer_count=MASSIVE_TIMER_COUNT,
            expected_duration_ms=MASSIVE_TIMER_DURATION_MS,
        )
        ready_stats = read_process_stats(
            process.pid, allow_macos_ps_fallback=False
        )
        try:
            stdout, stderr = process.communicate(
                timeout=MASSIVE_COMPLETION_TIMEOUT_SECONDS
            )
        except subprocess.TimeoutExpired as error:
            raise BenchmarkError("massive benchmark timed out after READY") from error
    finally:
        try:
            monitor.stop()
        finally:
            reap_owned_process_group(process, "massive")
    require_monitor_evidence(monitor, "massive")
    if process.returncode != 0:
        raise BenchmarkError(
            "massive benchmark exited with status " + str(process.returncode)
        )
    if stderr:
        raise BenchmarkError(
            "massive benchmark wrote unexpected stderr: "
            + stderr.decode("utf-8", errors="replace")
        )
    samples = parse_massive_samples(
        io.BytesIO(stdout),
        expected_sleepers=MASSIVE_SLEEPER_COUNT,
        expected_timer_count=MASSIVE_TIMER_COUNT,
    )
    peak_rss_bytes = max(monitor.peak_rss_bytes, ready_stats.rss_bytes)
    incremental_peak_rss_bytes = max(
        0, peak_rss_bytes - baseline_stats.rss_bytes
    )
    arm_span_ms = float(ready_observation["arm_span_ms"])
    overshoots = [sample["overshoot_ms"] for sample in samples]
    return {
        "command": [str(binary)],
        "baseline": baseline.decode("ascii"),
        "ready": ready.decode("ascii"),
        "ready_observation": ready_observation,
        "baseline_rss_bytes": baseline_stats.rss_bytes,
        "ready_rss_bytes": ready_stats.rss_bytes,
        "peak_rss_bytes": peak_rss_bytes,
        "incremental_peak_rss_bytes": incremental_peak_rss_bytes,
        "arm_span_ms": arm_span_ms,
        "arm_span_limit_ms": TIMER_ARM_SPAN_LIMIT_MS,
        "arm_span_valid": arm_span_ms <= TIMER_ARM_SPAN_LIMIT_MS,
        "samples": samples,
        "summary": timer_summary(overshoots),
        "process_samples": monitor.samples,
        "sample_interval_s": monitor.sample_interval_seconds,
        "sampling_error": monitor.sampling_error,
        "returncode": process.returncode,
    }


def run_v6_probe_once(
    binary: pathlib.Path, expected_stdout: bytes
) -> Dict[str, object]:
    started = time.perf_counter()
    result = run_owned_process(
        [str(binary)],
        binary.name,
        stdin=subprocess.DEVNULL,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        cwd=ROOT,
        env=controlled_runtime_environment(),
    )
    elapsed = time.perf_counter() - started
    if result.returncode != 0:
        raise BenchmarkError(
            binary.name + " exited with status " + str(result.returncode)
        )
    if result.stderr:
        raise BenchmarkError(binary.name + " wrote unexpected stderr")
    if result.stdout != expected_stdout:
        raise BenchmarkError(
            binary.name
            + " stdout did not exactly match "
            + repr(expected_stdout)
        )
    return {
        "command": [str(binary)],
        "elapsed_s": elapsed,
        "stdout": result.stdout.decode("ascii"),
        "returncode": result.returncode,
    }


def run_v6_once(binary: pathlib.Path) -> Dict[str, object]:
    return run_v6_probe_once(binary, EXPECTED_V6_STDOUT)


def run_v6_startup_once(binary: pathlib.Path) -> Dict[str, object]:
    return run_v6_probe_once(binary, EXPECTED_V6_STARTUP_STDOUT)


def run_starvation(binary: pathlib.Path) -> Dict[str, object]:
    started = time.perf_counter()
    try:
        result = run_owned_process(
            [str(binary)],
            "starvation",
            stdin=subprocess.DEVNULL,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            cwd=ROOT,
            env=controlled_runtime_environment(worker_count=1),
            timeout=STARVATION_TIMEOUT_SECONDS,
        )
    except subprocess.TimeoutExpired as error:
        raise BenchmarkError("starvation benchmark timed out") from error
    elapsed = time.perf_counter() - started
    if result.returncode != 0:
        raise BenchmarkError(
            "starvation benchmark exited with status " + str(result.returncode)
        )
    if result.stderr:
        raise BenchmarkError(
            "starvation benchmark wrote unexpected stderr: "
            + result.stderr.decode("utf-8", errors="replace")
        )
    observation = parse_starvation_output(
        result.stdout, expected_sleep_ms=STARVATION_SLEEP_MS
    )
    return {
        "command": [str(binary)],
        "environment": {"AURORA_WORKERS": "1"},
        "elapsed_s": elapsed,
        **observation,
        "stdout": result.stdout.decode("ascii"),
        "returncode": result.returncode,
    }


def run_multicore_once(
    binary: pathlib.Path, *, tasks: int
) -> Dict[str, object]:
    if tasks not in MULTICORE_TASK_COUNTS:
        raise BenchmarkError("multicore task count must be 1 or 4")
    environment = controlled_runtime_environment(
        worker_count=MULTICORE_WORKERS
    )
    process = launch_owned_process(
        [str(binary), str(tasks)],
        stdin=subprocess.PIPE,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        cwd=ROOT,
        env=environment,
    )
    monitor = ProcessMonitor(
        process.pid,
        sample_interval_seconds=MULTICORE_SAMPLE_INTERVAL_SECONDS,
        allow_macos_ps_fallback=False,
    )
    try:
        ready = read_process_ready_line(
            process,
            "multicore",
            timeout_seconds=READY_TIMEOUT_SECONDS,
        )
        ready_observation = parse_multicore_ready_line(
            ready,
            expected_tasks=tasks,
            expected_iterations=MULTICORE_ITERATIONS,
            expected_multiplier=MULTICORE_MULTIPLIER,
            expected_modulus=MULTICORE_MODULUS,
        )
        expected_checksum = park_miller_checksum(
            tasks=tasks,
            iterations=MULTICORE_ITERATIONS,
        )
        started_stats = read_process_stats(
            process.pid, allow_macos_ps_fallback=False
        )
        monitor.start()
        started = time.perf_counter()
        assert process.stdin is not None
        process.stdin.write(b"GO multicore\n")
        process.stdin.flush()
        done = read_process_ready_line(
            process,
            "multicore",
            timeout_seconds=MULTICORE_TIMEOUT_SECONDS,
        )
        elapsed = time.perf_counter() - started
        finished_stats = read_process_stats(
            process.pid, allow_macos_ps_fallback=False
        )
        done_observation = parse_multicore_done_line(
            done,
            expected_tasks=tasks,
            expected_checksum=expected_checksum,
        )
        monitor.stop()

        process.stdin.write(b"ACK multicore\n")
        process.stdin.flush()
        process.stdin.close()
        process.stdin = None
        try:
            stdout, stderr = process.communicate(timeout=5.0)
        except subprocess.TimeoutExpired as error:
            raise BenchmarkError(
                "multicore benchmark did not exit after ACK"
            ) from error
    finally:
        try:
            monitor.stop()
        finally:
            reap_owned_process_group(process, "multicore")
    require_monitor_evidence(monitor, "multicore")
    if process.returncode != 0:
        raise BenchmarkError(
            "multicore benchmark exited with status " + str(process.returncode)
        )
    if stderr:
        raise BenchmarkError(
            "multicore benchmark wrote unexpected stderr: "
            + stderr.decode("utf-8", errors="replace")
        )
    if stdout:
        raise BenchmarkError(
            "multicore benchmark emitted trailing output after DONE: "
            + repr(stdout)
        )
    cpu_delta = max(
        0.0, finished_stats.cpu_seconds - started_stats.cpu_seconds
    )
    return {
        "command": [str(binary), str(tasks)],
        "environment": {"AURORA_WORKERS": str(MULTICORE_WORKERS)},
        "ready": ready.decode("ascii"),
        "ready_observation": ready_observation,
        "go": "GO multicore\n",
        "done": done.decode("ascii"),
        "done_observation": done_observation,
        "ack": "ACK multicore\n",
        "elapsed_s": elapsed,
        "cpu_start_s": started_stats.cpu_seconds,
        "cpu_end_s": finished_stats.cpu_seconds,
        "process_cpu_s": cpu_delta,
        "process_cpu_percent": cpu_delta / elapsed * 100.0,
        "process_samples": monitor.samples,
        "sample_interval_s": monitor.sample_interval_seconds,
        "sampling_error": monitor.sampling_error,
        "completion": {
            "returncode": process.returncode,
            "stdout": "",
            "stderr": "",
        },
    }


def run_multicore_benchmark(
    binary: pathlib.Path, *, repeats: int
) -> Dict[str, object]:
    if repeats < MULTICORE_MIN_REPEATS or repeats % 2 == 0:
        raise BenchmarkError(
            "multicore repeats must be odd and at least "
            + str(MULTICORE_MIN_REPEATS)
        )
    warmups = {
        "1": run_multicore_once(binary, tasks=1),
        "4": run_multicore_once(binary, tasks=4),
    }
    pairs: List[Dict[str, object]] = []
    for repeat in range(repeats):
        order = (1, 4) if repeat % 2 == 0 else (4, 1)
        runs: Dict[str, Dict[str, object]] = {}
        for tasks in order:
            runs[str(tasks)] = run_multicore_once(binary, tasks=tasks)
        one_elapsed = float(runs["1"]["elapsed_s"])
        four_elapsed = float(runs["4"]["elapsed_s"])
        ratio = four_elapsed / one_elapsed
        pairs.append(
            {
                "repeat": repeat,
                "order": list(order),
                "runs": runs,
                "paired_ratio": ratio,
                "pair_passed": ratio <= MULTICORE_GATE_RATIO,
            }
        )
    return {"warmups": warmups, "pairs": pairs}


def build_workloads(
    aura: pathlib.Path, output_directory: pathlib.Path
) -> Tuple[Dict[str, pathlib.Path], List[Dict[str, object]]]:
    binaries: Dict[str, pathlib.Path] = {}
    records: List[Dict[str, object]] = []
    for name, source in WORKLOADS.items():
        if not source.is_file():
            raise BenchmarkError("missing benchmark workload " + str(source))
        output = output_directory / name
        command = [
            str(aura),
            "build",
            "--backend",
            "direct",
            "-o",
            str(output),
            str(source),
        ]
        result = subprocess.run(
            command,
            cwd=ROOT,
            stdin=subprocess.DEVNULL,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            shell=False,
            check=False,
        )
        if result.returncode != 0:
            raise BenchmarkError(
                "failed to build "
                + str(source)
                + ":\n"
                + result.stderr.decode("utf-8", errors="replace")
            )
        if not output.is_file() or not os.access(str(output), os.X_OK):
            raise BenchmarkError("aura build did not create executable " + str(output))
        binaries[name] = output
        records.append(
            {
                "name": name,
                "command": command,
                "source": str(source),
                "source_sha256": sha256_file(source),
                "binary": str(output),
                "binary_sha256": sha256_file(output),
                "stdout": result.stdout.decode("utf-8", errors="strict"),
                "stderr": result.stderr.decode("utf-8", errors="strict"),
                "returncode": result.returncode,
            }
        )
    return binaries, records


def process_cwd(pid: int) -> Optional[pathlib.Path]:
    if platform.system() == "Linux":
        try:
            return pathlib.Path(os.readlink("/proc/" + str(pid) + "/cwd"))
        except (FileNotFoundError, PermissionError, OSError):
            return None
    if platform.system() == "Darwin" and shutil.which("lsof"):
        result = subprocess.run(
            ["lsof", "-a", "-p", str(pid), "-d", "cwd", "-Fn"],
            stdout=subprocess.PIPE,
            stderr=subprocess.DEVNULL,
            text=True,
            check=False,
        )
        for line in result.stdout.splitlines():
            if line.startswith("n"):
                return pathlib.Path(line[1:])
    return None


def process_rows() -> List[ProcessRow]:
    result = subprocess.run(
        ["ps", "-axo", "pid=,comm=,args="],
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        text=True,
        check=True,
    )
    rows: List[ProcessRow] = []
    for line in result.stdout.splitlines():
        fields = line.strip().split(None, 2)
        if len(fields) < 2:
            continue
        pid = int(fields[0])
        command = fields[1]
        arguments = fields[2] if len(fields) == 3 else ""
        executable = pathlib.Path(command).name
        argument_executable = (
            pathlib.Path(arguments.split(None, 1)[0]).name
            if arguments
            else ""
        )
        needs_cwd = executable in {"cargo", "rustc", "aura"} or (
            argument_executable in {"cargo", "rustc", "aura"}
        )
        rows.append(
            ProcessRow(
                pid=pid,
                command=command,
                arguments=arguments,
                cwd=process_cwd(pid) if needs_cwd else None,
            )
        )
    return rows


def find_competing_processes(
    root: pathlib.Path,
    rows: Optional[Iterable[ProcessRow]] = None,
    ignored_pids: Optional[Set[int]] = None,
) -> List[ProcessRow]:
    ignored = ignored_pids or set()
    candidates = []
    for row in process_rows() if rows is None else rows:
        if row.pid in ignored:
            continue
        executable = pathlib.Path(row.command).name
        argument_executable = (
            pathlib.Path(row.arguments.split(None, 1)[0]).name
            if row.arguments
            else ""
        )
        if executable not in {"cargo", "rustc", "aura"} and argument_executable not in {
            "cargo",
            "rustc",
            "aura",
        }:
            continue
        in_repository = row.cwd is not None and is_within(row.cwd, root)
        mentions_repository = str(root.resolve()) in row.arguments
        if in_repository or mentions_repository:
            candidates.append(row)
    return sorted(candidates, key=lambda row: row.pid)


def process_row_record(row: ProcessRow) -> Dict[str, object]:
    return {
        "pid": row.pid,
        "command": row.command,
        "arguments": row.arguments,
        "cwd": str(row.cwd) if row.cwd else None,
    }


def require_quiet_process_check(
    competitors: Sequence[ProcessRow],
    *,
    allow_competing_processes: bool,
    phase: str,
) -> None:
    if not competitors or allow_competing_processes:
        return
    details = ", ".join(
        str(row.pid) + ":" + pathlib.Path(row.command).name
        for row in competitors
    )
    raise BenchmarkError(
        "competing Aurora-repo build processes detected "
        + phase
        + " ("
        + details
        + "); wait for a quiet machine or pass --allow-competing-processes"
    )


def benchmark_is_contractual(
    allow_competing_processes: bool,
    process_checks: Sequence[Sequence[ProcessRow]],
    *,
    host: Optional[Dict[str, object]] = None,
    repository: Optional[Dict[str, object]] = None,
) -> bool:
    return not benchmark_noncontractual_reasons(
        allow_competing_processes,
        process_checks,
        host=host,
        repository=repository,
    )


def benchmark_noncontractual_reasons(
    allow_competing_processes: bool,
    process_checks: Sequence[Sequence[ProcessRow]],
    *,
    host: Optional[Dict[str, object]] = None,
    repository: Optional[Dict[str, object]] = None,
) -> List[str]:
    reasons = []
    if allow_competing_processes:
        reasons.append("the competing-process override was enabled")
    if any(process_checks):
        reasons.append("competing Aurora-repository processes were observed")
    if host is not None and host.get("hardware_model") != "Mac14,9":
        reasons.append(
            "host hardware model is not the contractual Mac14,9 baseline"
        )
    if repository is not None and repository.get("dirty_files"):
        reasons.append("repository worktree was dirty")
    return reasons


def validate_options(options: Options, root: pathlib.Path = ROOT) -> None:
    if not options.label.strip():
        raise BenchmarkError("--label must not be empty")
    if options.repeats <= 0 or options.timer_repeats <= 0 or options.v6_repeats <= 0:
        raise BenchmarkError("all repeat counts must be positive")
    if (
        options.multicore_repeats < MULTICORE_MIN_REPEATS
        or options.multicore_repeats % 2 == 0
    ):
        raise BenchmarkError(
            "--multicore-repeats must be odd and at least "
            + str(MULTICORE_MIN_REPEATS)
        )
    if (
        not math.isfinite(options.idle_seconds)
        or options.idle_seconds <= 0
        or options.idle_seconds > 30
    ):
        raise BenchmarkError(
            "--idle-seconds must be a positive finite value no greater than "
            "the workloads' advertised 30-second stable window"
        )
    aura = options.aura.resolve()
    if "debug" in aura.parts:
        raise BenchmarkError("refusing a debug Aura binary; build and pass release Aura")
    if not aura.is_file() or not os.access(str(aura), os.X_OK):
        raise BenchmarkError("--aura must name an executable file")
    json_path = options.json_path.resolve()
    if is_within(json_path, root / "target"):
        raise BenchmarkError("--json must be stored outside target/")
    if json_path.exists() and json_path.is_dir():
        raise BenchmarkError("--json must name a file, not a directory")


def qualify_aura_binary(
    aura: pathlib.Path, *, root: pathlib.Path = ROOT
) -> Dict[str, object]:
    expected = (root / "target/release/aura").resolve()
    resolved = aura.resolve()
    if resolved != expected:
        raise BenchmarkError(
            "--aura must name this checkout's target/release/aura so its "
            "source identity can be qualified"
        )
    command = [
        "cargo",
        "build",
        "--release",
        "--locked",
        "-p",
        "aura",
        "--target-dir",
        str((root / "target").resolve()),
    ]
    result = subprocess.run(
        command,
        cwd=root,
        stdin=subprocess.DEVNULL,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        shell=False,
        check=False,
    )
    if result.returncode != 0:
        raise BenchmarkError(
            "failed to qualify Aura with a fresh checkout build:\n"
            + result.stderr.decode("utf-8", errors="replace")
        )
    if not expected.is_file() or not os.access(str(expected), os.X_OK):
        raise BenchmarkError(
            "fresh cargo build did not create executable " + str(expected)
        )
    return {
        "path": str(expected),
        "sha256": sha256_file(expected),
        "fresh_cargo_build": True,
        "command": command,
        "stdout": result.stdout.decode("utf-8", errors="strict"),
        "stderr": result.stderr.decode("utf-8", errors="strict"),
        "returncode": result.returncode,
    }


def aura_version(aura: pathlib.Path) -> str:
    result = subprocess.run(
        [str(aura), "version"],
        cwd=ROOT,
        stdin=subprocess.DEVNULL,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        text=True,
        shell=False,
        check=False,
    )
    if (
        result.returncode != 0
        or result.stderr
        or not result.stdout.startswith("aura ")
        or not result.stdout.endswith("\n")
    ):
        raise BenchmarkError("Aura version probe did not return the exact CLI contract")
    return result.stdout.rstrip("\n")


def command_output(command: Sequence[str]) -> Optional[str]:
    try:
        result = subprocess.run(
            list(command),
            stdout=subprocess.PIPE,
            stderr=subprocess.DEVNULL,
            text=True,
            check=False,
        )
    except OSError:
        return None
    return result.stdout.strip() if result.returncode == 0 else None


def hardware_record() -> Dict[str, object]:
    system = platform.system()
    cpu_model = None
    physical_cores = None
    memory_bytes = None
    if system == "Darwin":
        hardware_model = command_output(["sysctl", "-n", "hw.model"])
        cpu_model = command_output(["sysctl", "-n", "machdep.cpu.brand_string"])
        if cpu_model is None:
            cpu_model = hardware_model
        physical = command_output(["sysctl", "-n", "hw.physicalcpu"])
        memory = command_output(["sysctl", "-n", "hw.memsize"])
        physical_cores = int(physical) if physical else None
        memory_bytes = int(memory) if memory else None
    else:
        hardware_model = None
    if system == "Linux":
        cpuinfo = pathlib.Path("/proc/cpuinfo").read_text(
            encoding="utf-8", errors="replace"
        )
        for line in cpuinfo.splitlines():
            if line.lower().startswith("model name"):
                cpu_model = line.split(":", 1)[1].strip()
                break
        meminfo = pathlib.Path("/proc/meminfo").read_text(encoding="utf-8")
        for line in meminfo.splitlines():
            if line.startswith("MemTotal:"):
                memory_bytes = int(line.split()[1]) * 1024
                break
    uname = platform.uname()
    affinity_cpus = None
    if hasattr(os, "sched_getaffinity"):
        try:
            affinity_cpus = len(os.sched_getaffinity(0))
        except OSError:
            affinity_cpus = None
    return {
        "system": system,
        "release": uname.release,
        "version": uname.version,
        "machine": uname.machine,
        "processor": uname.processor,
        "hardware_model": hardware_model,
        "cpu_model": cpu_model,
        "logical_cpus": os.cpu_count(),
        "physical_cores": physical_cores,
        "affinity_cpus": affinity_cpus,
        "memory_bytes": memory_bytes,
        "python": platform.python_version(),
    }


def repository_record(root: pathlib.Path) -> Dict[str, object]:
    commit = command_output(["git", "-C", str(root), "rev-parse", "HEAD"])
    status = subprocess.run(
        ["git", "-C", str(root), "status", "--porcelain=v1", "-z"],
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        check=True,
    ).stdout
    return {
        "root": str(root),
        "commit": commit,
        "dirty_files": [
            record.decode("utf-8", errors="surrogateescape")
            for record in status.split(b"\0")
            if record
        ],
    }


def compiler_runtime_inputs(aura: pathlib.Path) -> Dict[str, object]:
    artifacts = []
    release = ROOT / "target/release"
    if release.is_dir():
        candidates = list(release.glob("libaurora_compiler*.a"))
        candidates.extend((release / "deps").glob("libaurora_compiler*.a"))
        for candidate in sorted(set(candidates)):
            artifacts.append(
                {
                    "path": str(candidate),
                    "size_bytes": candidate.stat().st_size,
                    "sha256": sha256_file(candidate),
                }
            )
    return {
        "aura": {
            "path": str(aura),
            "version": aura_version(aura),
            "size_bytes": aura.stat().st_size,
            "sha256": sha256_file(aura),
        },
        "runtime_archives": artifacts,
        "environment": {
            name: os.environ.get(name)
            for name in ("CC", "CARGO", "RUSTC", "AURORA_NATIVE_CACHE_DIR")
        },
    }


def v6_startup_loop_summary(
    startup_durations: Sequence[float],
    whole_process_durations: Dict[str, Sequence[float]],
) -> Dict[str, object]:
    if not startup_durations:
        raise BenchmarkError("V6 startup split requires at least one repetition")
    loop_estimates: Dict[str, Dict[str, object]] = {}
    for width, durations in whole_process_durations.items():
        if len(durations) != len(startup_durations):
            raise BenchmarkError(
                "V6 startup and " + width + " samples must be paired"
            )
        raw_estimates = [
            float(whole) - float(startup)
            for startup, whole in zip(startup_durations, durations)
        ]
        invalid = [
            repeat
            for repeat, estimate in enumerate(raw_estimates)
            if estimate < 0.0
        ]
        valid_repetitions = [
            repeat
            for repeat, estimate in enumerate(raw_estimates)
            if estimate >= 0.0
        ]
        valid_estimates = [
            raw_estimates[repeat] for repeat in valid_repetitions
        ]
        if not valid_estimates:
            raise BenchmarkError(
                "all paired V6 " + width + " loop estimates were negative"
            )
        loop_estimates[width] = {
            **duration_summary(valid_estimates),
            "samples_s": valid_estimates,
            "valid_repetitions": valid_repetitions,
            "invalid_negative_pair_repetitions": invalid,
        }
    return {
        "method": (
            "paired whole-process duration minus the same repetition's "
            "startup duration"
        ),
        "startup": duration_summary(startup_durations),
        "loop_estimate": loop_estimates,
    }


def run_v6_benchmark(
    startup_binary: pathlib.Path,
    int32_binary: pathlib.Path,
    int64_binary: pathlib.Path,
    repeats: int,
) -> Dict[str, object]:
    warmups = {
        "startup": run_v6_startup_once(startup_binary),
        "int32": run_v6_once(int32_binary),
        "int64": run_v6_once(int64_binary),
    }
    binaries = {
        "startup": startup_binary,
        "int32": int32_binary,
        "int64": int64_binary,
    }
    runs: List[Dict[str, object]] = []
    startup_durations: List[float] = []
    durations: Dict[str, List[float]] = {"int32": [], "int64": []}
    for repeat in range(repeats):
        probes = ("startup", "int32", "int64")
        offset = repeat % len(probes)
        order = probes[offset:] + probes[:offset]
        for workload in order:
            observation = (
                run_v6_startup_once(binaries[workload])
                if workload == "startup"
                else run_v6_once(binaries[workload])
            )
            elapsed = float(observation["elapsed_s"])
            if workload == "startup":
                startup_durations.append(elapsed)
            else:
                durations[workload].append(elapsed)
            runs.append(
                {
                    "repeat": repeat,
                    "workload": workload,
                    "order": list(order),
                    **observation,
                }
            )
    return {
        "warmups": warmups,
        "runs": runs,
        "summary": {
            width: duration_summary(values) for width, values in durations.items()
        },
        "startup_vs_loop": v6_startup_loop_summary(
            startup_durations, durations
        ),
    }


def parse_options(argv: Optional[Sequence[str]] = None) -> Options:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--label", required=True)
    parser.add_argument("--aura", type=pathlib.Path, required=True)
    parser.add_argument("--repeats", type=int, default=3)
    parser.add_argument("--timer-repeats", type=int, default=3)
    parser.add_argument("--v6-repeats", type=int, default=5)
    parser.add_argument(
        "--multicore-repeats",
        type=int,
        default=MULTICORE_DEFAULT_REPEATS,
    )
    parser.add_argument("--idle-seconds", type=float, default=30.0)
    parser.add_argument("--json", type=pathlib.Path, required=True, dest="json_path")
    parser.add_argument(
        "--allow-competing-processes",
        action="store_true",
        help="run despite detected repo cargo/rustc/aura processes",
    )
    arguments = parser.parse_args(argv)
    return Options(
        label=arguments.label,
        aura=arguments.aura,
        repeats=arguments.repeats,
        timer_repeats=arguments.timer_repeats,
        v6_repeats=arguments.v6_repeats,
        multicore_repeats=arguments.multicore_repeats,
        idle_seconds=arguments.idle_seconds,
        json_path=arguments.json_path,
        allow_competing_processes=arguments.allow_competing_processes,
    )


def write_json_atomic(path: pathlib.Path, report: Dict[str, object]) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    with tempfile.NamedTemporaryFile(
        mode="w",
        encoding="utf-8",
        dir=path.parent,
        prefix=path.name + ".",
        suffix=".tmp",
        delete=False,
    ) as stream:
        temporary = pathlib.Path(stream.name)
        json.dump(report, stream, indent=2, sort_keys=True, allow_nan=False)
        stream.write("\n")
    os.replace(str(temporary), str(path))


def execute(options: Options) -> Dict[str, object]:
    validate_options(options)
    aura = options.aura.resolve()
    before_build_competitors = find_competing_processes(
        ROOT, ignored_pids={os.getpid(), os.getppid()}
    )
    require_quiet_process_check(
        before_build_competitors,
        allow_competing_processes=options.allow_competing_processes,
        phase="before workload builds",
    )

    aura_qualification = qualify_aura_binary(aura)
    host = hardware_record()
    repository = repository_record(ROOT)
    report: Dict[str, object] = {
        "schema_version": REPORT_SCHEMA_VERSION,
        "label": options.label,
        "generated_at": datetime.now(timezone.utc).isoformat(),
        "runner_command": [sys.executable, str(pathlib.Path(__file__).resolve()), *sys.argv[1:]],
        "host": host,
        "repository": repository,
        "aura_qualification": aura_qualification,
        "parameters": {
            "repeats": options.repeats,
            "timer_repeats": options.timer_repeats,
            "v6_repeats": options.v6_repeats,
            "multicore_repeats": options.multicore_repeats,
            "idle_seconds": options.idle_seconds,
            "ready_timeout_seconds": READY_TIMEOUT_SECONDS,
            "massive_ready_timeout_seconds": MASSIVE_READY_TIMEOUT_SECONDS,
            "timer_timeout_seconds": TIMER_TIMEOUT_SECONDS,
            "massive_completion_timeout_seconds": (
                MASSIVE_COMPLETION_TIMEOUT_SECONDS
            ),
            "starvation_timeout_seconds": STARVATION_TIMEOUT_SECONDS,
            "natural_completion_timeout_seconds": NATURAL_COMPLETION_TIMEOUT_SECONDS,
            "timer_arm_span_limit_ms": TIMER_ARM_SPAN_LIMIT_MS,
            "rss_sample_interval_seconds": RSS_SAMPLE_INTERVAL_SECONDS,
            "idle_sample_interval_seconds": IDLE_SAMPLE_INTERVAL_SECONDS,
            "timer_sample_interval_seconds": TIMER_SAMPLE_INTERVAL_SECONDS,
            "multicore_sample_interval_seconds": (
                MULTICORE_SAMPLE_INTERVAL_SECONDS
            ),
            "multicore_iterations": MULTICORE_ITERATIONS,
            "multicore_workers": MULTICORE_WORKERS,
            "multicore_min_signal_seconds": MULTICORE_MIN_SIGNAL_SECONDS,
            "multicore_max_relative_mad": MULTICORE_MAX_RELATIVE_MAD,
            "multicore_min_four_task_cpu_percent": (
                MULTICORE_MIN_FOUR_TASK_CPU_PERCENT
            ),
            "allow_competing_processes": options.allow_competing_processes,
        },
        "quiet_process_checks": {
            "before_build": [
                process_row_record(row) for row in before_build_competitors
            ],
            "before_timing": None,
        },
    }

    with tempfile.TemporaryDirectory(
        prefix="aurora-scalable-runtime-bench-"
    ) as directory:
        binaries, build_records = build_workloads(aura, pathlib.Path(directory))
        report["builds"] = build_records
        # A build may establish the release runtime archive, so capture the
        # compiler/runtime identities after all builds and before any timing.
        report["inputs"] = compiler_runtime_inputs(aura)
        before_timing_competitors = find_competing_processes(
            ROOT, ignored_pids={os.getpid(), os.getppid()}
        )
        report["quiet_process_checks"]["before_timing"] = [  # type: ignore[index]
            process_row_record(row) for row in before_timing_competitors
        ]
        require_quiet_process_check(
            before_timing_competitors,
            allow_competing_processes=options.allow_competing_processes,
            phase="immediately before timing",
        )
        report["contractual"] = benchmark_is_contractual(
            options.allow_competing_processes,
            (before_build_competitors, before_timing_competitors),
            host=host,
            repository=repository,
        )
        report["noncontractual_reasons"] = benchmark_noncontractual_reasons(
            options.allow_competing_processes,
            (before_build_competitors, before_timing_competitors),
            host=host,
            repository=repository,
        )

        sleepers = [
            run_sleepers(binaries["sleepers"], options.idle_seconds)
            for _ in range(options.repeats)
        ]
        massive = [
            run_massive(binaries["massive"]) for _ in range(options.repeats)
        ]
        timers = [
            run_timers(binaries["timers"]) for _ in range(options.timer_repeats)
        ]
        idle = [
            run_idle(binaries["idle"], options.idle_seconds)
            for _ in range(options.repeats)
        ]
        starvation = [
            run_starvation(binaries["starvation"]) for _ in range(options.repeats)
        ]
        v6 = run_v6_benchmark(
            binaries["startup"],
            binaries["int32"],
            binaries["int64"],
            options.v6_repeats,
        )
        multicore = run_multicore_benchmark(
            binaries["multicore"],
            repeats=options.multicore_repeats,
        )

    sleeper_rss_gate = rss_gate_summary(
        sleepers,
        limit_bytes=SLEEPER_RSS_LIMIT_BYTES,
    )
    massive_timer_gate = timer_gate_summary(massive)
    massive_gate = massive_gate_summary(massive)
    timer_overshoots = [
        float(sample["overshoot_ms"])
        for run in timers
        for sample in run["samples"]
    ]
    timers_summary = timer_summary(timer_overshoots)
    timer_gate = timer_gate_summary(timers)
    worst_valid_timer_p99 = timer_gate["worst_valid_run_p99_ms"]
    idle_cpu_max = max(float(run["cpu_percent"]) for run in idle)
    all_arm_spans_valid = all(bool(run["arm_span_valid"]) for run in timers)
    starvation_gate = starvation_gate_summary(starvation)
    multicore_gate = multicore_gate_summary(
        multicore["pairs"],
        host=host,
    )
    gates = {
        "sleepers_peak_rss": sleeper_rss_gate,
        "massive_concurrency": massive_gate,
        "timer_p99_overshoot": {
            "observed_ms": worst_valid_timer_p99,
            "limit_ms": TIMER_P99_LIMIT_MS,
            "operator": "<=",
            "valid_run_indexes": timer_gate["valid_run_indexes"],
            "invalid_overlap_runs": timer_gate["invalid_overlap_runs"],
            "passed": (
                worst_valid_timer_p99 is not None
                and float(worst_valid_timer_p99) <= TIMER_P99_LIMIT_MS
            ),
        },
        "timer_arm_span": {
            "observed_max_ms": max(float(run["arm_span_ms"]) for run in timers),
            "limit_ms": TIMER_ARM_SPAN_LIMIT_MS,
            "operator": "<=",
            "invalid_overlap_runs": timer_gate["invalid_overlap_runs"],
            "passed": all_arm_spans_valid,
        },
        "idle_cpu": {
            "observed_max_percent": idle_cpu_max,
            "limit_percent": IDLE_CPU_LIMIT_PERCENT,
            "operator": "<",
            "passed": idle_cpu_max < IDLE_CPU_LIMIT_PERCENT,
        },
        "starvation_latency": starvation_gate,
        "multicore_scaling": multicore_gate,
    }
    report["benchmarks"] = {
        "sleepers": {"runs": sleepers},
        "massive": {
            "runs": massive,
            "gate_summary": massive_timer_gate,
        },
        "timers": {
            "runs": timers,
            "combined_summary": timers_summary,
            "gate_summary": timer_gate,
        },
        "idle": {"runs": idle},
        "starvation": {"runs": starvation},
        "v6": v6,
        "multicore": multicore,
    }
    report["gates"] = gates
    report["performance_gates_passed"] = all(
        bool(gate["passed"]) for gate in gates.values()
    )
    report["all_gates_passed"] = bool(
        report["contractual"] and report["performance_gates_passed"]
    )
    return report


def main(argv: Optional[Sequence[str]] = None) -> int:
    try:
        options = parse_options(argv)
        report = execute(options)
        write_json_atomic(options.json_path.resolve(), report)
    except (BenchmarkError, OSError, subprocess.SubprocessError) as error:
        print("benchmark error: " + str(error), file=sys.stderr)
        return 2
    print("wrote " + str(options.json_path.resolve()))
    for name, gate in report["gates"].items():
        print(name + ": " + ("PASS" if gate["passed"] else "FAIL"))
    return 0 if report["all_gates_passed"] else 1


if __name__ == "__main__":
    raise SystemExit(main())
