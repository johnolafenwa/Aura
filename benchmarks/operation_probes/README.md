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

`iterator_next`, `generator_next`, and `generator_start` (step 9f, added
on 7 November 2026, measured at `f86a588`) cost one `next` of a
user-defined `Iterator` class, one resumed `yield` of a generator, and
creating, running to its first `yield`, and dropping a generator. Rust's
side uses a plain `Iterator` impl and `std::iter::from_fn`, which LLVM
inlines into the loop, so these ratios measure Aura's dispatch and
coroutine switching rather than the work inside the loop body.

| operation | step | count | Rust instr/op | Aura instr/op | Aura/Rust |
| --- | --- | ---: | ---: | ---: | ---: |
| `iterator_next` | 9f | 100,000 | 7.0 | 2,161.1 | 308.7x |
| `generator_next` | 9f | 100,000 | 5.0 | 8,099.3 | 1,619.5x |
| `generator_start` | 9f | 10,000 | 11.0 | 24,551.1 | 2,231.9x |

That was the baseline. With the fast iteration forms (measured on
7 November 2026, after #227), a loop over such an iterator calls an
unboxed `__aura_advance` method that LLVM inlines, and a loop over a
generator with one `yield` that calls nothing runs the generator's body in
place, so neither builds a `Step` or switches stacks:

| operation | step | count | Rust instr/op | Aura instr/op | Aura/Rust |
| --- | --- | ---: | ---: | ---: | ---: |
| `iterator_next` | 9f | 100,000 | 7.0 | 15.9 | 2.3x |
| `generator_next` | 9f | 100,000 | 5.0 | 6.0 | 1.2x |
| `generator_start` | 9f | 10,000 | 11.0 | 0.4 | 0.0x |

`generator_start` now costs nothing: the one-item generator runs in place
and LLVM folds it into the loop around it, while Rust still builds its
`from_fn` closure. A generator that does not qualify for running in place
(more than one `yield`, or a call in its body) runs as a state machine,
about 22 instructions per item on the same loop. A generator passed
around as a value, or with locals other than numbers, `bool`, and `str`,
still ran on its own stack.

`generator_value`, `generator_pipeline`, `generator_list`, and
`generator_class` (step 9g, added on 7 November 2026) measure those cases:
a generator passed to a function that sums it, a generator that doubles the
items of another generator it was passed, a generator over a `list[int64]`
it owns, and a generator whose loop counter is a field of a class instance.
`str_append` and `str_format` measure a string-building loop. At `453a42b`,
before step 9g:

| operation | step | count | Rust instr/op | Aura instr/op | Aura/Rust |
| --- | --- | ---: | ---: | ---: | ---: |
| `generator_value` | 9g | 100,000 | 5.0 | 8,087.3 | 1,617.1x |
| `generator_pipeline` | 9g | 100,000 | 8.0 | 16,170.6 | 2,021.0x |
| `generator_list` | 9g | 100,000 | 5.0 | 8,094.4 | 1,618.7x |
| `generator_class` | 9g | 100,000 | 5.0 | 8,115.3 | 1,622.7x |
| `str_append` | 9g | 100,000 | 38.4 | 101,245.1 | 2,639.8x |
| `str_format` | 9g | 100,000 | 308.9 | 291,220.8 | 942.7x |

A call that passes a generator call to a function that only loops over it
now calls a copy of the function that loops over the generator call
itself, and generator frames hold collections, classes, and other
generators' frames, so all four run as a state machine or in place:

| operation | step | count | Rust instr/op | Aura instr/op | Aura/Rust |
| --- | --- | ---: | ---: | ---: | ---: |
| `generator_value` | 9g | 100,000 | 5.0 | 6.0 | 1.2x |
| `generator_pipeline` | 9g | 100,000 | 8.0 | 10.0 | 1.2x |
| `generator_list` | 9g | 100,000 | 5.0 | 7.0 | 1.4x |
| `generator_class` | 9g | 100,000 | 5.0 | 28.1 | 5.6x |

`generator_class` costs what the same loop costs without a generator: each
read or write of a field of an instance with a `str` field checks the
instance's layout first, which `class_field_read` and `field_write` also
measure.

A class local that the function builds with a constructor and otherwise
reaches only through its fields keeps its layout, so its field reads and
writes now skip that check:

| operation | step | count | Rust instr/op | Aura instr/op | Aura/Rust |
| --- | --- | ---: | ---: | ---: | ---: |
| `generator_class` | 9g | 100,000 | 5.0 | 10.1 | 2.0x |

`class_field_read` (14.7x) and `field_write` (4.0x) reach instances held in
a list, which other code can share, so they still check.

A loop that reaches a list only through its length, its elements, and loans
of its elements now reads the list's view (its form, length, and element
storage) once before the loop, instead of after every call the loop makes.
When that view is one the loop's reads and writes keep, such as a list of
plain class instances that no other value shares, the loop runs a copy of
itself with the view's checks already settled:

