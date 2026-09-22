# Calling A Small C API

Aura's Foreign Function Interface (FFI) v0 is for small, reviewed bindings to
trusted C symbols that are already visible in the running process. It does
not expose raw pointers or arbitrary library loading.

Start with a package, because a standalone file cannot opt in to FFI:

```toml
[package]
name = "ffi_getpid"
version = "0.1.0"
edition = "2026"
allow_ffi = true
```

Then declare a C function with no body and call it directly:

```aura
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

Both commands print `true`.

The `allow_ffi` opt-in in the manifest is a review boundary. It marks the
package as containing native declarations whose correctness Aura cannot
prove.

## The Safe Surface Is Small

Use fixed-width scalars for ordinary C values. These include `int32`,
`uint64`, `float32`, and their supported peers. `int` is accepted as the
exact alias for `int64`, but an explicit width makes an ABI declaration
easier to review. ABI stands for application binary interface.

Strings and byte lists pass as temporary views:

| Parameter | What C receives |
| --- | --- |
| bare `str` | Temporary UTF-8 bytes and a byte length |
| bare `list[uint8]` | Read-only bytes and a length |
| `mut list[uint8]` | A same-length scratch buffer, for fixed-length copy-in and copy-out |

An empty view passes a null pointer with length zero. The C function must not
keep these pointers. The string view is not guaranteed to end in a NUL byte.

Use an opaque handle when C owns an object whose layout Aura should not see:

```aura
public extern "C" opaque class Handle
public extern "C" def acquire() -> Handle
public extern "C" def inspect(handle: Handle) -> int32
public extern "C" def close(handle: own Handle) -> None
```

- A bare `Handle` parameter shares the pointer for one synchronous call.
- An `own Handle` parameter consumes it.
- An opaque handle cannot be cloned or sent to another Aura task.
- A binding must call the matching native close or free function.

## What Aura Does Not Promise

The compiler checks the Aura declaration, not the native implementation. A
wrong C signature, a retained temporary pointer, or an out-of-bounds native
write can corrupt or end the process. Aura does not translate native aborts,
signals, or unwinds into Aura failures. Calls are synchronous and occupy the
current Aura worker.

[Foreign Function Interface (FFI) v0](/manual/ffi) in the Manual has the
complete ABI table, the manifest dependency-report rule, the diagnostics, and
the backend contract.
