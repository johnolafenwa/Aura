# API Index

This page lists every public builtin constant, function, method, module type,
and builtin enum that the manual documents. Each entry gives the exact
signature and a one-line summary. The linked manual pages have the examples
and full rules.

`assert condition` and `assert condition, message` are statements, not
callable APIs. [Assertions](/manual/assertions) covers their typing, lazy
message evaluation, `AU4001`, cleanup, and matching behavior across backends.

## Top-Level Builtins

| API | Signature | Behavior |
| --- | --- | --- |
| `print` | `print(value) -> None` | Renders `value` and writes it with a newline. |
| `range` | `range(stop: int64) -> Range`; `range(start: int64, stop: int64) -> Range` | Integer range that excludes the end value. |
| `cancelled` | `cancelled() -> bool` | Returns whether the current task is cancelled. |
| `yield_now` | `yield_now() -> None` | Yields the current lightweight task to the scheduler. |
| `sleep` | `sleep(duration: Duration) -> None` | Suspends the current task through the scheduler. |
| `select` | `select(source, ...) -> SelectOutcome[Q, T]` | Waits on one or more positional sources. Each source is a Queue, a Task, or a relative Duration. Cancellation wins. Otherwise the ready source with the lowest index wins. |
| `wait_any` | `wait_any(tasks: list[Task[T]], timeout: Duration = ...) -> WaitAny[T]` | Waits for the first task outcome. When `T` is non-repeatable, it consumes the list and abandons the rights of the tasks it does not choose. `wait_any([])` returns `TimedOut` immediately. |
| `wait_all` | `wait_all(tasks: list[Task[T]], timeout: Duration = ...) -> WaitAll[T]` | Waits for all tasks, the first task error, the timeout, or cancellation. When `T` is non-repeatable, it consumes the list. |
| `abs` | `abs(value: number) -> number` | Absolute value of an integer or a float. |
| `min` | `min(left: number, right: number) -> number` | The smaller of two values of the same numeric type. |
| `max` | `max(left: number, right: number) -> number` | The larger of two values of the same numeric type. |
| `sqrt` | `sqrt(value: float32|float64) -> float32|float64` | Square root. |
| `round` | `round(value: T) -> T` for integers; `round(value: float32\|float64) -> int64` | Returns an integer unchanged. Rounds a float to the nearest integer, with ties to even. |
| `divmod` | `divmod(left: T, right: T) -> (T, T)` for one exact integer or float type | Returns the floor quotient and the remainder. The remainder takes the sign of the divisor. |
| `parse_int32` | `parse_int32(text: str) -> Result[int32, str]` | Parses a signed 32-bit integer. |
| `parse_int64` | `parse_int64(text: str) -> Result[int64, str]` | Parses a signed 64-bit integer. |
| `parse_float64` | `parse_float64(text: str) -> Result[float64, str]` | Parses a 64-bit float. |
| `len` | `len(value: str\|list[T]\|dict[K, V]\|set[T]\|Array[T]) -> int64` | Returns the same `int64` as the value's own `len()` method. |
| `str` | `str(value) -> str` | Renders `value` exactly as `print` and f-string interpolation do. |

## Foreign Declarations

Foreign function interface declarations, or FFI declarations, are direct calls
that a package authorizes. They are not builtins and not first-class function
values. The binding package supplies their names and exact signatures.

| Surface | Signature | Behavior |
| --- | --- | --- |
| C function | `extern "C" def name(...) -> R` | Synchronous call with no body to the process-global symbol of the same name. |
| Opaque handle | `extern "C" opaque class Handle` | Wraps a non-null foreign pointer. The handle is not Copy, not cloneable, and not Transfer. |
| str view | `text: str` | Passes a temporary const UTF-8 pointer and a byte length. An empty string is `(NULL, 0)`. No NUL terminator is promised. |
| Byte view | `bytes: list[uint8]` | Passes a temporary const pointer and a byte length. An empty list is `(NULL, 0)`. |
| Mutable byte view | `bytes: mut list[uint8]` | Copies into a same-length scratch buffer and back out. The writeback happens after the native call returns and before the result is validated. |
| Consuming handle | `handle: own Handle` | Moves the opaque handle into a foreign call that closes or frees it. |

[FFI v0](/manual/ffi) has the full scalar table, the manifest report, the
safety boundary, diagnostics, and backend rules.

## Scalars And str

