# 2026-10-16 AuraDB Benchmark Fixes

## Goal

John's AuraDB project implements the same document database in Aura,
Python, and Rust. Its benchmark showed Aura 0.4.1 far behind Python on point
operations. The review traced the gap to compiler and runtime defects. This
pass fixes the ones that are bugs and records the rest on the roadmap.

## Work Completed

- **Builtin modules in dependency packages.** A dependency package function
  that called `fs.exists` failed at run time with `AU4001` "unknown MIR
  function `depfs.fs::exists`". `qualify_imported_module_namespaces` gave
  builtin module namespaces the dependency's package prefix. It now skips
  builtin modules, as it already skipped dependency aliases.
- **Collection fields lent, not cloned, on the direct backend.** Reading
  `self.docs` deep-cloned the whole field, so a method call or a `for` loop
  on a field copied it on every use. The direct backend now lends the field:
  it moves the value out of its slot, makes the call, and moves it back.
  This covers:
  - builtin `list`, `dict`, and `set` methods on a field, including the
    `in` test and `for` loops;
  - a field passed to a borrowed parameter, and a field passed to a `mut`
    parameter whose writeback targets the same field;
  - the entry a `match [mut] d.lookup(k)` arm binds, through two new runtime
    helpers, `aura_direct_element_path_probe` and
    `aura_direct_element_path_take`;
  - methods called on that entry, when the method borrows its receiver.
- **Aliasing guard.** Lending is skipped when another argument names an
  overlapping place, or when any argument's type may hold an instance, a
  callable, or a type parameter. Those arguments could reach the lent field
  while it is out of its slot.
- **Clone counter.** `AURA_RUNTIME_STATS=1` reports `cloned_elements`, the
  number of elements copied by deep clones. The CLI tests use it to pin the
  fixes.
- **Roadmap.** John moved Batch 7's runtime work ahead of Batch 6 as Batch
  7a. It owns hash-indexed `dict` and `set` storage, the value
  representation rework, and `fs.rename` with a file sync call.
- The MIR interpreter still clones on each place read. `aura run` without
  `--backend direct` does not get these fixes.

## Verification

- New tests:
  - `dependency_package_functions_can_call_builtin_modules` in
    `crates/aura-compiler/tests/packages.rs`;
  - `direct_element_path_take_and_probe_lend_entries_without_cloning` in the
    runtime unit tests;
  - two programs in `crates/aura/tests/representation_stats.rs`. The
    collection field program cloned 582,900 elements before the fix and now
    stays under 2,400, the count for the same program written with locals.
    The lookup entry program went from 261,498 to under 1,000.
- AuraDB benchmark, Linux x86-64 container, release builds, median of
  repetitions. "Before" is Aura 0.4.1 codegen. "After" is this branch.
  Python 3.13 runs the same workload on the same machine.

  | Operation | Before | After | Python |
  | --- | ---: | ---: | ---: |
  | `get` by id, n=1,000 | 16,240 µs/op | 35 µs/op | 4.9 µs/op |
  | `get` by id, n=5,000 | 114,740 µs/op | 65 µs/op | |
  | insert singles, memory | 9,775 µs/op | 114 µs/op | 0.98 µs/op |
  | indexed find, n=5,000 | 12,502 ms | 53 ms | |
  | open and load, n=5,000 | 51,082 ms | 1,428 ms | 19.1 ms |
  | `update` by id, n=2,000 | 84,415 µs/op | 7,775 µs/op | 5.96 µs/op |
  | `delete` by id, n=2,000 | 159,900 µs/op | 74,050 µs/op | 440 µs/op |
  | scan find, n=5,000 | 419 ms | 302 ms | 22.6 ms |
  | `insert_many`, 5,000 | 475 ms | 518 ms | 3.5 ms |

- The "before" run shared the machine with a two-job compiler build for its
  first rows. The large ratios do not depend on that.
- `cargo test --workspace`, `cargo fmt --all --check`, clippy, and the
  forced MIR and direct parity matrix: see the PR.

## Follow-Up

- Batch 7a: hash-indexed `dict` and `set`. AuraDB's `update` and `delete`
  rebuild a positions dictionary per call, which is quadratic with the
  association list.
- Batch 7a: `fs.rename` and a file sync call.
- Batch 7a: unboxed scalars and strings, no per-value lock, and no copies
  on reads. These account for most of the gap left after this pass.
- The MIR interpreter still clones on place reads.
- Lending a field whose argument list also holds an instance-typed value is
  skipped. A precise alias analysis could cover it.
