"""Callgrind instruction counts of the binaries runtime_bench.py built.

Usage: python3 callgrind_counts.py RUST_DIR WORK OUT_JSON

Counts cover the whole process, startup included. fib30 gets its GO line on
stdin before it starts.
"""
import json, pathlib, re, subprocess, sys

rust_dir, work, out_path = map(pathlib.Path, sys.argv[1:4])
progs = ['fib30', 'int_list_sum', 'string_list_scan', 'dict_build_get', 'set_insert_contains',
         'record_field_reads']
variants = ['cranelift', 'llvm-O1', 'llvm-O2', 'llvm-O3', 'rust']
result = {}
for name in progs:
    row = {}
    for tag in variants:
        binary = rust_dir / name if tag == 'rust' else work / f'{name}.{tag}'
        if not binary.exists():
            continue
        stdin = b'GO release-performance fib30\n' if name == 'fib30' else b''
        r = subprocess.run(['valgrind', '--tool=callgrind', '--callgrind-out-file=/dev/null',
                            str(binary)], input=stdin, capture_output=True, check=True)
        row[tag] = int(re.search(rb'Collected : (\d+)', r.stderr).group(1))
    result[name] = row
    print(name + ': ' + ', '.join(f'{k} {v / 1e6:.1f}M' for k, v in row.items()), flush=True)
out_path.write_text(json.dumps(result, indent=2, sort_keys=True) + '\n')
