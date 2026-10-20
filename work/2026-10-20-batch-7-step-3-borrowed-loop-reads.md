# Batch 7 Step 3: Borrowed Loop Element Reads

## Goal

Remove the per-element copy from shared `for` loops over local lists. Step 3
of the [Batch 7 design checkpoint](../architecture_docs/22-batch-7-design-checkpoint.md)
plans explicit copies in MIR and a pass that cancels them. Profiles at the
step 2 head showed where the copies cost most:

- `string_list_scan`: `vec_index` cloned and boxed each element, 40% of
  instructions, and releasing the copy took another 34.5%.
- `record_field_reads`: each iteration deep-cloned a `dict[str, json.Value]`,
  about 14% plus its drop.

## Work Completed

- `crates/aura-compiler/src/mir_copies.rs` decides when a loop body only
  borrows its binding: a side of `==` or `!=`, the receiver of `len()`, or
  the receiver of `get(key)` on a `dict`. Rebinding, member or index writes,
  closures, and comprehensions that mention the binding keep the copy.
- MIR lowering lends each element of a qualifying loop through the
  `for x in mut` loan machinery, as a shared loan. Loops that do not qualify
  lower exactly as before.
- `cancel_loan_copies` removes a `ReadLoan` copy whose only use is the next
  comparison in its block. The comparison reads the loan.
- The direct backend reads a lent element in place. The loan start calls
  `aura_direct_vec_check_index`. Comparisons call
  `aura_direct_compare_str_literal_at` or `aura_direct_compare_values_at`.
  `len()` and `get(key)` call `aura_direct_vec_element_len` and
  `aura_direct_vec_element_map_get`.
- Fixed a loan bug found on the way. A loan live before a loop, from a `view`
  or an outer `for x in mut`, ended at its last use inside the loop body. The
  next iteration then failed with `AU4001`. `LoopLabels.outer_loans` keeps
  such loans active through the loop.

## Verification

- New fixtures: `borrowed_list_loop_reads` and `loan_used_inside_later_loop`
  match on both backends.
- New tests: two MIR tests, one native codegen symbol test, a runtime test
  for in-place reads, four runtime trap modes, and
  `direct_borrowed_loop_reads_copy_no_element`. That test pins zero cloned
  elements. Its box count falls from 65,003 to 45,003. The rest come from
  the setup loop and from `get`, which boxes its key and result.
- Callgrind, whole process:

| Program | Step 2 head | Step 3 |
| --- | ---: | ---: |
| `string_list_scan` | 1,159,541,933 | 363,819,459 |
| `record_field_reads` | 1,373,117,275 | 1,137,797,396 |

- Eleven alternating whole-process runs on the shared Linux container,
  medians against `aa52bfc`:

| Program | Before | After | After / Rust |
| --- | ---: | ---: | ---: |
| `string_list_scan` | 115.0 ms | 32.8 ms | about 12x |
| `record_field_reads` | 149.9 ms | 122.9 ms | about 13x |
| `dict_build_get` | 298.2 ms | 283.4 ms | about 13x |
| `set_insert_contains` | 33.6 ms | 33.0 ms | about 5x |
| `int_list_sum` | 44.7 ms | 45.6 ms | about 20x |

- The workspace tests pass with `--no-fail-fast`, except
  `package_io_helpers_cover_local_error_paths`, which fails only because the
  container runs tests as root. The documentation examples, `cargo fmt`,
  clippy, the hygiene check, and the forced backend parity matrix pass.

## Follow-Up

Both list programs still miss their Cranelift targets of 10x and 8x. The
remaining costs are outside step 3:

- `tag_value_type` parses the type name on each union tag check.
- `get` boxes its string literal key on each call.
- Matching a `Lookup.Found` payload copies it even when the result dies.
- `task_runtime_boundary` and the owned-value ledger cost about 15% of each
  runtime call.

Steps 4 and 5 inline list reads and drop proven bounds checks, which
can also remove the separate index check at each loan start.
