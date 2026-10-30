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

Measured on 30 October 2026 at the 0.10.0 release candidate, after phase 5
of the plan to bring Aura to Rust speed, in a Linux x86-64 container with 4
vCPUs (Intel Xeon, 2.1 GHz), Python 3.13.17, and Rust 1.95.0. Each time is
the median of ten whole-process runs at the bench size, pooled from two
passes, so it includes process startup and, for Aura, the runtime's task
scheduler. Lower ratios are better for Aura. The Aura 0.9.0 column is the
same program built by the 0.9.0 compiler, measured in the same passes on
the same machine. The container ran slower that day than when 0.9.0 was
released: 0.9.0 took 26.1 seconds for all 30 programs instead of 23.5.
`file_io` syncs a file to disk in all three versions, and the threaded
programs (`http_api`, `kv_server`, `pubsub_broker`, `task_pool`) and the
shortest ones (`bytecode_vm`, `nbody`, `order_book`) move by 20 to 40%
from run to run: their instruction counts did not change between 0.9.0
and 0.10.0 by more than the wall times suggest.

| Program | Size | Python ms | Rust ms | Aura ms | Aura 0.9.0 ms | Aura / Python | Aura / Rust |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| `astar_pathfinding` | 500 | 2,473 | 91 | 282 | 274 | 0.11x | 3.1x |
| `bank_ledger` | 200000 | 371 | 29 | 157 | 224 | 0.42x | 5.4x |
| `binary_trees` | 14 | 329 | 67 | 247 | 620 | 0.75x | 3.7x |
| `bytecode_vm` | 200000 | 2,385 | 46 | 71 | 55 | 0.03x | 1.5x |
| `csv_aggregate` | 200000 | 521 | 152 | 658 | 618 | 1.26x | 4.3x |
| `docdb` | 50000 | 631 | 393 | 2,032 | 2,265 | 3.22x | 5.2x |
| `file_io` | 300000 | 1,328 | 834 | 2,300 | 2,323 | 1.73x | 2.8x |
| `graph_paths` | 200000 | 4,871 | 237 | 908 | 2,484 | 0.19x | 3.8x |
| `hashing` | 2000000 | 1,660 | 44 | 58 | 62 | 0.04x | 1.3x |
| `http_api` | 3000 | 1,290 | 314 | 312 | 379 | 0.24x | 1.0x |
| `image_filters` | 1000 | 3,085 | 54 | 73 | 76 | 0.02x | 1.3x |
| `interpreter` | 20000 | 3,752 | 340 | 1,263 | 1,535 | 0.34x | 3.7x |
| `json_roundtrip` | 50 | 494 | 154 | 959 | 1,110 | 1.94x | 6.2x |
| `kv_server` | 50000 | 4,047 | 1,987 | 2,369 | 2,306 | 0.59x | 1.2x |
| `lru_cache` | 200000 | 279 | 28 | 100 | 122 | 0.36x | 3.5x |
| `lzw_compress` | 1000000 | 535 | 42 | 265 | 290 | 0.49x | 6.3x |
| `markdown_render` | 20000 | 905 | 63 | 325 | 387 | 0.36x | 5.1x |
| `mini_browser` | 500 | 1,013 | 215 | 940 | 1,509 | 0.93x | 4.4x |
| `nbody` | 300000 | 1,601 | 16 | 99 | 81 | 0.06x | 6.3x |
| `order_book` | 100000 | 190 | 7 | 32 | 27 | 0.17x | 4.8x |
| `pubsub_broker` | 50000 | 4,381 | 2,158 | 2,935 | 2,948 | 0.67x | 1.4x |
| `raytracer` | 500 | 3,250 | 26 | 106 | 131 | 0.03x | 4.1x |
| `regex_match` | 5000 | 1,910 | 89 | 767 | 922 | 0.40x | 8.6x |
| `sort_records` | 100000 | 1,079 | 107 | 424 | 495 | 0.39x | 3.9x |
| `spreadsheet` | 1200 | 1,015 | 37 | 161 | 255 | 0.16x | 4.3x |
| `sql_engine` | 10000 | 712 | 57 | 336 | 1,234 | 0.47x | 5.9x |
| `task_pool` | 100000 | 950 | 283 | 592 | 584 | 0.62x | 2.1x |
| `template_render` | 10000 | 707 | 315 | 1,343 | 1,985 | 1.90x | 4.3x |
| `text_diff` | 20000 | 1,987 | 96 | 184 | 175 | 0.09x | 1.9x |
| `word_freq` | 500000 | 821 | 176 | 582 | 589 | 0.71x | 3.3x |

All 30 Aura programs take 20.9 seconds, down from 26.1 seconds with 0.9.0
in the same passes. Python takes 49 seconds and Rust 8.5 seconds. The
median program takes 0.40 times Python's time, down from 0.50 times, and
3.9 times Rust's, down from 4.6 times. Aura is faster than Python on 25
programs. It is slower on `csv_aggregate`, `docdb`, `file_io`,
`json_roundtrip`, and `template_render`.

Phase 5 took more values out of boxes, cut the cost of the boxes left, and
stopped copying values a function only reads:

- A payload-free variant such as `Tree.Leaf` is one shared box, and a new
  variant's box carries its type's tag, so `binary_trees` builds only its
  inner nodes (9.2x Rust with 0.9.0, 3.7x now).
- A box remembers its last type tag, so generated code skips a repeated
  tag call (`regex_match`, `interpreter`).
- `str.from_bytes(bytes[a:b])` reads the bytes in place, and a parse
  matched on `Ok` keeps only the value (`sql_engine`, `csv_aggregate`).
- Comparisons and tests on instance fields, and fields reached through a
  `view` of a list element, read the field where it lies, and appends to
  an element of a list change it in place (`mini_browser`,
  `template_render`, `graph_paths`).
- A new allocator keeps freed small blocks on per-thread lists and cuts
  new ones from per-thread chunks, so allocation takes under 3% of the
  instructions of 29 programs instead of up to 35% (`sql_engine`,
  `graph_paths`, `sort_records`).
- A field passed to a function that only reads its arguments, a `match`
  on an element of a `view`, and an element read beside a write to the
  same list no longer copy the value (`sql_engine` 21.8x Rust with 0.9.0,
  5.9x now; `graph_paths` 10.5x to 3.8x).

`regex_match` (8.6x Rust) is the one program above 8x; its matcher builds
and compares small enum values for each step. `nbody`, `lzw_compress`
(6.3x), and `json_roundtrip` (6.2x) come next. `task_pool` runs in 2.1
times Rust's time with four workers: tasks stay on the worker they start
on, so pipeline stages on different workers hand off each value across
cores.

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
