# Current Language Surface

This chapter is a reference for the part of Aura that the compiler supports
today. Learn the language from the earlier chapters, then use this page to
check whether a form, type, or library call is available.

## Top-Level Items

A file can declare these items at the top level:

| Kind | Forms |
|------|-------|
| Imports | `import module`, `import module as alias`, `from module import name` |
| Module constants | immutable complete-value constants such as `limit: int64 = 10` |
| Classes | `class`, `copy class`, `public class`, `public copy class` |
| Enums | `enum`, `public enum` |
| Functions | `def`, `public def` |
| Traits | `trait`, `public trait` |
| Implementations | `impl Trait for Type` |
| Foreign declarations | authorized `extern "C" def` and `extern "C" opaque class` |

A script-style file may also contain top-level executable statements.

## Entry Styles

A program starts in one of two ways:

- a top-level script
- an explicit `main`

Do not mix top-level executable statements with `main` in the same file.

## Types

### Scalar And Utility Types

| Group | Types |
|-------|-------|
| Boolean | `bool` |
| Signed integers | `int8`, `int16`, `int32`, `int64`, `int128`, `intsize`, and `int`, an alias for `int64` |
| Unsigned integers | `uint8`, `uint16`, `uint32`, `uint64`, `uint128`, `uintsize` |
| Floating point | `float32`, `float64` |
| Text | `str`, including in shared type positions |
| Other values | `None`, `Duration`, `Range` |
| `io` and `fs` | `io.Error`, `fs.File` |
| `net` | `net.TcpListener`, `net.TcpStream`, `net.UdpSocket`, `net.UdpDatagram`, `net.HttpListener`, `net.HttpExchange`, `net.HttpResponse`, `net.WebSocketListener`, `net.WebSocket`, `net.UnixListener`, `net.UnixStream`, `net.TlsListener`, `net.TlsStream` |
| `process` | `process.Child`, `process.Pipe`, `process.Completed`, `process.Supervisor`, `process.ExitStatus`, `process.Wait`, `process.Stdio`, `process.Error`, `process.RestartPolicy`, `process.SupervisorEvent`, `process.SupervisorWait` |
| `random`, `bytes`, `json` | `random.Rng`, `bytes.Error`, `json.Value`, `json.Error` |

### Generic And Runtime Types

- **Optional and result types:** optional unions `T | None`, `Lookup[T]`,
  `Poll[T]`, and `Result[T, E]`.
- **Collections:** `list[T]`, `dict[K, V]`, `set[T]`, and `Array[T]`. The
  `T` in `Array[T]` must be exactly `int32`, `int64`, `float32`, or
  `float64`.
- **Queues:** `Queue[T]`, `QueueReceive[T]`, and `SendError[T]`.
- **Tasks:** `Task[T]`, `TaskResult[T]`, `TaskGroup`, `SelectOutcome[Q, T]`,
  `WaitAny[T]`, and `WaitAll[T]`.

All of these built-in type names are reserved. You cannot reuse them for your
own classes, enums, or traits.

### Tuples

Tuple types are structural, such as `(str, int64)` or the singleton
`(bool,)`. A tuple is copyable exactly when every element is copyable.

### Numeric Literals

- A floating-point literal defaults to `float64`. It adopts an expected
  `float32` type from an annotation, parameter, return type, or class field.
- An unsuffixed integer literal defaults to `int64`. An expected integer type
  takes precedence, so fixed `int32` APIs and annotations stay `int32`.
- An integer literal can adopt an expected `float32` or `float64` type when
  its value is exactly representable there. This never converts an
  already-bound integer variable.
- Integer literals cover the full `uint128` range when that type is expected.

### Function Types

