# 2026-03-31 Vec Insert Analysis Detail

## Goal

Bring compiler-backed analysis and LSP completion metadata for `Vec.insert(...)` back into alignment with the checker and runtime surface.

## Work Completed

- Added a Rust analysis regression asserting that compiler-owned member completion reports `insert(index: int32, value: T) -> bool`.
- Added a compiler-bridge regression asserting that the LSP-facing completion detail for `Vec.insert(...)` also reports `-> bool`.
- Fixed the Rust analysis layer so `Vec.insert(...)` now infers `bool` instead of `None` and exposes the matching completion detail.

## Verification

- `cargo fmt --all`
- `cargo test -p aurora-compiler`
- `npm run test:lsp`
- `npm run check:extension`

## Follow-up

- Keep using compiler-bridge tests to catch future drift between checker/runtime semantics and compiler-backed editor metadata.
