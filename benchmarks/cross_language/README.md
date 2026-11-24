# Cross-language programs

Thirty-one complete programs, each written in Aura, Python, and Rust, that do the
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
| `date_time` | The `time` module over generated instants: RFC 3339 text out and back, Duration arithmetic, calendar fields, and a log layout out and back |
| `docdb` | A schemaless JSON document database with a primary and a secondary index, saved and reloaded as JSON lines |
| `file_io` | Writes and reads a line log, a binary record file, and a directory of small files |
| `graph_paths` | BFS, Dijkstra with a binary heap, and union-find on a random graph |
| `hashing` | FNV-1a, CRC-32, Adler-32, and content-defined chunking over a byte buffer |
| `http_api` | An HTTP JSON item service and a client, over loopback |
| `http_stream` | An HTTP client reading a chunked body in line batches, server-sent events one at a time, and small replies, all over one kept connection |
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
| `regex_scan` | The `regex` module over generated log lines: a test, groups, every match with its position, a rewrite, and a split |
| `sort_records` | Built-in sorts, key sorts, string sorts, and hand-written quicksort and merge sort |
| `spreadsheet` | Formulas, a dependency graph in topological order, and recalculation after edits |
| `sql_engine` | A SQL tokenizer, parser, planner, and executor with an id index, range scans, updates, deletes, and `GROUP BY` |
| `stream_io` | Reads a log in line batches and in 64 KiB chunks, with gzip and HMAC-SHA256 on each chunk |
| `task_pool` | A worker pool on bounded queues, a three-stage pipeline, and a fan-out of tasks |
| `template_render` | A Mustache-style template engine producing HTML pages |
| `text_diff` | A line diff with the Myers O(ND) algorithm, checked against an LCS table |
| `word_freq` | Tokenizes a generated corpus and counts words and bigrams |

## Rules

- Every program makes its own input from a seeded MINSTD generator,
  `state * 48271 % 2147483647`, so all three versions see the same data and no
  data files are needed. `file_io`, `stream_io`, and `docdb` also take a work
  directory, which the script creates fresh for each run.
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

Measured on 24 November 2026 at the 0.17.0 release candidate, in a Linux
x86-64 container with 4 vCPUs (Intel Xeon, 2.1 GHz), Python 3.13.18, and
Rust 1.95.0. Each time is the median of twenty whole-process runs at the
bench size, pooled from four passes, so it includes process startup and,
for Aura, the runtime's task scheduler. Lower ratios are better for Aura.
The Aura 0.16.0 column is the same program built by the published 0.16.0
release archive, measured in the same passes on the same machine. The
program sources are the ones 0.16.0 was measured with. The container was
busy: one compiler's medians for one program differ by 10 to 50% between
passes for most programs. `file_io` syncs a file to disk in all three
versions, and its 0.16.0 medians ran from 992 to 2,093 ms between passes.

