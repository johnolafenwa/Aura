## Goal

Close the fourth-pass externally reviewed correctness, ownership, runtime, LSP, and documentation defects end to end, with failing regressions first and full verification at the end.

## Session

- Start: 2026-04-20 20:31:12 BST
- Stop: 2026-04-20 22:19:08 BST
- Elapsed: 1h 48m
- Stop rule: Complete the work or reach 12 continuous hours.

## Work Completed

- Added failing-first compiler fixtures for indirect recursive enum construction, `match borrow mut` reassignment and nested aliasing, managed `with` resource field moves, nested missing-pattern diagnostics, `Option.Some(...)` / `Option.None` inference, expression-form `match` positions, unreachable enum arms, and supertrait syntax plus inherited obligations.
- Fixed the compiler, MIR lowering/runtime, and direct backend so recursive indirect enums no longer overflow the host stack, nested trait-bound direct binaries no longer segfault, `match borrow mut` no longer clobbers explicit scrutinee writes, nested mutable matches on the same place are rejected, managed `with` resources reject moving a non-copy field that cleanup still needs, and `aura run` now enforces the same 1 MiB `fs.read_to_string(...)` / `fs.read_bytes(...)` cap as direct-built binaries.
- Implemented supertrait parsing and checking, including inherited method visibility through bounds and explicit impl-time supertrait enforcement; added unreachable-pattern diagnostics for covered enum arms; and completed expression-form `match` support in binding, argument, and nested block positions.
- Fixed runtime networking behavior by capping TLS handshakes to the maintained deadline budget, keeping the handshake checks cancellation-aware, returning `413 Payload Too Large` for oversized HTTP requests, and preserving underlying `io::ErrorKind` values in websocket send/recv/close paths.
- Fixed the compiler-backed language server cache invalidation so imported-file edits refresh open dependents, shared `file://` URI parsing between bridge/server, and preserved Windows UNC workspace paths.
- Updated the maintained architecture docs, tutorials, and examples to match the removed `select` / unstructured `spawn` surface, document supertraits, expression-form `match` positions, `Option.Some(...)` inference, and the unified one-shot filesystem read cap; added runnable examples for match-expression positions and supertraits.

## Verification

- `cargo fmt --all`
- `cargo test -p aurora-compiler --lib -- --nocapture`
- `cargo test -p aurora-compiler --test fixtures -- --nocapture`
- `cargo test -p aura --test cli -- --nocapture`
- `cd tools/aurora-language-server && npm test`
- `cd tools/aurora-language-server && npm run check`
- `./target/debug/aura run examples/enums/match_expression_positions.au`
- `./target/debug/aura run examples/traits/supertraits.au`

## Follow-up

- No additional follow-up is required for this fix pass beyond the standing broader product and coverage work already tracked in `work/task-board.md`.
