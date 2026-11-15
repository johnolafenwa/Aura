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

Measured on 14 November 2026 at the 0.16.0 release candidate, in a Linux
x86-64 container with 4 vCPUs (Intel Xeon, 2.1 GHz), Python 3.13.18, and
Rust 1.95.0. Each time is the median of twenty whole-process runs at the
bench size, pooled from four passes, so it includes process startup and,
for Aura, the runtime's task scheduler. Lower ratios are better for Aura.
The Aura 0.15.0 column is the same program built by the published 0.15.0
release archive, measured in the same passes on the same machine. The
program sources are the ones 0.15.0 was measured with. The container was
busy: one compiler's medians for one program differ by 10 to 30% between
passes for `astar_pathfinding`, `bank_ledger`, `mini_browser`,
`raytracer`, `regex_match`, and the threaded programs (`http_api`,
`kv_server`, `pubsub_broker`, `task_pool`). `file_io` syncs a file to
disk in all three versions, and its 0.15.0 medians ran from 679 to 1,147
ms between passes.

| Program | Size | Python ms | Rust ms | Aura ms | Aura 0.15.0 ms | Aura / Python | Aura / Rust |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| `astar_pathfinding` | 500 | 1,770 | 80 | 226 | 232 | 0.13x | 2.8x |
| `bank_ledger` | 200000 | 275 | 25 | 119 | 116 | 0.43x | 4.8x |
| `binary_trees` | 14 | 262 | 54 | 140 | 140 | 0.53x | 2.6x |
| `bytecode_vm` | 200000 | 1,817 | 34 | 46 | 47 | 0.03x | 1.4x |
| `csv_aggregate` | 200000 | 405 | 115 | 403 | 434 | 0.99x | 3.5x |
| `date_time` | 200000 | 4,187 | 183 | 355 | 360 | 0.08x | 1.9x |
| `docdb` | 50000 | 490 | 333 | 293 | 284 | 0.60x | 0.9x |
| `file_io` | 300000 | 831 | 496 | 1,059 | 855 | 1.27x | 2.1x |
| `graph_paths` | 200000 | 4,205 | 230 | 703 | 718 | 0.17x | 3.1x |
| `hashing` | 2000000 | 1,228 | 41 | 56 | 57 | 0.05x | 1.4x |
| `http_api` | 3000 | 857 | 301 | 141 | 146 | 0.16x | 0.5x |
| `http_stream` | 400000 | 4,723 | 587 | 896 | 875 | 0.19x | 1.5x |
| `image_filters` | 1000 | 2,339 | 46 | 62 | 61 | 0.03x | 1.3x |
| `interpreter` | 20000 | 2,367 | 287 | 846 | 837 | 0.36x | 2.9x |
| `json_roundtrip` | 50 | 365 | 135 | 255 | 250 | 0.70x | 1.9x |
| `kv_server` | 50000 | 2,124 | 1,882 | 438 | 427 | 0.21x | 0.2x |
| `lru_cache` | 200000 | 185 | 22 | 75 | 73 | 0.40x | 3.4x |
| `lzw_compress` | 1000000 | 391 | 37 | 200 | 196 | 0.51x | 5.4x |
| `markdown_render` | 20000 | 725 | 57 | 256 | 250 | 0.35x | 4.5x |
| `mini_browser` | 500 | 719 | 164 | 537 | 552 | 0.75x | 3.3x |
| `nbody` | 300000 | 1,122 | 15 | 28 | 28 | 0.03x | 1.9x |
| `order_book` | 100000 | 123 | 6 | 20 | 20 | 0.16x | 3.4x |
| `pubsub_broker` | 50000 | 2,515 | 1,988 | 713 | 706 | 0.28x | 0.4x |
| `raytracer` | 500 | 2,193 | 23 | 81 | 79 | 0.04x | 3.5x |
| `regex_match` | 5000 | 1,326 | 78 | 416 | 402 | 0.31x | 5.3x |
| `regex_scan` | 200000 | 1,532 | 276 | 895 | 892 | 0.58x | 3.2x |
| `sort_records` | 100000 | 828 | 84 | 318 | 324 | 0.38x | 3.8x |
| `spreadsheet` | 1200 | 722 | 29 | 129 | 134 | 0.18x | 4.5x |
| `sql_engine` | 10000 | 528 | 42 | 266 | 261 | 0.50x | 6.4x |
| `stream_io` | 400000 | 789 | 344 | 633 | 627 | 0.80x | 1.8x |
| `task_pool` | 100000 | 622 | 217 | 279 | 247 | 0.45x | 1.3x |
| `template_render` | 10000 | 506 | 219 | 700 | 718 | 1.38x | 3.2x |
| `text_diff` | 20000 | 1,393 | 72 | 125 | 124 | 0.09x | 1.7x |
| `word_freq` | 500000 | 560 | 161 | 432 | 444 | 0.77x | 2.7x |

All 34 Aura programs take 12.1 seconds, against 45.0 seconds for Python
and 8.7 seconds for Rust. The median program takes 0.36 times Python's
time and 2.8 times Rust's. Aura is faster than Python on 32 programs and
slower on `file_io` and `template_render`; `csv_aggregate` is level with
Python. No program is above 8x Rust; the highest is `sql_engine` at 6.4x.

