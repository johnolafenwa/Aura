# Current Limits

This page lists what the Aura compiler, runtime, and tools do not implement today, and the fixed limits they enforce. Limits are grouped by area.

## Language

### Source Text And Layout

- Identifiers are ASCII. Unicode is allowed in string contents, not in identifier spelling.
- A physical tab outside a triple-quoted string is rejected. Inside a triple-quoted string it is exact content. Use `\t` in an ordinary string.
- Physical newlines continue a logical line only while `(`, `[`, or `{` remains open. Continuation indentation is visual, and delimiter kinds must match.
- Backslash continuation is not implemented.
- Ordinary, raw, and f-strings are single-line. Triple-quoted ordinary strings may span physical lines.
- Source lists do not accept trailing commas. The one exception is the required comma in singleton tuple values, types, targets, and patterns. Multi-element tuples reject a trailing comma.
- The parser's nesting, postfix-chain, and binary-chain guards allow 128 operations. Deeper input is rejected with a diagnostic.
- Statement match arms cannot be inline. An expression match arm may put its expression on the same line after `case pattern:` or use an indented expression body.
- A `for` loop binding cannot shadow a name already visible in the same scope.

### Numbers And Durations

- Integers are fixed-width.
- Aura has no arbitrary-precision integer, implicit width promotion, rotate operator, literal suffix, hexadecimal floating literal, or distinct unsigned-right-shift operator.
- Power is builtin-only.
- `round` has no digit-count overload.
- Non-numeric casts are not implemented.
- Duration literals have only the integral `ms`, `s`, and `m` suffixes. There is no `ns` suffix, no fractional Duration literal, and no unary `-Duration`. Use the associated constructors and checked Duration arithmetic for signed and sub-millisecond values.

### Types, Classes, And Returns

- Direct recursive fields require `indirect`.
- Empty list, dictionary, and set literals need an expected collection type.
- Class field defaults cannot call user-defined functions. Compute the value before construction and pass it as an explicit field argument.
- An ordinary `-> T` return value is always owned. A Copy result is an ordinary copy. A non-Copy result must be constructed, cloned when clone-safe, moved from owned input, or produced by an owner operation.
- A function may instead declare one `-> view [mut] T from origin` result tied to a receiver or parameter place.
- A union value passed through a generic bound dispatches its shared and consuming trait methods on the active member on both backends. A `mut self` trait method reached this way runs on the interpreter. The direct backend reports `AU4001` at that call because it has no write-back path to the active payload behind an erased place. Narrow the union to its member before a mutable call.
- The MIR interpreter allocates a box for each union payload and closure environment. MIR is Aura's mid-level intermediate representation. The direct backend stores unions and callables inline, as [Performance](/manual/performance#representation) describes. Aura makes no foreign-function or application binary interface (ABI) stability claim for these layouts.

### Strings And Bytes

- `str(...)` is not a constructor. Use string literals and string methods.
- Ordinary and triple-quoted strings may use single or double quotes. F-strings use double quotes.
- Raw strings are single-line. Raw triple strings, raw f-strings, and byte-string literals are not implemented.
- F-strings use static format specifications. Dynamic width, nested fields, conversion flags, locale formatting, and the `g`, `G`, `n`, `c`, `#`, `0`, and `=` forms are not implemented.
- `str` has scalar-count `len()`, UTF-8 `byte_len()`, and owned Unicode-scalar slicing. It has no integer indexing, `chars()`, `ord()`, or `chr()`.
- One concatenated or formatted `str` result is limited to 64 MiB. Aura checks each append before it happens and reports `AU4005` without committing an oversized partial result.
- `list[uint8]` is the bytes type, and UTF-8 conversion is explicit. The reserved `encoding` argument, non-UTF-8 text codecs, byte-string literals, URL-safe or unpadded base64, streaming codecs, incremental hashes, and HMAC are not implemented.

### Tuples

- Tuples have fixed structural types, recursive unpack targets and patterns, copy-only constant indexing, and non-consuming recursive `==` and `!=` for operands of the same static tuple type.
- There is no empty tuple, multi-element trailing tuple comma, tuple iteration, tuple method, tuple ordering, named or rest unpacking, mutable tuple-target writeback, dynamic or negative tuple indexing, or tuple-to-collection conversion.
- To take ownership of a non-Copy element, unpack the tuple.

### Views

