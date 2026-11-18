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

## ASCII indexing

On a shared Linux x86-64 worker, the median of nine rotated runs for the
400,000 reads above was 0.64 ms in Aura, 0.86 ms in Rust and 81.26 ms in
Python. The previous Aura compiler took 3.74 ms on the same runs.

A second comparison increased the loop to 40 million reads, pinned one CPU
and recorded process CPU time. Per 400,000 reads, the medians were 0.841 ms
for Aura, 0.971 ms for Rust and 40.755 ms for Python. Aura took 4.490 ms
when the function received the string as an argument of unknown content.
All outputs counted the same characters.

## UTF-16 round trip

Nine rotated runs pinned to one CPU compared the original compiler, the
literal-encoding optimization, and the same optimization with the updated
UTF-16 decoder. Process CPU time per 4,000 round trips was 18.528 ms for
the original compiler, 13.506 ms with preselected encoding names, and
11.432 ms with the updated decoder. Rust took 10.341 ms and Python took
13.882 ms. The loop did 400,000 round trips per process. All three
implementations returned the same character count, 89,600,000.

## Materialized set union

On Linux x86-64, seven rotated runs of `set_union` took 4.493 ms in Aura,
16.457 ms in Rust, and 5.699 ms in Python. The previous Aura implementation
took 16.576 ms on the same input. Each implementation returned 384,000.
The new runtime determines membership and insertion order at union creation.
Large sets of scalar or string keys can share their backing storage until
one of the participating sets changes.

A separate process CPU comparison took 5.451 ms for Aura, 16.027 ms for
Rust and 5.150 ms for Python. The former Aura implementation took
12.264 ms in that run.

A second comparison shuffled the input and removed entries before each
union. Process CPU times include the operation following the union.

| Operation on the union | Previous Aura (ms) | Updated Aura (ms) |
| --- | ---: | ---: |
| Membership lookup | 13.102 | 4.221 |
| Consume every entry in order | 22.405 | 19.701 |
| Remove, add, check membership and read length | 16.350 | 18.100 |

The first mutation copies the storage. It cost 10.7% on the immediate-write
workload. The three checksums agreed with the previous implementation.