| API | Signature | Behavior |
| --- | --- | --- |
| `float64.sqrt` | `sqrt() -> float64` | Square root of the receiver. |
| integer `.to_float` | `to_float() -> float64` | Converts any integer type with IEEE-754 round-to-nearest, ties-to-even. The result may round. |
| integer wrapping methods | `wrapping_add(rhs)`, `wrapping_sub(rhs)`, `wrapping_mul(rhs)` | Two's-complement arithmetic that wraps at the type's fixed width. Operands and result share one type. |
| integer saturating methods | `saturating_add(rhs)`, `saturating_sub(rhs)`, `saturating_mul(rhs)` | Arithmetic that clamps to the declared width. Operands and result share one type. |
| integer wrapping shifts | `wrapping_shl(count)`, `wrapping_shr(count)` | `count` has the receiver's type. Left shift discards high bits. Right shift matches `>>` once the count is validated. |
| integer saturating shifts | `saturating_shl(count)`, `saturating_shr(count)` | `count` has the receiver's type. Left shift clamps. Right shift matches `>>` once the count is validated. |
| scalar `.to_string` | `to_string() -> str` | Available on `bool`, the integer types, `float32`, and `float64`. |
| `Duration.ms` | `Duration.ms(value: int64) -> Duration` | Builds an exact signed Duration from milliseconds. |
| `Duration.seconds` | `Duration.seconds(value: int64) -> Duration` | Builds an exact signed Duration from seconds. |
| `Duration.minutes` | `Duration.minutes(value: int64) -> Duration` | Builds an exact signed Duration from minutes. |
| `Duration.to_ms` | `to_ms() -> float64` | Converts the exact nanosecond count to the nearest binary64 milliseconds, ties-to-even. The result may round. |
| `Duration.to_seconds` | `to_seconds() -> float64` | Converts the exact nanosecond count to the nearest binary64 seconds, ties-to-even. The result may round. |
| `str.len` | `len() -> int64` | Counts Unicode scalar values in O(n) time. |
| `str.byte_len` | `byte_len() -> int64` | Returns the UTF-8 byte count in O(1) time. |
| `str.to_bytes` | `to_bytes() -> list[uint8]` | Returns a fresh list of the receiver's exact UTF-8 bytes. |
| `str.from_bytes` | `from_bytes(bytes: list[uint8]) -> Result[str, bytes.Error]` | Validates UTF-8 strictly. Returns a fresh str, or an error with the offset of the first invalid byte. |
| `str.contains` | `contains(text: str) -> bool` | `true` when the receiver contains `text`. |
| `str.starts_with` | `starts_with(text: str) -> bool` | Tests for a prefix. |
| `str.ends_with` | `ends_with(text: str) -> bool` | Tests for a suffix. |
| `str.split` | `split(text: str) -> list[str]` | Splits at each occurrence of `text`. |
| `str.replace` | `replace(from: str, to: str) -> str` | Returns a new string with the replacements applied. |
| `str.to_lower` | `to_lower() -> str` | Converts to Unicode lowercase. |
| `str.to_upper` | `to_upper() -> str` | Converts to Unicode uppercase. |
| `str.strip_prefix` | `strip_prefix(text: str) -> str \| None` | Returns the rest of the string when the prefix matches, or `None` when it does not. An empty rest is a present `""`. |
| `str.strip_suffix` | `strip_suffix(text: str) -> str \| None` | Returns the rest of the string when the suffix matches, or `None` when it does not. An empty rest is a present `""`. |
| `str.trim` | `trim() -> str` | Removes leading and trailing Unicode whitespace. |
| `str.join` | `join(parts: list[str]) -> str` | Joins `parts` with the receiver as the separator. |
| `str.clone` | `clone() -> str` | Returns a new owned string. |

These Duration operators each return a `Duration`:

- `Duration + Duration`
- `Duration - Duration`
- `Duration * int64`
- `int64 * Duration`
- `Duration // int64`

Duration values also support equality and all four ordering comparisons.
Arithmetic is checked on signed i128 nanoseconds.

## Numeric Arrays

In this section, `T` and `U` are each exactly one of `int32`, `int64`,
`float32`, or `float64`. [Numeric Arrays](/manual/numeric-arrays) covers shape,
ownership, diagnostics, and backend behavior.

| API | Signature | Behavior |
| --- | --- | --- |
| `Array[T].zeros` | `zeros(shape: list[int64]) -> Array[T]` | Fresh zero-filled buffer in row-major order, with rank at least one. |
| `Array[T].full` | `full(shape: list[int64], value: T) -> Array[T]` | Fresh buffer filled with `value`. |
| `Array[T].from_list` | `from_list(values: list[T], shape: list[int64]) -> Array[T]` | Copies the shared list into the exact row-major shape. |
| `Array.shape` | `shape() -> list[int64]` | Returns an owned snapshot of the shape. |
| `Array.len` | `len() -> int64` | Total number of elements. |
| `Array.clone` | `clone() -> Array[T]` | Makes an explicit fresh copy of the whole buffer. |
| `Array.get` | `get(index: list[int64]) -> T \| None` | Reads one coordinate. Returns `None` when the coordinate is out of bounds or the rank does not match. |
| `Array.set` | `set(index: list[int64], value: T) -> T \| None` | Replaces one element in place and returns the old scalar. An invalid coordinate or rank traps instead of returning `None`. |
| `Array.fill` | `fill(value: T) -> None` | Fills every element in place, in row-major order. |
| `Array.map` | `map[U](f: def(T) -> U) -> Array[U]` | Applies a repeatable callback eagerly, in row-major order. |
| `Array.sum` | `sum() -> T` | Deterministic reduction in the element type. An empty Array returns zero. |
| `Array.min` | `min() -> T` | Smallest element. An empty Array is `AU4007`. |
| `Array.max` | `max() -> T` | Largest element. An empty Array is `AU4007`. |
| `Array.mean` | `mean() -> float64` | Accumulates in and returns `float64`. An empty Array is `AU4007`. |
| integer Array wrapping methods | `wrapping_add(rhs)`, `wrapping_sub(rhs)`, `wrapping_mul(rhs)` | `rhs` is an Array of the same shape or a scalar of the same element type. Returns a fresh Array. |
| integer Array saturating methods | `saturating_add(rhs)`, `saturating_sub(rhs)`, `saturating_mul(rhs)` | `rhs` is an Array of the same shape or a scalar of the same element type. Returns a fresh Array. |

## Math

Every argument is exactly `float64`. The module performs no implicit numeric
conversion. [Math Module](/manual/math) has the full IEEE-754, domain,
evaluation-order, diagnostic, and backend rules.

| API | Signature | Behavior |
| --- | --- | --- |
| `math.pi` | `float64` constant | Nearest binary64 value to pi, bits `0x400921fb54442d18`. |
| `math.e` | `float64` constant | Nearest binary64 value to Euler's number, bits `0x4005bf0a8b145769`. |
| `math.inf` | `float64` constant | Positive infinity, bits `0x7ff0000000000000`. |
| `math.nan` | `float64` constant | Canonical quiet NaN, bits `0x7ff8000000000000`. |
| `math.floor` | `floor(value: float64) -> int64` | Greatest integer less than or equal to `value`. Checked against the `int64` range. |
| `math.ceil` | `ceil(value: float64) -> int64` | Least integer greater than or equal to `value`. Checked against the `int64` range. |
| `math.trunc` | `trunc(value: float64) -> int64` | Truncates toward zero. Checked against the `int64` range. |
| `math.pow` | `pow(base: float64, exponent: float64) -> float64` | Binary64 exponentiation. The Math Module page specifies its identity, domain, and overflow cases. |
| `math.exp` | `exp(value: float64) -> float64` | Base-e exponential. |
| `math.log` | `log(value: float64) -> float64` | Natural logarithm. |
| `math.log2` | `log2(value: float64) -> float64` | Base-2 logarithm. |
| `math.log10` | `log10(value: float64) -> float64` | Base-10 logarithm. |
| `math.sin` | `sin(value: float64) -> float64` | Sine in radians. |
| `math.cos` | `cos(value: float64) -> float64` | Cosine in radians. |
| `math.tan` | `tan(value: float64) -> float64` | Tangent in radians. |

