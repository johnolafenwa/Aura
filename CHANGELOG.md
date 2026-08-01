# Changelog

All notable user-facing changes are recorded here. Aura follows semantic
versioning for release artifacts while it remains a technical preview; a minor
preview release may still contain source and API incompatibilities called out
in this file.

## 0.2.0 — 2026-07-31 (technical preview)

Aura 0.2.0 is the first complete distribution of the implemented
language reference: a typed Python-shaped source language, deterministic
ownership model, structured-concurrency runtime, MIR and direct-native
backends, package tooling, language server, VS Code extension, and maintained
Learn and Manual tracks. It is intended for evaluation and controlled
experiments, not production deployment or execution of untrusted code. It was
developed under the working name Aurora before its first publication.

### Breaking changes and migration

- Replaced the source `borrow` capability family with three
  declaration-stable forms: bare parameters and receivers grant logical shared
  access for every type, including Copy types; `mut` grants exclusive mutable
  access; and `own` transfers ownership. Code that depended on the former bare
  Copy snapshot must now spell `own CopyType`.
- Changed bare `match` to shared matching. Use `match mut value` for mutable
  matching and `match own value` when the match must consume the scrutinee or
  owned payloads.
- Retired every old capability spelling with an exact replacement diagnostic:
  `value: borrow T` becomes `value: T`, `value: borrow mut T` becomes
  `value: mut T`, `borrow self` becomes `self`, `borrow mut self` becomes
  `mut self`, and the same change applies after `match` and
  `for value in`. The word `borrow` remains reserved for one compatibility release
  solely to teach these replacements; it is not an accepted alias.
- Removed borrowed-return labels and `borrow`/`borrow mut` return capabilities.
  Copy-valued returns are ordinary owned returns. An API that previously
  promised access into a non-Copy owner must return an owned value, handle, or
  index, or move the operation onto the owner. Place-based returned views are
  designed in ADR-0038 for 0.3 and are not implemented in 0.2.
- Run `python3 scripts/capability_migrate.py apply` from the repository root to
  migrate maintained source, then run
  `python3 scripts/capability_migrate.py check` to verify the recorded rewrite.
  Review inserted `own` on former bare Copy parameters and matches: those are
  the two old spellings whose meaning changed without containing a retired
  keyword.
- `String.len()`, `String.byte_len()`, `Vec.len()`, `Map.len()`, and
  `Set.len()` now return `int64`. Update `int32` annotations and use an
  explicit checked `as int32` conversion at still-narrow range or Vec-index
  boundaries. `String.byte_len()` is the UTF-8 byte count; `String.len()` is
  the Unicode-scalar count.
- The maintained builtin surface now reserves `len`, `str`, `select`,
  `SelectOutcome`, and the `control` module namespace. Rename conflicting user
  declarations.
- Lightweight tasks now run on pinned cooperative OS workers instead of one
  scheduler thread. Per-producer Queue order stays FIFO, but global sibling
  scheduling and output order are unspecified. Programs must synchronize any
  order they observe.
- The native artifact-cache format is `aura-native-cache-v5`; artifacts
  carrying older capability or backend metadata are intentionally rebuilt.

### Language

- Added fixed structural tuples, recursive unpacking and patterns, recursive
  equality, and whole-source copy/move behavior.
- Added conditional expressions, `in`/`not in`, chained comparisons,
  `enumerate` and `zip` loop forms, and maintained `len` and `str` builtins.
- Added eager owned list, set, and map comprehensions, including nested `for`
  clauses and left-to-right filters. Generator expressions remain rejected
  with guidance to use a comprehension or explicit loop.
- Added owned `Vec[T]` and Unicode-scalar `String` slicing with omitted
  endpoints and one-time negative normalization. Aura deliberately traps
  invalid or reversed ranges with `AU4003`; unlike Python, it does not clamp
  slice bounds. Steps, slice assignment, String integer indexing, and views
  remain unavailable.
- Added checked contextual narrow integers, wrapping and saturating integer
  methods, multiline delimiter continuation, assertions, bytes/codecs,
  deterministic seeded randomness, JSON, and the maintained typed
  control-plane modules.
- Kept the Manual reference-frozen: every accepted semantic addition has an
  ADR, executable conformance coverage, examples or tutorials where relevant,
  and MIR/direct parity evidence.

### Runtime and structured concurrency

- Fixed a queue-iteration livelock under oversubscription. Receive iteration
  now subscribes only to producers that are still running; a producer that
  has already completed can no longer keep every consumer in an immediate
  scheduler-ready loop while CPU burners occupy the worker pool. The reported
  iteration-consumer shape is pinned on MIR and direct at the default worker
  count.
- Replaced the periodic scheduler tick with persistent readiness registration,
  heap deadlines, cross-worker notifications, and loop-backedge fairness
  checks. Idle workers block until runnable work, I/O readiness, or a deadline.
- Added guarded 512 KiB default task stacks and explicit bounded stack-size
  overrides, compiler-derived structural `Transfer`, conditional task-handle
  Copy, and statically single-consumer non-repeatable results.
- Added typed heterogeneous `select(...)` across Queue, Task, and Duration
  sources, plus `wait_any`, `wait_all`, cancellation, scheduler-aware Queue
  operations, and structured child cleanup.
- Added a lazily created blocking-I/O pool. `AURA_BLOCKING_WORKERS` selects an
  exact positive worker count;
  `AURA_BLOCKING_QUEUE_CAPACITY` optionally bounds accepted pending work.
  Invalid settings fail before user code with `AU4006` on both backends.
