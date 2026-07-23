# April 23 Audit Hardening Fix Pass

## Goal

Fix the April 23 review findings and make the currently supported Aurora API more solid and secure without changing unrelated project state.

## Work Completed

- Rejected moves of non-copy fields through `borrow self`, aligned existing fixtures/examples/tutorial snippets with explicit cloning or owned receivers, and added check-fail coverage for the regression.
- Validated `main` return types so only unit/implicit `None` and `int32` are accepted, with diagnostics for unsupported return surfaces.
- Ensured MIR `with` cleanups run when instructions or terminators raise runtime errors.
- Prevented duplicate supervisor names from spawning unmanaged children, including a Unix process-group regression.
- Stopped editor-style `analyze --stdin` and `complete --stdin` from writing package lockfiles.
- Preserved annotated empty `Vec`, `Set`, and `Map` element types during MIR lowering.
- Rejected hyphenated manifest package names to match the import grammar.
- Hardened bounded runtime reads against oversized allocations and deadline bypass, and added a timeout wrapper for package git commands that drains captured output while waiting.
- Updated the LSP fallback surface, compiler bridge, VS Code syntax checks, maintained examples, tutorials, and package lock state touched by the changed behavior.

## Verification

- `cargo fmt --all`
- `CARGO_TARGET_DIR=/tmp/aurora-fix-target cargo test -p aurora-compiler --test fixtures -- --nocapture`
- `CARGO_TARGET_DIR=/tmp/aurora-fix-target cargo test -p aurora-compiler --test coverage_runtime_errors -- --nocapture`
- `CARGO_TARGET_DIR=/tmp/aurora-fix-target cargo test -p aurora-compiler --test coverage_surface -- --nocapture`
- `CARGO_TARGET_DIR=/tmp/aurora-fix-target cargo test -p aurora-compiler --test modules -- --nocapture`
- `CARGO_TARGET_DIR=/tmp/aurora-fix-target cargo test -p aurora-compiler --test packages -- --nocapture`
- `CARGO_TARGET_DIR=/tmp/aurora-fix-target cargo test -p aurora-compiler --test process -- --nocapture`
- Focused runtime hardening unit tests for bounded reads, fd deadlines, and package git timeout behavior.
- `CARGO_TARGET_DIR=/tmp/aurora-fix-target cargo test -p aura -- --nocapture`
- `npm --prefix tools/vscode-aurora test`
- `npm --prefix tools/aurora-language-server test`
- `npm --prefix tools/vscode-aurora run check`
- `npm --prefix tools/aurora-language-server run check`
- `git diff --check`
- `CARGO_TARGET_DIR=/tmp/aurora-fix-target cargo clippy -p aurora-compiler -p aura -- -D clippy::correctness`

## Follow-up

- Existing non-correctness clippy style/dead-code warnings remain outside this pass.
