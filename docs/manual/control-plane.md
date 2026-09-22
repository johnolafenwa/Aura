# Control-Plane Modules

This page covers Aura 0.3's small, typed host and control-plane modules:
`sys`, `path`, `json`, `toml`, `log`, `trace`, `metrics`, and `control`, plus
a summary of the HTTP client. They are meant for service launchers, workers,
evaluation harnesses, and agent infrastructure. They behave the same through
`aura run` and in direct native binaries.

## System And Path

Import `sys` for process arguments, environment access, the current directory,
and clocks:

| API | Signature |
| --- | --- |
| `sys.args` | `args() -> list[str]` |
| `sys.env` | `env(name: str) -> str \| None` |
| `sys.current_dir` | `current_dir() -> Result[str, io.Error]` |
| `sys.unix_time_ms` | `unix_time_ms() -> int64` |
| `sys.monotonic_time_ms` | `monotonic_time_ms() -> int64` |

Pass program arguments after a separator:

```bash
aura run worker.au -- --model small --port 8080
./worker --model small --port 8080
```

`sys.args()` excludes the executable name.

- In `aura run`, arguments after `--` are passed explicitly into the MIR
  execution context and inherited by child tasks. MIR is the compiler's
  mid-level intermediate representation, which `aura run` executes.
- A built program reads its real host command line. Ambient environment
  variables cannot override it.
- On hosts that allow non-Unicode argv bytes, a built program replaces invalid
  byte sequences with the Unicode replacement character.

`sys.env` returns `None` both when a variable is missing and when its host
value is not valid Unicode. `sys.current_dir` and the string-producing `path`
helpers convert non-Unicode host paths lossily.

`unix_time_ms` returns milliseconds since the Unix epoch. `monotonic_time_ms`
returns milliseconds since the first call to that function in the current
process. Use it to compare elapsed time, not as a wall-clock timestamp.

`path` provides host-aware `join`, `parent`, `file_name`, `extension`, and
`is_absolute` operations. A component that may not exist is returned as
`str | None`.

## JSON And TOML

JSON represents an arbitrary recursive tree as `json.Value` and reports typed
parse failures as `json.Error`:

| API | Signature |
| --- | --- |
| `json.parse` | `parse(text: str) -> Result[json.Value, json.Error]` |
| `json.dumps` | `dumps(value: json.Value, indent: int64 \| None = None) -> str` |

[JSON Module](/manual/json) lists the exact inspecting and consuming
accessors. It is the normative contract for number classification, source
positions, ordering, formatting, and resource limits.

Flat `dict[str, str]` helpers give a typed data API alongside the dynamic JSON
tree. TOML uses the same typed top-level dictionary boundary:

| API | Signature |
| --- | --- |
| `json.is_valid` | `is_valid(text: str) -> bool` |
| `json.stringify_map` | `stringify_map(value: dict[str, str]) -> Result[str, str]` |
| `json.parse_string_map` | `parse_string_map(text: str) -> Result[dict[str, str], str]` |
| `toml.is_valid` | `is_valid(text: str) -> bool` |
| `toml.stringify_map` | `stringify_map(value: dict[str, str]) -> Result[str, str]` |
| `toml.parse_string_map` | `parse_string_map(text: str) -> Result[dict[str, str], str]` |

- JSON compact output sorts object keys.
- `json.is_valid` accepts any valid JSON value. `json.parse_string_map`
  succeeds only for an object whose values are all strings.
- TOML output is a sorted top-level string dictionary.
- `toml.is_valid` accepts any valid TOML document. `toml.parse_string_map`
  rejects nested tables and non-string values.

Aura 0.3 has no derived class or enum schemas and no generated codecs.

## Logs, Metrics, And Traces

`log.debug/info/warn/error(message, fields)` and `trace.event(name, fields)`
each write one compact JSON record to standard error. `fields` is a
`dict[str, str]`. Every record has the shape
`{ "kind": "log" | "trace", "level": str, "message": str, "fields": Object }`.
For a trace event, `level` is `event` and `message` is the event name.

`metrics.increment(name, value)`, `metrics.get(name)`, and `metrics.reset()`
work on process-global signed `int64` counters that all Aura tasks in the
process share.

- A missing counter reads as zero.
- Incrementing past either `int64` bound is a runtime diagnostic and leaves
  the checked operation incomplete.

The counters suit worker and test instrumentation. Aura 0.3 has no metrics
exporter, export protocol, or scoped trace span API.

## Network Boundary

The HTTP client accepts `http://` URLs and certificate-validated `https://`
URLs. Validation uses the platform-independent Web PKI root set.

- Request and response bodies support content length, connection-close
  framing, and chunked transfer encoding.
- The 0.3 parser keeps a 16 MiB limit on an incoming wire message.
- It accepts at most 64 headers and rejects conflicting framing headers.
- Its `dict[str, str]` header boundary cannot represent repeated equal header
  names losslessly.

