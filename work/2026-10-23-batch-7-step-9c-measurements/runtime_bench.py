"""Interleaved runtime medians: Cranelift, each LLVM pipeline, and Rust.

Usage: python3 runtime_bench.py AURA RUST_DIR WORK OUT_JSON [N] [VARIANT...]

AURA must be built with the `llvm` feature and the measurement patch in
pipeline.diff, which reads the pass pipeline from AURA_LLVM_PIPELINE_MEASURE.
Collection programs time the whole process. fib30 times GO to DONE. Each
variant builds with its own AURA_CACHE_DIR. The run order rotates, so no
variant always runs first.
"""
import hashlib, json, os, pathlib, statistics, subprocess, sys, time

aura, rust_dir, work, out_path = map(pathlib.Path, sys.argv[1:5])
n = int(sys.argv[5]) if len(sys.argv) > 5 else 31
root = pathlib.Path(__file__).resolve().parents[2]
progs = {
    'fib30': ('benchmarks/release_performance/fib30.au', True),
    'int_list_sum': ('benchmarks/runtime_values/int_list_sum.au', False),
    'string_list_scan': ('benchmarks/runtime_values/string_list_scan.au', False),
    'dict_build_get': ('benchmarks/runtime_values/dict_build_get.au', False),
    'set_insert_contains': ('benchmarks/runtime_values/set_insert_contains.au', False),
    'record_field_reads': ('benchmarks/runtime_values/record_field_reads.au', False),
}
variants = {
    'cranelift': {'AURA_NATIVE_CODEGEN': 'cranelift'},
    'llvm-O1': {'AURA_NATIVE_CODEGEN': 'llvm', 'AURA_LLVM_PIPELINE_MEASURE': 'default<O1>'},
    'llvm-O2': {'AURA_NATIVE_CODEGEN': 'llvm', 'AURA_LLVM_PIPELINE_MEASURE': 'default<O2>'},
    'llvm-O3': {'AURA_NATIVE_CODEGEN': 'llvm', 'AURA_LLVM_PIPELINE_MEASURE': 'default<O3>'},
}
chosen = sys.argv[6:] or list(variants)
variants = {k: v for k, v in variants.items() if k in chosen}
work.mkdir(parents=True, exist_ok=True)


def build(tag, name, src):
    out = work / f'{name}.{tag}'
    env = {k: v for k, v in os.environ.items() if not k.startswith('AURA_')}
    env.update(variants[tag])
    env['AURA_CACHE_DIR'] = str(work / f'cache-{tag}')
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
    bins = {tag: build(tag, name, src) for tag in variants}
    bins['rust'] = rust_dir / name
    sizes = {k: v.stat().st_size for k, v in bins.items()}
    digests = {k: hashlib.sha256(v.read_bytes()).hexdigest() for k, v in bins.items()}
    samples = {k: [] for k in bins}
    outputs = {}
    run = go_done if protocol else whole
    for k in bins:
        run(bins[k])  # warm
    keys = list(bins)
    for i in range(n):
        shift = i % len(keys)
        for k in keys[shift:] + keys[:shift]:
            e, o = run(bins[k])
            samples[k].append(e * 1e3)
            outputs.setdefault(k, o)
    assert len(set(outputs.values())) == 1, (name, outputs)
    med = {k: statistics.median(v) for k, v in samples.items()}
    result[name] = {'median_ms': med, 'samples_ms': samples, 'bytes': sizes, 'sha256': digests,
                    'ratio_to_rust': {k: med[k] / med['rust'] for k in bins}}
    print(name + ': ' + ', '.join(f'{k} {med[k]:.2f} ms ({med[k] / med["rust"]:.2f}x)' for k in bins),
          flush=True)
out_path.write_text(json.dumps(result, indent=2, sort_keys=True) + '\n')
