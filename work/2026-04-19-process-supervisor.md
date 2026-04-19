## Goal

Add a maintained process supervisor layer to Aurora's `process` module so Aurora code can keep named child processes alive with restart policies and backoff, without hand-writing restart loops.

This pass should cover the compiler, MIR runtime, direct runtime/backend, CLI-facing behavior, maintained examples/tutorials, and fallback editor metadata.

## Session

- Start: 2026-04-19 21:38:31 BST
- Stop: 2026-04-19 22:25:00 BST
- Elapsed: 0h 46m

## Work Completed

- Added the maintained `process.supervisor()` API and new builtin process types:
  - `process.Supervisor`
  - `process.RestartPolicy`
  - `process.SupervisorEvent`
  - `process.SupervisorWait`
- Implemented supervised named child processes in the shared runtime with:
  - restart policies `Never`, `OnFailure`, and `Always`
  - configurable restart backoff
  - configurable max restart counts
  - group-aware child startup and shutdown
  - `wait(...)`, `wait_or_none(...)`, `stop()`, `is_empty()`, and `close()`
- Wired the same surface through:
  - semantic checking
  - MIR runtime execution
  - direct native runtime execution
  - native direct codegen
  - compiler-owned analysis/completions
  - fallback LSP metadata and return-type specialization
- Added maintained regression coverage for:
  - type-checking the full supervisor surface
  - restarting a failing child once and surfacing `Restarted` then `Exited`
  - group-aware supervised stop behavior
  - direct-backend CLI support for the supervisor API
- Added the maintained runnable example:
  - `examples/io/process_supervisor.au`
- Updated user-facing docs:
  - `README.md`
  - `crates/aura/README.md`
  - `examples/README.md`
  - `tutorials/19-io-and-networking.md`
  - `tutorials/14-current-language-surface.md`

## Verification

- `cargo fmt --all`
- `cargo test -p aurora-compiler --test process`
- `cargo test -p aura direct_backend_build_supports_process_module_surface -- --nocapture`
- `npm run test:lsp`
- `cargo test -p aurora-compiler`
- `cargo test -p aura`
- `npm run check:extension`
- `cargo clippy -p aurora-compiler -p aura -- -D clippy::correctness`

Note:
- A concurrent `cargo test -p aurora-compiler` / `cargo test -p aura` verification attempt hit a rustdoc artifact-path race in doctests. The final verification reran `cargo test -p aurora-compiler` sequentially on the settled build tree and passed cleanly.

## Follow-up

- PTY support and a higher-level restart/supervisor policy layer remain separate follow-on work.
