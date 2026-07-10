# Current Limits

This page documents known current limits of the Aurora compiler and runtime.

## Language

- Non-numeric casts are not implemented.
- Direct recursive fields require `indirect`.
- Borrowed return inference is explicit through borrowed-return sources and labels.
- Empty list, map, and set literals need an expected collection type.
- `String(...)` is not a constructor; use string literals and string methods.
- Newlines are not continuation inside `(...)`, `[...]`, or `{...}`. Keep calls and collection literals on one physical line today.
- Backslash line continuation is not implemented.
- Inline match arms such as `case Variant: statement` are not implemented; put arm bodies on indented following lines.
- `for` loop bindings cannot shadow names already visible in the same scope.
- Duration arithmetic and ordering, such as `100ms + 50ms` or `timeout < 1s`, is not implemented.
- Task starting currently supports named functions and associated methods without `self`.
- `TaskGroup.start(...)` and `start_soon(...)` do not yet support borrowed parameters.
- Detached lightweight tasks are not a language form; use `TaskGroup`.
- `for value in borrow mut set:` is not currently supported.

## Runtime

- The MIR runtime stops with a friendly recursion-depth diagnostic after 256 nested Aurora calls.
- One-shot `fs.read_to_string(...)` and `fs.read_bytes(...)` are capped at 64 MiB.
- HTTP message parsing and client helper responses are capped at 1 MiB.
- High-level HTTP clients support HTTP/1.1 over `http://` and validated `https://`, including content-length, chunked, and close-delimited responses; redirects, pooling, HTTP/2, proxy configuration, decompression, and high-level custom CA arguments are not implemented.
- JSON and TOML codecs currently support the typed `Map[String, String]` boundary, not nested dynamic trees or derived class/enum schemas.
- Metrics are process-local counters; log and trace APIs emit structured stderr records and do not yet include exporters or scoped spans.
- Floating-point division by zero traps at runtime instead of producing IEEE 754 infinity.
- `float32` literals that overflow may currently become infinity; prefer `float64` when large literal validation matters.
- Unix domain sockets require a Unix host.
- TLS APIs require PEM certificate/key assets.
- Package support has local path and git dependencies, but no registry publish/install flow.

## Tooling

- `build` requires a host C compiler. Source-checkout builds may use Cargo to refresh the native runtime; release archives carry that runtime and do not require Rust or the source checkout.
- The direct backend is the maintained native backend for the implemented language surface.
- Editor tooling uses a persistent compiler service. If that process is unavailable, recovery is lexical only and intentionally has no semantic diagnostics or member inference.
- `aura fmt` currently normalizes line endings, trailing whitespace, and final newlines; it is not yet a syntax-reflowing formatter.
- `aura test` treats each selected `.au` file as an executable test program and succeeds when execution succeeds with a zero integer `main` result; function-level test discovery is not implemented.
