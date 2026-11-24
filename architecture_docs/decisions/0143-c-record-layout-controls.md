# ADR 0143. C Record Layout Controls

## Decision

An `extern "C" class` can set the packing limit to one byte with `packed`, raise its alignment with `align(N)`, and declare an unsigned integral bit-field with `bits(N)`. These names are contextual. A field declared with `bits` has no independent address. Reading it returns its unsigned value. Assignment checks its width and preserves surrounding bits.

The layout descriptor retains member order, explicit alignment and bit allocation. ELF and Darwin allocate consecutive bit-fields while respecting their base-type allocation boundaries in naturally aligned records. Microsoft targets reserve a whole base-type allocation unit and start a new unit when the base width changes. Packing suppresses member padding. The record alignment applies to its tail padding and array stride.

Use `mem.size_of(T)`, `mem.align_of(T)` and `mem.offset_of(T, member)` for compile-time layout constants. The last form requires an addressable member. Queries return `uint64` and fold before backend selection. The verifier sees integer constants.

Assignment to an `extern "C" union` member replaces exactly that member's bytes. Reading another member interprets the resulting bytes in its declared type. There is no discriminant. An assignment preserves any bytes beyond the member. Existing constructors, factory methods and copied property reads keep their behavior.

## Consequences

A direct field store uses the field's actual alignment. Bit-fields use masks restricted to the bytes the field occupies. Pointers to a C record, record copies and indirect C call results require the aggregate's explicit alignment. On System V x86-64 an unaligned member makes a small struct a memory argument or result. The hosted interpreter must describe that class to libffi as well as providing the true size and alignment.

System V x86-64 callers must realign the call stack when passing an over-aligned record by value. The interpreter caches an LLVM forwarding function for each such foreign signature and target address. Its libffi entry takes record pointers, and its outgoing call uses the true C stack alignment. AArch64 and Windows pass an aligned private copy by address when their ABI classifies the argument in memory.

The C comparison fixture checks packed field offsets, bit-field contents, aligned arguments and unaligned by-value returns. Build tests exercise layout constants for all three bare-metal targets.
