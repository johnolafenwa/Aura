#!/usr/bin/env python3
"""Runtime value benchmarks: direct-backend wall time against Python; schema 1."""
from __future__ import annotations
import argparse
import json
import pathlib
import statistics
import subprocess
import sys
import tempfile
import time
try:
    from scripts import benchmark_process
except ImportError:
    import benchmark_process
ROOT = pathlib.Path(__file__).resolve().parent.parent
BENCH_DIR = ROOT / 'benchmarks/runtime_values'
PROGRAMS = ('dict_build_get', 'set_insert_contains', 'string_list_scan', 'int_list_sum',
            'record_field_reads')
REPORT_SCHEMA_VERSION = 1


def resolve_aura():
    for candidate in (ROOT / 'target/release/aura', ROOT / 'target/debug/aura'):
        if candidate.is_file():
            return candidate
    sys.exit('no aura binary found; run cargo build --release -p aura first')


def time_command(command, label, repetitions):
    """Run `command` `repetitions` times; return its stdout and the median seconds."""
    samples = []
    stdout = None
    for _ in range(repetitions):
        started = time.perf_counter()
        result = benchmark_process.run_process_group(command, label, stdout=subprocess.PIPE,
                                                     stderr=subprocess.PIPE, timeout=900)
        samples.append(time.perf_counter() - started)
        result.check_returncode()
        output = result.stdout.decode()
        if stdout is not None and output != stdout:
            raise ValueError(f'{label}: output changed between repetitions')
        stdout = output
    return stdout, statistics.median(samples)


def parse_reference(stdout):
    """Map each program to the (result, seconds) line the Python reference printed."""
    reference = {}
    for line in stdout.splitlines():
        name, result, seconds = line.split()
        reference[name] = (result, float(seconds))
    return reference


def summarize(records, reference):
    """Pair Aura and Python results, which must agree, and report the ratio."""
    summary = {}
    for name, record in records.items():
        python_result, python_seconds = reference[name]
        if record['stdout'].strip() != python_result:
            raise ValueError(f'{name}: Aura printed {record["stdout"].strip()!r}, '
                             f'Python printed {python_result!r}')
        summary[name] = {'aura_s': record['elapsed_s'], 'python_s': python_seconds,
                         'ratio': record['elapsed_s'] / python_seconds if python_seconds else None}
    return summary


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--aura', type=pathlib.Path)
    parser.add_argument('--python', default=sys.executable)
    parser.add_argument('--repetitions', type=int, default=3)
    parser.add_argument('--raw-json', type=pathlib.Path, required=True)
    parser.add_argument('--allow-dirty', action='store_true')
    parser.add_argument('programs', nargs='*', default=list(PROGRAMS))
    args = parser.parse_args()
    aura = (args.aura or resolve_aura()).resolve()
    commit = subprocess.check_output(['git', 'rev-parse', 'HEAD'], cwd=ROOT, text=True).strip()
    if not args.allow_dirty and subprocess.check_output(['git', 'status', '--porcelain'], cwd=ROOT):
        parser.error('measurement requires a clean checkout (or --allow-dirty)')
    records = {}
    with tempfile.TemporaryDirectory(prefix='aura-runtime-values-bench-') as directory:
        work = pathlib.Path(directory)
        for name in args.programs:
            binary = work / name
            subprocess.run([str(aura), 'build', '--backend', 'direct', '-o', str(binary),
                            str(BENCH_DIR / f'{name}.au')], cwd=ROOT, check=True,
                           stdout=subprocess.PIPE, stderr=subprocess.PIPE, timeout=900)
            stdout, elapsed = time_command([str(binary)], name, args.repetitions)
            records[name] = {'stdout': stdout, 'elapsed_s': elapsed}
    reference = subprocess.run([args.python, str(BENCH_DIR / 'reference.py'), *args.programs],
                               check=True, stdout=subprocess.PIPE, text=True, timeout=900).stdout
    summary = summarize(records, parse_reference(reference))
    report = {'schema_version': REPORT_SCHEMA_VERSION, 'commit': commit, 'aura': str(aura),
              'records': records, 'summary': summary}
    args.raw_json.write_text(json.dumps(report, indent=2, sort_keys=True) + '\n')
    for name, row in summary.items():
        print(f"{name}: aura {row['aura_s']:.3f} s, python {row['python_s']:.3f} s, "
              f"ratio {row['ratio']:.1f}x")


if __name__ == '__main__':
    main()