- Views have identity only for these places: local, parameter, and receiver roots; existing views; class-field paths; fixed tuple positions; list elements; and dictionary entries. Field and tuple projections inside an element are included.
- Set elements, slices, arbitrary temporaries, and escaping enum-payload views cannot be loaned.
- A returned view of a list element or dictionary entry holds the whole collection at the call site. The caller cannot read a different element while it is live.
- A non-copy value, including a list element, cannot be borrowed into a `T | None` or other union parameter (`AU2010`). Pass a clone, or change the parameter to the member type.
- Beside a live element view of the same list, `set(index, value)` works only for a Copy element type. For other element types it is refused with `AU3011`, because it would move the old element out through the collection.
- In a top-level script, an immutable binding is a module constant and cannot be the source of a view (`AU3004`). Bind it with `mut` or inside a function.
- View-bearing aggregates, module storage, multi-origin results, returned loan closures, and lifetime-parameterized structural callable types are not available.
- Views and loan closures are never Transfer.

### Function Values

- Capture-free named functions are Copy, `Transfer` values. They may be stored, called through `def(T1, mut T2, own T3) -> R` types, and used as task targets. Bare function-type parameters are shared.
- `receiver.method` binds a closure over the receiver with the method's contract. `Class.method` on a non-generic class is a function value.
- A generic method's type arguments come from `method[T]` or an expected contract. An associated method of a generic class still needs a call.
- A stored callable may return a view of one named parameter. A `from self` view result and loan captures cannot be stored.
- Bare callable destinations require identical complete contracts (`AU2015`). A safe restriction needs a thin alias adapter such as `Unary(function)`, or an explicit `Callable[...]` or `TaskCallable[...]` constructor. The callable member of a union destination such as `(def(value: int64) -> int64) | None` is a bare destination too.
- Callable identities are tracked through at most eight levels of class field nesting. The MIR validator refuses a call through a callable stored deeper than that.

### Lambdas And Closures

- A lambda with parameters requires complete expected parameter types. A zero-parameter lambda may infer `def() -> R` from its expression body.
- A lambda without a capture list captures by value. An explicit exhaustive `[value, mut value, own value]` list creates shared loans, mutable loans, or owned captures.
- Lambdas cannot have inline parameter types, defaults, generics, or statement bodies.
- A mutable-loan closure is repeatable through a mutable place. A consuming closure is single-use.
- Capturing closures cannot pass through arbitrary written-`def` parameters, fields, collections, or annotated returns. Those boundaries describe capture-free code pointers.
- Compiler-known repeatable callback sites keep closure metadata. Task start accepts a qualifying closure by move for one call.
- Conditional and `match` expressions merge capturing closures only through an explicit common `Callable[...]` contract on the destination. Otherwise, invoke the closure within each branch, or use capture-free lambdas or named functions.
- Owned callable storage, `Callable[...]` and `TaskCallable[...]`, holds owned captures only. It is packed explicitly through an alias call.

### Comprehensions And Slices

- List, set, and dictionary comprehensions are eager and always return fresh owned collections.
- Comprehension clauses use bare-loop iteration only. There is no comprehension `mut` or `own` source form, no early `break` or `continue`, and no lazy result.
- Nested clauses are outer-major. A Queue comprehension receives until ordinary Queue iteration ends.
- Generator expressions are not available. They report `AU2005` and point to an eager comprehension or an explicit loop.
- List and str slices accept one contiguous half-open range and return fresh owned copies.
- Written slice endpoints use `int64`. Negative endpoints normalize once. Invalid or reversed ranges trap with `AU4003`. Endpoints are not clamped.
- str slice endpoints count Unicode scalar values, and str slicing is O(n).
- Slice steps and slice assignment are reserved `AU2005` forms.
- Arbitrary sliceable types, indexed zero-copy views, str integer indexing, grapheme slicing, and Python-style endpoint clamping are not available.
- List slicing requires clone-safe, repeatably observable elements.

### Collections And List Algorithms

- `for value in mut set:` is not supported.
- Callable-powered list algorithms are eager. `map` and `filter` return owned lists, and `filter` requires clone-safe elements.
- Built-in natural sorting covers all integer types, `float32`, `float64`, and `Duration`. `str` has no built-in `Ord[str]`.
- To order text records, preserve insertion order, use `sort(key=callback)` with an orderable key or index, or define a nominal type with an application-specific `Ord` implementation.
- Keyed `sort`, `map`, and `filter` accept only their exact bare or shared callback parameter capabilities. The callback may be a named function, a repeatable closure, or a packed Shared callable value.
- There is no comparator-form sort, lazy map or filter, parallel traversal, mutable standard-library algorithm callback contract, or algorithm callback with mutable or owned element access.

