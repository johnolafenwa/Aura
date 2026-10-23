"""Line classification of native_codegen.rs at ff35ee70, with inclusive ranges.

Groups: A translates MIR, B lays out values, boxes them, or follows the ABI,
and C1 to C7 are semantic decisions. The checkpoint lists each C group.
"""
R = [
 (1,93,'A','entry points, Cranelift flags'),
 (94,531,'B','scalar kinds, DirectType, union/callable/class layout, ValueRef, scratch ledger'),
 (532,576,'A','codegen state'),
 (577,681,'C5','named-argument binding for runtime ops'),
 (682,694,'B','slice word kinds'),
 (695,721,'C5','named-argument binding for runtime ops'),
 (722,732,'B','field slices'),
 (733,948,'A','module setup, declarations, emit'),
 (949,1342,'B','function prologue: frame, params and locals in words'),
 (1343,1623,'B','function thunks and default binders (boxed ABI)'),
 (1624,1654,'A','cleanup thunk setup'),
 (1655,1700,'C3','cleanup thunk: exit method by name'),
 (1701,1809,'B','cleanup thunk: unbox and release'),
 (1810,1913,'A','main wrapper'),
 (1914,1979,'B','runtime-op argument layout'),
 (1980,2008,'C2','MemberCallee: resolve an unresolved member call'),
 (2009,2075,'B','function-op argument and result layout'),
 (2076,2179,'B','view place and element selector shapes'),
 (2180,2491,'A','CFG order, view selector tags, closure writeback live-ins'),
 (2492,2659,'B','FunctionCompiler state, opaque ownership ledger'),
 (2660,3102,'B','retain/release by layout, transfer helpers, boxed Copy roots'),
 (3103,3278,'A','blocks and instructions'),
 (3279,3426,'B','Retain by layout, pair cancellation'),
 (3427,4068,'A','instruction bodies, terminators, returns'),
 (4069,4552,'A','rvalues, closures, literals'),
 (4553,4673,'A','unary and wide negation'),
 (4674,4793,'A','casts: helper by source and target kind'),
 (4794,4868,'B','inline union equality'),
 (4869,4934,'C7','borrowed-element comparison peephole'),
 (4935,4988,'B','literal str comparison without a box'),
 (4989,5180,'A','binary ops, Array operands'),
 (5181,5238,'B','int32 binary in 64-bit words'),
 (5239,5591,'A','wide integer, float, bool binary; trap branches'),
 (5592,5639,'A','call dispatch'),
 (5640,5751,'B','extern C calls (FFI marshalling)'),
 (5752,6002,'B','function values and inline callable calls'),
 (6003,6127,'A','print and f-strings'),
 (6128,6138,'A','named call'),
 (6139,6165,'C6','generic call: infer type arguments'),
 (6166,6215,'A','named call arguments, writebacks'),
 (6216,6227,'C5','named-argument binding for runtime ops'),
 (6228,6811,'A','named runtime ops, Array ctor, select, wait, host builtins, range'),
 (6812,6854,'C2','trait member call dispatch'),
 (6855,6903,'A','member call entry'),
 (6904,7023,'C7','borrowed-element len/get fusion'),
 (7024,7062,'A','Lend lowering'),
 (7063,7110,'C1','scalar-left Array add/sub/mul/div by member name'),
 (7111,7171,'C2','member call dispatch by receiver layout and name'),
 (7172,7227,'A','construct'),
 (7228,8204,'B','places, views, loans, loads, takes'),
 (8205,8616,'B','field extraction, coercion, boxing tags'),
 (8617,8641,'B','no_call_since tag elision'),
 (8642,8670,'B','raw tags, constants'),
 (8671,8712,'B','int32 range check'),
 (8713,9034,'B','inline list length, access, bounds checks'),
 (9035,9237,'B','stores, nested field replace'),
 (9238,9392,'C2','union member call: result type, clone, per-member dispatch'),
 (9393,9743,'B','union words, tag switch, boxing, ensure_opaque'),
 (9744,10133,'A','tuples, enums, variants, try'),
 (10134,10210,'C2','try: From impl selection'),
 (10211,10449,'B','mutable sinks'),
 (10450,10740,'A','cleanup registration and pending cleanups, writebacks'),
 (10741,10813,'C3','cleanup emission: exit method by name, TaskGroup inline'),
 (10814,10828,'A','manager exit failure record'),
 (10829,10865,'C2','class member call: method lookup'),
 (10866,10947,'A','class member call: arguments, writebacks'),
 (10948,11167,'A','runtime ops: handles, str'),
 (11168,11238,'C4','process and Supervisor argument defaults'),
 (11239,12096,'A','runtime ops: scalar, value, collections, Array, spans'),
 (12097,12419,'C2','opaque member call, erased-receiver dispatch, candidate chains'),
 (12420,12641,'A','opaque construct, task start'),
 (12642,12751,'B','runtime type tests for dispatch, span words'),
 (12752,12853,'C2','dynamic candidates, trait impl selection by specificity'),
 (12854,12920,'A','result storage, unit'),
 (12921,12934,'C2','concrete impl lookup'),
 (12935,12947,'A','consuming operands'),
 (12948,12956,'C2','method lookup by name'),
 (12957,13247,'B','helper and data declarations, frame descriptors, signatures, roots'),
 (13248,13275,'C3','cleanup exit method name and action'),
 (13276,13362,'B','cleanup thunk results, cleanup places, overlap'),
 (13363,13712,'A','emitter support checks'),
 (13713,14111,'B','FFI types, direct type mapping, enum payload types, field types'),
 (14112,14160,'A','operand types, rendering'),
 (14161,14670,'B','thunk boxing and unboxing, releases, signatures, mangling'),
 (14671,14686,'B','direct type to type'),
 (14687,14825,'C6','generic call: unify parameter and argument types'),
 (14826,14911,'A','numeric and wildcard type names, task start targets'),
 (14912,14942,'A','modules, comparison ops'),
]
import collections, sys
n=14942
seen=[0]*(n+1)
tot=collections.Counter()
for s,e,g,_ in R:
    for i in range(s,e+1):
        assert not seen[i], (i,g); seen[i]=1
    tot[g]+=e-s+1
missing=[i for i in range(1,n+1) if not seen[i]]
print('missing', missing[:10], len(missing))
a=tot['A']; b=tot['B']; c=sum(v for k,v in tot.items() if k.startswith('C'))
print('A',a,'B',b,'C',c,'total',a+b+c)
for k in sorted(tot):
    if k.startswith('C'): print(k,tot[k])
for s,e,g,d in R:
    if g.startswith('C'): print(g,f'{s}-{e}',e-s+1,d)
