# Representation measurements

Programs for the Batch 1 representation phase (checkpoint Q9 A, Q15 A,
Q16 A). `scripts/bench-representation.py` builds each program on both
backends, runs it with `AURA_RUNTIME_STATS=1`, and records the runtime's
allocation counters (union payload boxes, closure environments, opaque
runtime boxes, callable overflow allocations), wall time, and the direct
binary's size with commit provenance. The counters are the same numbers
the runtime reports to any program; the direct backend's union and
callable counts are the ratified layout claims, and the interpreter's are
reference numbers.

| Program | Measures |
| --- | --- |
| `union_scalar_local.au` | One million `int64 \| None` injections and tag tests in a loop |
| `union_class_field.au` | A plain class with a `Point \| None` field constructed and read one hundred thousand times |
| `callable_pack_inline.au` | One hundred thousand packings of a one-capture lambda into `Callable[...]`, each called once |
| `callable_pack_overflow.au` | One hundred thousand packings of a four-capture lambda (32 bytes, over the 24-byte inline buffer) |
| `callable_move.au` | One packed value moved through one hundred thousand locals and called once |
