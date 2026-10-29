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

Measured on 29 October 2026 at the 0.9.0 release candidate, after phases 2,
3, and 4 of the plan to bring Aura to Rust speed, in a Linux x86-64 container
with 4 vCPUs (Intel Xeon, 2.1 GHz), Python 3.13.17, and Rust 1.95.0. Each
time is the median of five whole-process runs at the bench size, so it
includes process startup and, for Aura, the runtime's task scheduler. Lower
ratios are better for Aura. The Aura 0.8.0 column is the same program built
by the 0.8.0 compiler, measured on 27 October 2026 on the same machine.
`file_io` syncs a file to disk in all three versions, and the threaded
programs (`http_api`, `kv_server`, `pubsub_broker`, `task_pool`) move by 10
to 20% from run to run.

| Program | Size | Python ms | Rust ms | Aura ms | Aura 0.8.0 ms | Aura / Python | Aura / Rust |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| `astar_pathfinding` | 500 | 2,244 | 83 | 264 | 875 | 0.12x | 3.2x |
| `bank_ledger` | 200000 | 327 | 36 | 190 | 892 | 0.58x | 5.2x |
| `binary_trees` | 14 | 326 | 54 | 566 | 2,715 | 1.74x | 10.5x |
| `bytecode_vm` | 200000 | 2,173 | 39 | 57 | 1,057 | 0.03x | 1.4x |
| `csv_aggregate` | 200000 | 478 | 132 | 600 | 1,398 | 1.25x | 4.6x |
| `docdb` | 50000 | 607 | 394 | 2,117 | 3,360 | 3.49x | 5.4x |
| `file_io` | 300000 | 947 | 649 | 2,044 | 2,799 | 2.16x | 3.2x |
| `graph_paths` | 200000 | 5,046 | 268 | 2,396 | 37,183 | 0.47x | 8.9x |
| `hashing` | 2000000 | 1,399 | 42 | 58 | 67 | 0.04x | 1.4x |
| `http_api` | 3000 | 1,160 | 314 | 317 | 1,179 | 0.27x | 1.0x |
| `image_filters` | 1000 | 2,621 | 48 | 66 | 4,657 | 0.03x | 1.4x |
| `interpreter` | 20000 | 3,228 | 309 | 1,277 | 29,407 | 0.40x | 4.1x |
| `json_roundtrip` | 50 | 418 | 140 | 928 | 1,823 | 2.22x | 6.6x |
| `kv_server` | 50000 | 3,672 | 1,782 | 2,171 | 3,280 | 0.59x | 1.2x |
| `lru_cache` | 200000 | 248 | 24 | 140 | 844 | 0.56x | 6.0x |
| `lzw_compress` | 1000000 | 579 | 38 | 250 | 376 | 0.43x | 6.6x |
| `markdown_render` | 20000 | 834 | 63 | 451 | 655 | 0.54x | 7.2x |
| `mini_browser` | 500 | 985 | 194 | 1,451 | 4,694 | 1.47x | 7.5x |
| `nbody` | 300000 | 1,407 | 16 | 74 | 422 | 0.05x | 4.7x |
| `order_book` | 100000 | 144 | 6 | 23 | 626 | 0.16x | 4.1x |
| `pubsub_broker` | 50000 | 4,135 | 1,908 | 2,700 | 6,464 | 0.65x | 1.4x |
| `raytracer` | 500 | 2,736 | 26 | 111 | 13,479 | 0.04x | 4.4x |
| `regex_match` | 5000 | 1,582 | 77 | 803 | 9,323 | 0.51x | 10.5x |
| `sort_records` | 100000 | 926 | 90 | 476 | 1,407 | 0.51x | 5.3x |
| `spreadsheet` | 1200 | 785 | 30 | 226 | 1,583 | 0.29x | 7.6x |
| `sql_engine` | 10000 | 575 | 47 | 1,045 | 7,497 | 1.82x | 22.2x |
| `task_pool` | 100000 | 738 | 211 | 501 | 2,812 | 0.68x | 2.4x |
| `template_render` | 10000 | 626 | 252 | 1,575 | 4,251 | 2.52x | 6.3x |
| `text_diff` | 20000 | 1,736 | 79 | 155 | 223 | 0.09x | 2.0x |
| `word_freq` | 500000 | 617 | 149 | 496 | 1,823 | 0.80x | 3.3x |

All 30 Aura programs take 23.5 seconds, down from 147 seconds with 0.8.0 and
227 seconds with 0.7.1. Python takes 43 seconds and Rust 7.5 seconds. The
median program takes 0.53 times Python's time, down from 2.7 times, and 4.6
times Rust's, down from 16 times. Aura is now faster than Python on 22
programs. It is slower on `binary_trees`, `csv_aggregate`, `docdb`,
`file_io`, `json_roundtrip`, `mini_browser`, `sql_engine`, and
`template_render`.

These changes removed the largest costs that 0.8.0 had:

- Class instances, records, tuples, enum payloads, and unions stay in typed
  native layouts, and a read from a list, a field, or a `match` lends the
  value instead of copying it into a new box (`graph_paths`, `raytracer`,
  `interpreter`, `order_book`).
- Strings keep short text in place, string operations read their text where
  it is, and an f-string builds one text (`template_render`,
  `markdown_render`, `regex_match`).
- A dictionary lookup probes once and lends the entry it finds, and JSON
  parses and writes without building a second tree (`docdb`,
  `json_roundtrip`, `word_freq`).
- A call pushes and pops its frame record with one compare each.
- Socket lines are read a 4 KiB chunk at a time, HTTP is parsed on the
  calling task, and queue hand-offs build no variant box and wake a worker
  only when it sleeps (`kv_server`, `http_api`, `pubsub_broker`,
  `task_pool`).

`task_pool` runs in 2.4 times Rust's time with four workers and 1.4 times
with one. The rest is moving each value between cores: tasks stay on the
worker they start on, so pipeline stages on different workers hand off
each value across cores. `sql_engine` (22x Rust) and `binary_trees` (10x)
still spend most of their time allocating and freeing boxes for strings,
enums, and class instances; taking those out of boxes is the next phase of
the plan.

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
