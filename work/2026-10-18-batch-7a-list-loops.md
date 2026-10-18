# Batch 7a Step 4: List Loops

## Goal

Remove the boxed `Lookup` value that every list `for` loop built per
element, and make `for value in own values:` linear. Section E of the
[Batch 7a checkpoint](../architecture_docs/20-batch-7a-design-checkpoint.md)
plans the first part. The second is a defect found while profiling.

## Work Completed

- MIR lowering of a bare list loop reads the length once, bounds the
  index, and reads each element with `__index`. The iteration loan keeps
  the length fixed, so the read cannot fail. No `Lookup` variant is built,
  tagged, matched, or unpacked per element.
- An owned list loop reverses its private copy once and takes index `-1`
  on each step. It took index `0` before, which shifted every remaining
  element, so the loop was quadratic.
- Set loops are unchanged.
- Both backends run the same MIR, so the MIR interpreter gains too.

## Verification

- New fixture `run-pass/owned_list_iteration_order.au` pins order,
  `break`, and the empty list on both backends.
- Four MIR lowering tests in `mir_tests.rs` now expect the bounded
  `__index` read.
- `cargo test -p aura-compiler`: all pass except the root-only
  `package_io_helpers_cover_local_error_paths`.
- `cargo test -p aura-runtime`, `cargo fmt --all --check`, and
  `npm run check:clippy` pass.

Release timings, direct backend:

| Benchmark | Step 3 | Step 4 |
| --- | ---: | ---: |
| `int_list_sum` | 0.88 s | 0.33 s |
| `string_list_scan` | 0.77 s | 0.51 s |
| 30,000-element owned loop, `aura run` with build | 3.0 s | 1.0 s |

## Follow-Up

- Owned set loops still remove from the front. Sets have no cheap reverse,
  so they need a different take.
