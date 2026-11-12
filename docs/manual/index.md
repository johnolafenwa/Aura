# Aura Language Reference

This Manual is the normative reference for the implemented Aura language and
runtime. It covers the complete syntax, name and type rules, ownership,
execution, module APIs, diagnostics, limits, and tool contracts, in enough
detail to reconstruct the language accurately.

This Manual documents **Aura 0.15.0 (technical preview)**. The release stamp
below records the implementation baseline commit the Manual was rendered from.

<ReleaseStamp />

The stamp uses the first commit source that applies:

- `AURA_DOCS_COMMIT`, set by release builders
- `GITHUB_SHA`, in GitHub builds
- the checkout's committed `HEAD`, in a clean local build

A dirty or Git-free build shows `local-uncommitted-checkout`. It never invents
a commit or writes a self-referential hash into this source page.

The Learn track teaches Aura as a story. A future book may build a longer
learning sequence from this material. This reference defines the facts that
all such teaching material must preserve.

Start with [Language Specification](/manual/language-specification) for
scope, terminology, authority, and conformance language.

## Language Reference

- [Lexical Structure](/manual/lexical-structure): files, physical and logical
  lines, delimiter continuation, indentation, comments, identifiers, keywords,
  literals, f-strings, and duration literals.
- [Complete Grammar](/manual/grammar): the normative Extended Backus-Naur Form
  (EBNF) grammar, precedence, associativity, contextual syntax, layout, and
  unsupported forms.
- [Names And Scopes](/manual/names-and-scopes): modules, imports, visibility,
  bindings, block scope, the no-shadowing rule, and member lookup.
- [Types](/manual/types): primitive types, tuples, `None`, `Duration`, generic
  types, copy and move categories, and type annotations.
- [Static Semantics](/manual/static-semantics): inference, type equality,
  assignment, calls, operators, constructors, matching, traits, resources, and
  entrypoints.
- [Expressions](/manual/expressions): operators, calls, indexing, owned list
  and `str` slicing, member access, literals, conditional expressions,
  membership and comparison chains, `match` expressions, `try`, and f-strings.
- [Statements](/manual/statements): bindings, assignment, control flow, loops,
  imports, `with`, `pass`, assertions, and top-level execution.
- [Tuples](/manual/tuples): fixed structural values and types, recursive
  unpacking and patterns, whole-source ownership, constant indexing, and
  recursive equality.
- [Assertions](/manual/assertions): exact boolean conditions, lazy messages,
  `AU4001` failures, cleanup precedence, and backend behavior.
- [Contracts](/manual/contracts): `requires`, `ensures`, loop and class
  invariants, `decreases`, `old`, `forall`, `exists`, `spec def`, and
  `AU4009` failures.
- [Verification](/manual/verification): `aura verify`, which proves with an
  SMT solver that contracts, arithmetic, and list indexes cannot fail.
- [Functions](/manual/functions): signatures, the bare, `own`, and `mut`
  parameter modes, default and named arguments, `main`, owned returns, and
  call binding.
- [Closures](/manual/closures): contextual expression lambdas, by-value
  captures, repeated read-only calls, consuming single-use calls, and
  structural Transfer.
- [Foreign Function Interface (FFI) v0](/manual/ffi): explicit package
  authorization, bodyless C declarations, fixed-width scalars, pointer-length
  views, opaque handles, and the native safety boundary.
- [Classes](/manual/classes): fields, constructors, methods, receivers,
  associated methods, resources, and mutation.
- [Enums And Pattern Matching](/manual/enums-and-match): variants, payloads,
  exhaustiveness, literal patterns, short-form variants, and match value flow.
- [Generics And Traits](/manual/generics-and-traits): type parameters, trait
  declarations, impls, bounds, dispatch, and current restrictions.
- [Ownership And Borrowing](/manual/ownership-and-borrowing): moves, copies,
  clones, shared borrows, mutable borrows, field moves, and task boundaries.
- [Execution Model](/manual/execution-model): evaluation order, entry
  execution, backends, cleanup, runtime failures, scheduling, cancellation,
  and external effects.

## Runtime And Library Reference

- [Collections](/manual/collections): `list[T]`, `dict[K, V]`, and `set[T]`,
  with literals, eager owned comprehensions and slices, iteration, mutation,
  and eager list algorithms that take callables.
- [Numeric Arrays](/manual/numeric-arrays): row-major `Array[T]`
  with four numeric dtypes, slices and reshapes that share storage, reductions, native
  kernels, and explicit checked, wrapping, and saturating integer arithmetic.