| Program | Size | Python ms | Rust ms | Aura ms | Aura 0.16.0 ms | Aura / Python | Aura / Rust |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| `astar_pathfinding` | 500 | 2,019 | 93 | 137 | 288 | 0.07x | 1.5x |
| `bank_ledger` | 200000 | 317 | 28 | 125 | 138 | 0.39x | 4.4x |
| `binary_trees` | 14 | 273 | 57 | 145 | 153 | 0.53x | 2.5x |
| `bytecode_vm` | 200000 | 1,947 | 36 | 53 | 50 | 0.03x | 1.5x |
| `csv_aggregate` | 200000 | 442 | 124 | 430 | 458 | 0.97x | 3.5x |
| `date_time` | 200000 | 4,747 | 197 | 380 | 413 | 0.08x | 1.9x |
| `docdb` | 50000 | 589 | 473 | 392 | 396 | 0.67x | 0.8x |
| `file_io` | 300000 | 939 | 647 | 1,101 | 1,786 | 1.17x | 1.7x |
| `graph_paths` | 200000 | 5,617 | 310 | 975 | 991 | 0.17x | 3.1x |
| `hashing` | 2000000 | 1,367 | 44 | 59 | 63 | 0.04x | 1.3x |
| `http_api` | 3000 | 962 | 351 | 160 | 175 | 0.17x | 0.5x |
| `http_stream` | 400000 | 5,282 | 623 | 964 | 947 | 0.18x | 1.5x |
| `image_filters` | 1000 | 2,539 | 51 | 69 | 76 | 0.03x | 1.4x |
| `interpreter` | 20000 | 2,587 | 302 | 914 | 978 | 0.35x | 3.0x |
| `json_roundtrip` | 50 | 417 | 149 | 290 | 288 | 0.70x | 1.9x |
| `kv_server` | 50000 | 2,702 | 2,102 | 473 | 484 | 0.17x | 0.2x |
| `lru_cache` | 200000 | 203 | 24 | 84 | 90 | 0.41x | 3.5x |
| `lzw_compress` | 1000000 | 438 | 40 | 220 | 235 | 0.50x | 5.6x |
| `markdown_render` | 20000 | 785 | 61 | 267 | 275 | 0.34x | 4.4x |
| `mini_browser` | 500 | 843 | 209 | 622 | 656 | 0.74x | 3.0x |
| `nbody` | 300000 | 1,259 | 15 | 32 | 32 | 0.03x | 2.2x |
| `order_book` | 100000 | 135 | 6 | 22 | 23 | 0.16x | 3.4x |
| `pubsub_broker` | 50000 | 3,088 | 2,332 | 799 | 870 | 0.26x | 0.3x |
| `raytracer` | 500 | 2,461 | 25 | 84 | 89 | 0.03x | 3.4x |
| `regex_match` | 5000 | 1,435 | 82 | 430 | 437 | 0.30x | 5.2x |
| `regex_scan` | 200000 | 1,712 | 297 | 964 | 951 | 0.56x | 3.2x |
| `sort_records` | 100000 | 1,021 | 100 | 345 | 364 | 0.34x | 3.5x |
| `spreadsheet` | 1200 | 806 | 32 | 136 | 161 | 0.17x | 4.2x |
| `sql_engine` | 10000 | 561 | 48 | 289 | 306 | 0.51x | 6.0x |
| `stream_io` | 400000 | 887 | 372 | 718 | 720 | 0.81x | 1.9x |
| `task_pool` | 100000 | 688 | 244 | 353 | 327 | 0.51x | 1.4x |
| `template_render` | 10000 | 535 | 241 | 759 | 764 | 1.42x | 3.2x |
| `text_diff` | 20000 | 1,551 | 81 | 148 | 143 | 0.10x | 1.8x |
| `word_freq` | 500000 | 663 | 204 | 542 | 523 | 0.82x | 2.7x |

All 34 Aura programs take 13.5 seconds, against 51.8 seconds for Python
and 10.0 seconds for Rust. The median program takes 0.34 times Python's
time and 2.6 times Rust's. Aura is faster than Python on 32 programs and
slower on `file_io` and `template_render`; `csv_aggregate` is level with
Python. No program is above 8x Rust; the highest is `sql_engine` at 6.0x.

With 0.16.0 the same programs take 14.6 seconds in the same passes, and
their median is 2.9 times Rust's time. Two things separate the columns:

- `astar_pathfinding` takes 137 ms against 288. Its heap swaps two list
  elements for every step up or down. Built by 0.16.0 the program calls
  the runtime for each swap, and built by 0.17.0 it does not.
- `file_io` reads 1,101 ms against 1,786 and runs the same instructions
  with either compiler. It waits on the disk, and the 0.16.0 passes ran
  while the disk was slower.

The other programs run within 3.2% of their 0.16.0 instructions, so the
rest of what the two columns differ by is what the container adds.

### Instruction counts

Instructions executed under callgrind at the bench size, in millions, one
run each, measured on 24 November 2026 for the 0.17.0 release candidate
against the published 0.16.0 archive. They vary much less from run to run
than times do. The threaded programs (`http_api`, `kv_server`,
`pubsub_broker`, `task_pool`) and `file_io` still move by up to a
percent.

