#!/usr/bin/env python3
"""Cross-language programs: Aura against Python and Rust on the same work; schema 1.

Each program in benchmarks/cross_language/<name>/ is written three times, as
<name>.au, <name>.py, and <name>.rs, and each version prints one summary line.
benchmarks/cross_language/programs.json gives every program a small check size
with the line it must print, and a larger bench size.

--check builds every program and runs all three versions at the check size.
Each must print the expected line. CI runs this mode, and it records no times.

Without --check, each version runs at the bench size (or the check size with
--size check) and the script prints the median wall time of each in
milliseconds, with Aura's ratio to Python and to Rust. All three versions must
print the same line. --callgrind also counts the instructions that the Aura and
Rust versions execute, under valgrind. --raw-json writes every sample.
"""
from __future__ import annotations
import argparse
import json
import os
import pathlib
import platform
import re
import shutil
import statistics
import subprocess
import sys
import tempfile
import time
try:
    from scripts import benchmark_process, rust_baselines
except ImportError:
    import benchmark_process
    import rust_baselines
ROOT = pathlib.Path(__file__).resolve().parent.parent
BENCH_DIR = ROOT / 'benchmarks/cross_language'
MANIFEST = BENCH_DIR / 'programs.json'
LANGUAGES = ('python', 'rust', 'aura')
# Replaced by a fresh, empty directory for each run of a program that writes files.
WORKDIR = '{workdir}'
REPORT_SCHEMA_VERSION = 1
# Timed runs use release Rust with overflow checks, as Aura checks every integer
# operation. --check only compares output, so it builds unoptimized and quickly.
TIMING_PROFILE = 'cross-language'
CHECK_PROFILE = 'dev'
RUN_TIMEOUT_S = 1800
CALLGRIND_TOTAL = re.compile(r'^==\d+== Collected : (\d+)$', re.MULTILINE)


class BenchmarkError(RuntimeError):
    pass


def load_programs(path=MANIFEST):
    return json.loads(path.read_text())


def resolve_aura():
    for candidate in (ROOT / 'target/release/aura', ROOT / 'target/debug/aura'):
        if candidate.is_file():
            return candidate
    sys.exit('no aura binary found; run cargo build --release -p aura first')


def expand_args(args, workdir):
    """The program's arguments, with the {workdir} placeholder filled in."""
    return [str(workdir) if arg == WORKDIR else arg for arg in args]


def run(command, label, **kwargs):
    """Run a command; fail with its stderr when it does not exit 0."""
    result = benchmark_process.run_process_group(command, label, stdout=subprocess.PIPE,
                                                 stderr=subprocess.PIPE, **kwargs)
    if result.returncode:
        raise BenchmarkError(f'{label} exited {result.returncode}:\n'
                             f'{result.stderr.decode(errors="replace")[-4000:]}')
    return result


def rust_build_command(target, profile, names):
    command = ['cargo', '+' + rust_baselines.TOOLCHAIN, 'build', '--locked', '--profile', profile,
               '--features', 'cross-language',
               '--manifest-path', str(rust_baselines.PROJECT / 'Cargo.toml'),
               '--target-dir', str(target)]
    for name in names:
        command += ['--bin', name]
    return command


def build_rust(target, profile, names):
    """Build the Rust versions; return each program's binary."""
    target = pathlib.Path(target).resolve()
    # Inherited profile overrides must not silently change the reference lane.
    env = {k: v for k, v in os.environ.items() if not k.startswith('CARGO_PROFILE_')
           and k not in ('RUSTFLAGS', 'CARGO_ENCODED_RUSTFLAGS', 'RUSTC_WRAPPER',
                         'RUSTC_WORKSPACE_WRAPPER')}
    run(rust_build_command(target, profile, names), 'Rust cross-language build', env=env,
        timeout=RUN_TIMEOUT_S)
    directory = 'debug' if profile == 'dev' else profile
    return {name: target / directory / name for name in names}


