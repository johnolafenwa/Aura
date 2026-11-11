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

## Step 2a: `regex` (work completed)

- The `regex` module (ADR-0078): `regex.compile`, `regex.escape`,
  `regex.Regex` with `is_match`, `count`, `find`, `find_all`, `matches`,
  `captures`, `replace`, `replace_all`, `split`, and `pattern`,
  `regex.Match`, `regex.Captures`, and `regex.Error`, on both backends.
- Example `examples/strings/regex_log.au`, the manual's Regex page, and the
  `regex_scan` cross-language program.

| Work | Aura | Python | Rust |
| --- | --- | --- | --- |
| `regex_scan` at 200,000 lines | 1.19 s | 1.63 s | 0.30 s |

How it got there, from a first version at 9.8 s:

- The compiled program is shared, not cloned, on each call. A clone starts
  with empty scratch space (49 microseconds per search). 9.8 s to 1.97 s.
- No copy of the pattern per call, a short list of recent patterns in
  front of the map, and a group-name lookup in the runtime. 1.83 s.
- A `Regex` holds a pattern number, not the text. Reading a `str` or a
  class-typed field out of a class instance costs about 2,000 instructions
  and a release, and every method read one. 1.19 s.

## Gaps found in step 2a, left for later

- A non-scalar field read (`str`, a list, or a class instance held by
  another class) goes through `aura_direct_instance_get_field`, which
  copies the field. Scalar fields are inline loads. This is the largest
  cost left in programs that keep text in objects.
- A small host call costs about 1,900 instructions around its work: an
  argument buffer, boxed scalar arguments, and a boxed result.

## Step 2b: `time` (work completed)

- The `time` module (ADR-0079): `time.now`, `now_local`, `monotonic`,
  `utc`, `from_unix`, `from_unix_ms`, `parse_rfc3339`, `parse`, and
  `time.DateTime` with calendar fields, offsets, exact `Duration`
  arithmetic, comparison, RFC 3339 text, and `format`, on both backends.
- `regex.Regex` and `time.DateTime` are `copy class` values with a
  `Display` implementation. `public copy class` did not parse before (it
  was read as a constant named `copy`); the parser is fixed.
- Example `examples/time/timestamps.au`, the manual's Time page, the book's
  history log now writes `time.now()`, and the `date_time` cross-language
  program.

| Work | Aura | Python | Rust |
| --- | --- | --- | --- |
| `date_time` at 200,000 instants | 0.77 s | 4.1 s | 0.16 s |

## Gaps found in step 2b, left for later

- A tuple of scalars returned from a function is a boxed runtime value
  (`aura_direct_tuple_new`, then `aura_direct_tuple_element` and an unbox
  per field). Returning the date as `(int64, int64, int64)` from a helper
  cost a third of the benchmark. The field methods now return one integer
  each.
- A module-level constant in a builtin source module is not visible inside
  its methods (`AU4001: unknown MIR place`), and negating one in a
  condition fails MIR validation. `std/time.au` writes its limits as
  literals.
- Host calls take scalar arguments boxed (about 330 instructions per
  integer) and return several values only as a tuple. A call path that
  passes integers in registers would bring `regex`, `time`, and
  `fs.File.read_line()` much closer to Rust.

## Step 3: the `http` client (work completed, 2026-11-11)

- The `http` module (ADR-0080) replaces `net.http_request_*` and
  `net.HttpResponse`. `Client`, `Response`, and `Event` are Aura classes in
  `std/http.au`; the runtime half is `crates/aura-runtime/src/http_client.rs`
  with ten host functions and the opaque `http.Stream`.
- A response is returned when its head arrives. The body is read with
  `read_chunk`, `read_line`, `read_lines`, `read_event` (server-sent
  events), or whole with `text` and `bytes` (64 MiB default limit).
- Redirects (301, 302, 303, 307, 308), a process-wide pool of kept
  connections with one retry of a request on a stale connection, gzip and
  deflate decoding, a head timeout and a read timeout, task cancellation,
  and a certificate authority file.
- 39 runtime tests against a scripted local server, plus the compiler,
  CLI, and editor tests moved off the old functions. The examples
  `http_roundtrip.au` and `retrying_network_worker.au`, `http_api`,
  `mini_browser`, and the release `retrying_worker` use the new module.
- Fixed on the way: `with` on a class from another module (`AU2017`), and
  completion of private members of a class from another module.

| Work | Aura | Python | Rust |
| --- | --- | --- | --- |
| `http_stream` at 400,000 lines | 1.19 s | 5.78 s | 0.61 s |

## Gaps found in step 3, left for later

- No HTTP/2, proxies, cookies, or streamed request bodies.
- A `str` cannot be indexed, and there is no integer-to-hex formatting;
  the benchmark's server builds chunk sizes digit by digit.
- A function in a builtin source module that appends to a `str` local with
  `+=` and also lends a field gets its temporaries numbered differently
  when ownership is stripped and inserted again
  (`stripping_undoes_the_lends_and_the_pass_makes_them_again`).
  `Response.header` avoids the pattern.
