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

Measured on 8 November 2026 at the 0.13.0 release candidate, in a Linux
x86-64 container with 4 vCPUs (Intel Xeon, 2.1 GHz), Python 3.13.18, and
Rust 1.95.0. Each time is the median of ten whole-process runs at the bench
size, pooled from two passes, so it includes process startup and, for Aura,
the runtime's task scheduler. Lower ratios are better for Aura. The Aura
0.12.0 column is the same program built by the published 0.12.0 release
archive, measured in the same passes on the same machine. The container was
much less busy that day than at the 0.12.0 release: Python took 30 seconds
for all 30 programs instead of 46, and 0.12.0 took 14.2 seconds instead of
19.9. `file_io` syncs a file to disk in all three versions, and the
shortest programs (`bytecode_vm`, `nbody`, `order_book`) move by 10 to 50%
from run to run.

| Program | Size | Python ms | Rust ms | Aura ms | Aura 0.12.0 ms | Aura / Python | Aura / Rust |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| `astar_pathfinding` | 500 | 1,499 | 69 | 203 | 208 | 0.14x | 2.9x |
| `bank_ledger` | 200000 | 236 | 22 | 108 | 114 | 0.46x | 5.0x |
| `binary_trees` | 14 | 213 | 45 | 148 | 164 | 0.70x | 3.3x |
| `bytecode_vm` | 200000 | 1,560 | 29 | 41 | 42 | 0.03x | 1.4x |
| `csv_aggregate` | 200000 | 349 | 103 | 377 | 415 | 1.08x | 3.7x |
| `docdb` | 50000 | 446 | 355 | 917 | 1,363 | 2.06x | 2.6x |
| `file_io` | 300000 | 932 | 563 | 1,081 | 1,022 | 1.16x | 1.9x |
| `graph_paths` | 200000 | 3,937 | 238 | 868 | 791 | 0.22x | 3.7x |
| `hashing` | 2000000 | 1,089 | 35 | 48 | 47 | 0.04x | 1.4x |
| `http_api` | 3000 | 716 | 267 | 108 | 223 | 0.15x | 0.4x |
| `image_filters` | 1000 | 2,028 | 44 | 61 | 59 | 0.03x | 1.4x |
| `interpreter` | 20000 | 2,094 | 248 | 718 | 850 | 0.34x | 2.9x |
| `json_roundtrip` | 50 | 334 | 133 | 571 | 738 | 1.71x | 4.3x |
| `kv_server` | 50000 | 1,917 | 1,751 | 353 | 1,784 | 0.18x | 0.2x |
| `lru_cache` | 200000 | 157 | 19 | 66 | 66 | 0.42x | 3.5x |
| `lzw_compress` | 1000000 | 335 | 34 | 209 | 190 | 0.62x | 6.2x |
| `markdown_render` | 20000 | 627 | 50 | 230 | 253 | 0.37x | 4.7x |
| `mini_browser` | 500 | 657 | 146 | 525 | 576 | 0.80x | 3.6x |
| `nbody` | 300000 | 1,006 | 12 | 58 | 59 | 0.06x | 4.8x |
| `order_book` | 100000 | 109 | 5 | 17 | 17 | 0.15x | 3.2x |
| `pubsub_broker` | 50000 | 2,213 | 1,763 | 633 | 2,229 | 0.29x | 0.4x |
| `raytracer` | 500 | 1,928 | 20 | 75 | 81 | 0.04x | 3.8x |
| `regex_match` | 5000 | 1,139 | 66 | 430 | 427 | 0.38x | 6.5x |
| `sort_records` | 100000 | 714 | 69 | 293 | 305 | 0.41x | 4.2x |
| `spreadsheet` | 1200 | 603 | 24 | 134 | 114 | 0.22x | 5.7x |
| `sql_engine` | 10000 | 441 | 36 | 284 | 238 | 0.64x | 8.0x |
| `task_pool` | 100000 | 553 | 159 | 217 | 534 | 0.39x | 1.4x |
| `template_render` | 10000 | 429 | 188 | 684 | 717 | 1.60x | 3.6x |
| `text_diff` | 20000 | 1,212 | 63 | 120 | 123 | 0.10x | 1.9x |
| `word_freq` | 500000 | 519 | 144 | 389 | 427 | 0.75x | 2.7x |

All 30 Aura programs take 10.0 seconds, down from 14.2 seconds with 0.12.0
in the same passes. Python takes 30 seconds and Rust 6.7 seconds. The
median program takes 0.37 times Python's time and 3.4 times Rust's, down
from 0.42 and 3.6 times with 0.12.0. Aura is faster than Python on 25
programs. It is slower on `csv_aggregate`, `docdb`, `file_io`,
`json_roundtrip`, and `template_render`. No program is above 8x Rust.

`stream_io` was added on 10 November 2026, after these passes, with
`fs.File.read_lines`, `read_chunk`, the `gzip` module, and
`bytes.hmac_sha256`. On the same kind of container, at 400,000 lines, it
takes 766 ms in Aura, 938 ms in Python, and 430 ms in Rust: 0.82 times
Python's time and 1.8 times Rust's. It is not in the table or the totals
above.

