# Current Language Surface

This chapter is a compact reference for the language subset that the bootstrap compiler supports today.

It is intentionally implementation-facing. Use the earlier chapters to learn the language progressively, then use this chapter to check what is actually available right now.

## Top-Level Items

Aurora currently supports these top-level declarations:

- `public class`
- `public enum`
- `public def`
- `public trait`
- `public copy class`
- `class`
- `copy class`
- `enum`
- `def`
- `trait`
- `impl Trait for Type`

It also supports top-level executable statements for script-style files.

## Entry Styles

You can write either:

- a top-level script
- an explicit `main`

Do not mix top-level executable statements with `main` in the same file.

Floating-point literals default to `float64`, but they can adopt an expected `float32` type from an annotation, parameter, return type, or class field.

Unsuffixed integer literals default to `int64`, and `int` is an alias for `int64`. Expected integer types still take precedence, so fixed `int32` APIs and annotations remain `int32`. An integer literal can also adopt an expected `float32` or `float64` type when its value is exactly representable there; this never converts an already-bound integer variable. Integer literals support the full `uint128` range when that integer type is expected.

## Types

Builtin scalar and utility type names currently accepted by the compiler:

- `bool`
- `int` (an alias for `int64`)
- `int8`, `int16`, `int32`, `int64`, `int128`, `intsize`
- `uint8`, `uint16`, `uint32`, `uint64`, `uint128`, `uintsize`
- `float32`, `float64`
- `String`
- `str` in shared type positions
- `None`
- `Duration`
- `Range`
- `io.Error`
- `fs.File`
- `net.TcpListener`
- `net.TcpStream`
- `net.UdpSocket`
- `net.UdpDatagram`
- `net.HttpListener`
- `net.HttpExchange`
- `net.HttpResponse`
- `net.WebSocketListener`
- `net.WebSocket`
- `net.UnixListener`
- `net.UnixStream`
- `net.TlsListener`
- `net.TlsStream`
- `process.Child`
- `process.Pipe`
- `process.Completed`
- `process.Supervisor`
- `process.ExitStatus`
- `process.Wait`
- `process.Stdio`
- `process.Error`
- `process.RestartPolicy`
- `process.SupervisorEvent`
- `process.SupervisorWait`
- `random.Rng`
- `bytes.Error`
- `json.Value`
- `json.Error`

Builtin generic or runtime-facing types currently accepted:

- `Option[T]`
- `Result[T, E]`
- `SendError[T]`
- `Queue[T]`
- `Vec[T]`
- `Map[K, V]`
- `Set[T]`
- `MapEntry[K, V]`
- `Task[T]`
- `TaskGroup`

Structural tuple types such as `(String, int64)` and singleton `(bool,)` are
also accepted. A tuple is copyable exactly when every element is copyable.

Capture-free named function values use `def(T1, mut T2, own T3) -> R`. They are copy
values, satisfy `Transfer`, and may be stored in bindings, parameters, fields,
and collections or used as `TaskGroup` targets. Bare function-type parameters
are shared; written or inferred `mut`/`own` modes are part of the contract.
Instance, associated, and trait method values remain
outside the implemented surface.

Contextually typed expression lambdas use `lambda parameters: expression`.
The expected `def(...) -> ...` type supplies parameter types and constrains
the result; `lambda: expression` may infer `def() -> R` without context.
Captures are by value: Copy values are snapshotted and owned non-Copy values
move at creation. A read-only closure is repeatable, a closure that consumes a
non-Copy capture is single-use, and a closure is Transfer exactly when every
capture is Transfer. Captured environments are read-only in Phase 6.3 and
cannot be erased through arbitrary stored or parameter `def` types.

These built-in type names are reserved and cannot be reused for user-defined classes, enums, or traits.

## Packages And Workspaces

Aurora now supports a first local package-system milestone:

- `Aurora.toml` package manifests with `[package]`
- package source roots under `src/`
- local path dependencies under `[dependencies]`
- git dependencies under `[dependencies]`
- workspace roots with `[workspace] members = [...]`
- package-aware `check`, `run`, `build`, `analyze`, and `complete`
- a local `Aurora.lock` written at the package root or workspace root
- FFI v0 authorization through `[package] allow_ffi = true`, with every
  reachable FFI-enabled dependency named exactly in the root package's
  `[ffi] dependencies` report

Current manifest shape:

```toml
[package]
name = "app"
version = "0.1.0"
edition = "2026"

[dependencies]
util = { path = "../util" }
jsonx = { git = "https://github.com/example/jsonx.git", branch = "main" }
```

Current workspace shape:

```toml
[workspace]
members = ["app", "util"]
```

Current package-system limits:

- dependency imports may come from local path dependencies or git dependencies
- import roots for dependencies are package-name-prefixed, such as `import util.math`
- version-only registry dependencies like `util = "0.1.0"` are rejected with a clear diagnostic
- git dependencies support `rev`, `tag`, or `branch`, and default to `branch = "main"` when no selector is provided
- git dependencies are materialized from a local cache and pinned by exact revision in `Aurora.lock`
- `aura deps update` refreshes all branch/tag/default-main git dependencies for the current package or workspace
- `aura deps update util` refreshes just the named git dependency
- there are still no registry or publish/install flows yet

