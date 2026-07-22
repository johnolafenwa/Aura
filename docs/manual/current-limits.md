# Current Limits

This page documents known current limits of the Aurora compiler and runtime.

## Language

- Identifiers are ASCII; Unicode is supported in string contents, not identifier spelling.
- A physical tab anywhere on a source line is rejected, including inside a comment or string literal. Use `\t` to encode a tab in a string.
- Source lists do not accept trailing commas.
- Parser nesting/postfix/binary-chain guards are limited to 128 operations; deeper input is rejected with a diagnostic.
- Non-numeric casts are not implemented.
- Direct recursive fields require `indirect`.
- Borrowed-return declarations use explicit sources and labels. Calls returning copy types materialize copies; calls producing non-copy borrowed results are rejected until Phase 6 live aliases.
- Empty list, map, and set literals need an expected collection type.
- Class field defaults cannot call user-defined functions in the current compiler. Compute the value before construction and pass it as an explicit field argument.
- `String(...)` is not a constructor; use string literals and string methods.
- Ordinary strings may use single or double quotes, but triple-quoted, raw, and byte-string literals are not implemented. F-strings remain double-quoted.
- `String` has scalar-count `len()` and UTF-8 `byte_len()`, but no integer indexing, slicing, `chars()`, `ord()`, or `chr()` in Aurora 0.1.
- Newlines are not continuation inside `(...)`, `[...]`, or `{...}`. Keep calls and collection literals on one physical line today.
- Backslash line continuation is not implemented.
- Statement match arms cannot be inline. Expression match arms may use a same-line expression after `case pattern:` or an indented expression body.
- Chained comparisons are rejected with migration guidance; write an explicit boolean combination such as `a < b and b < c`.
- `for` loop bindings cannot shadow names already visible in the same scope.
- Duration literals have only the integral `ms`, `s`, and `m` suffixes; there is no `ns` or fractional Duration literal and no unary `-Duration`. Associated constructors and checked Duration arithmetic provide signed and sub-millisecond results instead.
- Task starting currently supports named functions and associated methods without `self`.
- `TaskGroup.start(...)` and `start_soon(...)` support default/shared and `own` target parameters; `borrow mut` targets are rejected because child tasks cannot write back through the starting call frame.
- Detached lightweight tasks are not a language form; use `TaskGroup`.
- `for value in borrow mut set:` is not currently supported.

## Runtime

