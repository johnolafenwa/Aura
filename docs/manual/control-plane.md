# Control-Plane Modules

Aura 0.3 includes a small, typed host/control-plane surface intended for service launchers, workers, evaluation harnesses, and agent infrastructure. These modules behave the same through `aura run` and direct native binaries.

## System And Path

Import `sys` for process arguments, environment access, the current directory, and clocks:

| API | Signature |
| --- | --- |
| `sys.args` | `args() -> list[str]` |
| `sys.env` | `env(name: str) -> Option[str]` |
| `sys.current_dir` | `current_dir() -> Result[str, io.Error]` |
| `sys.unix_time_ms` | `unix_time_ms() -> int64` |
| `sys.monotonic_time_ms` | `monotonic_time_ms() -> int64` |

Pass program arguments after a separator:

```bash
aura run worker.au -- --model small --port 8080
./worker --model small --port 8080
```

`sys.args()` excludes the executable name. In `aura run`, arguments after `--` are passed explicitly into the MIR execution context and inherited by child tasks. A built program reads its real host command line; ambient environment variables cannot override it. On hosts that permit non-Unicode argv bytes, built programs replace invalid byte sequences with the Unicode replacement character.

`sys.env` returns `None` both when a variable is missing and when its host value is not valid Unicode. `sys.current_dir` and the string-producing `path` helpers convert non-Unicode host paths lossily. `unix_time_ms` is milliseconds since the Unix epoch; `monotonic_time_ms` is milliseconds since the first call to that function in the current process and is suitable for elapsed-time comparisons, not wall-clock timestamps.

`path` provides host-aware `join`, `parent`, `file_name`, `extension`, and `is_absolute` operations. Components that may not exist return `Option[str]`.

## JSON And TOML

JSON supports an arbitrary recursive tree through `json.Value` and typed parse
failures through `json.Error`:

| API | Signature |
| --- | --- |
| `json.parse` | `parse(text: str) -> Result[json.Value, json.Error]` |
| `json.dumps` | `dumps(value: json.Value, indent: Option[int64] = None) -> str` |

Exact inspecting and consuming accessors are listed in [JSON
Module](/manual/json), which is the normative contract for number
classification, source positions, ordering, formatting, and resource limits.

The flat `dict[str, str]` helpers provide a typed data API alongside the dynamic
JSON tree. TOML uses the same typed top-level dictionary boundary:

| API | Signature |
| --- | --- |
| `json.is_valid` | `is_valid(text: str) -> bool` |
| `json.stringify_map` | `stringify_map(value: dict[str, str]) -> Result[str, str]` |
| `json.parse_string_map` | `parse_string_map(text: str) -> Result[dict[str, str], str]` |
| `toml.is_valid` | `is_valid(text: str) -> bool` |
| `toml.stringify_map` | `stringify_map(value: dict[str, str]) -> Result[str, str]` |
| `toml.parse_string_map` | `parse_string_map(text: str) -> Result[dict[str, str], str]` |

JSON compact output has sorted object keys. `json.is_valid` accepts any valid
JSON value, while `json.parse_string_map` succeeds only for an object whose
values are all strings. TOML output is a sorted top-level string dictionary;
`toml.is_valid` accepts any valid TOML document, while
`toml.parse_string_map` rejects nested tables and non-string values. Aura 0.3
has no derived class/enum schemas or generated codecs.

## Logs, Metrics, And Traces

`log.debug/info/warn/error(message, fields)` and `trace.event(name, fields)` emit one compact JSON record to standard error. `fields` is a `dict[str, str]`. Every record has the shape `{ "kind": "log" | "trace", "level": str, "message": str, "fields": Object }`; for trace events, `level` is `event` and `message` is the event name.

`metrics.increment(name, value)`, `metrics.get(name)`, and `metrics.reset()` provide process-global signed `int64` counters shared by Aura tasks in that process. A missing counter reads as zero. Incrementing past either `int64` bound is a runtime diagnostic and leaves the checked operation incomplete. These counters are useful for worker and test instrumentation; Aura 0.3 has no metrics exporter, export protocol, or scoped trace span API.

## Network Boundary

