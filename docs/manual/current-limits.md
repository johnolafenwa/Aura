# Current Limits

This page documents known current limits of the Aurora compiler and runtime.

## Language

- Identifiers are ASCII; Unicode is supported in string contents, not identifier spelling.
- A physical tab anywhere on a source line is rejected, including inside a comment or string literal. Use `\t` to encode a tab in a string.
- Source lists do not accept trailing commas.
- Parser nesting/postfix/binary-chain guards are limited to 128 operations; deeper input is rejected with a diagnostic.
- Non-numeric casts are not implemented.
- Direct recursive fields require `indirect`.
- Borrowed return inference is explicit through borrowed-return sources and labels.
- Empty list, map, and set literals need an expected collection type.
- `String(...)` is not a constructor; use string literals and string methods.
- Newlines are not continuation inside `(...)`, `[...]`, or `{...}`. Keep calls and collection literals on one physical line today.
- Backslash line continuation is not implemented.
- Statement match arms cannot be inline. Expression match arms may use a same-line expression after `case pattern:` or an indented expression body.
- Comparison chains are ordinary left-associated binary expressions, not Python-style chained comparisons.
- `for` loop bindings cannot shadow names already visible in the same scope.
- Duration arithmetic and ordering, such as `100ms + 50ms` or `timeout < 1s`, is not implemented.
- Task starting currently supports named functions and associated methods without `self`.
- `TaskGroup.start(...)` and `start_soon(...)` do not yet support borrowed parameters.
- Detached lightweight tasks are not a language form; use `TaskGroup`.
- `for value in borrow mut set:` is not currently supported.

## Runtime

- Maintained execution paths stop with a friendly recursion-depth diagnostic after 256 nested Aurora calls.
- File, process-pipe, TCP, Unix, and TLS whole/bounded reads are capped at 64 MiB. Aurora 0.1 has no chunked file-read API. A bounded byte count of zero is invalid.
- UDP receives accept `max_bytes` from 1 through 65,535.
- HTTP parsing accepts at most 64 headers and 1 MiB per message. The high-level map header model cannot preserve repeated equal field names losslessly.
- WebSocket messages are capped at 64 MiB; individual frames and write buffers are capped at 16 MiB.
- TLS handshakes have a 10-second hard cap even when the caller supplies no shorter timeout.
- High-level HTTP clients support HTTP/1.1 over `http://` and validated `https://`, including content-length, chunked, and close-delimited responses; redirects, pooling, HTTP/2, proxy configuration, decompression, and high-level custom CA arguments are not implemented.
- JSON and TOML codecs currently support the typed `Map[String, String]` boundary, not nested dynamic trees or derived class/enum schemas.
- Metrics are process-global counters within one running program; log and trace APIs emit structured stderr records and do not yet include exporters or scoped spans.
- Floating-point division by zero traps at runtime instead of producing IEEE 754 infinity.
- `float32` literals that overflow may currently become infinity; prefer `float64` when large literal validation matters.
- Unix domain sockets require a Unix host.
- TLS APIs require PEM certificate/key assets.
- Package support has local path and git dependencies, but no registry publish/install flow.
- `fs.read_dir` silently skips an individual directory entry that fails after the directory itself was opened.
- High-level HTTP header conversion may expose duplicate equal map keys when the wire message repeats a header name; repeated headers are not a lossless 0.1 contract.
- Task results clone their stored value on each observation. A resource returned by a task can therefore be aliased through shared runtime handles; use one designated observer.
- Cancelling filesystem and other blocking-worker I/O cancels Aurora's wait, not an operating-system call already in progress. External side effects may still complete.
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
