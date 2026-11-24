# ADR-0160: A freestanding build keeps a type inside itself in counted heap blocks

- Status: Accepted
- Date: 2026-11-24
- Builds on: ADR-0128 (allocation hooks), ADR-0129 (tuples and enums of words)
- Design checkpoint: [A Heap For Freestanding Builds](../40-freestanding-heap-design-checkpoint.md)

## Context

ADR-0129 keeps a tuple or an enum in machine words when every part is a
word. An enum with an `indirect` payload of its own type, such as a list
or a tree, has no fixed size, so the build still refused it. It is the
first value of the language that needs a heap block in a freestanding
build, and it needs no library of operations: only a block, a reference
count, and a way to drop what the block holds.

## Decision

- **A type inside itself becomes the address of a block.** When the pass
  of ADR-0129 meets a type while it is making the class for that same
  type, the field becomes a `uint64`. The block holds an 8-byte reference
  count and then the value's words, 8 bytes each. A field of a variant
  that is not the live one holds 0.
- **The block functions are written in Aura.**
  `std/freestanding_values.au` holds `value_box_new`, `value_box_retain`,
  `value_box_release`, and `value_box_free` over `mem.alloc` and
  `mem.free`, so they reach the allocation hooks of ADR-0128. The build
  compiles them into the object only when the program has such a type.
- **The pass makes five functions for each class that owns a block:**
  store a value into a block, load a copy out, drop a block's content,
  retain, and release. Retain and release walk the class's fields and
  count the blocks it owns. Release frees a block whose count reaches
  zero, after it releases what the block holds.
- **The emitter writes store and load.** They copy the class's words to
  and from the block at fixed offsets. `mem` has no typed reads for every
  scalar yet, and the copy needs no checks.
- **The pass follows the references the ownership pass already stated.**
  Before each `Retain` and `Release` of a value that owns a block it
  calls the class's function. A read of a part that owns a block is a new
  reference, so it is followed by a retain.
- **A part moved out is read as a part copied out.** The copy takes a
  reference, and the release of the whole drops the first. The same holds
  for a lend, which the hosted planner would not make for a class of
  words: the call gets a reference of its own, released after it.
- **`try` consumes a `Result` that owns a block.** Each path takes a
  reference to the part it keeps and releases the result. The error path
  releases what the ownership pass listed for it.
- **The last variant of a `match` is not tested.** The checker has seen
  the `match` cover every variant. A chain of tag tests does not tell the
  ownership checker that, so the last arm is a jump.
- **What the pass cannot count is refused as `AU2052`:** a write to a
  part of a value that owns a block, and a union one of whose members
  owns a block.

## Text

`str` uses the same blocks and the same counting.

- **A `str` is a class of one word, the address of a text block:** a
  reference count, the length in bytes, the bytes, and a zero byte after
  them. The class needs drop, as `str` does, so every `Retain` and
  `Release` the ownership pass stated counts the block.
- **The text functions are Aura source** in `std/freestanding_text.au`,
  compiled in only when the program holds a `str`. Each takes text as
  the address of its first byte and a length.
- **A literal is not a block.** An operation reads a literal's bytes in
  the program's data, through one call the emitter writes. A literal
  becomes a block only where it becomes a value: assigned, returned, put
  in a class, a tuple, or a variant, or given to a function. A function
  that borrows its parameter leaves the block with the caller, which
  releases it after the call.
- **The pass rewrites** `+`, the six comparisons, `len`, `byte_len`,
  `starts_with`, `ends_with`, and `contains`. Any other text operation
  is still refused.
- **An f-string appends its parts one by one.** Text is read in place,
  and an integer or a `bool` is written into a block by a library
  function. A float or a part with a format spec leaves the f-string
  refused.
- **A `str` value given to a foreign function crosses as the address of
  its bytes** for a `cstr` parameter, and as the address and the length
  for a `str` parameter, as a literal already did.
- **Assertion text is dropped before the pass**, so an `assert` with a
  message allocates nothing.

## Lists

- **A `list` of a scalar type is a class of one word, the address of a
  header block:** a reference count, the length, the capacity, and the
  address of a buffer of one 8-byte word for each element. The header
  never moves, so a `mut` parameter and its caller name the same list.
- **The list functions are Aura source** in `std/freestanding_list.au`.
  A slot function checks the index and returns where the element lies,
  and the emitter writes the store or the load of the element there, as
  it does for a box.
- **The pass rewrites** list literals, `append`, `len`, `is_empty`,
  `clear`, and checked and unchecked element reads and writes. A `for`
  loop over a list is already those in MIR.
- **A fact that an index is in range is dropped with the read it
  served.** The fact also showed that stepping the index cannot
  overflow, so that step is checked again.
- **An index out of range is a failed assertion in the library**, trap
  code 3 with the library's file name. The position in the program and
  a code of its own are a later step.
- A list of any other type, and every other list operation, is still
  refused.

## Consequences

- A freestanding program builds lists and trees from enums, holds them in
  classes, tuples, and results, and every block is freed once. A linked
  test counts the calls of the hooks.
- A program with no such type gets none of these functions and still
  needs no hook.
- Counts are not atomic. A freestanding program has no tasks.
- Release of a long list recurses once for each link.
- A part moved out costs a retain and a release where a hosted program
  moves a pointer. A function that rebuilds a tree on the way out of a
  recursion copies the nodes it passes.
- A request the heap turns down is a failed assertion, trap code 3. A
  code of its own is a later step.
- Every literal that becomes a value is copied into a new block. A
  block the program's data holds, never counted, is a later step.
- `str | None` is refused with the other unions of values that own a
  block.
- An f-string of n parts allocates up to 2n blocks and frees all but
  the last. One block sized in advance is a later step.
- Float formatting, indexing, `bytes`, and `list` are the next steps.
