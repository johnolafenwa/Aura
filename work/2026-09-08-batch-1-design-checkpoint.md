# Batch 1 design checkpoint — 2026-09-08

## Goal and authorized scope

The detailed callable/type-foundation proposal was delivered in documentation
commit `fe9c6c0` on main, based on
`de3d6cc44a376502f2aaf3f3ef8faba59c523884`. The user has now ratified all
recommended options except Q6 B and Q20 B, accepting Q23 A with a required
Batch 2 follow-up. This documentation follow-up records those answers and
reconciles their dependent contracts. Status: **Ratified; implementation pending**.
It does not implement the design or amend accepted ADR bodies.

Authorized files are the design document, the roadmap links and Batch 2 note, architecture
reading-order index, ADR index, this work note, and the task-board entry.
Compiler/runtime, examples, tutorials, Manual, generated documents, release
metadata, and the ten Approved Decisions are unchanged. The protected
`personal/file_ops.au`, `.swp`, and untracked ADR-0022 draft are outside scope.

## Work completed

- Read the requested ADRs and Manual chapters, the Batch 1 scope/removal
  criteria, cross-ADR open conflicts, and the current reference agent.
- Added [the design checkpoint](../architecture_docs/16-batch-1-design-checkpoint.md)
  with unions/narrowing/FFI, aliases, owned callable representation and binding
  contracts, all Option library replacements, a sema split plan, the complete
  hypothetical reference agent, primary-source prior art, and two test-first
  implementation phases.
- Ended the document with 24 numbered option/trade-off questions, each with
  exactly one recommendation; mapped the detailed contract bundles to them.
- Cross-checked 39 public Option-bearing APIs against the API index and
  builtin/checker metadata. Identified the private dictionary replacement
  result separately without proposing a public `dict.set` method.
- Recorded the supplied inventory as planning input and a reproducible
  current-tree inventory with explicit scopes/units; distinguished Rust's
  host Option from the Aura builtin being removed in phase 2.
- Preserved the stored-loan deferral, recorded the cost of remove/call/reinsert
  for a non-cloneable callable registry, and listed ADR tensions explicitly.
- Linked the proposal from the Batch 1 row and both requested indexes.

## Proposal verification (fe9c6c0)

- Scoped documentation checks passed for all six authorized documents: relative
  links and anchors, architecture/ADR index coverage, balanced fences, final
  newlines, whitespace, and the 24 sequential questions with exactly one
  recommendation each. The Approved Decisions and technical-default paragraph
  are byte-identical to the source baseline; no ADR body changed.
- `python3 scripts/test_aura_identity.py`: all 15 tests passed. The first pass
  identified disallowed editorial wording in the proposal; that prose was
  corrected without changing the gate or the design.
- `python3 scripts/reference_integrity.py --inventory-only`: 39 Manual pages,
  274 fences, 129 verified Aura blocks, and no missing required sections or
  feature pages without a verified example. No compiler was invoked.
- The tutorial loader classified 340 fences in 28 tutorial pages and found
  zero Aura fences in the proposal. Its 12 fences are all plain `text`, and
  the document is outside both gates' source directories.
- Scoped `git diff --check` passed. The protected personal file's existing
  whitespace is excluded; new documents are also checked directly for final
  newlines and trailing whitespace. Staging is limited to the six authorized
  Markdown files.

## Ratification follow-up

- Recorded all 24 selected answers alongside the unchanged original options
  and recommendations, and changed the checkpoint and index statuses to
  **Ratified; implementation pending**.
- Q6 B: symmetric union/member equality uses the unique-member injection
  rule. Equality does not narrow. Union hashing delegates to the active
  member to preserve equal hashes for equal union/member values; documented
  the necessary later reconciliation of ADR-0052's tag-inclusive hash rule.
- Q20 B: removed the proposed wrap intrinsic and its special metadata,
  lifetime remapping, and storage obligations from the design. Added ordinary
  wrapper examples and test oracles for explicit outer and inner defaults.
- Q23 A: retained Batch 1 Lookup/Poll and added the user's required Batch 2
  review of an app-facing `V | None` form of `dict.get`, with element-loan
  lifetimes and present-None distinctions, in the checkpoint and roadmap.
- Reconciled equality/hash fixtures, forwarding fixtures, diagnostics, sema
  module responsibilities, and the confidence/reconciliation notes. The
  reference-agent candidate does not use either overridden feature and needs
  no source change.

Ratification follow-up verification passed:

- Six scoped documents, 150 relative links/anchors, 13 balanced fences, 64
  indexed ADRs, architecture index coverage, table columns, final newlines,
  and scoped whitespace.
- All 24 recorded answers match the user; the original questionnaire options
  and recommendations are byte-identical after removing the added answer
  lines. The ten Approved Decisions, ADR bodies, and reference-agent candidate
  are unchanged.
- All 15 identity tests passed. The Manual inventory passed with the same
  39 pages and 274 fences. The tutorial loader still sees 340 fences across
  28 tutorial pages and zero Aura fences in the design checkpoint.

Ratification recording is complete. No compiler/runtime test, benchmark,
hosted CI run, or implementation validation is claimed for this documentation
change.

## Follow-up

Ratification is complete. A later task folds the selected answers
into ADR-0052 and ADR-0058, reconciles the identified dependent contracts, and
then begins phase 1 under the existing test-first, backend-parity, coverage,
reference/tutorial, and editor gates. Phase 2 removes Option only after the
reviewable intermediate checkpoint meets the roadmap's stated criteria.
Batch 2 must revisit the app-facing optional dictionary lookup surface as
recorded under Q23; Batch 1's tagged generic lookup remains the accepted plan.
