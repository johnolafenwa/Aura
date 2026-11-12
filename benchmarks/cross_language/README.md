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

Measured on 10 November 2026 at the 0.14.0 release candidate, in a Linux
x86-64 container with 4 vCPUs (Intel Xeon, 2.1 GHz), Python 3.13.18, and
Rust 1.95.0. Each time is the median of ten whole-process runs at the bench
size, pooled from two passes, so it includes process startup and, for Aura,
the runtime's task scheduler. Lower ratios are better for Aura. The Aura
0.13.0 column is the same program built by the published 0.13.0 release
archive, measured in the same passes on the same machine. The container was
busier that day than at the 0.13.0 release: Python took 38 seconds for all
30 programs instead of 30, and 0.13.0 took 13.1 seconds instead of 10.0.
`file_io` syncs a file to disk in all three versions, and the shortest
programs (`bytecode_vm`, `nbody`, `order_book`) move by 10 to 50% from run
to run.

| Program | Size | Python ms | Rust ms | Aura ms | Aura 0.13.0 ms | Aura / Python | Aura / Rust |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| `astar_pathfinding` | 500 | 1,961 | 87 | 253 | 261 | 0.13x | 2.9x |
| `bank_ledger` | 200000 | 311 | 26 | 127 | 151 | 0.41x | 4.8x |
| `binary_trees` | 14 | 274 | 55 | 142 | 190 | 0.52x | 2.6x |
| `bytecode_vm` | 200000 | 1,930 | 37 | 55 | 56 | 0.03x | 1.5x |
| `csv_aggregate` | 200000 | 458 | 121 | 458 | 495 | 1.00x | 3.8x |
| `docdb` | 50000 | 554 | 370 | 309 | 977 | 0.56x | 0.8x |
| `file_io` | 300000 | 1,286 | 856 | 1,780 | 1,851 | 1.38x | 2.1x |
| `graph_paths` | 200000 | 4,881 | 260 | 907 | 1,004 | 0.19x | 3.5x |
| `hashing` | 2000000 | 1,414 | 42 | 57 | 58 | 0.04x | 1.4x |
| `http_api` | 3000 | 943 | 326 | 129 | 151 | 0.14x | 0.4x |
| `image_filters` | 1000 | 2,557 | 48 | 65 | 66 | 0.03x | 1.3x |
| `interpreter` | 20000 | 2,652 | 299 | 962 | 954 | 0.36x | 3.2x |
| `json_roundtrip` | 50 | 415 | 150 | 287 | 673 | 0.69x | 1.9x |
| `kv_server` | 50000 | 2,523 | 2,079 | 463 | 504 | 0.18x | 0.2x |
| `lru_cache` | 200000 | 205 | 24 | 84 | 89 | 0.41x | 3.4x |
| `lzw_compress` | 1000000 | 438 | 39 | 270 | 264 | 0.62x | 7.0x |
| `markdown_render` | 20000 | 794 | 64 | 256 | 357 | 0.32x | 4.0x |
| `mini_browser` | 500 | 860 | 201 | 658 | 702 | 0.76x | 3.3x |
| `nbody` | 300000 | 1,294 | 15 | 71 | 76 | 0.06x | 4.9x |
| `order_book` | 100000 | 139 | 7 | 22 | 23 | 0.16x | 3.1x |
| `pubsub_broker` | 50000 | 2,974 | 2,208 | 817 | 818 | 0.27x | 0.4x |
| `raytracer` | 500 | 2,430 | 25 | 106 | 89 | 0.04x | 4.2x |
| `regex_match` | 5000 | 1,449 | 80 | 572 | 534 | 0.39x | 7.1x |
| `sort_records` | 100000 | 923 | 88 | 351 | 362 | 0.38x | 4.0x |
| `spreadsheet` | 1200 | 793 | 33 | 148 | 154 | 0.19x | 4.5x |
| `sql_engine` | 10000 | 556 | 42 | 300 | 309 | 0.54x | 7.2x |
| `task_pool` | 100000 | 678 | 232 | 303 | 414 | 0.45x | 1.3x |
| `template_render` | 10000 | 559 | 232 | 768 | 846 | 1.37x | 3.3x |
| `text_diff` | 20000 | 1,536 | 79 | 133 | 144 | 0.09x | 1.7x |
| `word_freq` | 500000 | 616 | 178 | 484 | 527 | 0.79x | 2.7x |

