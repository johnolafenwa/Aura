# Memory Module

The `mem` module reads and writes scalars at a raw address. It is for
device registers, memory shared with C, and the lock-free cells a kernel or
a driver needs. Each function compiles to one machine instruction, in
hosted and [freestanding](/manual/freestanding) builds alike.

```aura fragment
import mem

UART: uint64 = 268435456

export "C" def put(byte: uint8) -> None:
    mem.volatile_write_uint8(UART, byte)
```

An address is a `uint64`. Aura does not check it. A read or write through
an address that is not valid for that access has undefined behavior, as in
C, and it can break the guarantees of every other part of the program. For
that reason a module may import `mem` only in a package whose `Aura.toml`
sets `allow_ffi = true`, the permission that [foreign
functions](/manual/ffi) need.

## Public API

`N` is 8, 16, 32, or 64 for the plain and volatile functions, and 32 or 64
for the atomic ones.

| API | Signature | Result |
| --- | --- | --- |
| `mem.read_uintN` | `read_uintN(address: uint64) -> uintN` | The integer at `address`. |
| `mem.write_uintN` | `write_uintN(address: uint64, value: uintN) -> None` | Stores `value` at `address`. |
| `mem.volatile_read_uintN` | `volatile_read_uintN(address: uint64) -> uintN` | Reads once, exactly where the program says. |
| `mem.volatile_write_uintN` | `volatile_write_uintN(address: uint64, value: uintN) -> None` | Writes once, exactly where the program says. |
| `mem.atomic_load_uintN` | `atomic_load_uintN(address: uint64) -> uintN` | The value of the cell. |
| `mem.atomic_store_uintN` | `atomic_store_uintN(address: uint64, value: uintN) -> None` | Stores `value` in the cell. |
| `mem.atomic_swap_uintN` | `atomic_swap_uintN(address: uint64, value: uintN) -> uintN` | Stores `value` and returns the previous value. |
| `mem.atomic_add_uintN` | `atomic_add_uintN(address: uint64, value: uintN) -> uintN` | Adds `value`, wrapping, and returns the previous value. |
| `mem.atomic_compare_exchange_uintN` | `atomic_compare_exchange_uintN(address: uint64, expected: uintN, replacement: uintN) -> uintN` | Stores `replacement` if the cell holds `expected`. Returns the previous value either way, so the exchange happened when the result equals `expected`. |
| `mem.fence` | `fence() -> None` | Orders every earlier memory access before every later one. |

## Rules

- **Byte order.** An integer is read and written in the byte order of the
  machine. Every supported target is little-endian.
- **Plain access.** `read_uintN` and `write_uintN` accept any address,
  aligned or not. The compiler may merge two reads of one address, drop a
  write that a later write replaces, and move an access across other code.
  Use them for ordinary memory.
- **Volatile access.** Each call is exactly one access of exactly `N` bits,
  in program order, and the compiler never merges, drops, or splits it.
  Use it for device registers. The address must be a multiple of `N / 8`.
- **Atomic access.** Each call is one indivisible step with respect to
  every other atomic call on the same cell, on every core. All atomic
  functions use sequentially consistent ordering. The address must be a
  multiple of `N / 8`.
- **`atomic_add` wraps.** It is the one place an Aura addition does not
  check for overflow, because a failed check could not undo the step.
- **Field addresses.** `mem.address_of(point.x)` returns a typed pointer to the field of a mutable C class. The pointer expires with its enclosing storage.
- **Verification.** `aura verify` treats a read as an unknown value of its
  type and does not model writes. A write to memory that an Aura value
  occupies is outside what a proof covers.

## Typed Access

Use `ptr[T]` to keep the memory representation with the address. The package must opt in to FFI even if the file uses only pointer casts and methods.

```aura fragment
p = address as ptr[float32]
p.write(-1.5)
next_value = p.offset(1).read()
raw_bytes = p as ptr[uint8]
address_again = raw_bytes as uint64
```

| Operation | Result |
| --- | --- |
| `mem.address_of(place)` | Address a mutable module binding, a fixed array element, or an addressable C field. Selecting an array element checks its index. |
| `p.read()`, `p.write(value)` | Read or write one value of the pointee type. |
| `p.volatile_read()`, `p.volatile_write(value)` | Read or write at the program point. A scalar uses one naturally aligned access. |
| `p.offset(count)` | Advance by a signed number of elements. The unit is the size of the pointee. |
| `p as ptr[U]` | Interpret the same address as a pointer to U. |
| `p as uint64`, `address as ptr[T]` | Convert between an address word and a typed pointer. |

A pointer to `fixed[T, N]` reads and writes a complete inline array. For example, `p.offset(1)` advances past all `N` elements, including padding in the element type. A pointer to an `extern "C"` class uses the C layout of the whole record. Ordinary accesses accept unaligned addresses, including fields in packed records. Aggregate volatile operations access each byte in order. Each volatile scalar operation requests the width of the scalar, including `int128` and `uint128`. Whether the hardware implements that width in a single bus transaction depends on the target.

Use `0 as ptr[T]` for null. Comparing pointers of the same type with `==` or `!=` compares their address words. The caller is responsible for the lifetime, alignment and access rights of a pointer obtained through FFI or `mem.address_of`. An address into a local expires when its storage goes out of scope. A bit-field shares its storage unit with other members and has no pointer.

The signed forms `mem.read_intN` and `mem.write_intN` use 8, 16, 32 or 64 bits. The floating forms `mem.read_floatN` and `mem.write_floatN` use 32 or 64 bits. Prefix either form with `volatile_` for a volatile access. A floating read interprets the stored bits in the target's floating-point representation.

## A Spin Lock

```aura fragment
import mem

def acquire(lock: uint64) -> None:
    while mem.atomic_compare_exchange_uint32(lock, 0, 1) != 0:
        pass

def release(lock: uint64) -> None:
    mem.atomic_store_uint32(lock, 0)
```

## Hosted Programs

The module works under `aura run` and in an ordinary `aura build`, with
the same results. An address there comes from C:

```aura fragment
import mem

extern "C" def malloc(size: uint64) -> uint64
extern "C" def free(address: uint64) -> None

def main() -> None:
    block = malloc(16)
    mem.write_uint32(block, 7)
    print(mem.read_uint32(block))
    free(block)
```

## Current Limits

- No atomic access to 8-bit and 16-bit cells.
- One ordering, sequentially consistent.
- No inline assembly and no port I/O instructions. On x86-64, reach I/O
  ports through an `extern "C"` function written in assembly.
