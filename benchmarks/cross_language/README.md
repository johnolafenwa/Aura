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

Measured on 31 October 2026 at the 0.11.0 release candidate, in a Linux
x86-64 container with 4 vCPUs (Intel Xeon, 2.1 GHz), Python 3.13.17, and
Rust 1.95.0. Each time is the median of ten whole-process runs at the bench
size, pooled from two passes, so it includes process startup and, for Aura,
the runtime's task scheduler. Lower ratios are better for Aura. The Aura
0.10.0 column is the same program built by the published 0.10.0 release
archive, measured in the same passes on the same machine. The container ran
faster that day than when 0.10.0 was released: 0.10.0 took 17.9 seconds for
all 30 programs instead of 20.9. `file_io` syncs a file to disk in all three
versions, and the threaded programs (`http_api`, `kv_server`,
`pubsub_broker`, `task_pool`) and the shortest ones (`bytecode_vm`,
`nbody`, `order_book`) move by 10 to 40% from run to run.

| Program | Size | Python ms | Rust ms | Aura ms | Aura 0.10.0 ms | Aura / Python | Aura / Rust |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| `astar_pathfinding` | 500 | 2,108 | 81 | 257 | 236 | 0.12x | 3.2x |
| `bank_ledger` | 200000 | 327 | 27 | 138 | 139 | 0.42x | 5.2x |
| `binary_trees` | 14 | 296 | 54 | 200 | 217 | 0.68x | 3.7x |
| `bytecode_vm` | 200000 | 2,074 | 39 | 51 | 53 | 0.02x | 1.3x |
| `csv_aggregate` | 200000 | 460 | 118 | 557 | 521 | 1.21x | 4.7x |
| `docdb` | 50000 | 557 | 315 | 1,317 | 1,773 | 2.36x | 4.2x |
| `file_io` | 300000 | 1,162 | 905 | 1,389 | 2,154 | 1.20x | 1.5x |
| `graph_paths` | 200000 | 4,300 | 216 | 803 | 869 | 0.19x | 3.7x |
| `hashing` | 2000000 | 1,427 | 42 | 55 | 61 | 0.04x | 1.3x |
| `http_api` | 3000 | 1,126 | 285 | 264 | 267 | 0.23x | 0.9x |
| `image_filters` | 1000 | 2,501 | 47 | 72 | 65 | 0.03x | 1.5x |
| `interpreter` | 20000 | 3,128 | 297 | 1,159 | 1,046 | 0.37x | 3.9x |
| `json_roundtrip` | 50 | 417 | 128 | 647 | 819 | 1.55x | 5.0x |
| `kv_server` | 50000 | 3,358 | 1,678 | 2,024 | 1,881 | 0.60x | 1.2x |
| `lru_cache` | 200000 | 227 | 27 | 88 | 88 | 0.39x | 3.3x |
| `lzw_compress` | 1000000 | 456 | 40 | 251 | 235 | 0.55x | 6.3x |
| `markdown_render` | 20000 | 788 | 57 | 292 | 295 | 0.37x | 5.1x |
| `mini_browser` | 500 | 858 | 167 | 753 | 817 | 0.88x | 4.5x |
| `nbody` | 300000 | 1,374 | 14 | 70 | 72 | 0.05x | 4.9x |
| `order_book` | 100000 | 142 | 6 | 20 | 20 | 0.14x | 3.4x |
| `pubsub_broker` | 50000 | 3,823 | 1,859 | 2,571 | 2,488 | 0.67x | 1.4x |
| `raytracer` | 500 | 2,730 | 27 | 96 | 111 | 0.04x | 3.6x |
| `regex_match` | 5000 | 1,615 | 83 | 544 | 613 | 0.34x | 6.5x |
| `sort_records` | 100000 | 872 | 82 | 366 | 370 | 0.42x | 4.5x |
| `spreadsheet` | 1200 | 807 | 32 | 136 | 126 | 0.17x | 4.2x |
| `sql_engine` | 10000 | 575 | 40 | 303 | 304 | 0.53x | 7.6x |
| `task_pool` | 100000 | 748 | 239 | 512 | 498 | 0.68x | 2.1x |
| `template_render` | 10000 | 605 | 234 | 895 | 1,090 | 1.48x | 3.8x |
| `text_diff` | 20000 | 1,708 | 76 | 138 | 138 | 0.08x | 1.8x |
| `word_freq` | 500000 | 604 | 157 | 463 | 500 | 0.77x | 3.0x |

All 30 Aura programs take 16.4 seconds, down from 17.9 seconds with 0.10.0
in the same passes. Python takes 41 seconds and Rust 7.4 seconds. The
median program takes 0.40 times Python's time, as with 0.10.0, and 3.7
times Rust's, down from 3.9 times. Aura is faster than Python on 25
programs. It is slower on `csv_aggregate`, `docdb`, `file_io`,
`json_roundtrip`, and `template_render`; the gap narrowed on all of them
but `csv_aggregate`. No program is above 8x Rust.

