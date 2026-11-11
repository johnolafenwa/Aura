# Streaming I/O and a deeper standard library

Date: 2026-11-10. Asked for by John: complete the first three of the next
core items, "bug free, within range of Rust, faster than Python and
complete". This note covers item 1: streamed HTTP bodies, line and chunk
file reads, redirects, connection pooling, gzip, regex, date and time, HMAC,
and a reference agent that streams a model reply.

## Plan

1. File line and chunk reads, gzip, HMAC (this change).
2. `regex` and `time` modules.
3. A new `http` client module: streamed bodies, redirects, a keep-alive
   pool, gzip and deflate decoding, server-sent events. It replaces
   `net.http_request_*` (ADR-0057: no compatibility aliases).
4. A reference agent that streams a model reply, and the consolidated
   benchmark and book updates.

Each step ships its own Aura, Python, and Rust benchmark.

## Step 1: work completed

- `fs.File.read_line()`, `read_lines(max_lines)`, and
  `read_chunk(max_bytes)`, on both backends, with a read-ahead buffer that
  all read methods share. Design and limits are in ADR-0073.
- `gzip.compress` and `gzip.decompress` (flate2 over zlib-rs), and
  `gzip.Error`.
- `bytes.hmac_sha256` and `bytes.constant_time_eq`.
- Examples `examples/io/read_lines.au` and
  `examples/io/sign_and_compress.au`, the manual's Filesystem, Bytes, and
  new Gzip pages, and the `stream_io` cross-language program.

## Measurements

4-core Linux cloud host, release build, medians.

| Work | Aura | Python | Rust |
| --- | --- | --- | --- |
| 2M lines (90 MB), `read_lines(1024)` loop, sum of line lengths | 300 ms | 340 to 400 ms | 150 ms |
| Same file, `read_line()` loop | 830 ms | | |
| `stream_io` at 400,000 lines | 766 ms | 938 ms | 430 ms |

How the batch read got there, from a first version at 480 ms:

- A fill no longer zeroes the buffer it is about to overwrite.
- A buffer of lines is checked for UTF-8 once, not line by line.
- Read-ahead grows from 64 KiB to 1 MiB. Each fill is a trip to the
  blocking pool, and 1,400 trips for a 90 MB file cost about 110 ms.
- The blocking pool only fills the buffer. Lines are built on the calling
  thread, so they are allocated and freed by the same thread.

## Gaps found, left for later

- Every `Result[str | None, io.Error]` or `Result[str, E]` value crosses
  the runtime boundary as a boxed enum (about 2,500 instructions each), so
  `read_line()` in a loop is 2.4 times slower than Python, and a user
  `Iterator[Result[str, io.Error]]` is as slow. This is a compiler gap, not
  a file one. A `for line in file` form waits on it.
- `for line in batch` over a `list[str]` copies each string (about 85 ns
  per line here). It is the larger half of the 300 ms above.
- A match on a builtin enum whose runtime name is not in
  `mir_runtime_enum_name`'s list matches no arm and says nothing. `gzip.Error`
  is in the list now; the silent fall-through deserves a check.

## Verification

Recorded with the pull request.
