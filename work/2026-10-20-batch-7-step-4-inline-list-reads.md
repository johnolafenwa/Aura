# Batch 7 Step 4: Inline List Reads

## Goal

Read list lengths and scalar list elements without a runtime call, with one
bounds check, as step 4 of the
[Batch 7 design checkpoint](../architecture_docs/22-batch-7-design-checkpoint.md)
plans. At the step 3 head, each element of `int_list_sum` cost two runtime
calls: `aura_direct_vec_len` for the bounds guard and
`aura_direct_vec_index_scalar`, which checked the index again.

## Work Completed

- `OpaqueValue` is `repr(C)` and keeps a list view: the first element's
  address and the length of a `list` value, or zeros. `OpaqueValue::new`
  sets it, and `value_mut` refreshes it through a drop guard, so a write
  that unwinds refreshes it too.
- The `shared: AtomicBool` and the inline `RwLock` became one
  `AtomicPtr<RwLock<()>>`, null while the value is confined. The lock is
  allocated by `share_direct_value`. This keeps a box at 160 bytes. With the
  view added and the old fields, it was 176 bytes, and `dict_build_get` ran
  about 2.3% more instructions.
- `direct_list_layout` computes offsets from field and variant references
  in this build: the view and lock fields, the element stride, and the
  payload offsets of `Value::Int`, `Value::Float`, and `Value::Bool`.
  `IntegerValue::wide_payload_offset` checks that the signed and unsigned
  representations keep their 128-bit value at one offset.
- The main wrapper calls `aura_direct_check_list_layout` with the layout
  fingerprint after `aura_direct_runtime_init`.
- `emit_list_len` and `emit_scalar_list_read` read in place when the lock
  pointer is null. Shared lists, out-of-range indexes, and integers past
  `int64` take the runtime path with its existing diagnostics.
- `aura_direct_vec_push_scalar` appends a scalar from its bits. A plain
  local receiver skips the store back after the append.
- `emit_int32_bounds_check` skips the check for an in-range `iconst`, which
  removes two dead branches per list index for the line and column words.

## Verification

- New fixture `scalar_list_reads_in_place` covers `int64` extremes, negative
  indexes, appends, copies, pops, `float64` and `bool` lists, and a list
  passed to a task. It matches on both backends. The two list index
  `run-fail` fixtures keep their diagnostics.
- New runtime tests read elements through the layout offsets, follow the
  view through pushes, pops, copy-on-write, clears, and an unwinding write,
  compare scalar pushes with boxed pushes, pin the 160-byte box, and trap on
  a layout mismatch and an unknown push kind.
- A codegen test checks that the entry calls the layout check and that the
  runtime path stays.
- The workspace tests pass with `--no-fail-fast`, except
  `package_io_helpers_cover_local_error_paths`, which fails only because the
  container runs tests as root. The forced backend parity matrix, clippy,
  and `cargo fmt` pass.
- Callgrind, whole process, step 3 head against step 4:

| Program | Step 3 | Step 4 |
| --- | ---: | ---: |
| `int_list_sum` | 444,958,513 | 106,060,271 |
| `string_list_scan` | 363,824,369 | 365,899,833 |
| `record_field_reads` | 1,137,789,339 | 1,154,420,211 |
| `dict_build_get` | 2,998,211,951 | 3,049,793,892 |
| `set_insert_contains` | 347,380,497 | 349,860,346 |

- Eleven alternating whole-process runs on the shared Linux container put
  `int_list_sum` at 47.3 ms before and 23.9 ms after. The host was noisy:
  the other programs moved by -3% to +10% between runs, so their callgrind
  counts are the better measure.

## Follow-Up

- `int_list_sum` is about 10x Rust against a 5x Cranelift target. The loop
  still checks the lock pointer and loads the view on every element. Step 5
  can hoist both out of a proven loop and drop the bounds check.
- The view update costs up to 1.7% more instructions in programs that box
  many values. A smaller `Value`, whose 112 bytes come from the two 48-byte
  `Type` fields in `MapValue`, would recover more than that.