## Randomness

[Randomness Module](/manual/randomness) specifies the xoshiro256** algorithm,
the seed-42 vectors, ownership, the secure-source boundary, and diagnostics.

| API | Signature | Behavior |
| --- | --- | --- |
| `random.Rng` | `Rng(seed: int64) -> random.Rng` | Creates a move-only deterministic stream. The seed's exact two's-complement bit pattern selects the stream. |
| `random.Rng.next_int` | `next_int(lo: int64, hi: int64) -> int64` | Uniform integer in the half-open range `[lo, hi)`. Needs a mutable receiver. |
| `random.Rng.next_float` | `next_float() -> float64` | Uniform 53-bit binary64 value in `[0.0, 1.0)`. Needs a mutable receiver. |
| `random.Rng.shuffle` | `shuffle[T](values: mut list[T]) -> None` | Shuffles in place with descending Fisher-Yates. Needs a mutable receiver and a mutable list. |
| `random.secure_int` | `secure_int(lo: int64, hi: int64) -> int64` | Uniform half-open integer from the operating system's secure source. Has no deterministic fallback. |
| `random.secure_bytes` | `secure_bytes(n: int64) -> list[uint8]` | Returns exactly `n` secure bytes from the operating system for `0 <= n <= 2147483647`. Zero skips the entropy read. A larger count traps with `AU4005` before allocating. |

`random.Rng` has no public clone route. `AU3007` rejects the clone-producing
collection and task APIs on this page when the value they produce contains, or
may contain, an `Rng`. That includes an `Rng` inside a user-defined wrapper.

Cloning an allowed Task or Queue handle copies only the handle. `AU3008`
rejects `random.Rng` as a task result or a Queue payload. A Task with a
non-repeatable result is not copyable.

Moves, collection removals, and in-place shuffles within one owning task move
or reorder values. None of them duplicates generator state.

Generic clone-producing calls infer clone-safety obligations and check them
after specialization. The obligation carries through generic callers and
module imports.

## Bytes, Text Codecs, And SHA-256

[Bytes, Text Codecs, And SHA-256](/manual/bytes) covers exact UTF-8
preservation, the strict policy for malformed input, error offsets, ownership,
up-front output size checks, and the cryptographic scope of SHA-256.

| API | Signature | Behavior |
| --- | --- | --- |
| `str.to_bytes` | `to_bytes() -> list[uint8]` | Exact UTF-8 bytes. Reads a shared receiver and returns a fresh list. |
| `str.from_bytes` | `from_bytes(bytes: list[uint8]) -> Result[str, bytes.Error]` | Strict UTF-8 with no replacement decoding. |
| `bytes.hex_encode` | `hex_encode(value: list[uint8]) -> str` | Two lowercase ASCII digits per byte. |
| `bytes.hex_decode` | `hex_decode(text: str) -> Result[list[uint8], bytes.Error]` | Accepts ASCII hex in mixed case. Rejects prefixes, separators, and whitespace. |
| `bytes.base64_encode` | `base64_encode(value: list[uint8]) -> str` | RFC 4648 standard alphabet with canonical padding. |
| `bytes.base64_decode` | `base64_decode(text: str) -> Result[list[uint8], bytes.Error]` | Strict decode of canonical standard-alphabet input. |
| `bytes.sha256` | `sha256(value: list[uint8]) -> list[uint8]` | Fresh raw 32-byte FIPS 180-4 digest. |
| `bytes.sha256_string` | `sha256_string(text: str) -> list[uint8]` | SHA-256 of the text's exact UTF-8 bytes. |

Every input above uses shared access and stays usable after the call. An
`encoding` argument is reserved but not implemented. Expanded output that
cannot be represented or allocated traps with `AU4005`. Malformed data returns
`bytes.Error`.

## Collections

[Collections](/manual/collections) covers ownership and iteration.

### list[T]

| API | Signature | Behavior |
| --- | --- | --- |
| `list[T]()` | `list[T]()` | Creates an empty list. |
| `list.len` | `len() -> int64` | Number of elements. |
| `list.is_empty` | `is_empty() -> bool` | `true` when the list is empty. |
| `list.copy` | `copy() -> list[T]` | Returns independent owned storage. Requires clone-safe `T`. |
| `list.append` | `append(value: own T) -> None` | Moves `value` onto the end. |
| `list.pop` | `pop(index: int64 = -1) -> T` | Removes the element at the normalized position and moves it out. An invalid position traps. |
| `list.get` | `get(index: int64) -> Lookup[T]` | Normalizes a negative index, then returns `Lookup.Found(value)` with a clone of the element. Returns `Lookup.Missing` when the index is out of bounds. Requires clone-safe `T`. |
| `list.lookup` | `match [mut] items.lookup(index):` | Normalizes a negative index once. Runs `case Lookup.Found(item):` with a view of the element, or the `Missing` arm when the index is out of bounds. Only valid as a `match` subject. Has no clone requirement. |
| `list.set` | `set(index: int64, value: own T) -> T` | Replaces the element and moves the old one out. An invalid position traps. |
| `list.remove` | `remove(value: T) -> None` | Removes the first equal value. A missing value traps with `AU4008`. |
| `list.index` | `index(value: T) -> int64` | Returns the position of the first equal value. A missing value traps with `AU4008`. |
| `list.count` | `count(value: T) -> int64` | Number of equal elements. |
| `list.swap` | `swap(first: int64, second: int64) -> None` | Normalizes two positions and swaps them. An invalid position traps. |
| `list.extend` | `extend(other: own list[T]) -> None` | Moves the elements of `other` into the receiver. |
| `list.insert` | `insert(index: int64, value: own T) -> None` | Inserts before the position, clamped the way Python clamps it. |
| `list.clear` | `clear() -> None` | Removes all elements. |
| `list.reverse` | `reverse() -> None` | Reverses the list in place. |
| `list.sort` | `sort(reverse: bool = false) -> None` | Stable in-place sort by natural order. Requires `T: Ord`. |
| `list.sort` | `sort[K](key: def(T) -> K, reverse: bool = false) -> None` | Stable sort by key. Computes each key once before changing the list. Requires `K: Ord`. |
| `list.map` | `map[U](f: def(T) -> U) -> list[U]` | Traverses the list eagerly through shared access into a fresh owned result. The source stays usable. |
| `list.filter` | `filter(f: def(T) -> bool) -> list[T]` | Traverses the list eagerly through shared access into a fresh owned result. The source stays usable. Requires clone-safe `T`. |
| `list.reserve` | `reserve(additional: int64) -> None` | Makes room for at least `len() + additional` elements. |
| `list[T].with_capacity` | `with_capacity(minimum: int64) -> list[T]` | Creates an empty list with at least the requested capacity. |

