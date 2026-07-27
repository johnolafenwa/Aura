# Batch 3 checkpoint

Date: 2026-07-27. Entry point: `e05c5e6`. Head at checkpoint: see the commit
list below. Nothing is pushed.

## Commits, in the authorized order

| Commit | Ticket |
| --- | --- |
| `79174dd` | B3.0-d — `int64` length-surface unification |
| `7998cc7` | B3.0-e — four diagnostic and comment polish items |
| `d9382a0` | ADR-0022 §1 inventory and §2 migrator |
| `9f7cb3f` | ADR-0022 §3–§7 — the capability-syntax flip |
| `aae9498` | coverage-surface suite migrated to `match own` |
| `ec90ad5` | io and process suites migrated to `match own` |
| `3d7827b` | normative reference migrated; 42 Manual blocks re-verified |

Each is an isolated decision commit with full gates green at the commit.

## B3.0 disposition

**B3.0-a through B3.0-d** were complete and committed on entry to this session
or during it. **B3.0-e** closed the four remaining polish items:

1. **`AU3005` guidance is clone-safety aware.** The rejection already
   classified the selected type through `rng_clone_safety`; its recovery text
   did not. For `Vec[random.Rng]` the old advice was a dead end, confirmed by
   running it: `generators[0]` → `AU3005` recommending `get(index)` →
   `generators.get(0)` → `AU3007` → only `remove(0)` works. The guidance now
   follows the same tri-state classification the rejection uses.
2. **`AU2007`** is a dedicated code for builtin function redefinition,
   registered append-only after `AU2006`, replacing the `AU2999` catch-all.
3. **`AU3002`'s recovery clause names the conflicting access.** It always said
   "perform the mutation in a separate statement first", including at pure
   read and pure consumption sites. Across the fixture corpus the real
   distribution is 17 consumption, 12 mutation, 3 read.
4. **The stale pre-selector comment** atop the runtime-fixture matrix now
   states that both sides are forced explicitly.

## ADR-0022 §1 inventory: found, migrated, `own`-annotated

Produced by `scripts/capability_inventory.py`, deterministic and re-runnable.

### `borrow` keyword occurrences

| Surface | Before | After |
| --- | --- | --- |
| Aurora source (`*.au`), keyword tokens | 952 in 438 files | **4 in 4 files** |
| Markdown, inside code only | 796 in 92 files | 93 in 12 files |
| Markdown prose ("borrow", "borrowing") | 324 | 336 |

The 4 surviving Aurora tokens are the fixtures that *prove* the retirement:
`borrow_call_argument_not_supported`, `prefix_borrow_param_not_supported`,
`borrowed_return_was_removed`, and
`borrowed_return_label_in_trait_was_removed`. The 93 Markdown tokens are
historical ADR context, this batch's work note, and the retirement
documentation itself — all deliberately excluded from the migrator. Prose is
untouched and slightly higher, which is correct: Q7 retires the keyword, not
the word.

### Matches

| Population | Before | After |
| --- | --- | --- |
| total matches | 764 | 775 |
| bare (shared) | 709 | 452 |
| `match mut` | 13 | 26 |
| `match own` | — | 297 |
| `match borrow` / `match borrow mut` | 42 / 13 | 0 / 0 |

Of the 709 bare matches found, 566 bound a payload and 390 selected a place.
The migrator annotated **333** with `own`; a further handful were annotated by
hand where the compiler's Q2 diagnostic flagged a moved payload the
conservative rule had left bare. That residue is the design working: it
surfaces as a compile error naming `match own <place>`, never as a silent
behavior change.

### Parameters and receivers

| Population | Before | After |
| --- | --- | --- |
| parameters | 1,175 | 1,190 |
| bare | 779 | 918 |
| of which declaration-known copy | 416 | 424 |
| `mut` (was `borrow mut`) | 48 | 56 |
| `own` | 215 | 216 |
| explicit `borrow` | 133 | 0 |
| receivers | 700 | 606 |
| bare `self` | 13 | 577 |
| `mut self` (was `borrow mut self`) | 93 | — (counted as `mut`) |
| `own self` | 29 | 29 |
| `borrow self` | 565 | 0 |

No bare copy parameter was annotated `own`. Q1's flip changes their
*sequencing*, not their value semantics, and every case where that matters
surfaces as an `AU3002` or `AU2004` rejection rather than a silent change. Two
did surface, and both are recorded below.

### Files the AST cannot see

