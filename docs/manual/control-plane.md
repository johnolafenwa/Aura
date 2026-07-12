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

`sys.args()` excludes the executable name. In `aura run`, arguments after `--` become this vector; a built program uses its host command-line arguments. The current runtime transports arguments through an internal environment value, so an ambient `AURORA_PROGRAM_ARGS_JSON` value can spoof arguments in a directly launched built program; treat that name as reserved until this defect is removed.

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

See `examples/agents/control_plane_foundations.au` for path operations, JSON/TOML metadata, counters, and structured events.
