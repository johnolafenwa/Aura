# API Index

This page indexes every maintained public builtin function, method, module type, and builtin enum documented by the manual. It is intentionally dense. Use the linked manual pages for examples and longer discussion.

`assert condition` and `assert condition, message` are statements rather than
callable APIs. Their exact typing, lazy-message, `AU4001`, cleanup, and
backend-parity contract is indexed separately in [Assertions](/manual/assertions).

## Top-Level Builtins

| API | Signature | Contract |
| --- | --- | --- |
| `print` | `print(value) -> None` | Renders `value` and writes a newline. |
| `range` | `range(stop: int32) -> Range`; `range(start: int32, stop: int32) -> Range` | End-exclusive integer range. |
| `cancelled` | `cancelled() -> bool` | Returns the current task cancellation state. |
| `yield_now` | `yield_now() -> None` | Voluntarily yields the current lightweight task to the scheduler. |
| `sleep` | `sleep(duration: Duration) -> None` | Suspends the current task using the scheduler. |
| `select` | `select(source, ...) -> SelectOutcome[Q, T]` | Waits on one or more positional Queue, Task, or relative-Duration sources; cancellation wins, otherwise the lowest ready source index wins. |
| `wait_any` | `wait_any(tasks: Vec[Task[T]], timeout: Duration = ...) -> WaitAny[T]` | Waits for the first task outcome; consumes the vector and abandons unchosen rights when `T` is non-repeatable. `wait_any([])` returns `TimedOut` immediately. |
| `wait_all` | `wait_all(tasks: Vec[Task[T]], timeout: Duration = ...) -> WaitAll[T]` | Waits for all tasks, the first task error, timeout, or cancellation; consumes the vector when `T` is non-repeatable. |
| `abs` | `abs(value: number) -> number` | Absolute value for integers and floats. |
| `min` | `min(left: number, right: number) -> number` | Smaller value of the same numeric type. |
| `max` | `max(left: number, right: number) -> number` | Larger value of the same numeric type. |
| `sqrt` | `sqrt(value: float32|float64) -> float32|float64` | Square root. |
| `parse_int32` | `parse_int32(text: String) -> Result[int32, String]` | Parses a signed 32-bit integer. |
| `parse_int64` | `parse_int64(text: String) -> Result[int64, String]` | Parses a signed 64-bit integer. |
| `parse_float64` | `parse_float64(text: String) -> Result[float64, String]` | Parses a 64-bit float. |
| `len` | `len(value: String\|Vec[T]\|Map[K, V]\|Set[T]) -> int64` | Delegates to the value's own `len()` member with the same `int64` type and value. |
| `str` | `str(value) -> String` | Renders `value` exactly as `print` and f-string interpolation render it. |

## Foreign Declarations

FFI declarations are package-authorized direct calls, not builtins or
first-class function values. Their names and exact signatures come from the
binding package.

| Surface | Signature | Contract |
| --- | --- | --- |
| C function | `extern "C" def name(...) -> R` | Bodyless synchronous call to the same process-global symbol name. |
| Opaque handle | `extern "C" opaque class Handle` | Non-null, non-Copy, non-cloneable, non-Transfer foreign pointer wrapper. |
| String view | `text: String` | Temporary const UTF-8 pointer plus byte length; empty is `(NULL, 0)` and no NUL terminator is promised. |
| Byte view | `bytes: Vec[uint8]` | Temporary const pointer plus byte length; empty is `(NULL, 0)`. |
| Mutable byte view | `bytes: mut Vec[uint8]` | Same-length scratch copy-in/out; writeback occurs after native return before result validation. |
| Consuming handle | `handle: own Handle` | Moves the opaque handle into a foreign close/free-style call. |

The complete scalar table, manifest report, safety boundary, diagnostics, and
backend rules are in [FFI v0](/manual/ffi).

## Scalars And String

