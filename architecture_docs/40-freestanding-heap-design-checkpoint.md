# A Heap For Freestanding Builds: Design Checkpoint

Status: **Step 1 accepted 2026-11-24 as ADR-0128.** Steps 2 and 3 are
proposals. The choice in section D is the owner's call.

A freestanding program holds only machine words (checkpoint 28). This
checkpoint settles how such a program gets memory at run time, and lays
out how the heap values of the language can follow: `str`, `list`,
`dict`, `set`, enums with payloads, classes kept in a box, closures, and
trait objects.

## Starting Point

- `aura build --freestanding` writes one relocatable object that names
  only the trap hook, the program's own foreign functions, and four C
  memory routines it defines weakly. It links nothing.
- The `mem` module reads and writes integers at a `uint64` address.
- Hosted native code keeps every heap value behind a handle to an
  `OpaqueValue`: a 144-byte box around a dynamically typed `Value`, with
  reference counts, a list view generated code reads in place, and links
  to the ledger of the task that owns it. About 600 runtime functions
  serve those boxes. The runtime needs threads, files, and an allocator.
- The emitter knows five kinds of value: a scalar, a class of scalars, an
  inline union, an inline callable, and an opaque handle. Every heap type
  is an opaque handle.
- MIR states ownership: each `Retain`, `Release`, and lend is an
  instruction. An emitter carries them out.

## A. Where Memory Comes From

- **A1. Three hooks with sized frees (default).** `aura_alloc(size,
  align)`, `aura_free(address, size, align)`, and `aura_realloc(address,
  old_size, align, new_size)`. The environment may define them. Null
  means no memory.
- A2. One hook, `aura_alloc`, and no free. Enough for a boot stage, and
  wrong for anything that runs for long.
- A3. An allocator object passed to each function that allocates. It
  keeps globals out, and it puts a parameter on every function that
  touches a list.

## B. The Default Allocator

- **B1. Aura source compiled into the object (default).** The allocator
  is written in the freestanding subset over `mem`. The build adds its
  functions to a program that allocates and declares no hook, and defines
  the hooks as weak symbols over them. The output is still one object.
- B2. A `no_std` Rust crate built as an archive for each target. It
  needs target packs for three bare-metal targets in every release and a
  second file in every link.
- B3. No default. Every program brings its own. A first program then
  starts with an allocator instead of its own work.

The allocator takes its memory from the program:
`mem.heap_add_region(start, size)`. Its state is zeroed bytes in the
object.

## C. The Allocator's Shape

- Blocks are multiples of 16 bytes with one header word. A free block
  also holds two list links and its size in its last word.
- 96 free lists: one for each size from 32 to 1040 bytes, then one for
  each power of two. Two bitmap words name the lists that are not empty.
- A freed block joins its free neighbors at once. `realloc` shrinks in
  place and grows into a free neighbor.
- An alignment above 16 is met by taking a larger block and freeing what
  lies before the aligned address.
- One spin lock guards every call.

## D. Heap Values Of The Language

The owner's call. Two routes lead to `str`, `list`, and the rest in a
freestanding program.

- **D1. A lean value layer written in Aura, with a rewrite of MIR for
  freestanding builds (recommended).** In a freestanding build, a pass
  turns each operation on a heap value into a call of a function of a
  small Aura library over the hooks: a text is a counted block of bytes, a
  list is a header and a buffer, a boxed value is a counted block with a
  drop function. The emitter then sees only words and calls. Each value
  costs what it would in C. The work is a second implementation of value
  semantics, checked against the hosted one by running the same programs
  both ways.
- D2. Split the hosted runtime into a core without an operating system
  and the rest, and link the core into freestanding programs. One
  implementation serves both. The split moves most of the two largest
  files of the runtime while the speed work is changing them, and the
  result keeps the 144-byte box and the task ledger on bare metal.
- D3. Stop at the hooks. With typed pointers and a destructor hook for
  classes, both planned, a program writes its own vector and text types
  as Aura classes. The built-in types stay out of freestanding programs.

Out of memory in a heap value of the language calls the trap hook with a
new code, 9. A program that must survive that uses `mem.alloc`.

The owner chose D1 on 2026-11-24.

## Steps

1. Hooks, `mem.alloc`, `mem.free`, `mem.realloc`, the region functions,
   and the default allocator (ADR-0128).
2. Tuples, enums, and `Result` whose parts are all words, kept in machine
   words with no heap (ADR-0129). Done.
3. Boxed values that need no library of operations: an enum that holds
   itself through an `indirect` payload, in a block with a reference
   count (ADR-0160). Done. A union that holds one and a write to a part
   of one are still refused.
4. `str`, `bytes`, and `list`. Then `dict` and `set`. Then closures and
   trait objects.

## Proof

- Compiler tests build each target and read the object's symbols: weak
  hooks when the program allocates, none when it does not, and none when
  the program declares a hook.
- The allocator runs as an ordinary program under the interpreter with a
  function that walks the heap and checks every rule.
- On Linux, objects link with a C harness and run a random workload of
  200,000 steps through the hooks, and 20,000 steps with the heap walked
  every 50.
- The example kernel keeps a growing block on the heap and boots under
  QEMU on RISC-V and AArch64.
