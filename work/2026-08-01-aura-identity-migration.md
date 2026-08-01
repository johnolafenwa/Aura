# Aura identity migration

## Goal

Ship the first public `v0.2.0-preview` tree under the single product identity
**Aura**, with no semantic language change. The CLI remains `aura` and source
files remain `.au`.

The maintained README, Manual, Learn track, tutorials, examples, tooling, and
release material must describe the current language directly. Removed-feature
backstory and speculative-version commentary are not part of that surface.

## Protected and historical zones

- `personal/file_ops.au` is user-owned and must remain untouched.
- The untracked `architecture_docs/decisions/0022-implicit-shared-capability-syntax.md`
  draft is user-owned and must remain untouched.
- Existing ADR bodies, `work/` history, `CHANGELOG.md`, and the historical
  language proposal retain truthful period language.
- ADR-0042 and the ADR index are the explicit bridge between the former working
  name and the release identity.

## Pre-flip inventory

Baseline: tracked tree at `e9b02a457a9b` on 2026-08-01. Counts exclude
`work/**`, existing ADR bodies, the historical proposal, and the protected
personal source. Categories intentionally overlap: a runtime symbol is also a
source identifier, and a diagnostic token is also prose.

| Category | Found | Migrated | Remaining |
| --- | ---: | ---: | ---: |
| Identity-style source identifiers | 6,174 | 6,174 | 0 |
| `aurora_direct_*` runtime symbols | 2,881 | 2,881 | 0 |
| `AURORA_*` environment-variable tokens | 329 | 329 | 0 |
| `Aurora.toml` / `Aurora.lock` references | 194 | 194 | 0 |
| Tracked old-name manifest paths | 10 | 10 | 0 |
| Branded diagnostic-oracle tokens | 711 | 711 | 0 |
| Branded maintained-documentation tokens | 1,568 | 1,568 | 0 |
| Old GitHub repository URLs | 3 | 3 | 0 |
| Identity-bearing tracked paths | 1,746 | 1,746 | 0 |
| Prior/future-feature prose candidates | 52 | 52 reviewed | 0 stale |

The identity-bearing path count includes 1,713 files below the compiler crate,
11 below the language-server package, and 11 below the VS Code extension. It
also includes 10 package manifests and `docs/public/aurora-mark.svg`.

The prose candidate count is a review queue, not a blind replacement count.
Ordinary current semantics such as “previously unseen binding” and collection
operations that remove a value are not feature-history narratives.

Post-flip reconciliation is enforced by `scripts/test_aura_identity.py`. It
requires zero former-identity tokens and paths outside the explicit history
zones, verifies every new identity surface, and rejects removed-feature
narrative in all maintained public Markdown. The 52 prose candidates were
reviewed individually; current semantic uses were retained and every stale
language-history passage was rewritten. A narrower return-source, return-label,
lifetime-label, and loan/view scan found and removed seven additional public
references that were outside the original candidate expression.

## Verification target

- identity/content regression tests reject old identity tokens and stale public
  narratives outside the explicit historical zones;
- compiler, CLI, package loader, LSP, extension, docs, release tooling, forced
  backend parity, reference integrity, audits, coverage, Clippy, and hygiene all
  pass through `npm run ci`;
- the local preview tag and rebuilt release artifacts identify the final commit;
- nothing is pushed or published.

## Work completed

- Renamed the compiler crate and library, direct-runtime ABI, native cache
  identity and location, environment-variable contract, package manifests,
  diagnostics, language server, VS Code extension, grammar scope, site assets,
  release archives, package metadata, and repository URLs to Aura.
- Migrated every maintained package fixture, example, tutorial, Manual page,
  Learn page, generated diagnostic oracle, and reference fence contract in the
  same tree as the implementation.
- Added regressions proving that former environment-variable and manifest names
  are not accepted, the new cache identity invalidates old artifacts, the LSP
  reports the Aura identity, release metadata uses Aura artifacts, and the
  maintained tree contains one product identity.
- Rewrote maintained public documentation to state current behavior directly,
  without removed return-source/label or loan/view backstory. Existing ADR
  bodies, work history, CHANGELOG history, and the proposal remain unchanged as
  historical records.
- Added ADR-0042 and the ADR-index bridge for the working-name transition. The
  current CHANGELOG entry introduces Aura and records that it was developed
  under the former working name.

The exact full `npm run ci` sign-off replay is green. Evidence includes:

- 336/336 CLI/runtime tests and 1,499/1,499 compiler-library tests;
- the complete forced MIR/direct fixture matrix, green in 1,102.95 seconds;
- 101/101 LSP tests and 19/19 VS Code extension tests;
- compiler coverage of 96.28% lines, 97.21% functions, and 94.62% regions;
- LSP coverage of 100% statements, branches, functions, and lines;
- reference integrity over 37 pages, 260 fences, and 126 verified blocks;
- all 683 package manifests migrated;
- docs build, npm audit, Cargo audit under the checked-in warning policy,
  warning-denied Clippy, formatting, and hygiene.

Coverage-only artifacts were cleaned after recording the report because the
instrumented build pushed `target/` past the repository's 20 GiB hygiene
threshold.

## Follow-up

Commit the atomic migration, move the local preview tag, rebuild the release
artifacts, and record their verified checksums. Nothing is pushed or published.
