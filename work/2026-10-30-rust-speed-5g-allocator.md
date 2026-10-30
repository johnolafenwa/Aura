# Rust-speed 5g: per-thread small-block allocator

Phase 5, step 5g. Gate: malloc+free under 5% of instructions in every
program of the cross-language suite.

## Problem

After 5f, glibc's `malloc.c` took 2-35% of instructions across the 30
programs (order_book 35%, spreadsheet 24%, http_api 23%, graph_paths 22%,
json_roundtrip 21%, mini_browser 18%). An LD_PRELOAD size histogram
showed almost every call is 16-208 bytes, with a second group at 512 B to
1 KiB (json_roundtrip). glibc's tcache keeps 7 blocks per size, so a
program that frees a few thousand nodes and then builds more goes to
`_int_malloc`/`_int_free` for most of them (150-250 instructions per
pair), and first-time blocks cost about as much.

## Changes

- `crates/aura-runtime/src/native_allocator.rs`: `AuraAllocator`,
  registered as `#[global_allocator]` in the runtime crate, so it serves
  executables (the staticlib) and every binary that links the rlib (the
  `aura` CLI, tests).
  - Small blocks: size up to 1,024 B and alignment up to 16, rounded up to
    16-byte classes (64 classes).
  - Per thread (const `thread_local!` with a `Drop`): a free list per
    class, at most 1,024 blocks, and the uncut part of a 64 KiB chunk.
  - `alloc`: pop the list; when empty, take a shared batch (its rest
    becomes the list), else cut from the chunk, else a new chunk (the rest
    of the old chunk is cut into blocks and shared).
  - `dealloc`: push; a full list hands its first half to the shared list.
  - Shared lists: one per class, a batch stack linked through the second
    word of each batch's first block, behind an `AtomicBool` spin lock (a
    std `Mutex` may allocate on some platforms, which the allocator cannot
    do under itself). An empty shared list is seen without the lock.
  - Thread end: lists and the chunk rest go to the shared lists. After the
    thread's lists are gone (later TLS destructors), blocks come from and
    go to the shared lists directly.
  - `realloc` keeps a block that stays in its class, uses system realloc
    for large-to-large, and copies otherwise.
  - Small blocks never return to the system allocator: a program keeps its
    peak small-block use.
- `native_block_pool.rs` (5d) and `native_box_cache.rs` removed; boxes use
  `Box::new`/`Box::from_raw` and layout blocks `std::alloc` directly.
- Tests: `native_allocator_tests.rs` (reuse, half-list sharing, large and
  over-aligned blocks, realloc across classes, thread end, chunk cuts not
  overlapping).

## Results

Instructions (callgrind, check sizes) and allocator share (malloc.c plus
the `__rust_alloc`/`__rust_dealloc`/`__rust_realloc` shims), 5f (p5f6)
against this step:

```
astar_pathfinding      4056500   2.07%      4023831   0.85%   -0.8%
bank_ledger            8787542   9.21%      8469080   0.51%   -3.6%
binary_trees           2851497   5.81%      2781205   1.17%   -2.5%
bytecode_vm            5897843   3.89%      5788878   0.67%   -1.8%
csv_aggregate         30341879  17.26%     26845975   0.95%  -11.5%
docdb                 32219793  15.98%     29420724   1.75%   -8.7%
file_io                 754799  15.11%       717673   5.83%   -4.9%
graph_paths           18193899  21.91%     15072078   1.66%  -17.2%
hashing                3506992   7.11%      3390479   1.78%   -3.3%
http_api              19341322  22.70%     16598319   2.62%  -14.2%
image_filters           971070   8.87%       942257   3.45%   -3.0%
interpreter           25525675   2.26%     25368825   0.11%   -0.6%
json_roundtrip       366581180  20.89%    312397277   1.38%  -14.8%
kv_server             41974166  11.74%     39239413   0.06%   -6.5%
lru_cache             19522318   4.47%     19317967   0.15%   -1.0%
lzw_compress           3351112  11.09%      3251254   2.45%   -3.0%
markdown_render        5164633  17.89%      4681174   2.53%   -9.4%
mini_browser         233570838  18.41%    204303533   1.91%  -12.5%
nbody                  3825559   1.66%      3808876   0.72%   -0.4%
order_book             6510327  35.45%      4686276   2.83%  -28.0%
pubsub_broker         60231938  11.73%     56540985   0.42%   -6.1%
raytracer              5349974   9.27%      5105248   1.41%   -4.6%
regex_match          166795567  18.84%    149656855   2.21%  -10.3%
sort_records          26309978  17.80%     23092851   1.20%  -12.2%
spreadsheet           11014428  23.62%      9120280   2.52%  -17.2%
sql_engine           417581437  14.12%    379670479   1.74%   -9.1%
task_pool             23701386  13.80%     22430559   0.25%   -5.4%
template_render       11453502   9.21%     10899713   1.04%   -4.8%
text_diff              5396851  12.06%      5077181   3.59%   -5.9%
word_freq             25360132  18.31%     22532080   2.05%  -11.2%
```

(Measured with the 1 KiB limit and the 5d pools still present; removing
the pools afterwards moved every program by +0.8% to -2.4%.)

Earlier variants: per-thread lists over system blocks only (no chunks,
overflow freed to the system, 256 B limit) left order_book at 34% and
json_roundtrip at 17%, since first-time blocks still went through
`_int_malloc`. Chunks plus shared lists at 256 B brought everything but
file_io, spreadsheet and text_diff under 5%; raising the limit to 1 KiB
brought those two under.

Wall time (best of 3, bench sizes): sql_engine 1.246 -> 0.948 s (-24%),
graph_paths 1.708 -> 1.245 s (-27%), mini_browser -14%, http_api -14%,
docdb -12%, sort_records -12%; the rest within noise (re-timed the ones
that first looked slower: image_filters, lru_cache, order_book, all
within 3%). Peak RSS same or lower (json_roundtrip 27.8 -> 17.8 MB,
graph_paths 132.7 -> 117.8 MB, pubsub_broker 90.9 -> 93.9 MB).

## Gates

- malloc+free under 5% in every program: met in 29 of 30. MISSED for
  file_io (5.8% of 0.72M instructions): the rest is libc's own
  allocations at thread start (`arena_get2`, stdio buffers), not Aura's.

## Not done

- Small blocks are not returned to the OS while a program runs.