### dict[K, V]

| API | Signature | Behavior |
| --- | --- | --- |
| `dict[K, V]()` | `dict[K, V]()` | Creates an empty dictionary. |
| `dict.len` | `len() -> int64` | Number of entries. |
| `dict.is_empty` | `is_empty() -> bool` | `true` when the dictionary is empty. |
| `dict.copy` | `copy() -> dict[K, V]` | Returns independent owned storage. Requires clone-safe `K` and `V`. |
| `dict.get` | `get(key: K) -> Lookup[V]` | Returns `Lookup.Found(value)` with a clone of the value, or `Lookup.Missing` when the key is absent. A stored `None` value is `Found(None)`. Requires clone-safe `V`. |
| `dict.lookup` | `match [mut] table.lookup(key):` | Runs `case Lookup.Found(value):` with a view of the value, or the `Missing` arm when the key is absent. Only valid as a `match` subject. Has no clone requirement. |
| `dict.remove` | `remove(key: K) -> Lookup[V]` | Removes the entry and moves its value into `Lookup.Found(value)`, or returns `Lookup.Missing`. Has no clone requirement. |
| `dict.keys` | `keys() -> list[K]` | Clones of the keys, in insertion order. Requires clone-safe `K`. |
| `dict.values` | `values() -> list[V]` | Clones of the values, in insertion order. Requires clone-safe `V`. |
| `dict.items` | `items() -> list[(K, V)]` | Cloned key/value tuples, in insertion order. Requires clone-safe `K` and `V`. |
| `dict.clear` | `clear() -> None` | Removes all entries. |
| `dict.update` | `update(other: own dict[K, V]) -> None` | Moves the entries of `other` into the receiver. A key that already exists keeps its position. |
| `dict.reserve` | `reserve(additional: int64) -> None` | Makes room for at least `len() + additional` entries. |
| `dict[K, V].with_capacity` | `with_capacity(minimum: int64) -> dict[K, V]` | Creates an empty dictionary with at least the requested capacity. |

### set[T]

| API | Signature | Behavior |
| --- | --- | --- |
| `set[T]()` | `set[T]()` | Creates an empty set. |
| `set.len` | `len() -> int64` | Number of unique values. |
| `set.is_empty` | `is_empty() -> bool` | `true` when the set is empty. |
| `set.copy` | `copy() -> set[T]` | Returns independent owned storage. Requires clone-safe `T`. |
| `set.add` | `add(value: own T) -> None` | Moves `value` into the set. |
| `set.remove` | `remove(value: T) -> None` | Removes an equal value. A missing value traps with `AU4008`. |
| `set.discard` | `discard(value: T) -> None` | Removes an equal value if one is present. |
| `set.clear` | `clear() -> None` | Removes all values. |
| `set.reserve` | `reserve(additional: int64) -> None` | Makes room for at least `len() + additional` values. |
| `set[T].with_capacity` | `with_capacity(minimum: int64) -> set[T]` | Creates an empty set with at least the requested capacity. |

## Concurrency

[Concurrency](/manual/concurrency) covers structured concurrency.

| API | Signature | Behavior |
| --- | --- | --- |
| `Queue[T]()` | `Queue[T](capacity: int32 = ...)` | Creates a queue. Supplying `capacity` makes it bounded. Requires `T: Transfer`. |
| `Queue.put` | `put(value: own T, timeout: Duration = ...) -> Result[None, SendError[T]]` | Sends a value. On failure, the error holds the unsent value. Requires `T: Transfer`. |
| `Queue.try_put` | `try_put(value: own T) -> Result[None, SendError[T]]` | Sends without waiting. Requires `T: Transfer`. |
| `Queue.get` | `get(timeout: Duration = ...) -> QueueReceive[T]` | Receives an item, or reports close, timeout, or cancellation. Does not check the payload for Transfer again. |
| `Queue.poll` | `poll(timeout: Duration = ...) -> Poll[T]` | Returns `Poll.Ready(value)`, or `Poll.Unavailable` on close, timeout, cancellation, or no item right away. |
| `Queue.get_or` | `get_or(default: own T, timeout: Duration = ...) -> T` | Returns a received value or the fallback. |
| `Queue.close` | `close() -> None` | Closes the queue and wakes any waiters. |
| `Task.result` | `result(timeout: Duration = ...) -> TaskResult[T]` | Waits for the task outcome. When `T` is non-repeatable, it consumes the observation right. |
| `Task.poll` | `poll(timeout: Duration = ...) -> Poll[T]` | Returns `Poll.Ready(value)`, or `Poll.Unavailable` on failure, timeout, cancellation, or no result right away. When `T` is non-repeatable, it consumes the observation right, even on `Unavailable`. |
| `Task.result_or` | `result_or(default: own T, timeout: Duration = ...) -> T` | Returns the value or the fallback. When `T` is non-repeatable, it consumes the observation right. |
| `TaskGroup()` | `TaskGroup()` | Creates a task group resource. |
| `TaskGroup.start` | `start(function, own ...) -> Task[T]` | Every capture and the result must be `Transfer`. The target may be inferred or explicit, as `function[Types]` or `Type.associated_method[Types]`. The child runs on the guarded 768 KiB default stack. |
| `TaskGroup.start_soon` | `start_soon(function, own ...) -> None` | Same Transfer and target rules as `start`, without returning a handle. |
| `TaskGroup.start_with_stack` | `start_with_stack(bytes: int64, function, own ...) -> Task[T]` | Same Transfer and target rules as `start`, with an explicit guarded stack request of 256 KiB..64 MiB. |
| `TaskGroup.start_soon_with_stack` | `start_soon_with_stack(bytes: int64, function, own ...) -> None` | Same rules and stack range as `start_with_stack`, without keeping a handle. |
| `TaskGroup.cancel` | `cancel() -> None` | Signals cancellation to the group's children. |