0.13.0 brings the Batch 8 multicore runtime and the first JSON steps of
the lean-values work:

- Tasks start on their parent's worker and idle workers steal queued
  tasks, finished task stacks are reused, and wakes and cancellation do
  less work. The threaded programs are now faster than Rust's thread-based
  versions: `kv_server` 0.2x Rust (from 1.0x), `pubsub_broker` 0.4x (from
  1.3x), `http_api` 0.4x (from 0.8x), and `task_pool` 1.4x (from 3.4x).
- `json.parse` gathers each container's items before building it once,
  small-dict probes compare keys inline, and `json.Value` copies share
  their dict types (`docdb` -33%, 47% fewer instructions;
  `json_roundtrip` -23%, 43% fewer).
- `text += x` on a `str` local appends in place, and the iterator,
  generator, and class-local work makes its loops cheaper.
  `interpreter`, `raytracer`, `csv_aggregate`, `markdown_render`, and
  `template_render` run 9 to 14% fewer instructions.

`sql_engine` (8.0x Rust in these passes) and `spreadsheet` read 18 to 19%
slower than 0.12.0 while running fewer instructions; timed again, two
alternating passes of five runs each, they were 15% and 32% faster
(7.3x and 4.1x Rust). `graph_paths` and `lzw_compress` read 4 to 10%
slower in both, with 4 to 5% fewer instructions.

### Instruction counts

Instructions executed under callgrind at the bench size, in millions, one
run each, measured on 8 November 2026 for the 0.13.0 release candidate
against the published 0.12.0 archive. They vary much less from run to run
than times do. The threaded programs (`http_api`, `kv_server`,
`pubsub_broker`, `task_pool`) still move by a few percent.

| Program | Rust M | Aura M | Aura 0.12.0 M | Change | Aura / Rust |
| --- | ---: | ---: | ---: | ---: | ---: |
| `astar_pathfinding` | 300.4 | 2,097.4 | 2,115.1 | -0.8% | 7.0x |
| `bank_ledger` | 315.9 | 1,243.9 | 1,234.2 | +0.8% | 3.9x |
| `binary_trees` | 778.1 | 1,616.9 | 1,621.7 | -0.3% | 2.1x |
| `bytecode_vm` | 457.1 | 883.9 | 884.0 | -0.0% | 1.9x |
| `csv_aggregate` | 1,329.8 | 3,897.6 | 4,387.1 | -11.2% | 2.9x |
| `docdb` | 1,365.3 | 3,626.9 | 6,861.8 | -47.1% | 2.7x |
| `file_io` | 536.7 | 2,849.1 | 3,073.1 | -7.3% | 5.3x |
| `graph_paths` | 557.5 | 3,085.3 | 3,249.8 | -5.1% | 5.5x |
| `hashing` | 182.6 | 386.7 | 388.1 | -0.4% | 2.1x |
| `http_api` | 177.7 | 413.3 | 452.8 | -8.7% | 2.3x |
| `image_filters` | 297.1 | 817.6 | 817.6 | +0.0% | 2.8x |
| `interpreter` | 4,476.6 | 12,444.1 | 14,232.8 | -12.6% | 2.8x |
| `json_roundtrip` | 1,459.0 | 2,549.9 | 4,445.5 | -42.6% | 1.7x |
| `kv_server` | 379.1 | 1,630.1 | 1,896.4 | -14.0% | 4.3x |
| `lru_cache` | 239.8 | 763.6 | 769.9 | -0.8% | 3.2x |
| `lzw_compress` | 273.2 | 2,011.2 | 2,085.6 | -3.6% | 7.4x |
| `markdown_render` | 320.9 | 2,122.9 | 2,345.0 | -9.5% | 6.6x |
| `mini_browser` | 726.7 | 4,601.8 | 4,694.4 | -2.0% | 6.3x |
| `nbody` | 120.1 | 964.2 | 964.2 | +0.0% | 8.0x |
| `order_book` | 19.8 | 153.7 | 149.6 | +2.7% | 7.8x |
| `pubsub_broker` | 846.9 | 3,760.5 | 4,204.8 | -10.6% | 4.4x |
| `raytracer` | 174.5 | 919.6 | 1,064.2 | -13.6% | 5.3x |
| `regex_match` | 1,200.7 | 5,835.2 | 5,795.5 | +0.7% | 4.9x |
| `sort_records` | 468.0 | 2,310.3 | 2,408.2 | -4.1% | 4.9x |
| `spreadsheet` | 179.2 | 615.3 | 629.0 | -2.2% | 3.4x |
| `sql_engine` | 437.3 | 2,061.3 | 2,200.0 | -6.3% | 4.7x |
| `task_pool` | 531.7 | 1,803.1 | 2,128.9 | -15.3% | 3.4x |
| `template_render` | 3,210.7 | 8,049.5 | 8,845.8 | -9.0% | 2.5x |
| `text_diff` | 497.7 | 1,159.9 | 1,192.7 | -2.8% | 2.3x |
| `word_freq` | 1,105.8 | 3,372.5 | 3,410.2 | -1.1% | 3.0x |

The 30 programs run 78,048 million instructions, down from 88,548 million
with 0.12.0. The median program runs 3.7 times Rust's instructions, down
from 4.4 times.
