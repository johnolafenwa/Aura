# Cross-language programs

Thirty complete programs, each written in Aura, Python, and Rust, that do the
same work and print the same summary line. They cover the kinds of software
people write: databases, servers, a web browser, compilers and interpreters,
text and data processing, graphics, simulations, and concurrent pipelines.
Timing the three side by side shows where Aura is slow on real code, so the
results can guide performance work.

Each program has its own directory with `<name>.au`, `<name>.py`, and
`<name>.rs`. The Rust versions are bins of the
[Rust baselines](../rust_baselines/README.md) project, behind its
`cross-language` feature. `programs.json` gives each program a small check
size with the line it must print, and a larger bench size.

## Running

```sh
cargo build --release -p aura
npm run check:cross-language   # every version prints the expected line
npm run bench:cross-language   # median wall time of each version, in ms
python3 scripts/bench-cross-language.py --callgrind --repetitions 1 nbody sql_engine
```

The [operation probes](../operation_probes/README.md) measure single
operations, such as a class field read or a `range` loop step, in Aura and
Rust, for work aimed at one cost.

`npm run ci` runs the check, which records no times. The timed mode builds
the Rust versions with the `cross-language` profile, which is the release
profile with overflow checks on, builds the Aura versions with `aura build`,
and runs the Python versions with the `python3` that runs the script. Each
version runs three times at the bench size and the median counts. The three
versions must print the same line. `--callgrind` also counts the instructions
the Aura and Rust versions execute under valgrind, and `--raw-json` saves
every sample with the commit.

## Programs

| Program | What it does |
| --- | --- |
| `astar_pathfinding` | Game AI: A* path finding on a weighted grid with a binary heap |
| `bank_ledger` | Business logic: accounts with typed errors and a double-entry journal |
| `binary_trees` | Allocation: builds and walks many complete binary trees (Benchmarks Game) |
| `bytecode_vm` | A two-pass assembler and a stack-machine dispatch loop running three programs |
| `csv_aggregate` | Parses a CSV sales export and aggregates it by region, product, and month |
| `docdb` | A schemaless JSON document database with a primary and a secondary index, saved and reloaded as JSON lines |
| `file_io` | Writes and reads a line log, a binary record file, and a directory of small files |
| `graph_paths` | BFS, Dijkstra with a binary heap, and union-find on a random graph |
| `hashing` | FNV-1a, CRC-32, Adler-32, and content-defined chunking over a byte buffer |
| `http_api` | An HTTP JSON item service and a client, over loopback |
| `image_filters` | Blur, edge detection, a histogram, downscaling, and PGM encoding |
| `interpreter` | Lexes, parses, and runs a small imperative language with a tree-walking interpreter |
| `json_roundtrip` | Parses, walks, and serializes an API-style JSON payload |
| `kv_server` | A key-value server with a line protocol over loopback TCP |
| `lru_cache` | A least-recently-used cache with string keys |
| `lzw_compress` | LZW with 12-bit codes and run-length coding, both round-tripped |
| `markdown_render` | Renders Markdown with headings, lists, code blocks, quotes, and inline markup to HTML |
| `mini_browser` | A text-mode web browser: fetches pages over HTTP, parses HTML and CSS, lays out text, follows links |
| `nbody` | The n-body simulation of the Jovian planets (Benchmarks Game) |
| `order_book` | A limit order book with price-time priority matching |
| `pubsub_broker` | A publish/subscribe broker with topic fan-out and mailboxes, over loopback TCP |
| `raytracer` | A ray tracer with shadows and reflections |
| `regex_match` | A regular-expression compiler and Pike VM run over generated log lines |
| `sort_records` | Built-in sorts, key sorts, string sorts, and hand-written quicksort and merge sort |
| `spreadsheet` | Formulas, a dependency graph in topological order, and recalculation after edits |
| `sql_engine` | A SQL tokenizer, parser, planner, and executor with an id index, range scans, updates, deletes, and `GROUP BY` |
| `task_pool` | A worker pool on bounded queues, a three-stage pipeline, and a fan-out of tasks |
| `template_render` | A Mustache-style template engine producing HTML pages |
| `text_diff` | A line diff with the Myers O(ND) algorithm, checked against an LCS table |
| `word_freq` | Tokenizes a generated corpus and counts words and bigrams |

## Rules

- Every program makes its own input from a seeded MINSTD generator,
  `state * 48271 % 2147483647`, so all three versions see the same data and no
  data files are needed. `file_io` and `docdb` also take a work directory,
  which the script creates fresh for each run.
- The three versions use the same algorithms and data layout, written the way
  each language is usually written. Each uses its own standard library for
  collections, strings, JSON, files, and sockets.
- Integers are 64-bit, and the Rust versions keep overflow checks on, as Aura
  checks every integer operation.