def build_aura(aura, names, output):
    """Build the Aura versions with `aura build`; return each program's binary."""
    binaries = {}
    for name in names:
        binary = output / name
        run([str(aura), 'build', '-o', str(binary), str(BENCH_DIR / name / f'{name}.au')],
            f'aura build {name}', cwd=ROOT, timeout=RUN_TIMEOUT_S)
        binaries[name] = binary
    return binaries


def commands(name, python, rust, aura):
    """Each language's command for one program, without its arguments."""
    return {'python': [python, str(BENCH_DIR / name / f'{name}.py')],
            'rust': [str(rust[name])], 'aura': [str(aura[name])]}


def run_once(command, args, label, scratch, prefix=()):
    """Run one program in a fresh work directory; return stdout, stderr, and seconds."""
    with tempfile.TemporaryDirectory(dir=scratch) as workdir:
        started = time.perf_counter()
        result = run([*prefix, *command, *expand_args(args, workdir)], label, timeout=RUN_TIMEOUT_S)
        elapsed = time.perf_counter() - started
    return result.stdout.decode(), result.stderr.decode(errors='replace'), elapsed


def callgrind_instructions(stderr):
    match = CALLGRIND_TOTAL.search(stderr)
    if match is None:
        raise BenchmarkError('callgrind printed no instruction total')
    return int(match.group(1))


def check(programs, names, python, rust, aura, scratch):
    """Run every version at its check size; return the failures."""
    failures = []
    for name in names:
        spec = programs[name]
        for language, command in commands(name, python, rust, aura).items():
            stdout, _, elapsed = run_once(command, spec['check'], f'{name} ({language})', scratch)
            line = stdout.strip()
            if line == spec['expected']:
                print(f'PASS {name} ({language}, {elapsed * 1000:.0f} ms)')
            else:
                failures.append(f'{name} ({language}) printed {line!r}, '
                                f'expected {spec["expected"]!r}')
                print(f'FAIL {failures[-1]}')
    return failures


def measure(programs, names, size, repetitions, python, rust, aura, scratch, callgrind):
    """Time every version of every program; return one record per program."""
    records = {}
    for name in names:
        args = programs[name][size]
        record = {'args': args, 'samples_s': {}, 'stdout': {}}
        for language, command in commands(name, python, rust, aura).items():
            samples = []
            for _ in range(repetitions):
                stdout, _, elapsed = run_once(command, args, f'{name} ({language})', scratch)
                samples.append(elapsed)
                previous = record['stdout'].setdefault(language, stdout)
                if stdout != previous:
                    raise BenchmarkError(f'{name} ({language}): output changed between runs')
            record['samples_s'][language] = samples
        if callgrind:
            record['instructions'] = {}
            for language in ('rust', 'aura'):
                with tempfile.TemporaryDirectory(dir=scratch) as profile:
                    prefix = [callgrind, '--tool=callgrind',
                              f'--callgrind-out-file={pathlib.Path(profile) / "out"}']
                    _, stderr, _ = run_once(commands(name, python, rust, aura)[language], args,
                                            f'{name} ({language}, callgrind)', scratch, prefix)
                record['instructions'][language] = callgrind_instructions(stderr)
        records[name] = record
        print(format_row(name, summarize_record(name, record)), flush=True)
    return records


def summarize_record(name, record):
    """Median milliseconds per language and Aura's ratios; the outputs must agree."""
    lines = {language: stdout.strip() for language, stdout in record['stdout'].items()}
    for language in ('rust', 'aura'):
        if lines[language] != lines['python']:
            raise BenchmarkError(f'{name}: {language} printed {lines[language]!r}, '
                                 f'Python printed {lines["python"]!r}')
    row = {f'{language}_ms': statistics.median(samples) * 1000
           for language, samples in record['samples_s'].items()}
    row['aura_vs_python'] = row['aura_ms'] / row['python_ms']
    row['aura_vs_rust'] = row['aura_ms'] / row['rust_ms']
    instructions = record.get('instructions')
    if instructions:
        row['rust_instructions'] = instructions['rust']
        row['aura_instructions'] = instructions['aura']
        row['aura_vs_rust_instructions'] = instructions['aura'] / instructions['rust']
    return row


