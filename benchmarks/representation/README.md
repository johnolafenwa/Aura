# Representation measurements

These programs measure how Aura represents unions and callables at runtime.

`scripts/bench-representation.py` builds each program on both backends and
runs it with `AURA_RUNTIME_STATS=1`. It records, with commit provenance:

- the runtime's allocation counters: union payload boxes, closure
  environments, opaque runtime boxes, and callable overflow allocations
- wall time
- the size of the direct binary

The counters are the same numbers the runtime reports to any program. The
direct backend's union and callable counts are the ratified layout claims. The
interpreter's counts are reference numbers.

| Program | Measures |
| --- | --- |
| `union_scalar_local.au` | One million `int64 \| None` injections and tag tests in a loop |
| `union_class_field.au` | A plain class with a `Point \| None` field constructed and read one hundred thousand times |
| `union_string_local.au` | One hundred thousand `str \| None` injections and matches; the string member is an owned handle word |
| `callable_pack_inline.au` | One hundred thousand packings of a one-capture lambda into `Callable[...]`, each called once |
| `callable_pack_overflow.au` | One hundred thousand packings of a four-capture lambda (32 bytes, over the 24-byte inline buffer) |
| `callable_move.au` | One packed value moved through one hundred thousand locals and called once |

Design record: these programs belong to the Batch 1 representation phase,
checkpoint answers Q9 A, Q15 A, and Q16 A.
