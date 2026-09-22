# 26. Foreign Function Interface v0

Aura's foreign function interface, FFI v0, binds small, trusted C APIs. It
does not open the language to general pointer manipulation. Only a package can
declare FFI functions, and its manifest must opt in explicitly.

The maintained example is `examples/packages/ffi_getpid`. Its manifest sets
`allow_ffi = true`:

```toml
[package]
name = "ffi_getpid"
version = "0.1.0"
edition = "2026"
allow_ffi = true
```

```aura fragment
public extern "C" def getpid() -> int32

def main() -> int32:
    print(getpid() > 0)
    return 0
```

On Unix-family hosts, run it with either maintained backend:

```bash
aura run --backend mir examples/packages/ffi_getpid/src/main.au
aura run --backend direct examples/packages/ffi_getpid/src/main.au
```

Both runs print `true`.

## Signatures

An FFI function is a bodyless `extern "C" def` declaration.

Fixed-width scalar parameters are bare and pass by value. The accepted types
are:

- signed and unsigned 8, 16, 32, and 64-bit integers
- `bool`
- `float32` and `float64`

`int` is the exact alias of `int64`, but `int64` states the ABI width more
directly. A result may be one of those scalars, `None`, or a declared opaque
handle.

Strings and byte lists pass as temporary views:

| Parameter type | What the callee receives |
| --- | --- |
| `str` | A temporary const UTF-8 pointer and a byte length. It is not NUL-terminated. |
| `list[uint8]` | The matching read-only byte view. |
| `mut list[uint8]` | A same-length scratch buffer, copied in and out. The list length does not change. |

An empty view passes a null pointer and length zero. The native callee must
not keep any view pointer after the synchronous call returns.

```aura fragment
public extern "C" def checksum(data: list[uint8]) -> uint64
public extern "C" def normalize(data: mut list[uint8]) -> None
```

## Opaque Handles

Declare an opaque class to hold a non-null foreign pointer:

```aura fragment
public extern "C" opaque class Handle
public extern "C" def acquire() -> Handle
public extern "C" def inspect(handle: Handle) -> int32
public extern "C" def close(handle: own Handle) -> None
```

Aura code cannot construct an opaque handle, clone it, transfer it, or inspect
its layout or address. A bare parameter keeps the handle, and an `own`
parameter consumes it. Printing a handle shows only `<opaque TypeName>`.

FFI v0 never calls a destructor for you. Your binding must call the right
consuming native function.

## Package Dependency Reports

When an application depends on an FFI-enabled package, the root package must
opt in too. It must also name every reachable FFI-enabled dependency,
including transitive ones:

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

The list is exact, so you can audit it. An unknown, duplicate, non-FFI, or
missing entry is an error.

## Safety Boundary

Aura checks that each declaration uses the supported surface. It cannot check
the real native signature or behavior.

Some native problems become Aura runtime failures:

| Code | Cause |
| --- | --- |
| `AU4005` | A missing process-global symbol, or a null handle. |
| `AU4001` | A non-canonical C boolean result, meaning a byte other than `0` or `1`. |

Other native problems can still end or corrupt the process: an abort, a
signal, a memory fault, an unwind, an out-of-bounds write, or a retained
temporary pointer.

FFI v0 does not have:

- callbacks
- variadics
- raw pointer arithmetic
- returned views
- nullable handles
- explicit library-loading declarations

The normative contract is
[Foreign Function Interface (FFI) v0](../docs/manual/ffi.md).
