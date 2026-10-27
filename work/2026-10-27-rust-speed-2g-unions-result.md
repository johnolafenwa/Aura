# Rust Speed Step 2g: Unions And Result Values

## Goal

Step 2g of Phase 2 of the Rust-speed plan John approved on 2026-10-26:
keep unions and `Result` values off boxes on hot paths. The design for all
of Phase 2 is in `architecture_docs/24-typed-layouts-design-note.md`. The
step plan is in the project files, `rust-speed-plan/phase-2/step-2g-plan.md`.

The gate: no union box on hot paths. Read from profiles, that means no
`union_inject_copy`, `union_take_payload`, `align_union_value`, or
`normalize_union` above 1% of `pubsub_broker`, `mini_browser`, `docdb`, or
`http_api`, and no `aura_direct_enum_variant` building `Result` values in
`bank_ledger`.

Profiles of the 2f build showed where the time went:

- Every optional result the runtime returns (`str | None` from
  `strip_prefix` and `read_line`, `int64 | None` from `json.as_int`) built
  `member | None` with the general union normalizer, about 4,500
  instructions a call: 13% of `pubsub_broker` and 10% of `docdb`.
- A `UnionValue` owned its `Type`, so building, aligning, and dropping a
  union copied and freed the whole union type: every key string and
  member. `plan_union_alignment` also cloned its target to compare it.
- `match own try conn.read_line()` moved the `Result`'s payload into a
  box, copied the string out into inline union words, copied it again
  into a new union box for the `match`, and took it from there: five
  runtime calls and two string copies per line.
- `Result` values that generated code built were still runtime-built
  variants in the `Values` form, matched by name: 24% of `bank_ledger`.
- A pre-existing bug: an erased generic body returning `T | int64` called
  with `T = int64` gave the caller a box in the erased union's layout,
  and native code trapped with AU4001 where `aura run` printed the value.

## Changes

- `UnionValue.union_type` is an `Arc<Type>`, and so is a runtime type
  tag's parsed type. A union built from a tag, aligned to one, or copied
  takes a reference to the type; dropping one releases it.
  `align_shared_union_value` and `inject_shared_union_member` take the
  shared type, and a union already holding that reference aligns without
  comparing keys.
- `optional_kept` keeps `member | None` per thread under a static key,
  with the positions of both members, and `optional_named` uses it for a
  named member. `optional_str_value`, `find`, `split_once`, and the
  `json.as_int`, `as_bool`, and `as_float` helpers take their unions from
  it, so they no longer run the general union normalizer.
  `plan_union_alignment` compares a union's keys with its target's in
  place.
- Generated code that reads a boxed union it owns into inline words moves
  the payload out (`aura_direct_union_payload_take`). The only reference
  to a box gets the same box back, now holding the payload, so the read
  allocates nothing; a box another handle shares is copied as before.
  `UnionTakePayload` on an inline union local moves the member's words
  out instead of boxing the union and taking from the box.
- `Result` is no longer excluded from the typed enum layouts of step 2f:
  a `Result` that generated code builds keeps its payloads in the
  specialization's layout, and `try` tests and reads it in place. A
  `Result` the runtime returns keeps the `Values` form and is read through
  the runtime, as before.
- An erased generic call whose declared return is a union over its type
  parameters, and whose type arguments collapse or merge that union,
  converts the box at the call site (`aura_direct_union_boundary_cell`,
  which unwraps or realigns it with `coerce_union_boundary`, as the
  interpreter does).
- `LayoutTable::needs_drop` marks visited layouts in a vector instead of a
  `BTreeSet`. Typed `Result` values make most programs register enum
  layouts, and the set's code put the `json` example within 661 bytes of
  its size limit.

## Problems Found

- The first measurement of the shared types put the reference agent 1.1
  KB over its Linux size budget, and the `json` example 661 bytes under
  its own. A general cache of optional unions keyed by the member type
  hashed and compared whole types; it went in favour of tables keyed by
  a static name, and the vector in `needs_drop` replaced a `BTreeSet`:
  the agent came to 2,099,336 bytes and `json` to 1,648,248 (limits
  2,106,500 and 1,657,101). Without the general cache, `find` and
  `split_once` built their unions anew again (3% of `mini_browser`), so
  they now use keyed tables too.
- `UnionTakePayload` assumed an inline union's members were all Copy, so a
  consuming take boxed the union first. An inline union can hold a `str`
  or collection handle, so the take now moves the handle out of the
  words.

## Verification

- Runtime tests: shared optional unions per name, payload takes that move
  from an owned box and copy from a shared one, and the call-boundary
  conversion of collapsed and merged unions, with their refusals.
- New fixtures: `run-pass/typed_result_values.au` (`Result` built,
  matched, passed through `try` with and without a `From` conversion,
  held in lists, and returned from a task) and
  `run-pass/erased_generic_union_collapse.au`.
- Backend parity, the runtime, core, compiler, and CLI tests,
  `check:cross-language`, `check:operation-probes`, the native-binaries
  check, `cargo fmt --check`, hygiene, and clippy pass.

## Measurements

Callgrind instruction counts, Linux container, release builds, the
30-program cross-language suite at the check size. Before is step 2f
(#164).

| Program | Before | After | Change | After vs Rust |
| --- | ---: | ---: | ---: | ---: |
| `kv_server` | 85.1M | 51.9M | -38.9% | 6.2x |
| `pubsub_broker` | 133.4M | 85.3M | -36.0% | 8.1x |
| `bank_ledger` | 18.4M | 13.6M | -25.8% | 3.9x |
| `http_api` | 31.6M | 25.7M | -18.7% | 4.0x |
| `docdb` | 79.1M | 66.0M | -16.6% | 11.9x |
| `mini_browser` | 707.8M | 595.6M | -15.8% | 18.3x |
| `spreadsheet` | 26.3M | 25.1M | -4.6% | 14.6x |

The largest increase is `astar_pathfinding` (+1.1%); `nbody` grows 1.0%
and every other program less. Micro probes: reading 20,000 lines with
`match own try conn.read_line()` went from 6.23M to 3.63M instructions,
and `strip_prefix` on each of them from 7.62M to 4.87M.

Native binary sizes on Linux: `hello` 1,034,456 bytes, reference agent
2,099,336, `json` 1,648,248 (limits 1,070,317, 2,106,500, and 1,657,101).

## Gates

The gate is met apart from one item. Inclusive profiles of the final
build:

- `union_inject_copy`, `align_union_value`, and `normalize_union` are
  under 1% in `pubsub_broker`, `mini_browser`, `docdb`, and `http_api`;
  none of them appears in any of the four profiles.
- The old payload take, which realigned the box and copied the string, no
  longer appears. Its replacement, `aura_direct_union_payload_take`, is
  1.2% of `pubsub_broker`: it moves the line out of the `str | None` box
  that `read_line` returns. Returning the string handle without a box
  (see Follow-Ups) removes it.
- `bank_ledger` builds no `Result` with `aura_direct_enum_variant`. The
  `aura_direct_enum_variant` calls left in `docdb` (2.9%) and `http_api`
  (1.9%) build `json.Value` variants, not unions or `Result`.

## Follow-Ups

- Union fields of classes, union payloads of enums, and union elements of
  lists still hold a box. None of the 30 programs has one, so the plan's
  part 3 (tag and payload in the layout) waits until a program needs it.
- Runtime helpers that return `str | None` still build a union box, which
  generated code then unwraps. Returning the string handle or null would
  remove the box.
- `json.Value` stays in the `Values` form, so `docdb`, `http_api`, and
  `json_roundtrip` still build and match its variants by name.
