"""Interleaved runtime medians: base Aura, head Aura, and Rust.

Usage: python3 runtime_bench.py BASE_AURA HEAD_AURA RUST_DIR WORK OUT_JSON [N]
Collection programs time the whole process. fib30 and tasks_10000 time GO to DONE.
"""
import json, os, pathlib, statistics, subprocess, sys, time
base_aura, head_aura, rust_dir, work, out_path = map(pathlib.Path, sys.argv[1:6])
n = int(sys.argv[6]) if len(sys.argv) > 6 else 21
root = pathlib.Path(__file__).resolve().parents[2]
progs = {
    'fib30': ('benchmarks/release_performance/fib30.au', True),
    'tasks_10000': ('benchmarks/release_performance/tasks_10000.au', True),
    'int_list_sum': ('benchmarks/runtime_values/int_list_sum.au', False),
    'string_list_scan': ('benchmarks/runtime_values/string_list_scan.au', False),
    'dict_build_get': ('benchmarks/runtime_values/dict_build_get.au', False),
    'set_insert_contains': ('benchmarks/runtime_values/set_insert_contains.au', False),
    'record_field_reads': ('benchmarks/runtime_values/record_field_reads.au', False),
}
work.mkdir(parents=True, exist_ok=True)
def build(aura, tag, name, src):
    out = work / f'{name}.{tag}'
    cache = work / f'cache-{tag}'
    env = {**os.environ, 'AURA_CACHE_DIR': str(cache)}
    subprocess.run([str(aura), 'build', '--backend', 'direct', '-o', str(out), str(root / src)],
                   check=True, capture_output=True, env=env)
    return out
def whole(binary):
    t = time.perf_counter()
    r = subprocess.run([str(binary)], stdout=subprocess.PIPE, stderr=subprocess.DEVNULL)
    e = time.perf_counter() - t
    assert r.returncode == 0, binary
    return e, r.stdout
def go_done(binary):
    p = subprocess.Popen([str(binary)], stdin=subprocess.PIPE, stdout=subprocess.PIPE, text=True)
    ready = p.stdout.readline()
    go = 'GO ' + ' '.join(ready.split()[1:3]) + '\n'
    t = time.perf_counter(); p.stdin.write(go); p.stdin.flush()
    done = p.stdout.readline(); e = time.perf_counter() - t
    p.wait(); assert done.startswith('DONE'), (ready, done)
    return e, done.encode()
result = {}
for name, (src, protocol) in progs.items():
    bins = {'base': build(base_aura, 'base', name, src), 'head': build(head_aura, 'head', name, src),
            'rust': rust_dir / name}
    sizes = {k: v.stat().st_size for k, v in bins.items()}
    samples = {k: [] for k in bins}
    outputs = {}
    run = go_done if protocol else whole
    run(bins['base']); run(bins['head'])  # warm
    for i in range(n):
        order = ['base', 'head', 'rust'] if i % 2 == 0 else ['head', 'base', 'rust']
        for k in order:
            e, o = run(bins[k])
            samples[k].append(e * 1e3)
            outputs.setdefault(k, o)
    assert outputs['base'] == outputs['head'] == outputs['rust'], (name, outputs)
    med = {k: statistics.median(v) for k, v in samples.items()}
    result[name] = {'median_ms': med, 'samples_ms': samples, 'bytes': sizes,
                    'base_ratio': med['base'] / med['rust'], 'head_ratio': med['head'] / med['rust'],
                    'identical_binary': bins['base'].read_bytes() == bins['head'].read_bytes()}
    print(f"{name}: base {med['base']:.2f} ms ({med['base']/med['rust']:.2f}x), head {med['head']:.2f} ms "
          f"({med['head']/med['rust']:.2f}x), rust {med['rust']:.2f} ms; bytes {sizes['base']} -> {sizes['head']}", flush=True)
out_path.write_text(json.dumps(result, indent=2, sort_keys=True) + '\n')
