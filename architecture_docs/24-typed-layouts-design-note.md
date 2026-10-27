# Typed native layouts: design note

Status: **Design for Phase 2 of the Rust-speed plan.** The owner approved
the plan's decisions on 2026-10-26. Step 2a shipped with this note, and
steps 2b and 2c follow it.

Date: 2026-10-26. Source baseline: Aura 0.7.1 preview (main @ f517376).

Native code keeps every non-scalar value in a boxed, dynamically typed
`Value`: a 192-byte `OpaqueValue` box, class fields in a
`BTreeMap<String, Value>`, enum payloads in a `Vec<Value>`. Reading a field
of a class held in a list clones the class into a new box and then reads the
field, about 4,850 instructions where Rust spends 3. Phase 2 gives each
concrete type a fixed native layout, as Rust does, so reads borrow in place.
Boxed `Value` stays for `aura run`, host calls, and FFI.

The plan's five decisions for this phase:

1. **Typed layouts** replace boxed values in native code.
2. **Class instances live inline in their owner.** A `list[Entry]` is one
   buffer of `Entry` structs. A move copies bytes.
3. **Generic user functions are specialized** for each concrete type at
   build time. Runtime collection helpers take a layout descriptor, and
   generated code inlines index, length, push, set, and iteration.
4. **The compiler adds the indirection on a recursive edge itself.** No
   language change.
5. **A per-task ledger of heap allocations** frees a failed or cancelled
   task's memory. Resources keep their own cleanup records.

This note covers the layout plan, the converter boundary, how lent pointers
stay valid, drop code, and the allocation ledger.

## The Layout Plan

[`crates/aura-core/src/layout.rs`](../crates/aura-core/src/layout.rs)
computes layouts. The compiler and the runtime both call it, so they agree
on every offset without sharing a table.

| Type | Layout |
| --- | --- |
| `bool`, integers, floats, `Duration` | A scalar of its width. `intsize` and `uintsize` are a pointer wide. `Duration` is sixteen bytes of signed nanoseconds. |
| `None` | Empty. |
| A class or tuple | A struct. Each field sits at the next offset its alignment allows, in declaration order. The size rounds up to the largest alignment. |
| An enum or union | A tag, then a payload area. Each variant lays its payloads out as a struct at the start of the area, so variants overlap. |
| `str`, collections, resources, tasks, queues, callables | A handle: one pointer to a box. |
| A type that names a type parameter | A handle. Step 2b copies generic functions for the concrete types their calls use, so only code it cannot copy keeps such a type. |

The tag is the variant's declaration index, or a union member's index in
canonical order, in the smallest unsigned width that holds every tag. That
is the rule the union plans of
[ADR-0052](decisions/0052-anonymous-closed-union-types.md) (A9) already use, so a
union's tags do not change when step 2g moves unions onto layouts.

Examples, from the layout tests:

```text
class Point:            Point: 16 bytes, align 8
    x: float64            x at 0, float64
    y: int32              y at 8, int32

enum Tree:              Tree: 24 bytes, align 8
    Leaf(int64)           tag at 0, one byte
    Node(Tree, Tree)      Leaf: int64 at 8
                          Node: pointer at 8, pointer at 16
```

### Recursion

A type that holds itself needs a pointer somewhere (decision 4). The
builder finds the classes and enums that reach each other through fields,
payloads, tuple elements, and union members: the strongly connected
components of the graph of instantiated types. Inside the definition of a
type in a component, each place that holds a type of the same component
holds a pointer to a heap block with that value, as `Box` does in Rust.
Every other place holds the type inline.

The components depend only on the program, so a type's layout never depends
on which type a walk started from. `Tree` above holds `Tree` behind a
pointer whether the program first mentions `Tree` or a class that holds a
`Tree`. A tuple or union inside a recursive definition that holds a member
of the cycle gets its own storage layout with the pointer, and everywhere
else the tuple keeps its plain layout.

### Budget

A class or enum that reaches more than 512 instantiated classes and enums,
itself included, or one whose type arguments nest more than 32 deep, stays a
handle. Only a type that names itself with an ever larger type argument
reaches either limit, as in `class Grow[T]: next: Grow[list[T]] | None`. The
count covers every type reached, including types an earlier walk laid out,
so the result does not depend on order either.

### Plans and Descriptors

[`mir_layout.rs`](../crates/aura-compiler/src/mir_layout.rs) builds a
module's `LayoutPlan`: the layout of every type its functions' parameters,
locals, and results, its class fields, its enum payloads, and its constants
use. Code generation reads the plan from step 2c.

