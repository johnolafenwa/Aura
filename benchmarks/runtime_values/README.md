# Runtime value measurements

These programs measure how fast the direct backend's runtime stores and reads
values: dictionary and set lookups, list iteration, string comparison, and
reads from JSON-shaped records.

`scripts/bench-runtime-values.py` builds each program with the direct
backend, runs it three times, and records the median wall time with commit
provenance. It runs the matching [Rust reference](../rust_baselines/README.md)
the same way. It then runs `reference.py`, which does the same work in Python.
All three must print the same result.

```sh
cargo build --release -p aura
python3 scripts/bench-runtime-values.py --raw-json /tmp/runtime-values.json
```

| Program | Measures |
| --- | --- |
| `dict_build_get.au` | Builds a `dict[str, int64]` of 20,000 keys, then makes 200,000 `get` calls |
| `set_insert_contains.au` | Adds 20,000 `int64` values to a set, then makes 200,000 `in` tests |
| `int_list_sum.au` | Sums a list of 100,000 `int64` values with a `for` loop, 20 times |
| `string_list_scan.au` | Compares each of 10,000 strings with `==`, 100 times |
| `record_field_reads.au` | Reads one field of 5,000 `dict[str, json.Value]` records, 20 times |

Design record: [Batch 7a design checkpoint](../../architecture_docs/20-batch-7a-design-checkpoint.md).