A capture-free named function value has a type such as
`def(T1, mut T2, own T3) -> R`. These values are copy values and satisfy
`Transfer`, the property that lets a value move into a task. The
[Concurrency](#concurrency) section defines it.

You can store function values in bindings, parameters, fields, and
collections, or use them as `TaskGroup` targets. A bare parameter in a
function type is shared. Written or inferred `mut` and `own` modes are part of
the contract.

- `receiver.method` binds a closure over the receiver with the method's
  contract.
- `Class.method` on a non-generic class is a function value.

### Lambdas

An expression lambda is written `lambda parameters: expression`. It is
contextually typed: the expected `def(...) -> ...` type supplies the parameter
types and constrains the result. Without context, `lambda: expression` may
infer `def() -> R`.

With no capture list, a lambda snapshots Copy values and moves owned non-Copy
values when it is created. An explicit capture list is exhaustive:

| Capture | Meaning |
|---------|---------|
| `[value]` | shared loan |
| `[mut value]` | mutable loan |
| `[own value]` | by-value capture |

How often you can call a closure depends on its captures:

- A shared closure is repeatable.
- A mutable-loan closure is repeatable through a mutable local.
- A closure that consumes a non-Copy owned capture is single-use.

Loan closures are not `Transfer`. You cannot erase them through arbitrary
stored or parameter `def` types.

## Packages And Workspaces

Aura supports a local package system:

- `Aura.toml` package manifests with `[package]`
- package source roots under `src/`
- local path dependencies and git dependencies under `[dependencies]`
- workspace roots with `[workspace] members = [...]`
- package-aware `check`, `run`, `build`, `analyze`, and `complete`
- a local `Aura.lock` written at the package root or workspace root
- FFI v0 authorization, described under [Foreign Functions](#foreign-functions)

A package manifest looks like this:

```toml
[package]
name = "app"
version = "0.1.0"
edition = "2026"

[dependencies]
util = { path = "../util" }
jsonx = { git = "https://github.com/example/jsonx.git", branch = "main" }
```

A workspace manifest looks like this:

```toml
[workspace]
members = ["app", "util"]
```

### Dependency Rules

- Dependencies come from local paths or git.
- A dependency import starts with the package name, such as
  `import util.math`.
- Version-only registry dependencies like `util = "0.1.0"` are rejected with
  a clear diagnostic.
- A git dependency selects `rev`, `tag`, or `branch`. With no selector, it
  defaults to `branch = "main"`.
- Git dependencies are materialized from a local cache and pinned by exact
  revision in `Aura.lock`.
- `aura deps update` refreshes every branch, tag, and default-main git
  dependency for the current package or workspace.
- `aura deps update util` refreshes only the named git dependency.
- There are no registry, publish, or install flows.

### Foreign Functions

The foreign function interface (FFI) lets an authorized package declare
bodyless `extern "C"` functions. A package is authorized by
`[package] allow_ffi = true`. The root package's `[ffi] dependencies` report
must name every reachable FFI-enabled dependency exactly.

- Extern functions work over the fixed-width scalar set, temporary `str` and
  byte pointer-length views, and opaque handles.
- Extern functions are direct-call-only and resolve process-global symbols
  synchronously.
- An empty view passes `(NULL, 0)`.
- `mut list[uint8]` uses a same-length scratch copy-in and copy-out.
- Opaque handles are non-Copy, non-cloneable, and non-Transfer. Each one
  needs an explicit consuming native close or free call.

Callbacks, variadics, raw pointer arithmetic, returned views, nullable
handles, and explicit library loading are not implemented. See
[26-ffi.md](26-ffi.md).

## Ownership And Borrowing

Aura uses ownership instead of a garbage collector. See
[06-ownership-and-borrowing.md](06-ownership-and-borrowing.md) for the full
tutorial.

### Copy And Move Types

Assigning a copy type duplicates the value. Assigning a move type transfers
ownership.

| Kind | Types |
|------|-------|
| Copy | all numeric types, `bool`, `Duration`, `Queue[T]`, and `Task[T]` when `T` is copyable, a `Queue[...]` handle, or a recursively repeatable `Task[...]` handle |
| Move | `str`, `list[T]`, `dict[K, V]`, `set[T]`, `Array[T]`, `random.Rng`, `TaskGroup`, opaque FFI handles, ordinary user-defined classes, and `Task[T]` for a non-repeatable `T` |

A `copy class` declaration is allowed when all its fields are copy types.

### Capabilities

A bare form means shared access everywhere. `mut` means mutable access, and
`own` means ownership transfer. Each capability has exactly one spelling:

| Position | Shared | Mutable | Consuming |
|----------|--------|---------|-----------|
| Parameter | `value: T` | `value: mut T` | `value: own T` |
| Receiver | `self` | `mut self` | `own self` |
| Iteration | `for x in collection:` | `for x in mut collection:` | `for x in own collection:` |
| Matching | `match value:` | `match mut value:` | `match own value:` |

A bare parameter is shared for every type, including copy types. A `mut`
parameter is exclusive. Mutable iteration and mutable matching write changes
back to the source.

The checker enforces these rules:

- A mutable argument must be a mutable place.
- A `mut` argument cannot overlap other shared access to the same value.
- A call is rejected when a `mut` parameter participates and borrowed
  arguments overlap. This includes a `mut self` receiver that overlaps
  another borrowed argument in the same method call.
- You cannot move a non-copy field out of a shared value.

### Cloning

`.clone()` makes an explicit, independent copy. It is available when the move
type exposes clone and its stored values are clone-safe. `random.Rng` has no
public clone route, and neither does an ordinary value that contains one.

A generic body that clones infers clone-safety obligations for its unresolved
type parameters. These requirements propagate through generic calls,
imports, trait default and associated dispatch, operators, and `From`. The
checker then rejects an unsafe concrete `random.Rng` specialization with
`AU3007`. See [15-generics.md](15-generics.md) for an example.

Handles stop this propagation in two cases:

- Queue handles are clone barriers, because copying a handle does not observe
  its payload.
- An allowed Task-handle copy does not observe its payload either. `Task[T]`
  is not copyable when `T` carries a single-consumer result right.

## Statements

The compiler supports these statements:

- assignment, and compound assignment with `+=`, `-=`, `*=`, `**=`, `/=`,
  `%=`, `//=`, `&=`, `|=`, `^=`, `<<=`, and `>>=`
- recursive tuple unpacking, such as `name, count = record`
- local shared and mutable view bindings: `view name = place` and
  `view mut name = place`
- `return`
- `if`, `elif`, and `else`
- `while`
- `for value in range(n):` and `for value in jobs:`
- recursive tuple-target iteration, such as `for name, count in records:`
- `match`
- `with`
- `break`, `continue`, and `pass`
- `assert condition` and `assert condition, message`
- expression statements

### Assertions

- The condition must be exactly `bool`. The optional message must be `str`.
- A true assertion does not evaluate its message.
- A false assertion traps with `AU4001` at the `assert` keyword. The message
  is `assertion failed` or the exact message you supplied.
- Assertions are never stripped, in any build mode.

## Expressions

### Literals

- **Integers:** decimal, hexadecimal, binary, and octal literals, with digit
  separators.
- **Other scalars:** float, boolean, `None`, and duration literals.
- **Strings:** ordinary, triple-quoted, raw, and f-string literals. Ordinary
  strings accept matching single or double quotes with shared escapes.
  F-strings stay double-quoted as `f"..."`, but an interpolation may contain
  either quote form. Write strings as quoted literals. `str(...)` is not a
  constructor.
- **Tuples:** parenthesized values such as `(name, count)` and the singleton
  `(value,)`.
- **Collections:** list literals such as `[1, 2, 3]`, dictionary literals
  such as `{"aura": 1}`, and set literals such as `{1, 2, 3}`.

Empty collections need a type:

- An empty list literal needs an expected `list[T]` type, such as
  `values: list[int32] = []`. You can also write `list[int32]()`.
- An empty dictionary literal needs an expected `dict[K, V]` type, such as
  `counts: dict[str, int32] = {}`. You can also write `dict[K, V]()`.
- An empty set uses a typed constructor, such as `set[int32]()`.

### Operators

Aura has arithmetic, comparison, and boolean operators, plus the unary prefix
operators `-` and `not`. Builtin numeric operators follow these rules:

- **Bitwise:** integer `&`, `|`, `^`, `~`, `<<`, and `>>` preserve exact
  widths and need exact same-type operands. Left shift is checked.
- **Power:** `**` is right-associative and preserves exact numeric types. It
  checks integer overflow and the exponent domain.
- **Floor division:** `//` is builtin floor division for matching integer or
  floating types.
- **Division:** builtin integer `/` and `/=` are rejected. Floating `/` and
  `/=` are true division.
- **Remainder:** builtin `%` follows the divisor's sign for matching integer
  or floating types.
- **Duration:** checked `+`, `-`, `* int64` in either order, `// int64`, and
  full comparison.
- **Tuples:** same-type tuples support recursive `==` and `!=` when every
  element is equatable. Both operands are read and retained. Tuple ordering is
  rejected.

User types can overload `+`, binary `-`, `*`, `/`, `//`, `%`, unary `-`, and
`not` through operator traits. `//` uses `FloorDiv.floor_div` when no builtin
numeric or Duration rule applies. See [16-traits.md](16-traits.md).

### Numeric Conversions

- `expr as Type` is an explicit numeric cast. Integer casts are
  range-checked, and integer-to-float casts reject silent precision loss.
- Integer `.to_float() -> float64` uses nearest-even conversion and may
  round.
- `round(value)` preserves integer types. It rounds a float to `int64` with
  ties-to-even.
- `divmod(left, right)` returns the floor quotient and the divisor-signed
  remainder together.
- `print` renders `float32` and `float64` in shortest-roundtrip form. It
  keeps an integral `.0` and signed zero.

### Comparisons And Conditionals

- **Membership:** `value in container` and `value not in container` test
  `list[T]` and `set[T]` elements, `dict[K, V]` keys, and `str` substrings.
  Both operands are read, and neither is moved.
- **Comparison chains:** a chain such as `low <= value < high` evaluates each
  operand at most once. A false link short-circuits the rest. Equality,
  ordering, and membership share one precedence level.
- **Conditional expressions:** `value if condition else alternative`
  evaluates the condition once and then exactly one arm, lazily. The
  condition must be exactly `bool`. Both arms must have one static result
  type. This form has the lowest expression precedence and associates to the
  right.

### Calls And Constructors

- names, member access with `.`, and function and method calls
- explicit type arguments on call targets, such as `Box[int32](...)` and
  `Result[int32, str].Ok(...)`
- enum and built-in enum variant construction
- `try expr`
- contextually typed lambdas, such as `lambda value: value + 1`
- expression-form `match` in bindings, returns, and call arguments
- parenthesized expressions and tuple values

Built-in enum variants follow a few extra rules:

- You may call a variant by its bare built-in name when an expected type is
  available, for example `ok: Result[int32, str] = Ok(7)`.
- A bare value or `None` at a `T | None` destination is injected into that
  union. An unannotated `None` is the unit value.
- `Lookup.Missing` and `Poll.Unavailable` carry no payload, so they need an
  expected type or explicit specialization.

### Comprehensions

List, set, and dictionary comprehensions are eager and build fresh owned
collections, as in `[value * 2 for value in values if value > 0]`.

- Nested clauses run outer-major.
- Targets do not leak out of the comprehension.
- Every clause follows the bare-loop contract, including the Queue exception
  that receives owned items.
- Clauses do not accept `mut` or `own`. Use a statement loop for mutable or
  consuming traversal.

Parenthesized generator expressions are not implemented. They report
`AU2005`, with guidance to use an eager owned list comprehension or an
explicit loop.

### Loop Helpers

`enumerate(seq)` yields `(int64, element)`. `zip(first, second)` stops at the
shorter sequence. Both take `list[T]` or `set[T]` operands with the shared
bare-loop default. Both are legal only as a `for` iterable.

### Indexing And Views

Indexing uses `expr[index]`. A copy-typed element read such as `values[idx]`
works directly. An owned read of a non-copy indexed value is rejected. Use one
of these instead:

- `view item = values[idx]` and `view entry = table[key]` bind an element or
  entry in place, including non-copy values.
- `view mut` gives write-through access from a mutable source.
- `get(index)` gives an explicit cloned read of a clone-safe non-copy list
  element.
- `pop(index)` transfers ownership of any element, including one that carries
  `random.Rng` state.

A negative list index normalizes once as `len + index`. This applies to
direct reads and writes and to every list index method, including `get`,
`set`, `pop`, and `swap`.

Dictionary indexing, including interpolations such as `f"{counts['key']}"`,
works when the value type is copy. For a clone-safe non-copy value,
`get(key)` gives a cloned optional read. `remove(key)` transfers any stored
value.

### Slices

`expr[start:end]`, `expr[:end]`, `expr[start:]`, and `expr[:]` slice a list or
a `str`. A slice is always a fresh owned copy.

- Written endpoints use the `int64` position domain. Negative endpoints
  normalize once.
- Both effective endpoints must be in `0..=len`, and start must not exceed
  end. Invalid bounds trap with `AU4003`. Aura never clamps them.
- String positions count Unicode scalar values, which takes an O(n) scan.
- List slicing needs clone-safe, repeatably observable elements.

Integer `str` indexing, step syntax, slice assignment, and indexed slice views
are not available. To access an addressable root, field, fixed tuple
position, list element, or dictionary entry in place, use a `view` binding.

### Line Continuation

A newline continues the expression while `(`, `[`, or `{` is still open.

- Continuation indentation is visual and does not open a block.
- Ordinary comma-separated forms still reject trailing commas. A singleton
  tuple needs its one comma.
- A backslash or physical newline inside an ordinary string or f-string does
  not continue the source.

### Numeric Arrays

A numeric `Array[T]` has rank one or more, may have zero-sized dimensions,
and uses contiguous row-major storage.

- **Construction:** `zeros(shape)`, `full(shape, value)`, and
  `from_list(values, shape)`. `from_list` copies its shared list input.
- **Members:** `shape`, `len`, `clone`, `get`, mutable `set`, mutable `fill`,
  `map[U]`, `sum`, `min`, `max`, and `mean`.
- **Indexing:** comma-separated `int64` coordinates such as
  `matrix[row, column]`, including indexed assignment.
- **Slicing:** the one-colon slice forms work on the first axis. The result
  is a fresh Array. Indexed Array views are not available.
- **Arithmetic:** Array-with-Array arithmetic needs exact shapes and dtypes.
  Same-dtype scalar arithmetic supports `+`, `-`, and `*` in either operand
  order. `/` is float-only. Integer Arrays also have wrapping and saturating
  add, subtract, and multiply methods.

Array operations trap with these codes:

| Code | Cause |
|------|-------|
| `AU4007` | `min`, `max`, or `mean` on an empty Array |
| `AU4003` | a coordinate or slice bound out of range |
| `AU4005` | shape-product overflow or allocation failure |

Arrays have no shape broadcasting, mixed-dtype promotion, equality, reshape,
transpose, view, step, slice assignment, or accelerator placement.

## Methods

A class method takes one of these receivers:

- `self` for shared access
- `mut self` for mutable access
- `own self` to consume the receiver
- no receiver, for an associated method

`self: Type` is not a receiver declaration. The checker rejects it with
guidance that names the valid forms.

Ordinary functions, instance methods, and associated methods support:

- positional calls
- named arguments
- mixed calls, with positional arguments first and named arguments after
- default parameter values, on ordinary functions and class methods
- bare, `own`, and `mut` parameters

The builtins `print(value=...)`, `range(...)`, `wait_any(...)`, and
`wait_all(...)` also take named arguments.

A bare parameter grants logical shared access for every type. That choice
stays fixed after generic specialization.

### Generic Declarations

Top-level declarations may be generic:

- `class Box[T]: ...`
- `class Box[T: Trait]: ...`
- `enum Wrapper[T]: ...`
- `enum Wrapper[T: Trait]: ...`
- `def identity[T](value: own T) -> T: ...`
- `trait Child: Parent: ...`
- `trait Child[T]: Parent[T]: ...`

Generic functions and methods may use inline trait bounds:

- `def speak[T: Greeter](value: T): ...`
- `def use_both[T: A + B](value: T) -> int32: ...`
- `def apply[T: Mapper[int32]](mapper: T, value: int32) -> int32: ...`

See [15-generics.md](15-generics.md) and [16-traits.md](16-traits.md).

### Return Values

An ordinary function returns an owned value. A copy result is an ordinary
copy. A non-copy result must be constructed, cloned, moved from owned input,
or obtained through an owner operation.

A function declared with `-> view [mut] T from origin` instead returns
non-owning shared or mutable access to that receiver or parameter. The caller
can use the result in three ways:

- bind it with a matching local view-binding statement
- read a shared result directly in one expression
- immediately reborrow a mutable result into a `mut` call

## Builtins

### Builtin Functions

| Group | Functions |
|-------|-----------|
| Output and sequences | `print`, `range`, `len`, `str` |
| Tasks and time | `cancelled`, `yield_now`, `sleep`, `select`, `wait_any`, `wait_all` |
| Math | `abs`, `min`, `max`, `sqrt`, `round`, `divmod` |
| Parsing | `parse_int32`, `parse_int64`, `parse_float64` |

These names are reserved. A module-level function that redefines one, such
as `len`, `str`, `abs`, or `print`, is rejected with `AU2007`.

`range(...)` accepts `range(stop)` and `range(start, stop)`, plus the matching
named-argument forms. Its bounds must fit the compiler's signed index space.

`len(value)` delegates to the matching `len()` method, so
`len(value) == value.len()`.

`int64` is the position type. Range bounds, yielded range values, list
indexes, slice endpoints, enumeration positions, and Array coordinates all use
`int64`. Narrower fixed-width integer values widen losslessly only at those
positions.

### Builtin Modules

`io`, `fs`, `net`, `process`, `random`, `sys`, `path`, `bytes`, `json`,
`toml`, `log`, `metrics`, `trace`, and `control`.

### JSON

- `json.parse(...) -> Result[json.Value, json.Error]`
- `json.dumps(..., indent=None) -> str`
- exact inspecting accessors `is_null`, `as_bool`, `as_int`, and `as_float`
- consuming accessors `into_string`, `into_array`, and `into_object`
- recursive Null, Boolean, Int, Float, str, Array, and Object variants
- deterministic sorted-key output, compact or pretty
- typed parse failures, with fixed depth and byte limits

### Bytes, Text Codecs, And Hashes

Bytes are represented as `list[uint8]`.

- `str.to_bytes()` and `str.from_bytes(...)` convert strict UTF-8.
- `bytes.hex_encode(...)` writes lowercase hex. `bytes.hex_decode(...)` is
  strict and accepts mixed case.
- `bytes.base64_encode(...)` and `bytes.base64_decode(...)` use the canonical
  standard alphabet.
- `bytes.sha256(...)` and `bytes.sha256_string(...)` return the raw 32-byte
  digest.
- Malformed input returns a typed `bytes.Error` variant that keeps `int32`
  offsets and lengths. Required metadata above `2147483647` traps with
  `AU4005`. It is never truncated or wrapped.
- Each fresh codec destination has a fixed 2,147,483,647-byte safety
  ceiling, independent of the public `str` and `list` length domains.
  Crossing it, overflowing the destination-size arithmetic, or failing to
  allocate traps with `AU4005`.

### Retry

`control.retry(worker, max_attempts=3, initial_backoff=0ms)` runs a
`def() -> Result[T, E]` worker immediately.

- It retries every `Err` with doubling delays and skips zero sleeps.
- It returns the exact final error, with no wait or multiply after the last
  attempt.
- Before the worker runs, it checks for a positive attempt budget and a
  non-negative, host-representable backoff.
- Traps and cancellation propagate.

### Files, Networking, And Processes

| API | Members |
|-----|---------|
| `io` | `io.write(...)`, `io.flush()`, `io.read_line()` |
| `fs` | `fs.exists(...)`, `fs.read_to_string(...)`, `fs.read_bytes(...)`, `fs.write_string(...)`, `fs.write_bytes(...)`, `fs.append_string(...)`, `fs.append_bytes(...)`, `fs.create_dir(...)`, `fs.read_dir(...)`, `fs.remove_file(...)`, `fs.open(...)`, `fs.create(...)`, `fs.append(...)` |
| `fs.File` | `read_all()`, `read_bytes()`, `write_all(...)`, `write_bytes(...)`, `flush()`, `close()` |
| `net` | `net.connect(...)`, `net.connect_timeout(...)`, `net.listen(...)`, `net.udp_bind(...)`, `net.http_listen(...)`, `net.http_request_text(...)`, `net.http_request_text_timeout(...)`, `net.http_request_bytes(...)`, `net.http_request_bytes_timeout(...)`, `net.websocket_listen(...)`, `net.websocket_connect(...)`, `net.websocket_connect_timeout(...)`, `net.unix_listen(...)`, `net.unix_connect(...)`, `net.unix_connect_timeout(...)`, `net.tls_listen(...)`, `net.tls_connect(...)`, `net.tls_connect_timeout(...)` |
| `net.TcpListener` | `accept(timeout=...)`, `local_addr()`, `close()` |
| `net.TcpStream` | `read_all(timeout=...)`, `read_line(timeout=...)`, `read_bytes(...)`, `read_exact(...)`, `write_all(...)`, `write_bytes(...)`, `flush()`, `local_addr()`, `peer_addr()`, `shutdown_read()`, `shutdown_write()`, `shutdown_both()`, `close()` |
| `net.UdpSocket` | `send_text(...)`, `send_bytes(...)`, `recv(...)`, `recv_from(...)`, `local_addr()`, `peer_addr()`, `close()` |
| `net.UdpDatagram` | `address()`, `bytes()`, `text()` |
| `net.HttpListener` | `accept(timeout=...)`, `local_addr()`, `close()` |
| `net.HttpExchange` | `method()`, `path()`, `headers()`, `body_text()`, `body_bytes()`, `respond_text(...)`, `respond_bytes(...)` |
| `net.HttpResponse` | `status()`, `reason()`, `headers()`, `text()`, `bytes()` |
| `net.WebSocketListener` | `accept(timeout=...)`, `local_addr()` |
| `net.WebSocket` | `send_text(...)`, `send_bytes(...)`, `recv_text(...)`, `recv_bytes(...)`, `close()` |
| `net.UnixListener` | `accept(timeout=...)`, `close()` |
| `net.UnixStream` | `read_line(timeout=...)`, `read_exact(...)`, `write_all(...)`, `close()` |
| `net.TlsListener` | `accept(timeout=...)`, `local_addr()`, `close()` |
| `net.TlsStream` | `read_line(timeout=...)`, `read_exact(...)`, `write_all(...)`, `close()` |
| `process` | `process.start(...)`, `process.run(...)`, `process.supervisor()`, `process.inherit()`, `process.null()`, `process.pipe()` |
| `process.Child` | `stdin()`, `stdout()`, `stderr()`, `wait(timeout=...)`, `wait_or_none(timeout=...)`, `wait_ok(timeout=...)`, `kill()`, `terminate()`, `close()` |
| `process.Pipe` | `read_all()`, `read_line(timeout=...)`, `read_bytes(...)`, `write_all(...)`, `write_bytes(...)`, `flush()`, `close()` |
| `process.Completed` | `status()`, `success()`, `stdout()`, `stderr()`, `stdout_bytes()`, `stderr_bytes()`, `check()` |
| `process.Supervisor` | `start(...)`, `wait(timeout=...)`, `wait_or_none(timeout=...)`, `stop()`, `is_empty()`, `close()` |

`process.Completed.stdout()` and `stderr()` return UTF-8 text.

`process.start(...)`, `process.run(...)`, and `process.Supervisor.start(...)`
accept `group=true`. It places the child in its own process group and makes
lifecycle cleanup group-aware on maintained Unix hosts.

Reads have fixed size caps:

| Read | Cap |
|------|-----|
| one-shot and `fs.File` whole-file reads, in both `aura run` and built binaries | 256 MiB of remaining content |
| process capture and pipe reads, and TCP, Unix, and TLS whole or bounded reads | 64 MiB |
| TLS certificate, private-key, and CA-file loading | a separate 64 MiB ceiling |
| incoming HTTP parsing | 16 MiB of wire data per message |

Aura 0.3 has no chunked file-read API.

### Member Methods

| Type | Methods |
|------|---------|
| scalars | `float64.sqrt()`, and `.to_string()` on scalar and boolean values |
| `str` | `len() -> int64`, `byte_len() -> int64`, `to_bytes()`, `from_bytes(...)`, `contains(...)`, `starts_with(...)`, `ends_with(...)`, `split(...)`, `join(...)`, `replace(...)`, `to_lower()`, `to_upper()`, `strip_prefix(...)`, `strip_suffix(...)`, `trim()`, `clone()` |
| `list` | `len() -> int64`, `is_empty()`, `copy()`, `append(...)`, `pop(index=-1)`, `get(...)`, `insert(...)`, `set(...)`, `remove(...)`, `index(...)`, `count(...)`, `swap(...)`, `extend(...)`, `clear()`, `reverse()`, `sort()`, `sort(reverse=...)`, `sort(key=..., reverse=...)`, `map(f)`, `filter(f)`, `reserve(...)`, `with_capacity(...)` |
| `dict` | `len() -> int64`, `is_empty()`, `copy()`, `get(...)`, `remove(...)`, `keys()`, `values()`, `items()`, `clear()`, `update(...)`, `reserve(...)`, `with_capacity(...)` |
| `set` | `len() -> int64`, `is_empty()`, `copy()`, `add(...)`, `remove(...)`, `discard(...)`, `clear()`, `reserve(...)`, `with_capacity(...)` |
| `Queue` | `put(...)`, `try_put(...)`, `get(...)`, `poll(...)`, `get_or(...)`, `close()` |
| `Task` | `result(timeout=...)`, `poll(timeout=...)`, `result_or(timeout=...)` |
| `TaskGroup` | `start(...)`, `start_soon(...)`, `start_with_stack(...)`, `start_soon_with_stack(...)`, `cancel()` |
| `random.Rng` | `next_int(...)`, `next_float()`, `shuffle(...)` |

String members:

- `str.len()` counts Unicode scalar values in O(n).
- `str.byte_len()` counts UTF-8 bytes in O(1).
- `str.to_bytes()` returns a fresh `list[uint8]`.
- `str.from_bytes(...)` is an associated strict UTF-8 conversion.

### Lists

- Bare list iteration is shared. `for value in own values:` consumes the
  list. `for value in mut values:` writes back, and needs the iterable place
  itself to be mutable.
- Direct indexed reads need a copy `T`. See [Indexing And Views](#indexing-and-views)
  for the alternatives.
- `get` returns `Lookup.Missing` when the normalized index is invalid. Direct
  access and the mutating methods trap instead.
- `list.set(index, value)`, `list.pop(index)`, and
  `list.swap(first, second)` trap on out-of-bounds indices.
- `list.remove(value)` traps with `AU4008` when no equal value exists.
- `insert(-1, value)` inserts before the last element.
  `insert(values.len(), value)` appends. Positions outside the range clamp to
  the nearest boundary.
- Two lists of the same `list[T]` type support `==` and `!=` when `T` defines
  equality. `remove`, `index`, `count`, membership, set insertion, and
  dictionary-key operations have the same requirement. A type that fails it
  is rejected with `AU2008`.

Sorting:

- `list.sort()` and `list.sort(key=callback)` are stable, in-place
  mutations.
- Keyed sorting evaluates one shared key per element, from left to right,
  before it mutates anything. A key that traps leaves the source unchanged.
- Built-in ordering covers all integer types, `float32`, `float64`, and
  `Duration`.
- `str` has no built-in `Ord[str]`. To order strings, keep insertion order,
  use `sort(key=callback)` with an orderable key or index, or define a
  nominal application type with the `Ord` behavior you need.

Callbacks:

- `list.map(f)` and `list.filter(f)` are eager shared traversals. They keep
  the source and return fresh owned lists.
- `filter` needs a clone-safe `T`.
- List algorithm callbacks take exact bare, shared element parameters. `mut`
  and `own` callback capabilities are rejected without adaptation.

### Dictionaries And Sets

- `dict[K, V]` supports literal construction, indexed writes for every `V`,
  and direct indexed reads under the value ownership rule.
- `dict.items()` returns `list[(K, V)]` in insertion order.
- `set[T]` supports literal construction with `{...}` and membership with
  `in`.
- Bare set iteration is shared, and `for value in own set:` consumes the set.
  `for value in mut set:` is not supported.

## Randomness

Import `random` for two separate sources of randomness. See
[20-randomness.md](20-randomness.md) for the tutorial.

`random.Rng(seed)` is a mutable, deterministic, move-only xoshiro256**
stream.

- `next_int` draws from a half-open range.
- `next_float` draws from `[0.0, 1.0)`.
- `shuffle` shuffles a generic list in place.
- Seed mapping and sequences are stable throughout Aura 0.3.x. They are
  identical under MIR execution and direct execution. MIR is the compiler's
  mid-level intermediate representation.

`random.secure_int(lo, hi)` and `random.secure_bytes(n)` use only the host
operating system's secure source. They take no seed and never fall back to
the deterministic generator.

- `secure_bytes(0)` returns an empty list without requesting entropy.
- The count is `int64`. Each request has a fixed resource and safety ceiling
  of `2147483647`, independent of the public `list` length domain.
- There is no `random.Error` and no secure floating-point function.

Secure calls trap with these codes:

| Code | Cause |
|------|-------|
| `AU4003` | invalid bounds or a negative count |
| `AU4005` | a count above the ceiling, checked before allocation or entropy |
| `AU4005` | entropy or allocation failure |

## Pattern Matching

The compiler supports these patterns:

- `Enum.Variant` and `Enum.Variant(name)`
- multi-payload enum variants, including named payload fields
- unqualified variants such as `Ok(value)` and `None` when the scrutinee type
  is known
- literal patterns over `bool`, integer, `str`, and floating-point values
- top-level complete-value bindings, such as `case value:` and
  `case value if condition:`
- the wildcard `case _:`
- nested enum patterns

`match value:` matches shared and `match mut value:` matches mutably.
Statement-form `match` must be exhaustive. Expression-form `match` works in
return, binding, and argument positions, and its arms may evaluate nested
block-form expressions.

A boolean literal match is exhaustive when it covers both `true` and `false`.
An integer or `str` literal match needs a final wildcard arm.

## Concurrency

The concurrency surface includes:

- typed queues, and `for` iteration over a queue until it closes
- task groups, with `TaskGroup.start(...)`, `TaskGroup.start_soon(...)`,
  `TaskGroup.start_with_stack(bytes, ...)`, and
  `TaskGroup.start_soon_with_stack(bytes, ...)`
- `Task.result(timeout=...)`
- typed `select(queue_or_task_or_duration, ...)`
- `wait_any(...)` and `wait_all(...)`
- cooperative cancellation
- signed i128-nanosecond `Duration` values, with `ms`, `s`, and `m`
  literals, integer constructors, checked arithmetic, conversions, and
  comparisons

### Scheduler

Aura 0.3 runs task bodies on cooperative, pinned scheduler workers on both
maintained backends. Aura tasks are lightweight tasks backed by this
scheduler.

- The default worker count is the available parallelism the host reports.
  The provisional `AURA_WORKERS=<positive integer>` override sets an explicit
  count.
- Each child task gets a stable worker assignment when it spawns.
- Coroutine stacks never migrate, and work is not stolen.
- `yield_now()` yields only to runnable work on the local worker.
- Queue waits, `sleep(...)`, socket waits, and the maintained HTTP helpers
  all use this evented scheduler.
- Ordinary file I/O offloads through the runtime, so a task does not hold a
  blocking host thread.
- Scheduler waits use persistent descriptor registrations, a timer heap, and
  direct Queue, task-completion, and blocking-pool notifications. An idle
  scheduler blocks until an event or deadline, with no periodic tick.

The compiler inserts a scheduling check on every loop backedge, including the
ordinary body tail and `continue`. `break` and `return` bypass it. So a tight
loop still lets ready timers, queues, or sockets on the same worker proceed.
A single long loop body can still delay siblings on the same worker. The
check does not inspect cancellation.

### Task Stacks

An ordinary task requests a guarded 768 KiB coroutine stack. The two
explicit stack-start methods accept an exact `int64` byte request from
256 KiB through 64 MiB inclusive. They reject out-of-range values without
clamping and page-round accepted requests.

The 256 KiB lower bound is for measured shallow tasks. It is not a generally
safe default. The complete compiled Aura HTTP example faulted at 256 KiB and
succeeds with the 768 KiB default. An isolated runtime round trip can use
256 KiB protocol callers. It excludes the compiled program's
language-execution frames and keeps deep host protocol frames on service
workers.

### Task Starts And Transfer

`TaskGroup.start(...)`, `TaskGroup.start_soon(...)`, and their explicit-stack
variants accept these targets:

- capture-free function values
- `Transfer` closure values
- direct named functions
- associated methods without `self`

A task start moves or copies its arguments into task-owned capture storage.
The target's parameters may be bare shared or `own`. `mut` targets are
rejected.

Every captured argument and the target's result must be structurally
`Transfer` after generic specialization.

- **Transfer:** copy values, `str`, recursively transferable collections,
  tuples, classes, enums, and Queue and Task handle identities.
- **Not Transfer:** shared or mutable access, `random.Rng`, `TaskGroup`, and
  live filesystem, process, pipe, supervisor, listener, socket, stream,
  HTTP-exchange, WebSocket, and TLS resources.

The compiler derives `Transfer`. There is no builtin user trait or escape
hatch, and an ordinary trait with the same name cannot confer the property. A
Copy value read through access becomes an owned snapshot and may cross. Non-copy
access cannot.

Queue and Task handle state is synchronized across workers. All other task
captures and results stay owned `Transfer` data, so tasks share nothing else.
Cancellation and diagnostic context are per task. Task scheduling,
independent completion, and output order are unspecified. Aura has no
worker-introspection API and no work stealing, so parallel speedup depends on
the workload.

A `with TaskGroup()` scope waits for its started tasks when it exits and
surfaces every unread task failure. `group.cancel()` wakes `for value in queue:`
iteration over a `Queue[T]` in the same scope, so the loop can exit cleanly.

### Queues

| Call | Result |
|------|--------|
| `Queue[T](capacity=...)` | a bounded-capacity queue |
| `put(...)` | `Result[None, SendError[T]]`. `SendError[T]` has `Closed(value)`, `Cancelled(value)`, `TimedOut(value)`, and `Full(value)`. |
| `get(timeout=...)` | `QueueReceive[T]`: `Item(value)`, `Closed`, `TimedOut`, or `Cancelled` |
| `poll(timeout=...)` | `Poll[T]`: `Ready(value)` for an item, or `Unavailable` when the wait closed, timed out, or was cancelled |
| `get_or(default, timeout=...)` | the queued value, or the caller's fallback |

- Construction, `put`, and `try_put` need a structurally `Transfer` payload
  type.
- Without a timeout, `poll` makes an immediate non-blocking check, and
  `get_or` returns the fallback immediately when no item is ready.
- Queue iteration receives owned items. Only bare `for value in queue:` is
  allowed. The `own` and `mut` modifiers are rejected.

### Task Results

A task result is repeatable only when `T` is a copy type, a `Queue[...]`, or a
recursively repeatable `Task[...]`. `Task[T]` is always transferable, but it
is copyable only for these repeatable results.

For every other transferable `T`, the handle is single-consumer:

- `result`, `poll`, and `result_or` consume the handle on the first attempt.
  This holds for every outcome, including timeout, cancellation, failure,
  fallback, and `Unavailable`.
- `wait_any` and `wait_all` consume the complete task list. `wait_any`
  abandons the observation rights it does not choose.

| Call | Result |
|------|--------|
| `Task.result(timeout=...)` | `TaskResult[T]`: `Ready(value)`, `Error(message)`, `TimedOut`, or `Cancelled` |
| `Task.poll(timeout=...)` | `Poll[T]`: `Ready(value)` for a result, or `Unavailable` when the task failed, timed out, or was cancelled |
| `Task.result_or(default, timeout=...)` | the task result, or the caller's fallback when the task fails, times out, or is cancelled |
| `wait_any(...)` | `WaitAny[T]`: `Ready(index, value)`, `Error(index, message)`, `TimedOut`, or `Cancelled` |
| `wait_all(...)` | `WaitAll[T]`: `Ready(results)`, `Error(index, message)`, `TimedOut`, or `Cancelled` |

Without a timeout, `Task.poll` makes an immediate non-blocking check, and
`Task.result_or` returns the fallback immediately when the task is not ready.
`wait_any([])` returns `TimedOut` immediately.

Handle errors use these codes:

| Code | Cause |
|------|-------|
| `AU3008` | a value that cannot cross the task boundary |
| `AU3009` | an attempt to duplicate a single-consumer right |
| `AU3001` | a moved value: using a directly observed handle again |

### Select

`select(...)` takes one or more positional Queue, Task, and relative-Duration
sources and returns `SelectOutcome[Q, T]`.

- All Queue payloads share `Q`, and all Task results share `T`. A missing
  category uses `None`.
- Source expressions run once, from left to right.
- Current-task cancellation wins. Otherwise, among ready sources, the lowest
  original argument index wins.
- Every non-repeatable Task right is consumed at entry, and a losing right is
  abandoned.
- `select` is an ordinary builtin call.

### Protocol Service

Deep HTTP, TLS, and maintained Unix WebSocket operations run on a separate,
bounded protocol-step service with deep native worker stacks. The service
starts lazily and stays alive until the process exits. It has no public
shutdown or join surface.

File reads, resolver work, and listener binding use the generic blocking-I/O
pool. For TLS assets, only the later PEM parsing and rustls construction use
protocol workers.

### Blocking-I/O Pool

| Setting | Effect | Default |
|---------|--------|---------|
| `AURA_BLOCKING_WORKERS=<positive integer>` | an exact, unclamped worker count | `2..=8` workers derived from host parallelism, with fallback `4` |
| `AURA_BLOCKING_QUEUE_CAPACITY=<positive integer>` | bounds accepted pending jobs only | an unbounded queue |

- Full-queue admission is FIFO and scheduler-aware.
- Expiry or cancellation before queue insertion prevents submission.
- Accepted work still runs once, and any abandoned result is discarded.
- A bound limits the accepted pending backlog, not admission waiters. It
  cannot guarantee unrelated blocking-I/O work while every worker stays
  stuck.

### Measured Capacity

The maintained memory-capacity bound is 10,000 sleepers within 512 MiB of
whole-process resident set size (RSS). In a clean Mac14,9 report, three
10,000-sleeper runs peaked at 207,798,272, 206,946,304, and 206,831,616
bytes. Standalone 1,000-timer controls passed with a 6 ms maximum arm span
and 1 ms worst p99 overshoot.

The runtime accepts larger task counts. Three clean runs of 100,000 sleepers
plus 1,000 timers peaked at 1,170,735,104, 1,921,531,904, and 2,001,305,600
bytes. Two exceeded the proposed limit. Timer behavior stayed stable, at a
3 ms maximum arm span and 2 ms worst p99 overshoot. Mac14,9 uses 16 KiB
pages, so those 101,000 stackful tasks have a 1,654,784,000-byte one-page
floor before other runtime and process memory. An earlier below-gate sample
depended on nondeterministic memory compression.

The four-worker workload passes at a `1.039673x` paired median wall-time
ratio, with `396.73%` median four-task process CPU.

## Tooling

The CLI commands are `check`, `run`, `build`, `ast`, `ast-json`, `mir`,
`analyze`, `complete`, `lsp`, `new`, `fmt`, `test`, `deps update`, `upgrade`,
`help`, and `version`.

- `build` accepts `--backend auto|direct`. `auto` is the default.
- `direct` covers the full implemented Aura language surface.
- Compiler-backed editor state is invalidated across open documents when
  imported files change.
- `file://` URI handling preserves both Windows drive-letter paths and UNC
  workspaces.

The VS Code tooling is compiler-backed for diagnostics, document symbols,
hover, go-to-definition, and completions.

## Current Boundaries

The compiler does not support:

- non-numeric casts
- direct recursive fields without `indirect`
- statement-bodied closures
- implicit shared parameter captures. For mutable captured state, store an
  explicit `lambda [mut place] ...` loan capture in a mutable local.
- PTY support for subprocesses

Concurrency uses only the maintained surface: `Queue[T]()`, `Task.result()`,
`TaskGroup()` and its four start methods, `yield_now()`, `wait_any(...)`, and
`wait_all(...)`.

Modules and imports:

- Imports resolve local `.au` files relative to the current package root.
- Checking or analyzing a nested package file directly infers the nearest
  package root that satisfies its imports.
- `import a.b` exposes module namespaces for calls like `a.b.func(...)`,
  `a.b.Type(...)`, and `a.b.Enum.Variant`.
- Type annotations may use namespace-imported types such as `a.b.Type`.

Runtime:

- Both maintained execution paths stop with a friendly recursion-depth
  diagnostic after 256 nested Aura calls.
- MIR and direct-native runtime failures keep matching typed Aura call frames
  and child-task ancestry. JSON tooling receives them as the always-present
  `call_frames` and `task_ancestry` arrays.
- Unix domain sockets need a Unix host at runtime.
- Subprocess APIs are shell-free and take explicit argv vectors. Process
  groups and restart supervision are implemented.