### Numeric Arrays

- Numeric `Array[T]` is CPU-only, contiguous, and row-major. It is specialized only by `int32`, `int64`, `float32`, or `float64`.
- Shape is runtime metadata. Rank is at least one, and zero dimensions are allowed.
- Same-dtype scalar broadcast is implemented. There is no array-shape broadcasting, mixed promotion, equality, views, reshape, transpose, matrix multiplication, multidimensional or step slicing, slice assignment, autograd, accelerator placement, distributed storage, or foreign-buffer aliasing.
- First-axis slices are fresh owned copies.
- Maintained NumPy comparisons record exact post-reboot workloads and provenance.
- Shape elements, coordinates, and element counts use `int64`. Practical Array size is bounded by address space, allocation limits, element size, and available memory.

### Foreign Functions

The foreign function interface (FFI) is at version 0.

- FFI v0 is package-only and requires `[package] allow_ffi = true`. A root package also reports every reachable FFI-enabled dependency under exact `[ffi] dependencies`.
- Calls resolve already-loaded process-global symbols and run synchronously on the current worker.
- The accepted ABI is limited to fixed-width scalars, temporary str and byte pointer-length views, same-length mutable byte scratch copy-in/out, and non-null opaque handles. Empty views use `(NULL, 0)`.
- There is no library-loading or link-name syntax, callback, variadic, raw-pointer arithmetic, returned view, foreign aggregate layout, automatic handle destructor, or async offload.
- The only nullable handle is an extern result of exactly `Handle | None`. See [FFI v0](/manual/ffi).
- Process-global lookup is supported on Unix-family hosts.
- A false C signature or a misbehaving native function is outside Aura's safety guarantees and may terminate or corrupt the process.

### Tasks

- `TaskGroup.start(...)` and `start_soon(...)` support bare shared and `own` target parameters. `mut` targets are rejected because child tasks cannot write back through the starting call frame.
- Detached lightweight tasks are not a language form. Use `TaskGroup`.

## Runtime

### Runtime Diagnostics

- MIR and native direct-backend traps carry the same typed Aura call frames and task ancestry. Call frames are innermost first, and task ancestry is youngest first. Every frame keeps its own defining or spawning source path.
- Human diagnostics synthesize the compact call-chain, task-entry, and task-ancestry note lines.
- Structured schema-version-1 diagnostics expose `call_frames` and `task_ancestry` arrays instead. They do not duplicate the generated frame prose in `notes`.
- Aura does not expose host Rust/Cranelift backtraces, debugger stack reflection, exception catching, or a standalone-binary JSON switch.

### Scheduling

- Aura task code runs on pinned cooperative scheduler workers. The default count is the available parallelism reported by the host. The `AURA_WORKERS=<positive integer>` override selects an explicit count.
- A child is assigned to a worker when it is spawned and keeps that worker for its lifetime. Coroutine stacks never migrate, and the runtime does not steal work between workers.
- A positive `AURA_WORKERS` value may exceed the host's available-core count. Empty, zero, signed, whitespace-padded, nonnumeric, and overflowing values are rejected before execution with `AU4006` and ``invalid AURA_WORKERS value `<raw>`: expected a positive integer``.
- Scheduling is cooperative, not preemptive. The compiler checks every loop backedge and eventually yields from a tight loop, but only to runnable work assigned to that task's worker.
- One long loop body or long straight-line computation can still delay siblings pinned to the same worker. The automatic checks do not inspect cancellation.
- Queue and Task handles are the maintained cross-worker communication surface. All other task captures and results must be owned `Transfer` values, which keeps a share-nothing boundary.
- A task's cancellation and diagnostic context stay isolated from work running on other workers.
- Task scheduling, cross-worker completion, and program-output order are unspecified. There is no worker-index or affinity-introspection API.
- Pinned task execution is maintained on the MIR and direct native backends. Work stealing, preemption, and detached tasks are not available.
- Parallel speedup depends on the workload. Automatic parallelism applies only to task execution.
- MIR is the checked development path, not the performance path. In the multicore control benchmark, four MIR tasks took about `2.1x` the wall time of one task. Interpreter work and synchronization raise the per-task cost when several workers run MIR at once. Use the direct native backend for performance measurements.
- The scheduler uses persistent reactor registrations for nonblocking descriptors, a timer heap for deadlines, and direct Queue, task-completion, and blocking-pool notifications. When idle it blocks until an event or deadline. It has no periodic scheduler tick.