| API | Signature | Contract |
| --- | --- | --- |
| `float64.sqrt` | `sqrt() -> float64` | Square root of the receiver. |
| integer `.to_float` | `to_float() -> float64` | Converts any integer type with IEEE-754 round-to-nearest, ties-to-even; may round. |
| scalar `.to_string` | `to_string() -> String` | Supported on `bool`, integer types, `float32`, and `float64`. |
| `Duration.ms` | `Duration.ms(value: int64) -> Duration` | Exact signed millisecond constructor. |
| `Duration.seconds` | `Duration.seconds(value: int64) -> Duration` | Exact signed second constructor. |
| `Duration.minutes` | `Duration.minutes(value: int64) -> Duration` | Exact signed minute constructor. |
| `Duration.to_ms` | `to_ms() -> float64` | Converts exact nanoseconds to nearest-representable binary64 milliseconds, ties-to-even; may round; accepted under ADR-0019. |
| `Duration.to_seconds` | `to_seconds() -> float64` | Converts exact nanoseconds to nearest-representable binary64 seconds, ties-to-even; may round; accepted under ADR-0019. |
| `String.len` | `len() -> int64` | Counts Unicode scalar values in O(n). |
| `String.byte_len` | `byte_len() -> int64` | Returns the UTF-8 byte count in O(1). |
| `String.to_bytes` | `to_bytes() -> Vec[uint8]` | Returns a fresh vector containing the receiver's exact UTF-8 bytes. |
| `String.from_bytes` | `from_bytes(bytes: Vec[uint8]) -> Result[String, bytes.Error]` | Strictly validates UTF-8 and returns a fresh String or the first invalid byte offset. |
| `String.contains` | `contains(text: String) -> bool` | `true` when the receiver contains `text`. |
| `String.starts_with` | `starts_with(text: String) -> bool` | Prefix test. |
| `String.ends_with` | `ends_with(text: String) -> bool` | Suffix test. |
| `String.split` | `split(text: String) -> Vec[String]` | Splits on each occurrence of `text`. |
| `String.replace` | `replace(from: String, to: String) -> String` | Returns a new string with replacements applied. |
| `String.to_lower` | `to_lower() -> String` | Unicode lowercase conversion. |
| `String.to_upper` | `to_upper() -> String` | Unicode uppercase conversion. |
| `String.strip_prefix` | `strip_prefix(text: String) -> Option[String]` | Returns the remainder when the prefix matches. |
| `String.strip_suffix` | `strip_suffix(text: String) -> Option[String]` | Returns the remainder when the suffix matches. |
| `String.trim` | `trim() -> String` | Removes surrounding Unicode whitespace. |
| `String.join` | `join(parts: Vec[String]) -> String` | Joins `parts` using the receiver as separator. |
| `String.clone` | `clone() -> String` | Returns a new owned string. |

Duration operators are `Duration + Duration`, `Duration - Duration`,
`Duration * int64`, `int64 * Duration`, and `Duration // int64`, all returning
`Duration`, plus equality and all four ordering comparisons between Duration
values. Arithmetic is checked on signed i128 nanoseconds.

## Randomness

See [Randomness Module](/manual/randomness) for the normative xoshiro256**
algorithm, seed-42 vectors, ownership, secure-source boundary, and diagnostics.

| API | Signature | Contract |
| --- | --- | --- |
| `random.Rng` | `Rng(seed: int64) -> random.Rng` | Creates a move-only deterministic stream from the seed's exact two's-complement bit pattern. |
| `random.Rng.next_int` | `next_int(lo: int64, hi: int64) -> int64` | Uniform half-open `[lo, hi)` integer; mutable receiver. |
| `random.Rng.next_float` | `next_float() -> float64` | Uniform 53-bit binary64 value in `[0.0, 1.0)`; mutable receiver. |
| `random.Rng.shuffle` | `shuffle[T](values: mut Vec[T]) -> None` | Descending Fisher-Yates shuffle in place; mutable receiver and vector. |
| `random.secure_int` | `secure_int(lo: int64, hi: int64) -> int64` | OS-secure uniform half-open integer with no deterministic fallback. |
| `random.secure_bytes` | `secure_bytes(n: int64) -> Vec[uint8]` | Exactly `n` OS-secure bytes for `0 <= n <= 2147483647`; zero skips entropy and larger counts trap with `AU4005` before allocation. |

`random.Rng` has no public clone route. `AU3007` rejects the clone-producing
collection and task APIs indexed below when their produced value contains, or
may contain, an `Rng`, including through a user-defined wrapper. Cloning
an allowed Task or Queue handle copies only the handle. Accepted ADR-0033
nevertheless rejects `random.Rng` as a task result or Queue payload with
`AU3008`, and makes a Task with a non-repeatable result non-copyable. Moves,
collection removals, and in-place shuffle within one owning task transfer or
rearrange values without duplicating generator state.
Generic clone-producing calls infer clone-safety obligations and discharge them
after specialization; the obligation is retained through generic callers and
module imports.

## Bytes, Text Codecs, And SHA-256

See [Bytes, Text Codecs, And SHA-256](/manual/bytes) for exact UTF-8
preservation, strict malformed-input policy, error offsets, ownership, output
size preflights, and the cryptographic scope of SHA-256.

| API | Signature | Contract |
| --- | --- | --- |
| `String.to_bytes` | `to_bytes() -> Vec[uint8]` | Exact UTF-8 bytes; shared receiver and fresh result. |
| `String.from_bytes` | `from_bytes(bytes: Vec[uint8]) -> Result[String, bytes.Error]` | Strict UTF-8; no replacement decoding. |
| `bytes.hex_encode` | `hex_encode(value: Vec[uint8]) -> String` | Two lowercase ASCII digits per byte. |
| `bytes.hex_decode` | `hex_decode(text: String) -> Result[Vec[uint8], bytes.Error]` | Accepts mixed-case ASCII hex; rejects prefixes, separators, and whitespace. |
| `bytes.base64_encode` | `base64_encode(value: Vec[uint8]) -> String` | RFC 4648 standard alphabet with canonical padding. |
| `bytes.base64_decode` | `base64_decode(text: String) -> Result[Vec[uint8], bytes.Error]` | Strict canonical standard-alphabet decode. |
| `bytes.sha256` | `sha256(value: Vec[uint8]) -> Vec[uint8]` | Fresh raw 32-byte FIPS 180-4 digest. |
| `bytes.sha256_string` | `sha256_string(text: String) -> Vec[uint8]` | SHA-256 over the text's exact UTF-8 bytes. |

