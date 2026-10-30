# Rust-speed 5i: borrowed reads for `sql_engine`

Phase 5, step 5i. John asked why the SQL engine was slow and then asked
for the fix in 0.10. Gate: `sql_engine` at most 8x Rust by wall time.

## Problem

At the 5g head `sql_engine` ran 5.8G instructions at the bench size
(10,000 rows) against Rust's 437M, 21x by wall time. A profile showed
three copies on its scan paths, each a runtime walk or a copy of a value
the callee only reads:

- `matches(row, conditions)` calls
  `compare(row[condition.column], condition.op, condition.value)` for
  every row: `condition.value` was copied with
  `aura_direct_instance_copy_field` on each call (1.37M calls, 13% of
  instructions), then released.
- `match row[group]` in the GROUP BY scan was a nested element loan used
  as a match scrutinee, which native code did not follow, so each of its
  two variant tests walked `rows.[i].[i]` in the runtime and copied the
  element (`aura_direct_element_path_load`, 17%).
- `best = max(totals[position][i], x)` copied `totals[position]` (a
  `List(Index)` read, since the list is also loaned mutably), which left
  the inner list's storage marked shared, so the write
  `totals[position][i] = best` missed its native loan every time and
  walked the path (`aura_direct_element_path_probe` and `_store`).

## Changes

- `native_codegen.rs`: `compile_named_call` loads a field place passed to
  a call through `load_borrowed_field_argument` when
  `reads_arguments_only` holds: every argument goes to a borrowed
  parameter or is a scalar, none is written back, and no argument type can
  be a callable. The load runs with `borrow_field_reads` set, as 5e's
  comparisons do, so `load_class_slot` passes a box field where it lies;
  the copy the runtime path makes, or null, is a temporary released after
  the call.
- `native_codegen_element_loans.rs`: a read-only loan whose only other
  uses are a `Match` terminator on it, or a `VariantPayload` read of it,
  is followed. `load_scrutinee` gives the match and the payload read the
  element's box through `pass_native_element`, with the runtime copy on a
  missed loan.
- `native_codegen_elements.rs`: a lent element read rejected a list that
  any mutable loan in the function names. The loan's beginning names the
  list, so `ends_element_loan` already copies a lent element still to be
  read there, and nothing can read the list while the loan lives; only a
  closure or a task that captures the list now rules it out.
- Fixture `run-pass/borrowed_call_reads.au` covers all three, a frozen
  module constant's fields passed to a call, and a `view` element missed
  and then promoted.

## Results

Callgrind at the check size against the 5g head (`p5g4`), all 30
programs with identical output:

| Program | 5g | 5i | Change |
| --- | ---: | ---: | ---: |
| sql_engine | 377.9M | 282.4M | -25.3% |
| graph_paths | 15.0M | 12.0M | -19.8% |
| task_pool | 23.5M | 21.0M | -10.7% |
| spreadsheet | 9.0M | 8.3M | -8.4% |
| others | | | within 1% |

At the bench size `sql_engine` runs 2.2G instructions instead of 5.8G
(Rust 437M) and about 0.30 to 0.37 s instead of 0.80 s (Rust 0.043 s),
so about 8x Rust by wall time; the release re-measure gives the final
ratio. The largest remaining costs are the tokenizer (25%), the parser
(11%), and boxed row values built per INSERT.

## Found on the way

`view row = rows[slot]` followed by `match row[k]` with no later use of
`row` fails to compile with "ends parent loan `row` before its child"
(AU4001), in 0.9.0 as well: the view's loan ends at its last use, before
the nested loan the match holds. Not fixed here.