HEADER = (f'{"program":<20} {"python ms":>10} {"rust ms":>9} {"aura ms":>10} '
          f'{"vs python":>9} {"vs rust":>8}')


def format_row(name, row):
    text = (f'{name:<20} {row["python_ms"]:>10.1f} {row["rust_ms"]:>9.1f} {row["aura_ms"]:>10.1f} '
            f'{row["aura_vs_python"]:>8.2f}x {row["aura_vs_rust"]:>7.1f}x')
    if 'aura_instructions' in row:
        text += (f'   instructions: rust {row["rust_instructions"] / 1e6:,.1f}M, '
                 f'aura {row["aura_instructions"] / 1e6:,.1f}M '
                 f'({row["aura_vs_rust_instructions"]:.1f}x)')
    return text


def main():
    parser = argparse.ArgumentParser(description=__doc__,
                                     formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument('--check', action='store_true',
                        help='compare every version against the expected output')
    parser.add_argument('--aura', type=pathlib.Path)
    parser.add_argument('--python', default=sys.executable)
    parser.add_argument('--size', choices=('bench', 'check'), default='bench')
    parser.add_argument('--repetitions', type=int, default=3)
    parser.add_argument('--callgrind', action='store_true',
                        help='also count Aura and Rust instructions under valgrind')
    parser.add_argument('--raw-json', type=pathlib.Path)
    parser.add_argument('--rust-target-dir', type=pathlib.Path,
                        help='reuse this Cargo target directory (default: a fresh one)')
    parser.add_argument('programs', nargs='*')
    args = parser.parse_args()
    programs = load_programs()
    names = args.programs or sorted(programs)
    unknown = sorted(set(names) - set(programs))
    if unknown:
        parser.error(f'unknown programs: {", ".join(unknown)}')
    aura = (args.aura or resolve_aura()).resolve()
    callgrind = None
    if args.callgrind:
        callgrind = shutil.which('valgrind')
        if callgrind is None:
            parser.error('--callgrind needs valgrind on PATH')
    with tempfile.TemporaryDirectory(prefix='aura-cross-language-') as directory:
        work = pathlib.Path(directory)
        (work / 'aura').mkdir()
        (work / 'runs').mkdir()
        profile = CHECK_PROFILE if args.check else TIMING_PROFILE
        rust = build_rust(args.rust_target_dir or work / 'rust-target', profile, names)
        aura_binaries = build_aura(aura, names, work / 'aura')
        if args.check:
            failures = check(programs, names, args.python, rust, aura_binaries, work / 'runs')
            if failures:
                sys.exit(f'{len(failures)} cross-language check(s) failed')
            print(f'{len(names)} programs print the expected line in Python, Rust, and Aura')
            return
        print(HEADER, flush=True)
        records = measure(programs, names, args.size, args.repetitions, args.python, rust,
                          aura_binaries, work / 'runs', callgrind)
    summary = {name: summarize_record(name, record) for name, record in records.items()}
    if args.raw_json:
        commit = subprocess.check_output(['git', 'rev-parse', 'HEAD'], cwd=ROOT, text=True).strip()
        dirty = bool(subprocess.check_output(['git', 'status', '--porcelain'], cwd=ROOT))
        python_version = subprocess.check_output([args.python, '--version'], text=True).strip()
        report = {'schema_version': REPORT_SCHEMA_VERSION, 'commit': commit, 'dirty': dirty,
                  'aura': str(aura), 'python': python_version, 'size': args.size,
                  'repetitions': args.repetitions, 'machine': platform.platform(),
                  'records': records, 'summary': summary}
        args.raw_json.write_text(json.dumps(report, indent=2, sort_keys=True) + '\n')


if __name__ == '__main__':
    main()