103 of 1,669 tracked `.au` files did not parse before the flip; 97 of 1,673 do
not parse after it. 78 are scratch-corpus files that `broad_scratch_corpus_*`
gates only for absence of panics. This is the concrete reason the migrator is
token-aware rather than an AST rewriter: those files must still be transformed
deterministically and have no usable AST.

## The migrator

`scripts/capability_migrate.py`, with 39 behavioral tests in
`scripts/test_capability_migrate.py`, both run from
`scripts/check-reference.sh`.

Token-aware: comments, string bodies, and identifiers are masked before any
rewrite, so `borrowed`, `reborrow`, `borrow_count`, `# borrow self`, and
`"borrow mut String"` are untouched. Markdown is migrated only inside fenced
blocks and inline code spans; `architecture_docs/decisions/` is excluded
entirely.

`build` records every file the migration would change with SHA-256 before and
after, sorted by path. `check` reports pending files and writes nothing.
`apply` rewrites only files still matching the recorded pre-migration hash,
skips files already matching the post-migration hash — which is what makes a
second `apply` a no-op — and refuses to write when a file matches neither, so
later edits are never clobbered.

Applied to **689 files**. Verified idempotent: the second `apply` migrated 0.

**A defect the corpus caught.** The first run annotated `match own` onto
matches that had been explicitly `match borrow`, turning shared matches into
consuming ones. The cause was ordering: the keyword rules collapsed `match
borrow X` to `match X` before the annotator ran, so it could no longer tell an
explicitly shared match from a bare one. The annotator now runs first, against
the original text, where `match borrow X` does not match the bare-match
pattern. Two regression tests pin it, and the 36 mis-annotated lines across 23
files were repaired.

## Behavior changes worth calling out

### Q3 was not implemented

The ratified requirement is that mutable-match writeback happens on every exit
path. Probing found it implemented only for normal arm fall-through:

| Exit | Before | After |
| --- | --- | --- |
| normal arm exit | writeback | writeback |
| `return` | **lost** | writeback |
| `break` | **lost** | writeback |
| `continue` | **lost** | writeback |
| error propagation (`try`) | **lost** | writeback |

`emit_active_match_writebacks` now applies every active writeback — innermost
first, so nested matches compose — before `return`, `break`, and `continue`
terminate. `try` returns from inside an rvalue rather than through a
terminator, so its writeback is applied immediately before the `Try`
instruction; a successful `try` falls through to the arm's own writeback with
the same or a newer value. Pinned by `match_mut_writeback_on_every_exit` and
`match_mut_writeback_try_projected_nested`, covering all five exit kinds plus
the projected-field and nested-match variants. Both are byte-identical across
the MIR and direct backends.

### Q1 turned two run-pass fixtures into rejections

Both pinned ADR-0006's declaration-known copy snapshot, which Q1 removes:
`set(second=value, first=value)` relied on `second` snapshotting before
`first` mutated it. Under universal logical sharing that is an overlapping
access.

- `call_copy_read_before_named_borrow_mut` moved to check-fail as
  `copy_argument_overlaps_named_mutable_argument`.
- The snapshot section of `explicit_and_default_argument_order` was replaced
  with evaluation-order coverage that does not depend on the retired rule.

### A diagnostic-code hazard

`stable_code_for_message` infers a stable code from message text, and one of
its rules maps `contains("borrow")` to `AU3002`. Rewording diagnostics to drop
the retired keyword therefore silently changed their codes — a contract break
no single test would have named. Comparing every regenerated oracle's code
against `HEAD` found 11 drifts. Six were this hazard and are now pinned
explicitly with `Diagnostic::coded_at`. The rest were genuine relocations,
each resolved deliberately:

- `call_own_then_projected_copy_read_rejected` → `..._overlaps`, `AU3002` →
  `AU2004`: under Q1 the copy read is a shared loan, caught earlier by
  argument binding.
- `match_borrow_self_member_suggests_match_borrow` →
  `match_own_self_member_rejects_field_move`: bare no longer moves, so the
  fixture spells the move `match own` to keep pinning the rule.
- `borrowed_noncopy_return_in_option_equality` and
  `trait_impl_borrowed_return_source_mismatch` were deleted; their premise is
  gone. Two parse-fail fixtures pin the retirement instead.

### ADR-0009's machinery removed, not orphaned

Borrowed returns cannot be written, so the resolution layer was unreachable.
Deleted: `resolve_return_borrow_source`, `borrow_source_slot`,
`BorrowSourceSlot`, `call_expr_borrow_info`, `bound_arguments_borrow_info`,
`borrow_source_matches`, `borrow_sources_compatible`, `borrow_info_for_place`,
`ensure_call_result_materializable`, the `return_passing` /
`return_borrow_source` fields, and `borrow_label` throughout. The containment
*semantics* survive as ordinary move rules, pinned by
`mir_and_forced_direct_reject_noncopy_internal_exposure`.

