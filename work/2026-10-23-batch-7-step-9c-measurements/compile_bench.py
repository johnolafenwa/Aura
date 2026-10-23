"""Interleaved cold `aura build` times and callgrind counts per code generator.

Usage: python3 compile_bench.py PREFIX_AURA WORK OUT_JSON [N]

PREFIX_AURA is bin/aura of an installed layout, built with the `llvm`
feature and pipeline.diff. Every build gets a fresh AURA_CACHE_DIR, so each
one is cold. The order rotates each round. Callgrind counts the `aura`
process alone: the `cc` link runs in a child it does not trace.
"""
import json, os, pathlib, re, shutil, statistics, subprocess, sys, tempfile, time

aura, work, out_path = map(pathlib.Path, sys.argv[1:4])
n = int(sys.argv[4]) if len(sys.argv) > 4 else 11
root = pathlib.Path(__file__).resolve().parents[2]
subjects = {
    'hello_world': root / 'examples/basics/hello_world.au',
    'reference_agent': root / 'examples/agents/tool_runner/src/main.au',
    'json_codec_derived': root / 'crates/aura-compiler/tests/fixtures/run-pass/json_codec_derived.au',
}
variants = {
    'cranelift': {'AURA_NATIVE_CODEGEN': 'cranelift'},
    'llvm-O1': {'AURA_NATIVE_CODEGEN': 'llvm', 'AURA_LLVM_PIPELINE_MEASURE': 'default<O1>'},
    'llvm-O2': {'AURA_NATIVE_CODEGEN': 'llvm', 'AURA_LLVM_PIPELINE_MEASURE': 'default<O2>'},
    'llvm-O3': {'AURA_NATIVE_CODEGEN': 'llvm', 'AURA_LLVM_PIPELINE_MEASURE': 'default<O3>'},
}
work.mkdir(parents=True, exist_ok=True)


def env_for(tag, cache):
    env = {k: v for k, v in os.environ.items() if not k.startswith('AURA_')}
    env.update(variants[tag])
    env['AURA_CACHE_DIR'] = str(cache)
    return env


def build(tag, name, prefix=()):
    cache = pathlib.Path(tempfile.mkdtemp(prefix='cache-', dir=work))
    out = work / f'{name}.{tag}'
    started = time.perf_counter()
    r = subprocess.run([*prefix, str(aura), 'build', '--backend', 'direct', '-o', str(out),
                        str(subjects[name])], env=env_for(tag, cache), capture_output=True)
    elapsed = time.perf_counter() - started
    shutil.rmtree(cache)
    assert r.returncode == 0, r.stderr.decode()
    return elapsed, r.stderr, out.stat().st_size


result = {}
for name in subjects:
    keys = list(variants)
    samples = {k: [] for k in keys}
    sizes = {}
    for k in keys:
        build(k, name)  # warm the page cache
    for i in range(n):
        shift = i % len(keys)
        for k in keys[shift:] + keys[:shift]:
            elapsed, _, size = build(k, name)
            samples[k].append(elapsed)
            sizes[k] = size
    counts = {}
    for k in keys:
        _, stderr, _ = build(k, name, ('valgrind', '--tool=callgrind', '--callgrind-out-file=/dev/null'))
        counts[k] = int(re.search(rb'Collected : (\d+)', stderr).group(1))
    med = {k: statistics.median(v) for k, v in samples.items()}
    result[name] = {'median_s': med, 'samples_s': samples, 'bytes': sizes, 'aura_instructions': counts}
    print(name + ': ' + ', '.join(f'{k} {med[k]:.3f} s, {counts[k] / 1e6:.0f}M' for k in keys), flush=True)
out_path.write_text(json.dumps(result, indent=2, sort_keys=True) + '\n')
