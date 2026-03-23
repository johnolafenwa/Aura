# 2026-03-22 CI And Coverage Gates

## Goal

Turn the repo from “coverage is measurable” into “coverage regressions and cross-package breakage fail loudly” with one local command and one CI workflow.

## Work Completed

- Added enforced compiler coverage gates through `npm run coverage:compiler:check`.
- Added enforced language-server coverage gates through `npm run coverage:lsp:check`.
- Added `npm run ci` as the current repo-level verification gate:
  - `cargo test`
  - `npm run test:lsp`
  - `npm run check:extension`
  - `npm run test:extension`
  - `npm run coverage:compiler:check`
  - `npm run coverage:lsp:check`
- Added `.github/workflows/ci.yml` to run the same gate on:
  - macOS
  - Linux
- Updated the testing strategy and repo README content to document the enforced floors and local workflow.

## Verification

- `npm run coverage:compiler`
- `npm run coverage:lsp`
- `npm run coverage:compiler:check`
- `npm run coverage:lsp:check`
- `npm run ci`

## Follow-Up

- Keep ratcheting the enforced coverage floors upward toward 100% as the implementation stabilizes.
- Add native-codegen artifact checks into the CI gate once the standalone backend moves beyond the current bootstrap launcher path.
