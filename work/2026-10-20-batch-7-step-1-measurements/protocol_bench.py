"""Diagnostic GO-to-DONE medians for Aura and Rust protocol workloads.

Usage: python3 protocol_bench.py RUST_RELEASE_DIR OUTPUT_JSON
"""
import subprocess, sys, time, statistics, json, tempfile, pathlib
root = pathlib.Path(__file__).resolve().parents[2]
aura = root / 'target/release/aura'
rust = pathlib.Path(sys.argv[1])
d = pathlib.Path(tempfile.mkdtemp())
work = {'fib30': 'fib30', 'tasks_10000': 'tasks_10000', 'tcp_fanout': 'tcp_fanout'}
def go_done(binary):
    p = subprocess.Popen([str(binary)], stdin=subprocess.PIPE, stdout=subprocess.PIPE, text=True)
    ready = p.stdout.readline()
    go = 'GO ' + ' '.join(ready.split()[1:3]) + '\n'
    t = time.perf_counter(); p.stdin.write(go); p.stdin.flush()
    done = p.stdout.readline(); e = time.perf_counter() - t
    p.wait(); assert done.startswith('DONE'), (ready, done)
    return e
out = {}
for name, src in work.items():
    b = d / name
    subprocess.run([str(aura), 'build', '--backend', 'direct', '-o', str(b), str(root / f'benchmarks/release_performance/{src}.au')], check=True, capture_output=True)
    a, r = [], []
    for i in range(11):
        a.append(go_done(b)); r.append(go_done(rust / name))
    out[name] = {'aura_ms': statistics.median(a)*1e3, 'rust_ms': statistics.median(r)*1e3}
    out[name]['ratio'] = out[name]['aura_ms'] / out[name]['rust_ms']
    print(name, json.dumps(out[name]))
json.dump(out, open(sys.argv[2], 'w'), indent=2)