All 30 Aura programs take 11.3 seconds, down from 13.1 seconds with 0.13.0
in the same passes. Python takes 38 seconds and Rust 8.3 seconds. The
median program takes 0.37 times Python's time and 3.2 times Rust's, down
from 0.38 and 3.5 times with 0.13.0. Aura is faster than Python on 27
programs and level with it on `csv_aggregate`. It is slower on `file_io`
and `template_render`. No program is above 8x Rust.

`stream_io` was added on 10 November 2026, after these passes, with
`fs.File.read_lines`, `read_chunk`, the `gzip` module, and
`bytes.hmac_sha256`. On the same kind of container, at 400,000 lines, it
takes 766 ms in Aura, 938 ms in Python, and 430 ms in Rust: 0.82 times
Python's time and 1.8 times Rust's. It is not in the table or the totals
above.

`regex_scan` was added the same day with the `regex` module. At 200,000
lines it takes 0.83 s in Aura, 1.55 s in Python, and 0.27 s in Rust: 0.54
times Python's time and 3.0 times Rust's. Rust's `regex` crate is the engine
under both the Aura and the Rust program, so the difference is the cost of
handing each result to the program as Aura values. It is not in the table or
the totals above either.

`date_time` was added the same day with the `time` module. At 200,000
instants it takes 0.34 s in Aura, 4.0 s in Python (`datetime`), and 0.18 s
in Rust (`chrono`): 0.08 times Python's time and 1.9 times Rust's. It is not
in the table or the totals above either.

`http_stream` was added on 11 November 2026 with the `http` module. At
400,000 lines, 100,000 events, and 4,000 small requests it takes 1.19 s in
Aura, 5.78 s in Python (`http.client`), and 0.61 s in Rust (a client
written by hand on the standard library): 0.21 times Python's time and 1.9
times Rust's. Each version also runs the server. It is not in the table or
the totals above either. `http_api` and `mini_browser` now use the `http`
module, and their times did not change.

0.14.0 brings compact JSON storage and the rest of the lean-values work:

- A parsed JSON document, and a `json.Value` literal spelled whole, is one
  flat tape that `match`, key lookups, and `json.dumps` read in place.
  `docdb` takes 309 ms instead of 977 and `json_roundtrip` 287 ms instead
  of 673. Both were slower than Python with 0.13.0 (1.8x and 1.6x) and are
  faster now (0.56x and 0.69x), and `docdb` is faster than the Rust
  version, which keeps its documents as `serde_json` values.
- Runtime values are 64 bytes instead of 96 and a box header is smaller,
  so programs that build many boxed values touch less memory. Besides the
  two JSON programs, thirteen programs were faster in both passes than
  0.13.0 was in either, among them `bank_ledger` (-16%), `http_api`
  (-15%), `graph_paths` (-10%), `template_render` (-9%), `kv_server`
  (-8%), and `csv_aggregate` (-7%).

`raytracer` was slower than 0.13.0 in both passes (106 ms against 89) and
`regex_match` in one. Timed again, two alternating passes of five runs
each, they were level (89 ms against 88, and 541 ms against 538), and
`interpreter` and `lzw_compress` were 4% and 11% faster. `binary_trees`,
`markdown_render`, and `task_pool` moved by 20 to 50% between the two
passes of one compiler, so their gains in the table are not a result.

### Instruction counts

Instructions executed under callgrind at the bench size, in millions, one
run each, measured on 10 November 2026 for the 0.14.0 release candidate
against the published 0.13.0 archive. They vary much less from run to run
than times do. The threaded programs (`http_api`, `kv_server`,
`pubsub_broker`, `task_pool`) still move by a few percent.