With 0.15.0 the same programs take 11.9 seconds in the same passes, and
their median is 2.8 times Rust's time. 0.16.0 adds GPU tensors, Windows,
Linux on arm64, other Linux targets, and freestanding builds, and changes
no code that these programs run, so the two columns differ by what the
container adds:

- `file_io` reads 1,059 ms against 855 and `task_pool` 279 ms against
  247. Both run the same instructions with either compiler. `file_io`
  waits on the disk, and `task_pool` on four threads.
- `csv_aggregate` reads 403 ms against 434. It also runs the same
  instructions.
- Every other program is within 4% of its 0.15.0 time.

### Instruction counts

Instructions executed under callgrind at the bench size, in millions, one
run each, measured on 14 and 15 November 2026 for the 0.16.0 release
candidate against the published 0.15.0 archive. They vary much less from
run to run than times do. The threaded programs (`http_api`, `kv_server`,
`pubsub_broker`, `task_pool`) and `file_io` still move by up to a
percent.

| Program | Rust M | Aura M | Aura 0.15.0 M | Change | Aura / Rust |
| --- | ---: | ---: | ---: | ---: | ---: |
| `astar_pathfinding` | 300.4 | 2,138.0 | 2,138.0 | +0.0% | 7.1x |
| `bank_ledger` | 316.0 | 1,331.9 | 1,331.9 | -0.0% | 4.2x |
| `binary_trees` | 778.1 | 1,783.0 | 1,783.0 | -0.0% | 2.3x |
| `bytecode_vm` | 457.1 | 868.1 | 868.1 | +0.0% | 1.9x |
| `csv_aggregate` | 1,328.8 | 4,100.0 | 4,099.6 | +0.0% | 3.1x |
| `date_time` | 1,998.2 | 4,252.8 | 4,256.8 | -0.1% | 2.1x |
| `docdb` | 1,365.4 | 1,969.8 | 1,969.9 | -0.0% | 1.4x |
| `file_io` | 536.7 | 2,902.2 | 2,901.8 | +0.0% | 5.4x |
| `graph_paths` | 557.5 | 3,056.4 | 3,056.3 | +0.0% | 5.5x |
| `hashing` | 182.6 | 271.2 | 271.2 | -0.0% | 1.5x |
| `http_api` | 176.4 | 547.3 | 547.3 | -0.0% | 3.1x |
| `http_stream` | 1,335.9 | 8,797.4 | 8,800.9 | -0.0% | 6.6x |
| `image_filters` | 297.1 | 753.9 | 753.9 | +0.0% | 2.5x |
| `interpreter` | 4,476.6 | 12,904.1 | 12,904.1 | +0.0% | 2.9x |
| `json_roundtrip` | 1,458.8 | 2,401.5 | 2,401.5 | -0.0% | 1.6x |
| `kv_server` | 379.2 | 1,672.5 | 1,672.9 | -0.0% | 4.4x |
| `lru_cache` | 240.9 | 789.3 | 788.4 | +0.1% | 3.3x |
| `lzw_compress` | 273.2 | 2,011.4 | 2,011.3 | +0.0% | 7.4x |
| `markdown_render` | 320.9 | 2,282.1 | 2,283.1 | -0.0% | 7.1x |
| `mini_browser` | 727.8 | 4,519.7 | 4,519.1 | +0.0% | 6.2x |
| `nbody` | 120.1 | 447.0 | 447.0 | -0.0% | 3.7x |
| `order_book` | 19.8 | 154.1 | 154.1 | +0.0% | 7.8x |
| `pubsub_broker` | 847.3 | 3,768.9 | 3,769.6 | -0.0% | 4.4x |
| `raytracer` | 174.5 | 858.3 | 858.3 | +0.0% | 4.9x |
| `regex_match` | 1,200.7 | 4,948.6 | 4,948.6 | -0.0% | 4.1x |
| `regex_scan` | 3,492.0 | 9,220.3 | 9,221.4 | -0.0% | 2.6x |
| `sort_records` | 468.0 | 2,358.3 | 2,357.5 | +0.0% | 5.0x |
| `spreadsheet` | 179.2 | 614.0 | 614.0 | -0.0% | 3.4x |
| `sql_engine` | 437.3 | 2,102.6 | 2,102.6 | +0.0% | 4.8x |
| `stream_io` | 3,444.2 | 6,112.0 | 6,111.9 | +0.0% | 1.8x |
| `task_pool` | 530.4 | 1,830.8 | 1,830.8 | +0.0% | 3.5x |
| `template_render` | 3,201.5 | 8,383.1 | 8,383.1 | -0.0% | 2.6x |
| `text_diff` | 497.7 | 1,124.0 | 1,124.0 | -0.0% | 2.3x |
| `word_freq` | 1,105.9 | 3,446.4 | 3,446.4 | +0.0% | 3.1x |

The 34 programs run 104,721 million instructions, against 104,729
million with 0.15.0. No program moves by more than 0.2%. The median
program runs 3.4 times Rust's instructions, as with 0.15.0.

The first count of the candidate showed `http_api` running 2.7% more
instructions and `http_stream` 1.9% more. The runtime finds a host
function's argument list by name in a table, once for each argument, and
the tensor functions had been added above the `http` ones. The tensor
functions are now last in the table.