- The concurrent programs use asyncio in Python, tokio's multi-thread runtime
  in Rust, and tasks and queues in Aura. The network programs talk over
  loopback TCP and serve one connection at a time.
- Each program prints one line, `<name> key=value ...`, and the versions must
  match it exactly. Floating-point results are printed with fixed digits.
- Where Aura lacks a feature, the Aura version works around it and a comment
  says why.

## Recorded results

Measured on 3 November 2026 at the 0.12.0 release candidate, in a Linux
x86-64 container with 4 vCPUs (Intel Xeon, 2.1 GHz), Python 3.13.18, and
Rust 1.95.0. Each time is the median of ten whole-process runs at the bench
size, pooled from two passes, so it includes process startup and, for Aura,
the runtime's task scheduler. Lower ratios are better for Aura. The Aura
0.11.0 column is the same program built by the published 0.11.0 release
archive, measured in the same passes on the same machine. The container was
busier that day than at the 0.11.0 release: Python took 46 seconds for all
30 programs instead of 41, and 0.11.0 took 19.7 seconds instead of 16.4.
`file_io` syncs a file to disk in all three versions, and the threaded
programs (`http_api`, `kv_server`, `pubsub_broker`, `task_pool`) and the
shortest ones (`bytecode_vm`, `nbody`, `order_book`) move by 10 to 50% from
run to run.

| Program | Size | Python ms | Rust ms | Aura ms | Aura 0.11.0 ms | Aura / Python | Aura / Rust |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| `astar_pathfinding` | 500 | 2,301 | 95 | 316 | 286 | 0.14x | 3.3x |
| `bank_ledger` | 200000 | 355 | 34 | 167 | 162 | 0.47x | 5.0x |
| `binary_trees` | 14 | 316 | 62 | 245 | 231 | 0.77x | 4.0x |
| `bytecode_vm` | 200000 | 2,236 | 43 | 61 | 62 | 0.03x | 1.4x |
| `csv_aggregate` | 200000 | 507 | 138 | 584 | 594 | 1.15x | 4.2x |
| `docdb` | 50000 | 625 | 421 | 1,651 | 1,652 | 2.64x | 3.9x |
| `file_io` | 300000 | 1,425 | 935 | 1,746 | 1,847 | 1.23x | 1.9x |
| `graph_paths` | 200000 | 4,903 | 266 | 936 | 921 | 0.19x | 3.5x |
| `hashing` | 2000000 | 1,663 | 46 | 64 | 65 | 0.04x | 1.4x |
| `http_api` | 3000 | 1,242 | 350 | 338 | 346 | 0.27x | 1.0x |
| `image_filters` | 1000 | 3,062 | 55 | 88 | 85 | 0.03x | 1.6x |
| `interpreter` | 20000 | 2,969 | 364 | 1,295 | 1,323 | 0.44x | 3.6x |
| `json_roundtrip` | 50 | 456 | 156 | 758 | 751 | 1.66x | 4.9x |
| `kv_server` | 50000 | 3,876 | 2,064 | 2,469 | 2,346 | 0.64x | 1.2x |
| `lru_cache` | 200000 | 267 | 33 | 125 | 102 | 0.47x | 3.7x |
| `lzw_compress` | 1000000 | 536 | 48 | 293 | 303 | 0.55x | 6.2x |
| `markdown_render` | 20000 | 903 | 72 | 365 | 370 | 0.40x | 5.1x |
| `mini_browser` | 500 | 951 | 228 | 1,004 | 973 | 1.06x | 4.4x |
| `nbody` | 300000 | 1,442 | 17 | 121 | 80 | 0.08x | 7.3x |
| `order_book` | 100000 | 154 | 7 | 29 | 23 | 0.19x | 4.1x |
| `pubsub_broker` | 50000 | 4,306 | 2,246 | 2,973 | 2,971 | 0.69x | 1.3x |
| `raytracer` | 500 | 2,948 | 32 | 150 | 124 | 0.05x | 4.7x |
| `regex_match` | 5000 | 1,685 | 106 | 668 | 692 | 0.40x | 6.3x |
| `sort_records` | 100000 | 982 | 103 | 436 | 502 | 0.44x | 4.3x |
| `spreadsheet` | 1200 | 923 | 39 | 159 | 162 | 0.17x | 4.1x |
| `sql_engine` | 10000 | 674 | 57 | 363 | 373 | 0.54x | 6.4x |
| `task_pool` | 100000 | 850 | 286 | 569 | 534 | 0.67x | 2.0x |
| `template_render` | 10000 | 690 | 306 | 1,143 | 1,127 | 1.66x | 3.7x |
| `text_diff` | 20000 | 1,768 | 91 | 169 | 185 | 0.10x | 1.8x |
| `word_freq` | 500000 | 714 | 207 | 608 | 558 | 0.85x | 2.9x |