For custom certificate authorities and TLS servers, use the lower-level
`net.tls_connect*` and `net.tls_listen` APIs in
[Network Module](/manual/network).

## Example

This example is self-contained. It needs no host files, network access, or
environment settings:

```aura
import json

def main():
    print(json.is_valid("{\"ready\":\"yes\"}"))
```

See `examples/agents/control_plane_foundations.au` for path operations, JSON
and TOML metadata, counters, and structured events.

## Retry

Import `control` for the eager retry helper:

| API | Signature |
| --- | --- |
| `control.retry` | `retry[T, E](worker: def() -> Result[T, E], max_attempts: int32 = 3, initial_backoff: Duration = 0ms) -> Result[T, E]` |

The helper runs the worker until it returns `Result.Ok` or runs out of
attempts:

- The first attempt runs immediately.
- Every `Result.Err` is retryable while an attempt remains.
- Before the second attempt, the current task waits for `initial_backoff`.
  Each later retry waits twice the preceding delay.
- A zero delay skips sleeping.
- When the final permitted attempt returns `Err`, the helper returns that exact
  error without another sleep or delay multiplication.

`max_attempts` must be at least one and counts the immediate attempt.
`initial_backoff` must be non-negative and representable by the host timer.
The helper validates both arguments before it calls the worker.

```aura
import control
import metrics

def eventually_succeeds() -> Result[int32, str]:
    metrics.increment("attempts", 1)
    if metrics.get("attempts") < 3:
        return Result.Err("not ready")
    return Result.Ok(42)

def main():
    metrics.reset()
    match control.retry(
        eventually_succeeds,
        max_attempts=3,
        initial_backoff=0ms
    ):
        case Result.Ok(value):
            print(value)
        case Result.Err(error):
            print(error)
    print(metrics.get("attempts"))
```

The worker may be any of these:

- a capture-free function value
- a repeatable, Shared closure that captures values
- a packed Shared `Callable[def() -> Result[T, E]]` or
  `TaskCallable[def() -> Result[T, E]]` value, which the helper borrows for
  the operation with its ABI-equal contract

Two kinds of worker fail with `AU2002`:

- A consuming closure or an `own def` value, because retry may call the worker
  more than once.
- A Mutable closure or a `mut def` value, because the helper holds the worker
  through a shared read.

The helper does not convert worker traps, delay overflow, or invalid runtime
operations to `E`. Cancellation of the current task propagates through the
retry operation instead of returning the most recent `Err`.

See `examples/agents/retry_with_backoff.au` for both eventual success and the
exact last-error behavior.

## Grammar

These modules add no source-language grammar. Programs import and call them
with the ordinary import, call, member-access, named-argument, collection,
`Result`, and `T | None` union forms defined elsewhere in this reference.
Module and member names are case-sensitive. The `--` separator that supplies
`sys.args()` belongs to the CLI protocol, not to Aura syntax.

## Typing Rules

The function signatures in the tables above are normative.

- `sys.args()` produces owned `str` values in a `list`.
- Environment and path components that may be absent use `str | None`.
- Fallible current-directory access uses `Result[..., io.Error]`.
- Dynamic JSON parsing returns `Result[json.Value, json.Error]`.
- The bounded JSON and TOML dictionary operations keep their
  `Result[..., str]` contracts.
- Telemetry fields are `dict[str, str]`.
- Metric names are `str`. Increments and results are signed `int64`, and
  reset returns `None`.

The checker rejects any other argument type, an unknown member, or an
unsupported argument shape.

`control.retry` infers `T` and `E` from the exact shared callback type
`def() -> Result[T, E]`. A callback with parameters, a non-`Result` return
type, or a different function-value contract fails with `AU2002`.
`max_attempts` is exactly `int32`, and `initial_backoff` is exactly
`Duration`.

## Runtime Semantics

**Arguments and environment.** `sys.args()` returns program arguments without
the executable name. `aura run` uses the arguments after the CLI `--`. A built
program uses its host argument list. Environment lookup returns `None` for a
missing or non-Unicode value. Path operations use host path rules, and their
string results follow the lossy Unicode policy above.

**Data formats.** Dynamic JSON object output and JSON and TOML top-level
dictionary output are sorted by key. Dynamic JSON parse and dump follow the
recursive value, strict-number, and formatting rules in the JSON chapter.
Validation accepts the broader source format. Each `parse_string_map`
operation accepts only its documented flat-string dictionary subset.

**Telemetry.** Logging and trace calls synchronously write one compact JSON
record to standard error. Metric operations address one process-global
dictionary that all tasks share. A missing counter is zero, and reset clears
the dictionary. A checked overflow leaves the attempted increment unapplied.