- Expanded the maintained filesystem, process, TCP/UDP, HTTP, WebSocket, Unix
  socket, TLS, JSON, and control-plane surfaces with typed errors, explicit
  limits, timeout policy, cancellation behavior, and deterministic cleanup.
- Made the direct-native artifact cache content-addressed, integrity checked,
  concurrency safe on maintained Unix hosts, and optional for installed
  immutable runtimes. Long cold operations report whether Aura is waiting for
  a concurrent build or building the native program.

### Callables and closures

- Added structural `def(...) -> ...` types and Copy/Transfer named-function
  values, with inference, generic specialization, cross-module use, and both
  backend implementations.
- Added eager callable-powered `Vec.sort`, `sort_by`, `map`, and `filter`, and
  let `control.retry` accept repeatable capturing closures as well as named
  functions.
- Added contextually typed expression lambdas. Captures are by value: Copy
  values are snapshotted, non-Copy values move at creation, read-only closures
  are repeatable, and a consuming capture makes the closure single-use. A
  closure is Transfer only when each capture is Transfer.
- Callable equality is uniformly rejected for named function values and
  closures. Compare call results or carry an explicit discriminant instead.
- Mutable captured state, capability capture, nested capture of an enclosing
  lambda's bare parameter, and arbitrary persistence of capturing-closure
  metadata remain outside the 0.2 surface. ADR-0038 defines the planned 0.3
  loan/view foundation.

### Foreign function interface

- Added explicitly authorized FFI v0 packages and direct synchronous
  `extern "C"` calls to process-global symbols.
- The supported ABI includes fixed-width scalars, pointer-length String and
  byte views, and non-null opaque handles. Package manifests must opt in and
  dependency authorization is reported exactly from the root package.
- FFI declarations are a native trust boundary. False declarations or
  misbehaving C code can violate Aura's invariants; callbacks, variadics,
  raw pointer values, nullable handles, returned views, retained views, and
  dynamic-library selection remain unsupported. The MIR and direct backends
  use the same validated ABI description and host-call engine.

### Numeric arrays

- Added owned contiguous row-major `Array[T]` for `int32`, `int64`, `float32`,
  and `float64`, with shape metadata and rank of at least one.
- Added `zeros`, `full`, and `from_vec`; multidimensional get/set; `fill`;
  first-axis owned copying slices; map; sum/min/max/mean; exact-shape
  elementwise arithmetic; and scalar arithmetic.
- Elementwise and reduction work runs in dtype-specialized contiguous native
  runtime kernels. Aura makes no vectorization claim. The Manual records the
  baseline-host one-million-element measurements as evidence, not as a portable
  performance claim or gate.
- Arrays intentionally have no views, array-shape broadcasting, mixed-dtype
  promotion, equality, shape transformations, autograd, accelerators, or
  integer division. Use explicit casts and owned copies.

### Tooling and diagnostics

- `aura --version` now prints the preview channel and 12-hex-digit source
  commit (`aura 0.2.0-preview (<commit>)`), so preview executables are
  distinguishable from a future final 0.2.0. Release publication is marked as
  a GitHub prerelease and includes a generated, verified `SHA256SUMS` asset.
- Retired the last AU3002 diagnostic use of the old “shared borrowed values”
  wording; it now says “shared values” and retains the explicit stable code.
- Added `aura run --backend auto|mir|direct`, native builds with relocatable
  runtime/link manifests, a content-addressed native cache, function-level
  `aura test` discovery, recursive formatting/testing, package/workspace
  resolution and lockfiles, and compiler inspection/analysis commands.
- Added complete typed Aura call frames and child-task ancestry to MIR and
  direct runtime failures. Human diagnostics, schema-version-1 JSON, analysis,
  the language server, and the VS Code extension preserve the structured
  records. A private direct-runtime trap channel keeps ordinary process exit
  status distinct from `AU####` runtime failures.
- Added teaching diagnostics for retired syntax and unsupported Python-shaped
  forms, stable dedicated codes for semantic defect classes, exact
  UTF-8/source spans, completion recovery on incomplete programs, hover and
  go-to-definition, and maintained example-file regression coverage.
- Release archives carry the compiler, native runtime, and linker manifest.
  Installed archives can check, run, and build Aura without Cargo or a source
  checkout; a host C compiler is still required for native output.

### Current limits

- Aura 0.2.0 is not a stable compatibility promise, production
  systems release, sandbox, or security boundary for untrusted source.
- Release archives support glibc Linux x86-64 and macOS x86-64/Apple silicon.
  Windows, musl Linux, other architectures, and cross-compilation are
  experimental source-build territory.
- Structured concurrency is cooperative and pinned: it does not promise
  preemption, migration, work stealing, detached tasks, deterministic sibling
  order, or a speedup for every workload.
- Owned copies are the only 0.2 slicing model. ADR-0038's place loans, returned
  views, mutable closure capture, and view-aware concurrency checks target 0.3.
- Arrays are CPU-only and intentionally narrower than NumPy; externs are
  direct-call-only values on both maintained backends; imported modules have no
  runtime initialization side effects; package registries and publishing are
  not implemented.
- Resource caps, protocol boundaries, backend-specific FFI availability, and
  migration hints are normative in the Manual's
  [Current Limits](docs/manual/current-limits.md) and
  [Status And Compatibility](docs/manual/status-and-compatibility.md) pages.
