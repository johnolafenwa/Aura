# Current Limits

This page documents known current limits of the Aurora compiler and runtime.

## Language

- Identifiers are ASCII; Unicode is supported in string contents, not identifier spelling.
- A physical tab anywhere on a source line is rejected, including inside a comment or string literal. Use `\t` to encode a tab in a string.
- Source lists do not accept trailing commas except the required comma in
  singleton tuple values, types, targets, and patterns. Multi-element tuples
  still reject a trailing comma.
- Parser nesting/postfix/binary-chain guards are limited to 128 operations; deeper input is rejected with a diagnostic.
- Non-numeric casts are not implemented.
- Direct recursive fields require `indirect`.
- Return values are always owned. Copy results are ordinary copies; a non-copy
  result must be constructed, cloned when clone-safe, moved from owned input,
  or produced by an owner operation. First-class loan or view returns are not
  part of Aurora 0.1, and current syntax reserves no future contract for them.
- Empty list, map, and set literals need an expected collection type.
- Class field defaults cannot call user-defined functions in the current compiler. Compute the value before construction and pass it as an explicit field argument.
- `String(...)` is not a constructor; use string literals and string methods.
- Ordinary strings may use single or double quotes, but triple-quoted, raw, and byte-string literals are not implemented. F-strings remain double-quoted.
- `String` has scalar-count `len()` and UTF-8 `byte_len()`, but no integer indexing, slicing, `chars()`, `ord()`, or `chr()` in Aurora 0.1.
- `Vec[uint8]` is the bytes type. UTF-8 conversion is explicit; the reserved `encoding` argument, non-UTF-8 text codecs, byte-string literals, URL-safe or unpadded base64, streaming codecs, incremental hashes, and HMAC are not implemented.
- Physical newlines continue a logical line only while `(`, `[`, or `{`
  remains open. Continuation indentation is visual; delimiter kinds must
  match.
- Backslash continuation is not implemented. Ordinary strings and f-strings
  remain single-line.
- Tuples have fixed structural types, recursive unpack targets and patterns,
  copy-only constant indexing, and non-consuming recursive `==` and `!=` for
  operands of the same static tuple type. There is no empty tuple,
  multi-element trailing tuple comma, tuple iteration or methods, tuple
  ordering, named/rest unpacking, mutable tuple-target writeback,
  dynamic/negative tuple indexing, or tuple-to-collection conversion. Unpack a
  tuple to take ownership of a non-copy element.
- Statement match arms cannot be inline. Expression match arms may use a same-line expression after `case pattern:` or an indented expression body.
- `for` loop bindings cannot shadow names already visible in the same scope.
- Duration literals have only the integral `ms`, `s`, and `m` suffixes; there is no `ns` or fractional Duration literal and no unary `-Duration`. Associated constructors and checked Duration arithmetic provide signed and sub-millisecond results instead.
- Task starting currently supports named functions and associated methods without `self`.
- `TaskGroup.start(...)` and `start_soon(...)` support bare shared and `own`
  target parameters; `mut` targets are rejected because child tasks cannot
  write back through the starting call frame.
- Detached lightweight tasks are not a language form; use `TaskGroup`.
- `for value in mut set:` is not currently supported.

## Runtime

- MIR runtime traps include Aurora function names and source spans in an innermost-first call-chain note. A trap escaping a structured child task also includes the child entry and its spawn ancestry.
- Runtime call chains and task ancestry are currently carried as flat prose entries in the diagnostic `notes` array. Structured frame-list fields are deferred to the later native-frames stage of the Batch 4 runtime work.
- Native direct-backend traps preserve the same primary diagnostic code, message, and span but do not yet include Aurora call-chain or task-ancestry notes. Native backtraces are deferred to that Batch 4 native-frames stage; until then, forced backend parity ignores only these three supplemental MIR note families and continues to compare the complete primary trap diagnostic.
- Aurora task code executes on pinned cooperative scheduler workers. The
  default count is the available parallelism reported by the host; the
  `AURORA_WORKERS=<positive integer>` override selects an explicit count.
  Assignment happens when a child is spawned and remains stable for its
  lifetime: coroutine stacks never migrate and the runtime does not steal work
  between workers.
- A positive `AURORA_WORKERS` value may exceed the host's available-core count.
  Empty, zero, signed, whitespace-padded, nonnumeric, and overflowing values
  are rejected before execution with `AU4006` and
  ``invalid AURORA_WORKERS value `<raw>`: expected a positive integer``.
- Scheduling is cooperative, not preemptive. The compiler checks every loop
  backedge and eventually yields from a tight loop, but only to runnable work
  assigned to that task's worker. One long loop body or long straight-line
  computation can still delay siblings pinned to the same worker. The
  automatic checks do not inspect cancellation.
- Queue and Task handles are the maintained cross-worker communication
  surface. All other task captures and results must be owned `Transfer` values,
  preserving a share-nothing boundary. A task's cancellation and diagnostic
  context remain isolated from work executing on other workers.
- Task scheduling, cross-worker completion, and program-output order are
  unspecified. There is no worker-index or affinity-introspection API.
