# Ordered collections

Each program processes 8,192 integers. The deque alternates between both
ends for insertion and removal. The min-heap accepts a permutation and pops
in ascending order. The map inserts the permutation as keys and then looks
up each key. The set inserts and removes the same keys. Every program
prints a checksum for each workload.

Rust uses `VecDeque`, `BinaryHeap<Reverse<i64>>`, `BTreeMap` and
`BTreeSet`. Python uses `collections.deque` and `heapq`. Its sorted map
keeps a dictionary with a sorted list of keys, and its sorted set keeps a
sorted list. Both use `bisect` for insertion. Maintaining order is part
of the measured work.

Build the release CLI and run from the repository root.

```sh
cargo build --release --locked -p aura
bash benchmarks/ordered_collections/run.sh
```

Each language times the operations with its monotonic clock. The runner
checks every checksum, discards one warm-up, alternates languages for
seven samples and reports the median in milliseconds. The two ratios
divide Aura's time by the reference time.

## Measurements

On 2026-11-17, the programs ran on one reserved CPU of a shared Intel Xeon
Platinum 8573C host running x86-64 Linux. Aura emitted optimized LLVM
code and linked the release runtime. The Rust reference used rustc 1.95.0
with `-O`. The Python reference used CPython 3.12.14.

| Workload | Aura (ms) | Rust (ms) | Python (ms) | Aura / Rust | Aura / Python |
| --- | ---: | ---: | ---: | ---: | ---: |
| Deque | 0.132 | 0.063 | 2.132 | 2.10 | 0.06 |
| Min-heap | 0.485 | 0.537 | 5.696 | 0.90 | 0.09 |
| Sorted map | 4.941 | 2.218 | 15.510 | 2.23 | 0.32 |
| Sorted set | 2.963 | 1.366 | 28.591 | 2.17 | 0.10 |

Each workload passed the three-times-Rust target and ran faster than
its Python reference. The map measures successful lookups and the set
measures successful removals. The deque and heap benchmark fresh local
receivers. Generic or aliased receivers and user-defined key orders take
different paths.
