# Aura Batch S1: 0.3 Python-surface program

## Goal and boundaries

Complete the complete source-incompatible 0.3 surface migration as one
coordinated program: S2 index-domain unification, S1 collection naming and
methods, S3 testing, S4 Python polish, and five ratification-ready design-only
ADRs. Stop at the requested checkpoint. ADR-0038 loans, the P1 performance
batch, publishing, and 0.4 implementation are outside this authorization.

Nothing from this batch is pushed without explicit user authorization.

## Preconditions

- Extension publication and the F1/F2/F3 hosted-CI stability work are complete.
- Hosted CI runs
  [30738666230](https://github.com/johnolafenwa/Aura/actions/runs/30738666230),
  [30738666191](https://github.com/johnolafenwa/Aura/actions/runs/30738666191),
  and
  [30738666169](https://github.com/johnolafenwa/Aura/actions/runs/30738666169)
  each passed Ubuntu 24.04 and macOS 15 at commit `cd93f221`.
- The batch is isolated on local branch `codex/batch-s1-python-surface`, stacked
  on the pending landing-page and CI-routing work. It has no remote branch.
- The pre-batch build tree is 10 GiB and the workstation has 88 GiB available.

## Standing batch rules

- Test first for every behavior change.
- Keep ADR, Manual, fixtures, examples, tutorials, and Python hints atomic with
  each semantic change.
- Run focused behavioral gates throughout. Reserve the full local gate for a
  completed logical migration family and the final checkpoint; do not replay
  the multi-hour gate after every small commit.
- Freeze compiler coverage floors at 96.28% lines, 97.20% functions, and
  94.62% regions. Re-ratchet once, downward-truncated, at checkpoint.
- Use inventory → migrator → atomic flip → identity gate for every rename.
- Use provisional decisions only for genuine gaps and record P1–P6 evidence.

## User amendment: no backward compatibility

Aura has no users yet, so the 0.3 surface carries no compatibility aliases,
shims, grace periods, or staged activation:

- Retired type and method spellings are immediate hard errors. Focused
  replacement diagnostics remain because they improve error quality; they do
  not make an old program compile.
- `list.remove(x)` activates immediately for every equatable element type,
  including integer lists. The prompt's one-release integer containment is
  removed. Maintained old index-removal calls migrate directly to `pop(index)`.
- Migration tooling exists to flip this repository atomically. It does not
  provide a runtime or source-compatibility mode.

## Progress

### 0.3 development identity

- Failing tests first pinned `0.3.0` across Cargo/npm workspace manifests and
  locks, the CLI `aura 0.3.0-dev (<commit>)` stamp, the language-server identity,
  the VS Code extension, and the Manual's development channel.
- The implementation now satisfies those focused contracts. The shipped
  v0.2.0-preview installer, archive names, and release workflow remain
  historical release behavior.

### Coordinated S1/S2 migration inventory

Inventory is in progress. This section will record pre-flip found counts,
per-rule rewrite counts, review queues, and post-flip identity-gate totals
before the breaking migration is applied.

## Verification

Current focused version-stamp evidence:

- `python3 -m unittest scripts/test_release_metadata.py` — 5 passed.
- `node --test docs/.vitepress/release-metadata.test.mjs` — 6 passed.
- `npm --prefix tools/vscode-aura test` — 20 passed.
- `cargo test -p aura --test cli version_flags_exit_successfully -- --exact` —
  passed.

The opening full-gate attempt stopped at the expected old identity guard that
classified “Aura 0.3” as future narration. That guard now advances to 0.4 and
its focused identity suite is green. Per the user's updated gate policy, the
full gate will next run when the coordinated S1/S2 migration family is ready.

## Follow-up

Proceed in the ratified order S2 → S1 → S3 → S4 → design-only ADRs. At the
checkpoint, report migration counts, containment evidence, zero-cast backend
parity, V6 numbers, assertion introspection, S4 evidence, provisional
decisions, final coverage, and three hosted run links, then stop.