The HTTP client accepts `http://` and certificate-validated `https://` URLs using the platform-independent Web PKI root set. HTTP request and response bodies support content length, connection-close framing, and chunked transfer encoding. The 0.3 parser keeps a 16 MiB incoming wire-message limit, accepts at most 64 headers, and rejects conflicting framing headers. Its `dict[str, str]` header boundary cannot represent repeated equal header names losslessly.

For custom certificate authorities and TLS servers, use the lower-level `net.tls_connect*` and `net.tls_listen` APIs documented in [Network Module](/manual/network).

## Example

This self-contained validation example is safe to run without host files,
network access, or environment assumptions:

```python
import json

def main():
    print(json.is_valid("{\"ready\":\"yes\"}"))
```

See `examples/agents/control_plane_foundations.au` for path operations, JSON/TOML metadata, counters, and structured events.

## Retry

Import `control` for the eager retry helper:

| API | Signature |
| --- | --- |
| `control.retry` | `retry[T, E](worker: def() -> Result[T, E], max_attempts: int32 = 3, initial_backoff: Duration = 0ms) -> Result[T, E]` |

The first attempt runs immediately. Every `Result.Err` is retryable while an
attempt remains. `max_attempts` must be at least one and counts the immediate
attempt. `initial_backoff` must be non-negative and representable by the host
timer. Both arguments are validated before the worker is invoked. Before the
second attempt the current task waits for `initial_backoff`; each later retry
uses twice the preceding delay. A zero delay skips sleeping. Once the final permitted
attempt returns `Err`, that exact error is returned without another sleep or
delay multiplication.

The worker may be a capture-free function value or a repeatable
value-capturing closure. A consuming closure is rejected because retry may
invoke the worker more than once. Traps from the worker, delay overflow, and
invalid runtime operations are not converted to `E`. Current-task
cancellation propagates through the retry operation instead of returning the
most recent `Err`.

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

See `examples/agents/retry_with_backoff.au` for both eventual success and exact
last-error behavior.

## Grammar

These modules add no source-language grammar. They are imported and called with the ordinary import, call, member-access, named-argument, collection, `Result`, and `Option` forms defined elsewhere in this reference. Module and member names are case-sensitive. The `--` separator that supplies `sys.args()` belongs to the CLI protocol, not Aura syntax.

## Typing Rules

The function signatures in the tables above are normative. `sys.args()`
produces owned `str` values in a `list`; environment and path components that
may be absent use `Option`; fallible current-directory access uses
`Result[..., io.Error]`. Dynamic JSON parsing returns
`Result[json.Value, json.Error]`. Bounded JSON and TOML dictionary operations
retain their `Result[..., str]` contracts.

Telemetry fields are `dict[str, str]`. Metric names are `str`, increments and results are signed `int64`, and reset returns `None`. Passing any other type, using an unknown member, or binding an unsupported argument shape is rejected statically.

`control.retry` infers `T` and `E` from the exact shared callback type
`def() -> Result[T, E]`. A callback with parameters, a non-`Result` return
type, or a different function-value contract is rejected with `AU2002`.
`max_attempts` is exactly `int32`; `initial_backoff` is exactly `Duration`.

## Runtime Semantics

`sys.args()` returns program arguments without the executable name. `aura run` uses arguments after the CLI `--`; a built program uses its host argument list. Environment lookup returns `None` for a missing or non-Unicode value. Path operations use host path rules, and their string results use the lossy Unicode policy stated above.

Dynamic JSON object output and JSON/TOML top-level dictionary output are sorted
by key. Dynamic JSON parse and dump follow the recursive value, strict-number,
and formatting rules in the JSON chapter. Validation accepts the broader
source format, but each `parse_string_map` operation accepts only its
documented flat-string dictionary subset. Logging and trace calls synchronously emit one
compact JSON record to standard error. Metric operations address one
process-global, task-shared dictionary; a missing counter is zero, reset clears
the dictionary, and checked overflow leaves the attempted increment unapplied.

`control.retry` invokes its worker sequentially. It never overlaps attempts.
An immediate first attempt is followed only as needed by the current delay and
the next attempt. Every `Err` has the same retry policy. On exhaustion the
helper returns the final worker's exact `Err`; it performs no terminal sleep
and does not compute an unused next delay. A zero current delay skips the
scheduler sleep. A worker trap, backoff overflow, or current-task cancellation
propagates through the helper.

