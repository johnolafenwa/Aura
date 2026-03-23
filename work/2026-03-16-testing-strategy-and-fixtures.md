# 2026-03-16 Testing Strategy And Fixture Harness

## Goal

Move Aurora toward a stricter test-first workflow for both the language implementation and the language server.

## Work Completed

- Added a repo-level `AGENTS.md` that makes test-first development the default workflow.
- Added `docs/testing_strategy.md` describing:
  - test-first feature work
  - mandatory regression tests
  - layered compiler/LSP/CLI test structure
  - coverage policy direction
- Added `crates/aurora-compiler/tests/fixtures.rs` as a fixture harness for:
  - parse-pass
  - check-pass
  - check-fail
  - run-pass
- Seeded the fixture tree with initial passing and failing Aurora programs.
- Added `crates/aurora-compiler/README.md` documenting the compiler test structure.
- Linked the testing strategy and compiler testing notes from the root README.

## Verification

- `cargo test`
- `npm run test:lsp`
- `npm run coverage:lsp`

## Notes

The repo is not at literal enforced 100% coverage across all packages yet. The immediate improvement is that new compiler work can now begin with filesystem fixtures instead of ad hoc assertions, and the LSP has a repeatable coverage command with visible numbers.