The two stack-size methods are provisional. Their 256 KiB minimum is for tasks
whose shallow stack use you have measured. It is not the default.

## I/O And Filesystem

[I/O Module](/manual/io) covers input and output on the standard streams.
[Filesystem Module](/manual/filesystem) covers files and directories.

| API | Signature | Behavior |
| --- | --- | --- |
| `io.write` | `write(text: str) -> Result[None, io.Error]` | Writes text without a newline. |
| `io.flush` | `flush() -> Result[None, io.Error]` | Flushes standard output. |
| `io.read_line` | `read_line() -> Result[str \| None, io.Error]` | Reads one line as strict UTF-8, without the trailing LF or CRLF. Returns `Ok(None)` only at a clean end of file. |
| `fs.exists` | `exists(path: str) -> bool` | Checks whether the path exists. |
| `fs.read_to_string` | `read_to_string(path: str) -> Result[str, io.Error]` | Reads UTF-8 text, capped at 256 MiB. |
| `fs.read_bytes` | `read_bytes(path: str) -> Result[list[uint8], io.Error]` | Reads bytes, capped at 256 MiB. |
| `fs.write_string` | `write_string(path: str, text: str) -> Result[None, io.Error]` | Creates or replaces a text file. |
| `fs.write_bytes` | `write_bytes(path: str, bytes: list[uint8]) -> Result[None, io.Error]` | Creates or replaces a byte file. |
| `fs.append_string` | `append_string(path: str, text: str) -> Result[None, io.Error]` | Appends text. |
| `fs.append_bytes` | `append_bytes(path: str, bytes: list[uint8]) -> Result[None, io.Error]` | Appends bytes. |
| `fs.create_dir` | `create_dir(path: str) -> Result[None, io.Error]` | Creates one directory. |
| `fs.read_dir` | `read_dir(path: str) -> Result[list[str], io.Error]` | Returns the sorted names of the immediate entries. Host paths are decoded lossily. |
| `fs.remove_file` | `remove_file(path: str) -> Result[None, io.Error]` | Removes a file. |
| `fs.open` | `open(path: str) -> Result[fs.File, io.Error]` | Opens a file for reading. |
| `fs.create` | `create(path: str) -> Result[fs.File, io.Error]` | Creates or truncates a file for writing. |
| `fs.append` | `append(path: str) -> Result[fs.File, io.Error]` | Opens a file for appending and creates it if needed. |
| `fs.File.read_all` | `read_all() -> Result[str, io.Error]` | Reads the rest of the file as strict UTF-8 text, capped at 256 MiB. |
| `fs.File.read_bytes` | `read_bytes() -> Result[list[uint8], io.Error]` | Reads the remaining bytes, capped at 256 MiB. |
| `fs.File.write_all` | `write_all(text: str) -> Result[None, io.Error]` | Writes all of the text. |
| `fs.File.write_bytes` | `write_bytes(bytes: list[uint8]) -> Result[None, io.Error]` | Writes all of the bytes. |
| `fs.File.flush` | `flush() -> Result[None, io.Error]` | Flushes pending writes. |
| `fs.File.close` | `close() -> None` | Closes the handle. |

## Control-Plane Modules

[Control-Plane Modules](/manual/control-plane) documents these modules.

| API | Signature |
| --- | --- |
| `sys.args` | `args() -> list[str]` |
| `sys.env` | `env(name: str) -> str \| None` |
| `sys.current_dir` | `current_dir() -> Result[str, io.Error]` |
| `sys.unix_time_ms` | `unix_time_ms() -> int64` |
| `sys.monotonic_time_ms` | `monotonic_time_ms() -> int64` |
| `path.join` | `join(base: str, child: str) -> str` |
| `path.parent` | `parent(path: str) -> str \| None` |
| `path.file_name` | `file_name(path: str) -> str \| None` |
| `path.extension` | `extension(path: str) -> str \| None` |
| `path.is_absolute` | `is_absolute(path: str) -> bool` |
| `json.parse` | `parse(text: str) -> Result[json.Value, json.Error]` |
| `json.dumps` | `dumps(value: json.Value, indent: int64 \| None = None) -> str` |
| `json.is_null` | `is_null(value: json.Value) -> bool` |
| `json.as_bool` | `as_bool(value: json.Value) -> bool \| None` |
| `json.as_int` | `as_int(value: json.Value) -> int64 \| None` |
| `json.as_float` | `as_float(value: json.Value) -> float64 \| None` |
| `json.into_string` | `into_string(value: own json.Value) -> str \| None` |
| `json.into_array` | `into_array(value: own json.Value) -> list[json.Value] \| None` |
| `json.into_object` | `into_object(value: own json.Value) -> dict[str, json.Value] \| None` |
| `json.is_valid` / `toml.is_valid` | `is_valid(text: str) -> bool` |
| `json.stringify_map` / `toml.stringify_map` | `stringify_map(value: dict[str, str]) -> Result[str, str]` |
| `json.parse_string_map` / `toml.parse_string_map` | `parse_string_map(text: str) -> Result[dict[str, str], str]` |
| `log.debug/info/warn/error` | `(message: str, fields: dict[str, str]) -> None` |
| `trace.event` | `(name: str, fields: dict[str, str]) -> None` |
| `metrics.increment` | `(name: str, value: int64) -> None` |
| `metrics.get` | `(name: str) -> int64` |
| `metrics.reset` | `() -> None` |
| `control.retry` | `retry[T, E](worker: def() -> Result[T, E], max_attempts: int32 = 3, initial_backoff: Duration = 0ms) -> Result[T, E]` |

