# Foreign Function Interface (FFI) v0

Aurora FFI v0 calls a deliberately small subset of the platform C ABI. It is
an unsafe package capability for binding trusted, already-loaded native
symbols; it is not a general dynamic-library, pointer, or callback system.

Every source file that declares an extern function or opaque handle must belong
to an Aurora package whose manifest explicitly opts in. Compiler embedders
must therefore use the public path-based checking, lowering, or execution APIs
for FFI source; source-only APIs cannot establish manifest authorization:

```toml
[package]
name = "native_binding"
version = "0.1.0"
edition = "2026"
allow_ffi = true
```

A standalone `.au` file outside a package cannot declare FFI. If any dependency
in the package graph enables FFI, the root package must also set
`allow_ffi = true` and list every reachable FFI-enabled dependency by package
name, including transitive dependencies:

```toml
[package]
name = "app"
version = "0.1.0"
edition = "2026"
allow_ffi = true

[dependencies]
native_binding = { path = "../native_binding" }

[ffi]
dependencies = ["native_binding"]
```

The report is exact. Duplicate, unknown, unreachable, non-FFI, and root-package
entries are rejected. An FFI-enabled dependency must opt itself in as well.
The report grants visibility, not trust: the root application remains
responsible for reviewing the declarations and the native code they invoke.

## Grammar

Only bodyless C declarations are accepted:

```aurora
public extern "C" opaque class ProcessHandle
public extern "C" def getpid() -> int32
extern "C" def inspect(label: String, data: Vec[uint8]) -> uint64
extern "C" def update(data: mut Vec[uint8]) -> None
extern "C" def close(handle: own ProcessHandle) -> None

def main() -> int32:
    print(getpid() > 0)
    return 0
```

`public` has its ordinary module-visibility meaning. A public declaration may
be imported from another module; a private declaration is local to its
defining module. The Aurora declaration name is the C symbol name. FFI v0 has
no source spelling for a separate link name, library name, calling convention,
symbol version, or variadic tail.

The ABI string must be exactly `"C"`. Every extern function must spell an
explicit `-> Type`; use `-> None` for a C function that returns no value.
Extern functions have no Aurora body, type parameters, receiver, defaults, or
trailing colon. An opaque declaration uses `extern "C" opaque class Name` and
has no fields, methods, body, or type parameters.

Raw pointer syntax, callback types, and `...` variadics are reserved and
rejected with teaching diagnostics. Aurora code cannot construct an opaque
handle or use an extern declaration as a first-class function value; externs
are direct-call-only.

## Typing Rules

The accepted scalar surface is fixed:

| Aurora type | C ABI value |
| --- | --- |
| `bool` | one-byte boolean; returns must be exactly `0` or `1` |
| `int8`, `int16`, `int32`, `int64` | signed 8-, 16-, 32-, or 64-bit integer |
| `uint8`, `uint16`, `uint32`, `uint64` | unsigned 8-, 16-, 32-, or 64-bit integer |
| `float32`, `float64` | IEEE-754 binary32 or binary64 |
| `int` | the exact `int64` alias; `int64` is preferred in ABI declarations |
| `None` | return-only void result |

Scalar parameters must be bare because their bits are passed by value.
`int128`, `uint128`, `intsize`, `uintsize`, `Duration`, tuples, user classes,
enums, generic types, and arbitrary collection types do not have an FFI v0
representation.

The three pointer-length parameter forms are:

| Aurora parameter | C parameters in order | Contract |
| --- | --- | --- |
| `text: String` | `const uint8_t *`, `size_t` | UTF-8 bytes; not NUL-terminated |
| `data: Vec[uint8]` | `const uint8_t *`, `size_t` | read-only bytes |
| `data: mut Vec[uint8]` | `uint8_t *`, `size_t` | fixed-length writable bytes |

The pointer is valid only during the synchronous foreign call. The native
callee must not retain it. An empty String or byte view passes a null pointer
and length zero; a non-empty view passes a valid pointer and its exact byte
length. A mutable byte view uses a same-length scratch buffer: Aurora copies
the vector's initial bytes in, then copies exactly that length back after the
foreign function returns. The writeback happens even if subsequent result
validation reports an Aurora error. Its length and capacity cannot be changed
by foreign code. `own String`, `mut String`, and `own Vec[uint8]` are rejected.
String or byte views cannot be returned because v0 has no foreign allocator or
lifetime contract.

An opaque handle is one non-null foreign pointer with no Aurora-visible
layout. A bare handle parameter shares the pointer for that call and retains
the Aurora handle. An `own Handle` parameter consumes it, normally for a
foreign close/free operation. `mut Handle` is reserved. Opaque handles are
non-Copy, non-cloneable, and never `Transfer`, so they cannot cross a task or
Queue boundary. A returned null pointer is an Aurora runtime failure; nullable
opaque handles are not part of FFI v0.

This non-cloneability is structural through tuples, collections, user classes,
enum payloads, and generic specializations. `.clone()` and clone-producing
collection observations such as `get`, projected reads, and `filter` are
rejected whenever the duplicated value contains an opaque handle. Consuming
transfer operations such as `pop`, `remove`, and replacement remain allowed.
Equality and inequality are also rejected for an opaque handle or any value
that structurally contains one. FFI v0 deliberately does not expose foreign
addresses or assume that address identity is the native API's semantic
identity; a binding should expose a stable scalar or String identifier when
callers need to compare foreign objects. Arithmetic and ordering operators on
the handle itself are rejected with dedicated diagnostics: raw pointer
arithmetic and foreign-address ordering are not language capabilities. A
binding must expose reviewed extern operations or stable scalar/String keys
instead.

