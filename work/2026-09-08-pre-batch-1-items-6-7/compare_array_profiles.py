"""Diagnostic profile control; the release publication uses detached runner reports."""
import hashlib
import importlib.util
import json
from pathlib import Path
import statistics
import sys

ROOT = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(ROOT))
spec = importlib.util.spec_from_file_location('array_runner', ROOT / 'scripts/bench-numeric-arrays.py')
runner = importlib.util.module_from_spec(spec)
sys.modules[spec.name] = runner
spec.loader.exec_module(runner)
paths = {name: Path('/private/tmp/aura-items-6-7-profile') / file for name, file in
         [('default', 'array-add-default'), ('tuned_before', 'array-add-before'), ('vectorized', 'array-add-vectorized')]}
inventory_before = runner.quiet_process_inventory()
if inventory_before:
    raise RuntimeError(f'host is not quiet: {inventory_before}')
rows = []
for repetition in range(13):
    order = list(paths)
    if repetition % 2:
        order.reverse()
    row = {name: runner.run_lane('aura_add', {'aura_add': [str(paths[name])]}) for name in order}
    if repetition >= 2:
        rows.append(row)
inventory_after = runner.quiet_process_inventory()
if inventory_after:
    raise RuntimeError(f'host ceased to be quiet: {inventory_after}')
medians = {name: statistics.median(row[name]['elapsed_s'] / 512 * 1e3 for row in rows) for name in paths}
report = {'status': 'diagnostic quiet-host profile control; not contractual detached-checkout evidence',
          'pre_inventory': inventory_before, 'post_inventory': inventory_after,
          'warmups_per_lane': 2, 'observations_per_lane': 11, 'pairs': rows,
          'median_ms_per_add': medians,
          'tuned_regression_percent': 100 * (medians['tuned_before'] / medians['default'] - 1),
          'vectorized_improvement_percent': 100 * (1 - medians['vectorized'] / medians['tuned_before']),
          'binaries': {name: {'path': str(path), 'sha256': hashlib.sha256(path.read_bytes()).hexdigest()} for name, path in paths.items()}}
(ROOT / 'work/2026-09-08-pre-batch-1-items-6-7/array-profile-timings.json').write_text(json.dumps(report, indent=2) + '\n')
print(json.dumps({k:v for k,v in report.items() if k != 'pairs'}, indent=2))