All 30 Aura programs take 19.9 seconds, against 19.7 seconds with 0.11.0
in the same passes. Python takes 46 seconds and Rust 8.9 seconds. The
median program takes 0.46 times Python's time and 3.8 times Rust's, against
0.45 and 3.7 times with 0.11.0. Aura is faster than Python on 24 programs.
It is slower on `csv_aggregate`, `docdb`, `file_io`, `json_roundtrip`,
`mini_browser`, and `template_render`. No program is above 8x Rust.

0.12.0 is a language release and has no performance work, so these
differences are run-to-run spread: both compilers execute the same number
of instructions on every program (below). `nbody`, `order_book`,
`lru_cache`, and `raytracer` read 21 to 52% slower here; timed alone, 20
alternating runs each, the two `nbody` binaries took 128 and 124 ms.

### Instruction counts

Instructions executed under callgrind at the bench size, in millions, one
run each, measured on 3 November 2026 for the 0.12.0 release candidate
against the published 0.11.0 archive. They vary much less from run to run
than times do. The threaded programs (`http_api`, `kv_server`,
`pubsub_broker`, `task_pool`) still move by a few percent.

| Program | Rust M | Aura M | Aura 0.11.0 M | Change | Aura / Rust |
| --- | ---: | ---: | ---: | ---: | ---: |
| `astar_pathfinding` | 300.4 | 2,115.0 | 2,115.1 | -0.0% | 7.0x |
| `bank_ledger` | 316.0 | 1,234.2 | 1,234.2 | +0.0% | 3.9x |
| `binary_trees` | 778.1 | 1,621.7 | 1,621.7 | +0.0% | 2.1x |
| `bytecode_vm` | 457.1 | 884.0 | 884.0 | +0.0% | 1.9x |
| `csv_aggregate` | 1,329.8 | 4,385.5 | 4,387.1 | -0.0% | 3.3x |
| `docdb` | 1,365.0 | 6,863.8 | 6,866.1 | -0.0% | 5.0x |
| `file_io` | 536.7 | 2,990.8 | 2,998.5 | -0.3% | 5.6x |
| `graph_paths` | 557.4 | 3,249.8 | 3,249.8 | +0.0% | 5.8x |
| `hashing` | 182.6 | 388.1 | 388.1 | +0.0% | 2.1x |
| `http_api` | 175.7 | 455.0 | 454.8 | +0.0% | 2.6x |
| `image_filters` | 297.1 | 817.6 | 817.6 | +0.0% | 2.8x |
| `interpreter` | 4,476.6 | 14,232.8 | 14,232.8 | +0.0% | 3.2x |
| `json_roundtrip` | 1,459.0 | 4,445.5 | 4,445.4 | +0.0% | 3.0x |
| `kv_server` | 379.4 | 1,896.9 | 1,897.6 | -0.0% | 5.0x |
| `lru_cache` | 239.3 | 769.6 | 770.5 | -0.1% | 3.2x |
| `lzw_compress` | 273.2 | 2,085.6 | 2,085.5 | +0.0% | 7.6x |
| `markdown_render` | 320.9 | 2,345.0 | 2,345.0 | +0.0% | 7.3x |
| `mini_browser` | 729.2 | 4,694.0 | 4,693.8 | +0.0% | 6.4x |
| `nbody` | 120.1 | 964.2 | 964.2 | +0.0% | 8.0x |
| `order_book` | 19.8 | 149.6 | 149.6 | +0.0% | 7.6x |
| `pubsub_broker` | 846.8 | 4,204.3 | 4,205.0 | -0.0% | 5.0x |
| `raytracer` | 174.5 | 1,064.2 | 1,064.2 | +0.0% | 6.1x |
| `regex_match` | 1,200.7 | 5,795.5 | 5,795.5 | +0.0% | 4.8x |
| `sort_records` | 468.0 | 2,407.4 | 2,407.4 | +0.0% | 5.1x |
| `spreadsheet` | 179.2 | 629.0 | 629.0 | +0.0% | 3.5x |
| `sql_engine` | 437.4 | 2,200.0 | 2,200.0 | +0.0% | 5.0x |
| `task_pool` | 532.9 | 2,002.9 | 2,035.6 | -1.6% | 3.8x |
| `template_render` | 3,170.5 | 8,845.8 | 8,845.8 | +0.0% | 2.8x |
| `text_diff` | 497.7 | 1,192.7 | 1,192.7 | +0.0% | 2.4x |
| `word_freq` | 1,105.8 | 3,410.3 | 3,410.3 | +0.0% | 3.1x |

The 30 programs run 88,341 million instructions, against 88,387 million
with 0.11.0. The median program runs 4.4 times Rust's instructions.