| Program | Rust M | Aura M | Aura 0.16.0 M | Change | Aura / Rust |
| --- | ---: | ---: | ---: | ---: | ---: |
| `astar_pathfinding` | 300.4 | 901.7 | 2,138.0 | -57.8% | 3.0x |
| `bank_ledger` | 316.0 | 1,334.5 | 1,331.9 | +0.2% | 4.2x |
| `binary_trees` | 778.1 | 1,786.1 | 1,783.1 | +0.2% | 2.3x |
| `bytecode_vm` | 457.1 | 868.1 | 868.1 | +0.0% | 1.9x |
| `csv_aggregate` | 1,328.8 | 4,078.9 | 4,099.6 | -0.5% | 3.1x |
| `date_time` | 1,998.2 | 4,226.4 | 4,252.8 | -0.6% | 2.1x |
| `docdb` | 1,365.4 | 1,981.2 | 1,969.9 | +0.6% | 1.5x |
| `file_io` | 536.7 | 2,889.5 | 2,892.3 | -0.1% | 5.4x |
| `graph_paths` | 557.5 | 3,059.9 | 3,056.3 | +0.1% | 5.5x |
| `hashing` | 182.6 | 271.4 | 271.2 | +0.1% | 1.5x |
| `http_api` | 175.6 | 549.9 | 547.3 | +0.5% | 3.1x |
| `http_stream` | 1,334.9 | 8,833.0 | 8,797.1 | +0.4% | 6.6x |
| `image_filters` | 297.1 | 753.9 | 753.9 | +0.0% | 2.5x |
| `interpreter` | 4,476.3 | 13,180.7 | 12,904.2 | +2.1% | 2.9x |
| `json_roundtrip` | 1,459.0 | 2,400.8 | 2,401.3 | -0.0% | 1.6x |
| `kv_server` | 379.6 | 1,673.1 | 1,673.3 | -0.0% | 4.4x |
| `lru_cache` | 240.1 | 792.1 | 788.7 | +0.4% | 3.3x |
| `lzw_compress` | 273.2 | 1,946.6 | 2,011.4 | -3.2% | 7.1x |
| `markdown_render` | 320.9 | 2,275.8 | 2,282.9 | -0.3% | 7.1x |
| `mini_browser` | 735.1 | 4,521.6 | 4,519.6 | +0.0% | 6.2x |
| `nbody` | 120.1 | 447.0 | 447.0 | +0.0% | 3.7x |
| `order_book` | 19.8 | 156.0 | 154.1 | +1.2% | 7.9x |
| `pubsub_broker` | 847.2 | 3,770.3 | 3,771.2 | -0.0% | 4.5x |
| `raytracer` | 174.6 | 871.1 | 858.3 | +1.5% | 5.0x |
| `regex_match` | 1,200.8 | 4,954.1 | 4,948.6 | +0.1% | 4.1x |
| `regex_scan` | 3,492.0 | 9,198.1 | 9,220.3 | -0.2% | 2.6x |
| `sort_records` | 467.8 | 2,294.0 | 2,357.5 | -2.7% | 4.9x |
| `spreadsheet` | 179.2 | 617.9 | 614.0 | +0.6% | 3.4x |
| `sql_engine` | 437.3 | 2,100.5 | 2,102.6 | -0.1% | 4.8x |
| `stream_io` | 3,444.2 | 5,937.3 | 6,110.5 | -2.8% | 1.7x |
| `task_pool` | 530.3 | 1,829.5 | 1,838.0 | -0.5% | 3.4x |
| `template_render` | 3,214.2 | 8,374.2 | 8,383.1 | -0.1% | 2.6x |
| `text_diff` | 497.7 | 1,123.7 | 1,124.0 | -0.0% | 2.3x |
| `word_freq` | 1,106.1 | 3,434.5 | 3,446.3 | -0.3% | 3.1x |

The 34 programs run 103,433 million instructions, against 104,718
million with 0.16.0. The median program runs 3.4 times Rust's
instructions, against 3.4 with 0.16.0.

- `astar_pathfinding` runs 58% fewer instructions, for the swap above.
  `lzw_compress`, `stream_io`, and `sort_records` run about 3% fewer.
- `interpreter` runs 2.1% more, `raytracer` 1.5% more, and `order_book`
  1.2% more. In `interpreter` about half of the growth is the dictionary
  lookup, which now tests for one more kind of storage: the union of two
  sets, which 0.17.0 keeps as a view of both until it is written.
- Every other program is within 0.6%.
