# Batch 7a Step 10: Lock-Free Confined Values

## Goal

Stop taking a lock on every read and write of a runtime value. Section D of
the [Batch 7a design checkpoint](../architecture_docs/20-batch-7a-design-checkpoint.md)
records the design. John chose to continue Batch 7a, and then lists before
lock removal, on the 2026-10-18 decision cards.

## Work Completed

- A build with no lock at all, which is unsound across threads, set the
  upper bound: `int_list_sum` 0.109 s to 0.047 s.
- `OpaqueValue` holds its value and type tag in `UnsafeCell`s. A value
  starts confined to its creating thread and its accessors take no lock.
- Task start and the module-constant registry are the two ways generated
  code hands a value to another thread. Both call `share_direct_value`
  first, and every later access to a shared value takes its `RwLock`.
- Debug builds of compiled programs record the creating thread and stop
  when another thread reaches an unshared value. Runtime unit tests hand
  values between threads by hand, so they skip that check.

## Verification

- `values_take_the_lock_only_once_shared` and
  `task_arguments_are_shared_before_the_child_starts`.
- With the debug thread check active: `cargo test -p aura` (392 CLI
  tests and the other program suites), the backend parity matrix with
  `--ignored`, ten iterations of `scripts/stress-scheduler.sh`, and every
  `aura-compiler` integration test.
- `cargo test -p aura-runtime --lib`: 694 passed. `cargo test -p
  aura-compiler --lib`: 1521 passed. The one failure,
  `package_io_helpers_cover_local_error_paths`, fails because the
  container runs tests as root.
- `npm run check:clippy` and `cargo fmt --all --check`.
- Back-to-back runs against main `535affa` in the same container:

| Program | Main | Step 10 | Python 3.13 |
| --- | ---: | ---: | ---: |
| `int_list_sum` | 0.116 s | 0.048 s | 0.048 s |
| `string_list_scan` | 0.186 s | 0.110 s | 0.018 s |
| `record_field_reads` | 0.207 s | 0.146 s | 0.007 s |
| `dict_build_get` | 0.348 s | 0.308 s | 0.032 s |
| AuraDB `update` by id | 20 µs | 20 µs | 6.0 µs |
| AuraDB `delete` by id | 7,310 µs | 6,250 µs | 381 µs |
| AuraDB `open`, 5,000 documents | 250 ms | 201 ms | 14.2 ms |

The raw reports are `runtime-values-before-confined-values.json`,
`runtime-values-confined-values.json`, `auradb-before-confined-values.txt`,
and `auradb-confined-values.txt` in
`work/2026-10-18-batch-7a-measurements/`.

## Follow-Up

- A value shared between task threads still locks on every access.
- `record_field_reads` spends most of its time in type tagging and the
  ownership ledger, not in locks.
