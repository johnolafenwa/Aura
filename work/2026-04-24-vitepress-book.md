# April 24 VitePress Book Pass

## Goal

Create a maintained VitePress documentation book for Aurora with a use-case-driven Learn track and a comprehensive manual/reference track for the currently supported language and API surface.

## Work Completed

- Added a VitePress site under `docs/` with custom navigation, sidebars, local search, light/dark theme polish, and a small Aurora mark asset.
- Added a Learn track that teaches Aurora through practical programs and case studies: first scripts, data modeling, collections, ownership and borrowing, results/options, packages, concurrency, I/O, processes, networking, native builds, log analysis, queue worker pools, and process supervision.
- Added a Manual track that documents the current language and runtime API surface in a reference style: lexical structure, types, expressions, statements, functions, classes, enums and match, generics and traits, ownership, collections, concurrency, I/O, filesystem, networking, process APIs, packages, CLI/tooling, current limits, and an API index.
- Added root npm docs scripts and README guidance for building and serving the book.
- Kept legacy proposal/support Markdown out of the VitePress page graph so the book can build cleanly without rewriting historical planning documents.
- Pinned VitePress to `2.0.0-alpha.17` because the current stable VitePress line pulls a Vite/esbuild development-server audit advisory; the alpha line builds successfully in this repo and keeps npm audit clean.

## Verification

- `npm run docs:build`
- `npm audit --audit-level=moderate`
- `git diff --check`
- Local preview smoke test: `curl -I --max-time 5 http://127.0.0.1:5173/` returned `HTTP/1.1 200 OK`.

## Follow-up

- As the language surface changes, keep the Learn examples and Manual API pages synchronized with compiler, LSP, examples, and tutorial updates.
- Consider moving older proposal/planning Markdown into an explicit archive section if those documents should become part of the rendered book later.
