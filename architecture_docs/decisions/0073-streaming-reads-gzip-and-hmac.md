# ADR-0073: Streaming file reads, gzip, and HMAC

- Status: Accepted
- Date: 2026-11-10
- Builds on: ADR-0018, ADR-0022

## Context

Until 0.13 a program could read a file only as one value, capped at 256 MiB
(ADR-0018). A log, an export, or a model file larger than that could not be
read at all, and a smaller one cost its whole size in memory. The standard
library also had no compression and no keyed hash, so a program could not
read a `.gz` file, answer `Content-Encoding: gzip`, or check a webhook
signature without a foreign library.

This is the first step of the streaming I/O and standard-library work. The
HTTP client that streams response bodies builds on the same pieces.

## Decision

### Reading a file in pieces

`fs.File` gains three methods on a shared receiver:

```aura fragment
read_line() -> Result[str | None, io.Error]
read_lines(max_lines: int64) -> Result[list[str], io.Error]
read_chunk(max_bytes: int64) -> Result[list[uint8] | None, io.Error]
```

- A line ends at LF. The result drops the LF and a CR before it. The last
  line needs no LF, a trailing LF adds no empty line, and an empty file has
  no lines. This is what Python's `splitlines` on a text file and Rust's
  `BufRead::lines` return.
- Each line is strict UTF-8, as every text read in Aura is. A bad line
  returns `io.Error.InvalidData` and is consumed, so a program can skip it
  and go on. `read_lines` returns the good lines before a bad one first and
  reports the bad line alone on the next call, so no line is lost.
- `read_lines` returns exactly `max_lines` lines until the file runs out,
  and an empty list at the end. `read_chunk` returns exactly `max_bytes`
  bytes until the file runs out, and `None` at the end. A short result
  therefore always means the end, which keeps record-oriented loops simple.
- Counts are `int64` and checked: 1 to 1,048,576 lines, 1 to 256 MiB per
  chunk. One line may be at most 64 MiB. A count out of range is
  `io.Error.InvalidInput`, a typed outcome like every other file failure.
- The handle reads ahead into a buffer that starts at 64 KiB and doubles to
  1 MiB while a file is read from start to end. All read methods take from
  that buffer first, so they share one position.
- The end of the file is not remembered. A later call sees appended data,
  which is what following a growing log needs.

`read_lines` exists beside `read_line` because a batch crosses the runtime
boundary once. On a 2 million line, 96 MB file, `read_lines(1024)` in a
loop takes about 130 ms and `read_line` about 155 ms, where Python's
`for line in file` takes about 250 ms and Rust's `BufRead::lines` 90 ms.

`line = try file.read_line()` compiles to one call that hands back the line
or null and writes a failure to a word on the caller's stack, so the usual
call builds no `Result` and no `str | None` box. A `read_line` result kept
as a value uses the plain entry.

A line that the buffer already holds is taken on the calling thread. A fill
is the only part of a read that can wait on the host. On Linux a fill first
asks for data the page cache already holds (`preadv2` with `RWF_NOWAIT`)
and reads it on the calling thread; a file on `tmpfs` or `ramfs` is always
in memory and is read directly. A fill that would wait for the disk, and
every fill on other hosts, runs on the blocking pool. Lines are built on
the calling thread, which also frees them.

### gzip

A new builtin module:

```aura fragment
gzip.compress(data: list[uint8], level: int64 = 6) -> list[uint8]
gzip.decompress(data: list[uint8], max_bytes: int64 = 268435456) -> Result[list[uint8], gzip.Error]
```

with `gzip.Error.InvalidData(message: str)` and
`gzip.Error.OutputTooLarge(limit_bytes: int64)`.

- Malformed input is data, so it is a `Result`. A level outside 0 to 9 or a
  limit outside 0 to 2,147,483,647 is a mistake in the program, so it traps
  with `AU4001`.
- `decompress` always has an output limit, 256 MiB unless the caller names
  one, because a few kilobytes of gzip can expand to gigabytes. It stops at
  the limit instead of decompressing first and measuring after.
- `decompress` reads every member of a multi-member stream, as the `gzip`
  tool does, and rejects trailing bytes that are not a member.
- `compress` writes no file name and no modification time, so its output is
  a function of the input and the level.
- The functions take and return whole buffers. A streaming form is not
  needed yet: a file is compressed chunk by chunk as separate members, and
  the HTTP client decompresses a body as it reads it inside the runtime.
- The implementation is the `flate2` crate over `zlib-rs`, which is Rust
  throughout and at least as fast as the C zlib that Python uses. A program
  that does not call `gzip` does not link it; the lean-binary gate checks
  that.

### HMAC

```aura fragment
bytes.hmac_sha256(key: list[uint8], message: list[uint8]) -> list[uint8]
bytes.constant_time_eq(left: list[uint8], right: list[uint8]) -> bool
```

HMAC-SHA256 is the keyed hash that API signatures and webhooks use. It is
built on the SHA-256 the runtime already links. `constant_time_eq` ships
with it because comparing a signature with `==` leaks, through timing, how
many leading bytes were right, and a library that offers the hash without
the comparison invites that mistake. It uses the `subtle` crate, which the
TLS stack already links. Other digests and a streaming hasher are left
until something needs them.

## Consequences

- A file of any size can be processed in constant memory.
- The 256 MiB cap of ADR-0018 stays on the whole-value reads and is now the
  cap of one chunk as well.
- `fs.File` holds up to 1 MiB of read-ahead per open file that is read in
  pieces. A handle that is only written holds none.
- Reading the same handle from two tasks at once interleaves at the
  granularity of a call, as before.
- `runtime` gains the `flate2`, `zlib-rs`, and `subtle` dependencies.
- The cross-language set gains `stream_io`, which reads a log in batches
  and chunks and runs gzip and HMAC over each chunk: Aura 766 ms, Python
  938 ms, Rust 430 ms at 400,000 lines on the 4-core Linux measurement
  host.

## Alternatives considered

- **An iterator, `for line in file.lines()`.** The natural spelling, but
  each item would be a boxed `Result` today and so slower than Python.
  Deferred, not rejected.
- **`read_line` returning the line ending.** Python keeps it, and it makes
  an unterminated last line visible. Almost every caller strips it at once,
  and Aura's `str.lines()` already drops it.
- **Lossy decoding of bad lines.** Rejected for the reason every other text
  read rejects it: silent replacement hides corrupted input.
- **A sticky end of file.** Simpler to describe, but it makes following a
  growing file impossible without reopening it.
- **`miniz_oxide` behind `flate2`.** Smaller, but slower than C zlib at
  compression, which would put `gzip.compress` behind Python.
- **A general `hmac(algorithm, ...)`.** SHA-256 is the only digest the
  library has. A second algorithm can add its own function.