### Task Results

- Non-Transfer task captures, task results, and Queue payloads are rejected with `AU3008`.
- Every other non-repeatable transferable task result has one statically enforced observation right. Direct result methods consume it on every outcome, and multi-task waits consume the complete task list.
- A second runtime claim that reaches the atomic containment check traps with `AU4001`. It does not return or clone the stored value.

### Task Stacks

- An ordinary lightweight task requests 768 KiB of writable coroutine stack.
- `TaskGroup.start_with_stack` and `start_soon_with_stack` accept exact `int64` requests from 256 KiB through 64 MiB inclusive.
- Accepted requests are rounded up to the host page size and guard-protected. Smaller and larger requests are rejected, not clamped.
- The stack override API is provisional.
- The MIR and direct runtime entry thread reserves 64 MiB. Maintained execution paths stop with a recursion-depth diagnostic after 256 nested Aura calls.
- On the MIR backend, a call that would leave less than the interpreter's headroom reserve on the task's writable stack traps with `AU4005` first. The reserve is 128 KiB in optimized builds and 224 KiB in debug-assertion builds, measured above the guard page. The direct backend has only the depth limit.
- The 256 KiB lower bound is an opt-in minimum for measured shallow tasks, not a generally safe default. The complete compiled Aura HTTP example faulted when 256 KiB was the global default and succeeded with a 512 KiB default.
- An isolated runtime protocol round trip succeeds with 256 KiB callers because it excludes compiled language-execution frames. It proves the service offload boundary, not a 256 KiB whole-program default.

### Task Memory Measurements

These figures come from the clean Mac14,9 measurement at `181204b`.

- 10,000 parked sleepers used 207,798,272 bytes of worst whole-process resident set size (RSS) and 198,787,072 bytes above the same-process pre-spawn baseline. This passes the maintained 512 MiB gate.
- The runtime accepts larger task counts, but 10,000 sleepers is the maintained memory-capacity bound.
- Three repetitions of 100,000 sleepers plus 1,000 timers peaked at 1,170,735,104, 1,921,531,904, and 2,001,305,600 bytes. Two of the three exceed the 1.5 GiB gate.
- On this 16 KiB-page host, one resident page for each of the 101,000 stackful child coroutines alone requires 1,654,784,000 bytes, before task metadata or the root runtime. An earlier passing observation depended on memory compression and reclaim behavior.
- The contractual 10,000-sleeper bound and the timer, idle, starvation, and multicore controls all pass. Standalone timers had a 6 ms worst arm span and 1 ms p99 overshoot. Idle CPU was below 2%. Starvation latency was 14 ms.
- The four-worker control had a `1.039673x` paired median wall-time ratio with `396.73%` median four-task process CPU on the measured Mac14,9 host.

### Protocol-Step And Blocking-I/O Pools

- Deep HTTP, TLS, and maintained Unix WebSocket library frames run on a separate protocol-step pool with two 2 MiB-stack workers and a 64-job queue.
- Each submitted protocol job is a bounded, nonblocking step. It returns owned protocol state before cancellation or reactor waiting resumes.
- The non-Unix WebSocket fallback does not use the protocol-step pool.
- The protocol-step pool is process-global, lazily initialized, shared by all lightweight schedulers, and lives for the whole process. It has no 0.3 runtime shutdown or join API.
- File reads, resolver work, and listener binding run on the generic blocking-I/O pool. TLS asset bytes are read there, and then PEM parsing and rustls construction run on protocol workers.
- The process-wide blocking-I/O pool defaults from host parallelism, with fallback `4` and a derived `2..=8` clamp. `AURA_BLOCKING_WORKERS=<positive integer>` requests that exact count without clamping.
- `AURA_BLOCKING_QUEUE_CAPACITY=<positive integer>` bounds pending accepted jobs only. Running jobs and admission waiters do not consume it. Without it, the queue is unbounded.
- Full-queue admission is FIFO and scheduler-aware.
- The first runtime preflight reads these settings once, without starting workers. They stay fixed for the process lifetime.
- The first submission creates the complete worker set. Production reuses it until process exit and has no Aura shutdown or join surface.
- The queue capacity bounds the accepted pending backlog, not admission waiters. It cannot interrupt a stuck accepted call or guarantee unrelated blocking-I/O progress while every worker is occupied.
- Cancelling filesystem and other blocking-worker I/O cancels Aura's wait, not an accepted operating-system call. Timeout or cancellation before insertion into the pending queue prevents submission.
- After insertion, the host operation runs exactly once. Its external side effects may still complete while its late result is discarded.