- Pinned task execution is maintained on the MIR and direct native backends.
  Aurora does not promise work stealing, preemption, detached tasks, a
  particular parallel speedup, or broader automatic parallelism outside task
  execution.
- Ordinary lightweight tasks request 512 KiB of writable coroutine stack.
  `TaskGroup.start_with_stack` and `start_soon_with_stack` accept exact
  `int64` requests from 256 KiB through 64 MiB inclusive. Accepted requests
  are rounded upward to the host page size and guard-protected; smaller and
  larger requests are rejected rather than clamped. The MIR/direct runtime
  entry thread reserves 64 MiB, and maintained execution paths stop with a
  friendly recursion-depth diagnostic after 256 nested Aurora calls. The
  override API is Provisional under ADR-0032. The 256 KiB lower bound is an
  opt-in minimum for measured shallow tasks, not the generally safe default;
  the complete compiled Aurora HTTP example faulted when 256 KiB was the
  global default and succeeds at 512 KiB. An isolated runtime protocol
  round trip succeeds with 256 KiB callers because it excludes compiled
  language-execution frames; it proves the service offload boundary, not a
  256 KiB whole-program default.
- On the clean Mac14,9 Phase 5.7 pinned-worker measurement, 10,000 parked
  sleepers used 206,503,936 bytes of worst whole-process RSS and 197,885,952
  bytes above the same-process pre-spawn baseline.
- That Phase 5.7 combined 100,000-sleeper plus 1,000-timer benchmark passed its
  5 ms timer-arm-span and 3 ms p99 gates but reached 1,989,033,984 bytes worst
  whole-process RSS, above 1.5 GiB. Aurora therefore makes no 100,000-task
  memory claim. The host uses 16 KiB pages; one resident page for each of
  101,000 stackful children already requires 1,654,784,000 bytes before task
  metadata. Reducing a demand-paged virtual stack reservation cannot remove
  that physical-page floor.
- The same contractual run passed the mandatory four-worker scaling gate at a
  `1.077123x` paired median wall-time ratio with `393.61%` median four-task
  process CPU. This is runtime evidence, not a portable speedup guarantee.
- The scheduler uses persistent reactor registrations for nonblocking descriptors, a timer heap for deadlines, and direct Queue, task-completion, and blocking-pool notifications. When idle it blocks until an event or deadline and has no periodic scheduler tick. No high-scale task-count claim is made for 0.1.
- Deep HTTP, TLS, and maintained Unix WebSocket library frames run on a
  distinct protocol-step pool with two 2 MiB-stack workers and a 64-job queue.
  Each submitted job is a bounded, nonblocking step and returns owned protocol
  state before cancellation or reactor waiting resumes. The non-Unix
  WebSocket fallback does not use this Phase 5.4 service. The pool is
  process-global, lazily initialized, shared by all lightweight schedulers,
  and intentionally process-lifetime; it has no 0.1 runtime shutdown or join
  API. File reads, resolver work, and listener binding remain on the generic
  blocking-I/O pool; TLS asset bytes are read there before PEM parsing and
  rustls construction run on protocol workers.
- Filesystem one-shot reads and `fs.File` whole-file reads are capped at 256 MiB of remaining content. Aurora 0.1 has no chunked file-read API.
- Process-pipe and captured-output reads plus TCP, Unix, and TLS whole/bounded reads remain capped at 64 MiB. TLS certificate, private-key, and CA-file loading uses the same independent 64 MiB ceiling. A bounded byte count of zero is invalid.
- UDP receives accept `max_bytes` from 1 through 65,535.
- Incoming HTTP parsing accepts at most 64 headers and 16 MiB of wire data per message, including the start line, headers, transfer framing, trailers, and body. Outbound HTTP writers have no separate size cap. The high-level map header model cannot preserve repeated equal field names losslessly.
- WebSocket messages are capped at 64 MiB; individual frames and write buffers are capped at 16 MiB.
- TLS handshakes have a 10-second hard cap even when the caller supplies no shorter timeout.
- Duration is a signed i128 nanosecond language value, but host timer ranges are narrower. Negative values, out-of-range host conversions, and overflowing deadline calculations are invalid input rather than unlimited waits. The exact error classification is accepted under ADR-0019.
- High-level HTTP clients support HTTP/1.1 over `http://` and validated `https://`, including content-length, chunked, and close-delimited responses; redirects, pooling, HTTP/2, proxy configuration, decompression, and high-level custom CA arguments are not implemented.
- Byte-codec inputs have no separate byte-count cap, but byte conversions and
  hex/padded-base64 codecs preflight each fresh destination against a fixed
  2,147,483,647-byte safety ceiling. Crossing this codec output/resource cap
  or failing allocation traps with `AU4005`. This ceiling is independent of
  the public String and `Vec` length domains. SHA-256 always returns 32 raw
  bytes.