Metrics are process-global `int64` counters. A missing name reads as zero.
Overflow is a runtime diagnostic.

Dynamic JSON object dumps and bounded JSON and TOML string maps serialize keys
in sorted order. The [JSON Module](/manual/json) page and the control-plane
chapter give the exact rules for values, limits, host strings and paths, and
telemetry records.

`control.retry` validates its arguments before it calls the worker. It needs
at least one attempt and a backoff that is non-negative and representable on
the host. It then:

- runs the first attempt immediately
- retries every `Err`, doubling the delay each time
- skips sleeps of zero
- returns the exact last `Err`
- does no sleep or multiplication after the final attempt

Worker traps, backoff overflow, and cancellation of the current task
propagate.

## Network Constructors And HTTP Client Helpers

[Network Module](/manual/network) covers behavior and examples.

| API | Signature |
| --- | --- |
| `net.connect` | `connect(address: str) -> Result[net.TcpStream, io.Error]` |
| `net.connect_timeout` | `connect_timeout(address: str, timeout: Duration) -> Result[net.TcpStream, io.Error]` |
| `net.listen` | `listen(address: str) -> Result[net.TcpListener, io.Error]` |
| `net.udp_bind` | `udp_bind(address: str) -> Result[net.UdpSocket, io.Error]` |
| `net.http_listen` | `http_listen(address: str) -> Result[net.HttpListener, io.Error]` |
| `net.websocket_listen` | `websocket_listen(address: str) -> Result[net.WebSocketListener, io.Error]` |
| `net.websocket_connect` | `websocket_connect(url: str) -> Result[net.WebSocket, io.Error]` |
| `net.websocket_connect_timeout` | `websocket_connect_timeout(url: str, timeout: Duration) -> Result[net.WebSocket, io.Error]` |
| `net.unix_listen` | `unix_listen(path: str) -> Result[net.UnixListener, io.Error]` |
| `net.unix_connect` | `unix_connect(path: str) -> Result[net.UnixStream, io.Error]` |
| `net.unix_connect_timeout` | `unix_connect_timeout(path: str, timeout: Duration) -> Result[net.UnixStream, io.Error]` |
| `net.tls_listen` | `tls_listen(address: str, cert_pem_path: str, key_pem_path: str) -> Result[net.TlsListener, io.Error]` |
| `net.tls_connect` | `tls_connect(address: str, server_name: str, ca_pem_path: str) -> Result[net.TlsStream, io.Error]` |
| `net.tls_connect_timeout` | `tls_connect_timeout(address: str, server_name: str, ca_pem_path: str, timeout: Duration) -> Result[net.TlsStream, io.Error]` |
| `net.http_request_text` | `http_request_text(method: str, url: str, body: str, headers: dict[str, str]) -> Result[net.HttpResponse, io.Error]` |
| `net.http_request_text_timeout` | `http_request_text_timeout(method: str, url: str, body: str, headers: dict[str, str], timeout: Duration) -> Result[net.HttpResponse, io.Error]` |
| `net.http_request_bytes` | `http_request_bytes(method: str, url: str, bytes: list[uint8], headers: dict[str, str]) -> Result[net.HttpResponse, io.Error]` |
| `net.http_request_bytes_timeout` | `http_request_bytes_timeout(method: str, url: str, bytes: list[uint8], headers: dict[str, str], timeout: Duration) -> Result[net.HttpResponse, io.Error]` |

## Network Resource Methods

These limits apply to the methods below:

- Bounded stream read counts are `1..=67108864`.
- UDP receive counts are `1..=65535`.
- Incoming HTTP parsing accepts at most 64 headers and 16 MiB of wire data per
  message.
- WebSocket limits are 64 MiB per message and 16 MiB per frame/write buffer.

[Network Module](/manual/network) covers timeouts, EOF, UTF-8, cancellation,
and repeated headers.

