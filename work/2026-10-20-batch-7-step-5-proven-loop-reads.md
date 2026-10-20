# Batch 7 Step 5: Proven Loop Reads And Array Reads

## Goal

Remove the bounds checks the compiler can prove in `for` loops, and read
`Array` scalar elements without boxing the coordinate or the element, as
step 5 of the
[Batch 7 design checkpoint](../architecture_docs/22-batch-7-design-checkpoint.md)
plans.

## Work Completed

- MIR has `Instruction::IndexInRange { list, index }`, which states
  `0 <= index < list.len()`. The `for` lowering over a whole list or a span
  emits it after the loop bound check, followed by the `__index` read and
  the index advance. The advance moved from the body block into the fetch
  block, so the fact covers it.
- The MIR interpreter checks each claim and stops with an invalid MIR
  error when one is false. The MIR validators treat the fact as a read of
  both places.
- The direct backend keeps the fact for the read and the advance after it.
  The read skips the negative-index normalization and the bounds check on
  the confined path, and skips the guard before a boxed element read. The
  advance is a plain add.
- The list view covers rank-1 `Array` values. `emit_array_element_read`
  reads `int32`, `int64`, `float32`, and `float64` elements in place, with
  one bounds check. Shared arrays, other ranks, and positions out of range
  call `aura_direct_array_index`, which reports the same diagnostics.

## Verification

- New fixture `loop_reads_in_range` covers list and span loops with
  `break`, `continue`, nesting, empty lists, `float64`, `bool`, `str` and
  tuple elements, a `Span` parameter, every Array dtype, negative Array
  indexes, a rank-2 Array, and an Array write. Both backends print the
  same output, and the interpreter accepts every claim.
- New fixture `array_index_out_of_bounds` keeps the `AU4003` diagnostic for
  a negative coordinate past the start.
- New unit tests cover the interpreter check, the lowering order, the
  missing overflow trap in a proven loop, the kept Array runtime path, and
  the Array list view through a write.
- Callgrind, whole process, step 4 head against step 5:

| Program | Step 4 | Step 5 |
| --- | ---: | ---: |
| `int_list_sum` | 106,060,271 | 78,654,607 |
| `string_list_scan` | 365,899,833 | 366,699,937 |
| `record_field_reads` | 1,154,420,211 | 1,164,171,808 |
| `dict_build_get` | 3,049,793,892 | 3,069,675,900 |
| `set_insert_contains` | 349,860,346 | 351,486,633 |

  The last four programs do not run the changed code. Their counts move by
  up to 1% between builds of the same source.

- This program reads 2,000,000 `float64` Array elements by index:

```aura
def main():
    values = Array[float64].full([100000], 0.5)
    mut total = 0.0
    mut round = 0
    while round < 20:
        mut index = 0
        while index < 100000:
            total += values[index]
            index += 1
        round += 1
    print(total)
```

  It ran 3,445,483,320 instructions at the step 3 head and 63,478,022 after
  this step. Five alternating runs took a median of 367 ms before and
  10.8 ms after.

## Follow-Up

- `int_list_sum` wall time barely moved. Each element is a 112-byte value,
  so the 100,000-element list does not fit in cache. The same loop over
  10,000 elements, 200 times, runs in 12 to 15 ms instead of 28 to 40 ms.
  Storing `int64`, `float64`, and `bool` lists unboxed is the open question
  to John.
- `for i in range(len(xs))` reads keep their bounds check. Proving them
  needs a body analysis that shows `xs` keeps its length.
- The loop still loads the lock pointer and the view on every element.
  Hoisting them needs a proof that the body cannot share the list.
