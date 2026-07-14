# Control-Plane Modules

Aurora 0.1 includes a small, typed host/control-plane surface intended for service launchers, workers, evaluation harnesses, and agent infrastructure. These modules behave the same through `aura run` and direct native binaries.

## System And Path

Import `sys` for process arguments, environment access, the current directory, and clocks:

| API | Signature |
| --- | --- |
| `sys.args` | `args() -> Vec[String]` |
| `sys.env` | `env(name: String) -> Option[String]` |
| `sys.current_dir` | `current_dir() -> Result[String, io.Error]` |
| `sys.unix_time_ms` | `unix_time_ms() -> int64` |
| `sys.monotonic_time_ms` | `monotonic_time_ms() -> int64` |

Pass program arguments after a separator:

```bash
aura run worker.au -- --model small --port 8080
./worker --model small --port 8080
```

`sys.args()` excludes the executable name. In `aura run`, arguments after `--` are passed explicitly into the MIR execution context and inherited by child tasks. A built program reads its real host command line; ambient environment variables cannot override it. On hosts that permit non-Unicode argv bytes, built programs replace invalid byte sequences with the Unicode replacement character.

`sys.env` returns `None` both when a variable is missing and when its host value is not valid Unicode. `sys.current_dir` and the string-producing `path` helpers convert non-Unicode host paths lossily. `unix_time_ms` is milliseconds since the Unix epoch; `monotonic_time_ms` is milliseconds since the first call to that function in the current process and is suitable for elapsed-time comparisons, not wall-clock timestamps.

`path` provides host-aware `join`, `parent`, `file_name`, `extension`, and `is_absolute` operations. Components that may not exist return `Option[String]`.

## JSON And TOML

The first serialization boundary is deliberately typed as `Map[String, String]`:

| API | Signature |
| --- | --- |
| `json.is_valid` | `is_valid(text: String) -> bool` |
| `json.stringify_map` | `stringify_map(value: Map[String, String]) -> Result[String, String]` |
| `json.parse_string_map` | `parse_string_map(text: String) -> Result[Map[String, String], String]` |
| `toml.is_valid` | `is_valid(text: String) -> bool` |
| `toml.stringify_map` | `stringify_map(value: Map[String, String]) -> Result[String, String]` |
| `toml.parse_string_map` | `parse_string_map(text: String) -> Result[Map[String, String], String]` |

This is not a dynamic JSON tree or class/enum derivation system. Nested schemas and generated codecs remain post-0.1 work; the current API gives control-plane code a checked configuration and metadata boundary without adding an untyped universal value.

JSON output is compact and object keys are sorted. `json.is_valid` accepts any valid JSON value, while `json.parse_string_map` succeeds only for an object whose values are all strings. TOML output is a sorted top-level string map; `toml.is_valid` accepts any valid TOML document, while `toml.parse_string_map` rejects nested tables and non-string values.

## Logs, Metrics, And Traces

`log.debug/info/warn/error(message, fields)` and `trace.event(name, fields)` emit one compact JSON record to standard error. `fields` is a `Map[String, String]`. Every record has the shape `{ "kind": "log" | "trace", "level": String, "message": String, "fields": Object }`; for trace events, `level` is `event` and `message` is the event name.

`metrics.increment(name, value)`, `metrics.get(name)`, and `metrics.reset()` provide process-global signed `int64` counters shared by Aurora tasks in that process. A missing counter reads as zero. Incrementing past either `int64` bound is a runtime diagnostic and leaves the checked operation incomplete. These counters are useful for worker and test instrumentation; they are not a metrics exporter. Export protocols and scoped trace spans remain future work.

## Network Boundary

The HTTP client accepts `http://` and certificate-validated `https://` URLs using the platform-independent Web PKI root set. HTTP request and response bodies support content length, connection-close framing, and chunked transfer encoding. The 0.1 parser keeps a 1 MiB message limit, accepts at most 64 headers, and rejects conflicting framing headers. Its `Map[String, String]` header boundary cannot represent repeated equal header names losslessly.

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

## Grammar

