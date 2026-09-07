"""Collect targeted Time Profiler samples from the unchanged fib30 executable."""
import argparse
import gzip
import hashlib
import json
from pathlib import Path
import subprocess
import tempfile

p = argparse.ArgumentParser()
p.add_argument('--binary', type=Path, required=True)
p.add_argument('--label', required=True)
p.add_argument('--output', type=Path, required=True)
p.add_argument('--runs', type=int, default=11)
a = p.parse_args()
a.output.mkdir(parents=True, exist_ok=True)
records = []
for index in range(a.runs):
    with tempfile.TemporaryDirectory(prefix='aura-fib-profile-') as temporary:
        root = Path(temporary)
        control = root / 'control.txt'
        control.write_text('GO release-performance fib30\n')
        trace = root / 'profile.trace'
        stdout = root / 'stdout.txt'
        command = ['xcrun', 'xctrace', 'record', '--template', 'Time Profiler',
                   '--time-limit', '2s', '--target-stdin', str(control),
                   '--target-stdout', str(stdout), '--output', str(trace),
                   '--no-prompt', '--launch', '--', str(a.binary.resolve())]
        run = subprocess.run(command, capture_output=True, timeout=90)
        log = a.output / f'{a.label}-{index:02d}.log'
        log.write_bytes(run.stdout + run.stderr)
        run.check_returncode()
        expected = b'READY release-performance fib30 30\nDONE release-performance fib30 832040\n'
        # xctrace may append a blank separator after target stdout.
        if stdout.read_bytes().rstrip() != expected.rstrip():
            raise RuntimeError(f'unexpected benchmark protocol: {stdout.read_bytes()!r}')
        export = ['xcrun', 'xctrace', 'export', '--input', str(trace), '--xpath',
                  '/trace-toc/run[@number="1"]/data/table[@schema="time-profile"]']
        raw = subprocess.run(export, capture_output=True, check=True, timeout=90).stdout
        artifact = a.output / f'{a.label}-{index:02d}.xml.gz'
        artifact.write_bytes(gzip.compress(raw, mtime=0))
        records.append({'record_command': command, 'export_command': export,
                        'protocol_stdout': stdout.read_text(),
                        'samples_file': artifact.name,
                        'samples_sha256': hashlib.sha256(artifact.read_bytes()).hexdigest(),
                        'uncompressed_samples_sha256': hashlib.sha256(raw).hexdigest()})
        (a.output / f'{a.label}-records.json').write_text(json.dumps({
            'binary': str(a.binary.resolve()),
            'binary_sha256': hashlib.sha256(a.binary.read_bytes()).hexdigest(),
            'runs': records}, indent=2) + '\n')
        print(f'{a.label}: {index + 1}/{a.runs} recordings validated', flush=True)
