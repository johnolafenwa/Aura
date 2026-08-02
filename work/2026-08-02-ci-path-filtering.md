# Hosted CI path filtering

## Goal

Keep the complete Ubuntu/macOS repository gate for changes that can affect
Aura's code, packages, tooling, tests, and release behavior. Route prose and
site-only changes through the fast documentation workflow.

## Work completed

- Limited full-CI push events to `main`, removing the duplicate run previously
  created by both a feature-branch push and its pull request.
- Added identical full-CI exclusions for Markdown, the VitePress tree, work
  notes, and passive repository metadata on push and pull-request events.
- Expanded the Docs workflow to maintained Markdown surfaces. Internal work
  notes, GitHub templates, and AGENTS.md remain outside the site gate.
- Added an inventory-only Manual reference check to the Docs workflow. It
  validates fenced-block hashes and classification, page roles, required
  normative sections, and executable-example coverage without building or
  invoking the Aura CLI.
- Added regressions that pin both workflow trigger contracts and the
  lightweight Manual check.

## Resulting policy

| Change | Docs workflow | Full CI matrix |
| --- | --- | --- |
| README or maintained Markdown | Yes | No |
| `docs/**` site content or assets | Yes | No |
| `work/**` or passive repository metadata | No | No |
| Compiler, runtime, CLI, LSP, extension, examples, packages, or scripts | As matched | Yes |
| Workflow or dependency configuration | As matched | Yes |

## Verification

- `python3 -m unittest scripts/test_release_packaging.py` — 34 passed.
- `python3 scripts/reference_integrity.py --inventory-only` — 38 pages and 261
  fences validated.
- `npm run docs:build` — passed.
- `npx --yes github-actionlint` — passed.
- Scoped diff hygiene — passed. The protected user file
  `personal/file_ops.au` retains its pre-existing whitespace and remains
  outside this change.

## Follow-up

No follow-up is required. If branch protection is enabled later, its required
checks should use the Docs workflow for documentation-only pull requests and
the CI workflow for code-bearing pull requests.