| Type | API | Signature |
| --- | --- | --- |
| `net.TcpListener` | `accept` | `accept(timeout: Duration = ...) -> Result[net.TcpStream, io.Error]` |
| `net.TcpListener` | `local_addr` | `local_addr() -> Result[str, io.Error]` |
| `net.TcpListener` | `close` | `close() -> None` |
| `net.TcpStream` | `read_all` | `read_all(timeout: Duration = ...) -> Result[str, io.Error]` |
| `net.TcpStream` | `read_line` | `read_line(timeout: Duration = ...) -> Result[str \| None, io.Error]` |
| `net.TcpStream` | `read_bytes` | `read_bytes(max_bytes: int32, timeout: Duration = ...) -> Result[list[uint8] \| None, io.Error]` |
| `net.TcpStream` | `read_exact` | `read_exact(count: int32, timeout: Duration = ...) -> Result[list[uint8], io.Error]` |
| `net.TcpStream` | `write_all` | `write_all(text: str, timeout: Duration = ...) -> Result[None, io.Error]` |
| `net.TcpStream` | `write_bytes` | `write_bytes(bytes: list[uint8], timeout: Duration = ...) -> Result[None, io.Error]` |
| `net.TcpStream` | `flush` | `flush() -> Result[None, io.Error]` |
| `net.TcpStream` | `local_addr` | `local_addr() -> Result[str, io.Error]` |
| `net.TcpStream` | `peer_addr` | `peer_addr() -> Result[str, io.Error]` |
| `net.TcpStream` | `shutdown_read` | `shutdown_read() -> Result[None, io.Error]` |
| `net.TcpStream` | `shutdown_write` | `shutdown_write() -> Result[None, io.Error]` |
| `net.TcpStream` | `shutdown_both` | `shutdown_both() -> Result[None, io.Error]` |
| `net.TcpStream` | `close` | `close() -> None` |
| `net.UdpSocket` | `send_text` | `send_text(address: str, text: str, timeout: Duration = ...) -> Result[None, io.Error]` |
| `net.UdpSocket` | `send_bytes` | `send_bytes(address: str, bytes: list[uint8], timeout: Duration = ...) -> Result[None, io.Error]` |
| `net.UdpSocket` | `recv` | `recv(max_bytes: int32, timeout: Duration = ...) -> Result[list[uint8] \| None, io.Error]` |
| `net.UdpSocket` | `recv_from` | `recv_from(max_bytes: int32, timeout: Duration = ...) -> Result[net.UdpDatagram \| None, io.Error]` |
| `net.UdpSocket` | `local_addr` | `local_addr() -> Result[str, io.Error]` |
| `net.UdpSocket` | `peer_addr` | `peer_addr() -> Result[str, io.Error]` |
| `net.UdpSocket` | `close` | `close() -> None` |
| `net.UdpDatagram` | `address` | `address() -> str` |
| `net.UdpDatagram` | `bytes` | `bytes() -> list[uint8]` |
| `net.UdpDatagram` | `text` | `text() -> Result[str, io.Error]` |
| `net.HttpListener` | `accept` | `accept(timeout: Duration = ...) -> Result[net.HttpExchange, io.Error]` |
| `net.HttpListener` | `local_addr` | `local_addr() -> Result[str, io.Error]` |
| `net.HttpListener` | `close` | `close() -> None` |
| `net.HttpExchange` | `method` | `method() -> str` |
| `net.HttpExchange` | `path` | `path() -> str` |
| `net.HttpExchange` | `headers` | `headers() -> dict[str, str]` |
| `net.HttpExchange` | `body_text` | `body_text() -> Result[str, io.Error]` |
| `net.HttpExchange` | `body_bytes` | `body_bytes() -> list[uint8]` |
| `net.HttpExchange` | `respond_text` | `respond_text(status: int32, text: own str, headers: own dict[str, str]) -> Result[None, io.Error]` |
| `net.HttpExchange` | `respond_bytes` | `respond_bytes(status: int32, bytes: own list[uint8], headers: own dict[str, str]) -> Result[None, io.Error]` |
| `net.HttpResponse` | `status` | `status() -> int32` |
| `net.HttpResponse` | `reason` | `reason() -> str` |
| `net.HttpResponse` | `headers` | `headers() -> dict[str, str]` |
| `net.HttpResponse` | `text` | `text() -> Result[str, io.Error]` |
| `net.HttpResponse` | `bytes` | `bytes() -> list[uint8]` |
| `net.WebSocketListener` | `accept` | `accept(timeout: Duration = ...) -> Result[net.WebSocket, io.Error]` |
| `net.WebSocketListener` | `local_addr` | `local_addr() -> Result[str, io.Error]` |
| `net.WebSocket` | `send_text` | `send_text(text: str, timeout: Duration = ...) -> Result[None, io.Error]` |
| `net.WebSocket` | `send_bytes` | `send_bytes(bytes: list[uint8], timeout: Duration = ...) -> Result[None, io.Error]` |
| `net.WebSocket` | `recv_text` | `recv_text(timeout: Duration = ...) -> Result[str \| None, io.Error]` |
| `net.WebSocket` | `recv_bytes` | `recv_bytes(timeout: Duration = ...) -> Result[list[uint8] \| None, io.Error]` |
| `net.WebSocket` | `close` | `close() -> None` |
| `net.UnixListener` | `accept` | `accept(timeout: Duration = ...) -> Result[net.UnixStream, io.Error]` |
| `net.UnixListener` | `close` | `close() -> None` |
| `net.UnixStream` | `read_line` | `read_line(timeout: Duration = ...) -> Result[str \| None, io.Error]` |
| `net.UnixStream` | `read_exact` | `read_exact(count: int32, timeout: Duration = ...) -> Result[list[uint8], io.Error]` |
| `net.UnixStream` | `write_all` | `write_all(text: str, timeout: Duration = ...) -> Result[None, io.Error]` |
| `net.UnixStream` | `close` | `close() -> None` |
| `net.TlsListener` | `accept` | `accept(timeout: Duration = ...) -> Result[net.TlsStream, io.Error]` |
| `net.TlsListener` | `local_addr` | `local_addr() -> Result[str, io.Error]` |
| `net.TlsListener` | `close` | `close() -> None` |
| `net.TlsStream` | `read_line` | `read_line(timeout: Duration = ...) -> Result[str \| None, io.Error]` |
| `net.TlsStream` | `read_exact` | `read_exact(count: int32, timeout: Duration = ...) -> Result[list[uint8], io.Error]` |
| `net.TlsStream` | `write_all` | `write_all(text: str, timeout: Duration = ...) -> Result[None, io.Error]` |
| `net.TlsStream` | `close` | `close() -> None` |

## Process

[Process Module](/manual/process) covers defaults, process groups, and
supervisor behavior.