| Program | Rust M | Aura M | Aura 0.13.0 M | Change | Aura / Rust |
| --- | ---: | ---: | ---: | ---: | ---: |
| `astar_pathfinding` | 300.4 | 2,103.3 | 2,097.4 | +0.3% | 7.0x |
| `bank_ledger` | 315.9 | 1,348.0 | 1,299.5 | +3.7% | 4.3x |
| `binary_trees` | 778.1 | 1,788.8 | 1,757.3 | +1.8% | 2.3x |
| `bytecode_vm` | 457.1 | 884.0 | 884.0 | +0.0% | 1.9x |
| `csv_aggregate` | 1,329.8 | 4,202.6 | 4,063.6 | +3.4% | 3.2x |
| `docdb` | 1,365.5 | 1,969.9 | 3,674.8 | -46.4% | 1.4x |
| `file_io` | 536.7 | 2,965.7 | 2,883.5 | +2.8% | 5.5x |
| `graph_paths` | 557.5 | 3,205.9 | 3,093.7 | +3.6% | 5.8x |
| `hashing` | 182.6 | 383.9 | 387.2 | -0.9% | 2.1x |
| `http_api` | 177.7 | 367.8 | 417.9 | -12.0% | 2.1x |
| `image_filters` | 297.1 | 817.6 | 817.6 | +0.0% | 2.8x |
| `interpreter` | 4,476.6 | 13,123.6 | 12,638.7 | +3.8% | 2.9x |
| `json_roundtrip` | 1,459.0 | 2,399.8 | 2,570.1 | -6.6% | 1.6x |
| `kv_server` | 379.3 | 1,692.4 | 1,660.8 | +1.9% | 4.5x |
| `lru_cache` | 239.5 | 793.5 | 778.8 | +1.9% | 3.3x |
| `lzw_compress` | 273.2 | 2,209.0 | 2,085.6 | +5.9% | 8.1x |
| `markdown_render` | 320.9 | 2,233.8 | 2,169.1 | +3.0% | 7.0x |
| `mini_browser` | 728.6 | 4,717.1 | 4,730.6 | -0.3% | 6.5x |
| `nbody` | 120.1 | 964.2 | 964.2 | +0.0% | 8.0x |
| `order_book` | 19.8 | 154.2 | 153.8 | +0.3% | 7.8x |
| `pubsub_broker` | 847.2 | 3,804.5 | 3,825.4 | -0.5% | 4.5x |
| `raytracer` | 174.5 | 956.9 | 928.6 | +3.0% | 5.5x |
| `regex_match` | 1,200.7 | 6,343.3 | 5,973.1 | +6.2% | 5.3x |
| `sort_records` | 468.0 | 2,362.4 | 2,369.9 | -0.3% | 5.0x |
| `spreadsheet` | 179.2 | 633.9 | 618.8 | +2.4% | 3.5x |
| `sql_engine` | 437.3 | 2,218.5 | 2,105.2 | +5.4% | 5.1x |
| `task_pool` | 529.8 | 1,855.7 | 1,819.1 | +2.0% | 3.5x |
| `template_render` | 3,202.9 | 8,523.5 | 8,414.2 | +1.3% | 2.7x |
| `text_diff` | 497.7 | 1,159.9 | 1,169.2 | -0.8% | 2.3x |
| `word_freq` | 1,105.9 | 3,486.8 | 3,501.6 | -0.4% | 3.2x |

The 30 programs run 79,670 million instructions, against 79,853 million
with 0.13.0. `docdb` runs 46% fewer, `http_api` 12% fewer, and
`json_roundtrip` 7% fewer; its time fell much further because it touches
far less memory. Fifteen programs run 2 to 6% more instructions than with
0.13.0, most of all `regex_match`, `lzw_compress`, and `sql_engine`. One
cause is known: creating a list now looks up its element type in a table of
shared types, which costs about 50 instructions for each new list
(`raytracer` creates 250,000). The median program runs 3.9 times Rust's
instructions, against 3.8 times with 0.13.0.
