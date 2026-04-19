## Goal

Add maintained process-group support to Aurora's shell-free `process` module so a child can own and be controlled as a whole process tree rather than only as a single process handle.

This pass should cover the compiler, MIR runtime, direct runtime/backend, CLI-facing behavior, maintained examples/tutorials, and fallback editor metadata.

## Session

- Start: 2026-04-19 21:17:15 BST

## Work Completed

- Added maintained `group=true` support to `process.start(...)` and `process.run(...)` in the builtin module surface.
- Implemented Unix child-process-group creation in the shared runtime `ProcessChildValue::spawn(...)` path and stored per-child group identifiers for later lifecycle control.
- Made `process.Child.kill()`, `process.Child.terminate()`, and `process.Child.close()` group-aware when a child was started with `group=true`, while preserving the existing single-process behavior for ordinary children.
- Tightened grouped `close()` semantics so it waits for the full child process group to disappear instead of only waiting for the leader process to exit.
- Added an end-to-end compiler regression proving that grouped child cleanup tears down descendant processes, not only the direct child handle.
- Updated direct runtime and MIR runtime process launch paths, direct native codegen signatures, fallback LSP metadata, CLI integration coverage, maintained process examples, and process tutorials/README references to include the grouped launch surface.

## Verification

- `cargo fmt --all`
- `cargo test -p aurora-compiler --test process`
- `cargo test -p aura direct_backend_build_supports_process_module_surface -- --nocapture`
- `npm run test:lsp`
- `cargo test -p aurora-compiler`
- `cargo test -p aura`
- `npm run check:extension`
- `cargo clippy -p aurora-compiler -p aura -- -D clippy::correctness`

## Follow-up

- Grouped children are currently supported on Unix hosts. PTY support, restart/supervisor policies, and broader process orchestration remain separate future work.
