# AuraDB and Python: JSON Parsing, Union Values, and Scans

## Report

After the lending fix (`2026-10-25-auradb-lending-regression.md`), John's
AuraDB benchmark still trailed Python most on `doc_from_json` (34 ms
against 7.3 ms on his Mac), open + load (59 against 14 ms), and equality
scans (27 against 14 ms).

## Causes

Instruction profiles of AuraDB's per-operation probe (`src/probe2.au`,
callgrind, setup subtracted) showed five costs that the language runtime,
not AuraDB, adds:

- Every `json.parse` handed its source to a codec worker thread and waited
  for the result: about three futex calls a parse. On Linux that was most
  of `doc_from_json`'s wall time.
- The union helpers (`aura_direct_union_inject`, its copy form, the tag
  test, and the payload take) decoded the union's type descriptor on every
  call, about 6,000 instructions each. AuraDB's `values_equal` calls
  `json.dumps` twice per document, and the `None` default of its
  `indent: int64 | None` parameter is a union injection. That was a third
  of each scanned document.
- Tagging a fresh `Result` value with its generic type built its tagged
  enum name in two allocations, three times per scanned document.
- `try_clone_array_containing_value` sent a `(str, json.Value)` pair, as
  `items()` returns, through its general copy machinery.
- `match own` with a payload pattern that only binds copied the payload
  into the pattern test and released it, then moved it out in the arm.
  `open` decodes each stored document through such a match.

## Changes

- `json.parse` of at most 16 KiB that nests at most 8 arrays or objects
  parses on the calling task (`json_parse_inline`). ADR-0032 has the
  amendment, and a test parses an eight-level source on a thread with half
  of the smallest task stack. JSON enum and variant names are static.
- The union helpers read the descriptor through the thread's tag table
  (`descriptor_runtime_type_tag`), which decodes it once. `RuntimeTypeTag`
  records whether its name is a canonical descriptor, so a display name
  still raises the helpers' "invalid ... type" errors.
- A generic enum tag prebuilds its tagged variant name, and a fresh variant
  of that enum shares it.
- A tuple of leaves and leaf-payload variants clones directly.
- Taking and restoring a one-segment class field (a lend per loop
  iteration) allocates nothing: no path copy, no segment list, and the
  restored field keeps its key.
- MIR pattern lowering skips the payload read of a consumed binding-only
  subpattern (`mir.rs`, `Pattern::Variant`).

Tests: `mir::tests::consuming_match_reads_a_binding_only_payload_once`
fails on the old lowering. The runtime tests cover the inline parse limits
against the service path, the stack bound, shared tag names, descriptor
decoding, and flat tuple copies.

## Measurements

Instructions per operation, Linux x86_64, before (main with the lending
fix) and after:

| Operation | Before | After | Change |
| --- | ---: | ---: | ---: |
| insert one (n up to 1k) | 124,506 | 115,713 | -7% |
| update by id (n=2k) | 86,777 | 73,867 | -15% |
| delete by id (n=2k) | 23.8M | 22.3M | -6% |
| delete_where (n=2k) | 126.5M | 91.2M | -28% |
| doc_from_json | 57,454 | 29,522 | -49% |
| get by id (n=5k) | 27,497 | 23,536 | -14% |
| find, scan (n=5k) | 261.9M | 164.7M | -37% |
| find, range (n=5k) | 263.6M | 238.6M | -9% |
| count (n=5k) | 259.2M | 162.2M | -37% |
| create_index (n=5k) | 105.8M | 103.4M | -2% |
| find, indexed (n=5k) | 30.2M | 18.3M | -40% |
| save (n=5k) | 108.8M | 106.5M | -2% |
| open + load (n=5k) | 627.6M | 278.1M | -56% |
| insert_many (5k) | 370.7M | 354.4M | -4% |
| autosaved insert (n~1k) | 22.2M | 21.5M | -3% |

`open` is measured serially: its count shifts with thread timing when
several profiles run at once.

Wall time in the container (median, 1 ms timer, noisy), before, after, and
Python 3:

| Benchmark | Before | After | Python |
| --- | ---: | ---: | ---: |
| doc_from_json x2000 | 88 ms | 7 ms | 7.8 ms |
| open + load (n=5000) | 125 ms | 72 ms | 16 ms |
| find city=Lagos scan (n=5000) | 35 ms | 23 ms | 15 ms |
| count city=Lagos (n=5000) | 36 ms | 24 ms | 12 ms |
| delete_where 10% (n=2000) | 21 ms | 13 ms | 5.2 ms |

## Remaining Gaps

- `for k, v in d.items()` builds a list of copied pairs and copies each
  pair again to read it. A loop that lends dict entries in place would
  remove both copies.
- `for doc in self.docs` lends the field to each element read and restores
  it after, once per iteration; only a local list is lent once for the
  whole loop.
- Each same-type coercion of a collection or generic value retags it, about
  20 times per scanned document.
- AuraDB's `delete` rebuilds its position map in O(n), as the Python
  version does. Aura spends about 11,000 instructions a document there.
