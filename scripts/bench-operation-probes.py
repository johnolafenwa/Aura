#!/usr/bin/env python3
"""Operation probes: what one Aura operation costs next to Rust; schema 1.

benchmarks/operation_probes/operation_probes.au and operation_probes.rs each
repeat one operation `count` times. A run takes `<operation> <count> run`, or
`<operation> <count> setup` to build the same data without the operation.
operations.json lists every operation with its count and the plan step that
targets it.

By default the script counts instructions under callgrind. One operation
costs the instructions of the `run` run minus those of the `setup` run,
divided by the count, for Aura and for Rust. Both versions must print the
same line in each phase.

--check runs every operation at a count of 3 in both phases without valgrind,
and Aura and Rust must print the same line. CI runs this mode.
"""
from __future__ import annotations
import argparse
import importlib.util
import json
import pathlib
import platform
import shutil
import subprocess
import sys
import tempfile

_spec = importlib.util.spec_from_file_location(
    'cross_language_bench', pathlib.Path(__file__).with_name('bench-cross-language.py'))
cross = importlib.util.module_from_spec(_spec)
_spec.loader.exec_module(cross)

ROOT = cross.ROOT
PROBE_DIR = ROOT / 'benchmarks/operation_probes'
MANIFEST = PROBE_DIR / 'operations.json'
PROGRAM = 'operation_probes'
PHASES = ('setup', 'run')
CHECK_COUNT = 3
REPORT_SCHEMA_VERSION = 1
# Below this many instructions per operation, Rust's cost is too close to zero
# for a ratio to mean anything.
MIN_RATIO_BASE = 0.5


def load_operations(path=MANIFEST):
    return json.loads(path.read_text())


def per_operation(setup, run, count):
    """Instructions one operation costs: the run minus the setup, per repetition."""
    return (run - setup) / count


def same_output(name, outputs):
    """Aura and Rust must print the same line, which names the operation."""
    for phase in PHASES:
        rust, aura = outputs[('rust', phase)], outputs[('aura', phase)]
        if aura != rust:
            raise cross.BenchmarkError(f'{name} ({phase}): Aura printed {aura!r}, '
                                       f'Rust printed {rust!r}')
        if not rust.startswith(name + ' '):
            raise cross.BenchmarkError(f'{name} ({phase}): unexpected output {rust!r}')


def run_probe(binary, name, count, phase, label, prefix=()):
    result = cross.run([*prefix, str(binary), name, str(count), phase], label,
                       timeout=cross.RUN_TIMEOUT_S)
    return result.stdout.decode().strip(), result.stderr.decode(errors='replace')


def check(names, binaries):
    """Run every operation at a small count; return the failures."""
    failures = []
    for name in names:
        outputs = {}
        for language, binary in binaries.items():
            for phase in PHASES:
                outputs[(language, phase)], _ = run_probe(
                    binary, name, CHECK_COUNT, phase, f'{name} ({language}, {phase})')
        try:
            same_output(name, outputs)
            print(f'PASS {name}')
        except cross.BenchmarkError as error:
            failures.append(str(error))
            print(f'FAIL {error}')
    return failures


def measure(operations, names, binaries, callgrind, scratch):
    """Count instructions per operation for each language; return one row per operation."""
    rows = {}
    for name in names:
        count = operations[name]['count']
        row = {'count': count, 'instructions': {}}
        outputs = {}
        for language, binary in binaries.items():
            totals = {}
            for phase in PHASES:
                with tempfile.TemporaryDirectory(dir=scratch) as profile:
                    prefix = [callgrind, '--tool=callgrind',
                              f'--callgrind-out-file={pathlib.Path(profile) / "out"}']
                    outputs[(language, phase)], stderr = run_probe(
                        binary, name, count, phase, f'{name} ({language}, {phase})', prefix)
                totals[phase] = cross.callgrind_instructions(stderr)
            row['instructions'][language] = totals
            row[language] = per_operation(totals['setup'], totals['run'], count)
        same_output(name, outputs)
        rows[name] = row
        print(format_row(name, operations[name], row), flush=True)
    return rows


HEADER = f'{"operation":<18} {"step":>6} {"count":>8} {"rust/op":>9} {"aura/op":>10} {"aura/rust":>10}'


def format_row(name, operation, row):
    ratio = (f'{row["aura"] / row["rust"]:>9.1f}x' if row['rust'] >= MIN_RATIO_BASE
             else f'{"-":>10}')
    return (f'{name:<18} {operation["plan_step"]:>6} {row["count"]:>8,} {row["rust"]:>9.1f} '
            f'{row["aura"]:>10.1f} {ratio}')


def main():
    parser = argparse.ArgumentParser(description=__doc__,
                                     formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument('--check', action='store_true',
                        help='run every operation at a small count and compare the outputs')
    parser.add_argument('--aura', type=pathlib.Path)
    parser.add_argument('--json', type=pathlib.Path, help='write the counts to this file')
    parser.add_argument('--rust-target-dir', type=pathlib.Path,
                        help='reuse this Cargo target directory (default: a fresh one)')
    parser.add_argument('operations', nargs='*')
    args = parser.parse_args()
    operations = load_operations()
    names = args.operations or list(operations)
    unknown = sorted(set(names) - set(operations))
    if unknown:
        parser.error(f'unknown operations: {", ".join(unknown)}')
    aura = (args.aura or cross.resolve_aura()).resolve()
    callgrind = None
    if not args.check:
        callgrind = shutil.which('valgrind')
        if callgrind is None:
            parser.error('counting instructions needs valgrind on PATH; --check does not')
    with tempfile.TemporaryDirectory(prefix='aura-operation-probes-') as directory:
        work = pathlib.Path(directory)
        profile = cross.CHECK_PROFILE if args.check else cross.TIMING_PROFILE
        rust = cross.build_rust(args.rust_target_dir or work / 'rust-target', profile,
                                [PROGRAM])[PROGRAM]
        aura_binary = work / PROGRAM
        cross.run([str(aura), 'build', '-o', str(aura_binary), str(PROBE_DIR / f'{PROGRAM}.au')],
                  f'aura build {PROGRAM}', cwd=ROOT, timeout=cross.RUN_TIMEOUT_S)
        binaries = {'rust': rust, 'aura': aura_binary}
        if args.check:
            failures = check(names, binaries)
            if failures:
                sys.exit(f'{len(failures)} operation probe(s) failed')
            print(f'{len(names)} operation probes print the same line in Rust and Aura')
            return
        print(HEADER, flush=True)
        rows = measure(operations, names, binaries, callgrind, work)
    if args.json:
        commit = subprocess.check_output(['git', 'rev-parse', 'HEAD'], cwd=ROOT, text=True).strip()
        dirty = bool(subprocess.check_output(['git', 'status', '--porcelain'], cwd=ROOT))
        report = {'schema_version': REPORT_SCHEMA_VERSION, 'commit': commit, 'dirty': dirty,
                  'aura': str(aura), 'machine': platform.platform(), 'rows': rows}
        args.json.write_text(json.dumps(report, indent=2, sort_keys=True) + '\n')


if __name__ == '__main__':
    main()
