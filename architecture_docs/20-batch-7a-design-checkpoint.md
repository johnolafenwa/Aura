# Batch 7a design checkpoint: runtime value performance

Status: **Steps 1 to 7 delivered 2026-10-18. Step 8 delivers copy-on-write `dict` and `set` storage. Lock removal remains open.**

Date: 2026-10-17. Source baseline: `5f68c34` (main after
[PR #64](https://github.com/johnolafenwa/Aura/pull/64)).

This checkpoint plans the runtime half of
[roadmap Batch 7a](14-priority-roadmap.md#priority-batches): hash-indexed
`dict` and `set`, and the runtime value representation. The third Batch 7a
item, `fs.rename` with a file sync call, ships separately with the checker
fix for resource methods on shared `match` bindings.

The checkpoint records where native programs spend their time today, then
states each change, the evidence it must pin, and the order of delivery.

## Measured Starting Point

All numbers are from a release build of `5f68c34` on Linux x86-64 in a
four-core container, with programs built by `aura build --backend direct`.
Python 3.13 runs the same workloads on the same machine. The programs live in
`benchmarks/runtime_values/`, and `scripts/bench-runtime-values.py` runs them.

| Program | Aura | Python | Ratio |
| --- | ---: | ---: | ---: |
| `dict_build_get`: 20,000 `str` keys, 200,000 `get` calls | 18.47 s | 0.026 s | 710x |
| `set_insert_contains`: 20,000 inserts, 200,000 `in` tests | 24.69 s | 0.007 s | 3,500x |
| `int_list_sum`: `for` over 100,000 `int64` values, 20 times | 1.96 s | 0.038 s | 52x |
| `string_list_scan`: 10,000 `str` compared, 100 times | 1.60 s | 0.014 s | 114x |
| `record_field_reads`: `doc.get("age")` on 5,000 dicts, 20 times | 0.80 s | 0.006 s | 133x |

The [AuraDB review](../work/2026-10-16-auradb-benchmark-fixes.md) workload
also runs in this container at the same commit. Point reads are now 30 to
45 µs per operation against 6 µs for Python. `update` by id is 7,895 µs
against 6 µs, and `delete` by id is 57,990 µs against 381 µs.

### Where The Time Goes

Instruction counts come from `valgrind --tool=callgrind` on reduced inputs,
with symbols kept by `AURA_NATIVE_KEEP_SYMBOLS=1`. Each share is the
inclusive cost of one runtime entry point.

| Runtime entry | `int_list_sum` | `string_list_scan` | `record_field_reads` | `dict_build_get` |
| --- | ---: | ---: | ---: | ---: |
| `aura_direct_tag_value_type` | 52% | 49% | 45% | 10% |
| `aura_direct_release_value` | 12% | 17% | 14% | 6% |
| `aura_direct_variant_payload` | 8% | 8% | 15% | |
| `aura_direct_vec_get` | 6% | 5% | 7% | |
| `aura_direct_map_get` | | | 2% | 50% |
| `aura_direct_map_set_index_in_place` | | | 3% | 25% |

Four costs explain the table.

- **Linear `dict` and `set`.** `MapValue` holds a `Vec` of pairs and
  `SetValue` a `Vec` of values. Every lookup, insert, and membership test
  scans with `==`, so building a collection is quadratic.
- **Type tags are strings parsed on every use.** Codegen tags a value with
  its type descriptor whenever it coerces the value to a declared type. The
  runtime copies the descriptor bytes into a fresh `String`, stores it
  behind a second lock, decodes it into a `Type` tree, and for enum values
  formats a new name. A `for` loop over a list tags each element's `Lookup`
  result, so the decode runs once per element.
- **Collections carry owned type trees.** Each `MapValue`, `SetValue`, and
  `VecValue` stores its element types as owned `Type` values. Cloning a JSON
  document clones a type tree per nested object. In the AuraDB `update`
  profile, `aura_direct_clone_value` spends 17% of all instructions in
  `runtime_type_from_name` alone.
- **Every value is a boxed, locked, registered heap object.** `OpaqueValue`
  holds a reference count, a `RwLock<Value>`, and a `RwLock` for its type
  tag. Each allocation also inserts into a per-task `BTreeMap` used to
  release leaked values when a task fails, and each release removes it.
  Reads such as `take_value` clone the whole value, so a dictionary key or a
  compared string is copied before it is used. A list element read returns a
  boxed `Lookup` variant whose payload is boxed again.

## Goals And Non-Goals

Goals:

- `dict` and `set` lookups, inserts, and membership take expected constant
  time and keep insertion order.
- Tagging a value with a type it already has, or with a type the runtime has
  seen before, does no parsing and no allocation.
- Reading a scalar or a string out of a collection or a field does not copy
  the collection, and reading a key does not copy the key.
- Each step keeps the forced MIR and direct parity matrix green and records
  its measurements.

Non-goals:

- No change to the code generator backend, inlining, bounds checks, or SIMD.
  Those stay in Batch 7.
- No change to the language surface. Iteration order, equality, rendering,
  and diagnostics stay as they are.
- The MIR interpreter keeps cloning on place reads. Its collection
  operations get the hash index because they share `MapValue` and
  `SetValue`.

## A. Hash-Indexed `dict` And `set`

`MapValue.entries` and `SetValue.elements` become `IndexedEntries`, a `Vec`
in insertion order plus a `hashbrown::HashTable` of `(hash, position)`.

- A collection with at most 8 entries scans. Above that, the index holds one
  slot per distinct key. Small JSON objects stay as cheap as today.
- `hash_value` agrees with `Value` equality. A union hashes as its payload,
  integers hash by numeric value, `0.0` and `-0.0` hash alike, and instance
  hashes skip the runtime type field. Sets, dictionaries, and resource
  handles hash by kind only, so equal values still meet in one bucket. The
  hash visits at most 64 value nodes, so deep keys cannot overflow the
  stack.
- The hasher is SipHash with keys chosen once per process. Iteration order
  comes from the `Vec`, so the random keys never change program output, and
  a hostile JSON object cannot force collisions.
- `remove`, `discard`, and set `remove` keep the order of the remaining
  entries. They shift the `Vec` and renumber the index, which is linear in
  the collection size, as today.
- The type derefs to a slice for reads. Writes go through methods that keep
  the index current, so no code can change a key in place.
- Dictionary and set equality use the index, which turns their quadratic
  comparison linear.

Evidence: unit tests for hash agreement, order across the threshold, removal,
and deep keys; a run fixture checked on both backends; the micro-benchmarks.

## B. Interned Type Tags

`aura_direct_tag_value_type` keeps one process-wide table from descriptor
bytes to a parsed entry: the descriptor as a shared string, its decoded
`Type`, and its integer kind. The value stores a shared handle instead of a
fresh `String`. Retagging with the same descriptor returns at once.

Evidence: a runtime test that tagging is idempotent and still sets list,
set, dict, tuple, instance, and enum types; callgrind counts on the loop
programs.

## C. Shared Element Types And Borrowed Reads

- Collection element, key, and value types become shared `Arc<Type>` values,
  so a clone copies a pointer instead of a tree. Step 3 drops this part.
  After section B, type tree clones cost about 1% of the AuraDB `update`
  profile.
- Runtime entries that only read an argument borrow it under its lock
  instead of cloning it. This covers dictionary keys, set members,
  comparisons, and string operations.

Evidence: the AuraDB `update` and `open` rows and the `cloned_elements`
counter.

## D. Lock-Free Value Cells And Cheaper Ownership Tracking

- `OpaqueValue` drops its `RwLock`s. The direct backend's ownership rules
  already guarantee that a value is either shared read-only or held by one
  writer. A shared value crossing to another worker thread is only read.
  The cell becomes an `UnsafeCell` behind the existing accessors, which keep
  their current signatures.
- The per-task registry of owned values moves from a `BTreeMap` keyed by
  address to a cheaper structure, or is skipped on the main task where no
  failure boundary needs it. The step measures both before choosing.

Evidence: the existing task, cancellation, and leak tests; a stress run of
`npm run test:scheduler-stress`; callgrind counts.

## E. Unboxed Scalars And Strings On Hot Paths

- A `for` loop over a list of scalars reads each element as a machine value
  instead of a boxed `Lookup` and a boxed payload.
- `in`, `==`, and `get` with a scalar or string operand take typed runtime
  entries, so no box is built for the operand.

Evidence: callgrind counts and the micro-benchmarks. The parity matrix pins
behavior.

Step 6 adds `aura_direct_vec_index_scalar(list, index, kind)`. The native
code generator calls it for `list[int64]`, `list[float64]`, and `list[bool]`
element reads, after its existing bounds guard. It returns the element as a
machine word: the integer, the float bit pattern, or 0 and 1. No box is
allocated, registered, or released. Under callgrind `int_list_sum` drops from
2.42 G to 0.55 G instructions, and its wall time from 0.36 s to 0.16 s.

Step 7 removes three boxes from a string comparison.

- `aura_direct_compare_values` returns a comparison result as 0 or 1, so
  codegen no longer boxes the `bool` and unboxes it again.
- `aura_direct_compare_str_literal` compares a `str` value with a literal
  held in the program image. It compares bytes, so the literal is never
  boxed or decoded.
- A type tag for `str`, `bool`, `float32`, `float64`, or `Duration` only
  records the name. The runtime skips the value lock for those tags, and
  codegen skips retagging when a value keeps the same static leaf type.

`string_list_scan` drops from 3.76 G to 1.43 G instructions.

## G. Copy-On-Write Collections

Reading a `dict` or `set` element out of a list, or binding it in a `for`
loop, deep-copies the collection. In `record_field_reads` that copy and its
release are 60% of all instructions. AuraDB's `update` and `delete` copy
every document the same way.

- `IndexedEntries` keeps its entries and index behind an `Arc`. A clone
  shares them. The first write through a shared handle copies them, so
  value semantics do not change.
- The fallible deep clone, `try_clone_array_containing_value`, shares a
  `dict` or `set` whose key and value types cannot hold an `Array`. It keeps
  the element-by-element copy for any other type, including unknown types,
  because an `Array` buffer must be copied, not shared.
- `list` keeps its plain `Vec` in this step. Its elements are a public field
  with about 300 direct uses, so sharing it is a separate change.

Evidence: runtime tests that a write to a clone leaves the original
unchanged on both sides of the index threshold; the parity matrix;
callgrind counts on `record_field_reads`; the AuraDB rows.

Step 8 shares array-free collections at the top of the deep clone and at
every nested level, so a `list` of documents copies the list but not the
documents. `record_field_reads` drops from 2.64 G to 1.54 G instructions.
The AuraDB `update` repro drops from 834 M to 407 M. AuraDB `update` by id
goes from 8,095 µs to 660 µs per operation, and `delete` from 31,630 µs
to 10,980 µs.

## F. Delivery Plan

Each step is one pull request with its measurements in the description and a
dated work note.

| Step | Scope | Status |
| --- | --- | --- |
| 1 | This checkpoint, the benchmark programs, and section A | Merged, PR #66 |
| 2 | Section B | Merged, PR #67 |
| 3 | Section C, borrowed reads | Merged, PR #68 |
| 4 | Section E, list loops | Merged, PR #69 |
| 5 | Section D, ownership ledger, and the final measurements | Merged, PR #70 |
| 6 | Section E, unboxed `int64`, `float64`, and `bool` list reads | Merged, PR #71 |
| 7 | Section E, unboxed comparison results, string literal operands, and leaf type tags | Merged, PR #72 |
| 8 | Section G, copy-on-write `dict` and `set` storage | In review |
| 9 | Section D, lock removal | Not started |

### Step 1 Measurements

Release build of the step 1 branch, same container. The container was
shared with a parallel build, so small differences are noise.

| Program | Before | After | Python |
| --- | ---: | ---: | ---: |
| `dict_build_get` | 18.47 s | 0.71 s | 0.026 s |
| `set_insert_contains` | 24.69 s | 0.062 s | 0.007 s |
| `int_list_sum` | 1.96 s | 2.15 s | 0.038 s |
| `string_list_scan` | 1.60 s | 1.52 s | 0.014 s |
| `record_field_reads` | 0.80 s | 0.96 s | 0.006 s |

The AuraDB rows did not move. Its `update` and `delete` spend their time in
whole-document clones and type tagging, which sections B to D address.

## Owner Questions

None block the work. The defaults below follow the roadmap entry. The owner
can overrule any of them.

1. Keep removal order-preserving and linear, as above, rather than an O(1)
   removal that moves the last entry into the gap. Order preservation is
   the documented contract.
2. Count interned type tags, shared element types, and lock removal as part
   of the value-representation item, although the roadmap entry names only
   boxes, locks, and copies. The profile shows tagging is the largest cost.
