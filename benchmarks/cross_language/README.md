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

Measured on 12 November 2026 at the 0.15.0 release candidate, in a Linux
x86-64 container with 4 vCPUs (Intel Xeon, 2.1 GHz), Python 3.13.18, and
Rust 1.95.0. Each time is the median of twenty whole-process runs at the
bench size, pooled from four passes, so it includes process startup and,
for Aura, the runtime's task scheduler. Lower ratios are better for Aura.
The Aura 0.14.0 column is the same program built by the published 0.14.0
release archive, measured in the same passes on the same machine. The
four programs that 0.15.0 adds have no 0.14.0 time, and 0.14.0 ran the
`http_api` and `mini_browser` of its own release, which used the `net`
calls that the `http` module replaces. The first two passes ran before a
fix to nested loops, so `astar_pathfinding`, `spreadsheet`, and
`text_diff` give the median of the ten runs after it. The container was
busy: one compiler's medians for one program differ by 10 to 30% between
passes for `binary_trees`, `interpreter`, `markdown_render`, and the
threaded programs (`http_api`, `kv_server`, `pubsub_broker`,
`task_pool`). `file_io` syncs a file to disk in all three versions.

| Program | Size | Python ms | Rust ms | Aura ms | Aura 0.14.0 ms | Aura / Python | Aura / Rust |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| `astar_pathfinding` | 500 | 2,127 | 89 | 274 | 260 | 0.13x | 3.1x |
| `bank_ledger` | 200000 | 348 | 30 | 143 | 144 | 0.41x | 4.7x |
| `binary_trees` | 14 | 297 | 66 | 163 | 163 | 0.55x | 2.5x |
| `bytecode_vm` | 200000 | 2,092 | 38 | 55 | 58 | 0.03x | 1.4x |
| `csv_aggregate` | 200000 | 514 | 136 | 478 | 466 | 0.93x | 3.5x |
| `date_time` | 200000 | 5,168 | 226 | 416 | n/a | 0.08x | 1.8x |
| `docdb` | 50000 | 611 | 471 | 397 | 379 | 0.65x | 0.8x |
| `file_io` | 300000 | 1,183 | 1,092 | 1,969 | 2,190 | 1.66x | 1.8x |
| `graph_paths` | 200000 | 5,541 | 301 | 998 | 982 | 0.18x | 3.3x |
| `hashing` | 2000000 | 1,652 | 43 | 60 | 60 | 0.04x | 1.4x |
| `http_api` | 3000 | 1,147 | 352 | 198 | 175 | 0.17x | 0.6x |
| `http_stream` | 400000 | 5,799 | 685 | 1,045 | n/a | 0.18x | 1.5x |
| `image_filters` | 1000 | 3,083 | 55 | 80 | 76 | 0.03x | 1.5x |
| `interpreter` | 20000 | 2,940 | 345 | 975 | 1,053 | 0.33x | 2.8x |
| `json_roundtrip` | 50 | 464 | 163 | 296 | 315 | 0.64x | 1.8x |
| `kv_server` | 50000 | 2,717 | 2,149 | 530 | 499 | 0.19x | 0.2x |
| `lru_cache` | 200000 | 228 | 27 | 93 | 90 | 0.41x | 3.5x |
| `lzw_compress` | 1000000 | 479 | 40 | 241 | 247 | 0.50x | 6.1x |
| `markdown_render` | 20000 | 853 | 64 | 324 | 292 | 0.38x | 5.1x |
| `mini_browser` | 500 | 911 | 203 | 679 | 721 | 0.75x | 3.3x |
| `nbody` | 300000 | 1,387 | 15 | 34 | 84 | 0.02x | 2.3x |
| `order_book` | 100000 | 142 | 7 | 25 | 23 | 0.17x | 3.6x |
| `pubsub_broker` | 50000 | 2,965 | 2,331 | 939 | 937 | 0.32x | 0.4x |
| `raytracer` | 500 | 2,709 | 26 | 94 | 92 | 0.03x | 3.6x |
| `regex_match` | 5000 | 1,519 | 93 | 514 | 626 | 0.34x | 5.6x |
| `regex_scan` | 200000 | 1,971 | 328 | 1,096 | n/a | 0.56x | 3.3x |
| `sort_records` | 100000 | 1,048 | 91 | 374 | 362 | 0.36x | 4.1x |
| `spreadsheet` | 1200 | 856 | 37 | 153 | 178 | 0.18x | 4.1x |
| `sql_engine` | 10000 | 594 | 57 | 300 | 335 | 0.50x | 5.3x |
| `stream_io` | 400000 | 958 | 412 | 788 | n/a | 0.82x | 1.9x |
| `task_pool` | 100000 | 762 | 252 | 342 | 332 | 0.45x | 1.4x |
| `template_render` | 10000 | 608 | 271 | 840 | 865 | 1.38x | 3.1x |
| `text_diff` | 20000 | 1,706 | 84 | 149 | 150 | 0.09x | 1.8x |
| `word_freq` | 500000 | 682 | 209 | 549 | 553 | 0.80x | 2.6x |

All 34 Aura programs take 15.6 seconds, against 56.1 seconds for Python
and 10.8 seconds for Rust. The median program takes 0.35 times Python's
time and 2.7 times Rust's. Aura is faster than Python on 32 programs and
slower on `file_io` and `template_render`. No program is above 8x Rust;
the highest is `lzw_compress` at 6.1x.

The 30 programs that both compilers build take 12.3 seconds, against 12.7
seconds with 0.14.0 in the same passes. Their median is 2.9 times Rust's
time, from 3.1 times with 0.14.0.

