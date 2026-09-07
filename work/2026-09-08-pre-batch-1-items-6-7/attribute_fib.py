"""Reproduce exclusive leaf-PC attribution for the pre-change arm64 binary.

PC intervals are audited against fib-before-selected.asm. Sampling skid and
inlining prevent treating these proportions as exact removable costs.
"""
import collections
import gzip
import json
from pathlib import Path
import xml.etree.ElementTree as E

ROOT = Path(__file__).resolve().parent
names = ['emitted Aura function body', 'runtime call-frame push and pop',
         'recursion-depth accounting', 'safepoint and backedge yield polling',
         'checked integer arithmetic', 'argument and result marshalling',
         'other runtime', 'unattributed']
counts = dict.fromkeys(names, 0)
leaves = collections.Counter()
pcs = collections.Counter()
total = active = missing = 0
for path in sorted((ROOT / 'profiles').glob('fib-profile-before-*.xml.gz')):
    root = E.fromstring(gzip.decompress(path.read_bytes()))
    ids = {e.get('id'): e for e in root.iter() if e.get('id')}
    def resolve(e):
        return ids[e.get('ref')] if e is not None and e.get('ref') else e
    for row in root.findall('.//row'):
        total += 1
        tagged = resolve(row.find('tagged-backtrace'))
        backtrace = resolve(tagged.find('backtrace')) if tagged is not None else None
        if backtrace is None:
            missing += 1
            continue
        frames = [resolve(e) for e in backtrace.findall('frame')]
        if not any('aura_fn_fib' in f.get('name', '') for f in frames):
            continue
        assert int(resolve(row.find("weight")).text) == 1_000_000
        active += 1
        leaf = frames[0]
        name = leaf.get('name', '?')
        binary = resolve(leaf.find('binary'))
        pc = (int(leaf.get('addr'), 16) - int(binary.get('load-addr'), 16) + 0x100000000) & ~3
        leaves[name] += 1
        pcs[(name, hex(pc))] += 1
        category = 'unattributed'
        if name == 'aura_fn_fib':
            if 0x100004050 <= pc <= 0x100004060 or 0x10000406c <= pc <= 0x10000407c or 0x100004088 <= pc <= 0x100004094:
                category = 'checked integer arithmetic'
            elif pc in (0x100004014, 0x100004044, 0x100004068, 0x100004084, 0x1000040a4, 0x100004138, 0x100004148):
                category = 'argument and result marshalling'
            else:
                category = 'emitted Aura function body'
        elif 'from_utf8' in name or 'drop_in_place' in name and 'DirectCallFrameStorage' in name:
            category = 'runtime call-frame push and pop'
        elif name == 'aura_direct_enter_call_with_frame':
            if 0x100052ce4 <= pc <= 0x100052cec or pc in (0x100052d24, 0x100052d30):
                category = 'recursion-depth accounting'
            elif 0x100052a98 <= pc <= 0x100052b48 or 0x100052b54 <= pc <= 0x100052b70 or 0x100052d2c <= pc <= 0x100052e88:
                category = 'runtime call-frame push and pop'
            else:
                category = 'other runtime'
        elif name == 'aura_direct_exit_call':
            if 0x100053268 <= pc <= 0x100053274:
                category = 'recursion-depth accounting'
            elif 0x100053278 <= pc <= 0x100053434:
                category = 'runtime call-frame push and pop'
            else:
                category = 'other runtime'
        elif 'direct_task_runtime_key' in name or name == '_tlv_get_addr':
            category = 'other runtime'
        counts[category] += 1
assert sum(counts.values()) == active
result = {'recordings': 11, 'all_process_samples': total, 'fib_active_samples': active,
          'excluded_non_fib_or_unwind_missing': total-active, 'missing_backtraces': missing,
          'method': 'Exclusive leaf PC of samples whose stack contains aura_fn_fib; ASLR-normalized, aligned arm64 instruction PC. Frame category includes metadata UTF-8 validation. Other runtime includes TLS/task-state lookup, returned-view bookkeeping and runtime prologues/epilogues. Scalar argument/result register moves are separate; there is no boxing. No safepoints emitted in this non-tasking fib program. Unattributed is conditional on a recovered fib stack; excluded rows may include unwind failures as well as startup/protocol work.',
          'categories': [{'category': name, 'samples': count, 'percent': 100*count/active} for name,count in counts.items()],
          'leaf_counts': dict(leaves.most_common()),
          'leaf_pcs': [{'name': name, 'pc': pc, 'samples': count} for (name,pc),count in pcs.most_common()]}
(ROOT / 'fib-attribution.json').write_text(json.dumps(result, indent=2)+'\n')
print(json.dumps({k:v for k,v in result.items() if k != 'leaf_pcs'}, indent=2))
