"""Inclusive instruction counts of the build phases in callgrind_annotate --inclusive=yes output.

Usage: python3 callgrind_phases.py ANNOTATED.txt ...
"""
import re, sys
names = ['aura::main','aura_compiler::sha256_hex','aura_compiler::lower_path_to_mir','aura_compiler::check_path',
 'aura_compiler::mir::lower','aura_compiler::mir::lower_implicit','aura_compiler::mir::checks::resolve_checks',
 'aura_compiler::mir::ownership::insert_ownership','aura_compiler::native_codegen::emit_host_object_with_metadata',
 'aura_compiler::native_codegen::reachability::prune_unreachable_functions','aura_compiler::mir::resolve_runtime_ops',
 'aura_compiler::mir::checks::mark_covered_accesses','aura_compiler::mir::validate_at_boundary','aura_compiler::mir::validate_loan_flow',
 'aura_compiler::mir::ownership_check::validate_ownership','aura_compiler::mir::checks::validate_checks',
 'aura_compiler::native_codegen::NativeCodegen::with_validated_loans','aura_compiler::native_codegen::NativeCodegen::emit',
 'cranelift_codegen::context::Context::compile']
for path in sys.argv[1:]:
    got = {}
    for line in open(path):
        m = re.match(r'\s*([\d,]+) \([^)]*\)\s+\S*?:(\S+)', line)
        if not m: continue
        ir, fn = int(m.group(1).replace(',', '')), m.group(2)
        if fn in names and fn not in got: got[fn] = ir
    print('==', path)
    for n in names:
        if n in got: print(f'{got[n]/1e6:10.1f}M  {n}')
