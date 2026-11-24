# 0142. Typed Raw Pointers

Date: 2026-11-24

## Decision

Use `ptr[T]` for a copyable address whose element has a C memory representation. A pointer has one machine word on each supported target. Equality compares addresses of the same pointer type. An explicit cast converts between pointer types or between a pointer and an unsigned 64-bit address.

```aura fragment
p = address as ptr[int16]
p.offset(1).volatile_write(-17)
sample = p.offset(1).volatile_read()
```

An offset counts elements. A plain access permits an unaligned address. A volatile scalar access requires the element's natural alignment and makes one access at that width. The caller supplies live, sufficiently large storage with a valid representation and must respect aliasing. A raw pointer neither owns storage nor keeps its owner alive.

A field whose address escapes into raw code keeps its enclosing C representation in stable storage for the lifetime of the local. Taking its address requires a mutable, addressable Aura place. Mutating the field through the pointer and assigning to it through Aura address the same bytes. An extern pointer parameter accepts either a raw pointer or the existing synchronous list view. A returned raw pointer has the lifetime specified by the C library.

## Permission and Proof

Require the package's existing FFI grant for pointer operations, including casts in files without a mem import. The grant already covers the same operations through C. A lexical unsafe block could identify the individual sites that need review. Adding it would require checking effects across wrappers and specifying migration rules for existing mem and extern calls. Keep the capability boundary at the package for this change.

The verifier cannot assume an ordinary field retains its value across a write through an alias. Proofs that depend on raw memory or unmodeled aliasing cannot justify removing runtime checks.