All displayed inputs use shared access and remain reusable. An `encoding`
argument is reserved but not implemented. Expanded output that cannot be
represented or allocated traps with `AU4005`; malformed data returns
`bytes.Error`.

## Collections

See [Collections](/manual/collections) for ownership and iteration details.

### Vec[T]

| API | Signature | Contract |
| --- | --- | --- |
| `Vec[T]()` | `Vec[T]()` | Empty vector constructor. |
| `Vec.len` | `len() -> int64` | Element count. |
| `Vec.is_empty` | `is_empty() -> bool` | `true` when empty. |
| `Vec.clone` | `clone() -> Vec[T]` | Clones the vector and elements; requires clone-safe `T`. |
| `Vec.push` | `push(value: own T) -> None` | Appends `value`. |
| `Vec.pop` | `pop() -> Option[T]` | Removes the last element or returns `None`. |
| `Vec.get` | `get(index: int32) -> Option[T]` | Cloned element after negative-index normalization, or `None` when out of bounds; requires clone-safe `T`. |
| `Vec.set` | `set(index: int32, value: own T) -> Option[T]` | Replaces and returns the old element after negative-index normalization; out of bounds is a runtime error. |
| `Vec.remove` | `remove(index: int32) -> Option[T]` | Removes an element after negative-index normalization; out of bounds is a runtime error. |
| `Vec.swap` | `swap(first: int32, second: int32) -> bool` | Normalizes both indexes, swaps the elements, and returns `true`; out of bounds is a runtime error. |
| `Vec.contains` | `contains(value: T) -> bool` | Equality lookup. |
| `Vec.extend` | `extend(other: own Vec[T]) -> None` | Moves elements from `other` into the receiver. |
| `Vec.insert` | `insert(index: int32, value: own T) -> bool` | Normalizes a negative index, inserts before it, and returns `true`; valid range is `0..=len`, without clamping. |
| `Vec.clear` | `clear() -> None` | Removes all elements. |
| `Vec.reverse` | `reverse() -> None` | Reverses in place. |
| `Vec.sort` | `sort() -> None` | Stable in-place natural ordering; mutable receiver and orderable `T`. |
| `Vec.sort_by` | `sort_by[K](key: def(T) -> K) -> None` | Stable in-place key ordering; evaluates the shared key callback once per element left-to-right before mutation and requires orderable `K`. |
| `Vec.map` | `map[U](f: def(T) -> U) -> Vec[U]` | Eager shared traversal into a fresh owned result; retains the source. |
| `Vec.filter` | `filter(f: def(T) -> bool) -> Vec[T]` | Eager shared traversal into a fresh owned result; retains the source and requires clone-safe `T`. |

### Map[K, V]

| API | Signature | Contract |
| --- | --- | --- |
| `Map[K, V]()` | `Map[K, V]()` | Empty map constructor. |
| `Map.len` | `len() -> int64` | Entry count. |
| `Map.is_empty` | `is_empty() -> bool` | `true` when empty. |
| `Map.clone` | `clone() -> Map[K, V]` | Clones keys and values; requires clone-safe `K` and `V`. |
| `Map.get` | `get(key: K) -> Option[V]` | Cloned value or `None` when absent; requires clone-safe `V`. |
| `Map.set` | `set(key: own K, value: own V) -> Option[V]` | Inserts or replaces, returning the previous value. |
| `Map.remove` | `remove(key: K) -> Option[V]` | Removes an entry and returns the previous value. |
| `Map.contains_key` | `contains_key(key: K) -> bool` | Key lookup. |
| `Map.keys` | `keys() -> Vec[K]` | Cloned keys in insertion order; requires clone-safe `K`. |
| `Map.values` | `values() -> Vec[V]` | Cloned values in insertion order; requires clone-safe `V`. |
| `Map.items` | `items() -> Vec[MapEntry[K, V]]` | Cloned entries in insertion order; requires clone-safe `K` and `V`. |
| `Map.entries` | `entries() -> Vec[MapEntry[K, V]]` | Same clone-safety and ordering contract as `items()`. |
| `Map.clear` | `clear() -> None` | Removes all entries. |
| `Map.extend` | `extend(other: own Map[K, V]) -> None` | Moves entries from `other`; matching keys are replaced. |
| `MapEntry.key` | field `key: K` | Entry key. |
| `MapEntry.value` | field `value: V` | Entry value. |

### Set[T]

| API | Signature | Contract |
| --- | --- | --- |
| `Set[T]()` | `Set[T]()` | Empty set constructor. |
| `Set.len` | `len() -> int64` | Unique value count. |
| `Set.is_empty` | `is_empty() -> bool` | `true` when empty. |
| `Set.clone` | `clone() -> Set[T]` | Clones the set; requires clone-safe `T`. |
| `Set.contains` | `contains(value: T) -> bool` | Membership lookup. |
| `Set.insert` | `insert(value: own T) -> bool` | `true` only when newly inserted. |
| `Set.remove` | `remove(value: T) -> bool` | `true` only when a value was removed. |

## Concurrency

See [Concurrency](/manual/concurrency) for structured-concurrency semantics.