| API | Signature |
| --- | --- |
| `process.inherit` | `inherit() -> process.Stdio` |
| `process.null` | `null() -> process.Stdio` |
| `process.pipe` | `pipe() -> process.Stdio` |
| `process.supervisor` | `supervisor() -> process.Supervisor` |
| `process.start` | `start(command: list[str], cwd: str \| None = ..., env: dict[str, str] = ..., stdin: process.Stdio = ..., stdout: process.Stdio = ..., stderr: process.Stdio = ..., group: bool = ...) -> Result[process.Child, process.Error]` |
| `process.run` | `run(command: list[str], cwd: str \| None = ..., env: dict[str, str] = ..., stdin: process.Stdio = ..., stdout: process.Stdio = ..., stderr: process.Stdio = ..., timeout: Duration = ..., group: bool = ...) -> Result[process.Completed, process.Error]` |
| `process.Child.stdin` | `stdin() -> process.Pipe \| None` |
| `process.Child.stdout` | `stdout() -> process.Pipe \| None` |
| `process.Child.stderr` | `stderr() -> process.Pipe \| None` |
| `process.Child.wait` | `wait(timeout: Duration = ...) -> process.Wait` |
| `process.Child.wait_or_none` | `wait_or_none(timeout: Duration = ...) -> Result[process.ExitStatus \| None, process.Error]` |
| `process.Child.wait_ok` | `wait_ok(timeout: Duration = ...) -> Result[process.ExitStatus, process.Error]` |
| `process.Child.kill` | `kill() -> Result[None, process.Error]` |
| `process.Child.terminate` | `terminate() -> Result[None, process.Error]` |
| `process.Child.close` | `close() -> None` |
| `process.Pipe.read_all` | `read_all() -> Result[str, process.Error]` |
| `process.Pipe.read_line` | `read_line(timeout: Duration = ...) -> Result[str \| None, process.Error]` |
| `process.Pipe.read_bytes` | `read_bytes(max_bytes: int32, timeout: Duration = ...) -> Result[list[uint8] \| None, process.Error]` |
| `process.Pipe.write_all` | `write_all(text: str, timeout: Duration = ...) -> Result[None, process.Error]` |
| `process.Pipe.write_bytes` | `write_bytes(bytes: list[uint8], timeout: Duration = ...) -> Result[None, process.Error]` |
| `process.Pipe.flush` | `flush() -> Result[None, process.Error]` |
| `process.Pipe.close` | `close() -> None` |
| `process.Completed.status` | `status() -> process.ExitStatus` |
| `process.Completed.success` | `success() -> bool` |
| `process.Completed.stdout` | `stdout() -> str` |
| `process.Completed.stdout_bytes` | `stdout_bytes() -> list[uint8]` |
| `process.Completed.stderr` | `stderr() -> str` |
| `process.Completed.stderr_bytes` | `stderr_bytes() -> list[uint8]` |
| `process.Completed.check` | `check() -> Result[None, process.Error]` |
| `process.Supervisor.start` | `start(name: own str, command: own list[str], cwd: own (str \| None) = ..., env: own dict[str, str] = ..., stdin: own process.Stdio = ..., stdout: own process.Stdio = ..., stderr: own process.Stdio = ..., restart: own process.RestartPolicy = ..., backoff: own Duration = ..., max_restarts: own int32 = ..., group: own bool = ...) -> Result[None, process.Error]` |
| `process.Supervisor.wait` | `wait(timeout: Duration = ...) -> process.SupervisorWait` |
| `process.Supervisor.wait_or_none` | `wait_or_none(timeout: Duration = ...) -> Result[process.SupervisorEvent \| None, process.Error]` |
| `process.Supervisor.stop` | `stop() -> Result[None, process.Error]` |
| `process.Supervisor.is_empty` | `is_empty() -> bool` |
| `process.Supervisor.close` | `close() -> None` |

Pipe `read_bytes` returns `Ok(None)` only at end of file. Timeout and
cancellation are `process.Error` variants. Whole and captured reads are capped
at 64 MiB.

`process.Completed.stdout()` and `.stderr()` raise a runtime diagnostic on
invalid UTF-8. Use the byte accessors to read untrusted output safely.

## Builtin Enum Variants

Each builtin enum and its variants, with payload fields.

| Type | Variants |
| --- | --- |
| `Lookup[T]` | `Found(value: own T)`, `Missing` |
| `Poll[T]` | `Ready(value: own T)`, `Unavailable` |
| `Result[T, E]` | `Ok(value: own T)`, `Err(error: own E)` |
| `SendError[T]` | `Closed(value: own T)`, `Cancelled(value: own T)`, `TimedOut(value: own T)`, `Full(value: own T)` |
| `QueueReceive[T]` | `Item(value: own T)`, `Closed`, `TimedOut`, `Cancelled` |
| `TaskResult[T]` | `Ready(value: own T)`, `Error(message: own str)`, `TimedOut`, `Cancelled` |
| `SelectOutcome[Q, T]` | `Queue(index: own int64, outcome: own QueueReceive[Q])`, `Task(index: own int64, outcome: own TaskResult[T])`, `Deadline(index: own int64)`, `Cancelled` |
| `WaitAny[T]` | `Ready(index: own int64, value: own T)`, `Error(index: own int64, message: own str)`, `TimedOut`, `Cancelled` |
| `WaitAll[T]` | `Ready(values: own list[T])`, `Error(index: own int64, message: own str)`, `TimedOut`, `Cancelled` |
| `bytes.Error` | `InvalidUtf8(index: own int32)`, `InvalidHexLength(length: own int32)`, `InvalidHexDigit(index: own int32, byte: own uint8)`, `InvalidBase64(index: own int32)` |
| `io.Error` | `NotFound`, `PermissionDenied`, `AlreadyExists`, `IsDirectory`, `ConnectionRefused`, `ConnectionReset`, `ConnectionAborted`, `NotConnected`, `AddrInUse`, `AddrNotAvailable`, `BrokenPipe`, `TimedOut`, `WouldBlock`, `UnexpectedEof`, `InvalidInput`, `InvalidData`, `Closed`, `Cancelled`, `Other(message: own str)` |
| `process.Stdio` | `Inherit`, `Null`, `Pipe` |
| `process.ExitStatus` | `Exited(code: own int32)`, `Signaled(signal: own int32)` |
| `process.Wait` | `Exited(status: own process.ExitStatus)`, `TimedOut`, `Cancelled`, `Failed(error: own process.Error)` |
| `process.RestartPolicy` | `Never`, `OnFailure`, `Always` |
| `process.Error` | `NoCommand`, `TimedOut`, `Cancelled`, `Io(error: own io.Error)`, `Spawn(message: own str)`, `Other(message: own str)` |
| `process.SupervisorEvent` | `Exited(name: own str, status: own process.ExitStatus, restart_count: own int32)`, `Restarted(name: own str, status: own process.ExitStatus, restart_count: own int32)`, `Failed(name: own str, error: own process.Error, restart_count: own int32)` |
| `process.SupervisorWait` | `Event(event: own process.SupervisorEvent)`, `TimedOut`, `Cancelled` |

Design records: [ADR-0019](https://github.com/johnolafenwa/Aura/blob/main/architecture_docs/decisions/0019-duration-conversion-and-timer-policy.md)
for Duration conversion,
[ADR-0032](https://github.com/johnolafenwa/Aura/blob/main/architecture_docs/decisions/0032-guarded-lightweight-task-stacks.md)
for task stack sizes, and
[ADR-0033](https://github.com/johnolafenwa/Aura/blob/main/architecture_docs/decisions/0033-structural-transfer-and-task-results.md)
for Transfer, task results, and Queue payloads.