A type's **descriptor** is every layout a value of the type reaches, as
the compiler's builder computed them: each layout's type, size, alignment,
and kind, with the offsets of its fields and payloads. Each layout comes
after every layout it holds in place, and only a heap pointer may name a
later one. The runtime reads the layouts back and runs no builder, so a
program does not link it.

The runtime checks every descriptor before it uses one: each rule
`LayoutTable::validate` checks (sizes are multiples of alignments, fields
fit and do not overlap, a tag has aligned room before the payload, every
id names a layout), and the order rule, so no value holds itself without a
heap pointer between. A descriptor is static data that the compiler wrote
next to the code that reads the same offsets. Step 2a's descriptors carried
the class and enum definitions instead, and the runtime ran the builder
again. That kept the runtime from reading a layout the builder would not
make, but it linked the builder into every program with a class: without
it, the reference agent is 88 KB smaller.

A test lowers every run-pass fixture (672 programs, 34,025 distinct types),
builds its plan, validates the table, and checks that each type's
descriptor reads back as the same layout.

## The Converter Boundary

[`native_typed_layout.rs`](../crates/aura-runtime/src/native_typed_layout.rs)
moves values between a layout and a boxed `Value`:

| Operation | What it does |
| --- | --- |
| `write` | Checks the whole value against the layout, then writes it into uninitialized memory. A value of another shape is refused before a byte is written. |
| `read` | Copies the value out and leaves it in place. |
| `take` | Moves the value out. The memory is dead afterwards. |
| `drop_in_place` | Drops the value where it lies. |
| `clone_into` | Writes an independent copy. |

The converters interpret the layout table rather than being generated per
type, which keeps binaries small. From step 2c they run where a typed value
meets code that reads `Value`: runtime helpers until they take descriptors,
host calls, FFI, and `aura run` interop. Step 2j removes every conversion
from the hot paths of the 30 benchmark programs. Generated code names a
layout by a cell, a data object the runtime fills with the decoded layout
when the program starts (see "Plain Records").

A value read back carries everything a box generated code makes would
carry: integer kinds, tuple element types, the union type, and the type
arguments of a generic class or enum. A handle written into a layout is a
new box tagged with the handle's static type.

## Plain Records

Step 2c starts with the values a layout holds without owning anything:
plain records. A class is plain when every field is a scalar or another
plain class, which is when the emitter already keeps a value of it in a
local as one word per field. Its layout holds no handle and no heap block,
so the bytes are the whole value: copying them copies the value, and
dropping them does nothing.

- **Lists keep them in place.** `ListElements::Typed` holds the values of
  one registered layout back to back, each padded to whole 8-byte words. A
  list takes the form when every element fits the layout and leaves it for
  `Value`s when one does not, as the scalar forms do, so a program cannot
  tell which form a list uses. The list view's access code names the
  layout.
- **Generated code reads and writes them there.** Indexing a list of
  records loads each field from its offset in the element, after one check
  of the access code and, unless the loop proved the index, one bounds
  check. Appending stores the fields past the end while the storage has
  room, and replacing an element stores them in place when no other list
  shares the storage. Every other case is one runtime call that copies the
  record's bytes.
- **Crossing into a box is one call.** Boxing a record, reading one out of
  a box, and reading a record field of a boxed instance each pass the
  record's bytes through a stack slot. Before 2c each was one call per
  field, and a field read cloned the record into a new box first.
- **Startup registers the layouts.** Each record type the code uses gets a
  cell and an access code. Before the first call, the main wrapper passes
  every descriptor to `aura_direct_register_plain_layouts`, which registers
  each layout under its code. A registered layout keeps its code, and a
  code names one layout, so a list the runtime builds keeps a record type
  in the form generated code checks for.

A record with a union field keeps its boxed paths, because the emitter's
words for a union, a tag word and payload words, are not the union's
layout bytes. Tuples, records held in classes that own handles, and records
in enum payloads also stay boxed: their owners become structs in steps 2e
and 2f, and the records move inline with them there. The tuples of the
cross-language benchmarks all hold `str`, so none is plain.

## Lent Pointers

A read of a typed value borrows its bytes in place. MIR already states every
loan, and the loan validator already refuses anything that could move or
free the borrowed bytes while a loan lives, so the native code needs no new
rule. There are three kinds of read.

**Reads inside one instruction.** `ps[i].x` computes the element's address
and loads the field in one MIR instruction. Nothing runs between the address
and the load.

