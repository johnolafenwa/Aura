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

Measured on 27 October 2026 at `abd8fef`, after the first phase of the plan
to bring Aura to Rust speed, in a Linux x86-64 container with 4 vCPUs (Intel
Xeon, 2.1 GHz), Python 3.13.17, and Rust 1.95.0. Each time is the median of
three whole-process runs at the bench size, so it includes process startup
and, for Aura, the runtime's task scheduler. Lower ratios are better for
Aura. The Aura 0.7.1 column is the same program built by the 0.7.1 compiler
(`f517376`), measured on 26 October 2026 on the same machine. `file_io`
syncs a file to disk in all three versions, and the threaded programs
(`http_api`, `kv_server`, `pubsub_broker`, `task_pool`) move by 10 to 20%
from run to run.

| Program | Size | Python ms | Rust ms | Aura ms | Aura 0.7.1 ms | Aura / Python | Aura / Rust |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| `astar_pathfinding` | 500 | 2,558 | 104 | 875 | 1,793 | 0.34x | 8x |
| `bank_ledger` | 200000 | 372 | 46 | 892 | 1,011 | 2.40x | 19x |
| `binary_trees` | 14 | 340 | 67 | 2,715 | 2,941 | 7.99x | 40x |
| `bytecode_vm` | 200000 | 2,350 | 42 | 1,057 | 1,533 | 0.45x | 25x |
| `csv_aggregate` | 200000 | 498 | 139 | 1,398 | 1,754 | 2.81x | 10x |
| `docdb` | 50000 | 596 | 409 | 3,360 | 5,060 | 5.64x | 8x |
| `file_io` | 300000 | 677 | 920 | 2,799 | 24,446 | 4.14x | 3x |
| `graph_paths` | 200000 | 4,915 | 245 | 37,183 | 44,681 | 7.57x | 152x |
| `hashing` | 2000000 | 1,546 | 45 | 67 | 7,915 | 0.04x | 1x |
| `http_api` | 3000 | 1,225 | 341 | 1,179 | 1,312 | 0.96x | 3x |
| `image_filters` | 1000 | 2,874 | 53 | 4,657 | 7,861 | 1.62x | 88x |
| `interpreter` | 20000 | 3,448 | 331 | 29,407 | 32,208 | 8.53x | 89x |
| `json_roundtrip` | 50 | 476 | 138 | 1,823 | 1,875 | 3.83x | 13x |
| `kv_server` | 50000 | 3,877 | 1,833 | 3,280 | 3,899 | 0.85x | 2x |
| `lru_cache` | 200000 | 241 | 40 | 844 | 2,249 | 3.50x | 21x |
| `lzw_compress` | 1000000 | 500 | 40 | 376 | 2,171 | 0.75x | 9x |
| `markdown_render` | 20000 | 828 | 62 | 655 | 6,363 | 0.79x | 11x |
| `mini_browser` | 500 | 975 | 215 | 4,694 | 8,587 | 4.81x | 22x |
| `nbody` | 300000 | 1,512 | 16 | 422 | 4,647 | 0.28x | 26x |
| `order_book` | 100000 | 155 | 7 | 626 | 829 | 4.04x | 96x |
| `pubsub_broker` | 50000 | 4,247 | 2,018 | 6,464 | 7,686 | 1.52x | 3x |
| `raytracer` | 500 | 3,118 | 28 | 13,479 | 14,394 | 4.32x | 489x |
| `regex_match` | 5000 | 1,761 | 84 | 9,323 | 13,327 | 5.29x | 111x |
| `sort_records` | 100000 | 965 | 89 | 1,407 | 2,127 | 1.46x | 16x |
| `spreadsheet` | 1200 | 913 | 42 | 1,583 | 3,397 | 1.73x | 38x |
| `sql_engine` | 10000 | 618 | 43 | 7,497 | 10,761 | 12.13x | 173x |
| `task_pool` | 100000 | 827 | 248 | 2,812 | 1,508 | 3.40x | 11x |
| `template_render` | 10000 | 717 | 271 | 4,251 | 4,786 | 5.93x | 16x |
| `text_diff` | 20000 | 1,992 | 82 | 223 | 3,509 | 0.11x | 3x |
| `word_freq` | 500000 | 711 | 174 | 1,823 | 2,015 | 2.56x | 10x |

All 30 Aura programs take 147 seconds, down from 227 seconds with 0.7.1, and
Python takes 46 seconds. The median program is 2.7 times Python's time, down
from 3.9 times, and 16 times Rust's, down from 35 times. Aura is now faster
than Python on nine programs: `astar_pathfinding`, `bytecode_vm`, `hashing`,
`http_api`, `kv_server`, `lzw_compress`, `markdown_render`, `nbody`, and
`text_diff`.

These changes removed the largest costs that 0.7.1 had:

- A `for` loop over `range(...)` counts on plain integers instead of boxing
  a `Range` value at each step.
- Native code reads, writes, and appends the elements of a list of any
  scalar type in place, and runs `as` casts and bitwise operators inline
  (`hashing`, `markdown_render`, `nbody`, `lzw_compress`).
- `list.sort` is a merge sort instead of an insertion sort, and `str` has an
  ordering (`sort_records`).
- A list or ASCII `str` slice copies only its result instead of walking up
  to its start (`mini_browser`).
- `dict.remove` takes constant time instead of shifting every later entry
  (`lru_cache`).
- `fs.File` buffers its writes (`file_io`).

`task_pool` is the one program that got slower, from 1,508 to 2,812 ms,
while it runs 39% fewer instructions. Its four pool workers now finish each
job so quickly that they find the job queue empty three times as often, and
each wait puts a thread to sleep and wakes it again. The cost of handing a
message from one task to another is now most of its time.

What remains is mostly the cost of the value representation. Reading a class
instance, a `copy class` value, or an enum payload out of a list, a field,
or a `match` copies it into a new box, which is most of the time in
`graph_paths`, `raytracer`, `interpreter`, `order_book`, and
`binary_trees`. Phase 2 of the plan stores such values in typed native
layouts, so a read does not copy.

### Instruction counts

Instructions executed under callgrind at the check size, in millions, from
the same commits. They vary much less from run to run than times do. The
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
