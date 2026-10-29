# Rust Speed Step 4a: The Call Path

Date: 2026-10-28

## Problem

Every Aura call pushes its function's descriptor on the task's frame
stack and pops it on return, so a trap can name the calls it happened in.
Generated code did both inline, but each checked two header words (the
limit or floor, and the flags) and LLVM reloaded the stack's length after
each push. A small function called from a loop cost 37 instructions per
call in the new `call` probe, where Rust's inlined call costs nothing.

Most of that was not the frame record. In a module large enough to split
into parallel codegen units (11d), every function body imports every
function of the module, and the partitioner linked units by those
imports. Every function became a hidden symbol that other units might
call, so LLVM no longer inlined a small function into its only caller.

## Changes

- `DirectFrameStack` keeps `limit` at 0 and a new `pop_floor` at
  `usize::MAX` while its flags are set, and at their real values
  otherwise. A push checks `len < limit` and a pop checks
  `len > pop_floor`, one compare each. The layout no longer exposes the
  flags, which changes the layout fingerprint.
- `MemFlags` carries an `AliasClass`. Frame stack header and entry
  accesses are tagged, and the LLVM backend gives them TBAA metadata, so
  an entry store is known not to change the header.
- The unit partitioner follows the functions a body calls or takes the
  address of (`called_functions`), not the imports.
- New operation probe `call`.

## Measurements

`call` probe: 37 instructions per call before, 12 after (Rust 0).

Callgrind instructions at check size, Linux, base = step 3e. All 30
programs print the same output. The 30 programs each compile as one unit,
so only the frame changes reach them.

| Program | Before | After | Change |
| --- | ---: | ---: | ---: |
| `raytracer` | 6.56M | 6.26M | -4.5% |
| `hashing` | 3.63M | 3.56M | -2.0% |
| `lru_cache` | 22.7M | 22.4M | -1.3% |
| `http_api` | 26.3M | 26.0M | -1.0% |
| `interpreter` | 31.0M | 30.7M | -0.9% |
| `binary_trees` | 4.24M | 4.20M | -0.8% |

No program moves up by more than 0.6%.

## Gate

4a's gate is missed. A call keeps 12 instructions of frame work against
the 8 the plan set: the pop reloads the length because the push's runtime
path may have changed it. binary_trees (9.6x Rust) and interpreter (4.2x)
stay far from their targets: their cost is enum construction and
allocation (`aura_direct_enum_new_typed` and malloc/free are about 60% of
binary_trees), not calls.