| API | Signature | Contract |
| --- | --- | --- |
| `Queue[T]()` | `Queue[T](capacity: int32 = ...)` | Queue constructor; bounded when capacity is supplied; Accepted ADR-0033 requires `T: Transfer`. |
| `Queue.put` | `put(value: own T, timeout: Duration = ...) -> Result[None, SendError[T]]` | Sends a value or returns the unsent value in the error; Accepted ADR-0033 requires `T: Transfer`. |
| `Queue.try_put` | `try_put(value: own T) -> Result[None, SendError[T]]` | Sends without waiting; Accepted ADR-0033 requires `T: Transfer`. |
| `Queue.get` | `get(timeout: Duration = ...) -> QueueReceive[T]` | Receives an item, close, timeout, or cancellation outcome; does not itself recheck payload Transfer. |
| `Queue.get_or_none` | `get_or_none(timeout: Duration = ...) -> Option[T]` | `Some(value)` or `None` for closed, timeout, cancellation, or immediate absence. |
| `Queue.get_or` | `get_or(default: own T, timeout: Duration = ...) -> T` | Value or fallback. |
| `Queue.close` | `close() -> None` | Closes the queue and wakes waiters. |
| `Task.result` | `result(timeout: Duration = ...) -> TaskResult[T]` | Waits for task outcome; consumes the observation right when `T` is non-repeatable. |
| `Task.result_or_none` | `result_or_none(timeout: Duration = ...) -> Option[T]` | `Some(value)` or `None` for failure, timeout, cancellation, or immediate absence; consumes the observation right when `T` is non-repeatable, including on `None`. |
| `Task.result_or` | `result_or(default: own T, timeout: Duration = ...) -> T` | Value or fallback; consumes the observation right when `T` is non-repeatable. |
| `TaskGroup()` | `TaskGroup()` | Task group resource constructor. |
| `TaskGroup.start` | `start(function, own ...) -> Task[T]` | Requires every capture and result to be `Transfer`; accepts inferred or explicit `function[Types]` / `Type.associated_method[Types]` targets; starts the child on the guarded 512 KiB default stack. |
| `TaskGroup.start_soon` | `start_soon(function, own ...) -> None` | Applies the same Transfer and target-specialization rules without returning a handle. |
| `TaskGroup.start_with_stack` | `start_with_stack(bytes: int64, function, own ...) -> Task[T]` | Applies the same Transfer and target-specialization rules with an explicit guarded 256 KiB..64 MiB request; 256 KiB is for measured shallow tasks, not the default; Provisional under ADR-0032. |
| `TaskGroup.start_soon_with_stack` | `start_soon_with_stack(bytes: int64, function, own ...) -> None` | Applies the same rules and explicit guarded range without retaining a handle; 256 KiB is for measured shallow tasks, not the default; Provisional under ADR-0032. |
| `TaskGroup.cancel` | `cancel() -> None` | Signals cancellation to children. |

## I/O And Filesystem

See [I/O Module](/manual/io) and [Filesystem Module](/manual/filesystem).

| API | Signature | Contract |
| --- | --- | --- |
| `io.write` | `write(text: String) -> Result[None, io.Error]` | Writes text without a newline. |
| `io.flush` | `flush() -> Result[None, io.Error]` | Flushes standard output. |
| `io.read_line` | `read_line() -> Result[Option[String], io.Error]` | Reads strict UTF-8 without trailing LF/CRLF; `Ok(None)` on EOF. |
| `fs.exists` | `exists(path: String) -> bool` | Path existence check. |
| `fs.read_to_string` | `read_to_string(path: String) -> Result[String, io.Error]` | Reads UTF-8 text, capped at 256 MiB. |
| `fs.read_bytes` | `read_bytes(path: String) -> Result[Vec[uint8], io.Error]` | Reads bytes, capped at 256 MiB. |
| `fs.write_string` | `write_string(path: String, text: String) -> Result[None, io.Error]` | Creates or replaces a text file. |
| `fs.write_bytes` | `write_bytes(path: String, bytes: Vec[uint8]) -> Result[None, io.Error]` | Creates or replaces a byte file. |
| `fs.append_string` | `append_string(path: String, text: String) -> Result[None, io.Error]` | Appends text. |
| `fs.append_bytes` | `append_bytes(path: String, bytes: Vec[uint8]) -> Result[None, io.Error]` | Appends bytes. |
| `fs.create_dir` | `create_dir(path: String) -> Result[None, io.Error]` | Creates one directory. |
| `fs.read_dir` | `read_dir(path: String) -> Result[Vec[String], io.Error]` | Returns sorted immediate entry names, with lossy host-path decoding. |
| `fs.remove_file` | `remove_file(path: String) -> Result[None, io.Error]` | Removes a file. |
| `fs.open` | `open(path: String) -> Result[fs.File, io.Error]` | Opens for reading. |
| `fs.create` | `create(path: String) -> Result[fs.File, io.Error]` | Creates or truncates for writing. |
| `fs.append` | `append(path: String) -> Result[fs.File, io.Error]` | Opens for append, creating if needed. |
| `fs.File.read_all` | `read_all() -> Result[String, io.Error]` | Reads remaining strict UTF-8 text, capped at 256 MiB. |
| `fs.File.read_bytes` | `read_bytes() -> Result[Vec[uint8], io.Error]` | Reads remaining bytes, capped at 256 MiB. |
| `fs.File.write_all` | `write_all(text: String) -> Result[None, io.Error]` | Writes all text. |
| `fs.File.write_bytes` | `write_bytes(bytes: Vec[uint8]) -> Result[None, io.Error]` | Writes all bytes. |
| `fs.File.flush` | `flush() -> Result[None, io.Error]` | Flushes pending writes. |
| `fs.File.close` | `close() -> None` | Closes the handle. |

