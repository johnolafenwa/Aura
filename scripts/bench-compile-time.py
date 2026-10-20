#!/usr/bin/env python3
"""Compile-time measurements for `aura build` with the direct backend; schema 1.

Each subject is built from a private copy with an installed runtime layout, so
the timings never include a Cargo build of the runtime:

- cold: an empty cache directory, so the build compiles and links everything
- warm: an unchanged source and a filled cache, so the build reuses the binary
- edit: a source that gains one unused function before each build, so the
  front end runs and the native build cannot reuse the cached binary

One extra cold build runs with a timing wrapper around the C compiler and
reports how much of the build the final link takes.
"""
from __future__ import annotations
import argparse
import json
import os
import pathlib
import platform
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
REPORT_SCHEMA_VERSION = 1
# name: (path copied into the work directory, entry file inside the copy)
SUBJECTS = {
    'hello_world': ('examples/basics/hello_world.au', 'hello_world.au'),
    'reference_agent': ('examples/agents/tool_runner', 'src/main.au'),
    'unix_tls_roundtrip': ('examples/io/unix_tls_roundtrip.au', 'unix_tls_roundtrip.au'),
}
LINK_WRAPPER = '''#!{python}
import os, subprocess, sys, time
started = time.perf_counter()
status = subprocess.call([{compiler!r}, *sys.argv[1:]])
with open({log!r}, 'a') as log:
    log.write(f'{{time.perf_counter() - started:.6f}}\\n')
sys.exit(status)
'''


def edit_source(text, revision):
    """Append one unused function, so each revision lowers to different MIR."""
    return f'{text.rstrip()}\n\n\ndef compile_time_edit_{revision}() -> int64:\n    return {revision}\n'


def link_wrapper(python, compiler, log):
    return LINK_WRAPPER.format(python=python, compiler=compiler, log=str(log))


def summarize(samples):
    """Median and spread of each mode's samples, in seconds."""
    return {mode: {'median_s': statistics.median(values), 'min_s': min(values),
                   'max_s': max(values), 'samples': len(values)}
            for mode, values in samples.items()}


def stage_installed_layout(prefix, aura, runtime):
    """Lay out bin/aura and lib/aura the way a release archive does."""
    (prefix / 'bin').mkdir(parents=True)
    (prefix / 'lib/aura').mkdir(parents=True)
    shutil.copy2(aura, prefix / 'bin/aura')
    shutil.copy2(runtime, prefix / 'lib/aura/libaura_runtime.a')
    libs = subprocess.run(['cargo', 'rustc', '-q', '-p', 'aura-runtime', '--lib', '--release',
                           '--locked', '--', '--print', 'native-static-libs'], cwd=ROOT,
                          env={**os.environ, 'CARGO_TERM_COLOR': 'never'},
                          stdout=subprocess.DEVNULL, stderr=subprocess.PIPE, text=True, check=True)
    subprocess.run([sys.executable, str(ROOT / 'scripts/write-native-link-args.py')], cwd=ROOT,
                   env={**os.environ, 'NATIVE_STATIC_LIBS': libs.stderr,
                        'ARCHIVE_ROOT': str(prefix)}, check=True)
    return prefix / 'bin/aura'


def build(aura, entry, output, cache, env=None):
    """Run one `aura build`; return its wall time in seconds."""
    command = [str(aura), 'build', '--backend', 'direct', '-o', str(output), str(entry)]
    started = time.perf_counter()
    benchmark_process.run_process_group(
        command, f'aura build {entry.name}', env={**(env or os.environ), 'AURA_CACHE_DIR': str(cache)},
        stdout=subprocess.PIPE, stderr=subprocess.PIPE, timeout=1800).check_returncode()
    return time.perf_counter() - started


