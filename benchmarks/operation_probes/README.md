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

Measured on 26 October 2026 at `4beaebb`, in a Linux x86-64 container, with
the release `aura` and Rust 1.95.0. Instructions per operation, lower is
better. `sort_ints` and `dict_remove` use 2,000 elements, as both are
quadratic in Aura today, so their cost per element grows with the count.

| Operation | Plan step | Count | Rust | Aura | Aura / Rust |
| --- | --- | ---: | ---: | ---: | ---: |
| `while_step` | - | 100,000 | 5.0 | 6.1 | 1.2x |
| `range_step` | 1a | 100,000 | 5.0 | 6.0 | 1.2x |
| `float_read` | 2d | 100,000 | 1.4 | 19.0 | 13.8x |
| `float_write` | 1b | 100,000 | 2.3 | 1,264.0 | 561.8x |
| `int32_read` | 1c | 100,000 | 5.0 | 551.0 | 110.2x |
| `u8_cast` | 1c | 100,000 | 7.0 | 4,201.0 | 600.1x |
| `sort_ints` | 1d | 2,000 | 136.4 | 246,265.5 | 1,805.1x |
| `str_slice` | 1e | 100,000 | 157.0 | 12,803.0 | 81.5x |
| `dict_remove` | 1f | 2,000 | 204.9 | 245,067.3 | 1,196.2x |
| `dict_get_str` | 3 | 100,000 | 246.2 | 2,324.6 | 9.4x |
| `list_push` | 2d | 100,000 | 20.2 | 4,579.6 | 226.8x |
| `class_field_read` | 2d, 2e | 100,000 | 1.5 | 4,847.7 | 3,230.3x |
| `field_write` | 2e | 100,000 | 7.0 | 5,395.8 | 770.8x |
| `enum_match` | 2f | 100,000 | 9.5 | 1,871.6 | 197.0x |

`range_step` was 694 instructions before step 1a (#147).
