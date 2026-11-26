# Systems Data Design Checkpoint

Proposed on 2026-11-24 for the integration branch. This document records the implementation target. The manual describes shipped behavior.

## Storage and Ownership

```aura fragment
mut ticks: uint64 = 0
mut ring: fixed[uint16, 4] = [0] * 4
```

A freestanding module can define a mutable cell of any type the profile can store inline. The initializer must determine its entire representation at build time. Start with typed literals, aggregate literals and their nested constructors. Adopt the shared constant evaluator once it lands. No initialization function runs before an export. Each definition has one address across all imports. Give it private module-qualified linkage, target alignment and writable storage. Use .bss for an all-zero representation and .data otherwise. Startup code supplies zeroed .bss. Freestanding callers provide synchronization across cores and interrupts.

Hosted tasks share a module namespace. Use an immutable binding to a sync handle for shared mutations. Reject a hosted module mut declaration with help showing a mutex or atomic binding. Replacing even a sync handle races with readers. A task-local binding would require storage on the logical task and initialization and cleanup across task suspension. Defer that separate feature.

A read into an owned value snapshots the cell according to existing copy and move rules. Never move an owned value out of a module cell and leave it uninitialized. A safe view cannot escape a module write, a call that could perform one, or reentry through an extern. Conservatively reject borrowing module storage when exclusivity cannot be proved. Treat field and element places as rooted in their enclosing cell. Raw pointers require the FFI capability and carry the caller's aliasing obligations.

## Inline Arrays

```aura fragment
def fill(a: mut fixed[uint16, 4]) -> None:
    a[0] = 12

def total(a: fixed[uint16, 4]) -> uint64:
    mut s: uint64 = 0
    for x in a:
        s = s + (x as uint64)
    return s
```

Use fixed[T, N]. The bracket syntax follows list[T] and C field array[T, N]. The separate name preserves both numerical Array[T] and the list semantics of existing array[T, N] fields. A fixed field in a C class represents T name[N]. N is a positive compile-time integer up to the existing C-field limit of 1048576. Check nested byte-size calculations against target limits. The element must have inline storage appropriate to the profile. C fields additionally require a C representation.

A list literal in a fixed destination must have the exact length. The repeat form [value] * N evaluates value once and requires a copyable element. Existing list expressions retain their meaning elsewhere. The storage contains consecutive elements with element alignment and no header or allocation. Parameters borrow the array for the call, shared by default and exclusive with mut. Iteration visits elements in order. len(a) is a constant. Indexes must be in the half-open interval from zero to N. A dynamic violation traps at the source index, using freestanding trap code 9. Elide checks for constant or range-proved indexes. Do not eliminate checks from unconstrained callers.

## Typed Pointers

```aura fragment
p = mem.address_of(ring[0])
p.offset(1).write(10)
byte_address = p as uint64
q = byte_address as ptr[uint16]
value = q.volatile_read()
```

ptr[T] holds a machine address. T can be a scalar with a memory representation, a C class or an inline fixed array of eligible elements. A pointer is a copyable value with equality and inequality for matching pointer types. Use 0 as ptr[T] for null. Use as between pointer types and between pointers and uint64. offset takes a signed element count and steps by the target size of T. read, write, volatile_read and volatile_write access the pointee. The caller guarantees a live allocation, sufficient size, validity of the stored representation, access rights and any required alignment. Volatile scalar operations must preserve access width. Aggregate volatile access cannot promise a single bus transaction.

mem.address_of takes a stable addressable place: module storage, a fixed array element or an addressable C field. Reject temporaries and bit-fields. Ordinary array indexing checks still apply. Taking a writable address must respect the binding's mutability and existing views. A pointer to local storage expires with the storage. Existing synchronous list-to-extern pointer conversions keep their ABI and lifetime. Gate raw operations by the declaring package's allow_ffi capability, including casts without a mem import. Extend mem with read_intN and read_floatN and their volatile counterparts. A lexical unsafe block would identify individual audit sites, at the cost of new syntax, effect tracking across wrappers, tooling and migration of mem and extern callers. Keep the package gate in this change.

## C Layout

```aura fragment
extern "C" packed align(8) class Header:
    kind: uint8
    flags: uint16 bits(3)

size = mem.size_of(Header)
alignment = mem.align_of(Header)
offset = mem.offset_of(Header, kind)
```

packed sets the member packing limit to one byte. align(N) supplies an explicit power-of-two aggregate alignment and rounds up the final size. Target ABI rules determine bit allocation, including storage units, padding and packing. Support named unsigned integral bit-fields with positive widths within the base type. Reject a bit-field as the operand of offset_of or address_of. Check a store fits the declared width, and preserve its neighbors. Read and write unaligned fields with their actual alignment. Every C union member begins at offset zero. Assignment to a member updates just the bytes of that member. size_of, align_of and offset_of are constants for the selected target, including in cross builds. Test byte representations and query results against C, including Windows layout. Keep the RISC-V restriction on passing C structs by value.

## Integers and Atomics

Integer methods rotate_left(k), rotate_right(k), count_ones(), leading_zeros(), trailing_zeros() and swap_bytes() operate on the receiver's fixed-width bit pattern. Rotations reduce a nonnegative count modulo the width. Counts return int64, including the full width for leading or trailing zeros of zero. Rotations and byte swaps return the receiver type. Permit int128 and uint128 inside freestanding code. Keep existing C export signature rules. Emit self-contained arithmetic on every target, including operations LLVM would otherwise lower to a compiler support library.

Extend mem atomic operations to uint8 and uint16. Keep sequential consistency as the default. Accept optional constant order names "relaxed", "acquire", "release", "acq_rel" and "seq_cst". Loads accept relaxed, acquire and seq_cst. Stores accept relaxed, release and seq_cst. Read-modify-write accepts all five. Compare-exchange accepts success and failure orders. A failure order cannot release or be stronger than success. When omitted derive failure from success, dropping the release part. Fences require acquire, release, acq_rel or seq_cst. No runtime string or dispatch belongs in the object.

## Proof and Evidence

Model pure integer operations with their exact fixed width. Fixed lengths and target layout queries are integer constants. Use known index ranges to discharge bounds checks only when the proof covers the access. Preserve checks when the verifier cannot model an expression. Treat raw reads, atomic observations and mutable module reads across writes or calls as fresh unknowns. Refuse proofs that require tracking raw aliases, device behavior, concurrent interleavings or an unmodeled memory write. Never reuse a fact about an addressable value across a possible clobber.

Test each item first through parser, checker and execution fixtures, C ABI comparisons and freestanding object inspection. Check symbol dependencies, sections and behavior on x86-64, AArch64 and RISC-V. Boot the kernel under QEMU and measure the freestanding routines against identical C and Rust workloads. Publish measured results after verifying the combined branch.

| Item | Decision | Reserved diagnostic codes |
| --- | --- | --- |
| Module storage | 0140 | AU2080, AU3030, AU4030 |
| Inline arrays | 0141 | AU2081, AU2082, AU3031, AU4031 |
| Raw pointers | 0142 | AU2083, AU2084, AU3032, AU4032 |
| C layout | 0143 | AU2085, AU2086, AU3033, AU4033 |
| Integer and atomic operations | 0144 | AU2087 through AU2089, AU3034, AU4034 |

Allocate only codes an implementation actually needs. Existing diagnostics for existing constructs retain their meaning.