## Control-Plane Modules

See [Control-Plane Modules](/manual/control-plane).

| API | Signature |
| --- | --- |
| `sys.args` | `args() -> Vec[String]` |
| `sys.env` | `env(name: String) -> Option[String]` |
| `sys.current_dir` | `current_dir() -> Result[String, io.Error]` |
| `sys.unix_time_ms` | `unix_time_ms() -> int64` |
| `sys.monotonic_time_ms` | `monotonic_time_ms() -> int64` |
| `path.join` | `join(base: String, child: String) -> String` |
| `path.parent` | `parent(path: String) -> Option[String]` |
| `path.file_name` | `file_name(path: String) -> Option[String]` |
| `path.extension` | `extension(path: String) -> Option[String]` |
| `path.is_absolute` | `is_absolute(path: String) -> bool` |
| `json.parse` | `parse(text: String) -> Result[json.Value, json.Error]` |
| `json.dumps` | `dumps(value: json.Value, indent: Option[int64] = None) -> String` |
| `json.is_null` | `is_null(value: json.Value) -> bool` |
| `json.as_bool` | `as_bool(value: json.Value) -> Option[bool]` |
| `json.as_int` | `as_int(value: json.Value) -> Option[int64]` |
| `json.as_float` | `as_float(value: json.Value) -> Option[float64]` |
| `json.into_string` | `into_string(value: own json.Value) -> Option[String]` |
| `json.into_array` | `into_array(value: own json.Value) -> Option[Vec[json.Value]]` |
| `json.into_object` | `into_object(value: own json.Value) -> Option[Map[String, json.Value]]` |
| `json.is_valid` / `toml.is_valid` | `is_valid(text: String) -> bool` |
| `json.stringify_map` / `toml.stringify_map` | `stringify_map(value: Map[String, String]) -> Result[String, String]` |
| `json.parse_string_map` / `toml.parse_string_map` | `parse_string_map(text: String) -> Result[Map[String, String], String]` |
| `log.debug/info/warn/error` | `(message: String, fields: Map[String, String]) -> None` |
| `trace.event` | `(name: String, fields: Map[String, String]) -> None` |
| `metrics.increment` | `(name: String, value: int64) -> None` |
| `metrics.get` | `(name: String) -> int64` |
| `metrics.reset` | `() -> None` |
| `control.retry` | `retry[T, E](worker: def() -> Result[T, E], max_attempts: int32 = 3, initial_backoff: Duration = 0ms) -> Result[T, E]` |

Metrics are process-global `int64` counters; missing names read as zero and
overflow is a runtime diagnostic. Dynamic JSON object dumps and legacy
JSON/TOML string maps serialize in sorted key order. See [JSON
Module](/manual/json) and the control-plane chapter for exact value, limit,
host-string/path, and telemetry-record rules.

`control.retry` validates at least one attempt and a non-negative,
host-representable backoff before invoking the worker. It runs its first
attempt immediately, retries every `Err` with doubling delays, skips zero
sleeps, returns the exact last `Err`, and performs no sleep or multiplication
after the final attempt. Worker traps, backoff overflow, and current-task
cancellation propagate.

## Network Constructors And HTTP Client Helpers

See [Network Module](/manual/network) for behavior and examples.

| API | Signature |
| --- | --- |
| `net.connect` | `connect(address: String) -> Result[net.TcpStream, io.Error]` |
| `net.connect_timeout` | `connect_timeout(address: String, timeout: Duration) -> Result[net.TcpStream, io.Error]` |
| `net.listen` | `listen(address: String) -> Result[net.TcpListener, io.Error]` |
| `net.udp_bind` | `udp_bind(address: String) -> Result[net.UdpSocket, io.Error]` |
| `net.http_listen` | `http_listen(address: String) -> Result[net.HttpListener, io.Error]` |
| `net.websocket_listen` | `websocket_listen(address: String) -> Result[net.WebSocketListener, io.Error]` |
| `net.websocket_connect` | `websocket_connect(url: String) -> Result[net.WebSocket, io.Error]` |
| `net.websocket_connect_timeout` | `websocket_connect_timeout(url: String, timeout: Duration) -> Result[net.WebSocket, io.Error]` |
| `net.unix_listen` | `unix_listen(path: String) -> Result[net.UnixListener, io.Error]` |
| `net.unix_connect` | `unix_connect(path: String) -> Result[net.UnixStream, io.Error]` |
| `net.unix_connect_timeout` | `unix_connect_timeout(path: String, timeout: Duration) -> Result[net.UnixStream, io.Error]` |
| `net.tls_listen` | `tls_listen(address: String, cert_pem_path: String, key_pem_path: String) -> Result[net.TlsListener, io.Error]` |
| `net.tls_connect` | `tls_connect(address: String, server_name: String, ca_pem_path: String) -> Result[net.TlsStream, io.Error]` |
| `net.tls_connect_timeout` | `tls_connect_timeout(address: String, server_name: String, ca_pem_path: String, timeout: Duration) -> Result[net.TlsStream, io.Error]` |
| `net.http_request_text` | `http_request_text(method: String, url: String, body: String, headers: Map[String, String]) -> Result[net.HttpResponse, io.Error]` |
| `net.http_request_text_timeout` | `http_request_text_timeout(method: String, url: String, body: String, headers: Map[String, String], timeout: Duration) -> Result[net.HttpResponse, io.Error]` |
| `net.http_request_bytes` | `http_request_bytes(method: String, url: String, bytes: Vec[uint8], headers: Map[String, String]) -> Result[net.HttpResponse, io.Error]` |
| `net.http_request_bytes_timeout` | `http_request_bytes_timeout(method: String, url: String, bytes: Vec[uint8], headers: Map[String, String], timeout: Duration) -> Result[net.HttpResponse, io.Error]` |

