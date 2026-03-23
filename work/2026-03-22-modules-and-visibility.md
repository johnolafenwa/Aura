# 2026-03-22 Modules And Visibility

## Goal

Implement the proposal-aligned bootstrap module system and make `public` meaningful at module boundaries.

## Work Completed

- Added path-aware compiler entry points with recursive local-file module loading.
- Implemented `import a.b` and `from a.b import Name` for local `.au` files.
- Added `public` on top-level classes, enums, functions, and traits.
- Added `public` on class methods.
- Enforced module-boundary visibility for:
  - top-level imports
  - field access
  - method calls
  - keyword construction through public participating fields
- Added module namespace support in both the interpreter path and the MIR runtime for dotted function calls like `helpers.math.double(...)`.
- Switched file-backed CLI commands onto the path-aware compiler path:
  - `check`
  - `run`
  - `run-mir`
  - `build`
  - `mir`
- Added compiler integration tests for:
  - `from ... import ...`
  - dotted module calls
  - private top-level import rejection
  - private method rejection across modules
- Added CLI product tests for running and building programs with local modules.
- Added maintained examples and tutorial coverage for modules and visibility.

## Verification

- `cargo test`

## Follow-up

- Extend compiler-backed `analyze` / `complete` and the LSP to resolve imported modules and exported names for editor features.
- Decide how far to push module-qualified type references versus keeping `from ... import Type` as the canonical typed path.
- Continue the remaining backend and coverage-enforcement work after this surface addition.
