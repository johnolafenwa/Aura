# AuraDB Regressions in 0.7.0: Lending Past Recursive Enums

## Report

John ran the AuraDB benchmark on 0.6.0 and 0.7.0. Most lines were
faster, but single inserts went from 9 to 71 ms, updates from 2 to 32 ms,
and autosaved inserts from 89 to 116 ms. The geometric mean was 0.82x.

## Reproduction

The container reproduces it with the release archives of both versions:
single inserts went from 23 to 147 ms and 200 updates from 4 to 68 ms.
`AURA_RUNTIME_STATS=1` shows the cause: `cloned_elements` grows with the
square of the document count. A smaller program does the same:

```aura
class Coll:
    docs: list[json.Value] = []

def main():
    mut c = Coll()
    mut i = 0
    while i < 100:
        c.docs.append(json.Value.Int(i))
        i += 1
```

On 0.7.0 it clones 4,950 elements, and the same loop over a
`list[list[int64]]` field clones none.

## Cause

The lending pass (step 8e part 4) moves a field into a temporary for a
call and back after it, so the call can change the field in place. It
refuses when an argument's type could hold a class instance, since that
instance could be the field's owner. `may_reach_instance` looked inside
enum payloads with a depth limit of 32 and answered yes past it.
`json.Value` holds `list[json.Value]` and `dict[str, json.Value]`, so the
walk never ended and every `json.Value` read as a possible instance.

Without a lend, the call loaded the field, which shares the list's
storage with the instance, and the first write copied every element. Each
`insert_into(self.docs, ...)`, each `self.docs.append(doc)`, and each
`doc[field] = value` through `match mut self.docs.lookup(i)` copied the
whole document list. The 0.6.0 emitter's lending check did not look inside
enums at all, which was unsound for an enum that holds an instance.

## Fix

`may_reach_instance` keeps the enums it is expanding. An enum that meets
itself again adds nothing, so `json.Value` and a user `Tree` that holds
`list[Tree]` reach no instance, and an enum with a class payload still
does. The depth limit stays for an enum whose arguments grow, such as
`Nest[list[T]]` inside `Nest[T]`.

Tests: `mir::lending::tests::a_recursive_enum_that_holds_no_instance_does_not_block_a_lend`
fails on the old check, and the `representation_stats` test
`direct_json_document_fields_are_lent_not_copied` pins an AuraDB-shaped
program at 4 cloned elements (99,704 before).

## Measurements

Instructions per operation under callgrind, Linux x86_64, from AuraDB's
`src/probe2.au` (setup subtracted):

| Operation | 0.6.0 | 0.7.0 | fix | fix vs 0.6.0 |
| --- | ---: | ---: | ---: | ---: |
| insert one (n up to 1k) | 179,557 | 1,241,516 | 124,505 | 0.69x |
| update by id (n=2k) | 102,630 | 2,441,659 | 87,241 | 0.85x |
| delete by id (n=2k) | 41.3M | 23.8M | 23.8M | 0.58x |
| delete_where (n=2k) | 177.8M | 123.0M | 128.9M | 0.72x |
| doc_from_json | 65,964 | 60,629 | 57,376 | 0.87x |
| get by id (n=5k) | 40,568 | 29,898 | 27,507 | 0.68x |
| find, scan (n=5k) | 345.8M | 263.4M | 261.7M | 0.76x |
| find, range (n=5k) | 398.8M | 263.7M | 264.4M | 0.66x |
| count (n=5k) | 343.4M | 260.4M | 260.1M | 0.76x |
| create_index (n=5k) | 176.9M | 108.3M | 109.9M | 0.62x |
| find, indexed (n=5k) | 41.3M | 30.3M | 30.2M | 0.73x |
| save (n=5k) | 123.3M | 110.6M | 108.8M | 0.88x |
| open + load (n=5k) | 619.2M | 436.3M | 377.5M | 0.61x |
| insert_many (5k) | 567.3M | 370.8M | 370.7M | 0.65x |
| autosaved insert (n~1k) | 25.4M | 55.3M | 22.2M | 0.88x |

Every operation now takes fewer instructions than on 0.6.0. In wall time
on the container, single inserts take 9 ms for 1,000 (0.6.0: 16 ms) and
2,000 updates take about 20 ms on both.

The Mac table also showed 0.7.0 slower on `doc_from_json`, `delete_where`,
and save, which the Linux instruction counts do not show. Those need a
Mac run of the fixed build.
