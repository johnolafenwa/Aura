# Rust speed, round 1: Result words and list operations in place

## Goal

John's 2026-11-24 target is that Aura programs run as fast as their Rust
versions in `benchmarks/cross_language/`. This is the first round of
changes toward it. Text handling and dictionary lookups are separate work.

## Starting point

On `main` at the 0.17.0 preview (`6642a7c2`), on the shared Linux x86-64
container, the median program takes 2.7 times Rust's wall time. Profiles
of the 23 slowest programs under callgrind at the bench size tied the gap
to specific causes:

- `graph_paths`: 46% of its instructions were `heap.swap(a, b)` on a list
  of a copy class, a runtime call of about 340 instructions. Only swaps
  on scalar lists were compiled in place.
- `order_book` (30%) and `raytracer` (15%): an append to a list held in a
  field or in a list of lists always called the runtime.
- `bank_ledger`: 713,000 boxes for `Result` values that the caller matched
  at once, 326,000 boxes for the `None` in `Result.Ok(None)`, and 241,000
  boxes for the integer a `dict.get` found.

## Work completed

- A local `Result[T, E]` whose payloads are scalars, `None`, or one boxed
  value, and which is only built, assigned from a static call, matched,
  unwrapped with `try`, moved or returned, is kept as a tag word and
  payload words with no box. A function whose every `return` gives back
  such a value gets a second entry that returns the words in registers,
  and callers that keep the result in words enter it. Every other caller
  uses the normal entry, which boxes at return.
- A `None` payload of any enum variant lies in place with zero size.
- `d.get(k)` on a dictionary of `int64` whose result is only matched
  writes the integer to a stack word.
- `xs.swap(a, b)` on a list of a copy class exchanges the records in
  place, with the same bounds trap as the runtime call.
- An append to a list held in a field or in an element of a list of lists
  stores the element in place while the list has room. The runtime takes
  the appended elements in on every route from the holder to the list.
  The list layout fingerprint changed, so a program built by this compiler
  refuses an older runtime at start-up.
- The leading elements of a list literal are stored in place, and a list
  of records gets room for four with its first element.
- An inner loop that starts beside the outer counter, as in
  `for j in range(i + 1, n)`, gets the range proof the outer loop has, so
  its element accesses carry no bounds checks.
- A function that only calls leaves that cannot fail keeps no frame
  record, and small ones are inlined. A frameless leaf is now named in
  the call chain of its float division by zero.
- A scalar list index is checked with one compare on the forward path.
- New operation probes: `result_call`, `record_swap`, `field_list_push`.

## Measurements

Instructions under callgrind at the bench size, in millions, each change
measured on its own branch against `6642a7c2`:

| Program | Before | After | Rust | Before / Rust | After / Rust |
| --- | ---: | ---: | ---: | ---: | ---: |
| `bank_ledger` | 1,334.3 | 661.9 | 316.0 | 4.2x | 2.1x |
| `graph_paths` | 3,059.9 | 1,475.9 | 557.5 | 5.5x | 2.6x |
| `order_book` | 156.0 | 103.4 | 19.8 | 7.9x | 5.2x |
| `lzw_compress` | 1,946.6 | 1,565.3 | 273.2 | 7.1x | 5.7x |
| `raytracer` | 871.1 | 781.0 | 174.6 | 5.0x | 4.5x |
| `sql_engine` | 2,100.5 | 1,985.8 | 437.4 | 4.8x | 4.5x |
| `nbody` | 447.0 | 265.8 | 120.1 | 3.7x | 2.2x |
| `image_filters` | 753.9 | 553.2 | 297.1 | 2.5x | 1.9x |
| `bytecode_vm` | 868.1 | 636.0 | 457.1 | 1.9x | 1.4x |
| `text_diff` | 1,123.7 | 950.0 | 497.7 | 2.3x | 1.9x |
| `astar_pathfinding` | 901.7 | 734.0 | 300.4 | 3.0x | 2.4x |

The `result_call` probe went from 561 to 46 instructions per call, and
`dict_get_str` from 1,222 to 709.

## Follow-up

- `d[key]` with a scalar value still boxes it: 39% of `interpreter`.
- Enums other than `Result` are still boxed when built and matched.
- List creation, growth through the runtime, and a `for` over a list
  literal (`regex_match`, `graph_paths`, `raytracer`).
- List fields of `self` indexed in loops are checked on every access
  (`order_book`, `raytracer`).
