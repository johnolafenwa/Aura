# 2026-03-24 Native MIR Build

## Goal

Replace `aura build`'s generated Rust launcher path with a native MIR artifact build that produces standalone host binaries for macOS and Linux.

## Work Completed

- Added a compiled Aurora runtime entrypoint in `aurora-compiler` for executing serialized MIR directly.
- Added `staticlib` output for `aurora-compiler` and linked `aura build` against that runtime library instead of generating a temporary Rust runner.
- Switched `aura build` to lower to MIR at build time, embed serialized MIR plus source context in a native launcher, and compile that launcher with the host C toolchain.
- Made stdin-backed builds module-aware by lowering through `lower_path_with_source_to_mir(...)`.
- Added product coverage for:
  - simple native binaries
  - concurrency binaries
  - local-module binaries
  - stdin-backed module builds
  - binaries that still run after the original source file is removed
  - binaries that exit cleanly when stdout closes early
- Updated maintained READMEs, tutorials, and the task board to reflect the new build/runtime model.

## Verification

- `cargo test -p aura --test cli build_produces_a_runnable_binary -- --exact`
- `cargo test -p aura --test cli build_produces_runnable_concurrency_binary -- --exact`
- `cargo test -p aura --test cli build_produces_runnable_binary_for_program_with_local_modules -- --exact`
- `cargo test -p aura --test cli build_from_stdin_produces_runnable_module_binary -- --exact`
- `cargo test -p aura --test cli built_binary_runs_after_source_file_is_removed -- --exact`
- `cargo test -p aura --test cli built_binary_exits_cleanly_when_stdout_pipe_closes -- --exact`
- `cargo run -p aura -- build -o ./target/aurora-point examples/point.au`
- `./target/aurora-point`
- `npm run ci`

## Follow-up

- Direct low-level MIR-to-machine-code lowering remains a future optimization beyond the current runtime-linked native artifact path.