## Ownership And Evaluation Order

Call arguments are evaluated left to right. Inputs to these host helpers are shared for the duration of the call and are not retained as Aura values after it returns. Returned lists, dictionaries, strings, options, and results are fresh owned values. The metrics implementation copies the metric name into process-global host state; it does not keep an Aura borrow alive.

Telemetry emission and metric updates are observable side effects and occur at the call's position in source evaluation order. Concurrent tasks share standard error and the metric dictionary. Each individual metric operation is synchronized, but a sequence such as `get` followed by `increment` is not one atomic transaction.

The retry helper reads a capture-free function value or repeatable capturing
closure and invokes it under ordinary call rules.
The helper can therefore reuse one repeatable capturing closure across all
attempts without consuming its environment. Each `Result.Ok` or `Result.Err`
owns its payload.
Intermediate errors are consumed by the retry decision; the final error is
returned without cloning. Attempt calls and delay waits occur in the stated
sequence.

## Diagnostics

Unknown modules or members use `AU2001`; type and callback-contract mismatches
use `AU2002`; invalid argument binding uses `AU2004`; remaining static
rejections use `AU2999`. Incrementing a metric beyond either `int64` bound, or
doubling a retry delay beyond the exact `Duration` range, produces `AU4002` and
does not wrap. A retry attempt budget below one uses `AU4003`; a negative
or host-unrepresentable initial backoff uses `AU4001`.

Invalid JSON or TOML data is ordinary program data: validation returns
`false`, dynamic JSON parsing returns `Result.Err(json.Error)`, and bounded
flat-dictionary operations return `Result.Err(str)` as documented. JSON dumping
has the runtime traps and limits specified by the JSON chapter. A missing
environment variable returns `Option.None`, and current-directory failure
returns `Result.Err(io.Error)`; none of those typed outcomes is a language
diagnostic.

## Backend Support

All APIs on this page are implemented by both the MIR runtime used by `aura
run` and the direct native backend. Argument injection differs only at the host
boundary described above. Recursive JSON parse/dump behavior, JSON/TOML
ordering, time units, telemetry record shape, and checked metric arithmetic
are backend-parity contracts. Retry attempt order, backoff, exact final-error
return, trap propagation, and cancellation are also MIR/direct parity
contracts.

The HTTP client summarized here has the same MIR/direct support as the full [Network Module](/manual/network). Host-dependent path and environment results may differ with the host while preserving their Aura types and error policy.

## Limits And Implementation-Defined Behavior

Host argument and path bytes that are not Unicode are handled with the lossy or absent-value policies stated above. Path separators, roots, case sensitivity, and absolute-path rules follow the host. Unix time reflects the host clock and may move; monotonic time is process-local, millisecond-granularity elapsed time whose zero is the first call in that process.

Dynamic JSON has the fixed tree, numeric, depth, node-materialization, and byte
boundaries documented in [JSON Module](/manual/json); TOML and the bounded
flat-dictionary helpers retain their limits. There are no derived
codecs or streaming encoders.
Telemetry has no exporter, batching, delivery guarantee beyond the
standard-error write, scoped spans, or metric labels. Concurrent
standard-error records are individually emitted but ordering between tasks
follows scheduling. HTTP limits are the 16 MiB incoming wire-message cap,
64-header cap, framing checks, and repeated-header loss described above and in
[Current Limits](/manual/current-limits).

`control.retry` is an eager sequential helper, not a policy engine: it has no
error classifier, jitter, attempt callback, retry budget shared between calls,
or detached execution. Every `Err` is retryable. Applications that need
status-specific retry policy or jitter must express it explicitly. Its
`Duration` delays remain bounded by the host timer range described in
[Current Limits](/manual/current-limits).

## Status

The system, path, JSON, TOML, logging, trace-event, metrics, retry, and
summarized HTTP contracts on this page are implemented and maintained in
Aura 0.3. Recursive JSON gap-fill semantics are accepted under ADR-0021. The
summarized fixed HTTP cap is Accepted under ADR-0018.

Nested TOML data models, derived codecs, telemetry exporters, metric labels,
scoped tracing spans, and richer HTTP header representations are unavailable.
Mentions of those facilities are future, non-normative direction rather than
accepted language behavior.
