# 26. Foreign Function Interface v0

Aurora FFI v0 binds small, trusted C APIs without opening the language to
general pointer manipulation. FFI declarations are package-only and require
an explicit manifest opt-in.

The maintained example is `examples/packages/ffi_getpid`:

```toml
[package]
name = "ffi_getpid"
version = "0.1.0"
edition = "2026"
allow_ffi = true
```

```python
public extern "C" def getpid() -> int32

def main() -> int32:
    print(getpid() > 0)
    return 0
```

On Unix-family hosts, run it through either maintained backend:

```bash
aura run --backend mir examples/packages/ffi_getpid/src/main.au
aura run --backend direct examples/packages/ffi_getpid/src/main.au
```

The two runs print `true`.

## Signatures

FFI functions are bodyless `extern "C" def` declarations. Fixed-width scalar
parameters are bare and pass by value. The accepted widths are signed and
unsigned 8/16/32/64-bit integers plus `bool`, `float32`, and `float64`.
`int` is the exact `int64` alias, although `int64` communicates the ABI width
more directly. Results may use one of those scalars, `None`, or a declared
opaque handle.

A bare `String` lowers to a temporary const UTF-8 pointer and byte length; it
is not NUL-terminated. `Vec[uint8]` is the matching read-only byte view, while
`mut Vec[uint8]` uses a same-length scratch buffer for copy-in/out without
changing the vector length. Empty views pass a null pointer and length zero.
The native callee must not retain any view pointer after the synchronous call.

```python
public extern "C" def checksum(data: Vec[uint8]) -> uint64
public extern "C" def normalize(data: mut Vec[uint8]) -> None
```

## Opaque Handles

Use a declaration-only opaque class for a non-null foreign pointer:

```python
public extern "C" opaque class Handle
public extern "C" def acquire() -> Handle
public extern "C" def inspect(handle: Handle) -> int32
public extern "C" def close(handle: own Handle) -> None
```

Aurora cannot construct, inspect the layout or address of, clone, or transfer
an opaque handle. A bare parameter retains it; `own` consumes it. Rendering
shows only `<opaque TypeName>`. FFI v0 does not automatically invoke a
destructor, so a binding must call the correct consuming native function.

## Package Dependency Reports

When an application depends on an FFI-enabled package, the root package must
also opt in and name every reachable FFI-enabled dependency, including a
transitive one:

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

The list is exact and auditable. Unknown, duplicate, non-FFI, or missing
entries are errors.

## Safety Boundary

Aurora checks that the declaration uses the supported surface. It cannot
verify the real native signature or behavior. A missing process-global symbol,
or null handle becomes an `AU4005` runtime failure. A non-canonical C boolean
result (a byte other than `0` or `1`) traps with `AU4001`.
A native abort, signal, memory fault, unwind, out-of-bounds write, or retained
temporary pointer can still terminate or corrupt the process. There are no
callbacks, variadics, raw pointer arithmetic, returned views, nullable
handles, or explicit library-loading declarations in FFI v0.

The normative contract is
[Foreign Function Interface (FFI) v0](../docs/manual/ffi.md).