### Size Limits

- Filesystem one-shot reads and `fs.File` whole-file reads are capped at 256 MiB of remaining content. Aura 0.3 has no chunked file-read API.
- Process-pipe and captured-output reads are capped at 64 MiB. So are TCP, Unix, and TLS whole and bounded reads.
- TLS certificate, private-key, and CA-file loading uses the same independent 64 MiB ceiling.
- A bounded byte count of zero is invalid.
- UDP receives accept `max_bytes` from 1 through 65,535.
- Incoming HTTP parsing accepts at most 64 headers and 16 MiB of wire data per message. The 16 MiB covers the start line, headers, transfer framing, trailers, and body. Outbound HTTP writers have no separate size cap.
- WebSocket messages are capped at 64 MiB. Individual frames and write buffers are capped at 16 MiB.
- Byte-codec inputs have no separate byte-count cap. Byte conversions and the hex and padded-base64 codecs check each fresh destination against a fixed 2,147,483,647-byte safety ceiling first.
- Crossing that codec output ceiling, or failing allocation, traps with `AU4005`. The ceiling is independent of the public str and `list` length domains.
- SHA-256 always returns 32 raw bytes.

### Networking

- TLS handshakes have a 10-second hard cap, even when the caller supplies no shorter timeout.
- TLS APIs require PEM certificate and key assets.
- High-level HTTP clients support HTTP/1.1 over `http://` and validated `https://`, including content-length, chunked, and close-delimited responses. Redirects, pooling, HTTP/2, proxy configuration, decompression, and high-level custom CA arguments are not implemented.
- The high-level dictionary header model cannot preserve repeated equal field names losslessly. When the wire message repeats a header name, conversion may expose duplicate equal dictionary keys. Repeated headers are not a lossless 0.3 contract.
- Unix domain sockets require a Unix host.
- `WebSocketListener` has no explicit `close()` method. WebSocket cancellation and error propagation are not fully aligned with TCP and UDP.

### Time

- Duration is a signed i128 nanosecond language value, but host timer ranges are narrower.
- Negative values, out-of-range host conversions, and overflowing deadline calculations are invalid input, not unlimited waits. The exact error classification is fixed by the Duration design record.

### JSON

- JSON supports the recursive `json.Value` tree, typed `json.Error` parse failures, and deterministic dumps.
- JSON has a 128-container depth limit and a shared root-inclusive 262,144-value materialization limit. Parse input and dump output have independent 64 MiB caps.
- Exceeding the node limit, or a controlled parse or conversion allocation failure, traps with `AU4005`. It is not a `json.Error` variant.
- Dynamic `json.parse` uses a separate process-global service with two 2 MiB-stack workers and a total in-flight capacity of two.
- The service reserves capacity before the fallible source copy. Lightweight tasks that find it saturated park through the scheduler. Once admitted, a synchronous parse defers cancellation until the codec completes.
- Runtime materialization, JSON-aware clone and render, and dumping use iterative traversals.
- The parse service lives for the whole process and has no 0.3 sizing or shutdown API.
- The bounded `json.is_valid` and `json.parse_string_map` helpers keep their bounded caller-side paths and do not use that service.
- JSON flat-dictionary and TOML helpers are restricted to typed `dict[str, str]`.
- JSON has no arbitrary-precision number, streaming codec, or derived class or enum schemas.

### Randomness

- `random.Rng` provides one fixed deterministic stream with integer, floating, and mutable-list shuffle operations.
- There is no global generator, state serialization, reseeding, jump or substream operation, distribution library, choice helper, public direct or transitive clone route, secure floating function, or `random.Error`.
- Clone-producing collection operations are rejected with `AU3007` when their produced value contains or may contain an `Rng`.
- An owned generator may move within one owning task, but it is not `Transfer`. It cannot be a task result or Queue payload.
- Queue handle copies stay valid. A Task handle is copyable only for a repeatable result.
- Generic clone-safety requirements are inferred from callable bodies, propagated through generic calls and imports, and checked after specialization. There is no source annotation for them.
- Trait defaults may establish the clone-safety contract, but an explicit implementation may not strengthen it. Recursive nominal inspection stops conservatively when safety cannot be proved.
- `secure_bytes` accepts at most 2,147,483,647 bytes per request. This is a fixed resource and safety ceiling, independent of the public `list` length domain.
- Larger `secure_bytes` counts fail with `AU4005` before allocation or entropy. Within the ceiling, an unsatisfied allocation or OS entropy request also traps with `AU4005`.