## Network Resource Methods

Bounded stream read counts are `1..=67108864`; UDP receive counts are `1..=65535`. Incoming HTTP parsing accepts at most 64 headers and 16 MiB of wire data per message; WebSocket limits are 64 MiB per message and 16 MiB per frame/write buffer. See [Network Module](/manual/network) for timeout, EOF, UTF-8, cancellation, and repeated-header contracts.

| Type | API | Signature |
| --- | --- | --- |
| `net.TcpListener` | `accept` | `accept(timeout: Duration = ...) -> Result[net.TcpStream, io.Error]` |
| `net.TcpListener` | `local_addr` | `local_addr() -> Result[String, io.Error]` |
| `net.TcpListener` | `close` | `close() -> None` |
| `net.TcpStream` | `read_all` | `read_all(timeout: Duration = ...) -> Result[String, io.Error]` |
| `net.TcpStream` | `read_line` | `read_line(timeout: Duration = ...) -> Result[Option[String], io.Error]` |
| `net.TcpStream` | `read_bytes` | `read_bytes(max_bytes: int32, timeout: Duration = ...) -> Result[Option[Vec[uint8]], io.Error]` |
| `net.TcpStream` | `read_exact` | `read_exact(count: int32, timeout: Duration = ...) -> Result[Vec[uint8], io.Error]` |
| `net.TcpStream` | `write_all` | `write_all(text: String, timeout: Duration = ...) -> Result[None, io.Error]` |
| `net.TcpStream` | `write_bytes` | `write_bytes(bytes: Vec[uint8], timeout: Duration = ...) -> Result[None, io.Error]` |
| `net.TcpStream` | `flush` | `flush() -> Result[None, io.Error]` |
| `net.TcpStream` | `local_addr` | `local_addr() -> Result[String, io.Error]` |
| `net.TcpStream` | `peer_addr` | `peer_addr() -> Result[String, io.Error]` |
| `net.TcpStream` | `shutdown_read` | `shutdown_read() -> Result[None, io.Error]` |
| `net.TcpStream` | `shutdown_write` | `shutdown_write() -> Result[None, io.Error]` |
| `net.TcpStream` | `shutdown_both` | `shutdown_both() -> Result[None, io.Error]` |
| `net.TcpStream` | `close` | `close() -> None` |
| `net.UdpSocket` | `send_text` | `send_text(address: String, text: String, timeout: Duration = ...) -> Result[None, io.Error]` |
| `net.UdpSocket` | `send_bytes` | `send_bytes(address: String, bytes: Vec[uint8], timeout: Duration = ...) -> Result[None, io.Error]` |
| `net.UdpSocket` | `recv` | `recv(max_bytes: int32, timeout: Duration = ...) -> Result[Option[Vec[uint8]], io.Error]` |
| `net.UdpSocket` | `recv_from` | `recv_from(max_bytes: int32, timeout: Duration = ...) -> Result[Option[net.UdpDatagram], io.Error]` |
| `net.UdpSocket` | `local_addr` | `local_addr() -> Result[String, io.Error]` |
| `net.UdpSocket` | `peer_addr` | `peer_addr() -> Result[String, io.Error]` |
| `net.UdpSocket` | `close` | `close() -> None` |
| `net.UdpDatagram` | `address` | `address() -> String` |
| `net.UdpDatagram` | `bytes` | `bytes() -> Vec[uint8]` |
| `net.UdpDatagram` | `text` | `text() -> Result[String, io.Error]` |
| `net.HttpListener` | `accept` | `accept(timeout: Duration = ...) -> Result[net.HttpExchange, io.Error]` |
| `net.HttpListener` | `local_addr` | `local_addr() -> Result[String, io.Error]` |
| `net.HttpListener` | `close` | `close() -> None` |
| `net.HttpExchange` | `method` | `method() -> String` |
| `net.HttpExchange` | `path` | `path() -> String` |
| `net.HttpExchange` | `headers` | `headers() -> Map[String, String]` |
| `net.HttpExchange` | `body_text` | `body_text() -> Result[String, io.Error]` |
| `net.HttpExchange` | `body_bytes` | `body_bytes() -> Vec[uint8]` |
| `net.HttpExchange` | `respond_text` | `respond_text(status: int32, text: own String, headers: own Map[String, String]) -> Result[None, io.Error]` |
| `net.HttpExchange` | `respond_bytes` | `respond_bytes(status: int32, bytes: own Vec[uint8], headers: own Map[String, String]) -> Result[None, io.Error]` |
| `net.HttpResponse` | `status` | `status() -> int32` |
| `net.HttpResponse` | `reason` | `reason() -> String` |
| `net.HttpResponse` | `headers` | `headers() -> Map[String, String]` |
| `net.HttpResponse` | `text` | `text() -> Result[String, io.Error]` |
| `net.HttpResponse` | `bytes` | `bytes() -> Vec[uint8]` |
| `net.WebSocketListener` | `accept` | `accept(timeout: Duration = ...) -> Result[net.WebSocket, io.Error]` |
| `net.WebSocketListener` | `local_addr` | `local_addr() -> Result[String, io.Error]` |
| `net.WebSocket` | `send_text` | `send_text(text: String, timeout: Duration = ...) -> Result[None, io.Error]` |
| `net.WebSocket` | `send_bytes` | `send_bytes(bytes: Vec[uint8], timeout: Duration = ...) -> Result[None, io.Error]` |
| `net.WebSocket` | `recv_text` | `recv_text(timeout: Duration = ...) -> Result[Option[String], io.Error]` |
| `net.WebSocket` | `recv_bytes` | `recv_bytes(timeout: Duration = ...) -> Result[Option[Vec[uint8]], io.Error]` |
| `net.WebSocket` | `close` | `close() -> None` |
| `net.UnixListener` | `accept` | `accept(timeout: Duration = ...) -> Result[net.UnixStream, io.Error]` |
| `net.UnixListener` | `close` | `close() -> None` |
| `net.UnixStream` | `read_line` | `read_line(timeout: Duration = ...) -> Result[Option[String], io.Error]` |
| `net.UnixStream` | `read_exact` | `read_exact(count: int32, timeout: Duration = ...) -> Result[Vec[uint8], io.Error]` |
| `net.UnixStream` | `write_all` | `write_all(text: String, timeout: Duration = ...) -> Result[None, io.Error]` |
| `net.UnixStream` | `close` | `close() -> None` |
| `net.TlsListener` | `accept` | `accept(timeout: Duration = ...) -> Result[net.TlsStream, io.Error]` |
| `net.TlsListener` | `local_addr` | `local_addr() -> Result[String, io.Error]` |
| `net.TlsListener` | `close` | `close() -> None` |
| `net.TlsStream` | `read_line` | `read_line(timeout: Duration = ...) -> Result[Option[String], io.Error]` |
| `net.TlsStream` | `read_exact` | `read_exact(count: int32, timeout: Duration = ...) -> Result[Vec[uint8], io.Error]` |
| `net.TlsStream` | `write_all` | `write_all(text: String, timeout: Duration = ...) -> Result[None, io.Error]` |
| `net.TlsStream` | `close` | `close() -> None` |