## ADR dispositions

- **ADR-0022: Accepted**, ratified 2026-07-27, with all ten answers recorded
  in the ADR plus the range-iteration ruling.
- **ADR-0009: Superseded in part.** The syntax is gone; the containment
  semantics survive. The reserved Phase-6 live-alias contract is recorded as
  lost, with Batch 5's alias milestone re-scoped to a designed-from-scratch
  loan/view proposal.
- **Amended:** ADR-0005 (receiver spellings), ADR-0006 (copy snapshot removed,
  spellings changed), ADR-0013 (capture contract, amended not superseded),
  ADR-0016 (sequencing now applies to copy parameters), ADR-0017 (iteration
  spellings, plus the range rejection).

No provisional ADRs were needed.

## Deliberate follow-ups

1. **`AU3006` keeps its unconditional clone wording.** It has the same shape
   of problem `AU3005` had, but the authoritative ticket named `AU3005` only.
   Recorded rather than silently expanded.
2. **The direct backend collapses same-named match-arm bindings of different
   types.** Found while writing the `remove(...)` transfer fixture, unrelated
   to any ticket. Two sibling match arms both declaring `mut taken`, at
   different types, share one slot and `Member` inference then fails with
   `AU2002: direct backend could not infer direct type for temporary %t29`.
   The MIR is well-formed, so this is a direct-backend binding-slot isolation
   gap in the same family as the loop-target slots isolated in `fc22696`.
   Renaming either binding compiles and runs correctly on both backends.
3. **The `match own` fix edit was removed rather than re-pointed.** The old
   machine-applicable edit inserted `borrow ` before a scrutinee. The new fix
   is to delete the `own` keyword, whose span that check does not carry, so
   the precise help text stands alone rather than offering an edit that would
   write a retired spelling.

## What should move between batches 4–6

- The direct-backend match-arm binding-slot gap (follow-up 2) belongs with the
  Batch 4 backend work, not with a diagnostics ticket.
- Batch 5's alias milestone is now a from-scratch loan/view design, per the
  ADR-0009 supersession, and should be scoped accordingly.
- `AU3006`'s guidance (follow-up 1) is a small Batch 4 polish item.

## Gate evidence

Exact `npm run ci`, every stage on the committed tree:

| Stage | Result |
| --- | --- |
| `check:format` | pass |
| `test:rust` | 919 compiler unit tests, 266 CLI integration tests, and every fixture and package suite pass |
| `test:backend-parity` | forced MIR vs forced direct across every runtime fixture: pass |
| `test:lsp` | 73 pass, 0 fail |
| `check:extension` / `test:extension` | 13 pass, 0 fail |
| `coverage:compiler:check` | 96.13% lines, 96.88% functions, 94.34% regions |
| `coverage:lsp:check` | 100% statements, branches, functions, lines (845/845 lines) |
| `check:reference` | reference integrity passed |
| `docs:build` | pass |
| `check:audit` | 0 npm vulnerabilities; `cargo audit` clean across 173 crates |
| `check:clippy` | pass with warnings denied |
| `check:hygiene` | pass |

### Reference-integrity re-baseline

The manifest is content-hashed and fails closed, so the Manual migration
invalidated 42 block hashes. Each was re-baselined and then re-executed by the
integrity runner, which is what the fail-closed design asks for: the hashes
changed because the migration deliberately changed the code, and re-running
proves the migrated blocks still check and run with their recorded output.

### Coverage re-ratchet

Frozen floors were held at `96.07 / 96.81 / 94.29` for the whole migration, as
instructed. At this checkpoint they are re-ratcheted once to the measured
values:

| Metric | Old floor | New floor |
| --- | --- | --- |
| lines | 96.07 | **96.13** |
| functions | 96.81 | **96.88** |
| regions | 94.29 | **94.34** |

No synthetic coverage test, exclusion, or coverage-only branch was added at any
point in this batch.

### Build hygiene

`target/` reached 26 GiB after the coverage runs, past the 20 GiB threshold.
`cargo llvm-cov clean --workspace` brought it to 24 GiB with 139 GiB free — the
narrowest appropriate cleanup. No source, fixture, lockfile, dependency cache,
or user file was touched.

## Stop

This is the Batch 3 checkpoint. Phase 5 has not been started, and nothing has
been pushed.