0.11.0 aimed at the programs still slower than Python:

- File calls hand off to the blocking-I/O pool for less, and a pool worker
  never blocks a task worker (`file_io` 2.4x Rust with 0.10.0, 1.5x now,
  and 1.2 times Python's time instead of 1.9).
- A read-only `match d.lookup(key)` finds the entry once and reads it in
  place, `json.as_int` and its siblings make no box, and `json.parse`
  builds the tree in one pass (`docdb` -26%, `json_roundtrip` -21%).
- A list element passed to a function that only borrows it is lent
  instead of copied.
- One-character `replace`, `count`, and `split` patterns scan with
  `memchr`, and f-string parts skip a repeated ASCII check
  (`template_render` -18%).
- A new empty list keeps the shape of its elements through scalar pushes
  (`regex_match` 7.4x Rust with 0.10.0, 6.5x now).

`sql_engine` (7.6x Rust) and `regex_match` (6.5x) are furthest from Rust.
`interpreter`, `kv_server`, `image_filters`, and `astar_pathfinding` read 8
to 11% slower than 0.10.0. Run alone again, `interpreter` and
`csv_aggregate` came within 4% of 0.10.0, inside their run-to-run spread.

### Instruction counts

Instructions executed under callgrind at the check size, in millions,
recorded for 0.8.0 on 27 October 2026 at `abd8fef` against 0.7.1. They vary much less from run to run than times do. The
threaded `http_api` still moves by up to 10%, and so can a program whose
code layout changes, such as `binary_trees`.

| Program | Rust M | Aura M | Aura 0.7.1 M | Change | Aura / Rust |
| --- | ---: | ---: | ---: | ---: | ---: |
| `astar_pathfinding` | 1.1 | 17.6 | 39.7 | -56% | 16.0x |
| `bank_ledger` | 3.5 | 41.7 | 45.8 | -9% | 11.9x |
| `binary_trees` | 1.5 | 15.1 | 15.1 | +0% | 10.2x |
| `bytecode_vm` | 2.7 | 49.6 | 64.7 | -23% | 18.7x |
| `csv_aggregate` | 8.1 | 66.4 | 102.5 | -35% | 8.2x |
| `docdb` | 5.5 | 82.6 | 83.0 | -1% | 14.9x |
| `file_io` | 2.2 | 16.1 | 74.1 | -78% | 7.4x |
| `graph_paths` | 2.7 | 564.1 | 590.3 | -4% | 207.0x |
| `hashing` | 1.6 | 12.7 | 386.7 | -97% | 8.2x |
| `http_api` | 6.4 | 32.3 | 29.6 | +9% | 5.0x |
| `image_filters` | 0.5 | 18.2 | 26.7 | -32% | 33.4x |
| `interpreter` | 8.0 | 367.9 | 372.6 | -1% | 46.1x |
| `json_roundtrip` | 99.2 | 640.7 | 644.4 | -1% | 6.5x |
| `kv_server` | 8.3 | 84.8 | 85.7 | -1% | 10.2x |
| `lru_cache` | 8.3 | 201.2 | 487.6 | -59% | 24.2x |
| `lzw_compress` | 0.8 | 5.1 | 16.6 | -69% | 6.2x |
| `markdown_render` | 0.9 | 10.3 | 81.4 | -87% | 10.8x |
| `mini_browser` | 32.4 | 1,348.0 | 2,700.2 | -50% | 41.6x |
| `nbody` | 0.8 | 15.9 | 132.1 | -88% | 19.5x |
| `order_book` | 0.8 | 112.8 | 133.4 | -15% | 146.9x |
| `pubsub_broker` | 10.6 | 149.5 | 151.1 | -1% | 14.1x |
| `raytracer` | 1.1 | 457.8 | 471.0 | -3% | 407.7x |
| `regex_match` | 24.3 | 1,914.8 | 2,356.3 | -19% | 78.8x |
| `sort_records` | 4.2 | 75.2 | 612.4 | -88% | 18.1x |
| `spreadsheet` | 1.7 | 54.2 | 103.0 | -47% | 31.6x |
| `sql_engine` | 47.4 | 2,818.5 | 4,787.3 | -41% | 59.5x |
| `task_pool` | 5.6 | 41.2 | 67.0 | -38% | 7.4x |
| `template_render` | 4.0 | 39.5 | 42.2 | -6% | 10.0x |
| `text_diff` | 1.3 | 10.2 | 14.1 | -28% | 8.1x |
| `word_freq` | 4.9 | 58.6 | 76.4 | -23% | 12.0x |

The 30 programs run 9,323 million instructions, down from 14,793 million.
The median program runs 14.5 times Rust's instructions, down from 35 times.
