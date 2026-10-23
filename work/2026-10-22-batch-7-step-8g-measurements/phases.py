"""Interleaved compile-time phases for two aura builds.

Usage: python3 phases.py BASE_AURA BASE_RUNTIME HEAD_AURA HEAD_RUNTIME WORK OUT_JSON [N]
Per subject and build: `aura check` (front end), a cold `aura build` with an
empty AURA_CACHE_DIR and a timing wrapper around cc (link), alternating base
and head. Lowering, MIR passes, and emit = build - check - link.
"""
import importlib.util, json, os, pathlib, shutil, statistics, subprocess, sys, tempfile, time
ROOT = pathlib.Path(__file__).resolve().parents[2]
spec = importlib.util.spec_from_file_location('bct', ROOT / 'scripts/bench-compile-time.py')
sys.path.insert(0, str(ROOT / 'scripts'))
bct = importlib.util.module_from_spec(spec); spec.loader.exec_module(bct)
ba, br, ha, hr, work, out = map(pathlib.Path, sys.argv[1:7])
n = int(sys.argv[7]) if len(sys.argv) > 7 else 7
if work.exists(): shutil.rmtree(work)
work.mkdir(parents=True)
layouts = {'base': bct.stage_installed_layout(work / 'base-prefix', ba, br),
           'head': bct.stage_installed_layout(work / 'head-prefix', ha, hr)}
compiler = shutil.which(os.environ.get('CC', 'cc'))
res = {}
for name, (source, entry_name) in bct.SUBJECTS.items():
    copy = work / 'src' / name
    if (ROOT / source).is_dir(): shutil.copytree(ROOT / source, copy)
    else:
        copy.mkdir(parents=True); shutil.copy2(ROOT / source, copy / entry_name)
    entry = copy / entry_name
    s = {k: {'check': [], 'build': [], 'link': []} for k in layouts}
    for i in range(n):
        for k in (['base', 'head'] if i % 2 == 0 else ['head', 'base']):
            aura = layouts[k]
            cache = pathlib.Path(tempfile.mkdtemp(prefix=f'cache-{k}-', dir=work))
            s[k]['check'].append(bct.build(aura, entry, work / 'o.bin', cache, mode='check'))
            log = work / f'{k}.link'
            if log.exists(): log.unlink()
            wrapper = work / f'wrap-{k}'
            wrapper.write_text(bct.link_wrapper(sys.executable, compiler, log)); wrapper.chmod(0o755)
            s[k]['build'].append(bct.build(aura, entry, work / f'{name}.{k}.bin', cache,
                                           env={**os.environ, 'CC': str(wrapper)}))
            s[k]['link'].append(sum(float(x) for x in log.read_text().split()))
            shutil.rmtree(cache)
    row = {}
    for k in layouts:
        m = {p: statistics.median(v) for p, v in s[k].items()}
        mid = [b - c - l for b, c, l in zip(s[k]['build'], s[k]['check'], s[k]['link'])]
        m['lower_mir_emit'] = statistics.median(mid)
        m['bytes'] = (work / f'{name}.{k}.bin').stat().st_size
        row[k] = {'median_s': m, 'samples_s': s[k]}
    res[name] = row
    b, h = row['base']['median_s'], row['head']['median_s']
    print(f"{name}: build {b['build']:.3f} -> {h['build']:.3f} s; check {b['check']:.3f} -> {h['check']:.3f}; "
          f"lower+mir+emit {b['lower_mir_emit']:.3f} -> {h['lower_mir_emit']:.3f}; link {b['link']:.3f} -> {h['link']:.3f}; "
          f"bytes {b['bytes']} -> {h['bytes']}", flush=True)
out.write_text(json.dumps(res, indent=2, sort_keys=True) + '\n')
