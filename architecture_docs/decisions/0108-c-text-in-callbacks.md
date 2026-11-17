# ADR-0108: C Text In Callbacks

- Status: Accepted
- Date: 2026-11-17
- Design checkpoint: [FFI v2 and raylib, section E1](../29-ffi-v2-graphics-design-checkpoint.md#e-other-gaps)

## Decision

A C library can pass a NUL-terminated UTF-8 string to a named Aura
callback. The extern declaration spells the parameter `cstr`. The Aura
function takes a `str`. For example, GLFW's error callback takes an
`int32` error code and a `cstr` description, and returns `None`.

Both backends check the pointer for null, validate its UTF-8 bytes up to
the first NUL, and copy the text before entering the Aura body. An empty
C string is valid. A null pointer or invalid UTF-8 records `AU4005`.
The callback follows the existing failure convention and the enclosing
foreign call reports the diagnostic when control returns from C.

## Rationale

GLFW owns its error description and may reuse the buffer. A copy gives
the Aura function the same string operations and ownership rules as any
other function. C supplies one pointer for this argument. Aura's
pointer-and-length text ABI would change the C calling convention.

Returning text would require an agreement on who owns the returned
buffer and how long it lives. Callback results remain scalars or
`None`. A callback receives a handle or a user-data pointer as a
`uint64`. The library retains responsibility for that pointer's lifetime.