### Retry

- `control.retry` is a sequential eager helper for a repeatable `def() -> Result[T, E]` worker.
- The worker may be a capture-free function value, a repeatable capturing closure, or a packed Shared `Callable` or `TaskCallable` value. Mutable and Consuming workers are rejected with `AU2002`.
- Every `Err` is retryable. There is no error classifier, jitter, attempt hook, shared retry budget, or detached or parallel mode.
- Attempt budgets below one and negative or host-unrepresentable backoffs trap before the worker runs.
- Backoff overflow traps, and worker traps propagate. Task cancellation is not converted to the worker's `E`.

### Other Runtime Limits

- Floating-point `/`, `//`, or `%` by zero traps at runtime instead of producing IEEE 754 infinity or NaN.
- `float32` literals that overflow may become infinity. Prefer `float64` when large literal validation matters.
- Metrics are process-global counters within one running program.
- Log and trace APIs emit structured stderr records. They do not include exporters or scoped spans.
- Package support has local path and git dependencies, but no registry publish or install flow.
- `fs.read_dir` silently skips an individual directory entry that fails after the directory itself was opened.

## Tooling

- `build` requires a host C compiler. Source-checkout builds may use Cargo to refresh the native runtime. Release archives carry that runtime and do not require Rust or the source checkout.
- Native `run` cache entries larger than 512 MiB are not retained. The just-built program still runs, but a later invocation rebuilds it instead of using the cache.
- The direct backend is the maintained native backend for the implemented language surface.
- The default `--backend auto` first tries direct emission. It may package an embedded-MIR launcher when direct emission is unavailable. Use `--backend direct` when fallback is unacceptable.
- Editor tooling uses a persistent compiler service. If that process is unavailable, recovery is lexical only, with no semantic diagnostics or member inference.
- `aura fmt` normalizes line endings, trailing whitespace, and final newlines. It is not a syntax-reflowing formatter.
- `aura test` discovers each parameterless `def test_*()` function as a separate result. A file with no such function still runs as one file-level test.
- Optional `setup()` and `teardown()` run for each selected case. Teardown runs even after a setup or body failure.
- Parameterized registration returns labeled capture-free `def() -> None` values in `list[(str, def() -> None)]`. Registration runs before `-k` filtering.
- Test discovery is based on the name prefix. Test annotations are not implemented.
- A timed-out `aura test` stops waiting but cannot terminate its worker thread. The timed-out program may continue host side effects until the process exits.
- Recursive `aura fmt` and `aura test` traversal follows directory symlinks without cycle detection in 0.3.

### Native Artifact Profile

- The workspace release profile builds the shipped compiler executable with optimization level 3, fat link-time optimization (LTO), one codegen unit, no debug data, and symbol stripping.
- Panic unwinding stays enabled. The runtime archive keeps linkable symbols.
- Direct and embedded-MIR user executables link with `-Wl,-dead_strip` on macOS or `-Wl,--gc-sections` on Linux.
- After linking, `strip -S -x` removes debug and local symbols while keeping the globals that process-global FFI needs.
- Source locations, typed Aura call frames, and task ancestry are embedded runtime metadata and stay available. Native symbol and debugger information is reduced.
- The host toolchain must provide `strip` as well as `cc`.
- The compiler and runtime share one static archive.

## Design Record

- [ADR-0019: Duration conversion and timer policy](https://github.com/johnolafenwa/Aura/blob/main/architecture_docs/decisions/0019-duration-conversion-and-timer-policy.md)
- [ADR-0032: Guarded lightweight-task stacks](https://github.com/johnolafenwa/Aura/blob/main/architecture_docs/decisions/0032-guarded-lightweight-task-stacks.md)
- [ADR-0033: Structural Transfer and task-result consumption](https://github.com/johnolafenwa/Aura/blob/main/architecture_docs/decisions/0033-structural-transfer-and-task-results.md)
- [ADR-0038: Place-based loans and views](https://github.com/johnolafenwa/Aura/blob/main/architecture_docs/decisions/0038-place-based-loans-and-views.md)