An authorized package may declare bodyless `extern "C"` functions over the
fixed-width scalar set, temporary String/byte pointer-length views, and opaque
handles. Extern functions are direct-call-only and resolve process-global
symbols synchronously. Empty views pass `(NULL, 0)`; `mut Vec[uint8]` uses
same-length scratch copy-in/out. Opaque handles are non-Copy, non-cloneable,
non-Transfer values and require an explicit consuming native close/free call.
Callbacks, variadics, raw pointer arithmetic, returned views, nullable handles,
and explicit library loading are not implemented. See
[26-ffi.md](26-ffi.md).

## Ownership And Borrowing

Aurora uses an ownership model with no garbage collector. See [06-ownership-and-borrowing.md](06-ownership-and-borrowing.md) for the full tutorial.

Copy types (all numeric types, `bool`, `Duration`, and `Queue[T]`) are
duplicated on assignment. `Task[T]` is copyable only when `T` is copyable, a
`Queue[...]` handle, or a recursively repeatable `Task[...]` handle. Move
types (`String`, `Vec[T]`, `Map[K, V]`, `Set[T]`, `random.Rng`, `TaskGroup`,
opaque FFI handles, ordinary user-defined classes, and `Task[T]` for a
non-repeatable `T`) transfer
ownership on assignment.

`copy class` declarations are allowed when all fields are copy types.

Capability forms. Bare means shared access everywhere, `mut` means mutable
access, and `own` means ownership transfer. There is one spelling per
capability:

- `value: T` -- shared parameter, for every type including copy types
- `value: mut T` -- exclusive, mutable parameter
- `value: own T` -- consuming parameter
- `self` -- shared receiver
- `mut self` -- mutable receiver
- `own self` -- consuming receiver
- `for x in collection:` -- shared collection iteration
- `for x in mut collection:` -- mutable iteration with writeback
- `for x in own collection:` -- consuming iteration
- `match value:` -- shared pattern matching
- `match mut value:` -- mutable pattern matching with writeback
- `match own value:` -- consuming pattern matching

The `borrow` keyword is retired and reserved. Writing it produces a diagnostic
naming the exact replacement. For example, a retired `borrow mut T` parameter
receives guidance to write `mut T`.

Mutable arguments must be mutable places. Overlapping `mut` arguments with
other shared access to the same value are rejected. Non-copy fields cannot be
moved out of a shared value.

`.clone()` produces an explicit independent copy when the move type exposes
clone and its stored values are clone-safe. `random.Rng`, and an ordinary value
that contains one, has no public clone route.

## Statements

The current compiler supports these statement forms:

- assignment and compound assignment through `+=`, `-=`, `*=`, `/=`, `%=`, and `//=`
- recursive tuple unpack assignment such as `name, count = record`
- `return`
- `if` / `elif` / `else`
- `while`
- `for value in range(n):`
- `for value in jobs:`
- recursive tuple-target iteration such as `for name, count in records:`
- `match`
- `with`
- `break`
- `continue`
- `pass`
- `assert condition` and `assert condition, message`
- expression statements

Assertion conditions must be exactly `bool`, and optional messages must be
`String`. A true assertion does not evaluate its message. A false assertion
traps with `AU4001` at the `assert` keyword, using `assertion failed` or the
exact supplied message. Assertions are not stripped in any build mode.

## Expressions

The current compiler supports these expression forms:

- names
- parenthesized tuple values such as `(name, count)` and singleton `(value,)`
- integer, float, string, f-string, boolean, `None`, and duration literals
  - ordinary strings accept matching single or double quotes with shared escapes
  - f-strings remain double-quoted as `f"..."`, while interpolations may contain either ordinary quote form
- arithmetic, comparison, and boolean operators
  - `//` is builtin floor division for matching integer or floating types
  - builtin integer `/` and `/=` are rejected; floating `/` and `/=` remain true division
  - builtin `%` follows the divisor's sign for matching integer or floating types
  - Duration supports checked `+`, `-`, `* int64` in either order, `// int64`, and full comparison
  - same-type tuples support recursive `==` and `!=` when every element is
    equatable; both operands are read and retained, while tuple ordering remains
    rejected
- unary prefix operators `-` and `not`
- operator-trait dispatch for `+`, binary `-`, `*`, `/`, `//`, `%`, unary `-`, and `not`
  - `//` uses `FloorDiv.floor_div` when no builtin numeric or Duration rule applies
- explicit numeric casts with `expr as Type`
  - integer casts are range-checked and integer-to-float casts reject silent precision loss
