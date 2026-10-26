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
  says why. For example, `str` has no ordering, so `sort_records` sorts names
  with a merge sort over their bytes.
- The bench size of `sort_records` is 20,000 records, because Aura's built-in
  sort is quadratic today. At 100,000 records it takes minutes.

## Recorded results

Measured on 26 October 2026 at `f517376` (Aura 0.7.1) with this suite added,
in a Linux x86-64 container with 4 vCPUs (Intel Xeon, 2.1 GHz), Python 3.13.17,
and Rust 1.95.0. Each time is the median of three whole-process runs at the
bench size, so it includes process startup and, for Aura, the runtime's task
scheduler. Lower ratios are better for Aura. `file_io` syncs a file to disk
in all three versions, so its Python and Rust times vary from run to run with
the disk; a rerun gave Python 647 ms and Rust 1,243 ms.

| Program | Size | Python ms | Rust ms | Aura ms | Aura / Python | Aura / Rust |
| --- | ---: | ---: | ---: | ---: | ---: | ---: |
| `astar_pathfinding` | 500 | 2,540 | 117 | 1,793 | 0.71x | 15x |
| `bank_ledger` | 200000 | 385 | 31 | 1,011 | 2.62x | 32x |
| `binary_trees` | 14 | 350 | 71 | 2,941 | 8.39x | 41x |
| `bytecode_vm` | 200000 | 2,489 | 44 | 1,533 | 0.62x | 35x |
| `csv_aggregate` | 200000 | 518 | 163 | 1,754 | 3.38x | 11x |
| `docdb` | 50000 | 760 | 435 | 5,060 | 6.66x | 12x |
| `file_io` | 300000 | 1,204 | 1,084 | 24,446 | 20.31x | 23x |
| `graph_paths` | 200000 | 5,901 | 264 | 44,681 | 7.57x | 170x |
| `hashing` | 2000000 | 1,906 | 51 | 7,915 | 4.15x | 154x |
| `http_api` | 3000 | 1,367 | 368 | 1,312 | 0.96x | 4x |
| `image_filters` | 1000 | 3,382 | 60 | 7,861 | 2.32x | 131x |
| `interpreter` | 20000 | 4,473 | 351 | 32,208 | 7.20x | 92x |
| `json_roundtrip` | 50 | 495 | 150 | 1,875 | 3.79x | 13x |
| `kv_server` | 50000 | 4,187 | 2,058 | 3,899 | 0.93x | 2x |
| `lru_cache` | 200000 | 268 | 28 | 2,249 | 8.40x | 80x |
| `lzw_compress` | 1000000 | 533 | 45 | 2,171 | 4.07x | 48x |
| `markdown_render` | 20000 | 931 | 67 | 6,363 | 6.83x | 95x |
| `mini_browser` | 500 | 1,053 | 238 | 8,587 | 8.16x | 36x |
| `nbody` | 300000 | 1,639 | 17 | 4,647 | 2.83x | 276x |
| `order_book` | 100000 | 156 | 7 | 829 | 5.31x | 117x |
| `pubsub_broker` | 50000 | 4,655 | 2,297 | 7,686 | 1.65x | 3x |
| `raytracer` | 500 | 3,161 | 28 | 14,394 | 4.55x | 516x |
| `regex_match` | 5000 | 1,910 | 106 | 13,327 | 6.98x | 125x |
| `sort_records` | 20000 | 197 | 18 | 15,222 | 77.42x | 848x |
| `spreadsheet` | 1200 | 971 | 47 | 3,397 | 3.50x | 72x |
| `sql_engine` | 10000 | 720 | 59 | 10,761 | 14.94x | 181x |
| `task_pool` | 100000 | 888 | 300 | 1,508 | 1.70x | 5x |
| `template_render` | 10000 | 818 | 282 | 4,786 | 5.85x | 17x |
| `text_diff` | 20000 | 2,077 | 113 | 3,509 | 1.69x | 31x |
| `word_freq` | 500000 | 745 | 185 | 2,015 | 2.71x | 11x |

Aura is faster than Python on `astar_pathfinding`, `bytecode_vm`, `http_api`,
and `kv_server`, and slower on the other 26. The largest gaps come from a few
runtime costs that many programs share:

- Reading a class instance, a `copy class` value, or an enum payload out of a
  list, a field, or a `match` copies it into a new box. Dijkstra's heap in
  `graph_paths`, the vectors in `raytracer`, the syntax tree in `interpreter`,
  and the trees in `binary_trees` spend most of their time there.
- `list.sort` is an insertion sort, so it is quadratic (`sort_records`).
- A list slice `xs[a:b]` walks the first `a` elements, so scanning a buffer
  with slices is quadratic (`mini_browser`).
- `dict.remove` shifts every later entry to keep insertion order
  (`lru_cache`).
- Each `File.write_all` is a separate round trip to the blocking I/O pool,
  with no buffering (`file_io`).
- Casts from `uint8` and arithmetic on it take a generic runtime path
  (`hashing`, `markdown_render`, `image_filters`), and writes into a
  `list[float64]` box each value (`nbody`).
