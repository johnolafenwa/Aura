#!/usr/bin/env python3
"""Check what native Aura executables contain and how large they are.

The script builds each subject in `benchmarks/native_binaries/contents.json`
with a release `aura` compiler, attributes the executable's symbols to
runtime components, and enforces three rules:

- an `absent` component has no symbol in the subject;
- a `present` component has at least one symbol in the subject;
- a subject with a recorded size for this platform is at most
  `budget_percent` larger than that size.

`--attribute` prints the per-component symbol bytes instead of checking.
"""
from __future__ import annotations

import argparse
import json
import os
from pathlib import Path
import platform
import shutil
import subprocess
import sys
import tempfile

ROOT = Path(__file__).resolve().parent.parent
CONFIG = ROOT / 'benchmarks/native_binaries/contents.json'
TEXT_TYPES = set('tTwW')


def platform_key(system=None, machine=None):
    system = (system or platform.system()).lower()
    machine = (machine or platform.machine()).lower()
    machine = {'amd64': 'x86_64', 'aarch64': 'arm64'}.get(machine, machine)
    return f'{system}-{machine}'


def parse_nm(text):
    """Return `(address, type, name)` rows from `nm -n -C` output."""
    rows = []
    for line in text.splitlines():
        parts = line.split(None, 2)
        if len(parts) != 3 or len(parts[1]) != 1:
            continue
        try:
            address = int(parts[0], 16)
        except ValueError:
            continue
        rows.append((address, parts[1], parts[2]))
    return rows


def symbol_sizes(rows):
    """Estimate each text symbol's size from the next symbol's address."""
    rows = sorted(rows)
    sized = []
    for index, (address, kind, name) in enumerate(rows):
        if kind not in TEXT_TYPES:
            continue
        following = next((row[0] for row in rows[index + 1:] if row[0] > address), address)
        sized.append((name, following - address))
    return sized


def component_of(name, components):
    for component, patterns in components.items():
        if any(pattern in name for pattern in patterns):
            return component
    return None


def attribute(rows, components):
    totals = {component: 0 for component in components}
    totals['other'] = 0
    for name, size in symbol_sizes(rows):
        totals[component_of(name, components) or 'other'] += size
    return totals


def present_components(rows, components):
    found = set()
    for _, _, name in rows:
        component = component_of(name, components)
        if component:
            found.add(component)
    return found


def check_subject(name, subject, rows, size, components, baseline, budget_percent):
    """Return a list of failure messages for one subject."""
    failures = []
    found = present_components(rows, components)
    for component in subject.get('absent', []):
        if component in found:
            failures.append(f'{name}: component `{component}` must be absent but is linked')
    for component in subject.get('present', []):
        if component not in found:
            failures.append(f'{name}: component `{component}` must be present but is missing')
    if baseline is not None:
        limit = baseline * (100 + budget_percent) // 100
        if size > limit:
            failures.append(
                f'{name}: {size} bytes exceeds the {budget_percent}% budget over '
                f'the recorded {baseline} bytes (limit {limit})'
            )
    return failures


def run(command, env=None, cwd=None):
    result = subprocess.run(command, cwd=cwd, env=env, stdout=subprocess.PIPE,
                            stderr=subprocess.PIPE, text=True)
    if result.returncode != 0:
        raise RuntimeError(f'command failed: {" ".join(map(str, command))}\n{result.stderr}')
    return result.stdout


def build(aura, source, output, keep_symbols):
    env = {key: value for key, value in os.environ.items() if key != 'AURA_NATIVE_KEEP_SYMBOLS'}
    if keep_symbols:
        env['AURA_NATIVE_KEEP_SYMBOLS'] = '1'
    run([str(aura), 'build', '--backend', 'direct', '-o', str(output), str(ROOT / source)], env=env)


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument('--aura', type=Path, default=ROOT / 'target/release/aura')
    parser.add_argument('--config', type=Path, default=CONFIG)
    parser.add_argument('--attribute', action='store_true', help='print symbol bytes per component')
    parser.add_argument('--subject', action='append', help='limit to the named subjects')
    args = parser.parse_args(argv)

    config = json.loads(args.config.read_text())
    components = config['components']
    key = platform_key()
    baselines = config['baselines'].get(key, {})
    nm = shutil.which('nm')
    if nm is None:
        raise RuntimeError('`nm` is required')

    failures = []
    report = {'platform': key, 'subjects': {}}
    with tempfile.TemporaryDirectory(prefix='aura-native-binaries-') as directory:
        for name, subject in config['subjects'].items():
            if args.subject and name not in args.subject:
                continue
            stripped = Path(directory) / name
            symbols = Path(directory) / f'{name}.symbols'
            build(args.aura, subject['source'], stripped, keep_symbols=False)
            build(args.aura, subject['source'], symbols, keep_symbols=True)
            rows = parse_nm(run([nm, '-n', '-C', str(symbols)]))
            size = stripped.stat().st_size
            entry = {'bytes': size, 'components': sorted(present_components(rows, components))}
            if args.attribute:
                entry['symbol_bytes'] = attribute(rows, components)
            else:
                failures += check_subject(name, subject, rows, size, components,
                                          baselines.get(name), config['budget_percent'])
            report['subjects'][name] = entry
    print(json.dumps(report, indent=2))
    if not args.attribute:
        missing = [name for name in report['subjects'] if name not in baselines]
        if missing:
            print(f'note: no recorded size for {key}: {", ".join(missing)}', file=sys.stderr)
        for failure in failures:
            print(f'error: {failure}', file=sys.stderr)
    return 1 if failures else 0


if __name__ == '__main__':
    sys.exit(main())