def measure_subject(aura, name, work, repetitions):
    source, entry_name = SUBJECTS[name]
    copy = work / 'src' / name
    if (ROOT / source).is_dir():
        shutil.copytree(ROOT / source, copy)
    else:
        copy.mkdir(parents=True)
        shutil.copy2(ROOT / source, copy / entry_name)
    entry = copy / entry_name
    original = entry.read_text()
    output = work / f'{name}.bin'
    samples = {'cold': [], 'warm': [], 'edit': []}
    for repetition in range(repetitions):
        cache = pathlib.Path(tempfile.mkdtemp(prefix='cache-', dir=work))
        samples['cold'].append(build(aura, entry, output, cache))
        shutil.rmtree(cache)
    warm_cache = pathlib.Path(tempfile.mkdtemp(prefix='cache-', dir=work))
    build(aura, entry, output, warm_cache)  # fills the cache; excluded
    for _ in range(repetitions):
        samples['warm'].append(build(aura, entry, output, warm_cache))
    for revision in range(1, repetitions + 1):
        entry.write_text(edit_source(original, revision))
        samples['edit'].append(build(aura, entry, output, warm_cache))
    entry.write_text(original)
    shutil.rmtree(warm_cache)
    log = work / f'{name}.link-times'
    wrapper = work / 'link-wrapper'
    compiler = shutil.which(os.environ.get('CC', 'cc'))
    wrapper.write_text(link_wrapper(sys.executable, compiler, log))
    wrapper.chmod(0o755)
    cache = pathlib.Path(tempfile.mkdtemp(prefix='cache-', dir=work))
    linked_total = build(aura, entry, output, cache, env={**os.environ, 'CC': str(wrapper)})
    shutil.rmtree(cache)
    link_s = sum(float(line) for line in log.read_text().split())
    return {'source': source, 'binary_bytes': output.stat().st_size,
            'modes': summarize(samples), 'samples': samples,
            'link': {'build_s': linked_total, 'link_s': link_s,
                     'link_share': link_s / linked_total if linked_total else None}}


def main():
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument('--aura', type=pathlib.Path, default=ROOT / 'target/release/aura')
    parser.add_argument('--runtime', type=pathlib.Path,
                        default=ROOT / 'target/release/libaura_runtime.a')
    parser.add_argument('--repetitions', type=int, default=5)
    parser.add_argument('--raw-json', type=pathlib.Path, required=True)
    parser.add_argument('--allow-dirty', action='store_true')
    parser.add_argument('subjects', nargs='*', default=list(SUBJECTS))
    args = parser.parse_args()
    if args.repetitions < 1:
        parser.error('--repetitions must be positive')
    for path in (args.aura, args.runtime):
        if not path.is_file():
            parser.error(f'{path} is missing; run cargo build --release -p aura -p aura-runtime')
    commit = subprocess.check_output(['git', 'rev-parse', 'HEAD'], cwd=ROOT, text=True).strip()
    if not args.allow_dirty and subprocess.check_output(['git', 'status', '--porcelain'], cwd=ROOT):
        parser.error('measurement requires a clean checkout (or --allow-dirty)')
    with tempfile.TemporaryDirectory(prefix='aura-compile-time-bench-') as directory:
        work = pathlib.Path(directory)
        aura = stage_installed_layout(work / 'prefix', args.aura, args.runtime)
        subjects = {name: measure_subject(aura, name, work, args.repetitions)
                    for name in args.subjects}
    report = {'schema_version': REPORT_SCHEMA_VERSION, 'commit': commit,
              'aura_sha256': rust_baselines.digest(args.aura),
              'host': f'{platform.system()} {platform.machine()} {platform.release()}',
              'cpus': os.cpu_count(), 'subjects': subjects}
    args.raw_json.parent.mkdir(parents=True, exist_ok=True)
    args.raw_json.write_text(json.dumps(report, indent=2, sort_keys=True) + '\n')
    for name, row in subjects.items():
        modes = row['modes']
        print(f"{name}: cold {modes['cold']['median_s']:.3f} s, warm {modes['warm']['median_s']:.3f} s, "
              f"edit {modes['edit']['median_s']:.3f} s, link {row['link']['link_share']:.0%} of a cold build")


if __name__ == '__main__':
    main()
