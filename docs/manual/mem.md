# Memory Module

The `mem` module reads and writes scalars at a raw address. It is for
device registers, memory shared with C, and the lock-free cells a kernel or
a driver needs. The raw-address functions work in hosted and
[freestanding](/manual/freestanding) builds. A freestanding target also
provides the machine operations below.
Hosted and freestanding builds use the same target memory ordering rules.

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

`N` is 8, 16, 32 or 64 for unsigned plain, volatile and atomic access.

| API | Signature | Result |
| --- | --- | --- |
| `mem.read_uintN` | `read_uintN(address: uint64) -> uintN` | The integer at `address`. |
| `mem.write_uintN` | `write_uintN(address: uint64, value: uintN) -> None` | Stores `value` at `address`. |
| `mem.volatile_read_uintN` | `volatile_read_uintN(address: uint64) -> uintN` | Reads once, exactly where the program says. |
| `mem.volatile_write_uintN` | `volatile_write_uintN(address: uint64, value: uintN) -> None` | Writes once, exactly where the program says. |
| `mem.atomic_load_uintN` | `atomic_load_uintN(address: uint64, order="seq_cst") -> uintN` | The value of the cell. |
| `mem.atomic_store_uintN` | `atomic_store_uintN(address: uint64, value: uintN, order="seq_cst") -> None` | Stores `value` in the cell. |
| `mem.atomic_swap_uintN` | `atomic_swap_uintN(address: uint64, value: uintN, order="seq_cst") -> uintN` | Stores `value` and returns the previous value. |
| `mem.atomic_add_uintN` | `atomic_add_uintN(address: uint64, value: uintN, order="seq_cst") -> uintN` | Adds `value`, wrapping, and returns the previous value. |
| `mem.atomic_compare_exchange_uintN` | `atomic_compare_exchange_uintN(address: uint64, expected: uintN, replacement: uintN, order="seq_cst", failure_order=…) -> uintN` | Stores `replacement` if the cell holds `expected`. Returns the previous value either way, so the exchange happened when the result equals `expected`. |
| `mem.fence` | `fence(order="seq_cst") -> None` | Orders every earlier memory access before every later one. |

The `order` argument takes a string literal. Loads allow `"relaxed"`, `"acquire"` or `"seq_cst"`. Stores allow `"relaxed"`, `"release"` or `"seq_cst"`. Read-modify-write operations allow all five orders, including `"acq_rel"`. A fence accepts all except `"relaxed"`. A compare-exchange failure order must permit a load and must be no stronger than the success order. Omitting it drops release semantics from the success order.

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
  every other atomic call on the same cell, on every core. Atomic operations default to sequentially consistent ordering. The address must be a
  multiple of `N / 8`.
- **`atomic_add` wraps.** It is the one place an Aura addition does not
  check for overflow, because a failed check could not undo the step.
- **Field addresses.** `mem.address_of(point.x)` returns a typed pointer to the field of a mutable C class. The pointer expires with its enclosing storage.
- **Verification.** `aura verify` treats a read as an unknown value of its
  type and does not model writes. A write to memory that an Aura value
  occupies is outside what a proof covers.

Compile-time queries accept C layout types. `mem.size_of(T)` returns the storage size, `mem.align_of(T)` the alignment, and `mem.offset_of(T, member)` the byte offset of an addressable member. Results are `uint64` constants for the selected target.

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

## Ports And Processor Registers

Select the target with `aura check --freestanding --target <name>` or
`aura build --freestanding --target <name>`. The package needs
`allow_ffi = true`. The processor enforces its privilege rules at run time.

On x86-64, write one byte to a serial port's transmit register like this:

```aura fragment
import mem

def put(byte: uint8) -> None:
    mem.out_uint8(0x3f8, byte)
```

| x86-64 operation | Signature |
| --- | --- |
| Read an I/O port | `in_uintN(port: uint16) -> uintN`, for N = 8, 16 or 32 |
| Write an I/O port | `out_uintN(port: uint16, value: uintN) -> None`, for N = 8, 16 or 32 |
| Read a model-specific register | `read_msr(index: uint32) -> uint64` |
| Write a model-specific register | `write_msr(index: uint32, value: uint64) -> None` |

Named registers have a `read_<name>() -> uint64` function. The registers
in the last column also have `write_<name>(value: uint64) -> None`.
For example, use `mem.read_mhartid()` on RISC-V or
`mem.write_vbar_el1(address)` on AArch64.

| Target | Read only in Aura | Read and write |
| --- | --- | --- |
| x86-64 | | `cr0`, `cr2`, `cr3`, `cr4`, `cr8` |
| AArch64 | `midr_el1`, `mpidr_el1`, `currentel`, `cntpct_el0`, `cntfrq_el0` | `sctlr_el1`, `tcr_el1`, `ttbr0_el1`, `ttbr1_el1`, `mair_el1`, `vbar_el1`, `esr_el1`, `far_el1`, `elr_el1`, `spsr_el1`, `cntp_ctl_el0`, `cntp_cval_el0` |
| RISC-V | `mhartid`, `misa`, `cycle`, `time` | `mstatus`, `mie`, `mip`, `mtvec`, `mscratch`, `mepc`, `mcause`, `mtval`, `medeleg`, `mideleg`, `sstatus`, `sie`, `sip`, `stvec`, `sscratch`, `sepc`, `scause`, `stval`, `satp` |

All three targets provide `mem.enable_interrupts()`,
`mem.disable_interrupts()` and `mem.halt()`, each returning `None`.
The first two control x86 IF, AArch64's IRQ mask or RISC-V's machine-mode
interrupt-enable bit. The last emits `hlt` on x86-64 or `wfi` on the other
targets. Execution can resume after that instruction.

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

The raw-address API works under `aura run` and in an ordinary `aura build`, with
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

- Port numbers are 16 bits. The largest single port transfer is 32 bits.