- [SIMD Module](/manual/simd): lane values of two, four, or eight floats
  that native code keeps in vector registers, with loads and stores over
  lists and results that are the same on every machine.
- [Tensor Module](/manual/tensors): `Tensor[T]` values that live on a
  device, the CPU or the Metal GPU of a Mac, with the `Array` shape rules,
  one meaning on every device, and whole expressions run as one piece of
  GPU work.
- [Math Module](/manual/math): exact binary64 constants and scalar rounding,
  power, exponential, logarithmic, and trigonometric functions, with explicit
  domain and overflow behavior.
- [Bytes, Text Codecs, And SHA-256](/manual/bytes): `list[uint8]`, strict
  UTF-8 conversion, canonical hex and base64, typed data errors, raw
  SHA-256, and HMAC-SHA256.
- [Gzip Module](/manual/gzip): gzip compression and bounded decompression of
  byte lists, with typed errors.
- [Regex Module](/manual/regex): compiled regular expressions with
  linear-time search, groups, replacement, and splitting.
- [Time Module](/manual/time): the clocks, `DateTime` with fixed offsets,
  Duration arithmetic, RFC 3339, and pattern formatting and parsing.
- [HTTP Module](/manual/http): the HTTP client, with streamed bodies,
  server-sent events, redirects, kept connections, and decompression.
- [JSON Module](/manual/json): recursive JSON values, typed parse errors,
  exact number classification, deterministic dumping, and resource limits.
- [Randomness Module](/manual/randomness): deterministic seeded streams, exact
  sequence compatibility, unbiased ranges, in-place shuffle, and OS-secure
  integers and bytes.
- [Concurrency](/manual/concurrency): `TaskGroup`, `Task[T]`, `Queue[T]`,
  cancellation, `yield_now`, typed heterogeneous `select`, `wait_any`,
  `wait_all`, and scheduler-aware waits.
- [I/O Module](/manual/io): standard input and output, and `io.Error`.
- [Filesystem Module](/manual/filesystem): one-shot helpers, `fs.File`, line
  and chunk reads, scoped file cleanup, and byte and text limits.
- [Network Module](/manual/network): TCP, UDP, HTTP, WebSocket, Unix sockets,
  TLS, and HTTP client helpers.
- [Process Module](/manual/process): subprocess spawning, pipes, completed
  processes, process groups, supervisors, and restart policy.
- [Control-Plane Modules](/manual/control-plane): system and path helpers,
  JSON and TOML compatibility APIs, telemetry, and `control.retry`.
- [Packages](/manual/packages): manifests, package roots, import resolution,
  lockfiles, and editor analysis behavior.
- [CLI And Tooling](/manual/cli-and-tooling): `aura` commands, diagnostics,
  analysis JSON, completions, and build modes.
- [API Index](/manual/api-index): every maintained builtin function, method,
  enum, and module type in one place.
- [Diagnostics](/manual/diagnostics): compile-time and runtime categories,
  source rendering, machine-readable positions, and CLI exit status.
- [Performance](/manual/performance): reproducible measurements, current gaps,
  evidence provenance, and the optimization direction for later releases.
- [Current Limits](/manual/current-limits): intentional current boundaries and
  practical workarounds.
- [Conformance](/manual/conformance): how executable fixtures and tests map to
  the Manual, and the rules for changing the language safely.

## Conventions Used In This Manual

Code blocks marked `aura` contain Aura code and use the documentation's Aura
highlighter. Shell blocks contain repository commands.
[Complete Grammar](/manual/grammar) defines the language grammar itself.

In signatures, `Duration = ...` marks an optional timeout parameter. The
relevant API section documents its default. Timeouts follow these general
rules:

- A blocking API waits when the timeout is omitted.
- A convenience helper ending in `_or_none` or `_or` may use an immediate
  non-blocking check when its API section says so.
- A timeout result is an explicit variant, such as `TimedOut`, `None`, or
  `process.Error.TimedOut`.
- An explicit timeout must be non-negative and fit the host deadline. An
  invalid value returns the documented typed InvalidInput or process error.
  When the API has no typed error carrier, it traps with `AU4001`.

Two words have fixed meanings:

- **Cloned**: a value returned "cloned" is a new owned value for the caller.
- **Moves**: when a method "moves" an argument, the caller cannot use that
  argument after the call unless it is a copy type.
