# Call frame cost

Date: 2026-11-11. Asked for by John: close the gap the element method call
work left, where `element_mut_call` and `element_plain_mut_call` ran at 7.8
and 4.0 times Rust, outside the bound of 3.

## What the instructions were

Measured with callgrind on a release build at `d37640c`.

- `Tally.bump` (two integers) ran 41 instructions plus 8 in
  `aura_direct_frame_stack`: the frame push and pop, the check for a
  write-through sink, and the registers saved around those calls. The
  arithmetic is 4. The caller's loop ran 14.
- `Counter.bump` (a `str` field, so a boxed receiver) ran 46 plus 8. The
  caller ran 40, of which 18 were two checks of the box's type tag, one
  before and one after the call.
- LLVM did not inline either method, because each had a frame push with
  runtime calls on its cold paths.

## Work completed

- A leaf with a `mut self` receiver or a `mut` parameter gets a second
  body, `aura_fn_<name>__leaf`, with no frame record and no sinks. The
  first body stays for calls that hand a sink over and for calls the
  runtime enters. A direct call picks the second body when no sink can
  follow any of its `mut` arguments, which is known when the call is
  compiled, and then emits no sink code at all.
- A method of a class kept in a box is a leaf when every mention of `self`
  is a scalar field. This also makes getters on such classes frameless.
- A frameless leaf of at most 8 blocks and 32 MIR instructions that only
  branches forward is marked for inlining. A leaf with a loop is not:
  inlining `generator_next`'s loop into its caller cost it 2 instructions
  per item.
- A call of a method that only touches scalar fields of its boxed receiver
  tags neither the receiver nor the box it hands back.

## Results

| probe | Rust | before | after | after / Rust |
| --- | ---: | ---: | ---: | ---: |
| `element_mut_call` | 12.0 | 94.1 | 34.0 | 2.8x |
| `element_plain_mut_call` | 16.0 | 64.1 | 26.0 | 1.6x |

No other operation probe changed.

## Tests

- Run fixture `leaf_method_calls` and the failing runs
  `leaf_method_overflow` and `leaf_plain_method_overflow`, whose call
  chains still name the method. Each prints the same on both backends, and
  the first is clean under valgrind natively.
- Library tests for which functions get a second body, which are inlined,
  and that the second bodies are in the object.

## Left open

- A function that is not a leaf still pays about 30 instructions a call for
  its record. The stack accessor is a call of its own (8 instructions);
  reading the scheduler slot in generated code would remove it.
- After a `mut self` call on a boxed element the caller still checks that
  the slot holds the box and nothing shares it (5 instructions).
- A field-only method reads its receiver's layout code once per field
  access.
