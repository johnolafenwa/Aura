# Batch 3 checkpoint

Date: 2026-07-27. Entry point: `e05c5e6`. Previously recorded checkpoint:
`91e0d5f`. Nothing is pushed.

## Current checkpoint status

The first checkpoint gate at `91e0d5f` was green, but the subsequent
line-by-line audit of the ratified ADR-0022 requirements found binding gaps.
The corrective pass closed those gaps and the exact final worktree now passes
the complete `npm run ci` gate. The implementation and verification target is
complete. Post-gate coverage artifacts were cleaned, leaving `target/` at
6.8 GiB with 193 GiB free; the corrective tree is ready to commit.

The corrective pass is limited to Batch 3 requirements and their maintained
tests, documentation, migration tooling, and tracking. Phase 5 has not been
started.

The corrective checkpoint contains these verified slices:

| Slice | Final disposition |
| --- | --- |
| Range iteration capability ruling | `mut Range` and `own Range` reject with teaching `AU3004`; bare Range behavior is retained and backend-pinned |
| Capability-position restrictions | misplaced `mut` / `own` in field types, enum payloads, return annotations, casts, and call arguments receive `AU1101` guidance |
| Shared-match place retention and mutable-source aliases | bare matches retain the scrutinee place for the arm; local aliases from non-copy mutable parameters, receivers, match payloads, and loop elements reject |
| Borrowed-return containment documentation | maintained documentation describes owned returns and clone/index/handle/owner containment; a future loan/view design remains Batch 5 work |
| Semantic-interface invalidation | compiler and `aura lsp` use semantic-interface schema version 2; the LSP rejects missing/mismatched versions and invalidates document ownership metadata |
| Retired-syntax gate | syntax-aware maintained-source scanning is green with only the four exact retirement fixtures allowed |
| Inventory and migration preservation | manifest v2 records 1,260 semantic occurrences and 832 resolved findings with zero unresolved; strict builtin inventory is clean |
| Release notes | `CHANGELOG.md` records the breaking syntax/semantic flip, migration command and compatibility window, and borrowed-return removal |

## Commits, in the authorized order

| Commit | Ticket |
| --- | --- |
| `6afe47c` | B3.0-a — verify native cache artifacts before execution |
| `fc22696` | B3.0-b — isolate heterogeneous loop-target binding slots; ADR-0029 Accepted |
| `e05c5e6` | B3.0-c — structural tuple equality/inequality; ADR-0026 amended and Accepted |
| `79174dd` | B3.0-d — `int64` length-surface unification |
| `7998cc7` | B3.0-e — four diagnostic and comment polish items |
| `d9382a0` | ADR-0022 §1 inventory and §2 migrator |
| `9f7cb3f` | ADR-0022 §3–§7 — the capability-syntax flip |
| `aae9498` | coverage-surface suite migrated to `match own` |
| `ec90ad5` | io and process suites migrated to `match own` |
| `3d7827b` | normative reference migrated; 42 Manual blocks re-verified |

Each is an isolated decision commit with full gates green at the commit. The
corrective work after `91e0d5f` is not yet committed and is not represented in
this table.

## B3.0 disposition

**B3.0-a through B3.0-d** were complete and committed on entry to this session
or during it. **B3.0-e** closed the four remaining polish items:

| Ticket | Reproduction and disposition |
| --- | --- |
| B3.0-a | Truncated and wrong-byte cache artifacts were previously executable through the cache path; hits now verify identity, digest, regular/executable state, size, and native launch shape, and corruption rebuilds instead of executing unverified bytes. A verified warm hit remains a hit. |
| B3.0-b | Reusing names across heterogeneous `zip` loops previously ran on MIR but trapped direct with `AU4001`; fresh function-wide target slots now make the required heterogeneous reuse byte-identical across backends. |
| B3.0-c | Direct tuple `==` / `!=` was rejected despite structural membership equality; equatable tuples now compare structurally and short-circuit on both backends, while tuple ordering remains rejected. |
| B3.0-d | `len(x)` returned `int64` while String/Vec/Map/Set length members returned `int32`; the maintained length/count surface is now consistently `int64`, with narrowing explicit. |
| B3.0-e | The four diagnostic/comment findings below are closed in `7998cc7`. |

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

The final manifest-v2 ledger is based on historical baseline `d9382a0` and
contains **1,260 semantic occurrences** plus **832 resolved findings**, with
zero unresolved. It records each occurrence rather than inferring completion
from aggregate post-migration totals.

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

### Final semantic-preservation accounting

| Population | Found | Final disposition |
| --- | ---: | --- |
| pre-flip bare matches | 773 | all reviewed |
| place matches | 417 | 416 migrated to `match own`; 1 fixture was deleted |
| temporary matches | 356 | 22 migrated to `match own` because nested ownership transfer was load-bearing; 334 retained bare |
| declaration-known bare copy parameters | 468 | 466 retained bare shared; 2 were deleted; zero required `own` because no maintained snapshot-sequencing dependency was found |
| borrowed returns | 19 | 11 copy-valued returns became ordinary owned returns; 8 non-copy/unresolved redesign findings were resolved as 6 maintained-fixture redesigns and 2 obsolete-fixture deletions |

The exact match `own` count is therefore 438 applied annotations across the
retained corpus: 416 place matches plus 22 temporary matches. No bare-copy
parameter required `own`; that zero is a per-occurrence review result, not an
assumption based on the absence of compiler errors.

### Strict builtin inventory