0.15.0 changes these programs most:

- `nbody` takes 34 ms instead of 84. Its loops over lists of floats are
  counted loops, which check their range once where they start, and a
  float `sqrt` is one instruction.
- `regex_match` (-18%), `spreadsheet` (-14%), and `sql_engine` (-11%) gain
  from the regression fixes that followed 0.14.0 and from method calls on
  list elements that run in place.
- `file_io` (-10%), `interpreter` (-7%), `json_roundtrip` (-6%), and
  `mini_browser` (-6%) are faster by less than they move between passes.
- `http_api` reads 198 ms against 175 and `markdown_render` 324 ms against
  292. `http_api` now uses the `http` client and runs 49% more
  instructions, so its change is real. `markdown_render` runs 2% more
  instructions, and its time moved by more than that between passes of
  one compiler.

### Instruction counts

Instructions executed under callgrind at the bench size, in millions, one
run each, measured on 12 November 2026 for the 0.15.0 release candidate
against the published 0.14.0 archive. They vary much less from run to run
than times do. The threaded programs (`http_api`, `kv_server`,
`pubsub_broker`, `task_pool`) still move by a few percent.

| Program | Rust M | Aura M | Aura 0.14.0 M | Change | Aura / Rust |
| --- | ---: | ---: | ---: | ---: | ---: |
| `astar_pathfinding` | 300.4 | 2,138.0 | 2,103.3 | +1.6% | 7.1x |
| `bank_ledger` | 316.0 | 1,331.9 | 1,348.0 | -1.2% | 4.2x |
| `binary_trees` | 778.1 | 1,783.0 | 1,788.8 | -0.3% | 2.3x |
| `bytecode_vm` | 457.1 | 868.1 | 884.0 | -1.8% | 1.9x |
| `csv_aggregate` | 1,329.8 | 4,100.0 | 4,203.0 | -2.5% | 3.1x |
| `date_time` | 1,998.2 | 4,256.7 | n/a | n/a | 2.1x |
| `docdb` | 1,365.1 | 1,970.2 | 1,969.9 | +0.0% | 1.4x |
| `file_io` | 536.7 | 2,864.9 | 2,930.7 | -2.2% | 5.3x |
| `graph_paths` | 557.5 | 3,056.3 | 3,205.9 | -4.7% | 5.5x |
| `hashing` | 182.6 | 271.2 | 383.9 | -29.3% | 1.5x |
| `http_api` | 177.9 | 547.3 | 367.8 | +48.8% | 3.1x |
| `http_stream` | 1,335.9 | 8,800.0 | n/a | n/a | 6.6x |
| `image_filters` | 297.1 | 753.9 | 817.6 | -7.8% | 2.5x |
| `interpreter` | 4,476.6 | 12,904.1 | 13,123.6 | -1.7% | 2.9x |
| `json_roundtrip` | 1,459.0 | 2,401.5 | 2,399.8 | +0.1% | 1.6x |
| `kv_server` | 380.0 | 1,672.9 | 1,692.4 | -1.2% | 4.4x |
| `lru_cache` | 239.0 | 789.1 | 792.8 | -0.5% | 3.3x |
| `lzw_compress` | 273.2 | 2,011.4 | 2,206.2 | -8.8% | 7.4x |
| `markdown_render` | 320.9 | 2,282.5 | 2,235.0 | +2.1% | 7.1x |
| `mini_browser` | 728.7 | 4,519.2 | 4,716.4 | -4.2% | 6.2x |
| `nbody` | 120.1 | 447.0 | 964.2 | -53.6% | 3.7x |
| `order_book` | 19.8 | 154.1 | 154.2 | -0.1% | 7.8x |
| `pubsub_broker` | 847.2 | 3,769.6 | 3,804.5 | -0.9% | 4.4x |
| `raytracer` | 174.5 | 858.3 | 956.9 | -10.3% | 4.9x |
| `regex_match` | 1,200.7 | 4,948.6 | 6,343.3 | -22.0% | 4.1x |
| `regex_scan` | 3,492.0 | 9,221.4 | n/a | n/a | 2.6x |
| `sort_records` | 468.0 | 2,357.5 | 2,362.4 | -0.2% | 5.0x |
| `spreadsheet` | 179.2 | 614.0 | 633.9 | -3.1% | 3.4x |
| `sql_engine` | 437.3 | 2,102.6 | 2,218.6 | -5.2% | 4.8x |
| `stream_io` | 3,444.2 | 6,110.5 | n/a | n/a | 1.8x |
| `task_pool` | 529.7 | 1,830.6 | 1,855.9 | -1.4% | 3.5x |
| `template_render` | 3,205.6 | 8,383.1 | 8,523.5 | -1.6% | 2.6x |
| `text_diff` | 497.7 | 1,124.0 | 1,159.9 | -3.1% | 2.3x |
| `word_freq` | 1,105.9 | 3,446.7 | 3,487.0 | -1.2% | 3.1x |

The 30 programs that both compilers build run 76,302 million
instructions, against 79,633 million with 0.14.0, 4.2% fewer. `nbody`
runs 54% fewer, `hashing` 29% fewer, `regex_match` 22% fewer, and
`raytracer` 10% fewer. Three programs run more. `http_api` runs 49% more
because it now uses the `http` client, which reads a response in pieces.
`markdown_render` runs 2.1% more and `astar_pathfinding` 1.6% more, and
neither is traced to a change. The median of the 30 runs 3.6 times
Rust's instructions, against 3.9 times with 0.14.0. All 34 programs run
104,691 million.