**Retry.** `control.retry` calls its worker sequentially and never overlaps
attempts. After the immediate first attempt, it waits the current delay and
runs the next attempt only as needed. Every `Err` has the same retry policy.
On exhaustion the helper returns the final worker's exact `Err`. It performs
no terminal sleep and does not compute an unused next delay. A zero current
delay skips the scheduler sleep. A worker trap, backoff overflow, or
current-task cancellation propagates through the helper.

## Ownership And Evaluation Order

Call arguments are evaluated left to right. Inputs to these host helpers are
shared for the duration of the call. They are not kept as Aura values after
the call returns. Returned lists, dictionaries, strings, options, and results
are fresh owned values. The metrics implementation copies the metric name into
process-global host state and does not keep an Aura borrow alive.

Telemetry output and metric updates are observable side effects. They happen
at the call's position in source evaluation order. Concurrent tasks share
standard error and the metric dictionary. Each metric operation is
synchronized, but a sequence such as `get` followed by `increment` is not one
atomic transaction.

The retry helper reads its worker and calls it under ordinary call rules. A
packed callable value is borrowed for the operation and is never cloned,
moved, or erased. The helper can therefore reuse one repeatable worker across
all attempts without consuming its environment. Each `Result.Ok` or
`Result.Err` owns its payload. The retry decision consumes intermediate
errors, and the final error is returned without cloning. Attempt calls and
delay waits occur in the order given under [Runtime Semantics](#runtime-semantics).

## Diagnostics

| Code | Cause |
| --- | --- |
| `AU2001` | Unknown module or member. |
| `AU2002` | Type or callback-contract mismatch. |
| `AU2004` | Invalid argument binding. |
| `AU2999` | Remaining static rejections. |
| `AU4001` | A negative or host-unrepresentable retry `initial_backoff`. |
| `AU4002` | Incrementing a metric past either `int64` bound, or doubling a retry delay past the exact `Duration` range. The value does not wrap. |
| `AU4003` | A retry attempt budget below one. |

These typed outcomes are ordinary program data, not language diagnostics:

- Invalid JSON or TOML makes validation return `false`.
- Dynamic JSON parsing returns `Result.Err(json.Error)`.
- The bounded flat-dictionary operations return `Result.Err(str)` as
  documented.
- A missing environment variable returns `None`.
- A current-directory failure returns `Result.Err(io.Error)`.

JSON dumping has the runtime traps and limits specified in the JSON chapter.

## Backend Support

Every API on this page is implemented by both the MIR runtime used by
`aura run` and the direct native backend. Argument injection differs only at
the host boundary described above.

These behaviors are parity contracts between the two backends:

- recursive JSON parse and dump
- JSON and TOML ordering
- time units
- telemetry record shape
- checked metric arithmetic
- retry attempt order, backoff, exact final-error return, trap propagation,
  and cancellation

The HTTP client summarized here has the same MIR and direct support as the full
[Network Module](/manual/network). Path and environment results depend on the
host and may differ between hosts, but they keep their Aura types and error
policy.

## Limits And Implementation-Defined Behavior

**Host text and paths.** Non-Unicode host argument and path bytes follow the
lossy or absent-value policies above. Path separators, roots, case
sensitivity, and absolute-path rules follow the host.

**Clocks.** Unix time reflects the host clock and may move. Monotonic time is
process-local elapsed time with millisecond granularity. Its zero is the first
call in the process.

**Data formats.** Dynamic JSON has the fixed tree, numeric, depth,
node-materialization, and byte limits documented in
[JSON Module](/manual/json). TOML and the bounded flat-dictionary helpers keep
their limits. There are no derived codecs or streaming encoders.

**Telemetry.** Telemetry has no exporter, batching, scoped spans, or metric
labels. It guarantees delivery only as far as the standard-error write.
Concurrent standard-error records are written individually, but their order
across tasks follows scheduling.

**HTTP.** The HTTP limits are the 16 MiB incoming wire-message cap, the
64-header cap, the framing checks, and the repeated-header loss described
above and in [Current Limits](/manual/current-limits).

**Retry.** `control.retry` is an eager sequential helper, not a policy engine.
It has no error classifier, jitter, attempt callback, retry budget shared
between calls, or detached execution. Every `Err` is retryable. An application
that needs status-specific retry policy or jitter must write it explicitly.
Retry delays stay bounded by the host timer range described in
[Current Limits](/manual/current-limits).

## Status

The system, path, JSON, TOML, logging, trace-event, metrics, retry, and
summarized HTTP contracts on this page are implemented and maintained in
Aura 0.3.

Nested TOML data models, derived codecs, telemetry exporters, metric labels,
scoped tracing spans, and richer HTTP header representations are unavailable.
Any mention of them describes possible future direction, not accepted language
behavior.

**Design record.** The rationale is in these decision records:

- `architecture_docs/decisions/0021-json-value-model-and-codec-policy.md`:
  recursive JSON semantics. Accepted.
- `architecture_docs/decisions/0018-fixed-resource-read-limits.md`: the fixed
  HTTP cap summarized here. Accepted.