- integer `.to_float() -> float64`, which uses nearest-even conversion and may round
- shortest-roundtrip `float32`/`float64` rendering through `print`, preserving integral `.0` and signed zero
- list literals such as `[1, 2, 3]`
- map literals such as `{"aurora": 1}`
- set literals such as `{1, 2, 3}`
- eager owned list, set, and map comprehensions such as
  `[value * 2 for value in values if value > 0]`; nested clauses are
  outer-major, targets do not leak, and every clause uses the bare-loop
  contract (including Queue's receive-owned item carve-out)
- member access with `.`
- indexing with `expr[index]`
- owned Vec/String slicing with `expr[start:end]`, `expr[:end]`,
  `expr[start:]`, and `expr[:]`
- function and method calls
- explicit type arguments on call targets such as `Box[int32](...)` and `Result[int32, String].Ok(...)`
- enum and built-in enum variant construction
- `try expr`
- conditional expressions written `value if condition else alternative`; the
  condition must be exactly `bool`, is evaluated once, and selects exactly one
  lazily evaluated arm. Both arms must have one static result type. This form
  has the lowest expression precedence and associates to the right.
- `value in container` and `value not in container` over `Vec[T]` and `Set[T]`
  elements, `Map[K, V]` keys, and `String` substrings; both operands are read
  and neither is moved
- comparison chains such as `low <= value < high`, where equality, ordering,
  and membership share one precedence level, every operand is evaluated at most
  once, and a false link short-circuits the rest
- the compiler-known `for` iterable forms `enumerate(seq)`, yielding
  `(int64, element)`, and `zip(first, second)`, which stops at the shorter
  sequence; both take `Vec[T]` or `Set[T]` operands over the bare-loop shared
  default and are legal only as a `for` iterable
- the builtin functions `len(value)`, which delegates to the value's `len()`
  member and produces `int64`, and `str(value)`, which produces the same
  `String` that `print` writes; both names are reserved and cannot be redefined
- parenthesized expressions and tuple values
- delimiter-based newline continuation while `(`, `[`, or `{` remains open
  - continuation indentation is visual and does not create a block
  - ordinary comma-separated forms still reject trailing commas; singleton
    tuples require one comma
  - backslashes and physical newlines inside ordinary/f-strings do not
    continue source

Parenthesized generator expressions are not implemented. They report `AU2005`
with guidance to use an eager owned list comprehension or an explicit loop.
Comprehension clauses do not accept `mut` or `own`; use a statement loop for
mutable or consuming source traversal.

Indexed expressions remain ordinary values after parsing. Copy-typed element
reads like `values[idx]` still work directly, while clone-safe non-copy vector
elements such as `String` use `get(index)` for an explicit cloned read, and
elements carrying `random.Rng` state must use `remove(index)` because they
cannot be cloned at all. Negative Vec indexes normalize as `len + index` for
direct access and every maintained Vec index method. Map indexing and
interpolations such as `f"{counts['key']}"` remain supported when the Map
value type is copy; clone-safe non-copy values use `get(key)` for an explicit
cloned optional read, while `remove(key)` transfers any stored value.

One-colon Vec and String slices return fresh owned copies. Written endpoints
are exact `int32`, negatives normalize once, both effective endpoints must be
in `0..=len`, and start must not exceed end. Invalid bounds trap with `AU4003`
rather than clamping. String positions count Unicode scalar values and require
an O(n) scan. Integer String indexing, step syntax, slice assignment, and
views remain unavailable.

## Methods

Class methods currently support these receiver forms:

- `self` for shared access
- `self`
- `own self`
- `mut self`
- no receiver for associated methods

Bare `self` and `self` have the same shared semantics. `self: Type` is
not a receiver declaration and is rejected with guidance naming these forms.

Ordinary functions, instance methods, and associated methods support:

- positional calls
- named arguments
- mixed calls where positional arguments come first and named arguments come after
- default parameter values on ordinary functions and class methods
- ordinary bare, `own`, and `mut` parameters
- builtin named arguments for `print(value=...)`, `range(...)`, `wait_any(...)`, and `wait_all(...)`

Bare parameters grant logical shared access for every type, and that choice is
stable after specialization. Task starts move/copy arguments into task-owned
capture storage, then allow bare shared or `own` target parameters; `mut`
targets are rejected.
Calls also reject overlapping borrowed arguments whenever a `mut` parameter
participates, including a `mut self` receiver overlapping another borrowed
argument in the same method call.
Empty list literals currently require an expected `Vec[T]` type such as `values: Vec[int32] = []`, or you can use `Vec[int32]()` explicitly.
Empty map literals currently require an expected `Map[K, V]` type such as `counts: Map[String, int32] = {}`.
Empty set literals currently require an expected `Set[T]` type such as `seen: Set[int32] = {}`, or you can use `Set[int32]()` explicitly.

Top-level declarations may also be generic:

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

Built-in enum constructor notes:

- `Option.Some(...)` can infer `T` from its payload without a separate annotation
- `Option.None` still requires an expected `Option[T]` type

## Builtins

Current builtin functions:

- `print`
- `range`
- `cancelled`
- `yield_now`
- `sleep`
- `wait_any`
- `wait_all`
- `abs`
- `min`
- `max`
- `sqrt`
- `parse_int32`
- `parse_int64`
- `parse_float64`

Current builtin module namespaces:

- `io`
- `fs`
- `net`
- `process`
- `random`
- `sys`
- `path`
- `bytes`
- `json`
- `toml`
- `log`
- `metrics`
- `trace`
- `control`

Current builtin `range(...)` notes:

- supports `range(stop)` and `range(start, stop)`
- supports the matching named-argument forms
- currently requires bounds that fit the bootstrap compiler's signed index space

Current dynamic JSON surface:

- `json.parse(...) -> Result[json.Value, json.Error]`
- `json.dumps(..., indent=Option.None) -> String`
- exact inspecting accessors `is_null`, `as_bool`, `as_int`, and `as_float`
- consuming accessors `into_string`, `into_array`, and `into_object`
- recursive Null, Boolean, Int, Float, String, Array, and Object variants
- deterministic sorted-key compact or pretty output
- typed parse failures plus fixed depth and byte limits

Current bytes, text-codec, and hash surface:

- `Vec[uint8]` as the bytes representation
- `String.to_bytes()` and `String.from_bytes(...)` for strict UTF-8
- lowercase `bytes.hex_encode(...)` and strict mixed-case
  `bytes.hex_decode(...)`
- canonical standard-alphabet `bytes.base64_encode(...)` and
  `bytes.base64_decode(...)`
- raw 32-byte `bytes.sha256(...)` and `bytes.sha256_string(...)`
- typed `bytes.Error` malformed-input variants with retained `int32` offsets and
  lengths; required metadata above `2147483647` traps with `AU4005` rather than
  being truncated or wrapped
- a fixed 2,147,483,647-byte safety ceiling for each fresh codec destination,
  independent of public String and `Vec` length domains; crossing it,
  destination-size arithmetic overflow, or allocation failure traps with
  `AU4005`

Current builtin I/O, networking, and process surface:

- `io.write(...)`
- `io.flush()`
- `io.read_line()`
- `fs.exists(...)`
- `fs.read_to_string(...)`
- `fs.read_bytes(...)`
- `fs.write_string(...)`
- `fs.write_bytes(...)`
- `fs.append_string(...)`
- `fs.append_bytes(...)`
- `fs.create_dir(...)`
- `fs.read_dir(...)`
- `fs.remove_file(...)`
- `fs.open(...)`
- `fs.create(...)`
- `fs.append(...)`
- `fs.File.read_all()`
- `fs.File.read_bytes()`
- `fs.File.write_all(...)`
- `fs.File.write_bytes(...)`
- `fs.File.flush()`
- `fs.File.close()`
- one-shot and `fs.File` whole-file reads are capped at 256 MiB of remaining content in both `aura run` and built binaries; Aurora 0.1 has no chunked file-read API
- process capture/pipe reads and TCP, Unix, and TLS whole or bounded reads are
  capped at 64 MiB; TLS certificate, private-key, and CA-file loading uses the
  same independent 64 MiB ceiling
- incoming HTTP parsing is capped at 16 MiB of wire data per message
- `net.connect(...)`
- `net.connect_timeout(...)`
- `net.listen(...)`
- `net.udp_bind(...)`
- `net.http_listen(...)`
- `net.http_request_text(...)`
- `net.http_request_text_timeout(...)`
- `net.http_request_bytes(...)`
- `net.http_request_bytes_timeout(...)`
- `net.websocket_listen(...)`
- `net.websocket_connect(...)`
- `net.websocket_connect_timeout(...)`
- `net.unix_listen(...)`
- `net.unix_connect(...)`
- `net.unix_connect_timeout(...)`
- `net.tls_listen(...)`
- `net.tls_connect(...)`
- `net.tls_connect_timeout(...)`
- `process.start(...)`
- `process.run(...)`
- `process.supervisor()`
  - both accept `group=true` to place the child in its own process group and make lifecycle cleanup group-aware on maintained Unix hosts
- `process.inherit()`
- `process.null()`
- `process.pipe()`
- `net.TcpListener.accept(timeout=...)`
- `net.TcpListener.local_addr()`
- `net.TcpListener.close()`
- `net.TcpStream.read_all(timeout=...)`
- `net.TcpStream.read_line(timeout=...)`
- `net.TcpStream.read_bytes(...)`
- `net.TcpStream.read_exact(...)`
- `net.TcpStream.write_all(...)`
- `net.TcpStream.write_bytes(...)`
- `net.TcpStream.flush()`
- `net.TcpStream.local_addr()`
- `net.TcpStream.peer_addr()`
- `net.TcpStream.shutdown_read()`
- `net.TcpStream.shutdown_write()`
- `net.TcpStream.shutdown_both()`
- `net.TcpStream.close()`
- `net.UdpSocket.send_text(...)`
- `net.UdpSocket.send_bytes(...)`
- `net.UdpSocket.recv(...)`
- `net.UdpSocket.recv_from(...)`
- `net.UdpSocket.local_addr()`
- `net.UdpSocket.peer_addr()`
- `net.UdpSocket.close()`
- `net.UdpDatagram.address()`
- `net.UdpDatagram.bytes()`
- `net.UdpDatagram.text()`
- `net.HttpListener.accept(timeout=...)`
- `net.HttpListener.local_addr()`
- `net.HttpListener.close()`
- `net.HttpExchange.method()`
- `net.HttpExchange.path()`
- `net.HttpExchange.headers()`
- `net.HttpExchange.body_text()`
- `net.HttpExchange.body_bytes()`
- `net.HttpExchange.respond_text(...)`
- `net.HttpExchange.respond_bytes(...)`
- `net.HttpResponse.status()`
- `net.HttpResponse.reason()`
- `net.HttpResponse.headers()`
- `net.HttpResponse.text()`
- `net.HttpResponse.bytes()`
- `net.WebSocketListener.accept(timeout=...)`
- `net.WebSocketListener.local_addr()`
- `net.WebSocket.send_text(...)`
- `net.WebSocket.send_bytes(...)`
- `net.WebSocket.recv_text(...)`
- `net.WebSocket.recv_bytes(...)`
- `net.WebSocket.close()`
- `net.UnixListener.accept(timeout=...)`
- `net.UnixListener.close()`
- `net.UnixStream.read_line(timeout=...)`
- `net.UnixStream.read_exact(...)`
- `net.UnixStream.write_all(...)`
- `net.UnixStream.close()`
- `net.TlsListener.accept(timeout=...)`
- `net.TlsListener.local_addr()`
- `net.TlsListener.close()`
- `net.TlsStream.read_line(timeout=...)`
- `net.TlsStream.read_exact(...)`
- `net.TlsStream.write_all(...)`
- `net.TlsStream.close()`
- `process.Child.stdin()`
- `process.Child.stdout()`
- `process.Child.stderr()`
- `process.Child.wait(timeout=...)`
- `process.Child.wait_or_none(timeout=...)`
- `process.Child.wait_ok(timeout=...)`
- `process.Child.kill()`
- `process.Child.terminate()`
- `process.Child.close()`
- `process.Pipe.read_all()`
- `process.Pipe.read_line(timeout=...)`
- `process.Pipe.read_bytes(...)`
- `process.Pipe.write_all(...)`
- `process.Pipe.write_bytes(...)`
- `process.Pipe.flush()`
- `process.Pipe.close()`
- `process.Completed.status()`
- `process.Completed.success()`
- `process.Completed.stdout()` for UTF-8 text
- `process.Completed.stderr()` for UTF-8 text
- `process.Completed.stdout_bytes()`
- `process.Completed.stderr_bytes()`
- `process.Completed.check()`
- `process.Supervisor.start(...)`
- `process.Supervisor.wait(timeout=...)`
- `process.Supervisor.wait_or_none(timeout=...)`
- `process.Supervisor.stop()`
- `process.Supervisor.is_empty()`
- `process.Supervisor.close()`

Current builtin member methods include:

- `float64.sqrt()`
- scalar and boolean `.to_string()`
- `String.len() -> int64` (Unicode scalar values, O(n))
- `String.byte_len() -> int64` (UTF-8 bytes, O(1))
- `String.to_bytes()` (fresh `Vec[uint8]`)
- `String.from_bytes(...)` (associated strict UTF-8 conversion)
- `String.contains(...)`
- `String.starts_with(...)`
- `String.ends_with(...)`
- `String.split(...)`
- `String.join(...)`
- `String.replace(...)`
- `String.to_lower()`
- `String.to_upper()`
- `String.strip_prefix(...)`
- `String.strip_suffix(...)`
- `String.trim()`
- `String.clone()`
- `Vec.len() -> int64`
- `Vec.is_empty()`
- `Vec.clone()`
- `Vec.push(...)`
- `Vec.pop()`
- `Vec.get(...)`
- `Vec.insert(...)`
- `Vec.set(...)`
- `Vec.remove(...)`
- `Vec.swap(...)`
- `Vec.contains(...)`
- `Vec.extend(...)`
- `Vec.clear()`
- `Vec.reverse()`
- `Vec.sort()`
- `Vec.sort_by(key)`
- `Vec.map(f)`
- `Vec.filter(f)`
- `Map.len() -> int64`
- `Map.is_empty()`
- `Map.clone()`
- `Map.get(...)`
- `Map.set(...)`
- `Map.remove(...)`
- `Map.contains_key(...)`
- `Map.keys()`
- `Map.values()`
- `Map.items()`
- `Map.entries()`
- `Map.clear()`
- `Map.extend(...)`
- `Set.len() -> int64`
- `Set.is_empty()`
- `Set.clone()`
- `Set.contains(...)`
- `Set.insert(...)`
- `Set.remove(...)`
- `Queue.put(...)`
- `Queue.try_put(...)`
- `Queue.get(...)`
- `Queue.get_or_none(...)`
- `Queue.get_or(...)`
- `Queue.close()`
- `Task.result(timeout=...)`
- `Task.result_or_none(timeout=...)`
- `Task.result_or(timeout=...)`
- `TaskGroup.start(...)`
- `TaskGroup.start_soon(...)`
- `TaskGroup.start_with_stack(...)`
- `TaskGroup.start_soon_with_stack(...)`
- `TaskGroup.cancel()`
- `random.Rng.next_int(...)`
- `random.Rng.next_float()`
- `random.Rng.shuffle(...)`

## Randomness

Import `random` for two deliberately separate surfaces. A mutable
`random.Rng(seed)` is a deterministic, move-only xoshiro256** stream with
half-open `next_int`, `[0.0, 1.0)` `next_float`, and in-place generic Vec
shuffle. Seed mapping and sequences are stable throughout Aurora 0.1.x and
identical through MIR and direct execution.

`random.secure_int(lo, hi)` and `random.secure_bytes(n)` use only the host
operating system's secure source. They have no seed and never fall back to the
deterministic generator. `secure_bytes(0)` returns an empty vector without an
entropy request. Its count is `int64`, with a fixed per-request resource and
safety ceiling of `2147483647` independent of the public `Vec` length domain.
Invalid bounds or a negative count traps with `AU4003`; a count above the
ceiling traps with `AU4005` before allocation or entropy, and entropy or
allocation failure also traps with `AU4005`. There is no `random.Error` or
secure floating function. See [20-randomness.md](20-randomness.md).

Clone-producing generic bodies infer clone-safety obligations rather than
rejecting unresolved type parameters. Requirements propagate through generic
calls, imports, trait/default/associated dispatch, operators, and `From`, then
reject an unsafe concrete `random.Rng` specialization with `AU3007`. Queue
handles remain clone barriers because copying a handle does not observe its
payload. An allowed Task-handle copy also does not observe its payload, but
`Task[T]` is not copyable when `T` carries a single-consumer result right.

## Pattern Matching

The current compiler supports:

- `Enum.Variant`
- `Enum.Variant(name)`
- multi-payload enum variants including named payload fields
- unqualified variants such as `Ok(value)` and `None` when the scrutinee type is known
- literal patterns over `bool`, integer, and `String`
- floating-point literal patterns
- `match value:`
- `match mut value:`
- `case _:`
- exhaustive statement-form `match`
- expression-form `match` in return, binding, and argument positions
- nested enum patterns

Boolean literal matches are exhaustive when they cover both `true` and `false`. Integer and `String` literal matches still require a final wildcard arm. Expression-form arms may also evaluate nested block-form expressions.

## Concurrency

The current bootstrap concurrency surface includes:

- typed queues
- `for` iteration over queues until close
- task groups
- `TaskGroup.start(...)`
- `TaskGroup.start_soon(...)`
- `TaskGroup.start_with_stack(bytes, ...)`
- `TaskGroup.start_soon_with_stack(bytes, ...)`
- `Task.result(timeout=...)`
- typed `select(queue_or_task_or_duration, ...)`
- `wait_any(...)`
- `wait_all(...)`
- cooperative cancellation
- signed i128-nanosecond Duration values with `ms`, `s`, and `m` literals,
  integer constructors, checked arithmetic, conversions, and comparisons

Aurora 0.1 executes task bodies on cooperative pinned scheduler workers on
both maintained backends. The default count is the available parallelism
reported by the host; the
provisional `AURORA_WORKERS=<positive integer>` override selects an explicit
count. Each child receives a stable assignment at spawn time. Coroutine stacks
never migrate, work is not stolen, and `yield_now()` yields only to runnable
work on the local worker.

Every loop backedge has a compiler-inserted scheduling check, including the
ordinary body tail and `continue`; `break` and `return` bypass it. Tight loops
therefore no longer starve ready timers, queues, or sockets assigned to the
same worker indefinitely, although a single long loop body can still delay
same-worker siblings. The check does not inspect cancellation. Ordinary tasks
request a guarded 512 KiB coroutine stack. The two explicit stack-start
methods accept an exact `int64` byte request from 256 KiB through 64 MiB
inclusive, reject out-of-range values without clamping, and page-round
accepted requests. The 256 KiB lower bound is for measured shallow tasks, not
the generally safe default. The complete compiled Aurora HTTP example requires
the 512 KiB default; an isolated runtime round trip can use 256 KiB protocol
callers because it excludes the compiled program's language-execution frames
and keeps deep host protocol frames on service workers.
Scheduler waits use persistent descriptor
registrations, a timer heap, and direct Queue, task-completion, and
blocking-pool notifications; an idle scheduler blocks until an event or
deadline without a periodic tick.

Task starts require every captured argument and the target result to be
structurally `Transfer` after generic specialization. Copy values, `String`,
recursively transferable collections, tuples, classes, enums, and
Queue/Task handle identities pass. Shared or mutable access,
`random.Rng`, `TaskGroup`, and live filesystem, process, pipe, supervisor,
listener, socket, stream, HTTP-exchange, WebSocket, and TLS resources do not.
`Transfer` is compiler-derived and has no builtin user trait or escape hatch;
an ordinary same-named trait cannot confer the property. A Copy value read
through access becomes an owned snapshot and may cross; non-copy access cannot.

Queue and Task handle state is synchronized across workers. All other task
captures and results remain owned `Transfer` data, preserving a share-nothing
boundary. Cancellation and diagnostic context stay per task, while task
scheduling, independent completion, and output order remain unspecified.
Aurora exposes no worker-introspection API and promises neither work stealing
nor parallel speedup for every workload.

Task results are repeatable only for copy `T`, `Queue[...]`, or recursively
repeatable `Task[...]`. `Task[T]` is always transferable but is copyable only
for those repeatable results. For every other transferable `T`, `result`,
`result_or_none`, and `result_or` consume the handle on their first attempt,
including timeout, cancellation, failure, and fallback outcomes. `wait_any`
and `wait_all` consume the complete task vector for such a `T`; `wait_any`
abandons unchosen observation rights. Boundary failures are `AU3008`,
attempted duplication of a single-consumer right is `AU3009`, and using a
directly observed handle again is moved-value `AU3001`.

`select(...)` accepts one or more positional Queue, Task, and relative-Duration
sources and returns `SelectOutcome[Q, T]`. All Queue payloads share `Q`, all
Task results share `T`, and a missing category uses `None`. Source expressions
run once from left to right. Current-task cancellation wins; otherwise the
lowest original argument index wins among ready sources. Every
non-repeatable Task right is consumed at entry and a losing right is
abandoned. The old statement-shaped `select` remains unsupported.

Deep HTTP, TLS, and maintained Unix WebSocket operations use a distinct bounded
protocol-step service with deep native worker stacks. In the clean Mac14,9
Phase 5.10 report, three 10,000-sleeper runs peaked at 207,798,272,
206,946,304, and 206,831,616 bytes whole-process RSS, preserving the maintained
512 MiB bound. Standalone 1,000-timer controls passed with a 6 ms maximum arm
span and 1 ms worst p99 overshoot.

Aurora does not maintain a “100,000 tasks in 1.5 GiB” claim. Three clean runs
of 100,000 sleepers plus 1,000 timers peaked at 1,170,735,104, 1,921,531,904,
and 2,001,305,600 bytes; two exceeded the proposed limit while timer behavior
remained stable at a 3 ms maximum arm span and 2 ms worst p99 overshoot.
Mac14,9 uses 16 KiB pages, giving those 101,000 stackful tasks a
1,654,784,000-byte one-page floor before other runtime and process memory.
The earlier Phase 5.9 below-gate sample depended on nondeterministic memory
compression and is not a repeatable bound. The roadmap escape hatch therefore
retains the measured result without publishing the massive-concurrency
marketing claim. This does not impose a 100,000-task language limit. The
current four-worker workload passes at a `1.039673x` paired median wall-time
ratio with `396.73%` median four-task process CPU.

The protocol service is lazily initialized and remains alive until process
exit; it has no 0.1 shutdown or join surface. File reads, resolver work, and
listener binding use the generic blocking-I/O pool. Only subsequent PEM
parsing and rustls construction use protocol workers for TLS assets.

The generic pool accepts two process settings:
`AURORA_BLOCKING_WORKERS=<positive integer>` selects an exact, unclamped worker
count, while the absent default derives `2..=8` workers from host parallelism
with fallback `4`; `AURORA_BLOCKING_QUEUE_CAPACITY=<positive integer>` bounds
accepted pending jobs only, while omission preserves the unbounded queue.
Full-queue admission is FIFO and scheduler-aware. Expiry or cancellation before
queue insertion prevents submission. Accepted work still runs once and has
any abandoned result discarded. A bound limits accepted pending backlog, not
admission waiters, and cannot guarantee unrelated blocking-I/O work while every
worker remains stuck.

Current collection notes:

- `String.len()`, `String.byte_len()`, `Vec.len()`, `Map.len()`, and
  `Set.len()` return `int64`; `len(value)` delegates to the corresponding
  `len()` and therefore satisfies `len(value) == value.len()`
- `range(...)` bounds and Vec indexes remain `int32`, so length-driven
  iteration narrows explicitly with the checked
  `range(values.len() as int32)` form
- bare Vec iteration is shared; `for value in own vec:`
  consumes; `for value in mut vec:` supports writeback
- `for value in mut vec:` requires the iterable place itself to be mutable
- `Vec.sort()` and `Vec.sort_by(key)` are stable in-place mutations;
  `sort_by` evaluates one shared key per element from left to right before
  mutating, so a key trap leaves the source unchanged
- `Vec.map(f)` and `Vec.filter(f)` are eager shared traversals that retain the
  source and return fresh owned vectors; `filter` requires clone-safe `T`
- Vec algorithm callbacks have exact bare/shared element parameters; `mut` and
  `own` callback capabilities are rejected rather than adapted
- indexed reads from `Vec[T]` work directly only when `T` is copy; clone-safe non-copy element reads use `get(index)` for an explicit cloned read, while an element carrying `random.Rng` state is directed to `remove(index)` instead
- module-level functions cannot redefine a builtin function name such as `len`, `str`, `abs`, or `print`; that rejection is `AU2007`
- negative Vec indexes normalize once as `len + index` for direct reads/writes, `get`, `set`, `remove`, `swap`, and `insert`
- `get` returns `None` when the normalized index is invalid; direct access and mutating methods trap
- Vec and String slices accept all four omitted-endpoint forms, return fresh
  owned copies, and never clamp invalid or reversed bounds; String slicing
  counts Unicode scalars in O(n), while Vec slicing requires clone-safe,
  repeatably observable elements
- `insert(-1, value)` inserts before the last element;
  `insert(values.len() as int32, value)` appends through a checked narrowing,
  and out-of-range indexes are never clamped
- `Vec[T]` supports equality and inequality when both sides have the same `Vec[T]` type
- `Vec.insert(index, value)`, `Vec.set(index, value)`, `Vec.remove(index)`, and `Vec.swap(first, second)` now trap on out-of-bounds indices instead of silently ignoring the operation
- empty map literals still need an expected `Map[K, V]` type, or you can use `Map[K, V]()` explicitly
- `Map[K, V]` supports literal construction, indexed writes for every `V`, direct indexed reads only when `V` is copy, and the maintained method surface `len`, `is_empty`, `clone`, `get`, `set`, `remove`, `contains_key`, `keys`, `values`, `items`, `entries`, `clear`, and `extend`; non-copy reads use `get` for an explicit clone or `remove` for ownership transfer
- `Map.items()` and `Map.entries()` return `Vec[MapEntry[K, V]]`, where entry values expose `.key` and `.value`
- `Set[T]` supports literal construction with `{...}` and the maintained method surface `len`, `is_empty`, `clone`, `contains`, `insert`, and `remove`
- bare Set iteration is shared; `for value in own set:` consumes
- `for value in mut set:` is not currently supported
- `Queue[T]` supports `Queue[T](capacity=...)` for bounded-capacity queues on
  the pinned-worker runtime scheduler; construction, `put`, and `try_put`
  require a structurally `Transfer` payload type
- `Queue.put(...)` returns `Result[None, SendError[T]]`, where `SendError[T]` currently includes `Closed(value)`, `Cancelled(value)`, `TimedOut(value)`, and `Full(value)`
- `Queue.get(timeout=...)` returns `QueueReceive[T]`, distinguishing `Item(value)`, `Closed`, `TimedOut`, and `Cancelled`
- `Queue.get_or_none(timeout=...)` returns `Option[T]` for the common case where closed, timed out, and cancelled waits all map to “no value”; without a timeout it performs an immediate non-blocking check
- `Queue.get_or(default, timeout=...)` returns either the queued value or a caller-provided fallback; without a timeout it returns the fallback immediately when no item is ready
- Queue iteration receives owned items and accepts only bare `for value in
  queue:`; the explicit `own` and `mut` modifiers are rejected
- `Task.result(timeout=...)` returns `TaskResult[T]`, distinguishing
  `Ready(value)`, `Error(message)`, `TimedOut`, and `Cancelled`; for
  non-repeatable `T`, the call consumes the task handle on every outcome
- `wait_any(...)` returns `WaitAny[T]`, distinguishing `Ready(index, value)`,
  `Error(index, message)`, `TimedOut`, and `Cancelled`; `wait_any([])` returns
  `TimedOut` immediately, and a non-repeatable `T` makes the call consume the
  entire task vector and abandon unchosen rights
- `wait_all(...)` returns `WaitAll[T]`, distinguishing `Ready(results)`,
  `Error(index, message)`, `TimedOut`, and `Cancelled`; a non-repeatable `T`
  makes the call consume the entire task vector
- `Task.result_or_none(timeout=...)` returns `Option[T]` for the common case
  where task failure, timeout, and cancellation all map to “no result yet”;
  without a timeout it performs an immediate non-blocking check, and for
  non-repeatable `T` even a `None` outcome consumes the handle
- `Task.result_or(default, timeout=...)` returns either the task result or a
  caller-provided fallback when the task fails, times out, or is cancelled;
  without a timeout it returns the fallback immediately when the task is not
  ready, and for non-repeatable `T` every outcome consumes the handle

## Tooling

The current CLI commands are:

- `check`
- `run`
- `build`
- `ast`
- `ast-json`
- `mir`
- `analyze`
- `complete`

Current backend/tooling notes:

- `build` accepts `--backend auto|direct`
- `auto` is the default
- `direct` now covers the full currently implemented Aurora language surface
- compiler-backed editor state is invalidated across open documents when imported files change
- `file://` URI handling now preserves both Windows drive-letter paths and UNC workspaces

The current VS Code tooling is compiler-backed for:

- diagnostics
- document symbols
- hover
- go-to-definition
- completions

## Still Outside The Bootstrap Compiler

Not yet implemented:

- non-numeric casts
- direct recursive fields without `indirect`
- first-class loan or view values; the current return syntax reserves no future
  aliasing contract
- method values, statement-bodied closures, mutable captures, and in-loan
  captures

Current module/import limitations:

- imports resolve local `.au` files relative to the current package root
- directly checking or analyzing a nested package file now infers the nearest package root that satisfies its imports
- `import a.b` exposes module namespaces for calls like `a.b.func(...)`, `a.b.Type(...)`, and `a.b.Enum.Variant`
- type annotations may use namespace-imported types such as `a.b.Type`
- both maintained execution paths stop with a friendly recursion-depth diagnostic after 256 nested Aurora calls
- MIR and direct-native runtime failures preserve matching typed Aurora call
  frames and child-task ancestry; JSON tooling receives them as always-present
  `call_frames` and `task_ancestry` arrays
- package manifests, local path dependencies, and git dependencies are now implemented

Current expression/ergonomics limitations:

- empty list literals still require an expected `Vec[T]` type such as `values: Vec[int32] = []`
- strings use quoted literals; `String(...)` is not a constructor
- enum variants may be called by bare built-in name when an expected type is available, for example `ok: Result[int32, String] = Ok(7)`
- `TaskGroup.start(...)`, `TaskGroup.start_soon(...)`, and their explicit-stack
  variants support capture-free function values, Transfer closure values, and
  the existing direct named-function and associated-method-without-`self`
  targets, using task-owned captures; every capture and target result must be
  structurally `Transfer` after specialization
- `TaskGroup()` scope exit waits for started tasks and surfaces unread task failures instead of silently dropping them
- `group.cancel()` wakes queue iteration over `Queue[T]` in the same `with TaskGroup()` scope so `for value in queue:` can exit cleanly
- concurrency uses only the maintained `Queue[T]()`, `Task.result()`,
  `TaskGroup()`, its four start methods, `yield_now()`, `wait_any(...)`, and
  `wait_all(...)` surface
- `control.retry(worker, max_attempts=3, initial_backoff=0ms)` runs a
  `def() -> Result[T, E]` worker immediately, retries every `Err` with doubling
  delays, skips zero sleeps, and returns the exact final error without a
  post-final wait or multiply; it validates a positive attempt budget and a
  non-negative, host-representable backoff before the worker runs, and traps
  and cancellation propagate
- queue waits, `sleep(...)`, socket waits, and the maintained HTTP helpers all
  use the pinned-worker evented runtime scheduler
- Aurora tasks are pinned-worker scheduler-backed lightweight tasks, and
  ordinary file I/O offloads through that runtime instead of pinning a task on
  a blocking host thread
- Unix domain sockets require a Unix host at runtime
- subprocess APIs are shell-free and use explicit argv vectors; process groups and restart supervision are implemented, while PTY support is not
- every function return is owned: copy results are ordinary copies, while a
  non-copy result must be constructed, cloned, moved from owned input, or
  obtained through an owner operation
