# Rust Speed Step 5a: Repeated Type Tags

Date: 2026-10-29

## Problem

After Phase 4, `regex_match` and `interpreter` spent a large share of
their instructions applying runtime type tags. A value whose static type
names a generic gets its tag through a tag cell in generated code
(`aura_direct_tag_value_type_cell`). The cell caches the interned tag,
but every call still entered the runtime, took the value's name, compared
it, and stored it, even when the box already carried that tag. Methods
also took `self` tagged on every call, because the shapes analysis only
left a function's parameters untagged when the function had no receiver.

## Changes

- `OpaqueValue` gains `shape_tag`, the tag last applied through a cell.
  `aura_direct_tag_value_type_cell` sets it. `refresh_list_view`, which
  runs after every write that may change the value's shape, clears it,
  and so does clearing the value's runtime name.
- `DirectOwnershipLayout` exposes `shape_tag_offset`, and the layout
  fingerprint includes it.
- `emit_runtime_type_tag` loads the cell's tag and, when it and the value
  are both non-null, compares it with the box's `shape_tag`. Equal tags
  skip the call.
- `native_codegen_shapes.rs`: a method reached only by named or statically
  dispatched calls (`module.borrowing`) and never used as a value treats
  `self` as written instead of excluded, like any other parameter, so
  `self` is not tagged on entry.

## Measurements

Instructions at check size (callgrind), 0.9.0 release prefix against this
step's release prefix:

| Program | 0.9.0 | 5a | Change |
| --- | ---: | ---: | ---: |
| regex_match | 185,128,545 | 166,824,956 | -9.89% |
| interpreter | 30,735,591 | 26,513,081 | -13.74% |
| spreadsheet | 11,251,714 | 11,094,655 | -1.40% |
| sql_engine | 621,723,160 | 615,623,928 | -0.98% |
| binary_trees | 4,195,916 | 4,283,928 | +2.10% |
| json_roundtrip | 364,745,349 | 366,593,673 | +0.51% |
| template_render | 15,171,940 | 15,243,295 | +0.47% |
| graph_paths | 34,263,085 | 34,406,147 | +0.42% |
| docdb | 32,117,320 | 32,248,005 | +0.41% |
| mini_browser | 394,225,221 | 394,190,799 | -0.01% |

All outputs match. The step's gate was regex_match -20%; it is missed.
The rest of regex_match's cost is instance field reads and writes and
stores through `mut self`, which step 5e takes on. `binary_trees` pays
for the larger box: `shape_tag` adds 8 bytes, and the box's 16-byte
alignment adds 8 more, so a box grows from 192 to 208 bytes.

A list that is pushed to between tags is retagged in full each time,
since the push clears the remembered tag.