**Lends to a call.** Today `Lend` moves a field or element into a temporary
for one call and `EndLend` moves it back. With layouts, a shared lend passes
the address of the place instead, and a mutable lend passes the address the
callee writes through, so `EndLend` copies nothing. The lending pass lends a
place only when the call cannot reach it another way: no other operand
overlaps it, no operand reaches a different root through a field or an
element, and no argument's type can hold a class instance or a callable that
aliases the owner. The callee therefore cannot move, free, or resize what
the address points into.

**Views and element loans.** `view name = users[0].name` and the loans MIR
begins for element reads compute the address once, when the loan begins,
after the bounds check. While the loan lives, the validator refuses every
overlapping mutation, move, rebind, and cleanup, and any structural change
to the collection, such as `append`, `remove`, or `clear`. A buffer cannot
grow or shrink under the address, so it stays valid until the loan ends.

Copy-on-write keeps sharing safe. Two lists may share one buffer. A write to
a shared buffer copies it first, and the write happens only outside every
loan of that list, so an address into the old buffer stays valid for the
other list's loans. A value another task can see is shared, and shared
buffers are never written in place (step 2i).

An address never outlives its owner's frame. A returned view points into
the caller's argument, which the caller's frame keeps alive, and MIR's
returned-loan rules end the view before that argument can change.

## Drop Code

`LayoutTable::needs_drop` says whether a layout holds a handle or a heap
pointer anywhere. A layout that holds neither is plain bytes: a copy is a
`memcpy`, and dropping it does nothing. Step 2e generates one drop function
per layout that needs one:

- a struct drops each field that needs it;
- a tagged value reads the tag and drops the active variant's payloads;
- a handle releases its box;
- a heap pointer drops the block's value and frees the block.

A move copies the bytes and leaves the source dead. MIR already tracks moved
places, so nothing drops the source again. Collection helpers receive the
element layout's drop function in their descriptor. The runtime's
`drop_in_place` interprets the same table for memory the runtime owns, and
it is the reference the generated functions are tested against.

## The Allocation Ledger

Today each task's ledger links every box generated code owns, and a trap or
a cancellation releases them all. Typed values have no box per value, so
step 2h adds a ledger of heap blocks (decision 5), and the two ledgers
together own every value.

- **Blocks.** Every heap block, whether an indirect target, a list buffer,
  or a queued value, comes from one allocator function. A block has a small
  header with an intrusive link into its task's block ledger and its
  layout. Freeing a block unlinks it.
- **One owner per value.** A box that a frame's locals hold, inline values
  on the stack included, stays in the box ledger as it is today. A box that
  a heap block holds is untracked: the block owns it. The converters already
  follow this rule. Each heap block is in the block ledger, including blocks
  that other blocks point to.
- **Failure.** When a task traps or is cancelled, its frames' cleanup runs
  first, as today, and closes files and sockets through their own records.
  Then the box ledger releases its boxes. Then the block ledger visits each
  block, releases the boxes it holds directly, and frees it. A block's
  pointers to other blocks are not followed, because those blocks are in
  the ledger themselves, so nothing is freed twice and no landing pads are
  needed.
- **Moves between tasks.** A value sent to another task moves its blocks to
  the receiver's ledger (step 2i).

In step 2a the converters allocate blocks with the system allocator through
`allocate_block`, so step 2h changes one function.

## Steps

| Step | Adds |
| --- | --- |
| 2a | The shared layout builder, `LayoutPlan`, descriptors, the runtime converters, and their tests. No change to generated code. |
| 2b | Specialized generic user functions, so fewer types name a type parameter. See "Specialized Generic Functions" in [06-mir.md](06-mir.md). |
| 2c | Plain records in their layouts: lists keep them in place, generated code reads and writes list elements where they lie, and boxing, unboxing, and field reads of boxed owners take one call. See "Plain Records". Tuples, fields of owning classes, and enum payloads move inline with their owners in 2e and 2f. |
| 2d | `list[T]` as a buffer of inline `T`, with inline index, length, push, set, and iteration, and lent element reads. |
| 2e | Classes as structs inline in their owners, field reads that lend, writes in place, and generated drop functions. |
| 2f | Enums as a tag and an inline payload. `match` borrows payloads, and `match own` moves them. |
| 2g | Unions on layouts, replacing the separate union plans. |
| 2h | Ownership off boxes and the block ledger. |
| 2i | Typed values across tasks and queues. |
| 2j | Conversions only at host calls, FFI, and `aura run`. |
| 2k | Deleting the boxed paths that layouts replace. |
