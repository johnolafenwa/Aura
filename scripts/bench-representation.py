#!/usr/bin/env python3
"""Representation-phase allocation measurements on both backends; schema 1."""
from __future__ import annotations
import argparse
import json
import os
import pathlib
import subprocess
import sys
import tempfile
import time
try:
    from scripts import benchmark_process
except ImportError:
    import benchmark_process
ROOT = pathlib.Path(__file__).resolve().parent.parent
PROGRAMS = ('union_scalar_local', 'union_class_field', 'union_string_local', 'callable_pack_inline',
            'callable_pack_overflow', 'callable_move')
BACKENDS = ('mir', 'direct')
COUNTERS = ('union_payload_boxes', 'closure_environments', 'opaque_boxes',
            'callable_overflow_allocations')
# These observations supplement the original representation counters. Historical
# reports omit them; absence is not evidence that the runtime observed no clones.
PAYLOAD_CLONE_COUNTERS = ('payload_container_clone_observations',
                          'payload_fallible_clone_observations',
                          'payload_value_clone_observations')
REPORT_SCHEMA_VERSION = 1


def resolve_aura():
    for candidate in (ROOT / 'target/release/aura', ROOT / 'target/debug/aura'):
        if candidate.is_file():
            return candidate
    sys.exit('no aura binary found; run cargo build -p aura first')


def parse_stats(stderr, backend):
    """Return the counters the runtime reported for `backend`."""
    prefix = f'aura runtime stats ({backend}): '
    for line in stderr.splitlines():
        if line.startswith(prefix):
            stats = {}
            for field in line[len(prefix):].split():
                name, value = field.split('=', 1)
                if name not in COUNTERS + PAYLOAD_CLONE_COUNTERS:
                    raise ValueError(f'unexpected counter {name}')
                stats[name] = int(value)
            missing = [name for name in COUNTERS if name not in stats]
            if missing:
                raise ValueError(f'missing counters {missing}')
            return stats
    raise ValueError(f'no {backend} stats line in stderr')


def measure(aura, program, backend, work):
    """Run one program once with the stats request and return counters and timing."""
    env = dict(os.environ, AURA_RUNTIME_STATS='1')
    command = [str(aura), 'run', '--backend', backend, str(program)]
    started = time.perf_counter()
    result = benchmark_process.run_process_group(command, program.stem, stdout=subprocess.PIPE,
                                                 stderr=subprocess.PIPE, timeout=600, env=env)
    elapsed = time.perf_counter() - started
    result.check_returncode()
    stdout = result.stdout.decode()
    stats = parse_stats(result.stderr.decode(), backend)
    record = {'stdout': stdout, 'elapsed_s': elapsed, 'stats': stats}
    if backend == 'direct':
        binary = work / f'{program.stem}-direct'
        build = subprocess.run([str(aura), 'build', '--backend', 'direct', '-o', str(binary), str(program)],
                               cwd=ROOT, stdout=subprocess.PIPE, stderr=subprocess.PIPE, timeout=600)
        build.check_returncode()
        record['binary_bytes'] = binary.stat().st_size
    return record


def summarize(records):
    """Pair each program's backends and check that both printed the same result."""
    summary = {}
    for program, backends in records.items():
        outputs = {backends[backend]['stdout'] for backend in backends}
        if len(outputs) != 1:
            raise ValueError(f'{program}: backends disagree on stdout')
        summary[program] = {backend: {'stats': backends[backend]['stats'],
                                      'elapsed_s': backends[backend]['elapsed_s']}
                            for backend in backends}
        if 'direct' in backends and 'binary_bytes' in backends['direct']:
            summary[program]['direct']['binary_bytes'] = backends['direct']['binary_bytes']
    return summary


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--aura', type=pathlib.Path)
    parser.add_argument('--raw-json', type=pathlib.Path, required=True)
    parser.add_argument('--allow-dirty', action='store_true')
    args = parser.parse_args()
    aura = (args.aura or resolve_aura()).resolve()
    commit = subprocess.check_output(['git', 'rev-parse', 'HEAD'], cwd=ROOT, text=True).strip()
    if not args.allow_dirty and subprocess.check_output(['git', 'status', '--porcelain'], cwd=ROOT):
        parser.error('measurement requires a clean checkout (or --allow-dirty)')
    records = {}
    with tempfile.TemporaryDirectory(prefix='aura-representation-bench-') as directory:
        work = pathlib.Path(directory)
        for name in PROGRAMS:
            program = ROOT / 'benchmarks/representation' / f'{name}.au'
            records[name] = {backend: measure(aura, program, backend, work) for backend in BACKENDS}
    report = {'schema_version': REPORT_SCHEMA_VERSION, 'commit': commit, 'aura': str(aura),
              'records': records, 'summary': summarize(records)}
    args.raw_json.write_text(json.dumps(report, indent=2, sort_keys=True) + '\n')
    for program, backends in report['summary'].items():
        for backend, record in backends.items():
            print(program, backend, json.dumps(record['stats'], sort_keys=True))


if __name__ == '__main__':
    main()
