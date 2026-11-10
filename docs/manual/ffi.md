# Foreign Function Interface (FFI)

Aura FFI calls C functions through a fixed subset of the platform C ABI. A
package declares each function it calls and names the native libraries they
come from. It is an unsafe package capability for binding trusted native
code. It is not a general system for pointers or callbacks.

## Package Opt-In

Every source file that declares an extern function or an opaque handle must
belong to an Aura package whose manifest opts in:

```toml
[package]
name = "native_binding"
version = "0.1.0"
edition = "2026"
allow_ffi = true
```

A standalone `.au` file outside a package cannot declare FFI. Compiler
embedders must use the public path-based checking, lowering, or execution APIs
for FFI source. Source-only APIs cannot establish manifest authorization.

If any dependency in the package graph enables FFI, the root package must also
set `allow_ffi = true`. It must list every reachable FFI-enabled dependency by
package name, including transitive dependencies:

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

The report must be exact. The resolver rejects duplicate, unknown, unreachable,
and non-FFI entries, and an entry for the root package. Each FFI-enabled
dependency must also opt itself in. The report grants visibility, not trust.
The root application is still responsible for reviewing the declarations and
the native code they call. [Packages](/manual/packages) documents the manifest
fields.

## Linking Libraries

A package names the native libraries its extern functions come from in the
`[ffi]` table of its manifest:

```toml
[package]
name = "store"
version = "0.1.0"
edition = "2026"
allow_ffi = true

[ffi]
libraries = ["sqlite3"]
library_paths = ["native/lib"]

[ffi.macos]
frameworks = ["Accelerate"]

[ffi.linux]
libraries = ["openblas"]
```

| Key | Meaning |
| --- | --- |
| `libraries` | Library names as the C linker's `-l` option takes them. `sqlite3` names `libsqlite3.so` on Linux and `libsqlite3.dylib` on macOS. |
| `library_paths` | Directories to search for those libraries before the system's own. A relative path starts at the manifest's directory. |
| `frameworks` | macOS frameworks. Other hosts ignore them. |

`[ffi.linux]` and `[ffi.macos]` take the same three keys and apply on that
host only. The settings of every package in the graph apply to the program:
each dependency's first, then the root package's.

The functions of the platform C library, such as `getpid` and `malloc`, need
no entry.

A package that names a library must set `allow_ffi = true`. A library or
framework name is a bare name of ASCII letters, digits, `_`, `+`, `.`, and
`-` that starts with a letter, digit, `_`, or `+`. The resolver rejects any
other entry with `AU2999`, so a manifest cannot pass options to the linker.

`aura build` and `aura run` pass the settings to the C linker. A program
that names a library the linker cannot find does not build, and the linker's
message names the library. The built program records each `library_paths`
directory as a run-time search path, so it finds a library there when it
starts. To move the program to another machine, install the library where
that machine's loader looks.

`aura run --backend mir` loads the same libraries into the interpreter
before the program starts. A library it cannot load stops the run with
`AU4005`.

## Grammar

FFI accepts only bodyless C declarations:

```aura fragment
public extern "C" opaque class ProcessHandle
public extern "C" def getpid() -> int32
extern "C" def inspect(label: str, data: list[uint8]) -> uint64
extern "C" def update(data: mut list[uint8]) -> None
extern "C" def close(handle: own ProcessHandle) -> None

def main() -> int32:
    print(getpid() > 0)
    return 0
```

The rules for these declarations:

- **ABI string.** It must be exactly `"C"`.
- **Symbol name.** The Aura declaration name is the C symbol name unless a
  `symbol` clause after the return type gives another:
  `extern "C" def c_abs(x: int32) -> int32 symbol "abs"`. Use the clause when
  the C name is long, or is already an Aura name. `symbol` is an ordinary
  name everywhere else. There is no source spelling for a calling
  convention, symbol version, or variadic tail.
- **Visibility.** `public` has its ordinary module meaning. Another module can
  import a public declaration. A private declaration is local to its module.
- **Return type.** Every extern function spells an explicit `-> Type`. Use
  `-> None` for a C function that returns no value.
- **No body.** An extern function has no Aura body, type parameters, receiver,
  or defaults. Its only block is an optional trusted contract.
