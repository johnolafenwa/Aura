# Aura Compiled-Language Positioning

## Goal

Present Aura consistently as a compiled, statically typed programming language
across maintained user-facing documentation. Record the general-purpose systems
language scope as a long-term goal that includes applications, services,
databases, language runtimes, embedded software, operating systems, and device
drivers.

The user approved the exact copy before implementation and authorized a direct
push to `main`.

## Work Completed

- Replaced present-tense systems-language positioning in the root README,
  VitePress homepage, site metadata, Why Aura page, tutorial overview, and VS
  Code Marketplace copy.
- Reworked the homepage hero and feature summaries to lead with native
  compilation, static types, and ownership-based reliability.
- Added a dedicated Long-Term Direction section that clearly separates the
  current Aura 0.3 application focus from the intended future systems scope.
- Reframed the ML systems roadmap as the ML Infrastructure Support Plan and
  removed unnecessary defensive comparison language from its introduction,
  principles, priorities, and summary.
- Updated the extension description and keywords to use `compiled language`
  in place of `systems programming`.
- Updated the `llms.txt` generator so AI agents receive the same current and
  long-term positioning as human readers.
- Added focused regression tests for landing-page copy, site metadata,
  extension listing metadata, and generated AI-agent summary text.

## Verification

- test-first red state: the new landing, extension-listing, and LLM-summary
  expectations all rejected the previous positioning
- focused landing-page positioning tests: green
- focused VS Code package/listing tests: green
- LLM generator unit tests: green
- complete VitePress component and positioning suite: 19 green
- repository identity suite: 15 green
- generated `llms.txt` and `llms-full.txt` freshness check: green
- production VitePress build: green
- Manual reference integrity: 39 pages, 271 fenced blocks, 132 verified, green
- executable tutorial gate: 336 total, green
- complete VS Code extension suite: 24 green
- VSIX packaging: green; the packaged manifest and README carry the new copy

## Follow-Up

The website and GitHub README update from `main`. The Visual Studio Marketplace
and Open VSX listing metadata will receive the new package description and
README with the next extension version; marketplace versions are immutable.

No full compiler/runtime gate was run because the change affects documentation,
generated documentation, positioning tests, and extension listing metadata.
The focused documentation, reference, tutorial, identity, and packaging gates
cover the changed surfaces.

The existing `personal/file_ops.au` modification and untracked ADR-0022 draft
are user work and remain unstaged and unchanged.