These modules add no source-language grammar. They are imported and called with the ordinary import, call, member-access, named-argument, collection, `Result`, and `Option` forms defined elsewhere in this reference. Module and member names are case-sensitive. The `--` separator that supplies `sys.args()` belongs to the CLI protocol, not Aurora syntax.

## Typing Rules

The function signatures in the tables above are normative. `sys.args()` produces owned `String` values in a `Vec`; environment and path components that may be absent use `Option`; fallible current-directory access uses `Result[..., io.Error]`. JSON and TOML parsing is intentionally restricted to `Map[String, String]`, and serialization failures use `Result[..., String]` rather than an untyped dynamic value.

Telemetry fields are `Map[String, String]`. Metric names are `String`, increments and results are signed `int64`, and reset returns `None`. Passing any other type, using an unknown member, or binding an unsupported argument shape is rejected statically.

## Runtime Semantics

`sys.args()` returns program arguments without the executable name. `aura run` uses arguments after the CLI `--`; a built program uses its host argument vector. Environment lookup returns `None` for a missing or non-Unicode value. Path operations use host path rules, and their string results use the lossy Unicode policy stated above.

JSON object output and TOML top-level-map output are compact and sorted by key. Validation accepts the broader source format, but each `parse_string_map` operation accepts only its documented string-map subset. Logging and trace calls synchronously emit one compact JSON record to standard error. Metric operations address one process-global, task-shared map; a missing counter is zero, reset clears the map, and checked overflow leaves the attempted increment unapplied.

## Ownership And Evaluation Order

Call arguments are evaluated left to right. Inputs to these host helpers are shared for the duration of the call and are not retained as Aurora values after it returns. Returned vectors, maps, strings, options, and results are fresh owned values. The metrics implementation copies the metric name into process-global host state; it does not keep an Aurora borrow alive.

Telemetry emission and metric updates are observable side effects and occur at the call's position in source evaluation order. Concurrent tasks share standard error and the metric map. Each individual metric operation is synchronized, but a sequence such as `get` followed by `increment` is not one atomic transaction.

## Diagnostics

Unknown modules or members use `AU2001`; type mismatches use `AU2002`; invalid argument binding uses `AU2004`; remaining static rejections use `AU2999`. Incrementing a metric beyond either `int64` bound produces `AU4002` and does not wrap.

Invalid JSON or TOML data is ordinary program data: validation returns `false`, and parsing or serialization returns `Result.Err(String)` as documented. A missing environment variable returns `Option.None`, and current-directory failure returns `Result.Err(io.Error)`; none of those typed outcomes is a language diagnostic.

## Backend Support

All APIs on this page are implemented by both the MIR runtime used by `aura run` and the direct native backend. Argument injection differs only at the host boundary described above. JSON/TOML ordering, time units, telemetry record shape, and checked metric arithmetic are backend-parity contracts.

The HTTP client summarized here has the same MIR/direct support as the full [Network Module](/manual/network). Host-dependent path and environment results may differ with the host while preserving their Aurora types and error policy.

## Limits And Implementation-Defined Behavior

Host argument and path bytes that are not Unicode are handled with the lossy or absent-value policies stated above. Path separators, roots, case sensitivity, and absolute-path rules follow the host. Unix time reflects the host clock and may move; monotonic time is process-local, millisecond-granularity elapsed time whose zero is the first call in that process.

Serialization supports only the documented string-map boundary. Telemetry has no exporter, batching, delivery guarantee beyond the standard-error write, scoped spans, or metric labels. Concurrent standard-error records are individually emitted but ordering between tasks follows scheduling. HTTP limits are the 1 MiB message cap, 64-header cap, framing checks, and repeated-header loss described above and in [Current Limits](/manual/current-limits).

## Status

The system, path, JSON, TOML, logging, trace-event, metrics, and summarized HTTP contracts on this page are implemented and maintained in Aurora 0.1. No semantics on this page are provisional.

Dynamic JSON trees, nested TOML data models, derived codecs, telemetry exporters, metric labels, scoped tracing spans, and richer HTTP header representations are unavailable. Mentions of those facilities are future, non-normative direction rather than accepted language behavior.
