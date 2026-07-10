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

## Logs, Metrics, And Traces

`log.debug/info/warn/error(message, fields)` and `trace.event(name, fields)` emit one structured JSON record to standard error. `fields` is a `Map[String, String]`.

`metrics.increment(name, value)`, `metrics.get(name)`, and `metrics.reset()` provide process-local signed integer counters. They are useful for worker and test instrumentation; they are not a metrics exporter. Export protocols and scoped trace spans remain future work.

## Network Boundary

The HTTP client accepts `http://` and certificate-validated `https://` URLs using the platform-independent Web PKI root set. HTTP request and response bodies support content length, connection-close framing, and chunked transfer encoding. The 0.1 parser keeps a 1 MiB message limit and rejects conflicting framing headers.

For custom certificate authorities and TLS servers, use the lower-level `net.tls_connect*` and `net.tls_listen` APIs documented in [Network Module](/manual/network).

## Example

See `examples/agents/control_plane_foundations.au` for path operations, JSON/TOML metadata, counters, and structured events.