## Process

See [Process Module](/manual/process) for defaults, groups, and supervisor behavior.

| API | Signature |
| --- | --- |
| `process.inherit` | `inherit() -> process.Stdio` |
| `process.null` | `null() -> process.Stdio` |
| `process.pipe` | `pipe() -> process.Stdio` |
| `process.supervisor` | `supervisor() -> process.Supervisor` |
| `process.start` | `start(command: Vec[String], cwd: Option[String] = ..., env: Map[String, String] = ..., stdin: process.Stdio = ..., stdout: process.Stdio = ..., stderr: process.Stdio = ..., group: bool = ...) -> Result[process.Child, process.Error]` |
| `process.run` | `run(command: Vec[String], cwd: Option[String] = ..., env: Map[String, String] = ..., stdin: process.Stdio = ..., stdout: process.Stdio = ..., stderr: process.Stdio = ..., timeout: Duration = ..., group: bool = ...) -> Result[process.Completed, process.Error]` |
| `process.Child.stdin` | `stdin() -> Option[process.Pipe]` |
| `process.Child.stdout` | `stdout() -> Option[process.Pipe]` |
| `process.Child.stderr` | `stderr() -> Option[process.Pipe]` |
| `process.Child.wait` | `wait(timeout: Duration = ...) -> process.Wait` |
| `process.Child.wait_or_none` | `wait_or_none(timeout: Duration = ...) -> Result[Option[process.ExitStatus], process.Error]` |
| `process.Child.wait_ok` | `wait_ok(timeout: Duration = ...) -> Result[process.ExitStatus, process.Error]` |
| `process.Child.kill` | `kill() -> Result[None, process.Error]` |
| `process.Child.terminate` | `terminate() -> Result[None, process.Error]` |
| `process.Child.close` | `close() -> None` |
| `process.Pipe.read_all` | `read_all() -> Result[String, process.Error]` |
| `process.Pipe.read_line` | `read_line(timeout: Duration = ...) -> Result[Option[String], process.Error]` |
| `process.Pipe.read_bytes` | `read_bytes(max_bytes: int32, timeout: Duration = ...) -> Result[Option[Vec[uint8]], process.Error]` |
| `process.Pipe.write_all` | `write_all(text: String, timeout: Duration = ...) -> Result[None, process.Error]` |
| `process.Pipe.write_bytes` | `write_bytes(bytes: Vec[uint8], timeout: Duration = ...) -> Result[None, process.Error]` |
| `process.Pipe.flush` | `flush() -> Result[None, process.Error]` |
| `process.Pipe.close` | `close() -> None` |
| `process.Completed.status` | `status() -> process.ExitStatus` |
| `process.Completed.success` | `success() -> bool` |
| `process.Completed.stdout` | `stdout() -> String` |
| `process.Completed.stdout_bytes` | `stdout_bytes() -> Vec[uint8]` |
| `process.Completed.stderr` | `stderr() -> String` |
| `process.Completed.stderr_bytes` | `stderr_bytes() -> Vec[uint8]` |
| `process.Completed.check` | `check() -> Result[None, process.Error]` |
| `process.Supervisor.start` | `start(name: own String, command: own Vec[String], cwd: own Option[String] = ..., env: own Map[String, String] = ..., stdin: own process.Stdio = ..., stdout: own process.Stdio = ..., stderr: own process.Stdio = ..., restart: own process.RestartPolicy = ..., backoff: own Duration = ..., max_restarts: own int32 = ..., group: own bool = ...) -> Result[None, process.Error]` |
| `process.Supervisor.wait` | `wait(timeout: Duration = ...) -> process.SupervisorWait` |
| `process.Supervisor.wait_or_none` | `wait_or_none(timeout: Duration = ...) -> Result[Option[process.SupervisorEvent], process.Error]` |
| `process.Supervisor.stop` | `stop() -> Result[None, process.Error]` |
| `process.Supervisor.is_empty` | `is_empty() -> bool` |
| `process.Supervisor.close` | `close() -> None` |

