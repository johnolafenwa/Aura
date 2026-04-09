# 2026-03-30 Parser Index Chains And F-String Indexing

## Goal

Fix the parser regression where ordinary indexed expressions like `keys[idx].clone()` were misread as explicit generic specialization, and verify the maintained f-string indexing surface with regression coverage.

## Work Completed

- Added run-pass regression fixtures for:
  - indexed member-call chains after variable indexing
  - f-string interpolations containing indexed map lookups
- Fixed postfix parsing so explicit specialization only wins in the syntactic cases that are actually intended, preserving generic calls like `identity[T](...)` while allowing ordinary chains like `keys[idx].clone()`.
- Added CLI direct-build product coverage for indexed member chains plus f-string indexing.
- Added compiler-backed LSP bridge and JS fallback regression tests so the same syntax stays accepted in editor analysis paths.
- Updated the tutorial/reference track and task board to document indexed-expression chaining and indexed f-string interpolations as part of the maintained surface.

## Verification

- `cargo test -p aurora-compiler --test fixtures run_pass_fixtures_match_expected_stdout -- --nocapture`
- `cargo test -p aurora-compiler --test fixtures run_pass_fixtures_match_expected_stdout_via_mir -- --nocapture`
- `cargo test -p aura build_with_direct_backend_supports_indexed_member_chains_and_fstring_indexing -- --nocapture`
- `npm run test:lsp`

## Follow-Up

- None beyond keeping the generic-specialization parser precedence covered as the expression surface expands.
