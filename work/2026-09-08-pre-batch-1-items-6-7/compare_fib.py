"""Diagnostic-only paired GO/DONE timings; not contractual release results."""
import argparse
import hashlib
import importlib.util
import json
from pathlib import Path
import statistics
import sys

ROOT = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(ROOT))
spec = importlib.util.spec_from_file_location('release_runner', ROOT / 'scripts/bench-release-performance.py')
runner = importlib.util.module_from_spec(spec)
spec.loader.exec_module(runner)
p = argparse.ArgumentParser()
p.add_argument('--before', type=Path, required=True)
p.add_argument('--after', type=Path)
p.add_argument('--output', type=Path, required=True)
a = p.parse_args()
lanes = {'before': a.before}
if a.after:
    lanes['after'] = a.after
contract = runner.PROTOCOL_WORKLOADS['fib30']
rows = []
for n in range(13):
    order = list(lanes.items())
    if n % 2:
        order.reverse()
    row = {name: runner.run_protocol_lane(contract, name, [str(path.resolve())]) for name, path in order}
    if n >= 2:
        rows.append(row)
medians = {name: statistics.median(row[name]['protocol_elapsed_s'] for row in rows) for name in lanes}
result = {'status': 'diagnostic, quiet host without profiler; not contractual release measurement',
          'logical_calls': 2692537, 'warmups_per_lane': 2, 'pairs': rows,
          'binaries': {name: {'path': str(path.resolve()), 'sha256': hashlib.sha256(path.read_bytes()).hexdigest()} for name, path in lanes.items()},
          'median_seconds': medians,
          'nanoseconds_per_logical_call': {name: value * 1e9 / 2692537 for name, value in medians.items()}}
if 'after' in medians:
    result['improvement_percent'] = 100 * (1 - medians['after'] / medians['before'])
    result['eligible_20_percent'] = result['improvement_percent'] >= 20
print(json.dumps(result, indent=2))
a.output.write_text(json.dumps(result, indent=2) + '\n')
