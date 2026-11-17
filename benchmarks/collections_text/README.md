# Collections, Text, And Integer Math

The programs perform the same four workloads in native Aura, optimized Rust and Python. Each prints its elapsed milliseconds and a checksum. The runner compares checksums across all three languages.

| Workload | Timed work | Checksum |
| --- | --- | ---: |
| `gcd` | Sum 500,000 greatest common divisors with varying first arguments. | 2,779,826 |
| `ascii_index` | Read 400,000 characters at varying positions in an ASCII string and count the occurrences of `a`. | 181,818 |
| `set_union` | Materialize and count 2,000 unions of two 128-element integer sets. The sets share 64 keys. | 384,000 |
| `utf16_round_trip` | Encode and decode a 224-scalar string 4,000 times as UTF-16 little endian. The emoji needs two UTF-16 code units. | 896,000 |

The set and string inputs are prepared before timing. Rust indexes bytes for the ASCII case and materializes the set union in a `HashSet`. Its UTF-16 workload converts each code unit to two bytes and reconstructs a UTF-8 string.

Run the benchmark from the repository root.

```sh
cargo build --locked --release -p aura
bash benchmarks/collections_text/run.sh
```
