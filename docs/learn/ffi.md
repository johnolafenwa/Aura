# Calling A Small C API

Aurora's FFI v0 is for small, reviewed bindings to trusted C symbols that are
already visible in the running process. It deliberately does not expose raw
pointers or arbitrary library loading.

Start with a package because standalone files cannot opt in to FFI:

```toml
[package]
name = "ffi_getpid"
version = "0.1.0"
edition = "2026"
allow_ffi = true
```

Then declare a bodyless C function and call it directly:

```python
public extern "C" def getpid() -> int32

def main() -> int32:
    print(getpid() > 0)
    return 0
```

On a Unix-family host, run the maintained example:

```bash
aura run --backend mir examples/packages/ffi_getpid/src/main.au
aura run --backend direct examples/packages/ffi_getpid/src/main.au
```

Both commands print `true`. The manifest opt-in is a review boundary: it says
that the package contains native declarations whose correctness Aurora cannot
prove.

## The Safe Surface Is Small

Use fixed-width scalars (`int32`, `uint64`, `float32`, and their supported
peers) for ordinary C values. `int` is accepted as the exact `int64` alias,
but an explicit width makes an ABI declaration easier to review.

A bare `String` parameter passes temporary UTF-8 bytes and a byte length. A
bare `Vec[uint8]` passes read-only bytes and a length. `mut Vec[uint8]` uses a
same-length scratch buffer for fixed-length copy-in/out. Empty views use a
null pointer with length zero. The C function must not retain those pointers,
and the string view is not promised to end in a NUL byte.

Use an opaque handle when C owns an object whose layout Aurora should not see:

```python
public extern "C" opaque class Handle
public extern "C" def acquire() -> Handle
public extern "C" def inspect(handle: Handle) -> int32
public extern "C" def close(handle: own Handle) -> None
```

The bare parameter shares the pointer for one synchronous call. `own Handle`
consumes it. Opaque handles cannot be cloned or sent to another Aurora task,
and a binding must call the appropriate native close/free function.

## What Aurora Does Not Promise

The compiler checks the Aurora declaration, not the native implementation. A
wrong C signature, retained temporary pointer, or out-of-bounds native write
can corrupt or terminate the process. Native aborts, signals, and unwinds are
not translated into Aurora failures. Calls are synchronous and occupy their
current Aurora worker.

The complete ABI table, manifest dependency-report rule, diagnostics, and
backend contract are in [Foreign Function Interface (FFI) v0](/manual/ffi).