| operation | step | count | Rust instr/op | Aura instr/op | Aura/Rust |
| --- | --- | ---: | ---: | ---: | ---: |
| `class_field_read` | 2d, 2e | 100,000 | 1.5 | 2.1 | 1.4x |
| `field_write` | 2e | 100,000 | 7.0 | 18.0 | 2.6x |

## Trait object call

`trait_object_call` (trait objects step 6, added on 11 November 2026)
measures one method call on each element of a `list[dyn Solid]` that holds
three classes, next to `Vec<Box<dyn Solid>>` in Rust. Before step 5 a loop
copied each element and the call tested the element's type by name against
each implementation in turn. Now the loop lends the element and the call
compares a type number in the element's box:

| operation | step | count | Rust instr/op | Aura instr/op | Aura/Rust |
| --- | --- | ---: | ---: | ---: | ---: |
| `trait_object_call`, before | trait objects 6 | 100,000 | 12.0 | about 4,600 | about 380x |
| `trait_object_call` | trait objects 6 | 100,000 | 12.0 | 34.0 | 2.8x |

The earlier figure was read on the same loop over 30,000 elements before
the probe existed.

## Method calls on list elements

`element_mut_call` and `element_plain_mut_call` (added on 11 November 2026)
measure a `mut self` method call on a list element by index,
`counters[i].bump(2)`, on a class with a `str` field and on a class of two
integers. Before, each such call took the element out of the list and put
it back through the runtime, and so did a shared call by index and every
call in `for item in mut items:`. Now the call works on the element where
the list keeps it:

| operation | count | Rust instr/op | Aura instr/op | Aura/Rust |
| --- | ---: | ---: | ---: | ---: |
| `element_mut_call`, before | 100,000 | 12.0 | about 2,800 | about 230x |
| `element_mut_call`, at `d37640c` | 100,000 | 12.0 | 94.1 | 7.8x |
| `element_mut_call` | 100,000 | 12.0 | 34.0 | 2.8x |
| `element_plain_mut_call`, before | 100,000 | 16.0 | about 2,800 | about 175x |
| `element_plain_mut_call`, at `d37640c` | 100,000 | 16.0 | 64.1 | 4.0x |
| `element_plain_mut_call` | 100,000 | 16.0 | 26.0 | 1.6x |

The earlier figures were read on the same loops over 1,000 elements before
the probes existed. Both phases of each probe make one pass first, because
the first calls on a list's elements settle how the list stores them (about
one call in eight of that first pass still goes through the runtime), and
the probes measure the calls after.

At `d37640c` what was left was the cost of a call that was not inlined:
`Tally.bump` ran 49 of the 64 instructions, most of them the entry and exit
of its frame record, and the caller checked the type tag of a boxed
receiver before and after the call. A small method that calls nothing now
has a body with no frame record, which is inlined into its direct callers
(see "Frame Records" in
[the native backend notes](../../architecture_docs/08-native-codegen-and-runtime.md)),
and a method that only touches scalar fields of its receiver needs no tag
check. No other probe changed.