## Runtime Semantics

The runtime resolves the declaration name against the process-global symbol
table at the moment of the call. FFI v0 does not open a dynamic library or
search a user-specified path. The symbol must already be visible to the
process, commonly because it comes from the platform C runtime or was linked
into the executable.

Arguments are evaluated left-to-right under ordinary Aurora call rules, then
marshalled to the C ABI. A missing symbol or marshalling failure prevents the
foreign call. After the function returns, Aurora writes back each mutable
same-length byte scratch buffer and then validates representable results,
including canonical booleans and non-null opaque handles. Foreign side effects
and completed byte writeback cannot be rolled back by a later return-value
validation failure.

Every foreign call is synchronous. It occupies the current Aurora worker
until the native function returns; it is not moved to the blocking I/O pool
and does not create an implicit scheduling point. A long or blocking native
call can therefore delay other tasks pinned to that worker.

FFI declarations are unsafe contracts. Aurora cannot verify that a
process-global symbol exists at compile time or that its real C signature,
pointer retention, allocation, thread-safety, and mutation behavior match the
declaration.

## Ownership And Evaluation Order

Ordinary scalar arguments are copied into ABI slots. Bare `String`,
`Vec[uint8]`, and opaque-handle arguments remain owned by the caller and are
available after the call. `mut Vec[uint8]` requires an exclusive mutable place
and exposes in-place byte updates after return. An `own` opaque-handle
argument moves the handle before the call and cannot be used afterward.

The declaration's capability is exact; no implicit clone, ownership
conversion, or pointer-lifetime extension is inserted. Because a process call
may have irreversible external effects, evaluating a later argument or
validating the result does not undo earlier evaluation, the foreign call, or
foreign writes.

Opaque handles have no automatic foreign destructor. A binding package must
declare and call the appropriate consuming C function. Dropping an unconsumed
handle discards only Aurora's wrapper and may leak the foreign resource if the
native API requires explicit destruction.

Printing, f-string interpolation, or `str(...)` renders a handle as
`<opaque TypeName>`, using its canonical Aurora type name. The pointer address
is never part of source-visible rendering or diagnostics.

## Diagnostics

- `AU1101` rejects malformed extern/opaque syntax and gives dedicated
  guidance for a foreign body, defaults, type parameters, callbacks,
  variadics, and raw-pointer spelling recognized by the parser.
- `AU2002` rejects types outside the fixed scalar, view, and opaque-handle
  table, including returned `String` or `Vec[uint8]` views.
- `AU2003` rejects equality or inequality on an opaque handle or a value that
  structurally contains one.
- `AU2005` rejects reserved FFI forms, constructing an opaque handle, and
  callback or raw-pointer contracts that reach static checking.
- `AU2999` reports missing package opt-in, an inaccurate root dependency
  report, standalone FFI source, a direct-call-only extern used as a value, or
  another FFI policy violation without a narrower code.
- `AU3001` reports use of an opaque handle after an `own` extern call.
- `AU3004` reports an invalid scalar/view/handle capability.
- `AU3008` rejects an opaque handle at a task or Queue `Transfer` boundary.
- `AU4001` reports a non-canonical C boolean result: the returned byte was
  neither `0` nor `1`.
- `AU4005` reports a recoverable runtime boundary failure such as a missing
  process-global symbol, null opaque-handle result, or runtime marshalling
  failure.

Aurora panics and traps never unwind through a foreign frame: pre-call
failures stop before entry and post-call failures are raised after return.
Conversely, FFI v0 cannot catch or translate a native abort, signal, memory
fault, or foreign unwind. Foreign code must not unwind across the C ABI. Such a
native failure may terminate the process rather than produce an Aurora
diagnostic. Out-of-bounds writes, a mismatched C signature, or retaining a
temporary view is outside Aurora's memory-safety guarantees.

## Backend Support

The MIR and direct native backends share one validated ABI description and one
host-call engine. They must agree on argument layout, ownership, mutable-view
writeback, results, and Aurora diagnostics. The maintained
`examples/packages/ffi_getpid` package and FFI acceptance test run the same
`getpid` declaration on both backends.

Process-global symbol lookup is currently implemented on Unix-family hosts.
On another host, a call fails with the documented runtime boundary diagnostic
rather than silently selecting a different ABI. The source declaration still
must name the host's actual C symbol.

## Limits And Implementation-Defined Behavior

FFI v0 does not load libraries, select symbols by link name, define C structs
or unions, pass enums, allocate foreign memory, expose pointer arithmetic,
return views, represent nullable handles, accept callbacks or variadics, or
offer asynchronous foreign calls. C ABI layout outside the explicit table is
not inferred.

Symbol availability and behavior are host-defined. `size_t`, pointer layout,
and process symbol visibility follow the target platform. The fixed-width
integer and floating contracts remain exact. A declaration that lies about the
real native signature has undefined foreign behavior and may corrupt or
terminate the process; no backend can make such a declaration safe.

## Status

FFI v0, its package opt-in and root dependency report, bodyless
`extern "C"` functions, opaque handles, fixed-width scalars, pointer-length
views, and Unix process-global lookup are implemented in Aurora 0.2.

Callbacks, raw pointers, variadics, returned views, nullable handles, explicit
library loading/link configuration, and foreign aggregate layout are reserved
or unavailable. They are not inferred from current syntax.
