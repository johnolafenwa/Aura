# Batch 1 design checkpoint — 2026-09-08

## Goal and authorized scope

Produce the detailed callable/type-foundation proposal requested by the user,
based on `de3d6cc44a376502f2aaf3f3ef8faba59c523884`, and commit it once on
main. The proposal remains **Proposed; awaiting user ratification**. This
task does not implement it or amend accepted ADR bodies.

Authorized files are the new design document, the roadmap link, architecture
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
  host Option from Aura's proposed retired builtin.
- Preserved the stored-loan deferral, recorded the cost of remove/call/reinsert
  for a non-cloneable callable registry, and listed ADR tensions explicitly.
- Linked the proposal from the Batch 1 row and both requested indexes.

## Verification

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

The documentation target is complete and awaits user ratification. No
compiler/runtime test, benchmark, hosted CI run, or implementation validation
is claimed for this documentation-only change.

## Follow-up

The user ratifies the questionnaire. A later task folds the selected answers
into ADR-0052 and ADR-0058, reconciles the identified dependent contracts, and
then begins phase 1 under the existing test-first, backend-parity, coverage,
reference/tutorial, and editor gates. Phase 2 removes Option only after the
reviewable intermediate checkpoint meets the roadmap's stated criteria.