The final schema-v2 inventory records the git/compiler/schema baseline,
path/line/column evidence, parse/type review queues, concrete trait
implementations, rendered signatures, structured call shapes, metadata, and
sibling-retention application evidence. Its strict check reports:

| Check | Final result |
| --- | ---: |
| rendered-signature/metadata mismatches | 0 |
| missing sibling-retention applications | 0 |
| builtin variants without rendered signatures | 0 |
| structured variants without call shapes | 0 |
| unlinked builtin signatures | 0 |

## The migrator

`scripts/capability_migrate.py`, with its behavioral suite in
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

The final manifest covers **683 files**. Verified idempotent: the second
`apply` migrated 0.

**A defect the corpus caught.** The first run annotated `match own` onto
matches that had been explicitly `match borrow`, turning shared matches into
consuming ones. The cause was ordering: the keyword rules collapsed `match
borrow X` to `match X` before the annotator ran, so it could no longer tell an
explicitly shared match from a bare one. The annotator now runs first, against
the original text, where `match borrow X` does not match the bare-match
pattern. Two regression tests pin it, and the 36 mis-annotated lines across 23
files were repaired.

The corrective pass adds a standing retired-syntax lint over maintained
Aurora, Markdown/HTML code, Rust diagnostic strings, and diagnostic snapshots.
It allows only the four exact Aurora fixtures that prove retirement and
distinguishes explanatory English from live syntax. `check` accepts a clean
post-migration file whose content has moved beyond the original manifest after
the retired syntax is gone, while `apply` remains strict about pre/post hashes.
The manifest no longer retains deleted or intentionally preserved entries.
The final 59 migrator tests pass, manifest build/apply/check/second-apply
behavior passes, and the retired-syntax scan has zero active findings outside
the four exact retirement fixtures.

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
- Phase 5 runtime remains Batch 4; no runtime milestone was pulled into this
  corrective pass.
- Batch 5's alias milestone is now a from-scratch loan/view design, per the
  ADR-0009 supersession, and should be scoped accordingly.
- `AU3006`'s guidance (follow-up 1) is a small Batch 4 polish item.
- Batch 6 remains Phase 7 plus self-audit and release preparation; this pass
  identifies no reason to move those milestones.

## Final gate evidence

The fresh exact-tree `npm run ci` passes end to end. Its first Rust pass found
one real TaskGroup named-argument forwarding regression; that defect was fixed,
its regression was pinned, and the full gate was restarted successfully.

| Stage | Result |
| --- | --- |
| `check:format` | pass |
| `test:rust` | 23 Aura unit tests, 268 CLI integration tests, 928 compiler unit tests, and every fixture/package suite pass |
| `test:backend-parity` | forced MIR vs forced direct across every runtime fixture: pass in 732.74 seconds |
| `test:lsp` | 79 pass, 0 fail |
| `check:extension` / `test:extension` | 13 pass, 0 fail |
| `coverage:compiler:check` | 64,645/67,244 lines (96.134971%), 4,200/4,335 functions (96.885813%), 94,962/100,649 regions (94.349671%) |
| `coverage:lsp:check` | 100% statements, branches, functions, and lines |
| `check:reference` | 59 migrator tests, strict inventory/lint, reference integrity, and live block execution pass |
| `docs:build` | pass |
| `check:audit` | npm and Rust audits pass |
| `check:clippy` | pass with warnings denied |
| `check:hygiene` | pass |

Suite-count precision: the 928 compiler and 268 CLI totals above are the
observed counts under the exact gate conditions: the debug profile and the
single-threaded Rust test invocation (`cargo test -- --test-threads=1`). They
are not profile-independent inventory totals; alternate invocations that
report 927 compiler and 265 CLI tests do not contradict this checkpoint.

### Reference-integrity re-baseline

The manifest is content-hashed and fails closed, so the Manual migration
invalidated 42 block hashes. Each was re-baselined and then re-executed by the
integrity runner, which is what the fail-closed design asks for: the hashes
changed because the migration deliberately changed the code, and re-running
proves the migrated blocks still check and run with their recorded output.

The final reference inventory is 34 pages and 246 fences: 118 verified and 128
illustrative. Of 194 Aurora blocks, 115 are verified. No normative section,
page, or required example is missing.

### Coverage re-ratchet

The corrective pass held the `91e0d5f` floors frozen at
`96.13 / 96.88 / 94.34`. At final sign-off they are re-ratcheted once, by
the checkpoint policy applied to the exact measurements:

| Metric | Exact measurement | Final floor |
| --- | ---: | ---: |
| lines | 64,645/67,244 (96.134971%) | **96.13** |
| functions | 4,200/4,335 (96.885813%) | **96.89** |
| regions | 94,962/100,649 (94.349671%) | **94.35** |

No synthetic coverage test, coverage exclusion, or coverage-only production
branch was added.

### Build hygiene

The corrective pass began with `cargo clean`, which removed 56.0 GiB and raised
free disk space to 199 GiB. After the final gates,
`cargo llvm-cov clean --workspace` removed the disposable coverage outputs,
leaving `target/` at 6.8 GiB and 193 GiB free. Source, fixtures, lockfiles,
dependency caches, and user files were preserved.

## Stop

Batch 3's implementation and exact-tree verification are complete at the
requested checkpoint. Post-gate cleanup is complete, and the corrective
checkpoint is committed at `1c249ab`. Phase 5 was not started during Batch 3,
and nothing has been pushed.