Pipe `read_bytes` returns `Ok(None)` only at EOF; timeout and cancellation are `process.Error` variants. Whole/captured reads are capped at 64 MiB. `process.Completed.stdout()` and `.stderr()` raise a runtime diagnostic on invalid UTF-8, so byte accessors are the safe boundary for untrusted output.

## Builtin Enum Variants

| Type | Variants |
| --- | --- |
| `Option[T]` | `Some(value: own T)`, `None` |
| `Result[T, E]` | `Ok(value: own T)`, `Err(error: own E)` |
| `SendError[T]` | `Closed(value: own T)`, `Cancelled(value: own T)`, `TimedOut(value: own T)`, `Full(value: own T)` |
| `QueueReceive[T]` | `Item(value: own T)`, `Closed`, `TimedOut`, `Cancelled` |
| `TaskResult[T]` | `Ready(value: own T)`, `Error(message: own String)`, `TimedOut`, `Cancelled` |
| `SelectOutcome[Q, T]` | `Queue(index: own int32, outcome: own QueueReceive[Q])`, `Task(index: own int32, outcome: own TaskResult[T])`, `Deadline(index: own int32)`, `Cancelled` |
| `WaitAny[T]` | `Ready(index: own int32, value: own T)`, `Error(index: own int32, message: own String)`, `TimedOut`, `Cancelled` |
| `WaitAll[T]` | `Ready(values: own Vec[T])`, `Error(index: own int32, message: own String)`, `TimedOut`, `Cancelled` |
| `bytes.Error` | `InvalidUtf8(index: own int32)`, `InvalidHexLength(length: own int32)`, `InvalidHexDigit(index: own int32, byte: own uint8)`, `InvalidBase64(index: own int32)` |
| `io.Error` | `NotFound`, `PermissionDenied`, `AlreadyExists`, `IsDirectory`, `ConnectionRefused`, `ConnectionReset`, `ConnectionAborted`, `NotConnected`, `AddrInUse`, `AddrNotAvailable`, `BrokenPipe`, `TimedOut`, `WouldBlock`, `UnexpectedEof`, `InvalidInput`, `InvalidData`, `Closed`, `Cancelled`, `Other(message: own String)` |
| `process.Stdio` | `Inherit`, `Null`, `Pipe` |
| `process.ExitStatus` | `Exited(code: own int32)`, `Signaled(signal: own int32)` |
| `process.Wait` | `Exited(status: own process.ExitStatus)`, `TimedOut`, `Cancelled`, `Failed(error: own process.Error)` |
| `process.RestartPolicy` | `Never`, `OnFailure`, `Always` |
| `process.Error` | `NoCommand`, `TimedOut`, `Cancelled`, `Io(error: own io.Error)`, `Spawn(message: own String)`, `Other(message: own String)` |
| `process.SupervisorEvent` | `Exited(name: own String, status: own process.ExitStatus, restart_count: own int32)`, `Restarted(name: own String, status: own process.ExitStatus, restart_count: own int32)`, `Failed(name: own String, error: own process.Error, restart_count: own int32)` |
| `process.SupervisorWait` | `Event(event: own process.SupervisorEvent)`, `TimedOut`, `Cancelled` |