- MIR runtime traps include Aurora function names and source spans in an innermost-first call-chain note. A trap escaping a structured child task also includes the child entry and its spawn ancestry.
- Runtime call chains and task ancestry are currently carried as flat prose entries in the diagnostic `notes` array. Structured frame-list fields are deferred to Batch 3 with the native-frame work.
- Native direct-backend traps preserve the same primary diagnostic code, message, and span but do not yet include Aurora call-chain or task-ancestry notes. Native backtraces are deferred to the Batch 3 frame work; until then, forced backend parity ignores only these three supplemental MIR note families and continues to compare the complete primary trap diagnostic.
- Aurora task code executes on one cooperative scheduler thread per program. Aurora 0.1 does not run two Aurora tasks in parallel; blocking-worker threads perform host operations only.
- Scheduling is cooperative, not preemptive. A task that runs CPU code without reaching `cancelled()` or another scheduler-aware operation can starve every other Aurora task.
- Every lightweight task reserves a fixed 1 MiB coroutine stack. The MIR/direct runtime entry thread reserves 64 MiB, and maintained execution paths stop with a friendly recursion-depth diagnostic after 256 nested Aurora calls.
- The bootstrap scheduler scans waiting tasks for readiness and rebuilds a host `poll` descriptor list. Readiness work is linear in the number of waiting tasks/descriptors; no high-scale task-count claim is made for 0.1.
- Filesystem one-shot reads and `fs.File` whole-file reads are capped at 256 MiB of remaining content. Aurora 0.1 has no chunked file-read API.
- Process-pipe and captured-output reads plus TCP, Unix, and TLS whole/bounded reads remain capped at 64 MiB. TLS certificate, private-key, and CA-file loading uses the same independent 64 MiB ceiling. A bounded byte count of zero is invalid.
- UDP receives accept `max_bytes` from 1 through 65,535.
- Incoming HTTP parsing accepts at most 64 headers and 16 MiB of wire data per message, including the start line, headers, transfer framing, trailers, and body. Outbound HTTP writers have no separate size cap. The high-level map header model cannot preserve repeated equal field names losslessly.
- WebSocket messages are capped at 64 MiB; individual frames and write buffers are capped at 16 MiB.
- TLS handshakes have a 10-second hard cap even when the caller supplies no shorter timeout.
- Duration is a signed i128 nanosecond language value, but host timer ranges are narrower. Negative values, out-of-range host conversions, and overflowing deadline calculations are invalid input rather than unlimited waits. The exact error classification remains Provisional under ADR-0019.
- High-level HTTP clients support HTTP/1.1 over `http://` and validated `https://`, including content-length, chunked, and close-delimited responses; redirects, pooling, HTTP/2, proxy configuration, decompression, and high-level custom CA arguments are not implemented.
- JSON and TOML codecs currently support the typed `Map[String, String]` boundary, not nested dynamic trees or derived class/enum schemas.
- Metrics are process-global counters within one running program; log and trace APIs emit structured stderr records and do not yet include exporters or scoped spans.
- Floating-point `/`, `//`, or `%` by zero traps at runtime instead of producing IEEE 754 infinity or NaN.
- `float32` literals that overflow may currently become infinity; prefer `float64` when large literal validation matters.
- Unix domain sockets require a Unix host.
- TLS APIs require PEM certificate/key assets.
- Package support has local path and git dependencies, but no registry publish/install flow.
- `fs.read_dir` silently skips an individual directory entry that fails after the directory itself was opened.
- High-level HTTP header conversion may expose duplicate equal map keys when the wire message repeats a header name; repeated headers are not a lossless 0.1 contract.
- Resource-bearing task results are single-observer-only in Aurora 0.1. This restriction is not yet enforced statically: each observation clones the stored runtime value and can alias one host resource through shared handles, so use exactly one designated observer. Repeated observation is supported only for copy data or explicitly shared synchronized handles.
- Cancelling filesystem and other blocking-worker I/O cancels Aurora's wait, not an operating-system call already in progress. External side effects may still complete.
- The process-wide blocking pool uses 2 through 8 host threads, selected from host parallelism, with no 0.1 configuration or queue backpressure. A timed-out or cancelled host job keeps its worker until the underlying call returns; enough slow or stuck DNS/filesystem jobs can occupy the whole pool and delay unrelated blocking operations queued behind them.
- `WebSocketListener` has no explicit `close()` method, and WebSocket cancellation/error propagation is not yet fully aligned with TCP and UDP.

## Tooling

- `build` requires a host C compiler. Source-checkout builds may use Cargo to refresh the native runtime; release archives carry that runtime and do not require Rust or the source checkout.
- The direct backend is the maintained native backend for the implemented language surface.
- The default `--backend auto` first tries direct emission and may package an embedded-MIR launcher when direct emission is unavailable. Use `--backend direct` when fallback is unacceptable.
- Editor tooling uses a persistent compiler service. If that process is unavailable, recovery is lexical only and intentionally has no semantic diagnostics or member inference.
- `aura fmt` currently normalizes line endings, trailing whitespace, and final newlines; it is not yet a syntax-reflowing formatter.
- `aura test` treats each selected `.au` file as an executable test program and succeeds when execution succeeds with a zero integer `main` result; function-level test discovery is not implemented.
- A timed-out `aura test` stops waiting but cannot terminate its worker thread; the timed-out program may continue host side effects until the process exits.
- Recursive `aura fmt` and `aura test` traversal follows directory symlinks without cycle detection in 0.1.