- **Trusted contract.** An indented block of `requires` and `ensures` clauses
  states what the C function needs and promises. `aura verify` proves each
  `requires` at every call and assumes each `ensures` after it, without
  proof. The program checks neither when it runs, because the contract is
  trusted. See [Verification](/manual/verification#trust).
- **Opaque handles.** An opaque declaration is written
  `extern "C" opaque class Name`. It has no fields, methods, body, or type
  parameters.

Raw pointer syntax, callback types, and `...` variadics are reserved. The
compiler rejects them with teaching diagnostics. Aura code cannot construct an
opaque handle. An extern declaration is direct-call-only, so it cannot be used
as a first-class function value.

## Typing Rules

### Scalars

The accepted scalar types are fixed:

| Aura type | C ABI value |
| --- | --- |
| `bool` | one-byte boolean; returns must be exactly `0` or `1` |
| `int8`, `int16`, `int32`, `int64` | signed 8-, 16-, 32-, or 64-bit integer |
| `uint8`, `uint16`, `uint32`, `uint64` | unsigned 8-, 16-, 32-, or 64-bit integer |
| `float32`, `float64` | IEEE-754 binary32 or binary64 |
| `int` | the exact `int64` alias; `int64` is preferred in ABI declarations |
| `None` | return-only void result |

Scalar parameters must be bare, because their bits are passed by value. These
types have no FFI representation: `int128`, `uint128`, `intsize`,
`uintsize`, `Duration`, tuples, user classes, enums, generic types, and
arbitrary collection types.

### Text And Byte Views

Three parameter forms pass a pointer and a length:

| Aura parameter | C parameters in order | Contract |
| --- | --- | --- |
| `text: str` | `const uint8_t *`, `size_t` | UTF-8 bytes; not NUL-terminated |
| `data: list[uint8]` | `const uint8_t *`, `size_t` | read-only bytes |
| `data: mut list[uint8]` | `uint8_t *`, `size_t` | fixed-length writable bytes |

The pointer is valid only during the synchronous foreign call. The native
callee must not keep it.

- An empty `str` or byte view passes a null pointer and length zero.
- A non-empty view passes a valid pointer and its exact byte length.
- A mutable byte view lends exactly the list's bytes. What the foreign
  function writes is in the list when it returns, even if later result
  validation reports an Aura error. Foreign code cannot change the list's
  length or capacity.

The checker rejects `own str`, `mut str`, and `own list[uint8]`. An extern
function cannot return a text or byte view, because FFI has no foreign
allocator or lifetime contract.

### Opaque Handles

An opaque handle is one non-null foreign pointer with no layout visible to
Aura.

- A bare handle parameter shares the pointer for that call. The caller keeps
  the Aura handle.
- An `own Handle` parameter consumes the handle. This is normally a foreign
  close or free operation.
- `mut Handle` is reserved.
- Handles are not Copy, not cloneable, and never `Transfer`. They cannot cross
  a task or Queue boundary.
- A null `-> Handle` result is an Aura runtime failure, `AU4005`.

### Nullable Handle Results

One nullable form exists. An extern result whose normalized type is exactly a
declared opaque handle plus `None` crosses C as one pointer. Write it as
`-> Handle | None` or through an alias of that shape. A null pointer becomes
`None`, and a non-null pointer becomes the owned handle. No union tag or
aggregate crosses C.

The checker rejects every other union shape with `AU2010`. That includes
optional scalars, strings, byte views, nullable parameters, and several handle
alternatives. These need an explicit C adapter. Mutable byte-view writeback
still happens before result translation. The resulting handle follows every
opaque-handle rule.

### Handles Inside Other Values

Non-cloneability is structural. It carries through tuples, collections, user
classes, enum payloads, and generic specializations.

- `.clone()` is rejected when the duplicated value contains an opaque handle.
  So are clone-producing collection observations such as `get`, projected
  reads, and `filter`.
- Consuming operations such as `pop`, `remove`, and replacement are allowed.
- Equality and inequality are rejected for an opaque handle or any value that
  structurally contains one.
- Arithmetic and ordering operators on a handle are rejected with dedicated
  diagnostics. Raw pointer arithmetic and ordering by foreign address are not
  language capabilities.

FFI does not expose foreign addresses. It does not assume that address
identity is the native API's identity. When callers need to compare foreign
objects, a binding should expose reviewed extern operations or a stable scalar
or `str` key.

## Runtime Semantics

A built program calls a foreign function the way C code does. The call is
one machine call instruction, with each argument in the register or stack
slot the C ABI gives it. The linker resolves the symbol when the program is
built, from the platform C library and the libraries the manifest names. A
symbol that no library defines is a link error, not a run-time failure.

The interpreter, `aura run --backend mir`, looks the symbol up when the call
runs, among the libraries loaded into the process. A missing symbol there is
`AU4005`. `aura run` with the default backend falls back to the interpreter
when the program does not link, so it reports the same `AU4005`.

A call runs in this order:

1. Aura evaluates the arguments left to right under ordinary call rules.
2. Aura passes the arguments by the C ABI.
3. The foreign function runs.
4. Aura validates the result, including canonical booleans and non-null opaque
   handles.

A result validation failure cannot roll back foreign side effects or bytes
the function wrote.

Every foreign call is synchronous. It occupies the current Aura worker until
the native function returns. It does not move to the blocking I/O pool and
does not create an implicit scheduling point. A long or blocking native call
can delay other tasks pinned to that worker.

FFI declarations are unsafe contracts. Aura cannot verify that the real C
signature, pointer retention, allocation, thread safety, and mutation behavior
match the declaration.

## Ownership And Evaluation Order

| Argument | Ownership |
| --- | --- |
| Scalar | Copied into an ABI slot. |
| Bare `str`, `list[uint8]`, or opaque handle | Stays owned by the caller and is usable after the call. |
| `mut list[uint8]` | Needs an exclusive mutable place. In-place byte updates are visible after return. |
| `own` opaque handle | Moves before the call and cannot be used afterward. |

The declared capability is exact. Aura inserts no implicit clone, ownership
conversion, or pointer-lifetime extension. A foreign call may have
irreversible external effects. Evaluating a later argument or validating the
result does not undo earlier evaluation, the foreign call, or foreign writes.

Opaque handles have no automatic foreign destructor. A binding package must
declare and call the right consuming C function. Dropping an unconsumed handle
discards only Aura's wrapper. It may leak the foreign resource if the native
API needs explicit destruction.

Printing, f-string interpolation, and `str(...)` render a handle as
`<opaque TypeName>`, using its canonical Aura type name. The pointer address
never appears in source-visible output or diagnostics.

## Diagnostics

| Code | Cause |
| --- | --- |
| `AU1101` | Malformed extern or opaque syntax. The parser gives dedicated guidance for a foreign body, defaults, type parameters, callbacks, variadics, and raw-pointer spelling. |
| `AU2002` | A type outside the fixed scalar, view, and opaque-handle tables, including a returned `str` or `list[uint8]` view. |
| `AU2003` | Equality or inequality on an opaque handle or a value that structurally contains one. |
| `AU2005` | A reserved FFI form, construction of an opaque handle, or a callback or raw-pointer contract that reaches static checking. |
| `AU2010` | A union result or parameter other than the `Handle \| None` result form. |
| `AU2999` | Missing package opt-in, an inaccurate root dependency report, an invalid `[ffi]` library, framework, or path entry, standalone FFI source, an extern used as a value, or another FFI policy violation without a narrower code. |
| `AU3001` | Use of an opaque handle after an `own` extern call. |
| `AU3004` | An invalid scalar, view, or handle capability. |
| `AU3008` | An opaque handle at a task or Queue `Transfer` boundary. |
| `AU4001` | A non-canonical C boolean result: the returned byte was neither `0` nor `1`. |
| `AU4005` | A recoverable runtime boundary failure: a null opaque-handle result, and in the interpreter a library that does not load, a missing symbol, or a marshalling failure. |

Aura panics and traps never unwind through a foreign frame. Pre-call failures
stop before entry, and post-call failures are raised after return.

FFI cannot catch or translate a native abort, signal, memory fault, or
foreign unwind. Foreign code must not unwind across the C ABI. Such a native
failure may terminate the process instead of producing an Aura diagnostic.
Out-of-bounds writes, a mismatched C signature, and keeping a temporary view
are outside Aura's memory-safety guarantees.

## Backend Support

The native backend and the interpreter share one validated ABI description.
The native backend emits the C call itself. The interpreter builds each call
at run time. They agree on argument layout, ownership, mutable views,
results, and Aura diagnostics. The maintained `examples/packages/ffi_getpid`
package and the FFI acceptance tests run the same declarations on both.

A foreign call from a built program costs about what the same call costs
from C. A scalar call such as `labs` takes under a nanosecond more than the
function itself. A `str` or `list[uint8]` argument adds a few nanoseconds to
find its bytes, and a `str` literal adds nothing.

Library loading and symbol lookup in the interpreter are implemented on
Unix-family hosts. On other hosts, a call there fails with the runtime
boundary diagnostic. Aura does not silently select a different ABI. The
declaration must still name the host's actual C symbol.

## Limits And Implementation-Defined Behavior

FFI does not:

- define C structs or unions, or pass enums
- allocate foreign memory or expose pointer arithmetic
- return views
- represent nullable handles beyond the `Handle | None` result form
- accept callbacks or variadics
- offer asynchronous foreign calls

Aura does not infer C ABI layout outside the tables above.

Symbol availability and behavior are host-defined. `size_t`, pointer layout,
and process symbol visibility follow the target platform. The fixed-width
integer and floating-point contracts are exact. A declaration that misstates
the real native signature has undefined foreign behavior. It may corrupt or
terminate the process, and no backend can make it safe.

## Status

Implemented:

- package opt-in and the root dependency report
- library, search path, and framework settings in `[ffi]`, with host tables
- bodyless `extern "C"` functions, the `symbol` clause, and opaque handles
- fixed-width scalars and pointer-length views
- direct calls from built programs, and library loading in the interpreter
- the `Handle | None` result form

Reserved or unavailable: callbacks, raw pointers, variadics, returned views,
other nullable shapes, and foreign aggregate layout. Aura does not infer them
from current syntax.

Design record:
[anonymous closed union types](https://github.com/johnolafenwa/Aura/blob/main/architecture_docs/decisions/0052-anonymous-closed-union-types.md)