- JSON supports the recursive `json.Value` tree, typed `json.Error` parse
  failures, deterministic dumps, a 128-container depth limit, a shared
  root-inclusive 262,144-value materialization limit, and independent 64 MiB
  parse-input and dump-output caps. Exceeding the node limit or encountering a
  controlled parse/conversion allocation failure traps with `AU4005`; it is not
  a `json.Error` variant. Dynamic `json.parse` uses a separate process-global
  service with two 2 MiB-stack workers and total in-flight capacity two;
  capacity is reserved before the fallible source copy, and saturated
  lightweight tasks park through the scheduler. Once admitted, synchronous
  parse defers cancellation until codec completion. Runtime materialization,
  JSON-aware clone/render, and dumping use iterative traversals. The service is
  process-lifetime and has no 0.1 sizing or shutdown API. The legacy
  `json.is_valid` and `json.parse_string_map` helpers retain their bounded
  caller-side compatibility paths and do not use that service; legacy JSON
  string-map and TOML helpers remain restricted to typed
  `Map[String, String]`. JSON has no arbitrary-precision number, streaming
  codec, or derived class/enum schemas.
- `random.Rng` provides one fixed deterministic stream with integer, floating,
  and mutable-Vec shuffle operations. There is no global generator, state
  serialization, reseeding, jump/substream operation, distribution library,
  choice helper, public direct or transitive clone route, secure floating
  function, or `random.Error`. Clone-producing collection operations are
  rejected with `AU3007` when their produced value contains or may contain an
  `Rng`. An owned generator may move within one owning task, but it is not
  `Transfer`: it cannot be a task result or Queue payload. Queue handle copies
  remain valid; a Task handle is copyable only for a repeatable result.
  Generic clone-safety requirements are inferred from callable bodies,
  propagated through generic calls and imports, and checked after
  specialization; there is no source annotation for them. Trait defaults may
  establish this contract, but an explicit implementation may not strengthen
  it. Recursive nominal inspection terminates conservatively when safety cannot
  be proved.
  `secure_bytes` accepts at most 2,147,483,647 bytes as a fixed per-request
  resource and safety ceiling, independently of the public `Vec` length
  domain. Larger counts fail with `AU4005` before allocation or entropy.
  Within that request ceiling, unsatisfied allocation or OS entropy requests
  also trap with `AU4005`.
- Metrics are process-global counters within one running program; log and trace APIs emit structured stderr records and do not yet include exporters or scoped spans.
- Floating-point `/`, `//`, or `%` by zero traps at runtime instead of producing IEEE 754 infinity or NaN.
- `float32` literals that overflow may currently become infinity; prefer `float64` when large literal validation matters.
- Unix domain sockets require a Unix host.
- TLS APIs require PEM certificate/key assets.
- Package support has local path and git dependencies, but no registry publish/install flow.
- `fs.read_dir` silently skips an individual directory entry that fails after the directory itself was opened.
- High-level HTTP header conversion may expose duplicate equal map keys when the wire message repeats a header name; repeated headers are not a lossless 0.1 contract.
- Provisional ADR-0033 rejects non-Transfer task captures, task results, and
  Queue payloads with `AU3008`. Every other non-repeatable transferable task
  result has one statically enforced observation right: direct result methods
  consume it on every outcome, and multi-task waits consume the complete task
  vector. A second runtime claim that reaches the atomic containment check
  traps with `AU4001` rather than returning or cloning the stored value.
- Cancelling filesystem and other blocking-worker I/O cancels Aurora's wait, not an operating-system call already in progress. External side effects may still complete.
- The process-wide blocking pool uses 2 through 8 host threads, selected from host parallelism, with no 0.1 configuration or queue backpressure. A timed-out or cancelled host job keeps its worker until the underlying call returns; enough slow or stuck DNS/filesystem jobs can occupy the whole pool and delay unrelated blocking operations queued behind them.
- `WebSocketListener` has no explicit `close()` method, and WebSocket cancellation/error propagation is not yet fully aligned with TCP and UDP.

## Tooling

- `build` requires a host C compiler. Source-checkout builds may use Cargo to refresh the native runtime; release archives carry that runtime and do not require Rust or the source checkout.
- Native `run` cache entries larger than 512 MiB are not retained. The
  just-built program still runs, but a later invocation rebuilds it instead of
  using the cache.
- The direct backend is the maintained native backend for the implemented language surface.
- The default `--backend auto` first tries direct emission and may package an embedded-MIR launcher when direct emission is unavailable. Use `--backend direct` when fallback is unacceptable.
- Editor tooling uses a persistent compiler service. If that process is unavailable, recovery is lexical only and intentionally has no semantic diagnostics or member inference.
- `aura fmt` currently normalizes line endings, trailing whitespace, and final newlines; it is not yet a syntax-reflowing formatter.
- `aura test` treats each selected `.au` file as an executable test program and succeeds when execution succeeds with a zero integer `main` result; function-level test discovery is not implemented.
- A timed-out `aura test` stops waiting but cannot terminate its worker thread; the timed-out program may continue host side effects until the process exits.
- Recursive `aura fmt` and `aura test` traversal follows directory symlinks without cycle detection in 0.1.
