# Operation probes

Small programs that each repeat one operation, so its cost can be read on its
own instead of inside a whole program. They back the per-operation numbers
in the plan to bring Aura to Rust speed on the
[cross-language programs](../cross_language/README.md). Each plan step names
the probe it should move.

`operation_probes.au` and `operation_probes.rs` do the same work, written the
way each language is usually written. A run takes
`<operation> <count> run`, which sets up the operation's data and then
performs the operation `count` times, or `<operation> <count> setup`, which
only sets up. One operation costs the instructions of the `run` run minus
those of the `setup` run, divided by `count`. Both versions print the same
line, `<operation> <checksum>`, in each phase. `operations.json` gives every
operation its count, the plan step that targets it, and what it measures.
The Rust version is the `operation_probes` bin of the
[Rust baselines](../rust_baselines/README.md) project, behind its
`cross-language` feature, and is built with the same `cross-language`
profile, which keeps overflow checks on.

## Running

```sh
cargo build --release -p aura
npm run check:operation-probes   # Aura and Rust print the same lines
npm run bench:operation-probes   # instructions per operation, under valgrind
python3 scripts/bench-operation-probes.py range_step enum_match --json probes.json
```

`npm run ci` runs the check. Counting needs valgrind.

## Recorded results

Measured on 27 October 2026 at `abd8fef`, after the first phase of the
plan, in a Linux x86-64 container, with the release `aura` and Rust 1.95.0.
Instructions per operation, lower is better. The last column is the first
measurement, at `4beaebb`, which already had step 1a (#147); `range_step`
cost 694 instructions before it. `sort_ints` and `dict_remove` use 2,000
elements because both were quadratic at `4beaebb`, so their cost per
element grew with the count.

| Operation | Plan step | Count | Rust | Aura | Aura / Rust | Aura at `4beaebb` |
| --- | --- | ---: | ---: | ---: | ---: | ---: |
| `while_step` | - | 100,000 | 5.0 | 6.0 | 1.2x | 6.1 |
| `range_step` | 1a | 100,000 | 5.0 | 6.0 | 1.2x | 6.0 |
| `float_read` | 2d | 100,000 | 1.4 | 18.0 | 13.1x | 19.0 |
| `float_write` | 1b | 100,000 | 2.3 | 26.0 | 11.6x | 1,264.0 |
| `int32_read` | 1c | 100,000 | 5.0 | 26.0 | 5.2x | 551.0 |
| `u8_cast` | 1c | 100,000 | 7.0 | 20.0 | 2.9x | 4,201.0 |
| `sort_ints` | 1d | 2,000 | 136.4 | 106.0 | 0.8x | 246,265.5 |
| `str_slice` | 1e | 100,000 | 157.0 | 2,116.0 | 13.5x | 12,803.0 |
| `dict_remove` | 1f | 2,000 | 200.9 | 2,411.7 | 12.0x | 245,067.3 |
| `dict_get_str` | 3 | 100,000 | 247.1 | 2,344.6 | 9.5x | 2,324.6 |
| `list_push` | 2d | 100,000 | 20.2 | 4,609.5 | 228.3x | 4,579.6 |
| `class_field_read` | 2d, 2e | 100,000 | 1.5 | 4,883.7 | 3,254.0x | 4,847.7 |
| `field_write` | 2e | 100,000 | 7.0 | 5,418.8 | 774.1x | 5,395.8 |
| `enum_match` | 2f | 100,000 | 9.5 | 1,900.7 | 200.1x | 1,871.6 |

Each operation a Phase 1 step targets now costs at most 14 times Rust's
instructions. The large ratios left belong to later phases: `class_field_read`, `field_write`, `list_push`, `float_read`,
and `enum_match` to the typed layouts of Phase 2, and `dict_get_str` to
the `str` and `dict` work of Phase 3.

`call` (step 4a, added on 28 October 2026) counts a call of a small
`int64` function that LLVM inlines, against the same arithmetic written
in the loop, so Rust's cost is 0. Aura's is the frame record each call
keeps: 37 instructions before step 4a, 12 after.
